//! Probes for Shadow Dance's state (`rendering/effects/shadow_dance.rs`,
//! AS-368): a dark-red bloom, red shadow ribbons drifting out and black shadow
//! drawn in, on the Rogue's chest for the whole window, thinning and
//! flickering over its last 1.5 s.
//!
//! These drive the graphical chain off `ActiveAuras`, as the sim leaves it,
//! with the aura the Rogue AI lands (through the engine's own constructor),
//! and assert what is DRAWN, in world space through `GlobalTransform` and the
//! spawned materials: every particle, every frame of a whole window, against
//! an independent transcription of the signed-off bench's three emitters and
//! its `rateMul` ending; the particles' flight; the sprites read back and each
//! layer's blend; camera facing; the anchor on the rendered chest; draw order
//! at the match camera; and the lifecycle — run out, removed early, refreshed,
//! its Rogue dead or gone, a sheep, and stealth on and off — always with other
//! auras on the Rogue.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. The systems run in the order `states/mod.rs` registers them.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy::render::render_phase::ViewRangefinder3d;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, CameraController, Combatant, PlayMatchEntity,
    PolymorphedVisual, VisualBody,
};
use arenasim::states::play_match::{
    place_shadow_dance, update_shadow_dance, update_stealth_visuals, DanceLayer, ShadowDancePiece,
    ShadowDanceRig,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;
/// The Dance's aura as the probes land it: 5 ms short of the RON's 8 s, as
/// if it had landed mid-frame, so no frame falls on a 120 ms step's edge
/// (`(8 − DUR0)` plus a whole number of 16 ms frames is never a multiple of
/// 120 ms) and the two clocks below cannot round to different steps.
const DUR0: f32 = 7.995;
/// The chest: 0.45 yd above the body's centre (1.45 above the floor).
const CHEST: f32 = 0.45;

struct Harness {
    app: App,
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
        app.add_systems(Update, (update_stealth_visuals, update_shadow_dance));
        app.add_systems(
            PostUpdate,
            place_shadow_dance.before(TransformSystem::TransformPropagate),
        );
        Harness { app }
    }

    /// A Rogue as `spawn_combatant` builds it: a `VisualBody` child wearing
    /// its own body material (Rogue yellow), and two other buffs that outlast
    /// any Dance.
    fn rogue(&mut self, at: Vec3) -> (Entity, Entity) {
        let world = self.app.world_mut();
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgb_u8(255, 245, 105),
                ..default()
            });
        let unit = world
            .spawn((
                Combatant::new(1, 0, CharacterClass::Rogue),
                Transform::from_translation(at),
                ActiveAuras {
                    auras: other_auras(),
                },
            ))
            .id();
        let body = world
            .spawn((
                VisualBody { rest_y: 0.0 },
                Transform::default(),
                MeshMaterial3d(material),
            ))
            .id();
        world.entity_mut(unit).add_child(body);
        (unit, body)
    }

    fn camera(&mut self, transform: Transform) -> Entity {
        self.app
            .world_mut()
            .spawn((Camera3d::default(), transform))
            .id()
    }

    fn auras(&mut self, unit: Entity) -> Mut<'_, Vec<Aura>> {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .map_unchanged(|a| &mut a.auras)
    }

    /// The Dance lands.
    fn dance(&mut self, unit: Entity) {
        self.auras(unit).push(dance_aura());
    }

    fn combatant(&mut self, unit: Entity) -> Mut<'_, Combatant> {
        self.app.world_mut().get_mut::<Combatant>(unit).unwrap()
    }

    /// One frame, the auras running down as the sim runs them and dropped
    /// when spent.
    fn step(&mut self) {
        let mut q = self.app.world_mut().query::<&mut ActiveAuras>();
        for mut auras in q.iter_mut(self.app.world_mut()) {
            for aura in &mut auras.auras {
                aura.duration -= DT;
            }
            auras.auras.retain(|a| a.duration > 0.0);
        }
        self.app.update();
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.step();
        }
    }

    /// Each rig: its entity, its Rogue, its age.
    fn rigs(&mut self) -> Vec<(Entity, Entity, f32)> {
        let mut q = self.app.world_mut().query::<(Entity, &ShadowDanceRig)>();
        q.iter(self.app.world())
            .map(|(e, r)| (e, r.bearer, r.age))
            .collect()
    }

    fn rig_at(&mut self) -> Vec3 {
        let mut q = self
            .app
            .world_mut()
            .query::<(&ShadowDanceRig, &GlobalTransform)>();
        q.single(self.app.world()).unwrap().1.translation()
    }

    /// Every piece as drawn.
    fn pieces(&mut self) -> Vec<Drawn> {
        let mut q = self.app.world_mut().query::<(
            Entity,
            &ShadowDancePiece,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            &Mesh3d,
            Has<NotShadowCaster>,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(e, p, g, m, mesh, no_shadow)| {
                (e, p.layer, *g, m.0.clone(), mesh.0.clone(), no_shadow)
            })
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(entity, layer, g, m, mesh, no_shadow)| {
                let material = materials.get(&m).unwrap().clone();
                let c = material.base_color.to_srgba();
                Drawn {
                    entity,
                    layer,
                    at: g.translation(),
                    rotation: g.rotation(),
                    scale: g.scale(),
                    rgb: [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8),
                    alpha: c.alpha,
                    material_id: m.id(),
                    material,
                    mesh,
                    no_shadow,
                }
            })
            .collect()
    }

    fn lit(&mut self) -> Vec<Drawn> {
        self.pieces()
            .into_iter()
            .filter(|d| d.alpha > 0.0)
            .collect()
    }

    fn count<T: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&T>();
        q.iter(self.app.world()).count()
    }

    fn remaining(&mut self, unit: Entity) -> Option<f32> {
        self.auras(unit)
            .iter()
            .find(|a| a.effect_type == AuraType::ShadowDance)
            .map(|a| a.duration)
    }

    fn body_color(&mut self, body: Entity) -> Color {
        let handle = self
            .app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(body)
            .unwrap()
            .0
            .clone();
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap()
            .base_color
    }
}

#[derive(Clone)]
struct Drawn {
    entity: Entity,
    layer: DanceLayer,
    at: Vec3,
    rotation: Quat,
    scale: Vec3,
    rgb: [u8; 3],
    alpha: f32,
    material_id: AssetId<StandardMaterial>,
    material: StandardMaterial,
    mesh: Handle<Mesh>,
    no_shadow: bool,
}

