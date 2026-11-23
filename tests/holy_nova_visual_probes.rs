//! Probes for Holy Nova's and Reflective Shield's visuals (AS-355): the gold
//! dome at the Priest's feet growing to the reach its book gives the spell,
//! its trailing stars, the holy starburst at two scales — Holy Nova's hit on
//! each enemy and a reflect on its attacker — and the TBC star fall on each
//! ally the nova heals (`rendering/effects/holy_nova.rs`,
//! `rendering/effects/holy_starburst.rs`).
//!
//! Everything here asserts what is DRAWN — world positions under propagated
//! `GlobalTransform`s, the materials' colours, alphas and blend modes, the
//! quads' scales — against the signed-off AS-335 Priest Talent Visuals Bench
//! (version 1792207490-53c9, the "Holy Nova" and "Reflective Shield" tabs),
//! transcribed below straight from
//! `docs/design/benches/2026-10-17-priest-talent-bench.html` and NOT from the
//! modules' tables, so a wrong value in a module cannot agree with itself.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the
//! chains `states/mod.rs` registers, and the real sim landings
//! (`land_instant_effects`, `process_damage_follow_ups`) and instant-ability
//! router wherever the wiring from the sim is the claim.

use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::TAU;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass::{self, Priest, Warrior};
use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{Scene, SceneInstantMarker};
use arenasim::states::play_match::class_ai::{QueuedAoeDamage, QueuedAoeHeal, QueuedInstantAttack};
use arenasim::states::play_match::combat_core::process_damage_follow_ups;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraDamageMods, AuraType, Combatant, GameRng, HealCastHand,
    HealCastKind, HitFlinch, InstantAbilityFired, MatchCountdown, Pet, PetType, PlayMatchEntity,
    PolymorphedVisual, StarFallLanding, StarburstLanding, StarburstScale, VisualBody,
};
use arenasim::states::play_match::instant_landings::{
    land_instant_effects, InstantLandings, PendingAreaHit, PendingStrike,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{
    animate_holy_nova_domes, animate_holy_star_falls, animate_holy_starbursts, cleanup_play_match,
    consume_instant_ability_signals, place_holy_nova_domes, place_holy_star_falls,
    place_holy_starbursts, spawn_holy_star_falls, spawn_holy_starbursts, spawn_instant_cast_hands,
    DomePiece, FallPiece, HolyNovaDome, HolyStarFall, HolyStarburst, StarburstPiece, FLINCH_DIP,
    IMPACT_PET_STATURE,
};

// ── The bench, transcribed ───────────────────────────────────────────────────

/// The bench's `ANCH.chest`.
const BENCH_CHEST: f32 = 1.45;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;
/// A pet's sim height, and its body's drop below it.
const PET_SIM_Y: f32 = 0.75;
const PET_BODY_DROP: f32 = -0.45;

/// The "Holy Nova" tab's knobs at the ruling: size 1, spread 1.6 (the dome's
/// reach is the book's, by the ruling, not 6.22 x 1.6).
const NOVA_SPREAD: f32 = 1.6;

/// `mkFx({id:'nova'...}).customDraw`, the dome:
/// `if(t<767){ r=6.22*(t/767)*s, a=keys([[0,0],[134,0.7],[767,0]],t);
///  for k<9: h=k/9; ring3([vp.x,0,vp.z], r*h, r*sqrt(1-h*h), [255,216,0], a*(1-h*0.5), 2);
///  groundDisc([vp.x,0,vp.z], 0.03, r, [255,216,0], a*0.25) }`.
const DOME_MS: f32 = 767.0;
const DOME_A: [(f32, f32); 3] = [(0.0, 0.0), (134.0, 0.7), (767.0, 0.0)];
const DOME_RGB: [f32; 3] = [255.0, 216.0, 0.0];
/// The 2 px stroke at the match camera, and the floor lift (`groundDisc`'s
/// 0.03).
const LINE: f32 = 0.092;
const LIFT: f32 = 0.03;

/// The `stars` layer: `{anchor:'feet', pos:[0,0,0.4], vRange:1.4, speed:1.67,
/// spin:0.6, life:1.0, rate:[[0,0],[300,500],[767,0]], win:[[0,1],[767,0]],
/// area:6.5, areaY:0.4, color:[[0,[255,252,222]],[0.5,[255,249,98]],
/// [1,[255,222,39]]], alpha:[[0,0.39],[0.5,1],[1,0]], scale:[[0,0.56],
/// [0.5,0.28],[1,0.03]], kind:'star'}`.
mod stars {
    pub const RATE: [(f32, f32); 3] = [(0.0, 0.0), (0.3, 500.0), (0.767, 0.0)];
    pub const LIFT: f32 = 0.4;
    pub const AREA: f32 = 6.5;
    pub const AREA_Y: f32 = 0.4;
    pub const CONE: f32 = 1.4;
    pub const SPEED: f32 = 1.67;
    pub const SPIN: f32 = 0.6;
    pub const LIFE: f32 = 1.0;
    pub const RGB: [(f32, [f32; 3]); 3] = [
        (0.0, [255.0, 252.0, 222.0]),
        (0.5, [255.0, 249.0, 98.0]),
        (1.0, [255.0, 222.0, 39.0]),
    ];
    pub const ALPHA: [(f32, f32); 3] = [(0.0, 0.39), (0.5, 1.0), (1.0, 0.0)];
    pub const SCALE: [(f32, f32); 3] = [(0.0, 0.56), (0.5, 0.28), (1.0, 0.03)];
}

/// The heals (`K.heal == 1`, TBC): `u = t - 150; if(u<2500){ a=keys([[0,0],
/// [300,1],[1234,1],[1667,0]],u); groundDisc(al,0.03,0.7,[255,210,60],a*0.6);
/// for k<12: ph=((u/1000)*0.9+k/12)%1; sprite('star',[al.x+0.5*sin(k*2.1),
/// 2.3-ph*2.1, al.z+0.5*cos(k*1.3)], 0.14, [253,255,170],
/// keys([[0,0],[300,1],[1500,0]],u)*(1-ph*0.5)) }`.
mod fall {
    pub const DELAY_MS: f32 = 150.0;
    pub const SPAN_MS: f32 = 2500.0;
    pub const GLOW_A: [(f32, f32); 4] = [(0.0, 0.0), (300.0, 1.0), (1234.0, 1.0), (1667.0, 0.0)];
    pub const GLOW_R: f32 = 0.7;
    pub const GLOW_RGB: [f32; 3] = [255.0, 210.0, 60.0];
    pub const GLOW_GAIN: f32 = 0.6;
    pub const STARS: u8 = 12;
    pub const STAR_A: [(f32, f32); 3] = [(0.0, 0.0), (300.0, 1.0), (1500.0, 0.0)];
    pub const STAR_RGB: [f32; 3] = [253.0, 255.0, 170.0];
    pub const STAR_SIZE: f32 = 0.14;
}

/// One of the starburst's emitter layers.
struct BenchLayer {
    /// The bench's `rate`, keyed in ms, read inside its `win` (also ms).
    rate: &'static [(f32, f32)],
    win: (f32, f32),
    speed: f32,
    gravity: f32,
    life: f32,
    rgb: [(f32, [f32; 3]); 2],
    scale: [(f32, f32); 2],
    tail: f32,
}

/// One scale: its flash (`bbGlow` size keys, alpha keys, gain) and layers.
struct BenchBurst {
    flash_size: &'static [(f32, f32)],
    flash_a: &'static [(f32, f32)],
    gain: f32,
    layers: &'static [BenchLayer],
}

const FLASH_RGB: [f32; 3] = [255.0, 241.0, 200.0];

/// The "Holy Nova" tab: `burstflash` (`sz=keys([[150,0.83],[300,3.33],
/// [500,0.03]],t), a=keys([[150,0],[220,1],[500,0]],t); bbGlow(..., a*0.7)`)
/// and `burst` (`vRange:3.14, speed:2.6, gravity:6.9, life:0.75,
/// rate:[[0,0],[200,250],[267,250],[400,0]], win:[[0,0],[200,1],[600,0]],
/// color:[[0,[255,255,255]],[1,[255,220,90]]], alpha:[[0,1],[1,0]],
/// scale:[[0,0.11],[1,0.03]]`) at the tab's spread 1.6.
const HIGH: BenchBurst = BenchBurst {
    flash_size: &[(150.0, 0.83), (300.0, 3.33), (500.0, 0.03)],
    flash_a: &[(150.0, 0.0), (220.0, 1.0), (500.0, 0.0)],
    gain: 0.7,
    layers: &[BenchLayer {
        rate: &[(0.0, 0.0), (200.0, 250.0), (267.0, 250.0), (400.0, 0.0)],
        win: (200.0, 600.0),
        speed: 2.6 * NOVA_SPREAD,
        gravity: 6.9,
        life: 0.75,
        rgb: [(0.0, [255.0, 255.0, 255.0]), (1.0, [255.0, 220.0, 90.0])],
        scale: [(0.0, 0.11), (1.0, 0.03)],
        tail: 0.0,
    }],
};

/// The "Reflective Shield" tab: `flash` (`sz=keys([[0,0.83],[150,2.5],
/// [400,0.03]],t), a=keys([[0,0],[80,1],[400,0]],t); bbGlow(..., a*0.75)`),
/// `showers` (`speed:2.4, gravity:6.9, life:0.75, rate:[[0,0],[150,100],
/// [300,0]], win:[[0,1],[400,0]], scale:[[0,0.09],[1,0.03]]`) and `streaks`
/// (`speed:2.0, life:0.6, rate:[[0,0],[150,150],[300,0]], win:[[0,1],
/// [400,0]], tail:4, color:[[0,[255,240,150]],[1,[212,131,58]]],
/// scale:[[0,0.05],[1,0.03]]`).
const LOW: BenchBurst = BenchBurst {
    flash_size: &[(0.0, 0.83), (150.0, 2.5), (400.0, 0.03)],
    flash_a: &[(0.0, 0.0), (80.0, 1.0), (400.0, 0.0)],
    gain: 0.75,
    layers: &[
        BenchLayer {
            rate: &[(0.0, 0.0), (150.0, 100.0), (300.0, 0.0)],
            win: (0.0, 400.0),
            speed: 2.4,
            gravity: 6.9,
            life: 0.75,
            rgb: [(0.0, [255.0, 255.0, 255.0]), (1.0, [255.0, 220.0, 90.0])],
            scale: [(0.0, 0.09), (1.0, 0.03)],
            tail: 0.0,
        },
        BenchLayer {
            rate: &[(0.0, 0.0), (150.0, 150.0), (300.0, 0.0)],
            win: (0.0, 400.0),
            speed: 2.0,
            gravity: 0.0,
            life: 0.6,
            rgb: [(0.0, [255.0, 240.0, 150.0]), (1.0, [212.0, 131.0, 58.0])],
            scale: [(0.0, 0.05), (1.0, 0.03)],
            tail: 4.0,
        },
    ],
};

fn bench(scale: StarburstScale) -> &'static BenchBurst {
    match scale {
        StarburstScale::High => &HIGH,
        StarburstScale::Low => &LOW,
    }
}

