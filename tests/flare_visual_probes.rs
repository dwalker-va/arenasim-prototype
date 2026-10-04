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
    animate_client_particles, spawn_flare_visuals, update_flare_visuals, AbilityDefinitions, Aura,
    ClientParticle, Combatant, FlarePart, FlareRig, FlareZone, GameRng, MovementConfig,
    FLARE_CORE_DIAMETER, FLARE_CORE_HEIGHT, FLARE_CORE_SPRITES, FLARE_POOL_GAIN,
    FLARE_POOL_RADIUS_SCALE, FLARE_TWINKLE_MAX, FLARE_TWINKLE_MIN,
};

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
        casting_ability: None,
        pet_type: None,
        pet: None,
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

        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let mut combat_log = CombatLog::default();
        let mut game_rng = GameRng::from_seed(7);
        let mut trace = DecisionTrace::default();
        trace.current_sim_time = 50.0 / 6.0 + 0.02;

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
        decide_hunter_action(
            &mut commands,
            &mut combat_log,
            &mut game_rng,
            &abilities,
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
        frames.push(Frame {
            burnt: FLARE_DURATION - left,
            pool_alpha: pool_rgb.alpha,
            pool_rgb: [pool_rgb.red, pool_rgb.green, pool_rgb.blue],
            core_sizes,
            core_rgb,
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
/// on the Flare; the rim exactly at the gameplay radius, the pool reaching it;
/// the burning flare's sprites at the flare, above the floor, facing the
/// camera.
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

    let rim = h.part(FlarePart::Rim);
    let rim_vs = h.world_vertices(rim);
    let outer = rim_vs
        .iter()
        .map(|v| v.xz().distance(centre.xz()))
        .fold(0.0f32, f32::max);
    assert!((outer - FLARE_RADIUS).abs() < 1e-3, "rim reaches {outer}");
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
    let want = FLARE_RADIUS * FLARE_POOL_RADIUS_SCALE;
    assert!((reach_x - want).abs() < 1e-3 && (reach_z - want).abs() < 1e-3);
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