/// Shadow Dance's aura as the Rogue AI lands it (`try_shadow_dance`), through
/// the engine's own constructor, set to [`DUR0`].
fn dance_aura() -> Aura {
    let defs = AbilityDefinitions::default();
    let def = defs.get(&AbilityType::ShadowDance).expect("Shadow Dance");
    let rogue = Entity::from_raw(1);
    let mut aura = AuraPending::from_ability(rogue, rogue, def)
        .expect("Shadow Dance applies an aura")
        .aura;
    assert_eq!(aura.effect_type, AuraType::ShadowDance);
    assert_eq!(aura.duration, 8.0, "an 8 s window");
    aura.duration = DUR0;
    aura
}

/// The other auras a Rogue wears through its Dance: a buff and a weapon
/// poison, each outlasting any Dance.
fn other_auras() -> Vec<Aura> {
    vec![
        Aura {
            effect_type: AuraType::AttackPowerIncrease,
            duration: 600.0,
            magnitude: 50.0,
            ability_name: "Battle Shout".into(),
            ..Default::default()
        },
        Aura {
            effect_type: AuraType::WeaponPoison,
            duration: 600.0,
            magnitude: 1.0,
            ability_name: "Instant Poison".into(),
            ..Default::default()
        },
    ]
}

fn other_names() -> BTreeSet<String> {
    other_auras().into_iter().map(|a| a.ability_name).collect()
}

fn names(h: &mut Harness, unit: Entity) -> BTreeSet<String> {
    h.auras(unit)
        .iter()
        .map(|a| a.ability_name.clone())
        .collect()
}

// ── the bench, transcribed ──────────────────────────────────────────────────
//
// The "Shadow Dance" tab of `docs/design/benches/2026-10-17-rogue-talent-bench.html`
// (version 1792302646-d16f), written out from its JavaScript at its default
// knobs (size, spread, duration, density and opacity 1; state "held for the
// 8 s"; every layer on). Independent of the module: none of its constants are
// read here.

/// One emitter record: rate, life, and the colour, alpha and scale keys.
struct BenchEmitter {
    layer: DanceLayer,
    rate: f32,
    life: f32,
    color: [(f64, [f64; 3]); 3],
    alpha: [(f64, f64); 3],
    scale: [(f64, f64); 3],
}

const BENCH: [BenchEmitter; 3] = [
    // {id:'bloom', ... life:1.5, rate:3, blend:'alpha', color:[[0,[255,255,255]],
    // [0.5,[144,35,35]],[1,[43,5,5]]], alpha:[[0,0],[0.5,0.78],[1,0]],
    // scale:[[0,0.04],[0.5,0.43],[1,1.74]], kind:'glow'}
    BenchEmitter {
        layer: DanceLayer::Bloom,
        rate: 3.0,
        life: 1.5,
        color: [
            (0.0, [255.0, 255.0, 255.0]),
            (0.5, [144.0, 35.0, 35.0]),
            (1.0, [43.0, 5.0, 5.0]),
        ],
        alpha: [(0.0, 0.0), (0.5, 0.78), (1.0, 0.0)],
        scale: [(0.0, 0.04), (0.5, 0.43), (1.0, 1.74)],
    },
    // {id:'red', ... speed:0.56, life:1.2, rate:33, area:0.5, areaY:1.2,
    // color:[[0,[55,47,47]],[0.5,[126,44,44]],[1,[92,0,0]]],
    // alpha:[[0,0],[0.5,1],[1,0]], scale:[[0,0.18],[0.5,0.18],[1,0.24]],
    // kind:'smoke'}
    BenchEmitter {
        layer: DanceLayer::RedRibbons,
        rate: 33.0,
        life: 1.2,
        color: [
            (0.0, [55.0, 47.0, 47.0]),
            (0.5, [126.0, 44.0, 44.0]),
            (1.0, [92.0, 0.0, 0.0]),
        ],
        alpha: [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        scale: [(0.0, 0.18), (0.5, 0.18), (1.0, 0.24)],
    },
    // {id:'black', inward:1.3, inwardFlat:1.2, speed:-1.39, life:1.0, rate:33,
    // blend:'alpha', color:[[0,[117,47,47]],[0.5,[44,44,44]],[1,[0,0,0]]],
    // alpha:[[0,0],[0.5,1],[1,0]], scale:[[0,0.16],[0.5,0.36],[1,0.48]],
    // kind:'smoke'}
    BenchEmitter {
        layer: DanceLayer::BlackInward,
        rate: 33.0,
        life: 1.0,
        color: [
            (0.0, [117.0, 47.0, 47.0]),
            (0.5, [44.0, 44.0, 44.0]),
            (1.0, [0.0, 0.0, 0.0]),
        ],
        alpha: [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        scale: [(0.0, 0.16), (0.5, 0.36), (1.0, 0.48)],
    },
];

/// The bench's `keys` over scalars.
fn keys(k: &[(f64, f64)], t: f64) -> f64 {
    if t <= k[0].0 {
        return k[0].1;
    }
    for i in 1..k.len() {
        if t <= k[i].0 {
            let (a, b) = (k[i - 1], k[i]);
            let span = b.0 - a.0;
            let u = if span == 0.0 { 1.0 } else { (t - a.0) / span };
            return a.1 + (b.1 - a.1) * u;
        }
    }
    k[k.len() - 1].1
}

/// The bench's `keys` over colours, before `rgb()` truncates them.
fn keys3(k: &[(f64, [f64; 3])], t: f64) -> [f64; 3] {
    let channel = |c: usize| keys(&k.iter().map(|(t, v)| (*t, v[c])).collect::<Vec<_>>(), t);
    [channel(0), channel(1), channel(2)]
}

/// The bench's `rateMul` for the tab at `ms` into the window:
/// `D = 8000; u = ms − (D − 1500); u < 0 → 1; ms > D → 0;
/// else max(0, 1 − u/1500) × (floor(ms/120) % 3 ? 1 : 0.15)`.
fn bench_rate_mul(ms: f32) -> f32 {
    let d = 8000.0;
    let u = ms - (d - 1500.0);
    if u < 0.0 {
        return 1.0;
    }
    if ms > d {
        return 0.0;
    }
    let flicker = if ((ms / 120.0).floor() as i64) % 3 != 0 {
        1.0
    } else {
        0.15
    };
    (1.0 - u / 1500.0).max(0.0) * flicker
}

/// The bench's `Emitter.update`, ages only (the spawn's scatter is the
/// module's own hash, checked geometrically below): the rate (times
/// `rateMul`) accrues while the window is on, each whole particle owed is
/// born at age 0, then every particle ages a frame and the spent go.
#[derive(Default)]
struct BenchSim {
    acc: [f32; 3],
    ages: [Vec<f32>; 3],
}

impl BenchSim {
    fn step(&mut self, dt: f32, ms: f32) {
        for (i, e) in BENCH.iter().enumerate() {
            if ms < 8000.0 {
                self.acc[i] += e.rate * dt * bench_rate_mul(ms);
            }
            while self.acc[i] >= 1.0 {
                self.acc[i] -= 1.0;
                self.ages[i].push(0.0);
            }
            for age in &mut self.ages[i] {
                *age += dt;
            }
            self.ages[i].retain(|age| *age < e.life);
        }
    }
}

/// A live particle's look, as the bench draws it: its colour before `rgb()`
/// truncates it, its alpha, and its diameter (`sprite` draws `scale × size ×
/// 2` across).
#[derive(Clone, Copy, Debug)]
struct BenchLook {
    rgb: [f64; 3],
    alpha: f64,
    diameter: f64,
}

fn bench_look(e: &BenchEmitter, age: f32) -> BenchLook {
    let t = age as f64 / e.life as f64;
    BenchLook {
        rgb: keys3(&e.color, t),
        alpha: keys(&e.alpha, t),
        diameter: 2.0 * keys(&e.scale, t),
    }
}

/// A drawn channel against the bench's: exact where the bench's value sits
/// more than 0.02 from an integer (its truncation is unambiguous), within one
/// either way only near one.
fn channel_matches(drawn: u8, bench: f64) -> bool {
    let near = (bench - bench.round()).abs() <= 0.02;
    if near {
        (drawn as f64 - bench.floor()).abs() <= 1.0
    } else {
        drawn as f64 == bench.floor()
    }
}

/// The order both sides are compared in: diameter (to a thousandth), then
/// alpha — increasing with age for every layer (a ribbon holds its size
/// through its first half, while its alpha rises).
fn by_age(a: (f64, f64), b: (f64, f64)) -> std::cmp::Ordering {
    let key = |(d, al): (f64, f64)| ((d * 1000.0).round() as i64, al);
    key(a).partial_cmp(&key(b)).unwrap()
}

/// The chest point of a Rogue standing at `at` with its body at rest.
fn chest(at: Vec3) -> Vec3 {
    at + Vec3::Y * CHEST
}

// ── the probes ──────────────────────────────────────────────────────────────

/// A dancing Rogue wears one rig at its chest, carrying every emitter's pool:
/// 6 blooms, 44 ribbons and 38 drawn-in shadows (more than each can hold
/// alive: 3/s × 1.5 s, 33/s × 1.2 s, 33/s × 1.0 s).
#[test]
fn a_dancing_rogue_wears_one_state_at_its_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, 1.0, -2.0);
    let (rogue, _) = h.rogue(at);
    h.tick(3);
    assert!(h.rigs().is_empty(), "nothing before the Dance");
    h.dance(rogue);
    h.tick(2);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert_eq!(rigs[0].1, rogue);
    // Tagged for the match's teardown: a match ending mid-Dance takes the rig
    // and its pieces (its children) with it.
    assert!(
        h.app.world().get::<PlayMatchEntity>(rigs[0].0).is_some(),
        "the rig is a PlayMatchEntity"
    );
    assert!((h.rig_at() - chest(at)).length() < 1e-5, "{}", h.rig_at());
    let pieces = h.pieces();
    let per_layer = |l: DanceLayer| pieces.iter().filter(|d| d.layer == l).count();
    assert_eq!(per_layer(DanceLayer::Bloom), 6);
    assert_eq!(per_layer(DanceLayer::RedRibbons), 44);
    assert_eq!(per_layer(DanceLayer::BlackInward), 38);
    assert_eq!(pieces.len(), 88);
}

