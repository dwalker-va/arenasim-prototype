//! Probes for Power Infusion's crackle and Pain Suppression's shell
//! (`rendering/effects/discipline_states.rs`, AS-353): the Priest's two
//! Discipline buffs, each drawn as its state on the ally for the buff's life,
//! with the Priest's Holy hands at the onset and, for Pain Suppression, a
//! swirl at the Priest's feet.
//!
//! These drive the graphical chain off `ActiveAuras`, as the sim leaves it,
//! and assert what is DRAWN, in world space through `GlobalTransform` and the
//! spawned materials, meshes and textures: every fixed piece against an
//! independent transcription of the signed-off bench's `customDraw` across
//! the whole buff, every particle against the bench's emitter records and its
//! own accumulator, the sprites read back, the draw order against the body
//! (and the Power Word: Shield the ally may wear) at the match camera, the
//! routing off the aura band, and the lifecycle with other buffs present.
//!
//! The transcription reads none of the module's constants.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`: no window,
//! no GPU. The systems run in the order `states/mod.rs` registers them.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy::render::render_phase::ViewRangefinder3d;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraApplyOwner, AuraApplyRoute, AuraBand, AuraPending, AuraType, CastEnding,
    CastEndingKind, Combatant, HealCastHand, HealCastKind, LandedCast, PlayMatchEntity,
    PolymorphedVisual, ShieldCard, VisualBody, PAIN_SUPPRESSION_AURA, POWER_INFUSION_AURA,
};
use arenasim::states::play_match::{
    cleanup_play_match, consume_cast_ending_signals, detect_aura_applications,
    place_discipline_states, place_shield_cards, spawn_instant_cast_hands,
    update_discipline_states, update_heal_cast_glows, update_shield_visuals, DisciplineState,
    DisciplineStatePiece, DisciplineStateRig, ShellQuad, StateLayer, StatePiece, SuppressionSwirl,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;

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
        app.insert_resource(AbilityDefinitions::default());
        app.add_systems(
            Update,
            (
                spawn_instant_cast_hands,
                consume_cast_ending_signals,
                detect_aura_applications,
                update_discipline_states,
                update_shield_visuals,
                update_heal_cast_glows,
            )
                .chain(),
        );
        app.add_systems(
            PostUpdate,
            (place_discipline_states, place_shield_cards)
                .before(TransformSystem::TransformPropagate),
        );
        Harness { app }
    }

    /// A unit as `spawn_combatant` builds it, with a `VisualBody` child.
    fn unit(&mut self, class: CharacterClass, slot: u8, at: Vec3) -> (Entity, Entity) {
        let world = self.app.world_mut();
        let unit = world
            .spawn((
                Combatant::new(1, slot, class),
                Transform::from_translation(at),
                ActiveAuras::default(),
            ))
            .id();
        let body = world
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        world.entity_mut(unit).add_child(body);
        (unit, body)
    }

    /// A Priest and a Mage ally.
    fn pair(&mut self) -> (Entity, Entity) {
        let (priest, _) = self.unit(CharacterClass::Priest, 0, Vec3::new(-3.0, 1.0, 0.0));
        let (ally, _) = self.unit(CharacterClass::Mage, 1, Vec3::new(2.0, 1.0, 1.0));
        (priest, ally)
    }

    fn camera(&mut self, transform: Transform) -> Entity {
        self.app
            .world_mut()
            .spawn((Camera3d::default(), transform))
            .id()
    }

    /// `ability`'s aura from `caster` onto `target`, as the real config lands
    /// it.
    fn land(&mut self, ability: AbilityType, target: Entity, caster: Entity) {
        let defs = AbilityDefinitions::default();
        let def = defs.get(&ability).unwrap();
        let aura = AuraPending::from_ability(target, caster, def).unwrap().aura;
        assert_eq!(aura.caster, Some(caster), "the aura names its caster");
        self.auras(target).push(aura);
    }

    fn infuse(&mut self, ally: Entity, priest: Entity) {
        self.land(AbilityType::PowerInfusion, ally, priest);
    }

    fn suppress(&mut self, ally: Entity, priest: Entity) {
        self.land(AbilityType::PainSuppression, ally, priest);
    }

    /// The other buffs an ally commonly wears beside these: a Power Word:
    /// Shield, a heal over time (Rejuvenation: the sim's Priest has no
    /// Renew) and Power Word: Fortitude.
    fn dress(&mut self, ally: Entity, priest: Entity) {
        self.land(AbilityType::PowerWordShield, ally, priest);
        self.land(AbilityType::Rejuvenation, ally, priest);
        self.land(AbilityType::PowerWordFortitude, ally, priest);
    }

    fn auras(&mut self, unit: Entity) -> Mut<'_, Vec<Aura>> {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .map_unchanged(|a| &mut a.auras)
    }

    fn remaining(&mut self, unit: Entity, name: &str) -> Option<f32> {
        self.auras(unit)
            .iter()
            .find(|a| a.ability_name == name)
            .map(|a| a.duration)
    }

    /// One frame, the buffs running down and running out as the sim runs
    /// them.
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

    fn rigs(&mut self) -> Vec<RigView> {
        let mut q =
            self.app
                .world_mut()
                .query::<(Entity, &DisciplineStateRig, &GlobalTransform, &Visibility)>();
        q.iter(self.app.world())
            .map(|(e, r, g, v)| RigView {
                entity: e,
                state: r.state,
                bearer: r.bearer,
                age: r.age,
                at: g.translation(),
                visibility: *v,
            })
            .collect()
    }

    fn rig(&mut self, state: DisciplineState) -> RigView {
        let rigs: Vec<RigView> = self
            .rigs()
            .into_iter()
            .filter(|r| r.state == state)
            .collect();
        assert_eq!(rigs.len(), 1, "one {state:?}");
        rigs[0]
    }

    fn swirls(&mut self) -> Vec<(Entity, Entity, f32, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &SuppressionSwirl, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(e, s, g)| (e, s.caster, s.age, g.translation()))
            .collect()
    }

    fn pieces(&mut self) -> Vec<Drawn> {
        let mut q = self.app.world_mut().query::<(
            &DisciplineStatePiece,
            &ChildOf,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            &Mesh3d,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(p, c, g, m, mesh)| (p.piece, c.parent(), *g, m.0.clone(), mesh.0.clone()))
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(piece, rig, g, m, mesh)| {
                let material = materials.get(&m).unwrap().clone();
                let c = material.base_color.to_srgba();
                Drawn {
                    piece,
                    rig,
                    at: g.translation(),
                    rotation: g.rotation(),
                    scale: g.scale(),
                    rgb: [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8),
                    alpha: c.alpha,
                    material,
                    mesh,
                }
            })
            .collect()
    }

    fn hands(&mut self) -> Vec<(HealCastKind, Entity)> {
        let mut q = self.app.world_mut().query::<&HealCastHand>();
        q.iter(self.app.world())
            .map(|h| (h.kind, h.caster))
            .collect()
    }

    fn count<T: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&T>();
        q.iter(self.app.world()).count()
    }
}

#[derive(Clone, Copy, Debug)]
struct RigView {
    entity: Entity,
    state: DisciplineState,
    bearer: Entity,
    age: f32,
    at: Vec3,
    visibility: Visibility,
}

#[derive(Clone, Debug)]
struct Drawn {
    piece: StatePiece,
    rig: Entity,
    at: Vec3,
    rotation: Quat,
    scale: Vec3,
    rgb: [u8; 3],
    alpha: f32,
    material: StandardMaterial,
    mesh: Handle<Mesh>,
}

/// Feet, chest and the shell's height for a unit whose centre is at `at`
/// (a combatant's centre stands a yard above its feet).
fn feet(at: Vec3) -> Vec3 {
    at - Vec3::Y
}

// ── the bench, transcribed ──────────────────────────────────────────────────
//
// The "Power Infusion" and "Pain Suppression" tabs of
// `docs/design/benches/2026-10-17-priest-talent-bench.html` (version
// 1792207490-53c9), written out from their JavaScript at the default knobs
// (size, spread, opacity, duration and density all 1). Milliseconds, as the
// bench keys them; `c` is the bench's chest (y 1.45) and the shell's `p`
// (y 1.3, 0.4 in front).

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

/// The bench's `keys` over colours.
fn keys3(k: &[(f64, [f64; 3])], t: f64) -> [f64; 3] {
    let channel = |c: usize| keys(&k.iter().map(|(t, v)| (*t, v[c])).collect::<Vec<_>>(), t);
    [channel(0), channel(1), channel(2)]
}

/// What the bench draws for one fixed piece: colour (before `rgb()`
/// truncates it), alpha, centre from the anchor, and full size (a ring: its
/// radius as `x`).
#[derive(Debug, Clone, Copy)]
struct BenchDraw {
    rgb: [f64; 3],
    alpha: f64,
    at: [f64; 3],
    size: [f64; 2],
}

fn draw(rgb: [f64; 3], alpha: f64, size: [f64; 2]) -> BenchDraw {
    BenchDraw {
        rgb,
        alpha,
        at: [0.0; 3],
        size,
    }
}

