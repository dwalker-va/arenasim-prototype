//! Probes for Crusader Strike's visual (AS-379): Mortal Strike's stroke,
//! Heroic Strike's shipped red trail, and TBC's `strike_impact_chest`
//! landing on the victim's chest (`crusader_strike.rs`, `client_landings.rs`).
//!
//! What is drawn is pinned against the signed-off AS-337 bench
//! (`docs/design/benches/2026-10-18-paladin-talent-bench.html`, its
//! "Crusader Strike" tab): the trail's material (the bench's `drawStrike`,
//! `lighter`), and the landing's two emitters (its `sparks` and `plume`
//! layers, both `lighter`) — their colour and alpha ramps, blend, sprite
//! texture, quad, sizes, speeds, counts and timing. And the contract with the
//! sim: a landed strike lands, a dodged or parried one swings and trails and
//! lands nothing, and the landing plays out whatever happens to either unit.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU.

use std::f32::consts::TAU;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{Scene, Who};
use arenasim::states::play_match::combat_core::{Avoidance, MeleeAvoided};
use arenasim::states::play_match::components::{
    Aura, AuraType, CameraController, ClientParticle, Combatant, ImpactAnchor, ImpactRig,
    InstantAbilityFired, ParticleFacing, SchoolImpact, SwingStyle, VisualBody, WeaponHand,
    WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_school_impacts, cleanup_mortal_strike,
    consume_avoided_strike_signals, consume_instant_ability_signals, heroic_strike_trail,
    impact_origin, note_avoided_strikes, spawn_school_impacts, update_weapon_trails,
    AvoidedStrikeSignal, WeaponTrail, HEROIC_STRIKE_TRAIL_COLOR,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;

/// Mortal Strike's stroke reaches the victim this long after the hit
/// (`SwingStyle::impact_at`), and the landing waits for it.
fn impact_at() -> f32 {
    SwingStyle::MortalStrike.impact_at()
}

// ── the bench's numbers (its "Crusader Strike" tab) ─────────────────────────

/// `sparks`: color [[0,[255,209,0]],[0.5,[255,159,35]],[1,[101,0,0]]].
const SPARK_COLOR: [[f32; 3]; 3] = [[255.0, 209.0, 0.0], [255.0, 159.0, 35.0], [101.0, 0.0, 0.0]];
/// alpha [[0,0],[0.5,1],[1,0]].
const SPARK_ALPHA: [f32; 3] = [0.0, 1.0, 0.0];
/// scale [[0,0.16],[1,0.08]], a radius: drawn twice that.
const SPARK_RADIUS: [f32; 2] = [0.16, 0.08];
const SPARK_SPEED: f32 = 2.78;
const SPARK_LIFE: f32 = 0.5;
/// `plume`: color [[0,[174,0,0]],[0.5,[181,181,181]],[1,[238,90,0]]].
const PLUME_COLOR: [[f32; 3]; 3] = [[174.0, 0.0, 0.0], [181.0, 181.0, 181.0], [238.0, 90.0, 0.0]];
/// alpha [[0,1],[1,1]].
const PLUME_ALPHA: [f32; 3] = [1.0, 1.0, 1.0];
/// scale [[0,0.3],[1,0.2]].
const PLUME_RADIUS: [f32; 2] = [0.3, 0.2];
const PLUME_SPEED: f32 = 5.56;
const PLUME_LIFE: f32 = 0.3;
/// Both emitters' `span`, 667 ms; the last spark is born at its end.
const SPAN: f32 = 0.667;

// ── harness ────────────────────────────────────────────────────────────────

/// `resolve_melee_avoidances` despawns each `MeleeAvoided` in the resolution
/// phase; this stands in for it.
fn retire_avoidances(mut commands: Commands, avoided: Query<Entity, With<MeleeAvoided>>) {
    for e in avoided.iter() {
        commands.entity(e).despawn();
    }
}

struct Harness {
    app: App,
    defs: AbilityDefinitions,
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
        // As the client orders them: the avoidance is noted before the
        // resolution phase retires it, and both strokes start after it.
        app.add_systems(
            Update,
            (
                note_avoided_strikes,
                retire_avoidances,
                consume_instant_ability_signals,
                consume_avoided_strike_signals,
                update_weapon_trails,
                cleanup_mortal_strike,
                spawn_school_impacts,
                animate_school_impacts,
                animate_client_particles,
            )
                .chain(),
        );
        // The first update of a MinimalPlugins app advances no time.
        app.update();
        Self {
            app,
            defs: AbilityDefinitions::default(),
        }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / DT).round() as u32);
    }

    /// A combatant of `class` at `at`, its book granted `grants`, carrying a
    /// main-hand socket in the hierarchy `spawn_combatant` builds (sim entity
    /// -> `VisualBody` -> socket). Returns (unit, socket).
    fn unit(
        &mut self,
        class: CharacterClass,
        at: Vec3,
        grants: &[AbilityType],
    ) -> (Entity, Entity) {
        let mut book = AbilityBook::for_class(&self.defs, class);
        for a in grants {
            book = book.granting(&self.defs, *a);
        }
        let team = if at.x < 0.0 { 1 } else { 2 };
        let unit = self
            .app
            .world_mut()
            .spawn((
                Combatant::new(team, 0, class).with_book(book),
                Transform::from_translation(at),
            ))
            .id();
        let body = self
            .app
            .world_mut()
            .spawn((
                VisualBody { rest_y: 1.0 },
                Transform::from_xyz(0.0, 1.0, 0.0),
            ))
            .id();
        self.app.world_mut().entity_mut(unit).add_child(body);
        let socket = self
            .app
            .world_mut()
            .spawn((
                WeaponSocket {
                    kind: WeaponKind::Mace,
                    hand: WeaponHand::Main,
                    owner: unit,
                    rest: Transform::IDENTITY,
                    release_t: None,
                    aim: Vec3::ZERO,
                    yaw_local: 0.0,
                    prev_owner_yaw: 0.0,
                    windup_s: 0.0,
                    swing_style: SwingStyle::Auto,
                    stroke_interval: 0.0,
                    last_s: 0.0,
                },
                Transform::IDENTITY,
                GlobalTransform::default(),
                Visibility::default(),
            ))
            .id();
        self.app.world_mut().entity_mut(body).add_child(socket);
        (unit, socket)
    }

    /// A Retribution Paladin (Crusader Strike granted) at x = -1.5 and a
    /// Warrior victim at the origin.
    fn paladin_and_victim(&mut self) -> (Entity, Entity, Entity) {
        let (paladin, socket) = self.unit(
            CharacterClass::Paladin,
            Vec3::new(-1.5, 0.0, 0.0),
            &[AbilityType::CrusaderStrike],
        );
        let (victim, _) = self.unit(CharacterClass::Warrior, Vec3::ZERO, &[]);
        (paladin, socket, victim)
    }

    /// The landed strike's marker, as the instant drain spawns it.
    fn strike(&mut self, ability: AbilityType, caster: Entity, target: Entity) -> Entity {
        self.app
            .world_mut()
            .spawn(InstantAbilityFired {
                caster,
                target: Some(target),
                ability,
                is_crit: false,
            })
            .id()
    }

    /// An avoided attack's marker, as `avoid_melee_attack` spawns it.
    fn avoided(&mut self, attacker: Entity, defender: Entity, attack: &str, avoidance: Avoidance) {
        self.app.world_mut().spawn(MeleeAvoided {
            attacker,
            defender,
            attack: attack.to_string(),
            avoidance,
        });
    }

    fn name(&self, ability: AbilityType) -> String {
        self.defs.get_unchecked(&ability).name.clone()
    }

    fn count<T: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query_filtered::<Entity, With<T>>();
        q.iter(self.app.world()).count()
    }

    fn socket(&self, socket: Entity) -> &WeaponSocket {
        self.app.world().get::<WeaponSocket>(socket).unwrap()
    }

    /// Every client particle, live or spent: (particle, world transform).
    fn particles(&mut self) -> Vec<(ClientParticle, GlobalTransform)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&ClientParticle, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(p, g)| {
                (
                    ClientParticle {
                        palette: p.palette.clone(),
                        ..*p
                    },
                    *g,
                )
            })
            .collect()
    }

    fn live(&mut self) -> Vec<(ClientParticle, GlobalTransform)> {
        self.particles()
            .into_iter()
            .filter(|(p, _)| p.age < p.life)
            .collect()
    }

    fn material(&self, handle: &Handle<StandardMaterial>) -> StandardMaterial {
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    fn trails(&mut self) -> Vec<StandardMaterial> {
        let mut q = self
            .app
            .world_mut()
            .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<WeaponTrail>>();
        let handles: Vec<_> = q.iter(self.app.world()).map(|m| m.0.clone()).collect();
        handles.iter().map(|h| self.material(h)).collect()
    }

    fn impacts(&mut self) -> Vec<(AbilityType, Entity)> {
        let mut q = self.app.world_mut().query::<&SchoolImpact>();
        q.iter(self.app.world())
            .map(|i| (i.ability, i.target))
            .collect()
    }
}

