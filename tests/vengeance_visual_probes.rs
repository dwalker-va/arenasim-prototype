//! Probes for Vengeance's hands (`rendering/effects/vengeance.rs`, AS-377):
//! both of the Paladin's hands glow pale blue, with soft glows and sky-blue
//! sparks, for the buff's whole life.
//!
//! These drive the graphical chain off `ActiveAuras`, as the sim leaves it,
//! with the aura Vengeance's own procs land, and assert what is DRAWN, in
//! world space through `GlobalTransform` and the spawned materials: every
//! piece against an independent transcription of the signed-off bench's
//! `customDraw` and emitters across seconds of the buff, the sprites read
//! back, camera facing under a turned camera and a turned, leaning body, the
//! hands' place and draw order at the match camera, a refresh that keeps the
//! glow going, and the hands going when the buff runs out, is purged, or the
//! Paladin dies.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. The systems run in the order `states/mod.rs` registers them.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::camera::CameraProjection;
use bevy::render::mesh::MeshAabb;
use bevy::render::render_phase::ViewRangefinder3d;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, CameraController, Combatant, PolymorphedVisual, VisualBody,
    VENGEANCE_AURA,
};
use arenasim::states::play_match::talent_procs::{effect_aura, ProcEvent, TalentProcSlot};
use arenasim::states::play_match::talents::{TalentDefinitions, TalentEffect, TalentId};
use arenasim::states::play_match::{
    place_vengeance_hands, update_vengeance_hands, HandPiece, VengeanceHand, VengeanceHandPiece,
    VengeanceHands,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;
/// The spell-hand socket on the rendered body, transcribed: 0.72 out to
/// either side, 0.05 above the body's centre, 0.30 forward.
const HAND: Vec3 = Vec3::new(0.72, 0.05, 0.30);
/// A combatant's capsule: `Capsule3d::new(0.5, 1.5)` about the body centre.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_LENGTH: f32 = 0.75;

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
        app.add_systems(Update, update_vengeance_hands);
        app.add_systems(
            PostUpdate,
            place_vengeance_hands.before(TransformSystem::TransformPropagate),
        );
        Harness { app }
    }

    /// A unit as `spawn_combatant` builds it, with a `VisualBody` child.
    fn unit(&mut self, at: Vec3) -> (Entity, Entity) {
        let world = self.app.world_mut();
        let unit = world
            .spawn((
                Combatant::new(1, 0, CharacterClass::Paladin),
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

    fn camera(&mut self, transform: Transform) -> Entity {
        self.app
            .world_mut()
            .spawn((Camera3d::default(), transform))
            .id()
    }

    /// Vengeance as its swing-crit proc lands it.
    fn avenge(&mut self, unit: Entity) {
        self.auras(unit).push(vengeance());
    }

    fn auras(&mut self, unit: Entity) -> Mut<'_, Vec<Aura>> {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .map_unchanged(|a| &mut a.auras)
    }

    /// One frame, the buffs running down as the sim runs them.
    fn step(&mut self) {
        let mut q = self.app.world_mut().query::<&mut ActiveAuras>();
        for mut auras in q.iter_mut(self.app.world_mut()) {
            for aura in &mut auras.auras {
                aura.duration -= DT;
            }
        }
        self.app.update();
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.step();
        }
    }

    fn rigs(&mut self) -> Vec<(Entity, Entity, f32)> {
        let mut q = self.app.world_mut().query::<(Entity, &VengeanceHands)>();
        q.iter(self.app.world())
            .map(|(e, r)| (e, r.bearer, r.age))
            .collect()
    }

    /// Each hand: its side and its world position.
    fn hands(&mut self) -> Vec<(f32, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&VengeanceHand, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(h, g)| (h.side, g.translation()))
            .collect()
    }

    fn pieces(&mut self) -> Vec<Drawn> {
        let mut q = self.app.world_mut().query::<(
            &VengeanceHandPiece,
            &ChildOf,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            &Mesh3d,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(p, parent, g, m, mesh)| {
                (p.piece, parent.parent(), *g, m.0.clone(), mesh.0.clone())
            })
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(piece, hand, g, m, mesh)| {
                let material = materials.get(&m).unwrap().clone();
                let c = material.base_color.to_srgba();
                Drawn {
                    piece,
                    side: world.get::<VengeanceHand>(hand).unwrap().side,
                    hand_at: world.get::<GlobalTransform>(hand).unwrap().translation(),
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

    fn count<T: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&T>();
        q.iter(self.app.world()).count()
    }
}

struct Drawn {
    piece: HandPiece,
    side: f32,
    hand_at: Vec3,
    at: Vec3,
    rotation: Quat,
    scale: Vec3,
    rgb: [u8; 3],
    alpha: f32,
    material: StandardMaterial,
    mesh: Handle<Mesh>,
}

/// Vengeance's aura as its first proc (a melee crit) lands it, through the
/// engine's own constructor.
fn vengeance() -> Aura {
    let node = TalentDefinitions::shipped()
        .get(TalentId::Vengeance)
        .expect("Vengeance is a node");
    let proc = node
        .per_rank
        .iter()
        .find_map(|effect| match effect {
            TalentEffect::Proc(proc) => Some(proc.clone()),
            _ => None,
        })
        .expect("Vengeance procs");
    let owner = Entity::from_raw(1);
    let slot = TalentProcSlot::new(TalentId::Vengeance, &node.name, proc);
    let event = ProcEvent::new(owner, slot.proc.trigger).amount(50.0);
    let aura = effect_aura(&slot, owner, &event).unwrap().aura;
    assert_eq!(aura.ability_name, VENGEANCE_AURA);
    assert!((aura.duration - 30.0).abs() < 1e-6, "a 30 s buff");
    aura
}

/// Every piece one hand draws, named.
fn every_piece() -> BTreeSet<HandPiece> {
    let mut all = BTreeSet::from([HandPiece::Quad, HandPiece::Ring]);
    all.extend((0..12).map(|slot| HandPiece::Glow { slot }));
    all.extend((0..20).map(|slot| HandPiece::Spark { slot }));
    all
}

// ── the bench, transcribed ──────────────────────────────────────────────────
//
// The "Vengeance" tab of `docs/design/benches/2026-10-18-paladin-talent-bench.html`
// (version 1792379243-e3d6), written out from its JavaScript at its default
// knobs (size, spread, duration, density and opacity 1). Independent of the
// module: none of its constants are read here.

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

/// The bench's `keys` over colours, truncated as `rgb()` does.
fn keys3(k: &[(f64, [f64; 3])], t: f64) -> [u8; 3] {
    let channel = |c: usize| keys(&k.iter().map(|(t, v)| (*t, v[c])).collect::<Vec<_>>(), t);
    [channel(0), channel(1), channel(2)].map(|v| v as u8)
}

/// One emitter as the bench declares it, per hand (its `rate` is over both
/// hands, `at: ['palR', 'palL']`, so each hand gets half).
struct Emitter {
    rate: f64,
    life: f64,
    speed: f64,
    area: f64,
    area_y: f64,
    color: [(f64, [f64; 3]); 3],
    alpha: [(f64, f64); 3],
    scale: [(f64, f64); 2],
}

const GLOWS: Emitter = Emitter {
    rate: 30.0 / 2.0,
    life: 0.8,
    speed: 0.0,
    area: 0.0,
    area_y: 0.0,
    color: [
        (0.0, [255.0, 255.0, 255.0]),
        (0.65, [255.0, 244.0, 180.0]),
        (1.0, [255.0, 255.0, 255.0]),
    ],
    alpha: [(0.0, 0.0), (0.65, 1.0), (1.0, 0.0)],
    scale: [(0.0, 0.2), (1.0, 0.3)],
};

const SPARKS: Emitter = Emitter {
    rate: 100.0 / 2.0,
    life: 0.4,
    speed: 0.44,
    area: 0.2,
    area_y: 0.28,
    color: [
        (0.0, [255.0, 255.0, 255.0]),
        (0.65, [75.0, 177.0, 255.0]),
        (1.0, [255.0, 255.0, 255.0]),
    ],
    alpha: [(0.0, 0.0), (0.65, 1.0), (1.0, 0.0)],
    scale: [(0.0, 0.08), (1.0, 0.05)],
};

/// A live particle as the bench draws it: colour, alpha, diameter (`sprite`
/// draws `scale × size × 2` across), and its own age.
#[derive(Debug, Clone, Copy)]
struct Particle {
    rgb: [u8; 3],
    alpha: f64,
    diameter: f64,
    age: f64,
}

/// The particles an emitter holds `age` seconds after the onset, one born
/// every `1 / rate` seconds from the onset, each living `life`.
fn bench_particles(e: &Emitter, age: f64) -> Vec<Particle> {
    let mut out = Vec::new();
    let mut j = 0;
    while j as f64 / e.rate <= age + 1e-9 {
        let born = j as f64 / e.rate;
        let own = age - born;
        if own < e.life - 1e-6 {
            let t = own / e.life;
            out.push(Particle {
                rgb: keys3(&e.color, t),
                alpha: keys(&e.alpha, t),
                diameter: keys(&e.scale, t) * 2.0,
                age: own,
            });
        }
        j += 1;
    }
    out
}

/// The bench's blue quad on each hand: `bbGlow(p, 0.37×2, 0.37×2,
/// [94,189,255], 0.8×0.8)` and `ring3(p, 0.12, [200,230,255], 0.8)`.
const BENCH_QUAD: ([u8; 3], f64, f64) = ([94, 189, 255], 0.64, 0.74);
const BENCH_RING: ([u8; 3], f64, f64) = ([200, 230, 255], 0.8, 0.12);
/// The ring sprite's stroke runs at 0.965 of the quad's half-width.
const RING_CENTRE: f32 = 0.965;

/// Lit particles (alpha above `floor`) sorted by diameter, for comparing
/// the drawn set with the bench's.
fn lit(mut ps: Vec<Particle>) -> Vec<Particle> {
    ps.retain(|p| p.alpha > 1e-3);
    ps.sort_by(|a, b| a.diameter.partial_cmp(&b.diameter).unwrap());
    ps
}

/// Compare one hand's drawn particles of one emitter against the bench's.
fn compare(what: &str, drawn: Vec<Particle>, bench: Vec<Particle>) -> usize {
    let (drawn, bench) = (lit(drawn), lit(bench));
    assert_eq!(drawn.len(), bench.len(), "{what}: lit particles");
    for (d, b) in drawn.iter().zip(&bench) {
        assert!(
            (d.diameter - b.diameter).abs() < 2e-3,
            "{what}: {d:?} vs {b:?}"
        );
        assert!((d.alpha - b.alpha).abs() < 1e-2, "{what}: {d:?} vs {b:?}");
        for c in 0..3 {
            assert!(
                (d.rgb[c] as i32 - b.rgb[c] as i32).abs() <= 2,
                "{what}: {d:?} vs {b:?}"
            );
        }
    }
    drawn.len()
}

// ── the probes ──────────────────────────────────────────────────────────────

/// A Paladin holding Vengeance wears one rig with two hands, one on each
/// side, each drawing every piece the bench draws.
#[test]
fn both_hands_rise_on_a_paladin_holding_vengeance() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(2.0, 1.0, -3.0));
    h.tick(2);
    assert!(h.rigs().is_empty(), "nothing before the buff");
    h.avenge(paladin);
    h.tick(2);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert_eq!(rigs[0].1, paladin);
    let sides: BTreeSet<i32> = h.hands().iter().map(|(s, _)| *s as i32).collect();
    assert_eq!(sides, BTreeSet::from([-1, 1]), "both hands");
    let pieces = h.pieces();
    for side in [1.0, -1.0] {
        let drawn: BTreeSet<HandPiece> = pieces
            .iter()
            .filter(|d| d.side == side)
            .map(|d| d.piece)
            .collect();
        assert_eq!(drawn, every_piece(), "side {side}");
    }
    assert_eq!(pieces.len(), 2 * 34);
    h.tick(30);
    assert_eq!(h.rigs().len(), 1, "one rig, held");
}