/// Every lit particle, every frame of a whole window, against the bench: per
/// layer, the drawn set equals the bench's live set in number, colour (to its
/// truncated byte), alpha and size, so the rates, the lives, the tracks and
/// the ending's thinning and flicker are all the bench's. The blooms stand
/// still at the chest. The comparison is not vacuous: it sees thousands of
/// particles, including the ending's sparse ones.
#[test]
fn every_particle_draws_what_the_bench_draws() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, 1.0, 0.0);
    let (rogue, _) = h.rogue(at);
    h.dance(rogue);
    h.tick(1);
    assert_eq!(h.rigs().len(), 1, "the rig rises the frame after the aura");
    assert!(h.lit().is_empty(), "and draws nothing on that frame");
    let mut bench = BenchSim::default();
    let mut compared = [0usize; 3];
    let mut ending_compared = 0usize;
    let mut frames = 0;
    while h.remaining(rogue).is_some() {
        h.step();
        let Some(remaining) = h.remaining(rogue) else {
            break;
        };
        // The bench's clock: the window's elapsed time, read off the aura.
        bench.step(DT, (8.0 - remaining) * 1000.0);
        frames += 1;
        let lit = h.lit();
        for e in &BENCH {
            // Both sides past the first or last thousandth of alpha: a
            // particle at the very end of its life is invisible, and which
            // side of `age < life` it falls on is float noise at a life that
            // is a whole number of frames (the ribbons' 1.2 s is 75).
            let mut drawn: Vec<&Drawn> = lit
                .iter()
                .filter(|d| d.layer == e.layer && d.alpha > 1e-3)
                .collect();
            let mut want: Vec<BenchLook> = bench.ages[e.layer as usize]
                .iter()
                .map(|a| bench_look(e, *a))
                .collect();
            want.retain(|w| w.alpha > 1e-3);
            assert_eq!(
                drawn.len(),
                want.len(),
                "{:?} at {remaining} s left: drawn {} vs bench {}",
                e.layer,
                drawn.len(),
                want.len()
            );
            drawn.sort_by(|a, b| {
                by_age(
                    (a.scale.x as f64, a.alpha as f64),
                    (b.scale.x as f64, b.alpha as f64),
                )
            });
            want.sort_by(|a, b| by_age((a.diameter, a.alpha), (b.diameter, b.alpha)));
            for (d, w) in drawn.iter().zip(&want) {
                assert!(
                    (d.alpha as f64 - w.alpha).abs() < 2e-4,
                    "{:?} alpha {} vs {}",
                    e.layer,
                    d.alpha,
                    w.alpha
                );
                assert!(
                    (d.scale.x as f64 - w.diameter).abs() < 2e-4
                        && (d.scale.y as f64 - w.diameter).abs() < 2e-4,
                    "{:?} size {} vs {}",
                    e.layer,
                    d.scale,
                    w.diameter
                );
                for c in 0..3 {
                    assert!(
                        channel_matches(d.rgb[c], w.rgb[c]),
                        "{:?} rgb {:?} vs {:?}",
                        e.layer,
                        d.rgb,
                        w.rgb
                    );
                }
                if e.layer == DanceLayer::Bloom {
                    assert!((d.at - chest(at)).length() < 1e-4, "a bloom is still");
                }
                compared[e.layer as usize] += 1;
                if remaining < 1.5 {
                    ending_compared += 1;
                }
            }
        }
    }
    assert!(frames > 490, "the whole window: {frames} frames");
    assert!(compared[0] > 1500, "{compared:?}");
    assert!(compared[1] > 15000, "{compared:?}");
    assert!(compared[2] > 12000, "{compared:?}");
    assert!(ending_compared > 2000, "{ending_compared}");
}