/// The bench's `keys`: linear between keys, clamped outside them.
fn keys(k: &[(f32, f32)], t: f32) -> f32 {
    if t <= k[0].0 {
        return k[0].1;
    }
    for w in k.windows(2) {
        if t <= w[1].0 {
            let span = w[1].0 - w[0].0;
            let u = if span > 0.0 { (t - w[0].0) / span } else { 1.0 };
            return w[0].1 + (w[1].1 - w[0].1) * u;
        }
    }
    k[k.len() - 1].1
}

fn keys3(k: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let ch = |c: usize| {
        let ks: Vec<(f32, f32)> = k.iter().map(|(t, v)| (*t, v[c])).collect();
        keys(&ks, t)
    };
    [ch(0), ch(1), ch(2)]
}

/// The bench's accumulator in continuous time: where the integral of a rate
/// (keyed in ms, gated by `win`) first reaches 1, 2, ... Brute force,
/// independent of the modules' solve.
fn bench_births(rate: &[(f32, f32)], win: (f32, f32)) -> Vec<f32> {
    let mut births = Vec::new();
    let (mut acc, mut t) = (0.0_f64, 0.0_f64);
    let dt = 1e-6;
    while t < win.1 as f64 / 1000.0 {
        let ms = ((t + dt / 2.0) * 1000.0) as f32;
        if ms >= win.0 {
            acc += keys(rate, ms) as f64 * dt;
        }
        t += dt;
        if acc >= births.len() as f64 + 1.0 - 1e-6 {
            births.push(t as f32);
        }
    }
    births
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

// ── The harness ──────────────────────────────────────────────────────────────

struct Harness {
    app: App,
    dt: f32,
}

impl Harness {
    fn new() -> Self {
        Harness::with_tick(0.016)
    }

    fn with_tick(dt: f32) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            dt,
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(0))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ArenaDampening::default())
        .insert_resource(InstantLandings::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        });
        // The sim's landings, the router, then the chains `states/mod.rs`
        // registers.
        app.add_systems(
            Update,
            (
                land_instant_effects,
                process_damage_follow_ups,
                spawn_instant_cast_hands,
                consume_instant_ability_signals,
                spawn_holy_starbursts,
                animate_holy_starbursts,
                spawn_holy_star_falls,
                animate_holy_star_falls,
                animate_holy_nova_domes,
            )
                .chain(),
        );
        app.add_systems(
            PostUpdate,
            (
                place_holy_nova_domes,
                place_holy_star_falls,
                place_holy_starbursts,
            )
                .before(TransformSystem::TransformPropagate),
        );
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness { app, dt }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    fn unit(&mut self, team: u8, slot: u8, class: CharacterClass, at: Vec3) -> Entity {
        self.world()
            .spawn((
                Combatant::new(team, slot, class),
                Transform::from_translation(at),
            ))
            .id()
    }

    /// A Priest whose book holds Holy Nova (a talent's), its reach patched to
    /// `reach` when one is given.
    fn priest(&mut self, at: Vec3, reach: Option<f32>) -> Entity {
        let mut c = Combatant::new(1, 0, Priest);
        let mut book = c
            .book()
            .clone()
            .granting(AbilityDefinitions::shipped(), AbilityType::HolyNova);
        if let Some(r) = reach {
            book = book.patching(AbilityType::HolyNova, |cfg| cfg.range = r);
        }
        c = c.with_book(book);
        self.world()
            .spawn((c, Transform::from_translation(at)))
            .id()
    }

    fn pet(&mut self, at: Vec3) -> Entity {
        let owner = self.unit(2, 3, CharacterClass::Hunter, at + Vec3::X * 6.0);
        self.world()
            .spawn((
                Combatant::new(2, 4, CharacterClass::Hunter),
                Pet {
                    owner,
                    pet_type: PetType::Spider,
                },
                Transform::from_translation(at),
            ))
            .id()
    }

    fn nova(&mut self, priest: Entity) {
        self.world().spawn((
            InstantAbilityFired {
                caster: priest,
                target: None,
                ability: AbilityType::HolyNova,
                is_crit: false,
            },
            PlayMatchEntity,
        ));
    }

    fn burst(&mut self, target: Entity, scale: StarburstScale) {
        self.world()
            .spawn((StarburstLanding { target, scale }, PlayMatchEntity));
    }

    fn fall(&mut self, target: Entity) {
        self.world()
            .spawn((StarFallLanding { target }, PlayMatchEntity));
    }

    fn camera(&mut self, t: Transform) {
        self.world()
            .spawn((Camera3d::default(), t, GlobalTransform::from(t)));
    }

    fn count<T: Component>(&mut self) -> usize {
        let mut q = self.world().query_filtered::<(), With<T>>();
        q.iter(self.app.world()).count()
    }

    fn domes(&mut self) -> Vec<(Entity, Entity, f32, f32)> {
        let mut q = self.world().query::<(Entity, &HolyNovaDome)>();
        q.iter(self.app.world())
            .map(|(e, d)| (e, d.caster, d.reach, d.age))
            .collect()
    }

    fn bursts(&mut self) -> Vec<(Entity, Entity, StarburstScale, f32)> {
        let mut q = self.world().query::<(Entity, &HolyStarburst)>();
        q.iter(self.app.world())
            .map(|(e, b)| (e, b.target, b.scale, b.age))
            .collect()
    }

    fn falls(&mut self) -> Vec<(Entity, Entity, f32)> {
        let mut q = self.world().query::<(Entity, &HolyStarFall)>();
        q.iter(self.app.world())
            .map(|(e, f)| (e, f.target, f.age))
            .collect()
    }

    /// Every drawn piece of `T` under rig `rig`.
    fn drawn<T: Component + Copy>(&mut self, rig: Entity) -> Vec<Drawn<T>> {
        let world = self.app.world();
        let Some(children) = world.get::<Children>(rig) else {
            return Vec::new();
        };
        let materials = world.resource::<Assets<StandardMaterial>>();
        let meshes = world.resource::<Assets<Mesh>>();
        children
            .iter()
            .filter_map(|c| {
                let piece = *world.get::<T>(c)?;
                let g = *world.get::<GlobalTransform>(c)?;
                let visible = *world.get::<Visibility>(c)? != Visibility::Hidden;
                let m = materials
                    .get(&world.get::<MeshMaterial3d<StandardMaterial>>(c)?.0)?
                    .clone();
                let mesh = meshes.get(&world.get::<Mesh3d>(c)?.0)?;
                let (scale, rotation, at) = g.to_scale_rotation_translation();
                let s = m.base_color.to_srgba();
                Some(Drawn {
                    piece,
                    at,
                    rotation,
                    scale,
                    visible,
                    rgb: [s.red * 255.0, s.green * 255.0, s.blue * 255.0],
                    alpha: s.alpha,
                    mode: m.alpha_mode,
                    unit_quad: is_unit_quad(mesh),
                })
            })
            .collect()
    }
}

/// A piece as it is drawn.
#[derive(Clone, Copy, Debug)]
struct Drawn<T> {
    piece: T,
    at: Vec3,
    rotation: Quat,
    scale: Vec3,
    visible: bool,
    rgb: [f32; 3],
    alpha: f32,
    mode: AlphaMode,
    unit_quad: bool,
}

/// Whether a mesh is the unit quad every scale is measured against: four
/// corners at ±0.5 in x and y, flat in z.
fn is_unit_quad(mesh: &Mesh) -> bool {
    let Some(positions) = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|a| a.as_float3())
    else {
        return false;
    };
    positions.len() == 4
        && positions
            .iter()
            .all(|p| p[0].abs() == 0.5 && p[1].abs() == 0.5 && p[2] == 0.0)
}

fn rgb_near(got: [f32; 3], want: [f32; 3], tol: f32) -> bool {
    (0..3).all(|i| (got[i] - want[i]).abs() <= tol)
}

/// The roll of a camera-facing quad about the camera's back axis.
fn roll_of(camera: Quat, rotation: Quat) -> f32 {
    let x = camera.inverse() * rotation * Vec3::X;
    x.y.atan2(x.x)
}

/// The match camera (`CameraController`): `zoom` yards out, `pitch` from the
/// vertical, at `yaw`, looking at `target`.
fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

fn chest(root: Vec3, pet: bool) -> Vec3 {
    if pet {
        root + Vec3::Y * (PET_BODY_DROP + (BENCH_CHEST - 1.0) * IMPACT_PET_STATURE)
    } else {
        root + Vec3::Y * (BENCH_CHEST - COMBATANT_Y)
    }
}

fn floor_under(root: Vec3) -> Vec3 {
    root - Vec3::Y * COMBATANT_Y
}

// ── The real path from the sim ───────────────────────────────────────────────

fn fortitude() -> Aura {
    Aura {
        effect_type: AuraType::MaxHealthIncrease,
        duration: 600.0,
        ability_name: "Power Word: Fortitude".into(),
        ..Default::default()
    }
}