/// Each hand stands on the RENDERED body's spell hand: the bearer's facing,
/// the body's bob, its lean and its step all carry it, as the swing moves
/// them; and every piece of the blue quad stands on its hand.
#[test]
fn each_hand_rides_the_rendered_spell_hand() {
    let mut h = Harness::new();
    let (paladin, body) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(2);
    let poses = [
        (Vec3::new(0.0, 1.0, 0.0), 0.0, Vec3::ZERO, Quat::IDENTITY),
        (
            Vec3::new(3.0, 1.0, -2.0),
            1.1,
            Vec3::new(0.0, 0.22, 0.0),
            Quat::IDENTITY,
        ),
        // A swing's lean and step.
        (
            Vec3::new(-4.0, 1.0, 5.0),
            -2.3,
            Vec3::new(0.18, 0.10, 0.0),
            Quat::from_rotation_x(0.3) * Quat::from_rotation_y(0.4),
        ),
    ];
    for (at, yaw, offset, lean) in poses {
        {
            let world = h.app.world_mut();
            *world.get_mut::<Transform>(paladin).unwrap() =
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw));
            *world.get_mut::<Transform>(body).unwrap() =
                Transform::from_translation(offset).with_rotation(lean);
        }
        h.tick(1);
        let bearer = Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw));
        let body_tf = Transform::from_translation(offset).with_rotation(lean);
        let hands = h.hands();
        assert_eq!(hands.len(), 2);
        for (side, got) in hands {
            let want = bearer.transform_point(body_tf.transform_point(Vec3::new(
                HAND.x * side,
                HAND.y,
                HAND.z,
            )));
            assert!(
                (got - want).length() < 1e-4,
                "side {side} at {at} yaw {yaw}: {got} vs {want}"
            );
            // Clear of the capsule: the glow is on the hand, not in the body.
            let axis = bearer.transform_point(offset);
            assert!((got - axis).xz().length() > BODY_RADIUS + 0.2, "{got}");
        }
        for d in h.pieces() {
            if matches!(d.piece, HandPiece::Quad | HandPiece::Ring) {
                assert!((d.at - d.hand_at).length() < 1e-4, "{:?}", d.piece);
            }
        }
    }
}