/// The ending: the multiplier the bench's `rateMul` puts on every emitter,
/// read off the aura. Full before the last 1.5 s; then falling linearly to
/// zero, every third 120 ms step (counted from the window's start) at 15 %.
/// Pinned on the drawn births: in the window's first 6.5 s every 120 ms step
/// bears its share of ribbons, and in the ending the 15 % steps bear almost
/// none while their neighbours bear the linear fall.
#[test]
fn the_ending_thins_and_flickers() {
    let mut h = Harness::new();
    let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.dance(rogue);
    h.tick(1);
    // Births per 120 ms step of the window, by layer: a piece lit this frame
    // and not the last was born.
    let mut births: HashMap<(DanceLayer, i64), u32> = HashMap::new();
    let mut lit_before: BTreeSet<Entity> = BTreeSet::new();
    while h.remaining(rogue).is_some() {
        h.step();
        let Some(remaining) = h.remaining(rogue) else {
            break;
        };
        let step = (((8.0 - remaining) * 1000.0) / 120.0).floor() as i64;
        let lit = h.lit();
        for d in &lit {
            if !lit_before.contains(&d.entity) {
                *births.entry((d.layer, step)).or_default() += 1;
            }
        }
        lit_before = lit.iter().map(|d| d.entity).collect();
    }
    let births_in = |s: i64| {
        births
            .get(&(DanceLayer::RedRibbons, s))
            .copied()
            .unwrap_or(0)
            + births
                .get(&(DanceLayer::BlackInward, s))
                .copied()
                .unwrap_or(0)
    };
    // Full rate: 66 particles a second over both layers, 7.92 a step; a step
    // holds 7 or 8 frames and both layers are born on the same frames, so a
    // step bears 6 to 10.
    let mut steady = 0;
    for s in 1..=52 {
        assert!(
            (6..=10).contains(&births_in(s)),
            "step {s}: {}",
            births_in(s)
        );
        steady += births_in(s);
    }
    let expected = 66.0 * 0.12 * 52.0;
    assert!(
        (steady as f32 - expected).abs() <= 6.0,
        "{steady} vs {expected}"
    );
    // The ending, 6.5-8 s: steps 54..=66. The 15 % ones (54, 57, ... 66) bear
    // at most 2 between both layers; the full ones keep the linear fall.
    let red = births_in;
    let mut low_total = 0;
    let mut full_total = 0;
    for s in 54..=66 {
        let mul = 1.0 - ((s as f32 * 0.12 + 0.06) - 6.5) / 1.5;
        if s % 3 == 0 {
            assert!(red(s) <= 2, "step {s} runs at 15 %: {}", red(s));
            low_total += red(s);
        } else {
            let expected = 66.0 * 0.12 * mul;
            assert!(
                (red(s) as f32 - expected).abs() <= 2.0,
                "step {s}: {} vs {expected}",
                red(s)
            );
            full_total += red(s);
        }
    }
    assert!(full_total > 4 * low_total, "{full_total} vs {low_total}");
}