/// The Priest AI that novas (a hurt Priest, an enemy Warrior beside it with
/// Pummel ready) spawns exactly one caster-centred marker for its nova, the
/// one its hands and its dome play from; its control, the same Priest with
/// the Warrior's Pummel cooling down, Flash Heals and spawns none.
#[test]
fn the_priest_ai_marks_its_nova_once_and_caster_centred() {
    let scene = |pummel_ready: bool| {
        let mut scene = Scene::new()
            .unit("priest", 1, Priest)
            .health(0.6)
            .aura_full("priest", fortitude())
            .aura("priest", AuraType::WeakenedSoul, 10.0)
            .cooldown("priest", AbilityType::PsychicScream, 20.0)
            .unit("enemy", 2, Warrior)
            .at(1.5, 0.0)
            .rage(30.0)
            .targeting("priest")
            .grant("priest", AbilityType::HolyNova);
        if !pummel_ready {
            scene = scene.cooldown("enemy", AbilityType::Pummel, 5.0);
        }
        scene.decide("priest")
    };
    let novas = scene(true);
    assert_eq!(novas.chose(), Some((AbilityType::HolyNova, None)));
    assert_eq!(
        novas.instant_markers,
        vec![SceneInstantMarker {
            caster: "priest",
            target: None,
            ability: AbilityType::HolyNova,
        }]
    );
    let heals = scene(false);
    assert_eq!(
        heals.chose(),
        Some((AbilityType::FlashHeal, Some("priest")))
    );
    assert!(heals.instant_markers.is_empty());
}

/// The marker raises one dome, growing to the reach in the Priest's own
/// book: the shipped 10 yards, and 7.5 for a book patched to it. Ring 0 (the
/// ground ring) is checked every frame against `r = reach × t / 767`.
#[test]
fn the_dome_reaches_the_reach_in_the_priests_book() {
    let shipped = AbilityDefinitions::shipped()
        .get(&AbilityType::HolyNova)
        .unwrap()
        .range;
    assert_eq!(shipped, 10.0);
    for (patch, reach) in [(None, shipped), (Some(7.5), 7.5)] {
        let mut h = Harness::new();
        let root = Vec3::new(2.0, COMBATANT_Y, -3.0);
        let priest = h.priest(root, patch);
        h.nova(priest);
        let mut drawn_frames = 0;
        let mut widest = 0.0_f32;
        loop {
            h.tick(1);
            let domes = h.domes();
            if domes.is_empty() {
                break;
            }
            assert_eq!(domes.len(), 1);
            let (rig, caster, got_reach, age) = domes[0];
            assert_eq!((caster, got_reach), (priest, reach));
            let ms = age * 1000.0;
            let r = reach * ms / DOME_MS;
            for d in h.drawn::<DomePiece>(rig) {
                let DomePiece::Ring { ring: 0, .. } = d.piece else {
                    continue;
                };
                if !d.visible {
                    continue;
                }
                let flat = (d.at - floor_under(root)).with_y(0.0).length();
                // A chord's midpoint stands cos(π/48) of the radius out.
                assert!(
                    close(flat, r * (TAU / 96.0).cos(), 2e-3),
                    "reach {reach} at {ms} ms: {flat} vs {r}"
                );
                widest = widest.max(flat);
                drawn_frames += 1;
            }
        }
        assert!(drawn_frames > 40 * 48, "{drawn_frames}");
        assert!(
            widest > 0.97 * reach * (TAU / 96.0).cos(),
            "the ring grows to the reach: {widest}"
        );
    }
}

fn aoe_hit(caster: Entity, c: &Combatant, target: Entity, ability: AbilityType) -> PendingAreaHit {
    PendingAreaHit {
        hit: QueuedAoeDamage {
            caster,
            target,
            damage: 20.0,
            caster_team: c.team,
            caster_slot: c.slot,
            caster_class: c.class,
            target_pos: Vec3::ZERO,
            is_crit: false,
            ability,
            school: SpellSchool::Holy,
        },
        dealt: 1.0,
    }
}

fn aoe_heal(caster: Entity, c: &Combatant, target: Entity) -> QueuedAoeHeal {
    QueuedAoeHeal {
        caster,
        target,
        heal: 20.0,
        is_crit: false,
        caster_team: c.team,
        caster_slot: c.slot,
        caster_class: c.class,
        ability: AbilityType::HolyNova,
    }
}

/// The real area landing: each enemy Holy Nova strikes — a Warrior and a pet
/// — gets exactly one High starburst, and each ally it heals — a teammate
/// and the Priest itself — exactly one star fall. A Frost Nova hit in the
/// same frame gets none, and neither does a dead enemy the nova was queued
/// on. The hits and heals landed (the log), so the count is not vacuous.
#[test]
fn each_enemy_hit_gets_one_starburst_and_each_healed_ally_one_fall() {
    let mut h = Harness::new();
    let priest = h.priest(Vec3::new(0.0, COMBATANT_Y, 0.0), None);
    let ally = h.unit(1, 1, Warrior, Vec3::new(4.0, COMBATANT_Y, 0.0));
    let enemy = h.unit(2, 0, Warrior, Vec3::new(2.0, COMBATANT_Y, 2.0));
    let pet = h.pet(Vec3::new(-2.0, PET_SIM_Y, 2.0));
    let frosted = h.unit(2, 1, Warrior, Vec3::new(-2.0, COMBATANT_Y, -2.0));
    let dead = h.unit(2, 2, Warrior, Vec3::new(1.0, COMBATANT_Y, -3.0));
    h.world().get_mut::<Combatant>(dead).unwrap().current_health = 0.0;
    let mage = h.unit(
        1,
        2,
        CharacterClass::Mage,
        Vec3::new(-6.0, COMBATANT_Y, 0.0),
    );
    let c = h.world().get::<Combatant>(priest).unwrap().clone();
    let m = h.world().get::<Combatant>(mage).unwrap().clone();
    {
        let mut landings = h.world().resource_mut::<InstantLandings>();
        for t in [enemy, pet, dead] {
            landings
                .area_damage
                .push(aoe_hit(priest, &c, t, AbilityType::HolyNova));
        }
        landings
            .area_damage
            .push(aoe_hit(mage, &m, frosted, AbilityType::FrostNova));
        landings.area_heals.push(aoe_heal(priest, &c, ally));
        landings.area_heals.push(aoe_heal(priest, &c, priest));
    }
    h.tick(1);
    let bursts: BTreeSet<(Entity, StarburstScale)> =
        h.bursts().into_iter().map(|(_, t, s, _)| (t, s)).collect();
    assert_eq!(
        bursts,
        BTreeSet::from([(enemy, StarburstScale::High), (pet, StarburstScale::High)])
    );
    assert_eq!(h.bursts().len(), 2, "one each");
    let falls: Vec<Entity> = h.falls().into_iter().map(|(_, t, _)| t).collect();
    assert_eq!(
        falls.iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([ally, priest])
    );
    assert_eq!(falls.len(), 2, "one each");
    let log = h.app.world().resource::<CombatLog>();
    let lines = |s: &str| log.entries.iter().filter(|e| e.message.contains(s)).count();
    assert_eq!(lines("Holy Nova hits"), 2);
    assert_eq!(lines("Holy Nova heals"), 2);
    assert_eq!(lines("Frost Nova hits"), 1);
    // A Holy Nova hit flinches nothing; the markers were all consumed.
    assert!(h.app.world().get::<HitFlinch>(enemy).is_none());
    assert_eq!(
        h.count::<StarburstLanding>() + h.count::<StarFallLanding>(),
        0
    );
}

fn strike(attacker: Entity, a: &Combatant, target: Entity) -> PendingStrike {
    PendingStrike {
        attack: QueuedInstantAttack {
            attacker,
            target,
            damage: 40.0,
            attacker_team: a.team,
            attacker_slot: a.slot,
            attacker_class: a.class,
            ability: AbilityType::MortalStrike,
            is_crit: false,
        },
        dealt: 1.0,
    }
}

/// The real reflect: two Warriors strike a Priest whose shield reflects
/// (`reflect_absorbed`, as Reflective Shield sets it), and each gets exactly
/// one Low starburst and the wound flinch; a third strikes a Paladin under
/// Retribution Aura, whose retaliation is silent by ruling, and gets none.
/// The reflects landed (the log).
#[test]
fn each_shield_reflect_plays_one_small_starburst_on_its_attacker() {
    let mut h = Harness::new();
    let priest = h.priest(Vec3::new(0.0, COMBATANT_Y, 0.0), None);
    let paladin = h.unit(
        1,
        1,
        CharacterClass::Paladin,
        Vec3::new(8.0, COMBATANT_Y, 0.0),
    );
    let a = h.unit(2, 0, Warrior, Vec3::new(1.5, COMBATANT_Y, 0.0));
    let b = h.unit(2, 1, Warrior, Vec3::new(-1.5, COMBATANT_Y, 0.0));
    let c = h.unit(2, 2, Warrior, Vec3::new(9.5, COMBATANT_Y, 0.0));
    h.world().entity_mut(priest).insert(ActiveAuras {
        auras: vec![Aura {
            effect_type: AuraType::Absorb,
            duration: 30.0,
            magnitude: 500.0,
            caster: Some(priest),
            spell_school: Some(SpellSchool::Holy),
            ability_name: "Power Word: Shield".into(),
            damage_mods: AuraDamageMods {
                reflect_absorbed: Some(0.3),
                ..Default::default()
            },
            ..Default::default()
        }],
    });
    h.world().entity_mut(paladin).insert(ActiveAuras {
        auras: vec![Aura {
            effect_type: AuraType::DamageReflect,
            duration: 300.0,
            magnitude: 6.0,
            caster: Some(paladin),
            spell_school: Some(SpellSchool::Holy),
            ability_name: "Retribution Aura".into(),
            ..Default::default()
        }],
    });
    let combatant = |h: &mut Harness, e| h.world().get::<Combatant>(e).unwrap().clone();
    let (ca, cb, cc) = (
        combatant(&mut h, a),
        combatant(&mut h, b),
        combatant(&mut h, c),
    );
    {
        let mut landings = h.world().resource_mut::<InstantLandings>();
        landings.strikes.push(strike(a, &ca, priest));
        landings.strikes.push(strike(b, &cb, priest));
        landings.strikes.push(strike(c, &cc, paladin));
    }
    h.tick(1);
    let mut bursts: Vec<(Entity, StarburstScale)> =
        h.bursts().into_iter().map(|(_, t, s, _)| (t, s)).collect();
    bursts.sort();
    let mut want = vec![(a, StarburstScale::Low), (b, StarburstScale::Low)];
    want.sort();
    assert_eq!(bursts, want);
    for attacker in [a, b] {
        let flinch = h.app.world().get::<HitFlinch>(attacker).expect("a flinch");
        assert!(close(flinch.depth, FLINCH_DIP, 1e-6));
    }
    assert!(h.app.world().get::<HitFlinch>(c).is_none());
    let log = h.app.world().resource::<CombatLog>();
    let reflects = |from: &str| {
        log.entries
            .iter()
            .filter(|e| e.message.contains("[REFLECT]") && e.message.contains(from))
            .count()
    };
    assert_eq!(reflects("Power Word: Shield"), 2);
    assert_eq!(reflects("Retribution Aura"), 1);
}