/// Every piece, every frame across 3 s of the buff, against the bench: the
/// blue quad's and ring's colour, alpha and size; each emitter's lit
/// particles, as a set, in colour, alpha and size (so the rate and the life
/// are the bench's); every glow on its hand; every spark inside its birth box
/// grown by how far it has flown.
#[test]
fn every_piece_draws_what_the_bench_draws() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(1.0, 1.0, 1.0));
    h.avenge(paladin);
    let (mut glows_seen, mut sparks_seen) = (0, 0);
    // How far from the hand the youngest sparks stand: their birth box.
    let mut young = Vec3::ZERO;
    for frame in 0..190 {
        h.tick(1);
        let age = h.rigs()[0].2;
        assert!((age - frame as f32 * DT).abs() < 1e-4, "the rig's clock");
        let pieces = h.pieces();
        for side in [1.0, -1.0] {
            let mine: Vec<&Drawn> = pieces.iter().filter(|d| d.side == side).collect();
            let what = |p: &str| format!("{p} side {side} frame {frame}");
            let quad = mine.iter().find(|d| d.piece == HandPiece::Quad).unwrap();
            assert_eq!(quad.rgb, BENCH_QUAD.0, "{}", what("quad"));
            assert!((quad.alpha as f64 - BENCH_QUAD.1).abs() < 1e-4);
            assert!((quad.scale.x as f64 - BENCH_QUAD.2).abs() < 1e-4);
            assert!((quad.scale.y as f64 - BENCH_QUAD.2).abs() < 1e-4);
            let ring = mine.iter().find(|d| d.piece == HandPiece::Ring).unwrap();
            assert_eq!(ring.rgb, BENCH_RING.0, "{}", what("ring"));
            assert!((ring.alpha as f64 - BENCH_RING.1).abs() < 1e-4);
            let radius = ring.scale.x * RING_CENTRE / 2.0;
            assert!((radius as f64 - BENCH_RING.2).abs() < 1e-4, "{radius}");

            let particle = |d: &&&Drawn, life: f64, d0: f64, d1: f64| {
                let t = (d.scale.x as f64 - d0) / (d1 - d0);
                Particle {
                    rgb: d.rgb,
                    alpha: d.alpha as f64,
                    diameter: d.scale.x as f64,
                    age: t * life,
                }
            };
            let glows: Vec<Particle> = mine
                .iter()
                .filter(|d| matches!(d.piece, HandPiece::Glow { .. }))
                .map(|d| {
                    assert!(
                        (d.at - d.hand_at).length() < 1e-4,
                        "a glow stays on its hand"
                    );
                    particle(&d, 0.8, 0.4, 0.6)
                })
                .collect();
            glows_seen += compare(&what("glows"), glows, bench_particles(&GLOWS, age as f64));
            let sparks: Vec<Particle> = mine
                .iter()
                .filter(|d| matches!(d.piece, HandPiece::Spark { .. }))
                .map(|d| {
                    let p = particle(&d, 0.4, 0.16, 0.10);
                    if p.alpha > 1e-3 {
                        let off = d.at - d.hand_at;
                        let flown = (SPARKS.speed * p.age) as f32 + 1e-3;
                        let (across, tall) = (SPARKS.area as f32 / 2.0, SPARKS.area_y as f32 / 2.0);
                        assert!(off.x.abs() <= across + flown, "{off} {p:?}");
                        assert!(off.z.abs() <= across + flown, "{off} {p:?}");
                        assert!(off.y.abs() <= tall + flown, "{off} {p:?}");
                        if p.age < 0.02 {
                            young = young.max(off.abs());
                        }
                    }
                    p
                })
                .collect();
            sparks_seen += compare(
                &what("sparks"),
                sparks,
                bench_particles(&SPARKS, age as f64),
            );
        }
    }
    // Not over darkness: once warmed up a hand holds about 12 glows and 20
    // sparks, less the ones at the ends of their lives.
    assert!(glows_seen > 2 * 150 * 10, "{glows_seen}");
    assert!(sparks_seen > 2 * 150 * 17, "{sparks_seen}");
    // The sparks are born all through the bench's box (0.2 across, 0.28
    // tall), not at the hand's centre.
    assert!(young.x > 0.07 && young.z > 0.07, "{young}");
    assert!(young.y > 0.1, "{young}");
}