fn is_spark(p: &ClientParticle) -> bool {
    p.facing == ParticleFacing::Streak
}

fn is_plume(p: &ClientParticle) -> bool {
    p.facing == ParticleFacing::Camera
}

fn chest(at: Vec3) -> Vec3 {
    impact_origin(ImpactAnchor::Chest, at, false)
}

/// A three-key ramp keyed at birth, half-life and death.
fn ramp(keys: [f32; 3], t: f32) -> f32 {
    if t < 0.5 {
        keys[0] + (keys[1] - keys[0]) * (t / 0.5)
    } else {
        keys[1] + (keys[2] - keys[1]) * ((t - 0.5) / 0.5)
    }
}

/// A colour channel drawn as the bench's value: exact where the value sits
/// clear of a rounding tie, either neighbour only within 0.02 of one.
fn channel_matches(got: u8, want: f32) -> bool {
    if (want.fract() - 0.5).abs() > 0.02 {
        got as f32 == want.round()
    } else {
        (got as f32 - want).abs() <= 0.52
    }
}

fn srgb_u8(c: Color) -> [u8; 3] {
    let c = c.to_srgba();
    [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8)
}

// ── the stroke ─────────────────────────────────────────────────────────────

/// A landed Crusader Strike swings Mortal Strike's stroke on the Paladin's
/// main hand, from a full windup, aimed at its victim; the marker is consumed.
#[test]
fn a_landed_strike_swings_mortal_strikes_stroke() {
    let mut h = Harness::new();
    let (paladin, socket, victim) = h.paladin_and_victim();
    let marker = h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick(1);
    let s = h.socket(socket);
    assert_eq!(s.swing_style, SwingStyle::MortalStrike);
    assert_eq!(s.release_t, Some(0.0));
    assert_eq!(s.windup_s, -1.0);
    assert_eq!(s.aim, Vec3::ZERO, "aimed at the victim");
    assert!(h.app.world().get_entity(marker).is_err(), "consumed");
}