/// The PI tab's `customDraw` for one piece at `ms`.
fn bench_infusion(piece: StatePiece, ms: f64) -> Option<BenchDraw> {
    let w = keys(&[(0.0, 0.1), (500.0, 0.5), (1000.0, 0.1)], ms % 1000.0);
    let pa = keys(
        &[
            (0.0, 1.0),
            (108.0, 0.62),
            (216.0, 1.16),
            (324.0, 0.5),
            (433.0, 1.0),
        ],
        ms % 433.0,
    );
    let onset = if ms < 1334.0 {
        (ms / 1334.0 * std::f64::consts::PI).sin()
    } else {
        0.0
    };
    Some(match piece {
        StatePiece::WideDisc => {
            let d = 4.6 * (0.85 + 0.15 * pa);
            draw([110.0, 150.0, 255.0], w * 0.55, [d, d])
        }
        StatePiece::MidDisc => draw([140.0, 180.0, 255.0], w * 0.6, [3.2, 3.2]),
        StatePiece::CoreDisc => draw([190.0, 215.0, 255.0], 0.35, [2.2, 2.2]),
        StatePiece::OnsetStar => draw([235.0, 240.0, 255.0], onset * 0.5, [3.4, 3.4]),
        StatePiece::OnsetStreak => draw([240.0, 245.0, 255.0], onset * 0.9, [6.1, 0.3]),
        StatePiece::Lattice { .. } => {
            let f = keys(
                &[
                    (0.0, 0.0),
                    (183.0, 0.73),
                    (366.0, 0.0),
                    (550.0, 0.73),
                    (733.0, 0.0),
                    (916.0, 0.73),
                    (1100.0, 0.0),
                ],
                ms % 1100.0,
            );
            draw(
                [210.0, 235.0, 255.0],
                if f > 0.05 { f } else { 0.0 },
                [0.0, 0.0],
            )
        }
        _ => return None,
    })
}

/// The lattice's point `j` of strand `k` at `ms`, from the chest.
fn bench_lattice_point(k: u8, j: u8, ms: f64) -> [f64; 3] {
    let (k, j) = (k as f64, j as f64);
    let a = k * 0.9 + j * 0.5 + (ms / 67.0).floor() * 0.37;
    let r = 0.85 * (0.6 + 0.4 * (a * 3.0 + j).sin());
    [
        r * (a * 2.1).cos(),
        (j / 5.0 - 0.5) * 1.5,
        r * (a * 1.7).sin(),
    ]
}

/// The PS tab's shell for one quad at `ms`; `at` from the shell's anchor
/// (y 1.3), along the ally's facing `fwd`.
fn bench_shell(quad: ShellQuad, ms: f64, fwd: [f64; 2]) -> BenchDraw {
    let sq = keys(
        &[(0.0, 0.52), (300.0, 1.09), (600.0, 0.9), (867.0, 1.0)],
        ms % 867.0,
    );
    let fl = if ms < 867.0 {
        keys(&[(0.0, 0.0), (467.0, 1.0), (867.0, 0.14)], ms)
    } else {
        keys(
            &[
                (0.0, 0.05),
                (217.0, 0.4),
                (433.0, 0.1),
                (650.0, 0.35),
                (867.0, 0.05),
            ],
            ms % 867.0,
        )
    };
    let (w, h, col) = match quad {
        ShellQuad::Teal => (3.4, 4.4, [34.0, 236.0, 219.0]),
        ShellQuad::Yellow => (2.4, 3.1, [254.0, 240.0, 81.0]),
        ShellQuad::Gold => (2.3, 2.9, [212.0, 176.0, 34.0]),
    };
    BenchDraw {
        rgb: col,
        alpha: fl * 0.55,
        at: [fwd[0] * 0.4, 0.0, fwd[1] * 0.4],
        size: [w, h * sq],
    }
}

/// The PS tab's cast at the Priest's feet, `ms` after the cast; `at` from
/// the feet. `None` once the bench stops drawing it.
fn bench_swirl(piece: StatePiece, ms: f64) -> Option<BenchDraw> {
    if ms >= 1067.0 {
        return None;
    }
    let r = keys(&[(0.0, 2.4), (833.0, 1.4)], ms) / 2.0;
    let a = keys(&[(0.0, 0.0), (333.0, 1.0), (833.0, 0.0)], ms);
    match piece {
        StatePiece::SwirlRing => Some(BenchDraw {
            rgb: keys3(
                &[
                    (0.0, [255.0, 132.0, 0.0]),
                    (333.0, [255.0, 216.0, 0.0]),
                    (833.0, [160.0, 80.0, 255.0]),
                ],
                ms,
            ),
            alpha: a,
            at: [0.0, 0.04, 0.0],
            size: [r, r],
        }),
        StatePiece::SwirlGlow => Some(BenchDraw {
            rgb: [255.0, 200.0, 60.0],
            alpha: a * 0.5,
            at: [0.0, 0.03, 0.0],
            size: [2.0 * 1.05, 2.0 * 1.05],
        }),
        _ => None,
    }
}

/// The bench's three emitter records.
struct BenchEmitter {
    layer: StateLayer,
    rate: f32,
    life: f32,
    speed: f32,
    color: Vec<(f64, [f64; 3])>,
    alpha: Vec<(f64, f64)>,
    scale: Vec<(f64, f64)>,
    tail: f64,
}

fn bench_emitters() -> Vec<BenchEmitter> {
    vec![
        // PI sparks.
        BenchEmitter {
            layer: StateLayer::InfusionSparks,
            rate: 60.0,
            life: 0.2,
            speed: 3.3,
            color: vec![(0.0, [255.0, 255.0, 255.0]), (1.0, [200.0, 220.0, 255.0])],
            alpha: vec![(0.0, 1.0), (1.0, 0.0)],
            scale: vec![(0.0, 0.42), (0.5, 0.28), (1.0, 0.03)],
            tail: 0.0,
        },
        // PS bubbles.
        BenchEmitter {
            layer: StateLayer::SuppressionBubbles,
            rate: 10.0,
            life: 1.6,
            speed: 0.15,
            color: vec![(0.0, [255.0, 250.0, 200.0]), (1.0, [180.0, 240.0, 230.0])],
            alpha: vec![(0.0, 0.61), (1.0, 0.0)],
            scale: vec![(0.0, 0.2), (1.0, 1.1)],
            tail: 0.0,
        },
        // PS streaks.
        BenchEmitter {
            layer: StateLayer::SuppressionStreaks,
            rate: 100.0,
            life: 0.45,
            speed: 4.7,
            color: vec![
                (0.0, [255.0, 240.0, 120.0]),
                (0.5, [80.0, 230.0, 230.0]),
                (1.0, [60.0, 120.0, 255.0]),
            ],
            alpha: vec![(0.0, 0.0), (0.5, 0.9), (1.0, 0.0)],
            scale: vec![(0.0, 0.05), (1.0, 0.03)],
            tail: 2.0,
        },
    ]
}

impl BenchEmitter {
    /// The bench's tail: `min(tail × 0.12, speed × 0.06)` at size 1.
    fn tail_length(&self) -> f64 {
        (self.tail * 0.12).min(self.speed as f64 * 0.06)
    }
}

/// An emitter's speed, yd/s, as the bench declares it.
fn speed(layer: StateLayer) -> f32 {
    bench_emitters()
        .into_iter()
        .find(|e| e.layer == layer)
        .unwrap()
        .speed
}

/// The bench's `Emitter.update` for one emitter: accrue, birth, age, retire
/// (the engine's f32 arithmetic).
#[derive(Default)]
struct BenchSim {
    acc: f32,
    ages: Vec<f32>,
}

impl BenchSim {
    fn step(&mut self, e: &BenchEmitter, dt: f32) {
        self.acc += e.rate * dt;
        while self.acc >= 1.0 {
            self.acc -= 1.0;
            self.ages.push(0.0);
        }
        for age in &mut self.ages {
            *age += dt;
        }
        self.ages.retain(|a| *a < e.life);
    }
}

/// A live particle's look at `age`: colour before truncation, alpha,
/// diameter.
fn bench_look(e: &BenchEmitter, age: f32) -> ([f64; 3], f64, f64) {
    let t = age as f64 / e.life as f64;
    (
        keys3(&e.color, t),
        keys(&e.alpha, t),
        2.0 * keys(&e.scale, t),
    )
}

/// A colour drawn from a bench value: exact to the truncated byte where the
/// value's fraction is clear of an integer, one either way where it is not.
fn rgb_matches(drawn: [u8; 3], bench: [f64; 3]) -> bool {
    (0..3).all(|c| {
        let v = bench[c];
        let frac = v - v.floor();
        if (0.02..=0.98).contains(&frac) {
            drawn[c] as f64 == v.floor()
        } else {
            (drawn[c] as f64 - v.round()).abs() <= 1.0
        }
    })
}

fn infusion_fixed_pieces() -> BTreeSet<StatePiece> {
    let mut all = BTreeSet::from([
        StatePiece::WideDisc,
        StatePiece::MidDisc,
        StatePiece::CoreDisc,
        StatePiece::OnsetStar,
        StatePiece::OnsetStreak,
    ]);
    for strand in 0..7 {
        for segment in 0..5 {
            all.insert(StatePiece::Lattice { strand, segment });
        }
    }
    all
}

