//! AS-222 — the Flare's visual (`rendering/effects/flare.rs`), driven from a
//! real cast: the Hunter's own decider lights the Flare, `flare_system` burns
//! it down, and the client's visual systems draw it, in production order.
//!
//! Geometry and colour over time, not bookkeeping: the probes read WORLD
//! positions off `GlobalTransform` and the meshes' own vertices (where the pool
//! and rim actually lie, how big the burning flare actually is), and the
//! materials the renderer would draw (how bright the pool is, what colour the
//! flare is) — sampled every tick across the whole burn.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window, no
//! GPU — with a real `Camera3d` so the sprites' facing is the client's.

use std::collections::BTreeMap;
use std::time::Duration;

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::hunter::decide_hunter_action;
use arenasim::states::play_match::class_ai::hunter_dip::HunterDipPlan;
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo, QueuedInstantAttack};
use arenasim::states::play_match::constants::{FLARE_DURATION, FLARE_RADIUS};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::traps::flare_system;
use arenasim::states::play_match::{
    animate_client_particles, spawn_flare_visuals, update_flare_visuals, Aura, ClientParticle,
    Combatant, FlarePart, FlareRig, FlareZone, GameRng, MovementConfig, FLARE_CORE_DIAMETER,
    FLARE_CORE_HEIGHT, FLARE_CORE_SPRITES, FLARE_POOL_GAIN, FLARE_TWINKLE_MAX, FLARE_TWINKLE_MIN,
};

/// Additive pool alpha (material alpha times texture alpha) at or above this
/// reads as lit floor at the match camera; below it the floor looks dark.
const VISIBLE_ALPHA: f32 = 0.08;
/// The rim's alpha never drops below this while the Flare burns.
const RIM_ALPHA_FLOOR: f32 = 0.5;
/// ...and is at least this many times the pool's own alpha where the rim
/// lies, so it reads as a line against the lit floor, not as more floor.
const RIM_CONTRAST: f32 = 2.0;
/// The rim is at least this wide, yards: a band, not a hairline.
const RIM_MIN_WIDTH: f32 = 0.3;
/// The pool's light must reach at least this far past the reveal radius,
/// yards, through the steady burn: an edge outside it, not a texel outside.
const LIT_MARGIN: f32 = 0.3;
/// Bearings the pool is measured along.
const BEARINGS: usize = 16;

const TICK: Duration = Duration::from_millis(50);
const DT: f32 = 0.05;

fn info(entity: Entity, team: u8, class: CharacterClass, position: Vec3) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0,
        max_health: 100.0,
        current_mana: 100.0,
        max_mana: 100.0,
        position,
        velocity: Vec3::ZERO,
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        pet_heeled: false,
        casting_ability: None,
        pet_type: None,
        pet: None,
        book: arenasim::states::play_match::ability_book::AbilityBook::shipped_for(class, None),
    }
}