/// The sparks fly at the bench's 0.44 yd/s, every one in its own direction,
/// spread all round the hand; and the two hands scatter differently.
#[test]
fn the_sparks_fly_out_at_their_speed_all_round() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(40);
    let snapshot = |h: &mut Harness| -> Vec<(HandPiece, f32, Vec3, f32)> {
        h.pieces()
            .into_iter()
            .filter(|d| matches!(d.piece, HandPiece::Spark { .. }))
            .map(|d| (d.piece, d.side, d.at - d.hand_at, d.scale.x))
            .collect()
    };
    let mut directions = Vec::new();
    let mut moved = 0;
    for _ in 0..30 {
        let before = snapshot(&mut h);
        h.tick(1);
        let after = snapshot(&mut h);
        for (piece, side, at, size) in &before {
            let (_, _, now, later) = after
                .iter()
                .find(|(p, s, _, _)| p == piece && s == side)
                .unwrap();
            // The same spark a frame on (it shrank by one frame of its life).
            if *later < *size && (*size - *later - 0.06 * DT / 0.4).abs() < 1e-4 {
                let step = *now - *at;
                assert!(
                    (step.length() - 0.44 * DT).abs() < 1e-4,
                    "{piece:?}: {} yd in a frame",
                    step.length()
                );
                directions.push((*side, step.normalize()));
                moved += 1;
            }
        }
    }
    assert!(moved > 30 * 2 * 15, "{moved}");
    for side in [1.0, -1.0] {
        let dirs: Vec<Vec3> = directions
            .iter()
            .filter(|(s, _)| *s == side)
            .map(|(_, d)| *d)
            .collect();
        let mean = dirs.iter().copied().sum::<Vec3>() / dirs.len() as f32;
        assert!(mean.length() < 0.5, "spread all round: mean {mean}");
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            assert!(dirs.iter().any(|d| d.dot(axis) > 0.6), "toward {axis}");
            assert!(dirs.iter().any(|d| d.dot(axis) < -0.6), "away from {axis}");
        }
    }
    // Mirror slots differ: the hands do not scatter as one.
    let sparks = snapshot(&mut h);
    let differ = sparks
        .iter()
        .filter(|(_, s, _, _)| *s > 0.0)
        .filter(|(p, _, at, _)| {
            let twin = sparks
                .iter()
                .find(|(q, s, _, _)| q == p && *s < 0.0)
                .unwrap();
            (twin.2 - *at).length() > 1e-3
        })
        .count();
    assert!(differ > 15, "{differ}");
}