fn shell_fixed_pieces() -> BTreeSet<StatePiece> {
    BTreeSet::from([
        StatePiece::Shell(ShellQuad::Teal),
        StatePiece::Shell(ShellQuad::Yellow),
        StatePiece::Shell(ShellQuad::Gold),
    ])
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

/// A point's place in the camera's view: `(x / depth, y / depth)` and its
/// depth.
fn project(cam: &GlobalTransform, p: Vec3) -> (Vec2, f32) {
    let v = cam.compute_matrix().inverse().transform_point3(p);
    (Vec2::new(v.x, v.y) / -v.z, -v.z)
}

// ── onset, anchors, hands ───────────────────────────────────────────────────

/// Power Infusion raises one crackle at the ally's rendered chest, drawing
/// every piece the bench does (the discs, the 35 lattice segments, the onset
/// pair, and a pool of 19 sparks, more than the 12 it can hold alive), each
/// on the one unit quad; flashes the Priest's instant Holy hands, both of
/// them, and nothing on the ally; and is tagged for the match's teardown.
#[test]
fn power_infusion_raises_one_crackle_at_the_chest() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    // The rendered body bobbed above and beside the sim transform.
    let body_offset = Vec3::new(0.1, 0.2, -0.05);
    let body = h.app.world().get::<Children>(ally).unwrap()[0];
    h.app
        .world_mut()
        .get_mut::<Transform>(body)
        .unwrap()
        .translation = body_offset;
    h.tick(2);
    assert!(h.rigs().is_empty(), "nothing before the buff");
    h.infuse(ally, priest);
    h.tick(1);
    let rig = h.rig(DisciplineState::InfusionCrackle);
    assert_eq!(rig.bearer, ally);
    assert!(
        h.app.world().get::<PlayMatchEntity>(rig.entity).is_some(),
        "the rig is a PlayMatchEntity"
    );
    let centre = Vec3::new(2.0, 1.0, 1.0) + body_offset;
    let chest = feet(centre) + Vec3::Y * 1.45;
    assert!((rig.at - chest).length() < 1e-5, "{} vs {}", rig.at, chest);

    let pieces = h.pieces();
    let fixed: BTreeSet<StatePiece> = pieces
        .iter()
        .filter(|d| !matches!(d.piece, StatePiece::Particle { .. }))
        .map(|d| d.piece)
        .collect();
    assert_eq!(fixed, infusion_fixed_pieces());
    let sparks: BTreeSet<StatePiece> = pieces
        .iter()
        .filter(|d| matches!(d.piece, StatePiece::Particle { .. }))
        .map(|d| d.piece)
        .collect();
    let expected: BTreeSet<StatePiece> = (0..19)
        .map(|slot| StatePiece::Particle {
            layer: StateLayer::InfusionSparks,
            slot,
        })
        .collect();
    assert_eq!(sparks, expected);
    assert_eq!(pieces.len(), 40 + 19);
    assert!(pieces.iter().all(|d| d.rig == rig.entity));

    // One mesh, a unit quad, read back.
    let meshes = h.app.world().resource::<Assets<Mesh>>();
    let handles: BTreeSet<_> = pieces.iter().map(|d| d.mesh.id()).collect();
    assert_eq!(handles.len(), 1, "every piece draws the one quad");
    let aabb = meshes.get(&pieces[0].mesh).unwrap().compute_aabb().unwrap();
    assert!((Vec3::from(aabb.half_extents) - Vec3::new(0.5, 0.5, 0.0)).length() < 1e-6);

    let hands = h.hands();
    assert_eq!(
        hands,
        vec![
            (HealCastKind::HolyFlash, priest),
            (HealCastKind::HolyFlash, priest)
        ]
    );
    assert_eq!(
        h.count::<SuppressionSwirl>(),
        0,
        "no swirl for Power Infusion"
    );
    // No more hands while it holds.
    h.tick(20);
    assert_eq!(
        h.hands(),
        vec![
            (HealCastKind::HolyFlash, priest),
            (HealCastKind::HolyFlash, priest)
        ],
        "still the one pair, mid-flash"
    );
}

/// Pain Suppression raises one shell 1.3 yd above the ally's rendered feet,
/// its quads 0.4 yd along the ally's facing, with every piece the bench
/// draws (three quads, pools of 18 bubbles and 56 streaks, each streak with
/// its tail); flashes the Priest's hands and raises the swirl at the
/// Priest's rendered feet.
#[test]
fn pain_suppression_raises_one_shell_and_a_swirl_at_the_priest() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    // The ally faces along +X, turned a quarter from +Z.
    h.app
        .world_mut()
        .get_mut::<Transform>(ally)
        .unwrap()
        .rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let priest_body = h.app.world().get::<Children>(priest).unwrap()[0];
    let priest_offset = Vec3::new(0.0, 0.15, 0.1);
    h.app
        .world_mut()
        .get_mut::<Transform>(priest_body)
        .unwrap()
        .translation = priest_offset;
    h.suppress(ally, priest);
    h.tick(1);
    let rig = h.rig(DisciplineState::SuppressionShell);
    assert_eq!(rig.bearer, ally);
    assert!(h.app.world().get::<PlayMatchEntity>(rig.entity).is_some());
    let anchor = feet(Vec3::new(2.0, 1.0, 1.0)) + Vec3::Y * 1.3;
    assert!((rig.at - anchor).length() < 1e-5, "{}", rig.at);

    let pieces = h.pieces();
    let mine: Vec<&Drawn> = pieces.iter().filter(|d| d.rig == rig.entity).collect();
    let fixed: BTreeSet<StatePiece> = mine
        .iter()
        .filter(|d| matches!(d.piece, StatePiece::Shell(_)))
        .map(|d| d.piece)
        .collect();
    assert_eq!(fixed, shell_fixed_pieces());
    let per = |f: &dyn Fn(StatePiece) -> bool| mine.iter().filter(|d| f(d.piece)).count();
    assert_eq!(
        per(&|p| matches!(
            p,
            StatePiece::Particle {
                layer: StateLayer::SuppressionBubbles,
                ..
            }
        )),
        18
    );
    assert_eq!(
        per(&|p| matches!(
            p,
            StatePiece::Particle {
                layer: StateLayer::SuppressionStreaks,
                ..
            }
        )),
        56
    );
    assert_eq!(per(&|p| matches!(p, StatePiece::Tail { .. })), 56);
    assert_eq!(mine.len(), 3 + 18 + 56 + 56);
    // No camera: nothing is held, so the quads stand where they truly do,
    // along the ally's facing (+X).
    for d in mine
        .iter()
        .filter(|d| matches!(d.piece, StatePiece::Shell(_)))
    {
        let want = anchor + Vec3::X * 0.4;
        assert!((d.at - want).length() < 1e-5, "{:?} at {}", d.piece, d.at);
    }

    let swirls = h.swirls();
    assert_eq!(swirls.len(), 1);
    let (swirl, caster, _, at) = swirls[0];
    assert_eq!(caster, priest);
    assert!(h.app.world().get::<PlayMatchEntity>(swirl).is_some());
    let priest_feet = feet(Vec3::new(-3.0, 1.0, 0.0) + priest_offset);
    assert!((at - priest_feet).length() < 1e-5, "{at}");
    let swirl_pieces: BTreeSet<StatePiece> = pieces
        .iter()
        .filter(|d| d.rig == swirl)
        .map(|d| d.piece)
        .collect();
    assert_eq!(
        swirl_pieces,
        BTreeSet::from([StatePiece::SwirlRing, StatePiece::SwirlGlow])
    );
    assert_eq!(
        h.hands(),
        vec![
            (HealCastKind::HolyFlash, priest),
            (HealCastKind::HolyFlash, priest)
        ]
    );
}

/// The Priest may infuse itself (its heals, while a teammate is dying): the
/// crackle and the hands are both on the Priest.
#[test]
fn a_self_infusion_wears_its_own_crackle() {
    let mut h = Harness::new();
    let (priest, _) = h.pair();
    h.infuse(priest, priest);
    h.tick(1);
    assert_eq!(h.rig(DisciplineState::InfusionCrackle).bearer, priest);
    assert_eq!(h.hands().len(), 2);
    assert!(h.hands().iter().all(|(_, c)| *c == priest));
}

/// Neither buff is a cast with a cast state, but a `Landed` marker naming
/// one (the animation sandbox's path for an instant) flashes no hands: the
/// state's onset is the one place they flash, so a cast flashes them once.
#[test]
fn a_landed_marker_flashes_no_hands_for_either() {
    for ability in [AbilityType::PowerInfusion, AbilityType::PainSuppression] {
        let mut h = Harness::new();
        let (priest, _) = h.pair();
        h.app.world_mut().spawn((
            CastEnding {
                caster: priest,
                kind: CastEndingKind::Landed,
            },
            LandedCast { ability },
        ));
        h.tick(3);
        assert!(h.hands().is_empty(), "{ability:?}");
        assert!(h.rigs().is_empty());
    }
}

// ── the fixed pieces against the bench ──────────────────────────────────────

/// Every fixed piece of the crackle, every frame of the 15 s buff, against
/// the bench: colour, alpha and size, standing at the chest. No camera, so
/// nothing is held or turned. The clock is the rig's own age, which runs one
/// frame a frame from 0 at the onset. Non-vacuous: the onset's star and
/// streak are lit, the lattice is both lit and dark, and the discs throb
/// through their range.
#[test]
fn every_crackle_piece_draws_what_the_bench_draws() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.infuse(ally, priest);
    h.tick(1);
    let chest = h.rig(DisciplineState::InfusionCrackle).at;
    let mut frames = 0;
    let (mut onset_lit, mut lattice_lit, mut lattice_dark) = (0, 0, 0);
    let (mut throb_lo, mut throb_hi) = (f32::MAX, f32::MIN);
    let mut pulse = BTreeSet::new();
    while h.remaining(ally, POWER_INFUSION_AURA).is_some() {
        let rig = h.rig(DisciplineState::InfusionCrackle);
        assert!(
            (rig.age - frames as f32 * DT).abs() < 1e-3,
            "the clock runs a frame a frame: {} at frame {frames}",
            rig.age
        );
        let ms = rig.age as f64 * 1000.0;
        for d in h.pieces() {
            if matches!(d.piece, StatePiece::Particle { .. }) {
                continue;
            }
            let b = bench_infusion(d.piece, ms).unwrap();
            let what = format!("{:?} at {ms} ms", d.piece);
            assert!(
                (d.alpha as f64 - b.alpha).abs() < 2e-3,
                "{what}: alpha {} vs {}",
                d.alpha,
                b.alpha
            );
            if d.alpha > 0.01 {
                assert!(
                    rgb_matches(d.rgb, b.rgb),
                    "{what}: {:?} vs {:?}",
                    d.rgb,
                    b.rgb
                );
            }
            if let StatePiece::Lattice { .. } = d.piece {
                if b.alpha > 0.0 {
                    lattice_lit += 1;
                } else {
                    lattice_dark += 1;
                }
                continue;
            }
            assert!((d.at - chest).length() < 1e-5, "{what}: off the chest");
            assert!(
                (d.scale.x as f64 - b.size[0]).abs() < 1e-3
                    && (d.scale.y as f64 - b.size[1]).abs() < 1e-3,
                "{what}: size {} vs {:?}",
                d.scale,
                b.size
            );
            match d.piece {
                StatePiece::OnsetStar | StatePiece::OnsetStreak if b.alpha > 0.05 => onset_lit += 1,
                StatePiece::MidDisc => {
                    throb_lo = throb_lo.min(d.alpha);
                    throb_hi = throb_hi.max(d.alpha);
                }
                StatePiece::WideDisc => {
                    pulse.insert((d.scale.x * 100.0).round() as i32);
                }
                _ => {}
            }
        }
        h.step();
        frames += 1;
    }
    assert_eq!(frames, 937, "the whole 15 s buff");
    assert!(onset_lit > 100, "the onset draws: {onset_lit}");
    assert!(lattice_lit > 5000 && lattice_dark > 1000);
    assert!(throb_lo < 0.07 && throb_hi > 0.29, "{throb_lo}..{throb_hi}");
    assert!(pulse.len() > 20, "the wide disc pulses");
}