/// Only the main hand strokes: a unit carrying a second, off-hand socket
/// keeps that one at rest, landed strike or dodged.
#[test]
fn only_the_main_hand_strokes() {
    for dodged in [false, true] {
        let mut h = Harness::new();
        let (paladin, main, victim) = h.paladin_and_victim();
        let off = h
            .app
            .world_mut()
            .spawn((
                WeaponSocket {
                    kind: WeaponKind::Mace,
                    hand: WeaponHand::Off,
                    owner: paladin,
                    rest: Transform::IDENTITY,
                    release_t: None,
                    aim: Vec3::ZERO,
                    yaw_local: 0.0,
                    prev_owner_yaw: 0.0,
                    windup_s: 0.0,
                    swing_style: SwingStyle::Auto,
                    stroke_interval: 0.0,
                    last_s: 0.0,
                },
                Transform::IDENTITY,
                GlobalTransform::default(),
            ))
            .id();
        if dodged {
            let name = h.name(AbilityType::CrusaderStrike);
            h.avoided(paladin, victim, &name, Avoidance::Parry);
        } else {
            h.strike(AbilityType::CrusaderStrike, paladin, victim);
        }
        h.tick(1);
        assert_eq!(h.socket(main).swing_style, SwingStyle::MortalStrike);
        assert_eq!(
            h.socket(off).swing_style,
            SwingStyle::Auto,
            "dodged {dodged}"
        );
        assert_eq!(h.socket(off).release_t, None, "dodged {dodged}");
    }
}