/// A melee crit refreshes the buff in place: the hands just continue — the
/// same rig, its clock running on, no new onset — whether the aura's time
/// is renewed or the aura is replaced within the frame.
#[test]
fn a_refresh_keeps_the_glow_going() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(50);
    let (rig, _, age) = h.rigs()[0];
    // Renewed in place, as `apply_pending_auras` refreshes a refresh-only aura.
    h.auras(paladin)[0].duration = 30.0;
    h.tick(10);
    // Replaced outright within the frame.
    {
        let mut auras = h.auras(paladin);
        auras.clear();
        auras.push(vengeance());
    }
    h.tick(10);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert_eq!(rigs[0].0, rig, "the same rig");
    assert!(
        (rigs[0].2 - (age + 20.0 * DT)).abs() < 1e-4,
        "its clock runs on"
    );
}

/// The hands go, pieces and all, on the frame the buff runs out, is purged,
/// or its Paladin dies (auras stay on a corpse) or is gone; and a new buff
/// after one ends starts a new onset.
#[test]
fn the_hands_go_when_the_buff_ends_or_the_paladin_dies() {
    type End = fn(&mut Harness, Entity);
    let endings: [(&str, End); 4] = [
        ("runs out", |h, p| {
            // The sim drops an aura whose time is spent.
            h.auras(p)[0].duration = DT / 2.0;
            h.step();
            h.auras(p).retain(|a| a.duration > 0.0);
        }),
        ("purged", |h, p| h.auras(p).clear()),
        ("dies", |h, p| {
            h.app
                .world_mut()
                .get_mut::<Combatant>(p)
                .unwrap()
                .current_health = 0.0;
        }),
        ("gone", |h, p| {
            h.app.world_mut().entity_mut(p).despawn();
        }),
    ];
    for (how, end) in endings {
        let mut h = Harness::new();
        let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
        h.avenge(paladin);
        h.tick(20);
        assert_eq!(h.rigs().len(), 1, "{how}");
        assert_eq!(h.count::<VengeanceHandPiece>(), 68, "{how}");
        end(&mut h, paladin);
        h.tick(1);
        assert!(h.rigs().is_empty(), "{how}: the rig");
        assert_eq!(h.count::<VengeanceHand>(), 0, "{how}: the hands");
        assert_eq!(h.count::<VengeanceHandPiece>(), 0, "{how}: the pieces");
        h.tick(10);
        assert!(h.rigs().is_empty(), "{how}: nothing comes back");
    }

    // A new buff after a purge is a new onset.
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(20);
    h.auras(paladin).clear();
    h.tick(1);
    h.avenge(paladin);
    h.tick(1);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert!(rigs[0].2 < 1e-6, "a fresh clock: {}", rigs[0].2);
}