/// The onset's star and streak draw only in the first 1.334 s, and a
/// refresh does not replay them.
#[test]
fn the_onset_flashes_once() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.infuse(ally, priest);
    let lit = |h: &mut Harness| {
        h.pieces()
            .iter()
            .filter(|d| matches!(d.piece, StatePiece::OnsetStar | StatePiece::OnsetStreak))
            .map(|d| d.alpha)
            .fold(0.0_f32, f32::max)
    };
    h.tick(1);
    assert_eq!(lit(&mut h), 0.0, "rising from nothing");
    h.tick(41);
    assert!(lit(&mut h) > 0.85, "at its height mid-onset");
    h.tick(41);
    assert!(lit(&mut h) > 0.0, "still in it at 1.31 s");
    h.tick(2);
    assert_eq!(lit(&mut h), 0.0, "gone by 1.34 s");
    for a in h.auras(ally).iter_mut() {
        a.duration = 15.0;
    }
    for _ in 0..90 {
        h.tick(1);
        assert_eq!(lit(&mut h), 0.0, "a refresh does not replay the onset");
    }
}

/// The lattice's segments join its strands' points, as the bench draws them
/// on screen: seen from a camera, each drawn segment's two ends land on the
/// bench's two points, re-jittered every 67 ms.
#[test]
fn the_lattice_joins_the_benchs_points() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    let target = Vec3::new(2.0, 1.45, 1.0);
    let cam = h.camera(match_camera(target, 20.0, 0.9, 0.6));
    h.infuse(ally, priest);
    h.tick(1);
    let chest = h.rig(DisciplineState::InfusionCrackle).at;
    let mut checked = 0;
    let mut layouts = BTreeSet::new();
    for _ in 0..140 {
        h.tick(1);
        let ms = h.rig(DisciplineState::InfusionCrackle).age as f64 * 1000.0;
        if ((ms / 67.0) - (ms / 67.0).round()).abs() < 1e-3 {
            continue; // on a jitter step's edge: either side is the bench
        }
        layouts.insert((ms / 67.0).floor() as i64);
        let view = *h.app.world().get::<GlobalTransform>(cam).unwrap();
        for d in h.pieces() {
            let StatePiece::Lattice { strand, segment } = d.piece else {
                continue;
            };
            let half = d.rotation * Vec3::X * (d.scale.x * 0.5);
            let ends = [d.at - half, d.at + half];
            let p = |q: [f64; 3]| chest + Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32);
            let want = [
                p(bench_lattice_point(strand, segment, ms)),
                p(bench_lattice_point(strand, segment + 1, ms)),
            ];
            let (_, depth) = project(&view, chest);
            let screen = |q: Vec3| project(&view, q).0 * depth;
            let (e0, e1) = (screen(ends[0]), screen(ends[1]));
            let (w0, w1) = (screen(want[0]), screen(want[1]));
            let err = ((e0 - w0).length() + (e1 - w1).length())
                .min((e0 - w1).length() + (e1 - w0).length());
            assert!(err < 2e-3, "{:?} at {ms} ms: {err}", d.piece);
            // The stroke is 0.07 yd wide, held.
            assert!(d.scale.y > 0.06 && d.scale.y < 0.071, "{}", d.scale.y);
            checked += 1;
        }
    }
    assert!(
        layouts.len() > 30,
        "the lattice re-jitters: {}",
        layouts.len()
    );
    assert!(checked > 4000, "{checked}");
}

/// Every shell quad, every frame of the 8 s buff, against the bench: colour,
/// alpha, size (its squash) and its stand 0.4 yd along the ally's facing,
/// whichever way the ally turns mid-buff. Non-vacuous: the onset reaches
/// full, and the loop flickers through 0.05-0.4.
#[test]
fn every_shell_quad_draws_what_the_bench_draws() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.suppress(ally, priest);
    h.tick(1);
    let anchor = h.rig(DisciplineState::SuppressionShell).at;
    let (mut peak, mut lo, mut hi) = (0.0_f64, f64::MAX, f64::MIN);
    let mut frames = 0;
    let mut checked = 0;
    while h.remaining(ally, PAIN_SUPPRESSION_AURA).is_some() {
        let turn = frames as f32 * 0.05;
        h.app
            .world_mut()
            .get_mut::<Transform>(ally)
            .unwrap()
            .rotation = Quat::from_rotation_y(turn);
        h.step();
        frames += 1;
        if h.remaining(ally, PAIN_SUPPRESSION_AURA).is_none() {
            assert!(h.rigs().is_empty(), "gone with its buff");
            break;
        }
        let rig = h.rig(DisciplineState::SuppressionShell);
        let ms = rig.age as f64 * 1000.0;
        let fwd = [turn.sin() as f64, turn.cos() as f64];
        for d in h.pieces() {
            let StatePiece::Shell(quad) = d.piece else {
                continue;
            };
            let b = bench_shell(quad, ms, fwd);
            let what = format!("{quad:?} at {ms} ms");
            assert!(
                (d.alpha as f64 - b.alpha).abs() < 2e-3,
                "{what}: {} vs {}",
                d.alpha,
                b.alpha
            );
            assert!(rgb_matches(d.rgb, b.rgb), "{what}: {:?}", d.rgb);
            assert!(
                (d.scale.x as f64 - b.size[0]).abs() < 1e-3
                    && (d.scale.y as f64 - b.size[1]).abs() < 2e-3,
                "{what}: {} vs {:?}",
                d.scale,
                b.size
            );
            let want = anchor + Vec3::new(b.at[0] as f32, 0.0, b.at[2] as f32);
            assert!(
                (d.at - want).length() < 1e-4,
                "{what}: at {} vs {want}",
                d.at
            );
            let w = b.alpha / 0.55;
            if ms < 867.0 {
                peak = peak.max(w);
            } else {
                lo = lo.min(w);
                hi = hi.max(w);
            }
            checked += 1;
        }
    }
    assert!(peak > 0.97, "the onset flashes to full: {peak}");
    assert!(lo < 0.06 && hi > 0.39, "the loop flickers: {lo}..{hi}");
    assert_eq!(checked, 3 * (frames - 1));
    assert!(frames > 490);
}

/// The swirl at the Priest's feet, every frame of its life, against the
/// bench: the ring's colour (orange -> gold -> violet), alpha and radius
/// (read through the ring sprite's stroke, measured off the texture), and the
/// ground glow beneath it, both level; and it is gone once the bench stops
/// drawing it.
#[test]
fn the_swirl_draws_what_the_bench_draws() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.suppress(ally, priest);
    h.tick(1);
    let (_, _, _, at) = h.swirls()[0];
    // Where the ring sprite's stroke peaks, as a fraction of its half-width.
    let ring_peak = {
        let d = h
            .pieces()
            .into_iter()
            .find(|d| d.piece == StatePiece::SwirlRing)
            .unwrap();
        let tex = d.material.base_color_texture.clone().unwrap();
        let images = h.app.world().resource::<Assets<Image>>();
        let image = images.get(&tex).unwrap();
        let px = image.width() as usize;
        let data = image.data.as_ref().unwrap();
        let row = px / 2;
        (px / 2..px)
            .max_by_key(|&x| data[(row * px + x) * 4 + 3])
            .map(|x| (x as f32 + 0.5 - px as f32 / 2.0) / (px as f32 / 2.0))
            .unwrap()
    };
    assert!(ring_peak > 0.9 && ring_peak < 1.0, "{ring_peak}");
    let mut frames = 0;
    let mut gold = false;
    let mut violet = false;
    loop {
        let swirls = h.swirls();
        let Some(&(swirl, _, age, _)) = swirls.first() else {
            break;
        };
        let ms = age as f64 * 1000.0;
        for d in h.pieces().into_iter().filter(|d| d.rig == swirl) {
            let b = bench_swirl(d.piece, ms).unwrap_or_else(|| panic!("drawn at {ms} ms"));
            let what = format!("{:?} at {ms} ms", d.piece);
            assert!(
                (d.alpha as f64 - b.alpha).abs() < 2e-3,
                "{what}: {}",
                d.alpha
            );
            if d.alpha > 0.01 {
                assert!(
                    rgb_matches(d.rgb, b.rgb),
                    "{what}: {:?} vs {:?}",
                    d.rgb,
                    b.rgb
                );
            }
            let want = at + Vec3::new(0.0, b.at[1] as f32, 0.0);
            assert!((d.at - want).length() < 1e-5, "{what}");
            // Level: the quad faces up.
            assert!(
                (d.rotation * Vec3::Z).dot(Vec3::Y) > 0.9999,
                "{what}: not level"
            );
            let radius = match d.piece {
                StatePiece::SwirlRing => d.scale.x * 0.5 * ring_peak,
                _ => d.scale.x * 0.5,
            };
            let want_r = match d.piece {
                StatePiece::SwirlRing => b.size[0],
                _ => b.size[0] / 2.0,
            };
            assert!(
                (radius as f64 - want_r).abs() < 0.02,
                "{what}: {radius} vs {want_r}"
            );
            if d.piece == StatePiece::SwirlRing && d.alpha > 0.5 {
                gold |= d.rgb[1] > 200 && d.rgb[2] < 30;
                violet |= d.rgb[2] > 120;
            }
        }
        h.tick(1);
        frames += 1;
        assert!(frames < 80, "the swirl ends");
    }
    assert_eq!(frames, 67, "drawn while the bench draws it (to 1067 ms)");
    assert!(gold && violet, "orange to gold to violet");
}