// ── the trail ──────────────────────────────────────────────────────────────

/// The trail is Heroic Strike's shipped red (the bench's `drawStrike` trail,
/// drawn `lighter`): the shipped colour, additive, unlit, glowing at its
/// gain — and it lasts the stroke plus its 600 ms tail, then goes.
#[test]
fn the_trail_is_heroic_strikes_shipped_red_for_the_stroke_and_its_tail() {
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick(1);
    let trails = h.trails();
    assert_eq!(trails.len(), 1, "one trail");
    let m = &trails[0];
    let style = heroic_strike_trail();
    assert_eq!(
        m.base_color.to_srgba(),
        HEROIC_STRIKE_TRAIL_COLOR.to_srgba(),
        "the shipped red"
    );
    assert_eq!(m.alpha_mode, AlphaMode::Add, "drawn lighter");
    assert!(m.unlit);
    let (er, eg, eb) = style.emissive;
    assert_eq!(m.emissive, LinearRgba::new(er, eg, eb, 1.0));
    assert_eq!(style.lifetime, 0.60, "the client's 600 ms");

    // Sampled while the blade moves, then 600 ms of tail.
    let gone_at = SwingStyle::MortalStrike.stroke_secs() + style.lifetime;
    h.tick_secs(gone_at - 0.1);
    assert_eq!(h.trails().len(), 1, "still fading at {:.2}s", gone_at - 0.1);
    h.tick_secs(0.2);
    assert!(h.trails().is_empty(), "gone after {gone_at:.2}s");
}

// ── the landing ────────────────────────────────────────────────────────────

/// Nothing lands while the blade is still travelling; then the sparks and
/// the plume burst from the victim's chest, flying outward at the bench's
/// speeds, and every one is spent by the end of the bench's span plus its
/// life.
#[test]
fn the_landing_waits_for_the_blade_then_bursts_from_the_chest() {
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick(1);
    assert_eq!(
        h.impacts(),
        vec![(AbilityType::CrusaderStrike, victim)],
        "one landing, on the victim"
    );
    h.tick_secs(impact_at() - 2.0 * DT);
    assert!(
        h.particles().is_empty(),
        "the landing fired before the blade arrived"
    );
    // The plume's puff is born by 67 ms after contact.
    h.tick_secs(0.1);
    let live = h.live();
    assert!(live.iter().any(|(p, _)| is_spark(p)), "sparks");
    assert!(live.iter().any(|(p, _)| is_plume(p)), "the plume");
    let at = chest(Vec3::ZERO);
    for (p, g) in &live {
        let (speed, life) = if is_spark(p) {
            (SPARK_SPEED, SPARK_LIFE)
        } else {
            (PLUME_SPEED, PLUME_LIFE)
        };
        assert!(
            (p.velocity.length() - speed).abs() < 1e-3,
            "{:?} at {}",
            p.facing,
            p.velocity.length()
        );
        let d = g.translation().distance(at);
        assert!(
            (d - speed * p.age).abs() < speed * DT + 1e-3,
            "{:?}: {d:.3} yd from the chest at age {:.3}",
            p.facing,
            p.age
        );
        assert!(p.life == life, "{:?} lives {}", p.facing, p.life);
    }
    // Every particle the landing owes (the last spark is born at the end of
    // the span), and then nothing.
    h.tick_secs(SPAN - 0.1 + 0.05);
    let all = h.particles();
    assert_eq!(
        all.iter().filter(|(p, _)| is_spark(p)).count(),
        31,
        "the bench's 31.7 sparks"
    );
    assert_eq!(
        all.iter().filter(|(p, _)| is_plume(p)).count(),
        1,
        "one plume puff"
    );
    h.tick_secs(SPARK_LIFE);
    assert!(h.live().is_empty(), "all spent");
}