/// The ribbons drift out at 0.56 yd/s, each in its own direction, from a box
/// round the chest; the drawn-in shadow flies 1.39 yd/s in through the chest
/// from a shell 0.78-1.3 yd out, stretched up and down.
#[test]
fn the_ribbons_drift_out_and_the_shadow_is_drawn_in() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, 1.0, 0.0);
    let (rogue, _) = h.rogue(at);
    h.dance(rogue);
    h.tick(2);
    let c = chest(at);
    let mut last: HashMap<Entity, Drawn> = HashMap::new();
    let (mut red_dirs, mut black_births) = (Vec::new(), Vec::new());
    let (mut red_steps, mut black_steps) = (0, 0);
    let mut shell: HashMap<Entity, Vec3> = HashMap::new();
    let mut red_births: Vec<Vec3> = Vec::new();
    for _ in 0..240 {
        h.step();
        let lit = h.lit();
        for d in &lit {
            match last.get(&d.entity) {
                Some(prev) => {
                    let step = d.at - prev.at;
                    match d.layer {
                        DanceLayer::RedRibbons => {
                            assert!(
                                (step.length() - 0.56 * DT).abs() < 1e-4,
                                "a ribbon moves {} a frame",
                                step.length()
                            );
                            red_dirs.push(step.normalize());
                            red_steps += 1;
                        }
                        DanceLayer::BlackInward => {
                            assert!(
                                (step.length() - 1.39 * DT).abs() < 1e-4,
                                "the shadow moves {} a frame",
                                step.length()
                            );
                            // Straight in along the line it was born on,
                            // through the chest and on past it.
                            if let Some(born) = shell.get(&d.entity) {
                                assert!(
                                    step.normalize().dot(-born.normalize()) > 1.0 - 1e-3,
                                    "drawn in through the chest"
                                );
                            }
                            black_steps += 1;
                        }
                        DanceLayer::Bloom => assert!(step.length() < 1e-5),
                    }
                }
                None => {
                    // Born this frame (first drawn one frame into its life).
                    let off = d.at - c;
                    match d.layer {
                        DanceLayer::RedRibbons => {
                            let grow = 0.56 * DT + 1e-4;
                            assert!(
                                off.x.abs() <= 0.25 + grow
                                    && off.y.abs() <= 0.6 + grow
                                    && off.z.abs() <= 0.25 + grow,
                                "a ribbon is born in its box: {off}"
                            );
                            red_births.push(off);
                        }
                        DanceLayer::BlackInward => {
                            assert!(
                                (0.78 - 1.39 * DT - 1e-4..=1.3 + 1e-4).contains(&off.length()),
                                "on its shell: {}",
                                off.length()
                            );
                            black_births.push(off);
                            shell.insert(d.entity, off);
                        }
                        DanceLayer::Bloom => assert!(off.length() < 1e-4),
                    }
                }
            }
        }
        last = lit.into_iter().map(|d| (d.entity, d)).collect();
    }
    assert!(
        red_steps > 5000 && black_steps > 4000,
        "{red_steps} {black_steps}"
    );
    // The ribbons go every way.
    let mean = red_dirs.iter().copied().sum::<Vec3>() / red_dirs.len() as f32;
    assert!(mean.length() < 0.3, "spread all round: {mean}");
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(red_dirs.iter().any(|d| d.dot(axis) > 0.8), "{axis}");
        assert!(red_dirs.iter().any(|d| d.dot(axis) < -0.8), "-{axis}");
    }
    // The ribbons' births fill their box, 0.5 x 1.2 x 0.5 yd: some well up
    // or down the chest's 0.6 yd half-height, some near the 0.25 yd edges.
    assert!(red_births.len() > 100, "{}", red_births.len());
    assert!(red_births.iter().any(|b| b.y > 0.5), "high in the box");
    assert!(red_births.iter().any(|b| b.y < -0.5), "low in the box");
    assert!(red_births.iter().any(|b| b.x.abs() > 0.2), "out along x");
    assert!(red_births.iter().any(|b| b.z.abs() > 0.2), "out along z");
    // The shadow comes from every side, with births high above and below
    // the chest (its directions stretched 1.2 up and down).
    assert!(black_births.len() > 100, "{}", black_births.len());
    for axis in [Vec3::X, Vec3::Z] {
        assert!(black_births.iter().any(|b| b.dot(axis) > 0.6));
        assert!(black_births.iter().any(|b| b.dot(axis) < -0.6));
    }
    assert!(black_births.iter().any(|b| b.y > 0.9));
    assert!(black_births.iter().any(|b| b.y < -0.9));
    let spread = black_births.iter().map(|b| b.length()).collect::<Vec<_>>();
    assert!(spread.iter().any(|r| *r < 0.85) && spread.iter().any(|r| *r > 1.2));
}

/// Read one sprite's alpha at `x` of its radius along its middle row.
fn alpha_along(image: &Image, x: f32) -> f32 {
    let px = image.width() as usize;
    let col = ((((x + 1.0) / 2.0) * px as f32) as usize).min(px - 1);
    image.data.as_ref().unwrap()[((px / 2) * px + col) * 4 + 3] as f32 / 255.0
}

/// Every piece's material and mesh, against the bench's call for each layer:
/// the bloom and the drawn-in shadow are `Emitter.draw` with `blend:'alpha'`,
/// painted source-over (blended, so they darken); the ribbons are
/// `Emitter.draw` with no `blend`, painted `lighter` (added). All unlit, both
/// faces, on a unit quad, casting no shadow; the bloom wears the bench's
/// `glow` sprite (1, 0.45 at 0.45 of its radius, 0) and the other two its
/// `smoke` (0.85, 0.5, 0).
#[test]
fn each_layer_blends_and_wears_the_benchs_sprite() {
    let mut h = Harness::new();
    let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.dance(rogue);
    h.tick(3);
    let pieces = h.pieces();
    assert_eq!(pieces.len(), 88);
    let world = h.app.world();
    let images = world.resource::<Assets<Image>>();
    let meshes = world.resource::<Assets<Mesh>>();
    let mut sprites: HashMap<DanceLayer, BTreeSet<AssetId<Image>>> = HashMap::new();
    let mut materials = BTreeSet::new();
    for d in &pieces {
        let m = &d.material;
        let (bench_call, blended) = match d.layer {
            DanceLayer::Bloom => ("Emitter.draw, blend:'alpha' (source-over)", true),
            DanceLayer::RedRibbons => ("Emitter.draw, no blend ('lighter')", false),
            DanceLayer::BlackInward => ("Emitter.draw, blend:'alpha' (source-over)", true),
        };
        if blended {
            assert!(
                matches!(m.alpha_mode, AlphaMode::Blend),
                "{:?} is drawn by {bench_call}: {:?}",
                d.layer,
                m.alpha_mode
            );
        } else {
            assert!(
                matches!(m.alpha_mode, AlphaMode::Add),
                "{:?} is drawn by {bench_call}: {:?}",
                d.layer,
                m.alpha_mode
            );
        }
        assert!(m.unlit && m.double_sided && m.cull_mode.is_none());
        assert_eq!(
            m.emissive,
            LinearRgba::BLACK,
            "colour × sprite × alpha only"
        );
        assert!(d.no_shadow);
        let aabb = meshes.get(&d.mesh).unwrap().compute_aabb().unwrap();
        assert!((Vec3::from(aabb.half_extents) - Vec3::new(0.5, 0.5, 0.0)).length() < 1e-6);
        let texture = m.base_color_texture.as_ref().unwrap();
        let image = images.get(texture).unwrap();
        let profile = [0.0, 0.45, 0.999].map(|x| alpha_along(image, x));
        let stops = match d.layer {
            DanceLayer::Bloom => [1.0, 0.45, 0.0],
            _ => [0.85, 0.5, 0.0],
        };
        for (got, want) in profile.iter().zip(stops) {
            assert!((got - want).abs() < 0.03, "{:?} {profile:?}", d.layer);
        }
        sprites.entry(d.layer).or_default().insert(texture.id());
        assert!(materials.insert(d.material_id), "a material a piece");
    }
    assert_eq!(sprites[&DanceLayer::Bloom].len(), 1);
    assert_eq!(
        sprites[&DanceLayer::RedRibbons],
        sprites[&DanceLayer::BlackInward]
    );
    assert_ne!(
        sprites[&DanceLayer::Bloom],
        sprites[&DanceLayer::RedRibbons]
    );
}