// ── particles ───────────────────────────────────────────────────────────────

/// Every lit particle, every frame of both buffs, against the bench's
/// emitter records run through its own accumulator: per emitter, the drawn
/// set equals the bench's live set in number, colour (to its truncated
/// byte), alpha and size. Each streak's tail lies back along its flight,
/// 0.24 yd long at 0.85 of its alpha and 0.7 of its radius wide. (The two
/// buffs' 8 s and 15 s both outlast the 7.2 s compared.)
#[test]
fn every_particle_draws_what_the_bench_draws() {
    let emitters = bench_emitters();
    let streak = &emitters[2];
    assert_eq!(streak.layer, StateLayer::SuppressionStreaks);
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    h.tick(1);
    let mut sims: Vec<BenchSim> = emitters.iter().map(|_| BenchSim::default()).collect();
    let mut compared = [0usize; 3];
    let mut tails = 0;
    let mut behind = 0;
    for _ in 0..450 {
        h.step();
        let anchor = h.rig(DisciplineState::SuppressionShell).at;
        for (e, sim) in emitters.iter().zip(sims.iter_mut()) {
            sim.step(e, DT);
        }
        let pieces = h.pieces();
        for (i, e) in emitters.iter().enumerate() {
            let mut drawn: Vec<&Drawn> = pieces
                .iter()
                .filter(|d| {
                    matches!(d.piece, StatePiece::Particle { layer, .. } if layer == e.layer)
                        && d.alpha > 1e-3
                })
                .collect();
            let mut want: Vec<([f64; 3], f64, f64)> = sims[i]
                .ages
                .iter()
                .map(|a| bench_look(e, *a))
                .filter(|w| w.1 > 1e-3)
                .collect();
            assert_eq!(drawn.len(), want.len(), "{:?}: live count", e.layer);
            drawn.sort_by(|a, b| a.scale.x.total_cmp(&b.scale.x));
            want.sort_by(|a, b| a.2.total_cmp(&b.2));
            for (d, w) in drawn.iter().zip(&want) {
                assert!((d.scale.x as f64 - w.2).abs() < 2e-3, "{:?} size", e.layer);
                assert!((d.alpha as f64 - w.1).abs() < 2e-3, "{:?} alpha", e.layer);
                assert!(
                    rgb_matches(d.rgb, w.0),
                    "{:?}: {:?} vs {:?}",
                    e.layer,
                    d.rgb,
                    w.0
                );
                compared[i] += 1;
            }
        }
        // Tails: one per lit streak, against its own particle.
        let streaks: BTreeMap<u16, &Drawn> = pieces
            .iter()
            .filter_map(|d| match d.piece {
                StatePiece::Particle {
                    layer: StateLayer::SuppressionStreaks,
                    slot,
                } => Some((slot, d)),
                _ => None,
            })
            .collect();
        for d in &pieces {
            let StatePiece::Tail { slot, .. } = d.piece else {
                continue;
            };
            let p = streaks[&slot];
            if p.alpha <= 1e-3 {
                assert!(d.alpha <= 1e-3, "no tail without its streak");
                continue;
            }
            assert!((d.alpha - p.alpha * 0.85).abs() < 2e-3);
            assert_eq!(d.rgb, p.rgb);
            assert!(
                (d.scale.x as f64 - streak.tail_length()).abs() < 1e-3,
                "tail length {}",
                d.scale.x
            );
            assert!((d.scale.y - 0.7 * p.scale.x * 0.5).abs() < 1e-4);
            // Behind the streak along its flight. No camera, so the tail is
            // the segment itself: its centre half its length back. A young
            // streak (radius above 0.0385) is still flying in toward the
            // anchor, so behind it is away from the anchor.
            if p.scale.x > 0.077 {
                let back = (p.at - anchor).normalize();
                let want = p.at + back * (streak.tail_length() as f32 * 0.5);
                assert!((d.at - want).length() < 1e-3, "tail behind its streak");
                behind += 1;
            }
            tails += 1;
        }
    }
    assert!(compared[0] > 3000, "sparks: {}", compared[0]);
    assert!(compared[1] > 900, "bubbles: {}", compared[1]);
    assert!(compared[2] > 15000, "streaks: {}", compared[2]);
    assert!(tails > 15000, "{tails}");
    assert!(behind > 8000, "{behind}");
}

/// Track each young particle of `layer` from one frame to the next (paired by
/// slot, its age read off its own size), returning `(place, step)` pairs
/// from the anchor, over `frames` frames.
fn flights(
    h: &mut Harness,
    state: DisciplineState,
    layer: StateLayer,
    frames: u32,
    young: f32,
) -> (Vec<(Vec3, Vec3)>, Vec<Vec3>) {
    let mut last: BTreeMap<u16, (Vec3, f32)> = BTreeMap::new();
    let mut steps = Vec::new();
    let mut births = Vec::new();
    for _ in 0..frames {
        h.step();
        let anchor = h.rig(state).at;
        let mut now = BTreeMap::new();
        for d in h.pieces() {
            let StatePiece::Particle { layer: l, slot } = d.piece else {
                continue;
            };
            if l != layer || d.alpha <= 0.0 && layer != StateLayer::SuppressionStreaks {
                continue;
            }
            if d.scale.x <= 1e-3 {
                continue;
            }
            let at = d.at - anchor;
            now.insert(slot, (at, d.scale.x));
            if let Some((was, size)) = last.get(&slot) {
                // The same particle a frame on, still young: its size has moved
                // by one frame's worth along its track, not reset.
                let continued = match layer {
                    StateLayer::SuppressionBubbles => d.scale.x > *size,
                    _ => d.scale.x < *size,
                };
                if continued && d.scale.x > young {
                    steps.push((*was, at - *was));
                }
            } else if !last.contains_key(&slot) {
                births.push(at);
            }
        }
        // A slot re-born this frame counts as a birth.
        for (slot, (at, size)) in &now {
            if let Some((_, was)) = last.get(slot) {
                let reborn = match layer {
                    StateLayer::SuppressionBubbles => size < was,
                    _ => size > was,
                };
                if reborn {
                    births.push(*at);
                }
            }
        }
        last = now;
    }
    (steps, births)
}

/// The sparks fly OUT of the chest at 3.3 yd/s, the bubbles drift at 0.15
/// yd/s, and the streaks are pulled IN through the shell's anchor at 4.7
/// yd/s. No camera, so every place is true.
#[test]
fn sparks_fly_out_bubbles_drift_and_streaks_are_drawn_in() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    h.tick(1);
    let (sparks, _) = flights(
        &mut h,
        DisciplineState::InfusionCrackle,
        StateLayer::InfusionSparks,
        150,
        0.3,
    );
    assert!(sparks.len() > 300, "{}", sparks.len());
    for (at, step) in &sparks {
        assert!((step.length() - speed(StateLayer::InfusionSparks) * DT).abs() < 1e-4);
        assert!(step.dot(*at) > 0.0, "a spark flies out");
        // Born at the chest itself, it flies straight along its line.
        assert!(at.normalize().dot(step.normalize()) > 0.9999);
    }
    let (bubbles, _) = flights(
        &mut h,
        DisciplineState::SuppressionShell,
        StateLayer::SuppressionBubbles,
        150,
        0.0,
    );
    assert!(bubbles.len() > 200, "{}", bubbles.len());
    for (_, step) in &bubbles {
        assert!(
            (step.length() - speed(StateLayer::SuppressionBubbles) * DT).abs() < 1e-4,
            "{}",
            step.length()
        );
    }
    let (streaks, _) = flights(
        &mut h,
        DisciplineState::SuppressionShell,
        StateLayer::SuppressionStreaks,
        150,
        // Young: under 0.26 s old (its radius above 0.0385), before even a
        // streak born on the shell's inner wall (1.26 yd) can reach the
        // anchor at 4.7 yd/s. (Each passes through it later in its life.)
        0.077,
    );
    assert!(streaks.len() > 3000, "{}", streaks.len());
    let mut inward = 0;
    for (at, step) in &streaks {
        assert!((step.length() - speed(StateLayer::SuppressionStreaks) * DT).abs() < 1e-4);
        // Straight in along its line through the anchor: the step is
        // antiparallel to the place until it passes through.
        if at.length() < 0.05 {
            continue;
        }
        assert!(at.normalize().dot(step.normalize()).abs() > 0.9999);
        if at.length() > 0.2 {
            assert!(
                step.dot(*at) < 0.0,
                "a streak flies in: at {at}, step {step}"
            );
            inward += 1;
        }
    }
    assert!(inward > 2000);
}