/// The two emitters draw the bench's ramps: each palette step's colour and
/// alpha on the bench's keys (half-life middle keys), additive (`lighter`),
/// on the client-particle kit's soft dot — read back by its alpha falloff.
#[test]
fn the_landing_draws_the_benchs_ramps_additively_on_the_soft_dot() {
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick_secs(impact_at() + 0.1);
    let live = h.live();
    let spark = live
        .iter()
        .find(|(p, _)| is_spark(p))
        .unwrap()
        .0
        .palette
        .clone();
    let plume = live
        .iter()
        .find(|(p, _)| is_plume(p))
        .unwrap()
        .0
        .palette
        .clone();
    assert_ne!(spark, plume, "two palettes");

    let images = h.app.world().resource::<Assets<Image>>();
    let mut textures = Vec::new();
    for (palette, colour, alpha) in [
        (&spark, SPARK_COLOR, SPARK_ALPHA),
        (&plume, PLUME_COLOR, PLUME_ALPHA),
    ] {
        let n = palette.len() as f32;
        for (i, handle) in palette.iter().enumerate() {
            let m = h.material(handle);
            let t = (i as f32 + 0.5) / n;
            let want: [f32; 3] =
                [0, 1, 2].map(|c| ramp([colour[0][c], colour[1][c], colour[2][c]], t));
            let got = srgb_u8(m.base_color);
            assert!(
                (0..3).all(|c| channel_matches(got[c], want[c])),
                "step {i}: {got:?}, the bench {want:?}"
            );
            assert!(
                (m.base_color.alpha() - ramp(alpha, t)).abs() < 1e-5,
                "step {i}: alpha {}",
                m.base_color.alpha()
            );
            assert_eq!(m.alpha_mode, AlphaMode::Add, "drawn lighter");
            textures.push(m.base_color_texture.clone().expect("a sprite"));
        }
    }
    // One sprite for both: the soft dot, (1 - r)^2.2.
    let first = images.get(&textures[0]).unwrap();
    for t in &textures {
        assert_eq!(images.get(t).unwrap().data, first.data);
    }
    let size = first.texture_descriptor.size.width as usize;
    let centre = (size as f32 - 1.0) / 2.0;
    let data = first.data.as_ref().unwrap();
    for x in [size / 2, size * 5 / 8, size * 3 / 4, size * 7 / 8, size - 1] {
        let r = (x as f32 - centre) / centre;
        let y = size / 2;
        let dy = (y as f32 - centre) / centre;
        let r = (r * r + dy * dy).sqrt();
        let want = (1.0 - r).clamp(0.0, 1.0).powf(2.2) * 255.0;
        let got = data[(y * size + x) * 4 + 3] as f32;
        assert!(
            (got - want).abs() <= 1.0,
            "alpha at r {r:.2}: {got} against {want:.1}"
        );
    }
}

/// Each particle is drawn on the unit quad at the bench's size, twice each
/// radius key on its straight line from birth to death; a spark is the soft
/// dot stretched four times along its flight.
#[test]
fn the_particles_are_drawn_at_the_benchs_sizes() {
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick_secs(impact_at() + 0.05);
    let mut seen = (0, 0);
    for _ in 0..20 {
        h.tick(1);
        for (p, g) in h.live() {
            let t = p.age / p.life;
            let (radii, stretch) = if is_spark(&p) {
                seen.0 += 1;
                (SPARK_RADIUS, 4.0)
            } else {
                seen.1 += 1;
                (PLUME_RADIUS, 1.0)
            };
            let want = 2.0 * (radii[0] + (radii[1] - radii[0]) * t);
            let (scale, ..) = g.to_scale_rotation_translation();
            assert!(
                (scale.x - want).abs() < 1e-4,
                "{:?}: {} vs {want}",
                p.facing,
                scale.x
            );
            assert!((scale.y - want * stretch).abs() < 1e-4, "{:?}", p.facing);
        }
    }
    assert!(seen.0 > 0 && seen.1 > 0, "both sampled: {seen:?}");
    // The quad is the unit square.
    let mut q = h.app.world_mut().query::<&ImpactRig>();
    let quad = q.iter(h.app.world()).next().unwrap().mote_mesh.clone();
    let meshes = h.app.world().resource::<Assets<Mesh>>();
    let Some(VertexAttributeValues::Float32x3(positions)) = meshes
        .get(&quad)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("positions");
    };
    for p in positions {
        assert_eq!((p[0].abs(), p[1].abs(), p[2]), (0.5, 0.5, 0.0), "{p:?}");
    }
}