/// Holy Nova's marker flashes the high Holy hands, once, on both hands.
#[test]
fn the_nova_flashes_the_high_holy_hands() {
    let mut h = Harness::new();
    let priest = h.priest(Vec3::new(0.0, COMBATANT_Y, 0.0), None);
    let body = h
        .world()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    h.world().entity_mut(priest).add_child(body);
    h.nova(priest);
    h.tick(1);
    let mut q = h.world().query::<&HealCastHand>();
    let hands: Vec<(HealCastKind, Entity)> =
        q.iter(h.app.world()).map(|r| (r.kind, r.caster)).collect();
    assert_eq!(hands, vec![(HealCastKind::HolyFlash, priest); 2]);
}

/// A second nova while the first dome plays raises its own dome; a second
/// heal on an ally whose fall is still playing, and a second hit on an enemy
/// whose starburst is, each play their own rig beside the first.
#[test]
fn a_second_landing_mid_effect_plays_its_own_rig() {
    let mut h = Harness::new();
    let priest = h.priest(Vec3::new(0.0, COMBATANT_Y, 0.0), None);
    let ally = h.unit(1, 1, Warrior, Vec3::new(3.0, COMBATANT_Y, 0.0));
    let enemy = h.unit(2, 0, Warrior, Vec3::new(-3.0, COMBATANT_Y, 0.0));
    h.nova(priest);
    h.fall(ally);
    h.burst(enemy, StarburstScale::High);
    h.tick(20);
    h.nova(priest);
    h.fall(ally);
    h.burst(enemy, StarburstScale::High);
    h.tick(1);
    let dt = h.dt;
    let ages = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v
    };
    let first = 21.0 * dt;
    for (what, got) in [
        ("domes", ages(h.domes().into_iter().map(|d| d.3).collect())),
        ("falls", ages(h.falls().into_iter().map(|d| d.2).collect())),
        (
            "bursts",
            ages(h.bursts().into_iter().map(|d| d.3).collect()),
        ),
    ] {
        assert_eq!(got.len(), 2, "{what}");
        assert!(
            close(got[0], dt, 1e-5) && close(got[1], first, 1e-4),
            "{what}: {got:?}"
        );
    }
}

// ── What is drawn, against the bench ────────────────────────────────────────

/// Every piece of every rig is drawn on the unit quad its scale is measured
/// against.
#[test]
fn every_piece_is_drawn_on_a_unit_quad() {
    let mut h = Harness::new();
    let priest = h.priest(Vec3::new(0.0, COMBATANT_Y, 0.0), None);
    let enemy = h.unit(2, 0, Warrior, Vec3::new(3.0, COMBATANT_Y, 0.0));
    h.nova(priest);
    h.fall(priest);
    h.burst(enemy, StarburstScale::Low);
    h.tick(2);
    let dome = h.domes()[0].0;
    let fall = h.falls()[0].0;
    let burst = h.bursts()[0].0;
    assert!(h.drawn::<DomePiece>(dome).iter().all(|d| d.unit_quad));
    assert!(h.drawn::<FallPiece>(fall).iter().all(|d| d.unit_quad));
    assert!(h.drawn::<StarburstPiece>(burst).iter().all(|d| d.unit_quad));
    // The pieces: 9 rings of 48, the ground glow, 191 stars; a glow and 12
    // stars; the flash, 15 showers and 22 streaks with their tails.
    assert_eq!(h.drawn::<DomePiece>(dome).len(), 9 * 48 + 1 + 191);
    assert_eq!(h.drawn::<FallPiece>(fall).len(), 13);
    assert_eq!(h.drawn::<StarburstPiece>(burst).len(), 1 + 15 + 22 * 2);
}

/// The dome, frame by frame at 8, 16 and 33 ms frames: each ring at its
/// height, its chords on its circle, the line's width, its alpha and colour,
/// additive; the ground glow at its radius and a quarter of the weight; all
/// gone at 767 ms, and the rig's age is its frames × dt throughout.
#[test]
fn the_dome_is_drawn_as_the_bench_draws_it_at_any_frame_rate() {
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let root = Vec3::new(-1.0, COMBATANT_Y, 2.0);
        let floor = floor_under(root);
        let priest = h.priest(root, None);
        h.nova(priest);
        let mut frames = 0u32;
        let (mut rings, mut grounds) = (0, 0);
        loop {
            h.tick(1);
            frames += 1;
            let Some(&(rig, _, reach, age)) = h.domes().first() else {
                break;
            };
            assert!(
                close(age, frames as f32 * dt, 1e-4),
                "{dt}: {age} at {frames}"
            );
            let ms = age * 1000.0;
            let a = keys(&DOME_A, ms);
            let r = reach * ms / DOME_MS;
            for d in h.drawn::<DomePiece>(rig) {
                match d.piece {
                    DomePiece::Ring { ring, .. } => {
                        let hk = ring as f32 / 9.0;
                        let want_a = a * (1.0 - hk * 0.5);
                        let rr = r * (1.0 - hk * hk).sqrt();
                        let shown = ms < DOME_MS && want_a > 0.003 && rr > 0.001;
                        assert_eq!(d.visible, shown, "{dt}: ring {ring} at {ms}");
                        if !shown {
                            continue;
                        }
                        let off = d.at - floor;
                        assert!(close(off.y, r * hk + LIFT, 1e-3), "ring {ring} height");
                        assert!(close(
                            off.with_y(0.0).length(),
                            rr * (TAU / 96.0).cos(),
                            2e-3
                        ));
                        assert!(close(d.scale.x, 2.0 * rr * (TAU / 96.0).sin(), 2e-3));
                        assert!(close(d.scale.y, LINE, 1e-5), "the line's width");
                        assert!((d.rotation * Vec3::Z - Vec3::Y).length() < 1e-4, "level");
                        assert!(close(d.alpha, want_a, 2e-3));
                        assert!(rgb_near(d.rgb, DOME_RGB, 0.5));
                        assert_eq!(d.mode, AlphaMode::Add);
                        rings += 1;
                    }
                    DomePiece::Ground => {
                        let want_a = a * 0.25;
                        let shown = ms < DOME_MS && want_a > 0.003 && r > 0.001;
                        assert_eq!(d.visible, shown, "{dt}: ground at {ms}");
                        if !shown {
                            continue;
                        }
                        assert!((d.at - (floor + Vec3::Y * LIFT)).length() < 1e-4);
                        assert!(close(d.scale.x, 2.0 * r, 1e-3) && close(d.scale.y, 2.0 * r, 1e-3));
                        assert!((d.rotation * Vec3::Z - Vec3::Y).length() < 1e-4, "level");
                        assert!(close(d.alpha, want_a, 2e-3));
                        assert!(rgb_near(d.rgb, DOME_RGB, 0.5));
                        assert_eq!(d.mode, AlphaMode::Add);
                        grounds += 1;
                    }
                    DomePiece::Star { .. } => {}
                }
            }
        }
        let shown_frames = (DOME_MS / 1000.0 / dt) as u32;
        assert!(
            grounds + 2 >= shown_frames && grounds <= shown_frames + 1,
            "{dt}: {grounds}"
        );
        assert!(rings > shown_frames * 48 * 8, "{dt}: {rings}");
    }
}