/// The births fill their areas, as the bench's do: the bubbles' box (1.0 yd
/// across, 1.2 yd tall, round the anchor) reaches its walls on every side,
/// and the streaks' shell runs 1.26-2.1 yd out (2.1 × 0.6-1.0), stretched
/// 1.3 up and down.
#[test]
fn births_fill_their_areas() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    let mut bubbles: Vec<Vec3> = Vec::new();
    let mut streak_r: Vec<f32> = Vec::new();
    let mut streak_up: Vec<f32> = Vec::new();
    // Many buffs, back to back: each a fresh rig with fresh births.
    for _ in 0..4 {
        h.suppress(ally, priest);
        h.tick(1);
        for _ in 0..490 {
            h.step();
            let Some(anchor) = h
                .rigs()
                .iter()
                .find(|r| r.state == DisciplineState::SuppressionShell)
                .map(|r| r.at)
            else {
                break;
            };
            for d in h.pieces() {
                let StatePiece::Particle { layer, .. } = d.piece else {
                    continue;
                };
                let at = d.at - anchor;
                match layer {
                    // One frame old: a bubble's radius is 0.2 + 0.9 × 0.016/1.6.
                    StateLayer::SuppressionBubbles if (d.scale.x - 0.418).abs() < 1e-3 => {
                        bubbles.push(at)
                    }
                    // One frame old: a streak's radius is 0.05 - 0.02 × 0.016/0.45.
                    StateLayer::SuppressionStreaks
                        if (d.scale.x - (0.1 - 0.04 * 0.016 / 0.45)).abs() < 1e-5 =>
                    {
                        let r = at.length() + 4.7 * DT;
                        streak_r.push(r);
                        streak_up.push(at.normalize().y);
                    }
                    _ => {}
                }
            }
        }
        h.tick(10);
    }
    assert!(bubbles.len() > 250, "{}", bubbles.len());
    let reach = |f: &dyn Fn(Vec3) -> f32| {
        let v: Vec<f32> = bubbles.iter().map(|b| f(*b)).collect();
        (
            v.iter().cloned().fold(f32::MAX, f32::min),
            v.iter().cloned().fold(f32::MIN, f32::max),
        )
    };
    let drift = 0.15 * DT + 1e-4;
    for (axis, half) in [(0usize, 0.5_f32), (1, 0.6), (2, 0.5)] {
        let (lo, hi) = reach(&|b| b[axis]);
        assert!(
            lo >= -half - drift && hi <= half + drift,
            "axis {axis}: {lo}..{hi}"
        );
        assert!(
            lo < -0.9 * half && hi > 0.9 * half,
            "axis {axis} fills: {lo}..{hi}"
        );
    }
    assert!(streak_r.len() > 2500, "{}", streak_r.len());
    let lo = streak_r.iter().cloned().fold(f32::MAX, f32::min);
    let hi = streak_r.iter().cloned().fold(f32::MIN, f32::max);
    assert!(lo >= 2.1 * 0.6 - 1e-3 && hi <= 2.1 + 1e-3, "{lo}..{hi}");
    assert!(lo < 1.3 && hi > 2.05, "the shell fills: {lo}..{hi}");
    // Stretched up and down: more of the shell lies toward the poles than
    // an unstretched sphere puts there (a third of directions beyond
    // |y| = 2/3 on a sphere of uniform cube points normalised is less).
    let polar = streak_up.iter().filter(|y| y.abs() > 0.8).count() as f32 / streak_up.len() as f32;
    let (min_up, max_up) = (
        streak_up.iter().cloned().fold(f32::MAX, f32::min),
        streak_up.iter().cloned().fold(f32::MIN, f32::max),
    );
    assert!(min_up < -0.95 && max_up > 0.95, "{min_up}..{max_up}");
    assert!(polar > 0.2, "stretched 1.3 up and down: {polar}");
}

// ── blend, sprites ──────────────────────────────────────────────────────────

/// How the bench composites each piece: `bbGlow`, `ring3`, `groundDisc`, the
/// lattice's own `'lighter'`, and three emitters with no `blend` (which
/// `Emitter.draw` composites `'lighter'`). Every one of them adds.
fn bench_blend(piece: StatePiece) -> AlphaMode {
    match piece {
        StatePiece::WideDisc
        | StatePiece::MidDisc
        | StatePiece::CoreDisc
        | StatePiece::OnsetStar
        | StatePiece::OnsetStreak
        | StatePiece::Shell(_) => AlphaMode::Add, // bbGlow
        StatePiece::Lattice { .. } => AlphaMode::Add, // globalCompositeOperation='lighter'
        StatePiece::Particle { .. } | StatePiece::Tail { .. } => AlphaMode::Add, // no blend
        StatePiece::SwirlRing | StatePiece::SwirlGlow => AlphaMode::Add, // ring3, groundDisc
    }
}

/// Every drawn piece composites as its bench call does, unlit (the bench
/// adds colour × sprite × alpha and nothing else), both faces, casting no
/// shadow.
#[test]
fn every_piece_blends_as_the_bench_draws_it() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    h.tick(2);
    let pieces = h.pieces();
    assert_eq!(pieces.len(), 59 + 133 + 2);
    for d in &pieces {
        assert_eq!(
            format!("{:?}", d.material.alpha_mode),
            format!("{:?}", bench_blend(d.piece)),
            "{:?}",
            d.piece
        );
        assert!(d.material.unlit, "{:?}", d.piece);
        assert!(d.material.cull_mode.is_none());
    }
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<(), (With<DisciplineStatePiece>, With<bevy::pbr::NotShadowCaster>)>();
    assert_eq!(q.iter(h.app.world()).count(), pieces.len());
}

/// Each piece's sprite, read back: the bench's `bbGlow` gradient (1 at the
/// centre, 0.45 at half the radius, 0 at the rim) on the discs, the onset
/// and the shell; its `sprite('glow')` (0.45 at 0.45 of the radius) on the
/// bubbles and streaks; a four-point star on the sparks; a band falling off
/// across its width on the lattice and the tails; a thin ring at the rim on
/// the swirl's ring; and the `groundDisc` gradient (0.55 at 0.55) under it.
#[test]
fn every_sprite_is_the_benchs() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    h.tick(2);
    let pieces = h.pieces();
    let images = h.app.world().resource::<Assets<Image>>();
    let alpha_at = |d: &Drawn, x: f32, y: f32| -> f32 {
        let image = images
            .get(d.material.base_color_texture.as_ref().unwrap())
            .unwrap();
        let (w, hgt) = (image.width() as usize, image.height() as usize);
        let col = (((x + 1.0) / 2.0) * w as f32).min(w as f32 - 1.0) as usize;
        let row = (((1.0 - y) / 2.0) * hgt as f32).min(hgt as f32 - 1.0) as usize;
        image.data.as_ref().unwrap()[(row * w + col) * 4 + 3] as f32 / 255.0
    };
    let mut kinds = BTreeSet::new();
    for d in &pieces {
        let kind = match d.piece {
            StatePiece::WideDisc
            | StatePiece::MidDisc
            | StatePiece::CoreDisc
            | StatePiece::OnsetStar
            | StatePiece::OnsetStreak
            | StatePiece::Shell(_) => {
                assert!(alpha_at(d, 0.0, 0.0) > 0.97, "{:?}", d.piece);
                assert!((alpha_at(d, 0.5, 0.0) - 0.45).abs() < 0.04, "{:?}", d.piece);
                assert!(alpha_at(d, 0.0, -0.995) < 0.03);
                "panel"
            }
            StatePiece::Particle {
                layer: StateLayer::InfusionSparks,
                ..
            } => {
                // Rays along the axes reach far; the diagonals are dark.
                assert!(alpha_at(d, 0.6, 0.0) > 0.1 && alpha_at(d, 0.0, 0.6) > 0.1);
                assert!(alpha_at(d, 0.6, 0.6) < 0.02);
                "star"
            }
            StatePiece::Particle { .. } => {
                assert!(alpha_at(d, 0.0, 0.0) > 0.97);
                assert!(
                    (alpha_at(d, 0.45, 0.0) - 0.45).abs() < 0.04,
                    "{:?}",
                    d.piece
                );
                assert!(alpha_at(d, 0.0, 0.995) < 0.03);
                "glow"
            }
            StatePiece::Lattice { .. } | StatePiece::Tail { .. } => {
                assert!(alpha_at(d, 0.0, 0.0) > 0.95);
                assert!((alpha_at(d, 0.9, 0.0) - alpha_at(d, -0.9, 0.0)).abs() < 0.02);
                assert!(alpha_at(d, 0.0, 0.97) < 0.1, "soft across its width");
                "band"
            }
            StatePiece::SwirlRing => {
                assert!(alpha_at(d, 0.0, 0.0) < 0.01, "hollow");
                assert!(alpha_at(d, 0.5, 0.0) < 0.01);
                "ring"
            }
            StatePiece::SwirlGlow => {
                assert!(alpha_at(d, 0.0, 0.0) > 0.97);
                assert!((alpha_at(d, 0.55, 0.0) - 0.55).abs() < 0.04);
                "ground"
            }
        };
        kinds.insert(kind);
    }
    assert_eq!(
        kinds,
        BTreeSet::from(["panel", "star", "glow", "band", "ring", "ground"])
    );
}

// ── draw order and the hold ─────────────────────────────────────────────────