/// Crusader Strike's landing is its own: Heroic Strike's decisivestrike ring
/// never plays for it, and its particles are not Heroic Strike's.
#[test]
fn the_landing_is_not_heroic_strikes() {
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick_secs(impact_at() + 0.4);
    let all = h.particles();
    assert!(!all.is_empty());
    // decisivestrike's ring is a camera-facing particle that never moves.
    assert!(
        all.iter().all(|(p, _)| p.velocity.length() > 1.0),
        "a still particle: a ring?"
    );
}

// ── dodge and parry ────────────────────────────────────────────────────────

/// A dodged or parried strike swings its stroke and lays its trail, and lands
/// nothing — read off the sim's own `MeleeAvoided`, by the name in the
/// attacker's book.
#[test]
fn an_avoided_strike_swings_and_trails_but_lands_nothing() {
    for avoidance in [Avoidance::Dodge, Avoidance::Parry] {
        let mut h = Harness::new();
        let (paladin, socket, victim) = h.paladin_and_victim();
        let name = h.name(AbilityType::CrusaderStrike);
        h.avoided(paladin, victim, &name, avoidance);
        h.tick(1);
        let s = h.socket(socket);
        assert_eq!(s.swing_style, SwingStyle::MortalStrike, "{avoidance:?}");
        assert_eq!(s.release_t, Some(0.0));
        assert_eq!(s.windup_s, -1.0);
        let trails = h.trails();
        assert_eq!(trails.len(), 1, "{avoidance:?}: the trail");
        assert_eq!(
            trails[0].base_color.to_srgba(),
            HEROIC_STRIKE_TRAIL_COLOR.to_srgba()
        );
        assert_eq!(
            h.count::<AvoidedStrikeSignal>(),
            0,
            "the signal is consumed"
        );
        for _ in 0..((impact_at() + SPAN + SPARK_LIFE) / DT) as u32 {
            h.tick(1);
            assert!(h.impacts().is_empty(), "{avoidance:?}: no landing");
            assert!(h.particles().is_empty(), "{avoidance:?}: no particle");
        }
    }
}

/// The controls: an avoided auto-attack, an avoided Mortal Strike, and a
/// "Crusader Strike" from a unit whose book has none — each swings nothing.
#[test]
fn only_an_avoided_crusader_strike_strokes() {
    let mut h = Harness::new();
    let (paladin, paladin_socket, victim) = h.paladin_and_victim();
    let (warrior, warrior_socket) = h.unit(CharacterClass::Warrior, Vec3::new(-1.5, 0.0, 1.0), &[]);
    let (untalented, untalented_socket) =
        h.unit(CharacterClass::Paladin, Vec3::new(-1.5, 0.0, -1.0), &[]);
    let mortal = h.name(AbilityType::MortalStrike);
    let crusader = h.name(AbilityType::CrusaderStrike);
    h.avoided(paladin, victim, "Auto Attack", Avoidance::Dodge);
    h.avoided(warrior, victim, &mortal, Avoidance::Parry);
    h.avoided(untalented, victim, &crusader, Avoidance::Dodge);
    h.tick(1);
    for socket in [paladin_socket, warrior_socket, untalented_socket] {
        assert_eq!(h.socket(socket).release_t, None, "no stroke");
        assert_eq!(h.socket(socket).swing_style, SwingStyle::Auto);
    }
    assert!(h.trails().is_empty());
    assert_eq!(h.count::<AvoidedStrikeSignal>(), 0);
}