struct Harness {
    app: App,
    camera: Entity,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.insert_resource(CombatLog::default());
        // The sim's burn, then the client's draw, as `states/mod.rs` orders
        // them (the visuals run after `CombatResolution`).
        app.add_systems(
            Update,
            (
                flare_system,
                spawn_flare_visuals,
                update_flare_visuals,
                animate_client_particles,
            )
                .chain(),
        );
        // The match camera's angle: 60 yd out, 38.7 degrees off vertical.
        let pitch = 38.7f32.to_radians();
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_xyz(0.0, 60.0 * pitch.cos(), 60.0 * pitch.sin())
                    .looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id();
        Harness { app, camera }
    }

    /// The Hunter's decider, on the frame its Flare comes due: a Hunter at
    /// (-30, 0, 0) on team 1 guarding itself against a stealthed Rogue (the
    /// `hunter_flare.rs` setup). Returns the lit Flare.
    fn cast_flare(&mut self) -> Entity {
        let world = self.app.world_mut();
        let hunter = world.spawn_empty().id();
        let priest = world.spawn_empty().id();
        let rogue = world.spawn_empty().id();

        let movement = MovementConfig::default();
        let mut combat_log = CombatLog::default();
        let mut game_rng = GameRng::from_seed(7);
        let mut trace = DecisionTrace {
            current_sim_time: 50.0 / 6.0 + 0.02,
            ..Default::default()
        };

        let my_pos = Vec3::new(-30.0, 0.0, 0.0);
        let mut combatant = Combatant::new(1, 0, CharacterClass::Hunter);
        combatant.current_mana = combatant.max_mana;
        combatant.target = Some(priest);

        let mut roster = BTreeMap::new();
        let mut me = info(hunter, 1, CharacterClass::Hunter, my_pos);
        me.target = Some(priest);
        roster.insert(hunter, me);
        roster.insert(priest, info(priest, 2, CharacterClass::Priest, Vec3::ZERO));
        let mut hidden = info(rogue, 2, CharacterClass::Rogue, Vec3::new(20.0, 0.0, 0.0));
        hidden.stealthed = true;
        roster.insert(rogue, hidden);

        let auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
        let dr = BTreeMap::new();
        let cooldowns = BTreeMap::new();
        let ctx = CombatContext::new(
            hunter,
            1,
            &roster,
            &auras,
            &dr,
            &cooldowns,
            &[],
            Default::default(),
            Default::default(),
        );

        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, world);
        let mut instant_attacks: Vec<QueuedInstantAttack> = Vec::new();
        let book = combatant.book().clone();
        decide_hunter_action(
            &mut commands,
            &mut combat_log,
            &mut game_rng,
            &book,
            hunter,
            &mut combatant,
            my_pos,
            None,
            &ctx,
            &mut instant_attacks,
            HunterDipPlan::default(),
            &movement.hunter.weights,
            &[],
            None,
            &mut trace,
        );
        queue.apply(world);

        let flares: Vec<Entity> = world
            .query_filtered::<Entity, With<FlareZone>>()
            .iter(world)
            .collect();
        assert_eq!(flares.len(), 1, "the Hunter's decider lit no Flare");
        flares[0]
    }

    fn tick(&mut self) {
        self.app.update();
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    fn rig(&mut self) -> Option<(Entity, Vec3)> {
        let world = self.app.world_mut();
        let mut q = world.query::<(Entity, &FlareRig, &GlobalTransform)>();
        let rigs: Vec<_> = q
            .iter(world)
            .map(|(e, _, t)| (e, t.translation()))
            .collect();
        assert!(rigs.len() <= 1, "one Flare, one rig: {}", rigs.len());
        rigs.first().copied()
    }

    fn part(&mut self, which: FlarePart) -> Entity {
        let world = self.app.world_mut();
        let mut q = world.query::<(Entity, &FlarePart)>();
        q.iter(world)
            .find(|(_, p)| **p == which)
            .map(|(e, _)| e)
            .unwrap_or_else(|| panic!("no {which:?}"))
    }

    /// The part's mesh vertices in world space.
    fn world_vertices(&mut self, part: Entity) -> Vec<Vec3> {
        let world = self.app.world();
        let mesh = world
            .get::<Mesh3d>(part)
            .expect("part has a mesh")
            .0
            .clone();
        let transform = *world.get::<GlobalTransform>(part).expect("placed");
        let meshes = world.resource::<Assets<Mesh>>();
        let Some(VertexAttributeValues::Float32x3(ps)) = meshes
            .get(&mesh)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        ps.iter()
            .map(|p| transform.transform_point(Vec3::from(*p)))
            .collect()
    }

    fn material(&mut self, part: Entity) -> StandardMaterial {
        let world = self.app.world();
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(part)
            .expect("part has a material")
            .0
            .clone();
        world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap()
            .clone()
    }

    /// What the pool actually draws along `bearing` (radians) from `centre`:
    /// for each `step` yards out to past the quad's edge, the additive alpha
    /// the renderer puts on the floor there — the material's alpha times its
    /// texture's alpha at that point of the pool quad (zero off the quad).
    fn pool_alpha_along(&mut self, centre: Vec3, bearing: f32, step: f32) -> Vec<(f32, f32)> {
        let pool = self.part(FlarePart::Pool);
        let vs = self.world_vertices(pool);
        let (min_x, max_x) = vs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.x), hi.max(v.x))
        });
        let (min_z, max_z) = vs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.z), hi.max(v.z))
        });
        let m = self.material(pool);
        let tex = m.base_color_texture.clone().expect("the pool is textured");
        let world = self.app.world();
        let image = world
            .resource::<Assets<Image>>()
            .get(&tex)
            .expect("pool texture");
        let w = image.width() as usize;
        let h = image.height() as usize;
        let data = image.data.as_ref().expect("pool texels");
        let alpha = m.base_color.alpha();
        let dir = Vec3::new(bearing.cos(), 0.0, bearing.sin());
        let reach = (max_x - min_x).max(max_z - min_z);
        let mut out = Vec::new();
        let mut r = 0.0;
        while r <= reach {
            let p = centre + dir * r;
            let u = (p.x - min_x) / (max_x - min_x);
            let v = (p.z - min_z) / (max_z - min_z);
            let a = if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
                let x = (u * (w - 1) as f32).round() as usize;
                let y = (v * (h - 1) as f32).round() as usize;
                data[(y * w + x) * 4 + 3] as f32 / 255.0
            } else {
                0.0
            };
            out.push((r, alpha * a));
            r += step;
        }
        out
    }

    fn flare_left(&mut self, flare: Entity) -> Option<f32> {
        self.app
            .world()
            .get::<FlareZone>(flare)
            .map(|f| f.duration_remaining)
    }
}