/// At the match camera, over the pitches it allows (its own 38.7° among
/// them), every yaw, the ally facing every way and the body's animation
/// offsets, with a Power Word: Shield on the ally: every piece of both states
/// and of the swirl sorts in front of its blended body, and in front of the
/// shield's blended outline and tint, as the bench draws them over the body.
/// The hold changes nothing on screen: every held piece projects exactly
/// where its unheld twin (the same harness without a camera) stands, at
/// exactly the same screen size. Non-vacuous: thousands of pieces are held.
#[test]
fn every_piece_sorts_over_the_body_and_holds_its_screen_place() {
    let pitches = [0.1_f32, 0.4, 38.7_f32.to_radians(), 1.0, 1.3, 1.5];
    let yaws = [0.0_f32, 1.3, 2.9, 4.4];
    let mut checked = 0usize;
    let mut held = 0usize;
    for (i, facing) in [0.0_f32, 1.6, 3.0, 4.7].into_iter().enumerate() {
        let offset = [
            Vec3::ZERO,
            Vec3::new(0.0, 0.22, 0.0),
            Vec3::new(0.18, 0.1, 0.0),
            Vec3::new(0.0, -0.1, -0.18),
        ][i];
        let mut seen = Harness::new();
        let mut truth = Harness::new();
        let mut units = Vec::new();
        for h in [&mut seen, &mut truth] {
            let (priest, ally) = h.pair();
            for unit in [priest, ally] {
                let body = h.app.world().get::<Children>(unit).unwrap()[0];
                h.app
                    .world_mut()
                    .get_mut::<Transform>(body)
                    .unwrap()
                    .translation = offset;
            }
            h.app
                .world_mut()
                .get_mut::<Transform>(ally)
                .unwrap()
                .rotation = Quat::from_rotation_y(facing);
            h.land(AbilityType::PowerWordShield, ally, priest);
            h.infuse(ally, priest);
            h.suppress(ally, priest);
            units.push((priest, ally));
        }
        let cam = seen.camera(Transform::default());
        let target = Vec3::new(2.0, 1.0, 1.0);
        for frame in 0..60_usize {
            let (pitch, yaw) = (
                pitches[frame % pitches.len()],
                yaws[(frame / pitches.len()) % yaws.len()],
            );
            *seen.app.world_mut().get_mut::<Transform>(cam).unwrap() =
                match_camera(target, 60.0, pitch, yaw);
            seen.tick(1);
            truth.tick(1);
            let view = *seen.app.world().get::<GlobalTransform>(cam).unwrap();
            let range = ViewRangefinder3d::from_world_from_view(&view.compute_matrix());
            let world = seen.app.world();
            let body_key = |unit: Entity| {
                let body = world.get::<Children>(unit).unwrap()[0];
                range.distance_translation(
                    &world.get::<GlobalTransform>(body).unwrap().translation(),
                )
            };
            let (priest, ally) = units[0];
            let (ally_key, priest_key) = (body_key(ally), body_key(priest));
            let mut q = seen.app.world_mut().query_filtered::<(
                &GlobalTransform,
                &MeshMaterial3d<StandardMaterial>,
            ), With<ShieldCard>>();
            let world = seen.app.world();
            let materials = world.resource::<Assets<StandardMaterial>>();
            let blended: Vec<f32> = q
                .iter(world)
                .filter_map(|(g, m)| {
                    let m = materials.get(&m.0)?;
                    matches!(m.alpha_mode, AlphaMode::Blend)
                        .then(|| range.distance_translation(&g.translation()) + m.depth_bias)
                })
                .collect();
            assert_eq!(blended.len(), 1, "the shield's blended outline draws");
            let truths: BTreeMap<(String, StatePiece), Drawn> = {
                let rigs = truth.rigs();
                let swirls = truth.swirls();
                truth
                    .pieces()
                    .into_iter()
                    .map(|d| {
                        let owner = rigs
                            .iter()
                            .find(|r| r.entity == d.rig)
                            .map(|r| format!("{:?}", r.state))
                            .or_else(|| {
                                swirls.iter().find(|s| s.0 == d.rig).map(|_| "swirl".into())
                            })
                            .unwrap();
                        ((owner, d.piece), d)
                    })
                    .collect()
            };
            let rigs = seen.rigs();
            let swirls = seen.swirls();
            for d in seen.pieces() {
                if d.alpha <= 0.0 || d.scale.x <= 2e-3 {
                    continue;
                }
                let (owner, body) = match rigs.iter().find(|r| r.entity == d.rig) {
                    Some(r) => (format!("{:?}", r.state), ally_key),
                    None => {
                        assert!(swirls.iter().any(|s| s.0 == d.rig));
                        ("swirl".to_string(), priest_key)
                    }
                };
                let what = format!("{owner} {:?} at frame {frame}, facing {facing}", d.piece);
                let key = range.distance_translation(&d.at) + d.material.depth_bias;
                assert!(key > body, "{what}: sorts behind its body");
                if owner != "swirl" {
                    for shield in &blended {
                        assert!(
                            key > *shield,
                            "{what}: the shield's blended layer sorts over it"
                        );
                    }
                }
                let t = &truths[&(owner.clone(), d.piece)];
                if matches!(
                    d.piece,
                    StatePiece::Lattice { .. } | StatePiece::Tail { .. }
                ) {
                    // Laid in the camera's plane: its centre still projects
                    // where the true centre does.
                    let (p, _) = project(&view, d.at);
                    let (q, _) = project(&view, t.at);
                    assert!((p - q).length() < 1e-4, "{what}");
                    checked += 1;
                    continue;
                }
                let (p, dp) = project(&view, d.at);
                let (q, dq) = project(&view, t.at);
                assert!((p - q).length() < 1e-5, "{what}: moved on screen");
                // Same screen size: scale over depth.
                assert!(
                    (d.scale.x / dp - t.scale.x / dq).abs() < 1e-5 * t.scale.x.max(1.0),
                    "{what}: resized on screen"
                );
                if (d.at - t.at).length() > 1e-4 {
                    held += 1;
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 20000, "{checked}");
    assert!(held > 5000, "{held}");
}

/// At the match camera (60 yd, 38.7°, a 45° vertical field of view on a
/// 1080-line screen), the largest pieces read as light on the ally, not a
/// screen-filling flash: the widest disc and the onset streak each span
/// well under a tenth of the screen's height, and the shell's tallest quad
/// under a ninth.
#[test]
fn the_largest_pieces_read_as_a_buff_at_the_match_camera() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    let target = Vec3::new(2.0, 1.0, 1.0);
    let cam = h.camera(match_camera(target, 60.0, 38.7_f32.to_radians(), 0.8));
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    let focal = 540.0 / (22.5_f32.to_radians()).tan();
    let mut widest = BTreeMap::<String, f32>::new();
    for _ in 0..200 {
        h.tick(1);
        let view = *h.app.world().get::<GlobalTransform>(cam).unwrap();
        for d in h.pieces() {
            let name = match d.piece {
                StatePiece::WideDisc => "wide disc",
                StatePiece::OnsetStreak if d.alpha > 0.0 => "onset streak",
                StatePiece::Shell(ShellQuad::Teal) => "teal quad",
                _ => continue,
            };
            let (_, depth) = project(&view, d.at);
            let px = d.scale.x.max(d.scale.y) * focal / depth;
            let e = widest.entry(name.to_string()).or_insert(0.0);
            *e = e.max(px);
        }
    }
    assert_eq!(widest.len(), 3);
    assert!(
        widest["wide disc"] > 90.0 && widest["wide disc"] < 108.0,
        "{widest:?}"
    );
    assert!(widest["onset streak"] > 125.0 && widest["onset streak"] < 140.0);
    assert!(widest["teal quad"] > 90.0 && widest["teal quad"] < 1080.0 / 9.0);
}

// ── routing ─────────────────────────────────────────────────────────────────

/// Both buffs are owned by their states, by NAME: the state's onset is the
/// landing. Their types are shared (a Flametongue pulse's spell power,
/// Devotion Aura's cut), so another aura of either type keeps the band, and
/// the names alone own no other type. The names are the ones the RON gives
/// the abilities.
#[test]
fn both_buffs_route_to_their_states_and_their_siblings_keep_the_band() {
    let defs = AbilityDefinitions::shipped();
    for (ability, name, owner) in [
        (
            AbilityType::PowerInfusion,
            POWER_INFUSION_AURA,
            AuraApplyOwner::InfusionCrackle,
        ),
        (
            AbilityType::PainSuppression,
            PAIN_SUPPRESSION_AURA,
            AuraApplyOwner::SuppressionShell,
        ),
    ] {
        let def = defs.get_unchecked(&ability);
        assert_eq!(def.name, name, "the RON name");
        let t = def.applies_aura.as_ref().unwrap().aura_type;
        assert_eq!(
            AuraApplyRoute::for_instance(t, name),
            AuraApplyRoute::Owned(owner)
        );
        assert_eq!(
            AuraApplyRoute::for_instance(t, "Some Other Boon"),
            AuraApplyRoute::for_aura(t),
            "{ability:?}'s type siblings route by type"
        );
        assert_eq!(AuraApplyRoute::for_aura(t), AuraApplyRoute::Band);
        for other in AuraType::ALL.into_iter().filter(|o| *o != t) {
            assert_eq!(
                AuraApplyRoute::for_instance(other, name),
                AuraApplyRoute::for_aura(other),
                "{other:?}"
            );
        }
    }
}

/// In the running chain, neither buff raises the aura band, while an aura of
/// the same type under another name does.
#[test]
fn neither_buff_raises_the_band_but_a_sibling_does() {
    for ability in [AbilityType::PowerInfusion, AbilityType::PainSuppression] {
        let mut h = Harness::new();
        let (priest, ally) = h.pair();
        h.land(ability, ally, priest);
        h.tick(3);
        assert_eq!(
            h.count::<AuraBand>(),
            0,
            "{ability:?}: its state owns its landing"
        );
        assert_eq!(h.rigs().len(), 1);

        let mut h = Harness::new();
        let (priest, ally) = h.pair();
        let defs = AbilityDefinitions::default();
        let mut aura = AuraPending::from_ability(ally, priest, defs.get(&ability).unwrap())
            .unwrap()
            .aura;
        aura.ability_name = "Some Other Boon".into();
        h.auras(ally).push(aura);
        h.tick(3);
        assert_eq!(
            h.count::<AuraBand>(),
            1,
            "{ability:?}'s sibling keeps the band"
        );
        assert!(h.rigs().is_empty(), "and wears no state");
    }
}

// ── lifecycle, with other buffs present ─────────────────────────────────────

/// The other buffs' auras, by name, on `unit`.
fn others(h: &mut Harness, unit: Entity) -> BTreeSet<String> {
    h.auras(unit)
        .iter()
        .map(|a| a.ability_name.clone())
        .filter(|n| n != POWER_INFUSION_AURA && n != PAIN_SUPPRESSION_AURA)
        .collect()
}

/// Run out: each state goes on the frame its buff does, and every piece with
/// it; the shield, the heal over time and Fortitude are untouched.
#[test]
fn each_state_ends_when_its_buff_runs_out() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.dress(ally, priest);
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    let dressed = others(&mut h, ally);
    assert_eq!(dressed.len(), 3, "{dressed:?}");
    h.tick(1);
    assert_eq!(h.rigs().len(), 2);
    let mut frames = 0;
    while h.remaining(ally, PAIN_SUPPRESSION_AURA).is_some() {
        assert_eq!(h.rigs().len(), 2, "both hold at frame {frames}");
        h.tick(1);
        frames += 1;
    }
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1, "the shell went with its buff");
    assert_eq!(rigs[0].state, DisciplineState::InfusionCrackle);
    assert!(h.pieces().iter().all(|d| d.rig == rigs[0].entity));
    assert_eq!(
        h.auras(ally)
            .iter()
            .filter(|a| a.ability_name == "Power Word: Fortitude")
            .count(),
        1
    );
    while h.remaining(ally, POWER_INFUSION_AURA).is_some() {
        h.tick(1);
    }
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<DisciplineStatePiece>(), 0, "nothing lingers");
    assert!(others(&mut h, ally).contains("Power Word: Fortitude"));
}