/// The trailing stars at 8, 16 and 33 ms frames: the bench's 191, each born
/// where its rate owes it inside the square round the Priest's feet, flying
/// straight at the spread speed within 1.4 rad of up, spinning 0.6 turns a
/// second, sized, faded and coloured on the bench's keys, additive.
#[test]
fn the_trailing_stars_fly_as_the_bench_draws_them_at_any_frame_rate() {
    let births = bench_births(&stars::RATE.map(|(t, r)| (t * 1000.0, r)), (0.0, DOME_MS));
    assert_eq!(births.len(), 191, "the bench's track owes 191");
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let root = Vec3::new(3.0, COMBATANT_Y, 1.0);
        let floor = floor_under(root);
        let priest = h.priest(root, None);
        h.nova(priest);
        let speed = stars::SPEED * NOVA_SPREAD;
        let half = stars::AREA * NOVA_SPREAD / 2.0;
        let half_y = stars::AREA_Y * NOVA_SPREAD / 2.0;
        // Per star: its launch, from its first sighting, and its roll then.
        let mut seen: BTreeMap<u32, (Vec3, Vec3, f32, f32)> = BTreeMap::new();
        loop {
            h.tick(1);
            let Some(&(rig, ..)) = h.domes().first() else {
                break;
            };
            let age = h.domes()[0].3;
            for d in h.drawn::<DomePiece>(rig) {
                let DomePiece::Star { j, born, .. } = d.piece else {
                    continue;
                };
                assert!(
                    close(born, births[j as usize], 1e-3),
                    "star {j}: born {born}"
                );
                let page = age - born;
                if page.abs() < 1e-4 || (page - stars::LIFE).abs() < 1e-4 {
                    continue;
                }
                assert_eq!(d.visible, (0.0..stars::LIFE).contains(&page), "star {j}");
                if !d.visible {
                    continue;
                }
                let t = page / stars::LIFE;
                assert!(close(d.scale.x, 2.0 * keys(&stars::SCALE, t), 1e-4));
                assert!(close(d.alpha, keys(&stars::ALPHA, t), 2e-3));
                assert!(rgb_near(d.rgb, keys3(&stars::RGB, t), 1.01));
                assert_eq!(d.mode, AlphaMode::Add);
                let roll = roll_of(Quat::IDENTITY, d.rotation);
                match seen.get(&j) {
                    None => {
                        // The first sighting gives its start and launch: the
                        // start in the square, the launch in the cone.
                        seen.insert(j, (d.at, Vec3::ZERO, page, roll));
                    }
                    Some(&(first, _, p0, r0)) => {
                        let v = (d.at - first) / (page - p0);
                        assert!(close(v.length(), speed, 1e-2), "star {j}: {}", v.length());
                        let start = first - v * p0 - floor;
                        assert!(start.x.abs() <= half + 1e-3 && start.z.abs() <= half + 1e-3);
                        assert!((start.y - stars::LIFT).abs() <= half_y + 1e-3);
                        assert!(v.normalize().y >= stars::CONE.cos() - 1e-3, "in the cone");
                        // Spin: the canvas turns it clockwise, 0.6 turns a second.
                        let turned = -stars::SPIN * TAU * (page - p0);
                        let diff = (roll - r0 - turned).rem_euclid(TAU);
                        assert!(diff < 1e-2 || diff > TAU - 1e-2, "star {j} spun {diff}");
                        seen.insert(j, (first, v, p0, r0));
                    }
                }
            }
        }
        assert_eq!(
            seen.keys().copied().collect::<BTreeSet<u32>>(),
            (0..191).collect(),
            "all 191 fly at {dt}"
        );
    }
}

/// The star fall at 8, 16 and 33 ms frames: from 150 ms to 2650 ms, twelve
/// gold stars falling round the ally from 2.3 to 0.2 yd above its feet,
/// blended as the bench's own sprites are, and a glow at its feet, additive,
/// each sized, placed and faded as the bench draws them.
#[test]
fn the_star_fall_is_drawn_as_the_bench_draws_it_at_any_frame_rate() {
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let root = Vec3::new(-2.0, COMBATANT_Y, 4.0);
        let feet = floor_under(root);
        let ally = h.unit(1, 1, Warrior, root);
        h.fall(ally);
        let (mut frames, mut stars_drawn, mut glows) = (0u32, 0, 0);
        loop {
            h.tick(1);
            frames += 1;
            let Some(&(rig, _, age)) = h.falls().first() else {
                break;
            };
            assert!(close(age, frames as f32 * dt, 1e-4));
            let u = age * 1000.0 - fall::DELAY_MS;
            let live = u > 0.0 && u < fall::SPAN_MS;
            for d in h.drawn::<FallPiece>(rig) {
                match d.piece {
                    FallPiece::Glow => {
                        let a = keys(&fall::GLOW_A, u) * fall::GLOW_GAIN;
                        assert_eq!(d.visible, live && a > 0.003, "glow at {u}");
                        if !d.visible {
                            continue;
                        }
                        assert!((d.at - (feet + Vec3::Y * LIFT)).length() < 1e-4);
                        assert!(close(d.scale.x, 2.0 * fall::GLOW_R, 1e-5));
                        assert!((d.rotation * Vec3::Z - Vec3::Y).length() < 1e-4);
                        assert!(close(d.alpha, a, 2e-3));
                        assert!(rgb_near(d.rgb, fall::GLOW_RGB, 0.5));
                        assert_eq!(d.mode, AlphaMode::Add);
                        glows += 1;
                    }
                    FallPiece::Star(k) => {
                        let kf = k as f32;
                        let ph = ((u / 1000.0) * 0.9 + kf / fall::STARS as f32).rem_euclid(1.0);
                        let a = keys(&fall::STAR_A, u) * (1.0 - ph * 0.5);
                        assert_eq!(d.visible, live && a > 0.003, "star {k} at {u}");
                        if !d.visible {
                            continue;
                        }
                        let want = feet
                            + Vec3::new(
                                0.5 * (kf * 2.1).sin(),
                                2.3 - ph * 2.1,
                                0.5 * (kf * 1.3).cos(),
                            );
                        assert!(
                            (d.at - want).length() < 2e-3,
                            "star {k}: {} vs {want}",
                            d.at
                        );
                        assert!(close(d.scale.x, fall::STAR_SIZE, 1e-5));
                        assert!(close(d.alpha, a, 2e-3));
                        assert!(rgb_near(d.rgb, fall::STAR_RGB, 0.5));
                        assert_eq!(
                            d.mode,
                            AlphaMode::Blend,
                            "customDraw's sprite is source-over"
                        );
                        assert!(close(roll_of(Quat::IDENTITY, d.rotation), 0.0, 1e-5));
                        stars_drawn += 1;
                    }
                }
            }
        }
        assert!(
            close(frames as f32 * dt, 2.65, dt + 1e-4),
            "the fall lives 2.65 s ({frames} frames at {dt})"
        );
        assert!(
            glows > 0 && stars_drawn > 12 * glows / 2,
            "{dt}: {glows} {stars_drawn}"
        );
    }
}

/// The starburst's flash at both scales, frame by frame: shown exactly where
/// the bench's weight clears its floor, at the chest, its width, alpha and
/// colour on the bench's keys, additive.
#[test]
fn the_starburst_flash_is_drawn_as_the_bench_draws_it() {
    for scale in [StarburstScale::High, StarburstScale::Low] {
        let b = bench(scale);
        let mut h = Harness::new();
        let root = Vec3::new(5.0, COMBATANT_Y, -1.0);
        let unit = h.unit(2, 0, Warrior, root);
        h.burst(unit, scale);
        let mut shown = 0;
        loop {
            h.tick(1);
            let Some(&(rig, _, _, age)) = h.bursts().first() else {
                break;
            };
            let ms = age * 1000.0;
            let a = keys(b.flash_a, ms) * b.gain;
            for d in h.drawn::<StarburstPiece>(rig) {
                if !matches!(d.piece, StarburstPiece::Flash) {
                    continue;
                }
                assert_eq!(d.visible, a > 0.003, "{scale:?} at {ms}");
                if !d.visible {
                    continue;
                }
                assert!((d.at - chest(root, false)).length() < 1e-4);
                let size = keys(b.flash_size, ms);
                assert!(close(d.scale.x, size, 1e-4) && close(d.scale.y, size, 1e-4));
                assert!(close(d.alpha, a, 2e-3));
                assert!(rgb_near(d.rgb, FLASH_RGB, 0.5));
                assert_eq!(d.mode, AlphaMode::Add);
                shown += 1;
            }
        }
        assert!(shown > 12, "{scale:?}: {shown}");
    }
}

/// The starburst's particles at both scales and at 8, 16 and 33 ms frames:
/// each layer bears exactly the bench's count, each particle born where its
/// rate owes it, flying out of the chest at the layer's speed under its
/// gravity (checked against its launch at every later frame), never
/// spinning, sized, faded and coloured on the bench's keys, additive; a
/// tailed layer's streak behind each, drawn while it outruns the bench's
/// 0.2 yd/s.
#[test]
fn every_starburst_particle_flies_as_the_bench_draws_it_at_any_frame_rate() {
    for scale in [StarburstScale::High, StarburstScale::Low] {
        let b = bench(scale);
        let births: Vec<Vec<f32>> = b
            .layers
            .iter()
            .map(|l| bench_births(l.rate, l.win))
            .collect();
        let counts: Vec<usize> = births.iter().map(Vec::len).collect();
        assert_eq!(
            counts,
            match scale {
                StarburstScale::High => vec![33],
                StarburstScale::Low => vec![15, 22],
            }
        );
        for dt in [0.008, 0.016, 0.033] {
            let mut h = Harness::with_tick(dt);
            let root = Vec3::new(-4.0, COMBATANT_Y, 2.0);
            let origin = chest(root, false);
            let unit = h.unit(2, 0, Warrior, root);
            h.burst(unit, scale);
            let mut frames = 0u32;
            let mut seen: BTreeMap<(usize, u32), (Vec3, f32, f32)> = BTreeMap::new();
            let mut streaks = 0;
            loop {
                h.tick(1);
                frames += 1;
                let Some(&(rig, _, _, age)) = h.bursts().first() else {
                    break;
                };
                assert!(close(age, frames as f32 * dt, 1e-4));
                let drawn = h.drawn::<StarburstPiece>(rig);
                for d in &drawn {
                    let StarburstPiece::Particle { layer, j, born, .. } = d.piece else {
                        continue;
                    };
                    let l = &b.layers[layer];
                    assert!(close(born, births[layer][j as usize], 1e-3), "{layer}/{j}");
                    let page = age - born;
                    if page.abs() < 1e-4 || (page - l.life).abs() < 1e-4 {
                        continue;
                    }
                    assert_eq!(d.visible, (0.0..l.life).contains(&page));
                    if !d.visible {
                        continue;
                    }
                    let t = page / l.life;
                    assert!(close(d.scale.x, 2.0 * keys(&l.scale, t), 1e-4));
                    assert!(close(d.alpha, 1.0 - t, 2e-3));
                    assert!(rgb_near(d.rgb, keys3(&l.rgb, t), 1.01));
                    assert_eq!(d.mode, AlphaMode::Add);
                    let roll = roll_of(Quat::IDENTITY, d.rotation);
                    // Out of the chest: its launch, from this sighting.
                    let launch = (d.at - origin + Vec3::Y * (0.5 * l.gravity * page * page)) / page;
                    assert!(
                        close(launch.length(), l.speed, 2e-3),
                        "{scale:?} {layer}/{j}"
                    );
                    match seen.get(&(layer, j)) {
                        None => {
                            seen.insert((layer, j), (launch, roll, page));
                        }
                        Some(&(v0, r0, _)) => {
                            assert!((launch - v0).length() < 2e-3, "a straight fall");
                            assert!(close(roll, r0, 1e-4), "no spin");
                        }
                    }
                    if l.tail > 0.0 {
                        let streak = drawn.iter().find(|s| {
                            matches!(s.piece, StarburstPiece::Streak { layer: sl, j: sj, .. }
                                if sl == layer && sj == j)
                        });
                        let streak = streak.expect("a tailed particle's streak");
                        assert!(streak.visible, "speed {} > 0.2", l.speed);
                        let len = (l.tail * 0.12).min(l.speed * 0.06);
                        assert!(close(streak.scale.x, len, 1e-4));
                        assert!(close(streak.scale.y, keys(&l.scale, t) * 0.7, 1e-4));
                        assert!(close(streak.alpha, (1.0 - t) * 0.85, 2e-3));
                        streaks += 1;
                    }
                }
            }
            let want: BTreeSet<(usize, u32)> = births
                .iter()
                .enumerate()
                .flat_map(|(l, bs)| (0..bs.len() as u32).map(move |j| (l, j)))
                .collect();
            assert_eq!(
                seen.keys().copied().collect::<BTreeSet<_>>(),
                want,
                "{scale:?} at {dt}"
            );
            assert_eq!(streaks > 0, scale == StarburstScale::Low);
            // The directions spread over the sphere.
            let octants: BTreeSet<(bool, bool, bool)> = seen
                .values()
                .map(|(v, ..)| (v.x > 0.0, v.y > 0.0, v.z > 0.0))
                .collect();
            assert!(octants.len() >= 6, "{octants:?}");
        }
    }
}