/// What one tick of the burn drew.
struct Frame {
    burnt: f32,
    pool_alpha: f32,
    pool_rgb: [f32; 3],
    core_sizes: Vec<f32>,
    core_rgb: Vec<[f32; 3]>,
    /// How far the pool visibly reaches: the nearest of every bearing's
    /// farthest visibly lit point, yards.
    pool_extent: f32,
    /// The dimmest the pool draws anywhere inside the reveal radius.
    pool_min_inside: f32,
    /// The brightest the pool draws on the rim's band.
    pool_at_radius: f32,
    /// The rim's drawn alpha.
    rim_alpha: f32,
    /// How far out the rim's mesh reaches (its outer edge), yards.
    rim_outer: f32,
}

/// Light a Flare from the Hunter's decider and record every tick of its burn
/// until the rig is gone. Returns the flare's centre, the frames, and how
/// many ticks the rig stood.
fn burn() -> (Harness, Entity, Vec3, Vec<Frame>) {
    let mut h = Harness::new();
    h.tick(); // a first frame with no delta
    let flare = h.cast_flare();
    let centre = h.world().get::<Transform>(flare).unwrap().translation;
    let mut frames = Vec::new();
    for _ in 0..((FLARE_DURATION / DT) as usize + 40) {
        h.tick();
        let Some(left) = h.flare_left(flare) else {
            break;
        };
        assert!(h.rig().is_some(), "{left}s left on the Flare and no rig");
        let pool = h.part(FlarePart::Pool);
        let m = h.material(pool);
        let pool_rgb = m.base_color.to_srgba();
        let mut core_sizes = Vec::new();
        let mut core_rgb = Vec::new();
        for k in 0..FLARE_CORE_SPRITES {
            let core = h.part(FlarePart::Core(k));
            let t = *h.world().get::<GlobalTransform>(core).unwrap();
            core_sizes.push(t.compute_transform().scale.x);
            let c = h.material(core).base_color.to_srgba();
            core_rgb.push([c.red, c.green, c.blue]);
        }
        let mut pool_extent = f32::MAX;
        let mut pool_min_inside = f32::MAX;
        let mut pool_at_radius = 0.0f32;
        for b in 0..BEARINGS {
            let bearing = b as f32 / BEARINGS as f32 * std::f32::consts::TAU;
            let ray = h.pool_alpha_along(centre, bearing, 0.02);
            let edge = ray
                .iter()
                .filter(|(_, a)| *a >= VISIBLE_ALPHA)
                .map(|(r, _)| *r)
                .fold(0.0f32, f32::max);
            pool_extent = pool_extent.min(edge);
            for (r, a) in &ray {
                if *r <= FLARE_RADIUS {
                    pool_min_inside = pool_min_inside.min(*a);
                }
                if (*r - FLARE_RADIUS).abs() <= 0.25 {
                    pool_at_radius = pool_at_radius.max(*a);
                }
            }
        }
        let rim = h.part(FlarePart::Rim);
        let rim_alpha = h.material(rim).base_color.alpha();
        let rim_outer = h
            .world_vertices(rim)
            .iter()
            .map(|v| v.xz().distance(centre.xz()))
            .fold(0.0f32, f32::max);
        frames.push(Frame {
            burnt: FLARE_DURATION - left,
            pool_alpha: pool_rgb.alpha,
            pool_rgb: [pool_rgb.red, pool_rgb.green, pool_rgb.blue],
            core_sizes,
            core_rgb,
            pool_extent,
            pool_min_inside,
            pool_at_radius,
            rim_alpha,
            rim_outer,
        });
    }
    (h, flare, centre, frames)
}