/// The name the sim records for an avoided Crusader Strike is the one the
/// graphical side matches: a scene's dodged strike, fed through, strokes.
#[test]
fn the_sims_avoided_strike_name_strokes() {
    const PALADIN: Who = "paladin";
    const VICTIM: Who = "victim";
    let view = Scene::new()
        .unit(PALADIN, 1, CharacterClass::Paladin)
        .targeting(VICTIM)
        .mana(1.0)
        .aura_full(
            PALADIN,
            Aura {
                effect_type: AuraType::DamageTakenReduction,
                duration: 600.0,
                magnitude: 0.10,
                ability_name: "Devotion Aura".into(),
                ..Default::default()
            },
        )
        .grant(PALADIN, AbilityType::CrusaderStrike)
        .cooldown(PALADIN, AbilityType::HammerOfJustice, 30.0)
        .unit(VICTIM, 2, CharacterClass::Priest)
        .at(1.5, 0.0)
        .avoidance(1.0, 0.0)
        .decide(PALADIN);
    assert_eq!(view.avoided.len(), 1, "the strike was dodged");
    let attack = view.avoided[0].attack.clone();

    let mut h = Harness::new();
    let (paladin, socket, victim) = h.paladin_and_victim();
    h.avoided(paladin, victim, &attack, Avoidance::Dodge);
    h.tick(1);
    assert_eq!(h.socket(socket).swing_style, SwingStyle::MortalStrike);
    assert_eq!(h.trails().len(), 1);
}

// ── how it ends ────────────────────────────────────────────────────────────

/// The landing retires on its own clock — its last spark's death, 667 ms
/// plus 0.5 s after contact — with another unit's landing alive beside it,
/// which goes on after it.
#[test]
fn the_landing_retires_on_its_own_clock_beside_another() {
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    let (other, _) = h.unit(CharacterClass::Mage, Vec3::new(4.0, 0.0, 0.0), &[]);
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick(1);
    h.tick_secs(impact_at() + 1.0);
    // Another landing, still playing when this one ends.
    h.strike(AbilityType::CrusaderStrike, paladin, other);
    h.tick(1);
    h.tick_secs(SPAN + SPARK_LIFE - 1.0 - 2.0 * DT);
    assert_eq!(
        h.impacts(),
        vec![
            (AbilityType::CrusaderStrike, victim),
            (AbilityType::CrusaderStrike, other)
        ],
        "both still playing"
    );
    h.tick(4);
    assert_eq!(
        h.impacts(),
        vec![(AbilityType::CrusaderStrike, other)],
        "the first retired, the second plays on"
    );
    h.tick_secs(1.6);
    assert!(h.impacts().is_empty());
}

/// The striker dying mid-stroke takes nothing away: the landing still plays
/// its whole count, and the trail ages out on its own clock.
#[test]
fn the_striker_dying_mid_stroke_leaves_the_landing() {
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick_secs(0.1);
    h.app.world_mut().entity_mut(paladin).despawn();
    h.tick_secs(impact_at() - 0.1 + SPAN + 0.05);
    let all = h.particles();
    assert_eq!(all.iter().filter(|(p, _)| is_spark(p)).count(), 31);
    assert_eq!(all.iter().filter(|(p, _)| is_plume(p)).count(), 1);
    h.tick_secs(SPARK_LIFE + 0.1);
    assert!(h.trails().is_empty(), "the trail ages out");
    assert!(h.impacts().is_empty(), "the landing retires");
}