/// A pet's starburst is at its stature: centred at its chest, its flash and
/// every particle sized, flung and dropped at the pet's scale.
#[test]
fn a_pets_starburst_is_at_its_stature() {
    let mut h = Harness::new();
    let root = Vec3::new(1.0, PET_SIM_Y, 1.0);
    let pet = h.pet(root);
    h.burst(pet, StarburstScale::High);
    h.tick(18);
    let (rig, _, _, age) = h.bursts()[0];
    let ms = age * 1000.0;
    let s = IMPACT_PET_STATURE;
    for d in h.drawn::<StarburstPiece>(rig).iter().filter(|d| d.visible) {
        match d.piece {
            StarburstPiece::Flash => {
                assert!((d.at - chest(root, true)).length() < 1e-4);
                assert!(close(d.scale.x, keys(HIGH.flash_size, ms) * s, 1e-4));
            }
            StarburstPiece::Particle { born, .. } => {
                let page = age - born;
                let l = &HIGH.layers[0];
                let launch = (d.at - chest(root, true)
                    + Vec3::Y * (0.5 * l.gravity * s * page * page))
                    / page;
                assert!(close(launch.length(), l.speed * s, 2e-3));
                assert!(close(
                    d.scale.x,
                    2.0 * keys(&l.scale, page / l.life) * s,
                    1e-4
                ));
            }
            StarburstPiece::Streak { .. } => unreachable!("the high scale has no tail"),
        }
    }
}

/// A streak is the bench's screen line: in the camera's plane at its
/// particle's depth, starting at the particle and ending where the true
/// segment behind its flight projects. Checked against the same burst played
/// with no camera, which draws every particle where it truly is.
#[test]
fn a_streak_is_its_flight_as_the_camera_sees_it() {
    let root = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let cam = match_camera(root, 60.0, 38.7f32.to_radians(), 0.9);
    let play = |camera: Option<Transform>| {
        let mut h = Harness::new();
        let unit = h.unit(2, 0, Warrior, root);
        match camera {
            Some(t) => h.camera(t),
            None => {
                h.world()
                    .spawn((Transform::default(), GlobalTransform::default()));
            }
        }
        h.burst(unit, StarburstScale::Low);
        h
    };
    let (mut seen, mut truth) = (play(Some(cam)), play(None));
    let (eye, f) = (cam.translation, cam.forward().as_vec3());
    let (right, up) = (cam.right().as_vec3(), cam.up().as_vec3());
    let screen = |p: Vec3| {
        let d = p - eye;
        Vec2::new(d.dot(right), d.dot(up)) / d.dot(f)
    };
    let l = &LOW.layers[1];
    let len = (l.tail * 0.12).min(l.speed * 0.06);
    let mut checked = 0;
    for _ in 0..20 {
        seen.tick(1);
        truth.tick(1);
        let (rig, real_rig) = (seen.bursts()[0].0, truth.bursts()[0].0);
        let drawn = seen.drawn::<StarburstPiece>(rig);
        let real = truth.drawn::<StarburstPiece>(real_rig);
        let particle = |set: &[Drawn<StarburstPiece>], j: u32| {
            set.iter()
                .find(|p| {
                    matches!(p.piece, StarburstPiece::Particle { layer: 1, j: pj, .. } if pj == j)
                })
                .map(|p| p.at)
                .unwrap()
        };
        for d in drawn.iter().filter(|d| d.visible) {
            let StarburstPiece::Streak { j, .. } = d.piece else {
                continue;
            };
            let (at, true_at) = (particle(&drawn, j), particle(&real, j));
            assert!(
                (screen(at) - screen(true_at)).length() < 1e-5,
                "held on its ray"
            );
            assert!(((d.at - at).dot(f)).abs() < 1e-4, "in the camera's plane");
            assert!((d.rotation * Vec3::Z - cam.rotation * Vec3::Z).length() < 1e-4);
            let true_far = true_at - (true_at - chest(root, false)).normalize() * len;
            let ends = [
                d.at + d.rotation * Vec3::X * (d.scale.x / 2.0),
                d.at - d.rotation * Vec3::X * (d.scale.x / 2.0),
            ];
            let near = ends
                .iter()
                .map(|e| (screen(*e) - screen(at)).length())
                .fold(f32::MAX, f32::min);
            let far = ends
                .iter()
                .map(|e| (screen(*e) - screen(true_far)).length())
                .fold(f32::MAX, f32::min);
            assert!(near < 1e-5, "it starts at the particle: {near}");
            assert!(far < 5e-5, "it ends where the flight projects: {far}");
            checked += 1;
        }
    }
    assert!(checked > 50, "{checked}");
}

/// Every sprite faces the camera — its Z is the camera's back, its X square
/// to the view — at its exact roll; every level piece lies flat. At the match
/// camera and at a low, near one.
#[test]
fn every_sprite_faces_the_camera_at_its_roll() {
    for cam_of in [
        |t: Vec3| match_camera(t, 60.0, 38.7f32.to_radians(), 0.4),
        |t: Vec3| match_camera(t, 15.0, 75f32.to_radians(), 2.2),
    ] {
        let root = Vec3::new(0.0, COMBATANT_Y, 0.0);
        let cam = cam_of(root);
        let mut h = Harness::new();
        let priest = h.priest(root, None);
        let enemy = h.unit(2, 0, Warrior, Vec3::new(2.0, COMBATANT_Y, 0.0));
        h.camera(cam);
        h.nova(priest);
        h.fall(priest);
        h.burst(enemy, StarburstScale::Low);
        let back = cam.rotation * Vec3::Z;
        let fwd = cam.forward().as_vec3();
        let (mut sprites, mut level) = (0, 0);
        for _ in 0..20 {
            h.tick(1);
            let face = |rot: Quat| {
                assert!((rot * Vec3::Z - back).length() < 1e-4, "faces the camera");
                assert!(
                    (rot * Vec3::X).dot(fwd).abs() < 1e-4,
                    "square to the screen"
                );
            };
            let dome = h.domes()[0].0;
            for d in h.drawn::<DomePiece>(dome).iter().filter(|d| d.visible) {
                match d.piece {
                    DomePiece::Star { .. } => {
                        face(d.rotation);
                        sprites += 1;
                    }
                    _ => {
                        assert!((d.rotation * Vec3::Z - Vec3::Y).length() < 1e-4);
                        level += 1;
                    }
                }
            }
            let fall = h.falls()[0].0;
            for d in h.drawn::<FallPiece>(fall).iter().filter(|d| d.visible) {
                match d.piece {
                    FallPiece::Star(_) => {
                        face(d.rotation);
                        assert!(close(roll_of(cam.rotation, d.rotation), 0.0, 1e-4));
                        sprites += 1;
                    }
                    FallPiece::Glow => {
                        assert!((d.rotation * Vec3::Z - Vec3::Y).length() < 1e-4);
                        level += 1;
                    }
                }
            }
            let burst = h.bursts()[0].0;
            for d in h
                .drawn::<StarburstPiece>(burst)
                .iter()
                .filter(|d| d.visible)
            {
                face(d.rotation);
                sprites += 1;
            }
        }
        assert!(sprites > 500 && level > 500, "{sprites} {level}");
    }
}

/// The same rig's stars at the match camera and with none: a star's roll
/// about the camera is the roll it has with no camera — the bench's `ang`,
/// exactly — so the roll is no accident of one view.
#[test]
fn a_stars_roll_is_its_own_under_any_camera() {
    let root = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let cam = match_camera(root, 60.0, 38.7f32.to_radians(), 1.3);
    let run = |camera: Option<Transform>| {
        let mut h = Harness::new();
        let priest = h.priest(root, None);
        match camera {
            Some(t) => h.camera(t),
            None => {
                h.world()
                    .spawn((Transform::default(), GlobalTransform::default()));
            }
        }
        h.nova(priest);
        h.tick(25);
        let dome = h.domes()[0].0;
        h.drawn::<DomePiece>(dome)
            .into_iter()
            .filter(|d| d.visible && matches!(d.piece, DomePiece::Star { .. }))
            .map(|d| match d.piece {
                DomePiece::Star { j, .. } => (j, d.rotation),
                _ => unreachable!(),
            })
            .collect::<BTreeMap<u32, Quat>>()
    };
    let seen = run(Some(cam));
    let bare = run(None);
    assert_eq!(
        seen.keys().collect::<Vec<_>>(),
        bare.keys().collect::<Vec<_>>()
    );
    assert!(seen.len() > 50);
    let rolls: BTreeSet<i32> = seen
        .iter()
        .map(|(j, r)| {
            let a = roll_of(cam.rotation, *r);
            let b = roll_of(Quat::IDENTITY, bare[j]);
            let diff = (a - b).rem_euclid(TAU);
            assert!(diff < 1e-3 || diff > TAU - 1e-3, "star {j}: {a} vs {b}");
            (a * 10.0) as i32
        })
        .collect();
    assert!(rolls.len() > 20, "the rolls spread: {rolls:?}");
}