/// The visual stands for the whole burn and goes the frame the Flare does:
/// a rig every tick the Flare burns, none after, and not one part left behind.
#[test]
fn the_flare_is_drawn_for_its_whole_burn_and_goes_with_it() {
    let (mut h, flare, _, frames) = burn();
    let expected = (FLARE_DURATION / DT) as usize;
    assert!(
        frames.len() + 2 >= expected && frames.len() <= expected + 1,
        "the rig stood {} ticks of a {expected}-tick burn",
        frames.len()
    );
    assert!(frames.last().unwrap().burnt > FLARE_DURATION - 2.0 * DT);
    assert!(h.world().get_entity(flare).is_err(), "the Flare burnt out");
    h.tick();
    assert!(h.rig().is_none(), "the rig outlived its Flare");
    let world = h.world();
    let parts = world.query::<&FlarePart>().iter(world).count();
    assert_eq!(parts, 0, "{parts} flare parts left behind");
}

/// The gameplay Flare carries nothing the renderer added: the rig is its own
/// entity, so the sim entity never changes table on the frame clock.
#[test]
fn the_gameplay_flare_is_never_drawn_on() {
    let mut h = Harness::new();
    h.tick();
    let flare = h.cast_flare();
    for _ in 0..10 {
        h.tick();
    }
    let world = h.world();
    assert!(world.get::<Mesh3d>(flare).is_none());
    assert!(world
        .get::<MeshMaterial3d<StandardMaterial>>(flare)
        .is_none());
    assert!(world.get::<Children>(flare).is_none());
}

/// Where it lies: the pool and rim flat on the floor, just above it, centred
/// on the Flare; the rim a band centred on the reveal radius, the pool quad
/// reaching past it; the burning flare's sprites at the flare, above the
/// floor, facing the camera.
#[test]
fn the_pool_and_rim_lie_on_the_floor_at_the_gameplay_radius() {
    let mut h = Harness::new();
    h.tick();
    let flare = h.cast_flare();
    let centre = h.world().get::<Transform>(flare).unwrap().translation;
    for _ in 0..4 {
        h.tick();
    }
    let (_, at) = h.rig().expect("a rig");
    assert!(
        at.xz().distance(centre.xz()) < 1e-4,
        "rig at {at}, Flare at {centre}"
    );

    // AS-232: the rim sits ON the reveal radius — a band straddling it,
    // centred on it — so the Rogue is revealed as its feet cross the band.
    let rim = h.part(FlarePart::Rim);
    let rim_vs = h.world_vertices(rim);
    let (inner, outer) = rim_vs
        .iter()
        .map(|v| v.xz().distance(centre.xz()))
        .fold((f32::MAX, 0.0f32), |(lo, hi), d| (lo.min(d), hi.max(d)));
    assert!(
        inner < FLARE_RADIUS && outer > FLARE_RADIUS,
        "the rim {inner}..{outer} does not straddle the {FLARE_RADIUS} yd reveal radius"
    );
    assert!(
        ((inner + outer) / 2.0 - FLARE_RADIUS).abs() < 0.02,
        "the rim {inner}..{outer} is not centred on the reveal radius"
    );
    assert!(
        outer - inner >= RIM_MIN_WIDTH && outer - inner <= 1.0,
        "the rim is {} yd wide",
        outer - inner
    );
    for v in &rim_vs {
        assert!(v.y > 0.0 && v.y < 0.1, "rim vertex off the floor: {v}");
    }

    let pool = h.part(FlarePart::Pool);
    let pool_vs = h.world_vertices(pool);
    let reach_x = pool_vs
        .iter()
        .map(|v| (v.x - centre.x).abs())
        .fold(0.0f32, f32::max);
    let reach_z = pool_vs
        .iter()
        .map(|v| (v.z - centre.z).abs())
        .fold(0.0f32, f32::max);
    assert!(
        reach_x > FLARE_RADIUS && (reach_x - reach_z).abs() < 1e-3,
        "the pool quad reaches {reach_x} x {reach_z}"
    );
    for v in &pool_vs {
        assert!(v.y > 0.0 && v.y < 0.1, "pool vertex off the floor: {v}");
    }
    // Facing up, so it is seen from above and not edge-on.
    let up = h.world().get::<GlobalTransform>(pool).unwrap().rotation() * Vec3::Y;
    assert!(up.dot(Vec3::Y) > 0.999);

    // Every piece glows additively, and none is `unlit`: Bevy's unlit branch
    // draws `base_color` alone and throws the emissive away — the reason the
    // first Flare neither glowed nor flickered.
    let mut pieces = vec![rim, pool, h.part(FlarePart::Halo)];
    pieces.extend((0..FLARE_CORE_SPRITES).map(|k| h.part(FlarePart::Core(k))));
    for piece in pieces {
        let m = h.material(piece);
        assert!(!m.unlit, "{piece} is unlit");
        assert_eq!(m.alpha_mode, AlphaMode::Add);
        assert!(m.emissive.red > 0.0, "{piece} has no glow");
    }

    let camera = h.camera;
    let camera_rotation = h.world().get::<GlobalTransform>(camera).unwrap().rotation();
    let to_camera = camera_rotation * Vec3::Z;
    for k in 0..FLARE_CORE_SPRITES {
        let core = h.part(FlarePart::Core(k));
        let t = h
            .world()
            .get::<GlobalTransform>(core)
            .unwrap()
            .compute_transform();
        assert!(t.translation.xz().distance(centre.xz()) < 0.2);
        assert!(t.translation.y > 0.2 && t.translation.y < FLARE_CORE_HEIGHT + 0.2);
        let normal = t.rotation * Vec3::Z;
        assert!(
            normal.dot(to_camera) > 0.999,
            "core {k} is not facing the camera"
        );
    }
}