/// Under a turned camera, and a Rogue turned and leaning, every particle
/// faces the camera, and the rig itself is never turned with the Rogue.
#[test]
fn every_particle_faces_a_turned_camera() {
    let mut h = Harness::new();
    let cam = Transform::from_xyz(7.0, 9.0, -5.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
    h.camera(cam);
    let (rogue, body) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.app
        .world_mut()
        .get_mut::<Transform>(rogue)
        .unwrap()
        .rotation = Quat::from_rotation_y(2.2);
    h.app
        .world_mut()
        .get_mut::<Transform>(body)
        .unwrap()
        .rotation = Quat::from_rotation_x(0.35);
    h.dance(rogue);
    h.tick(60);
    let lit = h.lit();
    assert!(lit.len() > 60, "{}", lit.len());
    for d in lit {
        assert!(
            d.rotation.angle_between(cam.rotation) < 1e-4,
            "{:?} faces the camera",
            d.layer
        );
    }
}

/// The state rides the RENDERED chest: the body's bob carries it, the Rogue's
/// facing does not turn it, and every particle in the air moves with the
/// Rogue (the bench's `follow`).
#[test]
fn the_state_rides_the_rendered_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, 1.0, 1.0);
    let (rogue, body) = h.rogue(at);
    h.dance(rogue);
    h.tick(40);
    h.app
        .world_mut()
        .get_mut::<Transform>(body)
        .unwrap()
        .translation = Vec3::new(0.0, 0.2, 0.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(rogue)
        .unwrap()
        .rotation = Quat::from_rotation_y(1.3);
    h.tick(1);
    assert!(
        (h.rig_at() - (at + Vec3::new(0.0, 0.2 + CHEST, 0.0))).length() < 1e-5,
        "{}",
        h.rig_at()
    );
    let mut q = h
        .app
        .world_mut()
        .query::<(&ShadowDanceRig, &GlobalTransform)>();
    let rot = q.single(h.app.world()).unwrap().1.rotation();
    assert!(rot.angle_between(Quat::IDENTITY) < 1e-6, "never turned");

    // A pair of frames with the Rogue still and a pair with it moving: each
    // particle's step differs by exactly the Rogue's.
    let still: HashMap<Entity, Vec3> = {
        let a: HashMap<Entity, Vec3> = h.lit().into_iter().map(|d| (d.entity, d.at)).collect();
        h.tick(1);
        h.lit()
            .into_iter()
            .filter_map(|d| a.get(&d.entity).map(|p| (d.entity, d.at - *p)))
            .collect()
    };
    let before: HashMap<Entity, Vec3> = h.lit().into_iter().map(|d| (d.entity, d.at)).collect();
    let shift = Vec3::new(0.3, 0.0, -0.2);
    h.app
        .world_mut()
        .get_mut::<Transform>(rogue)
        .unwrap()
        .translation += shift;
    h.tick(1);
    let mut checked = 0;
    for d in h.lit() {
        let (Some(p), Some(s)) = (before.get(&d.entity), still.get(&d.entity)) else {
            continue;
        };
        // Same particle, one frame on: its own flight (as the frame before)
        // plus the Rogue's shift.
        assert!(((d.at - *p) - *s - shift).length() < 2e-4, "{:?}", d.layer);
        checked += 1;
    }
    assert!(checked > 40, "{checked}");
}

/// A refresh keeps the state going — the same rig, its clock running on, no
/// new onset — and puts the rates back to full when it comes in the ending,
/// whether the aura's time is renewed or the aura is replaced within the
/// frame; a Dance landing again while the last one's particles are still in
/// the air takes the same rig back up.
#[test]
fn a_refresh_keeps_the_state_going() {
    let ribbon_births = |h: &mut Harness, frames: u32| {
        let mut born = 0;
        let mut before: BTreeSet<Entity> = h.lit().iter().map(|d| d.entity).collect();
        for _ in 0..frames {
            h.step();
            let lit = h.lit();
            born += lit
                .iter()
                .filter(|d| d.layer == DanceLayer::RedRibbons && !before.contains(&d.entity))
                .count();
            before = lit.iter().map(|d| d.entity).collect();
        }
        born
    };
    for replace in [false, true] {
        let mut h = Harness::new();
        let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
        h.dance(rogue);
        // Into the ending: 1.0 s left.
        h.tick(((DUR0 - 1.0) / DT) as u32);
        let (rig, _, age) = h.rigs()[0];
        let thin = ribbon_births(&mut h, 30);
        assert!(thin <= 6, "the ending thins the ribbons: {thin}");
        if replace {
            let mut auras = h.auras(rogue);
            auras.retain(|a| a.effect_type != AuraType::ShadowDance);
            auras.push(dance_aura());
        } else {
            for aura in h.auras(rogue).iter_mut() {
                if aura.effect_type == AuraType::ShadowDance {
                    aura.duration = DUR0;
                }
            }
        }
        let full = ribbon_births(&mut h, 30);
        assert!((15..=17).contains(&full), "full again: {full}");
        let rigs = h.rigs();
        assert_eq!(rigs.len(), 1);
        assert_eq!(rigs[0].0, rig, "the same rig");
        assert!(
            (rigs[0].2 - (age + 60.0 * DT)).abs() < 1e-3,
            "its clock runs on"
        );
    }

    // Removed, then landing again before the last particles are spent.
    let mut h = Harness::new();
    let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.dance(rogue);
    h.tick(100);
    let rig = h.rigs()[0].0;
    h.auras(rogue)
        .retain(|a| a.effect_type != AuraType::ShadowDance);
    h.tick(20);
    assert_eq!(h.rigs()[0].0, rig, "still spending its particles");
    h.dance(rogue);
    let again = ribbon_births(&mut h, 30);
    assert!((15..=17).contains(&again), "{again}");
    assert_eq!(h.rigs()[0].0, rig, "the same rig");
}