// ── Held in front of the moving body ─────────────────────────────────────────

const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_SEGMENT: f32 = 0.75;
const PET_BODY_BOUND: f32 = 0.35 + 0.3;
const CLEARANCE: f32 = 0.05;

fn add_body(app: &mut App, unit: Entity, rest_y: f32) -> Entity {
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y }, Transform::from_xyz(0.0, rest_y, 0.0)))
        .id();
    app.world_mut().entity_mut(unit).add_child(body);
    body
}

/// The run at real-play speed (about 7 yd/s) with the gait's 0.1 yd bob,
/// and from frame 20 a Cyclone's 1.5 yd lift.
fn run_and_lift(frame: u32, dt: f32) -> (Vec3, f32) {
    let t = frame as f32 * dt;
    (
        Vec3::new(7.0 * t, 0.0, 2.0 * (t * 3.0).sin()),
        0.1 * (t * 9.0).sin() + if frame >= 20 { 1.5 } else { 0.0 },
    )
}

/// Which effect a hold probe plays.
#[derive(Clone, Copy, Debug)]
enum Rig {
    Dome,
    Fall,
    Burst(bool),
}

/// Every drawn piece of one rig on a running, bobbing, lifted unit, frame by
/// frame: `(entity, world position, scale)`, and the unit's drawn root each
/// frame.
fn held_frames(rig: Rig, camera: Option<Transform>) -> (Vec<Vec<(Entity, Vec3, Vec3)>>, Vec<Vec3>) {
    let mut h = Harness::new();
    let pet = matches!(rig, Rig::Burst(true));
    let start = Vec3::new(-6.0, if pet { PET_SIM_Y } else { COMBATANT_Y }, 1.0);
    let unit = match rig {
        Rig::Dome => h.priest(start, None),
        Rig::Fall | Rig::Burst(false) => h.unit(1, 1, Warrior, start),
        Rig::Burst(true) => h.pet(start),
    };
    let rest = if pet { PET_BODY_DROP } else { 0.0 };
    let body = add_body(&mut h.app, unit, rest);
    match camera {
        Some(t) => h.camera(t),
        None => {
            h.world()
                .spawn((Transform::default(), GlobalTransform::default()));
        }
    }
    match rig {
        Rig::Dome => h.nova(unit),
        Rig::Fall => h.fall(unit),
        Rig::Burst(_) => h.burst(unit, StarburstScale::Low),
    }
    let dt = h.dt;
    let mut frames = Vec::new();
    let mut roots = Vec::new();
    for frame in 0..45 {
        let (run, lift) = run_and_lift(frame, dt);
        h.world().get_mut::<Transform>(unit).unwrap().translation = start + run;
        h.world().get_mut::<Transform>(body).unwrap().translation.y = rest + lift;
        h.tick(1);
        roots.push(start + run + Vec3::Y * lift);
        let world = h.app.world_mut();
        let mut q = world.query_filtered::<(
            Entity,
            &GlobalTransform,
            &Visibility,
            Option<&StarburstPiece>,
        ), Or<(With<DomePiece>, With<FallPiece>, With<StarburstPiece>)>>();
        frames.push(
            q.iter(world)
                .filter(|(_, _, v, _)| **v != Visibility::Hidden)
                // A streak is a screen line, not a held quad: its own test.
                .filter(|(.., p)| !matches!(p, Some(StarburstPiece::Streak { .. })))
                .map(|(e, g, ..)| {
                    let (s, _, t) = g.to_scale_rotation_translation();
                    (e, t, s)
                })
                .collect(),
        );
    }
    (frames, roots)
}

/// The hold, against the MOVING drawn body: every piece of every rig, each
/// frame, drawn on its own view ray at the same screen size as the piece
/// drawn with no camera, and in front of the body THIS frame — a piece that
/// would be behind it stands exactly the clearance in front of its nearest
/// point, so a hold measured from last frame's body fails. At the match
/// camera, a low pitch and a near zoom; on a combatant and, for the
/// starburst, a pet.
#[test]
fn every_piece_is_held_in_front_of_the_moving_drawn_body() {
    let cams: [(f32, f32, f32); 3] = [(60.0, 38.7, 0.3), (40.0, 75.0, 2.0), (12.0, 38.7, 4.0)];
    for rig in [Rig::Dome, Rig::Fall, Rig::Burst(false), Rig::Burst(true)] {
        let pet = matches!(rig, Rig::Burst(true));
        let mut kept_anywhere = 0;
        for (zoom, pitch, yaw) in cams {
            let target = Vec3::new(-2.0, COMBATANT_Y, 1.0);
            let cam = match_camera(target, zoom, pitch.to_radians(), yaw);
            let (seen, roots) = held_frames(rig, Some(cam));
            let (truth, _) = held_frames(rig, None);
            let (eye, fwd) = (cam.translation, cam.forward().as_vec3());
            let depth = |p: Vec3| (p - eye).dot(fwd);
            let (mut held, mut kept) = (0, 0);
            for ((drawn, real), root) in seen.iter().zip(&truth).zip(&roots) {
                let (top, bottom, radius) = if pet {
                    let c = *root + Vec3::Y * PET_BODY_DROP;
                    (c, c, PET_BODY_BOUND)
                } else {
                    (
                        *root + Vec3::Y * BODY_HALF_SEGMENT,
                        *root - Vec3::Y * BODY_HALF_SEGMENT,
                        BODY_RADIUS,
                    )
                };
                let nearest = depth(top).min(depth(bottom)) - radius - CLEARANCE;
                let real: BTreeMap<Entity, (Vec3, Vec3)> =
                    real.iter().map(|(e, t, s)| (*e, (*t, *s))).collect();
                for (e, d_at, d_scale) in drawn {
                    let Some(&(r_at, r_scale)) = real.get(e) else {
                        panic!("{rig:?}: a piece drawn only under the camera");
                    };
                    if depth(r_at) <= nearest {
                        assert!((*d_at - r_at).length() < 1e-4, "{rig:?}: in front, moved");
                        kept += 1;
                    } else {
                        let k = depth(*d_at) / depth(r_at);
                        assert!(
                            close(depth(*d_at), nearest, 2e-3),
                            "{rig:?} {zoom}/{pitch}: held at {} not {nearest}",
                            depth(*d_at)
                        );
                        assert!(
                            (*d_at - (eye + (r_at - eye) * k)).length() < 2e-3,
                            "{rig:?}: off its view ray"
                        );
                        // Its width and height; z is the quad's normal.
                        assert!(
                            (d_scale.truncate() - r_scale.truncate() * k).length()
                                < 2e-3 * r_scale.length().max(1.0),
                            "{rig:?}: keeps its screen size"
                        );
                        held += 1;
                    }
                }
            }
            assert!(held > 20, "{rig:?} {zoom}/{pitch}: {held} held");
            kept_anywhere += kept;
        }
        // Non-vacuity of the other branch: pieces already in front stay put.
        assert!(kept_anywhere > 5, "{rig:?}: {kept_anywhere} kept");
    }
}

/// The burst and the fall ride the DRAWN body: the starburst's rig stands at
/// the drawn chest and the fall's at the drawn feet while the unit runs,
/// bobs and is lifted; the dome stands on the Priest's own floor.
#[test]
fn the_rigs_ride_the_drawn_body() {
    for rig in [Rig::Burst(false), Rig::Burst(true), Rig::Fall, Rig::Dome] {
        let mut h = Harness::new();
        let pet = matches!(rig, Rig::Burst(true));
        let start = Vec3::new(1.0, if pet { PET_SIM_Y } else { COMBATANT_Y }, -2.0);
        let unit = match rig {
            Rig::Dome => h.priest(start, None),
            Rig::Fall | Rig::Burst(false) => h.unit(1, 1, Warrior, start),
            Rig::Burst(true) => h.pet(start),
        };
        let rest = if pet { PET_BODY_DROP } else { 0.0 };
        let body = add_body(&mut h.app, unit, rest);
        match rig {
            Rig::Dome => h.nova(unit),
            Rig::Fall => h.fall(unit),
            Rig::Burst(_) => h.burst(unit, StarburstScale::High),
        }
        let dt = h.dt;
        for frame in 0..40 {
            let (run, lift) = run_and_lift(frame, dt);
            let root = start + run;
            h.world().get_mut::<Transform>(unit).unwrap().translation = root;
            h.world().get_mut::<Transform>(body).unwrap().translation.y = rest + lift;
            h.tick(1);
            let world = h.app.world_mut();
            let mut q = world.query_filtered::<&GlobalTransform, Or<(
                With<HolyNovaDome>,
                With<HolyStarFall>,
                With<HolyStarburst>,
            )>>();
            let at: Vec<Vec3> = q.iter(world).map(|g| g.translation()).collect();
            assert_eq!(at.len(), 1, "{rig:?}");
            let want = match rig {
                Rig::Dome => floor_under(root),
                Rig::Fall => floor_under(root) + Vec3::Y * lift,
                Rig::Burst(p) => chest(root + Vec3::Y * lift, p),
            };
            assert!(
                (at[0] - want).length() < 1e-4,
                "{rig:?} frame {frame}: {} vs {want}",
                at[0]
            );
        }
    }
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

/// Each rig's whole life, as frames × dt at 8, 16 and 33 ms: the dome until
/// its last star (born 767 ms in at the latest) has lived its second, the
/// fall 2.65 s, the starbursts until their last particle is spent.
#[test]
fn each_rig_lives_its_life_at_any_frame_rate() {
    let star_births = bench_births(&stars::RATE.map(|(t, r)| (t * 1000.0, r)), (0.0, DOME_MS));
    let burst_life = |b: &BenchBurst| {
        let flash = b.flash_a[b.flash_a.len() - 1].0 / 1000.0;
        b.layers
            .iter()
            .map(|l| bench_births(l.rate, l.win).last().unwrap() + l.life)
            .fold(flash, f32::max)
    };
    let lives = [
        star_births.last().unwrap() + stars::LIFE,
        (fall::DELAY_MS + fall::SPAN_MS) / 1000.0,
        burst_life(&HIGH),
        burst_life(&LOW),
    ];
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let priest = h.priest(Vec3::new(0.0, COMBATANT_Y, 0.0), None);
        let enemy = h.unit(2, 0, Warrior, Vec3::new(3.0, COMBATANT_Y, 0.0));
        let other = h.unit(2, 1, Warrior, Vec3::new(-3.0, COMBATANT_Y, 0.0));
        h.nova(priest);
        h.fall(priest);
        h.burst(enemy, StarburstScale::High);
        h.burst(other, StarburstScale::Low);
        let mut last = [0u32; 4];
        for frame in 1..400 {
            h.tick(1);
            let present = [
                !h.domes().is_empty(),
                !h.falls().is_empty(),
                h.bursts().iter().any(|b| b.2 == StarburstScale::High),
                h.bursts().iter().any(|b| b.2 == StarburstScale::Low),
            ];
            for (i, p) in present.iter().enumerate() {
                if *p {
                    last[i] = frame;
                }
            }
        }
        for (i, life) in lives.iter().enumerate() {
            assert!(
                close(last[i] as f32 * dt, *life, dt + 2e-3),
                "rig {i} at {dt}: {} frames vs {life} s",
                last[i]
            );
        }
    }
}