/// It flickers and pulses all its life: in every second of the burn the pool's
/// brightness swings by the client's quad-A pulse, and the burning flare's
/// size jumps across the twinkle's range; and the flare cycles the client's
/// white-gold to orange ramp. Orange throughout, and gone dark at the end.
#[test]
fn the_pool_pulses_and_the_flare_flickers_for_the_whole_burn() {
    let (_, _, _, frames) = burn();
    let per_second = (1.0 / DT) as usize;
    let mut windows = 0;
    for window in frames.chunks(per_second) {
        if window.len() < per_second || window[0].burnt > FLARE_DURATION - 1.0 {
            continue;
        }
        windows += 1;
        let (lo, hi) = window.iter().fold((f32::MAX, f32::MIN), |(lo, hi), f| {
            (lo.min(f.pool_alpha), hi.max(f.pool_alpha))
        });
        assert!(
            lo > 0.0 && hi - lo >= 0.04 * FLARE_POOL_GAIN,
            "at {:.1}s the pool holds {lo:.3}..{hi:.3}",
            window[0].burnt
        );
        let sizes: Vec<f32> = window.iter().flat_map(|f| f.core_sizes.clone()).collect();
        let (smin, smax) = sizes
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
        let span = FLARE_CORE_DIAMETER * (FLARE_TWINKLE_MAX - FLARE_TWINKLE_MIN);
        assert!(
            smin > 0.0 && smax - smin >= 0.6 * span,
            "at {:.1}s the flare holds {smin:.3}..{smax:.3}",
            window[0].burnt
        );
        // Orange pool: red over green over blue.
        for f in window {
            let [r, g, b] = f.pool_rgb;
            assert!(r > g && g > b, "the pool is not orange: {r} {g} {b}");
        }
        // The flare's ramp: white-gold (blue high) and orange (blue low) both
        // appear within any second.
        let blues: Vec<f32> = window
            .iter()
            .flat_map(|f| f.core_rgb.iter().map(|c| c[2]))
            .collect();
        let (blo, bhi) = blues
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &b| (lo.min(b), hi.max(b)));
        assert!(
            bhi - blo > 0.3,
            "at {:.1}s the flare holds one colour",
            window[0].burnt
        );
    }
    assert!(windows >= 28, "only {windows} one-second windows sampled");

    // Burnt down to a glimmer by its last frame.
    let peak = frames.iter().map(|f| f.pool_alpha).fold(0.0f32, f32::max);
    let last = frames.last().unwrap();
    assert!(
        last.pool_alpha < 0.2 * peak,
        "the pool is still lit at burn-out: {} of a {peak} peak",
        last.pool_alpha
    );
}