/// Nothing rises on a corpse holding the buff, nor for any other damage
/// boost, nor for Vengeance's name on another type.
#[test]
fn a_corpse_and_other_boosts_raise_no_hands() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.app
        .world_mut()
        .get_mut::<Combatant>(paladin)
        .unwrap()
        .current_health = 0.0;
    h.avenge(paladin);
    h.tick(5);
    assert!(h.rigs().is_empty(), "a corpse");

    let mut other = vengeance();
    other.ability_name = "Some Other Fury".into();
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.auras(paladin).push(other);
    h.tick(5);
    assert!(h.rigs().is_empty(), "another damage boost");

    let mut renamed = vengeance();
    renamed.effect_type = AuraType::DamageTakenReduction;
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.auras(paladin).push(renamed);
    h.tick(5);
    assert!(h.rigs().is_empty(), "the name on another type");

    // The control: the buff itself rises.
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(5);
    assert_eq!(h.rigs().len(), 1);
}

/// A polymorphed Paladin's hands hide with its weapons (the sheep has no
/// hands), and come back with its body; the clock runs on underneath.
#[test]
fn a_sheep_hides_the_hands() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(5);
    let visibility = |h: &mut Harness| {
        let rig = h.rigs()[0].0;
        *h.app.world().get::<Visibility>(rig).unwrap()
    };
    assert_eq!(visibility(&mut h), Visibility::Inherited);
    h.app
        .world_mut()
        .entity_mut(paladin)
        .insert(PolymorphedVisual);
    h.tick(1);
    assert_eq!(visibility(&mut h), Visibility::Hidden);
    h.app
        .world_mut()
        .entity_mut(paladin)
        .remove::<PolymorphedVisual>();
    h.tick(1);
    assert_eq!(visibility(&mut h), Visibility::Inherited);
    assert_eq!(h.rigs().len(), 1);
    assert!((h.rigs()[0].2 - 6.0 * DT).abs() < 1e-4);
}

/// Read one sprite's alpha at `x` of its radius along its middle row.
fn alpha_along(image: &Image, x: f32) -> f32 {
    let px = image.width() as usize;
    let col = ((((x + 1.0) / 2.0) * px as f32) as usize).min(px - 1);
    image.data.as_ref().unwrap()[((px / 2) * px + col) * 4 + 3] as f32 / 255.0
}