/// The victim dying mid-landing: a corpse carries it on; a victim gone from
/// the world leaves it where it was. Either way it plays out and retires.
#[test]
fn the_victim_dying_mid_landing_plays_it_out() {
    for despawn in [false, true] {
        let mut h = Harness::new();
        let (paladin, _, victim) = h.paladin_and_victim();
        h.strike(AbilityType::CrusaderStrike, paladin, victim);
        h.tick_secs(impact_at() + 0.2);
        let moved = Vec3::new(0.0, 0.0, 3.0);
        if despawn {
            h.app.world_mut().entity_mut(victim).despawn();
        } else {
            let mut c = h.app.world_mut().get_mut::<Combatant>(victim).unwrap();
            c.current_health = 0.0;
            c.is_dead = true;
            h.app
                .world_mut()
                .get_mut::<Transform>(victim)
                .unwrap()
                .translation = moved;
        }
        h.tick_secs(0.1);
        let rig = {
            let mut q = h
                .app
                .world_mut()
                .query_filtered::<&GlobalTransform, With<SchoolImpact>>();
            q.single(h.app.world()).unwrap().translation()
        };
        let want = if despawn {
            chest(Vec3::ZERO)
        } else {
            chest(moved)
        };
        assert!(
            rig.distance(want) < 1e-4,
            "despawn {despawn}: rig at {rig:?}"
        );
        h.tick_secs(SPAN + SPARK_LIFE);
        assert!(h.impacts().is_empty(), "despawn {despawn}: retired");
        assert_eq!(
            h.count::<ClientParticle>(),
            0,
            "despawn {despawn}: its particles went with it"
        );
    }
}

// ── at the match camera ────────────────────────────────────────────────────

fn match_camera(target: Vec3, zoom: f32, yaw: f32) -> Transform {
    let pitch = CameraController::default().pitch;
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

fn project(camera: &Transform, p: Vec3, size: Vec2) -> Vec2 {
    let projection = PerspectiveProjection {
        aspect_ratio: size.x / size.y,
        ..default()
    };
    let clip = bevy::render::camera::CameraProjection::get_clip_from_view(&projection)
        * camera.compute_matrix().inverse()
        * p.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y)
}

/// Through the default match camera (60 yd, 38.7° pitch, 45° field of view)
/// on a 1080p screen, at every yaw: the sparks' burst, over its life, spans
/// more than twice the victim's body, and a spark at half-life is several
/// pixels across.
#[test]
fn the_landing_reads_at_the_match_camera() {
    let zoom = CameraController::default().zoom_distance;
    assert_eq!(zoom, 60.0);
    let screen = Vec2::new(1920.0, 1080.0);
    let mut h = Harness::new();
    let (paladin, _, victim) = h.paladin_and_victim();
    h.strike(AbilityType::CrusaderStrike, paladin, victim);
    h.tick_secs(impact_at());
    let mut sparks: Vec<Vec3> = Vec::new();
    for _ in 0..((SPAN + SPARK_LIFE - 0.05) / DT) as u32 {
        h.tick(1);
        sparks.extend(
            h.live()
                .iter()
                .filter(|(p, _)| is_spark(p))
                .map(|(_, g)| g.translation()),
        );
    }
    assert!(!sparks.is_empty());
    let at = chest(Vec3::ZERO);
    for step in 0..8 {
        let cam = match_camera(Vec3::ZERO, zoom, step as f32 * TAU / 8.0);
        let px: Vec<Vec2> = sparks.iter().map(|p| project(&cam, *p, screen)).collect();
        let span = px.iter().map(|p| p.x).fold(f32::MIN, f32::max)
            - px.iter().map(|p| p.x).fold(f32::MAX, f32::min);
        let body = (project(&cam, at + cam.right() * 0.5, screen)
            - project(&cam, at - cam.right() * 0.5, screen))
        .length();
        let spark = (project(&cam, at + cam.right() * 0.12, screen)
            - project(&cam, at - cam.right() * 0.12, screen))
        .length();
        assert!(
            span > 2.0 * body,
            "yaw {step}: burst {span:.0}px, body {body:.0}px"
        );
        assert!(spark > 4.0, "yaw {step}: a spark {spark:.1}px");
        eprintln!("1080p yaw {step}: burst {span:.0}px, body {body:.0}px, spark {spark:.1}px");
    }
}