/// Purged or dispelled mid-buff (taken off its bearer): the state goes the
/// next frame, the other buffs stay, and the other state is untouched.
#[test]
fn a_purged_buff_takes_its_state() {
    for (gone, kept) in [
        (POWER_INFUSION_AURA, DisciplineState::SuppressionShell),
        (PAIN_SUPPRESSION_AURA, DisciplineState::InfusionCrackle),
    ] {
        let mut h = Harness::new();
        let (priest, ally) = h.pair();
        h.dress(ally, priest);
        h.infuse(ally, priest);
        h.suppress(ally, priest);
        h.tick(60);
        let kept_rig = h.rig(kept).entity;
        h.auras(ally).retain(|a| a.ability_name != gone);
        h.tick(1);
        let rigs = h.rigs();
        assert_eq!(rigs.len(), 1, "{gone} purged");
        assert_eq!(rigs[0].entity, kept_rig, "the other state is the same rig");
        assert_eq!(others(&mut h, ally).len(), 3);
    }
}

/// A refresh (another Priest's cast, or the same Priest's) keeps the state
/// and its clock, and replays the cast: the caster's hands flash again, and
/// Pain Suppression raises another swirl at that caster's feet.
#[test]
fn a_refresh_keeps_the_state_and_replays_the_cast() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    let (other, _) = h.unit(CharacterClass::Priest, 2, Vec3::new(-6.0, 1.0, 3.0));
    h.dress(ally, priest);
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    h.tick(70); // the first hands and swirl are spent
    assert!(h.hands().is_empty());
    assert!(h.swirls().is_empty());
    let before: BTreeMap<DisciplineState, (Entity, f32)> = h
        .rigs()
        .iter()
        .map(|r| (r.state, (r.entity, r.age)))
        .collect();
    for aura in h.auras(ally).iter_mut() {
        if aura.ability_name == POWER_INFUSION_AURA || aura.ability_name == PAIN_SUPPRESSION_AURA {
            aura.duration += 5.0;
            aura.caster = Some(other);
        }
    }
    h.tick(1);
    for r in h.rigs() {
        let (entity, age) = before[&r.state];
        assert_eq!(r.entity, entity, "{:?}: the same rig", r.state);
        assert!(
            (r.age - age - DT).abs() < 1e-4,
            "{:?}: its clock runs on",
            r.state
        );
    }
    let hands = h.hands();
    assert_eq!(hands.len(), 4, "both buffs' hands: {hands:?}");
    assert!(hands
        .iter()
        .all(|(k, c)| *k == HealCastKind::HolyFlash && *c == other));
    let swirls = h.swirls();
    assert_eq!(swirls.len(), 1);
    assert_eq!(swirls[0].1, other);
    assert!((swirls[0].3 - feet(Vec3::new(-6.0, 1.0, 3.0))).length() < 1e-5);
    // A buff running down is never read as a refresh.
    h.tick(30);
    assert_eq!(h.hands().len(), 4, "still the two pairs, mid-flash");
    assert_eq!(h.swirls().len(), 1);
}

/// The ally dies mid-buff (its auras stay on the corpse): both states go at
/// once. The Priest dies mid-swirl: the swirl goes too.
#[test]
fn death_takes_the_states_and_the_swirl() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.dress(ally, priest);
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    h.tick(10);
    assert_eq!(h.rigs().len(), 2);
    assert_eq!(h.swirls().len(), 1);
    h.app
        .world_mut()
        .get_mut::<Combatant>(priest)
        .unwrap()
        .current_health = 0.0;
    h.tick(1);
    assert!(h.swirls().is_empty(), "the swirl goes with its Priest");
    assert_eq!(h.rigs().len(), 2, "the ally's states stay");
    h.app
        .world_mut()
        .get_mut::<Combatant>(ally)
        .unwrap()
        .current_health = 0.0;
    h.tick(1);
    assert!(
        h.remaining(ally, POWER_INFUSION_AURA).is_some(),
        "the corpse keeps the aura"
    );
    assert!(h.rigs().is_empty(), "nothing on a corpse");
    assert_eq!(h.count::<DisciplineStatePiece>(), 0);
}

/// Polymorphed: both states are hidden with the body (the sheep wears
/// nothing), their clocks running on; shown again when the sheep ends.
/// Stealthed: nothing changes, piece for piece, against a twin that is not.
#[test]
fn a_sheep_hides_the_states_and_stealth_changes_nothing() {
    let mut h = Harness::new();
    let mut twin = Harness::new();
    let mut allies = Vec::new();
    for h in [&mut h, &mut twin] {
        let (priest, ally) = h.pair();
        h.dress(ally, priest);
        h.infuse(ally, priest);
        h.suppress(ally, priest);
        h.tick(5);
        allies.push(ally);
    }
    let ally = allies[0];
    h.app
        .world_mut()
        .get_mut::<Combatant>(ally)
        .unwrap()
        .stealthed = true;
    for _ in 0..40 {
        h.tick(1);
        twin.tick(1);
        let mut a = h.pieces();
        let mut b = twin.pieces();
        a.sort_by_key(|d| d.piece);
        b.sort_by_key(|d| d.piece);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.piece, y.piece);
            assert_eq!(x.alpha, y.alpha, "{:?}", x.piece);
            assert!((x.at - y.at).length() < 1e-5);
        }
        assert!(h
            .rigs()
            .iter()
            .all(|r| r.visibility == Visibility::Inherited));
    }
    h.app.world_mut().entity_mut(ally).insert(PolymorphedVisual);
    let ages: Vec<f32> = h.rigs().iter().map(|r| r.age).collect();
    h.tick(30);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "kept, not taken down");
    assert!(rigs.iter().all(|r| r.visibility == Visibility::Hidden));
    for (r, a) in rigs.iter().zip(&ages) {
        assert!((r.age - a - 30.0 * DT).abs() < 1e-3, "the clock runs on");
    }
    h.app
        .world_mut()
        .entity_mut(ally)
        .remove::<PolymorphedVisual>();
    h.tick(1);
    assert!(h
        .rigs()
        .iter()
        .all(|r| r.visibility == Visibility::Inherited));
}

/// The match ends mid-state: the teardown takes the states, the swirl and
/// every piece (each hangs off its rig).
#[test]
fn the_match_ending_takes_everything() {
    let mut h = Harness::new();
    let (priest, ally) = h.pair();
    h.dress(ally, priest);
    h.infuse(ally, priest);
    h.suppress(ally, priest);
    h.tick(3);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2);
    let swirls = h.swirls();
    assert_eq!(swirls.len(), 1);
    for owner in rigs
        .iter()
        .map(|r| r.entity)
        .chain(swirls.iter().map(|s| s.0))
    {
        assert!(h.app.world().get::<PlayMatchEntity>(owner).is_some());
    }
    let owners: BTreeSet<Entity> = rigs
        .iter()
        .map(|r| r.entity)
        .chain(swirls.iter().map(|s| s.0))
        .collect();
    assert!(h.pieces().iter().all(|d| owners.contains(&d.rig)));
    h.app
        .world_mut()
        .run_system_once(cleanup_play_match)
        .unwrap();
    assert!(h.rigs().is_empty());
    assert!(h.swirls().is_empty());
    assert_eq!(h.count::<DisciplineStatePiece>(), 0);
}