/// Every piece's material and mesh: additive, unlit, both faces, on a unit
/// quad, with its sprite read back — the quad the bench's `bbGlow` (1, 0.45
/// at half its radius, 0), the ring a thin stroke at 0.965 of its radius,
/// the particles the bench's `sprite('glow')` (1, 0.45 at 0.45, 0).
#[test]
fn every_piece_is_an_additive_sprite_on_a_unit_quad() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(3);
    let pieces = h.pieces();
    let world = h.app.world();
    let images = world.resource::<Assets<Image>>();
    let meshes = world.resource::<Assets<Mesh>>();
    let mut sprites = BTreeSet::new();
    for d in &pieces {
        let m = &d.material;
        // Every piece is drawn additively by the bench: the quad by `bbGlow`
        // and the ring by `ring3`, both of which set the canvas to
        // 'lighter'; the glows and sparks by `Emitter.draw`, which sets
        // 'lighter' for any emitter without `blend: 'alpha'` (neither of
        // Vengeance's has one). None is a bare source-over `sprite(...)`,
        // which would be `AlphaMode::Blend`.
        let bench_call = match d.piece {
            HandPiece::Quad => "bbGlow ('lighter')",
            HandPiece::Ring => "ring3 ('lighter')",
            HandPiece::Glow { .. } => "Emitter.draw, glow layer ('lighter')",
            HandPiece::Spark { .. } => "Emitter.draw, sparks layer ('lighter')",
        };
        assert!(
            matches!(m.alpha_mode, AlphaMode::Add),
            "{:?} is drawn by {bench_call}: {:?}",
            d.piece,
            m.alpha_mode
        );
        assert!(m.unlit && m.double_sided && m.cull_mode.is_none());
        let aabb = meshes.get(&d.mesh).unwrap().compute_aabb().unwrap();
        assert!((Vec3::from(aabb.half_extents) - Vec3::new(0.5, 0.5, 0.0)).length() < 1e-6);
        let image = images.get(m.base_color_texture.as_ref().unwrap()).unwrap();
        let profile = [0.0, 0.45, 0.5, 0.965, 0.999].map(|x| alpha_along(image, x));
        match d.piece {
            HandPiece::Quad => {
                assert!(profile[0] > 0.98, "{profile:?}");
                assert!((profile[2] - 0.45).abs() < 0.03, "{profile:?}");
                assert!(profile[4] < 0.02, "{profile:?}");
            }
            HandPiece::Ring => {
                assert!(profile[0] < 0.01, "{profile:?}");
                assert!(profile[2] < 0.01, "{profile:?}");
                assert!(profile[3] > 0.6, "{profile:?}");
            }
            HandPiece::Glow { .. } | HandPiece::Spark { .. } => {
                assert!(profile[0] > 0.98, "{profile:?}");
                assert!((profile[1] - 0.45).abs() < 0.03, "{profile:?}");
                assert!(profile[4] < 0.02, "{profile:?}");
            }
        }
        sprites.insert(format!("{:?}", m.base_color_texture.as_ref().unwrap().id()));
    }
    assert_eq!(sprites.len(), 3, "three sprites in all");
}