/// The state ends with the aura, however it ends, and the Rogue keeps its
/// other auras through it. The aura run out or removed early: nothing more
/// is born from that frame, the particles in the air live out their lives,
/// and the rig goes with the last of them (by 1.5 s, the bloom's life). Its
/// Rogue dead (the aura stays on a corpse) or gone: the rig and every
/// particle go at once. A new Dance after the rig has gone is a new onset.
#[test]
fn the_state_ends_with_the_aura_or_the_rogue() {
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum End {
        RunsOut,
        Removed,
        Dies,
        Gone,
    }
    for end in [End::RunsOut, End::Removed, End::Dies, End::Gone] {
        let mut h = Harness::new();
        let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
        h.dance(rogue);
        h.tick(150);
        assert_eq!(h.rigs().len(), 1, "{end:?}");
        assert_eq!(h.count::<ShadowDancePiece>(), 88, "{end:?}");
        match end {
            End::RunsOut => {
                while h.remaining(rogue).is_some() {
                    h.step();
                }
            }
            End::Removed => {
                h.auras(rogue)
                    .retain(|a| a.effect_type != AuraType::ShadowDance);
                h.step();
            }
            End::Dies => {
                h.combatant(rogue).current_health = 0.0;
                h.step();
            }
            End::Gone => {
                h.app.world_mut().entity_mut(rogue).despawn();
                h.step();
            }
        }
        match end {
            End::RunsOut | End::Removed => {
                let lit = h.lit();
                if end == End::Removed {
                    assert!(
                        lit.len() > 40,
                        "{end:?}: the air is not cleared: {}",
                        lit.len()
                    );
                } else {
                    assert!(!lit.is_empty(), "{end:?}: the last particles live on");
                }
                let mut lit_before: BTreeSet<Entity> = lit.iter().map(|d| d.entity).collect();
                let mut frames = 0;
                while !h.rigs().is_empty() {
                    h.step();
                    frames += 1;
                    let lit = h.lit();
                    for d in &lit {
                        assert!(
                            lit_before.contains(&d.entity),
                            "{end:?}: a birth after the end"
                        );
                    }
                    lit_before = lit.iter().map(|d| d.entity).collect();
                    assert!(frames <= (1.5 / DT) as u32 + 2, "{end:?}: still up");
                }
                assert_eq!(h.count::<ShadowDancePiece>(), 0, "{end:?}");
                assert!(
                    frames > 30,
                    "{end:?}: gone with its last particle, {frames}"
                );
                let worn = names(&mut h, rogue);
                assert_eq!(worn, other_names(), "{end:?}: the other auras stay");
            }
            End::Dies | End::Gone => {
                assert!(h.rigs().is_empty(), "{end:?}: the rig");
                assert_eq!(h.count::<ShadowDancePiece>(), 0, "{end:?}: every particle");
                h.tick(10);
                assert!(h.rigs().is_empty(), "{end:?}: nothing comes back");
                if end == End::Dies {
                    let worn = names(&mut h, rogue);
                    assert!(worn.contains("Shadow Dance"), "on the corpse: {worn:?}");
                    assert!(worn.is_superset(&other_names()));
                }
            }
        }
    }

    // A new Dance after the last has gone is a new onset.
    let mut h = Harness::new();
    let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.dance(rogue);
    h.tick(50);
    let first = h.rigs()[0].0;
    h.auras(rogue)
        .retain(|a| a.effect_type != AuraType::ShadowDance);
    h.tick(100);
    assert!(h.rigs().is_empty());
    h.dance(rogue);
    h.tick(1);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert_ne!(rigs[0].0, first);
    assert!(rigs[0].2 < 1e-6, "a fresh clock: {}", rigs[0].2);
}

/// Nothing rises on a corpse holding the Dance, nor for the Rogue's other
/// auras alone.
#[test]
fn a_corpse_and_other_auras_raise_no_state() {
    let mut h = Harness::new();
    let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.combatant(rogue).current_health = 0.0;
    h.dance(rogue);
    h.tick(5);
    assert!(h.rigs().is_empty(), "a corpse");

    let mut h = Harness::new();
    h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.tick(5);
    assert!(h.rigs().is_empty(), "the other auras alone");

    let mut h = Harness::new();
    let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.dance(rogue);
    h.tick(5);
    assert_eq!(h.rigs().len(), 1, "the control");
}

/// A polymorphed Rogue's state hides with its body (the sheep wears
/// nothing), and comes back with it; the clock and the births run on
/// underneath.
#[test]
fn a_sheep_hides_the_state() {
    let mut h = Harness::new();
    let (rogue, _) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.dance(rogue);
    h.tick(5);
    let visibility = |h: &mut Harness| {
        let rig = h.rigs()[0].0;
        *h.app.world().get::<Visibility>(rig).unwrap()
    };
    assert_eq!(visibility(&mut h), Visibility::Inherited);
    h.app
        .world_mut()
        .entity_mut(rogue)
        .insert(PolymorphedVisual);
    h.tick(1);
    assert_eq!(visibility(&mut h), Visibility::Hidden);
    let before: BTreeSet<Entity> = h.lit().iter().map(|d| d.entity).collect();
    h.tick(60);
    assert_eq!(visibility(&mut h), Visibility::Hidden);
    let born = h
        .lit()
        .iter()
        .filter(|d| !before.contains(&d.entity))
        .count();
    assert!(born > 20, "births run on under the sheep: {born}");
    h.app
        .world_mut()
        .entity_mut(rogue)
        .remove::<PolymorphedVisual>();
    h.tick(1);
    assert_eq!(visibility(&mut h), Visibility::Inherited);
    assert_eq!(h.rigs().len(), 1);
    assert!((h.rigs()[0].2 - 66.0 * DT).abs() < 1e-3);
}