/// The flare spits sparks, more early than late, born at its foot and rising.
#[test]
fn the_flare_spits_sparks_that_die_away() {
    let mut h = Harness::new();
    h.tick();
    let flare = h.cast_flare();
    let centre = h.world().get::<Transform>(flare).unwrap().translation;
    let mut early = 0;
    for _ in 0..20 {
        h.tick();
        let world = h.world();
        let mut q = world.query::<(&ClientParticle, &Transform)>();
        for (p, t) in q.iter(world) {
            assert!(
                t.translation.xz().distance(centre.xz()) < 2.5,
                "a spark at {} for a Flare at {centre}, age {} v {}",
                t.translation,
                p.age,
                p.velocity
            );
            if p.age < DT * 1.5 {
                // Born at the flare's foot, launched upward (gravity brings
                // the flat ones down into the floor, as the client's do).
                assert!(t.translation.y > 0.0, "a spark born under the floor");
                assert!(
                    p.velocity.y + p.gravity * p.age >= 0.0,
                    "a spark launched downward"
                );
                early += 1;
            }
        }
    }
    // Run on to the last few seconds and count again.
    for _ in 0..((24.0 / DT) as usize) {
        h.tick();
    }
    let mut late = 0;
    for _ in 0..20 {
        h.tick();
        let world = h.world();
        let mut q = world.query::<&ClientParticle>();
        late += q.iter(world).filter(|p| p.age < DT * 1.5).count();
    }
    assert!(early >= 20, "only {early} sparks in the first second");
    assert!(
        late < early / 2,
        "the sparks do not die away: {early} early, {late} late"
    );
}

/// AS-232: a Rogue is never revealed while it visibly stands outside the
/// light. Measured off what the pool DRAWS — its quad's world extent, its
/// texture's alpha and its material's alpha, every tick, on every bearing:
/// through the steady burn the lit floor is unbroken inside the reveal radius
/// and visibly reaches past it; to the last frame, the lit area (the pool, or
/// the rim on the radius) reaches the reveal radius.
#[test]
fn the_lit_area_reaches_the_reveal_radius_for_the_whole_burn() {
    let (_, _, _, frames) = burn();
    // The client's own last half second (clock 29.5-30) puts the pool out.
    let steady_end = FLARE_DURATION * (29.5 / 30.0);
    let mut steady = 0;
    let mut tightest = f32::MAX;
    for f in &frames {
        let rim_lit = f.rim_alpha >= VISIBLE_ALPHA;
        let lit_extent = f.pool_extent.max(if rim_lit { f.rim_outer } else { 0.0 });
        assert!(
            lit_extent >= FLARE_RADIUS,
            "at {:.2}s the lit area reaches {lit_extent:.2} yd, inside the {FLARE_RADIUS} yd reveal",
            f.burnt
        );
        if f.burnt < steady_end {
            steady += 1;
            tightest = tightest.min(f.pool_extent);
            assert!(
                f.pool_min_inside >= VISIBLE_ALPHA,
                "at {:.2}s the pool is dark ({:.3}) somewhere inside the reveal radius",
                f.burnt,
                f.pool_min_inside
            );
        }
    }
    let expected = (steady_end / DT) as usize;
    assert!(
        steady + 2 >= expected,
        "only {steady} of {expected} steady-burn ticks measured"
    );
    assert!(
        tightest >= FLARE_RADIUS + LIT_MARGIN,
        "the pool's light stops at {tightest:.2} yd on some tick, not clear of the {FLARE_RADIUS} yd reveal"
    );
}

/// AS-232: the rim is the line a Rogue crosses to be seen. It is lit at a
/// stated floor on every tick the Flare burns — no end fade, since the reveal
/// does not fade — and brighter than the pool where it lies, so it reads as
/// an edge and not as more floor.
#[test]
fn the_rim_is_bright_against_the_pool_for_the_whole_burn() {
    let (_, _, _, frames) = burn();
    assert!(frames.len() > 500, "only {} ticks", frames.len());
    for f in &frames {
        assert!(
            f.rim_alpha >= RIM_ALPHA_FLOOR,
            "at {:.2}s the rim is drawn at alpha {:.3}",
            f.burnt,
            f.rim_alpha
        );
        assert!(
            f.rim_alpha >= RIM_CONTRAST * f.pool_at_radius,
            "at {:.2}s the rim ({:.3}) barely clears the pool under it ({:.3})",
            f.burnt,
            f.rim_alpha,
            f.pool_at_radius
        );
    }
}