/// Under a turned camera, and a bearer turned and leaning, the quad and every
/// particle face the camera, and the ring lies level.
#[test]
fn the_glows_face_a_turned_camera_and_the_ring_lies_level() {
    let mut h = Harness::new();
    let cam = Transform::from_xyz(7.0, 9.0, -5.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
    h.camera(cam);
    let (paladin, body) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.app
        .world_mut()
        .get_mut::<Transform>(paladin)
        .unwrap()
        .rotation = Quat::from_rotation_y(2.2);
    h.app
        .world_mut()
        .get_mut::<Transform>(body)
        .unwrap()
        .rotation = Quat::from_rotation_x(0.35);
    h.avenge(paladin);
    h.tick(30);
    let pieces = h.pieces();
    assert_eq!(pieces.len(), 68);
    for d in pieces {
        match d.piece {
            HandPiece::Ring => {
                let normal = d.rotation * Vec3::Z;
                assert!(normal.y.abs() > 1.0 - 1e-4, "level: {normal}");
            }
            _ => assert!(
                d.rotation.angle_between(cam.rotation) < 1e-4,
                "{:?} faces the camera",
                d.piece
            ),
        }
    }
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

/// The nearest a segment comes to another.
fn segment_gap(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> f32 {
    let mut best = f32::MAX;
    for i in 0..=400 {
        let p = a0.lerp(a1, i as f32 / 400.0);
        let ab = b1 - b0;
        let t = ((p - b0).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        best = best.min((p - (b0 + ab * t)).length());
    }
    best
}

/// At the match camera (60 yd, 38.7° from the vertical) and every bearing
/// round the Paladin: the hand nearer the camera is never hidden by the
/// capsule — the line from the camera to it passes clear of the body — and,
/// seen from the Paladin's front, its blue quad sorts in front of the
/// blended body; the farther hand shows beside the body when the Paladin
/// stands side-on to the camera.
#[test]
fn the_hands_show_at_the_match_camera() {
    let controller = CameraController::default();
    assert_eq!(controller.zoom_distance, 60.0);
    let mut h = Harness::new();
    let cam = h.camera(Transform::default());
    let (paladin, body) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(2);
    let (mut near_checked, mut far_clear, mut front_sorted) = (0, 0, 0);
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
        let world = h.app.world();
        let eye = view.translation;
        let matrix = world.get::<GlobalTransform>(cam).unwrap().compute_matrix();
        let range = ViewRangefinder3d::from_world_from_view(&matrix);
        let body_at = world.get::<GlobalTransform>(body).unwrap().translation();
        let axis = (
            body_at - Vec3::Y * BODY_HALF_LENGTH,
            body_at + Vec3::Y * BODY_HALF_LENGTH,
        );
        let mut hands = h.hands();
        hands.sort_by(|a, b| {
            (a.1 - eye)
                .length()
                .partial_cmp(&(b.1 - eye).length())
                .unwrap()
        });
        let (near, far) = (hands[0], hands[1]);
        let gap = segment_gap(near.1, eye, axis.0, axis.1);
        assert!(
            gap > BODY_RADIUS + 0.1,
            "yaw {yaw}: near hand behind the body ({gap})"
        );
        let quad = h
            .pieces()
            .into_iter()
            .find(|d| d.piece == HandPiece::Quad && d.side == near.0)
            .unwrap();
        // Seen from the Paladin's front half (the hands are presented
        // forward, toward +Z), the near quad also draws over the blended
        // body; from behind, it stands behind the body's centre and the
        // body covers only the part of it that overlaps the silhouette.
        if yaw.cos() > 0.3 {
            assert!(
                range.distance_translation(&quad.at) > range.distance_translation(&body_at),
                "yaw {yaw}: the near quad sorts behind the body"
            );
            front_sorted += 1;
        }
        near_checked += 1;
        if segment_gap(far.1, eye, axis.0, axis.1) > BODY_RADIUS {
            far_clear += 1;
        }
    }
    assert_eq!(near_checked, 24);
    assert!(front_sorted >= 8, "{front_sorted}");
    // Side-on (the camera square to the Paladin's left or right) both hands
    // show; a quarter of the bearings at least.
    assert!(far_clear >= 6, "{far_clear} of 24");
}

/// At the match camera and a 1080p view, the blue quad spans about 16 px —
/// what the bench's 0.74 yd comes to at 60 yd; a glow 9-13 px; a spark 2-3.
/// The size the Human testing note asks a person to judge.
#[test]
fn the_hand_glow_spans_its_size_at_the_match_camera() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(Vec3::new(0.0, 1.0, 0.0));
    h.avenge(paladin);
    h.tick(2);
    let quad = h
        .pieces()
        .into_iter()
        .find(|d| d.piece == HandPiece::Quad)
        .unwrap();
    let controller = CameraController::default();
    let cam = match_camera(quad.at, controller.zoom_distance, controller.pitch, 0.0);
    let projection = PerspectiveProjection {
        aspect_ratio: 16.0 / 9.0,
        ..default()
    };
    let to_px = |p: Vec3| {
        let clip = projection.get_clip_from_view() * cam.compute_matrix().inverse() * p.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec2::new((ndc.x + 1.0) / 2.0 * 1920.0, (1.0 - ndc.y) / 2.0 * 1080.0)
    };
    let right = cam.rotation * Vec3::X;
    let half = quad.scale.x / 2.0;
    let across = (to_px(quad.at + right * half) - to_px(quad.at - right * half)).length();
    assert!((14.0..19.0).contains(&across), "{across} px");
}