/// Stealth on and off through the Dance changes nothing in it — every piece
/// drawn the same, frame for frame, as a twin Dance whose Rogue never
/// stealths — and the Dance never touches the body: its material is exactly
/// what stealth makes it (40 % alpha, darkened) on a Rogue that is not
/// dancing.
#[test]
fn stealth_changes_nothing_in_the_dance_and_the_dance_never_touches_the_body() {
    let mut stealthy = Harness::new();
    let mut plain = Harness::new();
    let mut bare = Harness::new();
    let (r1, b1) = stealthy.rogue(Vec3::new(0.0, 1.0, 0.0));
    let (r2, _) = plain.rogue(Vec3::new(0.0, 1.0, 0.0));
    let (r3, b3) = bare.rogue(Vec3::new(0.0, 1.0, 0.0));
    // A Rogue enters the arena stealthed: start all three visible, and keep
    // the twin so.
    for (h, r) in [(&mut stealthy, r1), (&mut plain, r2), (&mut bare, r3)] {
        h.combatant(r).stealthed = false;
    }
    stealthy.dance(r1);
    plain.dance(r2);
    let base = stealthy.body_color(b1).to_srgba();
    let visibility = |h: &mut Harness| {
        let rig = h.rigs()[0].0;
        *h.app.world().get::<Visibility>(rig).unwrap()
    };
    let mut stealthed_frames = 0;
    for frame in 0..420 {
        let stealthed = (60..180).contains(&frame) || (300..400).contains(&frame);
        if stealthy.combatant(r1).stealthed != stealthed {
            stealthy.combatant(r1).stealthed = stealthed;
            bare.combatant(r3).stealthed = stealthed;
        }
        stealthy.step();
        plain.step();
        bare.step();
        let a = stealthy.pieces();
        let b = plain.pieces();
        assert_eq!(a.len(), b.len());
        let key = |d: &Drawn| (d.layer, d.at.to_array().map(f32::to_bits));
        let mut a: Vec<_> = a
            .iter()
            .map(|d| (key(d), d.rgb, d.alpha.to_bits(), d.scale))
            .collect();
        let mut b: Vec<_> = b
            .iter()
            .map(|d| (key(d), d.rgb, d.alpha.to_bits(), d.scale))
            .collect();
        a.sort_by(|x, y| format!("{:?}", x.0).cmp(&format!("{:?}", y.0)));
        b.sort_by(|x, y| format!("{:?}", x.0).cmp(&format!("{:?}", y.0)));
        assert_eq!(a, b, "frame {frame}: stealth moved the Dance");
        if frame > 0 {
            assert_eq!(visibility(&mut stealthy), Visibility::Inherited, "{frame}");
            assert_eq!(visibility(&mut plain), Visibility::Inherited, "{frame}");
            assert!(!plain.combatant(r2).stealthed);
        }

        let body = stealthy.body_color(b1);
        assert_eq!(
            body,
            bare.body_color(b3),
            "frame {frame}: the body is stealth's alone"
        );
        let c = body.to_srgba();
        if stealthed {
            stealthed_frames += 1;
            assert!((c.alpha - 0.4).abs() < 1e-6, "40 % alpha");
            assert!((c.red - base.red * 0.6).abs() < 1e-5, "darkened");
        } else {
            assert!((c.alpha - 1.0).abs() < 1e-6);
        }
    }
    assert!(stealthed_frames > 200);
}

/// The match camera (`CameraController`'s placement) at `zoom` and `pitch`
/// from the vertical, and `yaw`, looking at `target`.
fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

/// At the match camera (60 yd, 38.7° from the vertical) and every bearing
/// round the Rogue, the bloom (still, on the body's axis above its centre)
/// sorts in front of the blended body, so it is laid over it; and at 1080p
/// (45° vertical field) the state's pieces are the sizes the report cites:
/// the bloom swelling to about 75 px across, the ribbons about 8-10 px and
/// the drawn-in shadow 7-21 px, against a body about 22 px wide.
#[test]
fn the_state_reads_at_the_match_camera() {
    let controller = CameraController::default();
    assert_eq!(controller.zoom_distance, 60.0);
    let mut h = Harness::new();
    let cam = h.camera(Transform::default());
    let (rogue, body) = h.rogue(Vec3::new(0.0, 1.0, 0.0));
    h.dance(rogue);
    h.tick(120);
    let mut sorted = 0;
    for k in 0..24 {
        let yaw = k as f32 / 24.0 * std::f32::consts::TAU;
        let view = match_camera(
            Vec3::new(0.0, 1.0, 0.0),
            controller.zoom_distance,
            controller.pitch,
            yaw,
        );
        *h.app.world_mut().get_mut::<Transform>(cam).unwrap() = view;
        h.tick(1);
        let matrix = h
            .app
            .world()
            .get::<GlobalTransform>(cam)
            .unwrap()
            .compute_matrix();
        let range = ViewRangefinder3d::from_world_from_view(&matrix);
        let body_at = h
            .app
            .world()
            .get::<GlobalTransform>(body)
            .unwrap()
            .translation();
        let blooms: Vec<Drawn> = h
            .lit()
            .into_iter()
            .filter(|d| d.layer == DanceLayer::Bloom)
            .collect();
        assert!(!blooms.is_empty());
        for d in blooms {
            assert!(
                range.distance_translation(&d.at) > range.distance_translation(&body_at),
                "yaw {yaw}: the bloom sorts over the body"
            );
            sorted += 1;
        }
    }
    assert!(sorted >= 24);
    // Pixels across at 1080p for a size in yards, 60 yd off.
    let focal = 540.0 / (std::f32::consts::FRAC_PI_4 / 2.0).tan();
    let px = |yards: f32| yards * focal / controller.zoom_distance;
    assert!(
        (70.0..80.0).contains(&px(3.48)),
        "the bloom's swell: {}",
        px(3.48)
    );
    assert!((7.0..11.0).contains(&px(0.36)) && (9.0..12.0).contains(&px(0.48)));
    assert!((20.0..24.0).contains(&px(1.0)), "the body: {}", px(1.0));
}

/// The drawn-in shadow's shell is stretched 1.2 up and down (the bench's
/// `inwardFlat`): its birth directions are a random point of the box
/// `[-1, 1] × [-1.2, 1.2] × [-1, 1]` normalised, so they lean toward the
/// vertical — the mean of `|y|` stands about 1.16 times the mean of `|x|` and
/// `|z|` (1.0 unstretched). Measured over a long window's two thousand
/// births.
#[test]
fn the_shadow_is_drawn_in_from_a_shell_stretched_up_and_down() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, 1.0, 0.0);
    let (rogue, _) = h.rogue(at);
    let mut long = dance_aura();
    long.duration = 70.0;
    h.auras(rogue).push(long);
    h.tick(2);
    let c = chest(at);
    let mut seen: BTreeSet<Entity> = BTreeSet::new();
    let mut dirs = Vec::new();
    for _ in 0..(60.0 / DT) as u32 {
        h.step();
        let lit = h.lit();
        for d in &lit {
            if d.layer == DanceLayer::BlackInward && !seen.contains(&d.entity) {
                dirs.push((d.at - c).normalize());
            }
        }
        seen = lit.iter().map(|d| d.entity).collect();
    }
    assert!(dirs.len() > 1900, "{}", dirs.len());
    let mean = |f: fn(&Vec3) -> f32| dirs.iter().map(f).sum::<f32>() / dirs.len() as f32;
    let y = mean(|d| d.y.abs());
    let xz = (mean(|d| d.x.abs()) + mean(|d| d.z.abs())) / 2.0;
    assert!((1.10..1.22).contains(&(y / xz)), "{y} / {xz} = {}", y / xz);
}