/// With other auras on everyone: a unit that dies under its rig keeps it to
/// the end of its life; a polymorphed or stealthed one keeps it too; a unit
/// that is gone takes its rig — and every piece of it — that frame; and a
/// match that ends mid-effect leaves nothing. Every rig is a
/// `PlayMatchEntity`.
#[test]
fn the_rigs_play_through_death_sheep_and_stealth_and_go_with_their_unit_or_the_match() {
    let other_auras = || ActiveAuras {
        auras: vec![
            fortitude(),
            Aura {
                effect_type: AuraType::HealingOverTime,
                duration: 12.0,
                magnitude: 5.0,
                ability_name: "Renew".into(),
                ..Default::default()
            },
            Aura {
                effect_type: AuraType::MovementSpeedSlow,
                duration: 6.0,
                magnitude: 0.5,
                ability_name: "Hamstring".into(),
                ..Default::default()
            },
        ],
    };
    // (what happens, keeps the rig)
    for (what, keeps) in [
        ("dies", true),
        ("is polymorphed", true),
        ("stealths", true),
        ("is gone", false),
    ] {
        let mut h = Harness::new();
        let priest = h.priest(Vec3::new(0.0, COMBATANT_Y, 0.0), None);
        let enemy = h.unit(2, 0, Warrior, Vec3::new(3.0, COMBATANT_Y, 0.0));
        for e in [priest, enemy] {
            h.world().entity_mut(e).insert(other_auras());
        }
        h.nova(priest);
        h.fall(priest);
        h.burst(enemy, StarburstScale::Low);
        h.tick(3);
        let rigs: Vec<Entity> = h
            .domes()
            .iter()
            .map(|d| d.0)
            .chain(h.falls().iter().map(|f| f.0))
            .chain(h.bursts().iter().map(|b| b.0))
            .collect();
        assert_eq!(rigs.len(), 3);
        for rig in &rigs {
            assert!(h.app.world().get::<PlayMatchEntity>(*rig).is_some());
        }
        let pieces = |h: &mut Harness| {
            h.count::<DomePiece>() + h.count::<FallPiece>() + h.count::<StarburstPiece>()
        };
        let all_pieces = pieces(&mut h);
        assert_eq!(all_pieces, (9 * 48 + 1 + 191) + 13 + (1 + 15 + 22 * 2));
        for e in [priest, enemy] {
            match what {
                "dies" => {
                    let mut c = h.world().get_mut::<Combatant>(e).unwrap();
                    c.current_health = 0.0;
                    c.is_dead = true;
                }
                "is polymorphed" => {
                    h.world().entity_mut(e).insert(PolymorphedVisual);
                }
                "stealths" => h.world().get_mut::<Combatant>(e).unwrap().stealthed = true,
                _ => {
                    h.world().entity_mut(e).despawn();
                }
            }
        }
        h.tick(1);
        if keeps {
            assert_eq!(
                h.domes().len() + h.falls().len() + h.bursts().len(),
                3,
                "{what}"
            );
            assert_eq!(pieces(&mut h), all_pieces, "{what}");
            // ...and the burst still draws its flash.
            let burst = h.bursts()[0].0;
            assert!(
                h.drawn::<StarburstPiece>(burst).iter().any(|d| d.visible),
                "{what}"
            );
            // The match ends mid-effect: nothing is left.
            h.world().run_system_once(cleanup_play_match).unwrap();
        }
        assert_eq!(
            h.domes().len() + h.falls().len() + h.bursts().len(),
            0,
            "{what}"
        );
        assert_eq!(pieces(&mut h), 0, "{what}: no orphaned piece");
    }
}

/// A landing on a unit already gone draws nothing and consumes its marker.
#[test]
fn a_landing_on_a_unit_already_gone_draws_nothing() {
    let mut h = Harness::new();
    let gone = h.unit(2, 0, Warrior, Vec3::ZERO.with_y(COMBATANT_Y));
    h.world().entity_mut(gone).despawn();
    h.burst(gone, StarburstScale::Low);
    h.fall(gone);
    h.nova(gone);
    h.tick(2);
    assert_eq!(h.domes().len() + h.falls().len() + h.bursts().len(), 0);
    assert_eq!(
        h.count::<StarburstLanding>() + h.count::<StarFallLanding>(),
        0
    );
    assert_eq!(h.count::<InstantAbilityFired>(), 0);
}

/// The particles do not ride their unit, as the bench's emitters do not
/// (`follow` unset): on a unit running 7 yd/s, each starburst particle flies
/// from where the chest stood on its birth frame, and each trailing star from
/// the floor under the Priest then — not from where the unit is now.
#[test]
fn particles_fly_from_where_they_were_born_not_with_their_unit() {
    for rig in [Rig::Burst(false), Rig::Dome] {
        let mut h = Harness::new();
        let start = Vec3::new(-4.0, COMBATANT_Y, 0.0);
        let unit = match rig {
            Rig::Dome => h.priest(start, None),
            _ => h.unit(2, 0, Warrior, start),
        };
        match rig {
            Rig::Dome => h.nova(unit),
            _ => h.burst(unit, StarburstScale::Low),
        }
        let dt = h.dt;
        // Per particle: the unit's anchor on its first frame, its first
        // sighting (position, page).
        let mut first: BTreeMap<(usize, u32), (Vec3, Vec3, f32)> = BTreeMap::new();
        let mut checked = 0;
        for frame in 0..40 {
            let root = start + Vec3::X * (7.0 * frame as f32 * dt);
            h.world().get_mut::<Transform>(unit).unwrap().translation = root;
            h.tick(1);
            let anchor = match rig {
                Rig::Dome => floor_under(root),
                _ => chest(root, false),
            };
            let pieces: Vec<((usize, u32), f32, f32, Vec3)> = match rig {
                Rig::Dome => {
                    let Some(&(r, _, _, age)) = h.domes().first() else {
                        break;
                    };
                    h.drawn::<DomePiece>(r)
                        .into_iter()
                        .filter(|d| d.visible)
                        .filter_map(|d| match d.piece {
                            DomePiece::Star { j, born, .. } => Some(((0, j), born, age, d.at)),
                            _ => None,
                        })
                        .collect()
                }
                _ => {
                    let Some(&(r, _, _, age)) = h.bursts().first() else {
                        break;
                    };
                    h.drawn::<StarburstPiece>(r)
                        .into_iter()
                        .filter(|d| d.visible)
                        .filter_map(|d| match d.piece {
                            StarburstPiece::Particle { layer, j, born, .. } => {
                                Some(((layer, j), born, age, d.at))
                            }
                            _ => None,
                        })
                        .collect()
                }
            };
            for (key, born, age, at) in pieces {
                let page = age - born;
                let gravity = match (rig, key.0) {
                    (Rig::Dome, _) | (_, 1) => 0.0,
                    _ => 6.9,
                };
                let flown = at + Vec3::Y * (0.5 * gravity * page * page);
                match first.get(&key) {
                    None => {
                        first.insert(key, (anchor, flown, page));
                    }
                    Some(&(born_anchor, f0, p0)) if page - p0 > 0.05 => {
                        // A straight line out of a fixed point: back along
                        // it to page 0 lands on the birth frame's anchor.
                        let v = (flown - f0) / (page - p0);
                        let origin = f0 - v * p0;
                        let off = origin - born_anchor;
                        match rig {
                            // A star is born in a square round the floor it
                            // left, and flies at its own speed alone: one
                            // that rode the Priest would carry its 7 yd/s.
                            Rig::Dome => {
                                assert!(
                                    off.x.abs() <= stars::AREA * NOVA_SPREAD / 2.0 + 1e-2
                                        && off.z.abs() <= stars::AREA * NOVA_SPREAD / 2.0 + 1e-2
                                        && (off.y - stars::LIFT).abs() < 0.33,
                                    "star {key:?} born {off} off the floor it left"
                                );
                                assert!(
                                    close(v.length(), stars::SPEED * NOVA_SPREAD, 1e-2),
                                    "star {key:?} flies at {}",
                                    v.length()
                                );
                            }
                            _ => assert!(off.length() < 2e-3, "{key:?}: {off}"),
                        }
                        checked += 1;
                    }
                    Some(_) => {}
                }
            }
        }
        assert!(checked > 100, "{rig:?}: {checked}");
    }
}
