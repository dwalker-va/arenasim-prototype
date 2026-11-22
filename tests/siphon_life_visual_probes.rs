//! Probes for Siphon Life's DoT state (`rendering/effects/siphon_life.rs`,
//! AS-347): the TBC client's kit 234, `corruption_impactdot_med_base.m2`, on
//! the victim's chest — violet rings and flares bursting out, dark streaks
//! pulled in — held while the DoT runs, with Corruption's apply as it lands.
//!
//! These assert what is DRAWN, under propagated `GlobalTransform`s: where
//! each particle is in the world and which way it flies, the colour, alpha,
//! blend, emissive and texture falloff of the material it wears, and its
//! quad's size. Every bench value is transcribed below as a literal, from
//! the signed-off AS-334 bench's "Siphon Life" tab
//! (`docs/design/benches/2026-10-17-warlock-talent-bench.html`), never read
//! back from the module's own tables.
//!
//! The lifecycle probes run Corruption's whole chain beside Siphon Life's —
//! they share the DoT router and the apply burst — and put Corruption on the
//! same victim.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`, no window
//! and no GPU, with the chains `states/mod.rs` registers.

use std::collections::{BTreeSet, HashSet};
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, CorruptionShroudRig, DotApplyBurst, Pet, PetType,
    PlayMatchEntity, PolymorphedVisual,
};
use arenasim::states::play_match::{
    age_siphon_life_particles, age_warlock_dot_particles, animate_corruption_shrouds,
    animate_curse_apparitions, animate_dot_apply_bursts, animate_siphon_life_glows,
    animate_ua_states, billboard_siphon_life_particles, billboard_warlock_dot_visuals,
    cleanup_play_match, cleanup_siphon_life_glows, cleanup_warlock_dot_visuals,
    orient_curse_apparitions, spawn_warlock_dot_visuals, AbilityDefinitions, AbilityType,
    DotStateVisual, SiphonLifePart, SiphonLifeParticle, SiphonLifeRig, CORRUPTION_AURA,
};
use arenasim::CharacterClass;

// ── The bench, transcribed ───────────────────────────────────────────────────

const AURA: &str = "Siphon Life";
/// `ANCH.chest`, above the floor; combatants stand with their capsule centre
/// 1.0 above it.
const CHEST_ABOVE_CENTRE: f32 = 1.45 - 1.0;

/// Emitter indices, in the module's table order (P0, P1, P2).
const RINGS: usize = 0;
const FLARES: usize = 1;
const STREAKS: usize = 2;

struct Layer {
    speed: f32,
    life: f32,
    rate: f32,
    additive: bool,
    color: [[u8; 3]; 3],
    alpha: [f32; 3],
    /// Sprite radius at life 0 / 0.5 / 1.
    scale: [f32; 3],
}

const VIOLET: [[u8; 3]; 3] = [[22, 0, 133], [83, 0, 186], [112, 0, 183]];

const BENCH: [Layer; 3] = [
    Layer {
        speed: 1.67,
        life: 1.5,
        rate: 30.0,
        additive: true,
        color: VIOLET,
        alpha: [0.55, 0.7, 0.4],
        scale: [1.39, 0.56, 0.28],
    },
    Layer {
        speed: 1.67,
        life: 1.5,
        rate: 30.0,
        additive: true,
        color: VIOLET,
        alpha: [0.78, 1.0, 0.78],
        scale: [0.3, 0.15, 0.06],
    },
    Layer {
        speed: 3.89,
        life: 0.55,
        rate: 25.0,
        additive: false,
        color: [[0, 0, 0], [37, 0, 82], [112, 0, 183]],
        alpha: [0.0, 1.0, 0.0],
        scale: [0.139, 0.139, 0.139],
    },
];
/// The streaks' shell: born `0.6..1.0` of it out.
const SHELL: f32 = 2.22;
/// `min(tail * 0.12, speed * 0.06)` at tail 1.5, 3.89 yd/s.
const STREAK_LENGTH: f32 = 0.18;
/// The bench draws a streak at 0.85 of its sprite's alpha.
const STREAK_ALPHA: f32 = 0.85;

/// A three-key track (keys at 0, 0.5, 1), linear between.
fn track(keys: [f32; 3], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.5 {
        keys[0] + (keys[1] - keys[0]) * (t / 0.5)
    } else {
        keys[1] + (keys[2] - keys[1]) * ((t - 0.5) / 0.5)
    }
}

// ── The body and the camera ──────────────────────────────────────────────────

/// Combatants stand with capsule centres at world y = 1.0.
const COMBATANT_Y: f32 = 1.0;
/// Pets stand at sim y = 0.75 (`play_match/mod.rs`).
const PET_SIM_Y: f32 = 0.75;
/// A pet's rendered body hangs 0.45 below its sim transform and is 0.55 of a
/// combatant (`IMPACT_PET_BODY_Y`, `IMPACT_PET_STATURE`).
const PET_BODY_Y: f32 = -0.45;
const PET_STATURE: f32 = 0.55;
/// How close in front of the body's nearest point a held quad stands.
const HELD_WITHIN: f32 = 0.1;
/// The combatant body mesh, `Capsule3d::new(0.5, 1.5)`.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_SEGMENT: f32 = 0.75;

/// The match camera (`CameraController`) at `zoom` and `pitch` from the
/// vertical, and `yaw`, looking at `target`.
fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

// ── Harness ──────────────────────────────────────────────────────────────────

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;

struct Harness {
    app: App,
}

/// How many Siphon Life rigs and apply bursts have been built so far.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
struct Built {
    rigs: usize,
    bursts: usize,
}

fn count_built(
    mut built: ResMut<Built>,
    rigs: Query<(), Added<SiphonLifeRig>>,
    bursts: Query<(), Added<DotApplyBurst>>,
) {
    built.rigs += rigs.iter().count();
    built.bursts += bursts.iter().count();
}

/// One live particle, read back from the world.
#[derive(Clone)]
struct Seen {
    entity: Entity,
    emitter: usize,
    /// Where it is DRAWN.
    at: Vec3,
    velocity: Vec3,
    age: f32,
}

/// One drawn quad.
struct Quad {
    emitter: usize,
    part: SiphonLifePart,
    age: f32,
    scale: Vec3,
    material: StandardMaterial,
    mesh: Handle<Mesh>,
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
        // Corruption's chain: it shares the router and draws the apply burst.
        app.add_systems(
            Update,
            (
                spawn_warlock_dot_visuals,
                animate_dot_apply_bursts,
                animate_corruption_shrouds,
                animate_curse_apparitions,
                animate_ua_states,
                age_warlock_dot_particles,
                orient_curse_apparitions,
                billboard_warlock_dot_visuals,
                cleanup_warlock_dot_visuals,
            )
                .chain(),
        );
        app.add_systems(
            Update,
            (
                animate_siphon_life_glows,
                age_siphon_life_particles,
                billboard_siphon_life_particles,
                cleanup_siphon_life_glows,
            )
                .chain()
                // The Warlock DoT detector builds the rig (`states/mod.rs`).
                .after(spawn_warlock_dot_visuals),
        );
        // Counts every rig and apply burst as it is BUILT, before either
        // cleanup can take it down the same frame.
        app.init_resource::<Built>();
        app.add_systems(
            Update,
            count_built
                .after(spawn_warlock_dot_visuals)
                .before(cleanup_siphon_life_glows)
                .before(cleanup_warlock_dot_visuals),
        );
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness { app }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / TICK_SECS).ceil() as u32);
    }

    fn dot(name: &str) -> Aura {
        Aura {
            effect_type: AuraType::DamageOverTime,
            duration: 30.0,
            magnitude: 4.0,
            tick_interval: 3.0,
            time_until_next_tick: 3.0,
            ability_name: name.to_string(),
            ..Default::default()
        }
    }

    fn victim(&mut self, at: Vec3, dots: &[&str]) -> Entity {
        let auras = ActiveAuras {
            auras: dots.iter().map(|n| Self::dot(n)).collect(),
        };
        self.app
            .world_mut()
            .spawn((
                Combatant::new(2, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
                auras,
            ))
            .id()
    }

    fn camera(&mut self, transform: Transform) {
        self.app.world_mut().spawn((
            Camera3d::default(),
            transform,
            GlobalTransform::from(transform),
        ));
    }

    fn auras(&mut self, victim: Entity) -> Mut<'_, ActiveAuras> {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(victim)
            .expect("the victim has auras")
    }

    fn remove(&mut self, victim: Entity, name: &str) {
        self.auras(victim).auras.retain(|a| a.ability_name != name);
    }

    fn add(&mut self, victim: Entity, name: &str) {
        self.auras(victim).auras.push(Self::dot(name));
    }

    fn built(&self) -> Built {
        *self.app.world().resource::<Built>()
    }

    fn count<C: Component>(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&C>().iter(world).count()
    }

    fn entities<C: Component>(&mut self) -> HashSet<Entity> {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<C>>()
            .iter(world)
            .collect()
    }

    fn rigs(&mut self) -> Vec<(Entity, Entity, Vec3)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &SiphonLifeRig, &GlobalTransform)>()
            .iter(world)
            .map(|(e, r, g)| (e, r.target, g.translation()))
            .collect()
    }

    fn particles(&mut self) -> Vec<Seen> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &SiphonLifeParticle, &GlobalTransform)>()
            .iter(world)
            .map(|(entity, p, g)| Seen {
                entity,
                emitter: p.emitter,
                at: g.translation(),
                velocity: p.velocity,
                age: p.age,
            })
            .collect()
    }

    /// (emitter, part, world rotation) of every drawn quad.
    fn quads_with_rotation(&mut self) -> Vec<(usize, SiphonLifePart, Quat)> {
        let world = self.app.world_mut();
        let owners: Vec<(usize, Vec<Entity>)> = world
            .query::<(&SiphonLifeParticle, &Children)>()
            .iter(world)
            .map(|(p, c)| (p.emitter, c.iter().collect()))
            .collect();
        owners
            .into_iter()
            .flat_map(|(e, children)| {
                children
                    .into_iter()
                    .map(|c| {
                        let part = *world.get::<SiphonLifePart>(c).unwrap();
                        let g = world.get::<GlobalTransform>(c).unwrap();
                        (e, part, g.compute_transform().rotation)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn quads(&mut self) -> Vec<Quad> {
        let world = self.app.world_mut();
        let owners: Vec<(usize, f32, Vec<Entity>)> = world
            .query::<(&SiphonLifeParticle, &Children)>()
            .iter(world)
            .map(|(p, c)| (p.emitter, p.age, c.iter().collect()))
            .collect();
        let mut parts = world.query::<(
            &SiphonLifePart,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            &Mesh3d,
        )>();
        let mut out = Vec::new();
        for (emitter, age, children) in owners {
            for child in children {
                let (part, g, material, mesh) = parts.get(world, child).unwrap();
                let (scale, _, _) = g.to_scale_rotation_translation();
                let material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&material.0)
                    .unwrap()
                    .clone();
                out.push(Quad {
                    emitter,
                    part: *part,
                    age,
                    scale,
                    material,
                    mesh: mesh.0.clone(),
                });
            }
        }
        out
    }
}

fn chest(at: Vec3) -> Vec3 {
    at + Vec3::Y * CHEST_ABOVE_CENTRE
}

fn assert_nonempty<T>(label: &str, v: &[T]) {
    assert!(!v.is_empty(), "{label}: the probe went vacuous");
}

/// An sRGB channel as the 8-bit value the bench wrote.
fn byte(c: f32) -> u8 {
    (c * 255.0).round() as u8
}

fn rgb(color: Color) -> [u8; 3] {
    let c = color.to_srgba();
    [byte(c.red), byte(c.green), byte(c.blue)]
}

// ── The route ────────────────────────────────────────────────────────────────

#[test]
fn siphon_life_routes_to_its_own_state() {
    let defs = AbilityDefinitions::default();
    assert_eq!(defs.get_unchecked(&AbilityType::SiphonLife).name, AURA);
    assert_eq!(
        DotStateVisual::for_dot(AURA),
        Some(DotStateVisual::SiphonLifeGlow)
    );
    // Not Corruption's (the Era reading the TBC ruling replaced), and not a
    // DoT-less aura that shares the name.
    assert_ne!(
        DotStateVisual::for_dot(AURA),
        DotStateVisual::for_dot(CORRUPTION_AURA)
    );
    let not_a_dot = Aura {
        effect_type: AuraType::HealingOverTime,
        ability_name: AURA.to_string(),
        ..Default::default()
    };
    assert_eq!(DotStateVisual::for_aura(&not_a_dot), None);
}

#[test]
fn a_siphon_life_victim_wears_the_glow_and_a_corruption_victim_does_not() {
    let mut h = Harness::new();
    let siphoned = h.victim(Vec3::new(3.0, COMBATANT_Y, 0.0), &[AURA]);
    let _corrupted = h.victim(Vec3::new(-3.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA]);
    h.tick(3);
    let targets: Vec<Entity> = h.rigs().iter().map(|r| r.1).collect();
    assert_eq!(targets, vec![siphoned]);
    assert!(!h.particles().is_empty(), "the glow emits");
}

// ── Geometry ─────────────────────────────────────────────────────────────────

#[test]
fn the_rig_stands_at_the_victims_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(4.0, COMBATANT_Y, -7.0);
    h.victim(at, &[AURA]);
    h.tick(2);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert!(rigs[0].2.distance(chest(at)) < 1e-5, "{:?}", rigs[0].2);
}

#[test]
fn rings_and_flares_burst_out_of_the_chest_in_every_direction() {
    let mut h = Harness::new();
    let at = Vec3::new(-2.0, COMBATANT_Y, 5.0);
    h.victim(at, &[AURA]);
    let mut directions: Vec<Vec3> = Vec::new();
    let mut seen_ids = HashSet::new();
    for _ in 0..60 {
        h.tick(2);
        for p in h.particles() {
            if p.emitter == STREAKS {
                continue;
            }
            let out = p.at - chest(at);
            // Born AT the chest, flying straight out at the bench's speed.
            assert!(
                (out.length() - BENCH[p.emitter].speed * p.age).abs() < 1e-3,
                "emitter {} is {} yd out at age {}",
                p.emitter,
                out.length(),
                p.age
            );
            assert!((p.velocity.length() - BENCH[p.emitter].speed).abs() < 1e-4);
            if out.length() > 0.05 {
                let along = out.normalize().dot(p.velocity.normalize());
                assert!(along > 0.9999, "a burst particle flies off its ray");
                if seen_ids.insert(p.entity) {
                    directions.push(out.normalize());
                }
            }
        }
    }
    assert!(
        directions.len() > 50,
        "only {} directions",
        directions.len()
    );
    let octants: BTreeSet<(bool, bool, bool)> = directions
        .iter()
        .map(|d| (d.x > 0.0, d.y > 0.0, d.z > 0.0))
        .collect();
    assert_eq!(octants.len(), 8, "the burst fills the sphere");
    let mean = directions.iter().copied().sum::<Vec3>() / directions.len() as f32;
    assert!(mean.length() < 0.2, "no side is favoured: {mean:?}");
}

#[test]
fn the_dark_streaks_are_pulled_in_from_a_shell_they_fill() {
    let mut h = Harness::new();
    let at = Vec3::new(1.0, COMBATANT_Y, 2.0);
    h.victim(at, &[AURA]);
    let mut births: Vec<f32> = Vec::new();
    let mut seen_ids = HashSet::new();
    for _ in 0..250 {
        h.tick(1);
        for p in h.particles().into_iter().filter(|p| p.emitter == STREAKS) {
            assert!((p.velocity.length() - BENCH[STREAKS].speed).abs() < 1e-4);
            // Where it was born: straight back along its flight.
            let born = p.at - p.velocity * p.age - chest(at);
            let inward = (-born).normalize().dot(p.velocity.normalize());
            assert!(inward > 0.9999, "a streak flies off the chest line");
            if seen_ids.insert(p.entity) {
                births.push(born.length());
            }
        }
    }
    assert!(births.len() > 80, "only {} births", births.len());
    let min = births.iter().copied().fold(f32::MAX, f32::min);
    let max = births.iter().copied().fold(0.0, f32::max);
    assert!(
        min >= 0.6 * SHELL - 1e-3 && max <= SHELL + 1e-3,
        "births outside the shell: {min}..{max}"
    );
    assert!(
        min < 0.6 * SHELL + 0.1 && max > SHELL - 0.1,
        "births must FILL the 0.6-1.0 shell, not crowd one radius: {min}..{max}"
    );
}

#[test]
fn every_layer_emits_at_the_bench_rate() {
    let mut h = Harness::new();
    h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[AURA]);
    h.tick_secs(3.0);
    let live = h.particles();
    for (i, layer) in BENCH.iter().enumerate() {
        let n = live.iter().filter(|p| p.emitter == i).count() as f32;
        let expected = layer.rate * layer.life;
        assert!(
            (n - expected).abs() <= 2.0,
            "emitter {i}: {n} live, expected {expected}"
        );
    }
}

#[test]
fn each_quad_is_sized_along_the_bench_scale_track() {
    let mut h = Harness::new();
    h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[AURA]);
    h.tick_secs(2.0);
    let sprites: Vec<Quad> = h
        .quads()
        .into_iter()
        .filter(|q| q.part == SiphonLifePart::Sprite)
        .collect();
    assert_nonempty("sprites", &sprites);
    for q in &sprites {
        let layer = &BENCH[q.emitter];
        let radius = track(layer.scale, q.age / layer.life);
        assert!(
            (q.scale.x - 2.0 * radius).abs() < 1e-4 && (q.scale.y - 2.0 * radius).abs() < 1e-4,
            "emitter {} at age {}: quad {:?}, radius {radius}",
            q.emitter,
            q.age,
            q.scale
        );
    }
    // Every layer is in the sample.
    let layers: BTreeSet<usize> = sprites.iter().map(|q| q.emitter).collect();
    assert_eq!(layers, BTreeSet::from([RINGS, FLARES, STREAKS]));
}

#[test]
fn a_streak_trails_back_along_its_flight() {
    let mut h = Harness::new();
    h.victim(Vec3::new(2.0, COMBATANT_Y, -1.0), &[AURA]);
    h.tick_secs(1.0);
    let particles = h.particles();
    let quads = h.quads();
    let streaks: Vec<&Quad> = quads
        .iter()
        .filter(|q| q.part == SiphonLifePart::Streak)
        .collect();
    assert!(streaks.len() >= 8, "only {} streaks", streaks.len());
    assert!(
        quads
            .iter()
            .filter(|q| q.part == SiphonLifePart::Streak)
            .all(|q| q.emitter == STREAKS),
        "only the dark streaks have tails"
    );
    let mut checked = 0;
    for p in particles.iter().filter(|p| p.emitter == STREAKS) {
        let world = h.app.world_mut();
        let children: Vec<Entity> = world.get::<Children>(p.entity).unwrap().iter().collect();
        for child in children {
            if *world.get::<SiphonLifePart>(child).unwrap() != SiphonLifePart::Streak {
                continue;
            }
            let g = world.get::<GlobalTransform>(child).unwrap();
            let offset = g.translation() - p.at;
            assert!(
                (offset.length() - STREAK_LENGTH * 0.5).abs() < 1e-4,
                "the streak's middle sits {} back",
                offset.length()
            );
            assert!(
                offset.normalize().dot(p.velocity.normalize()) < -0.9999,
                "the streak must trail BEHIND the flight"
            );
            let scale = g.compute_transform().scale;
            assert!((scale.x - STREAK_LENGTH).abs() < 1e-4);
            // The bench's line is 0.7 of the sprite's radius wide.
            assert!((scale.y - 0.7 * BENCH[STREAKS].scale[0]).abs() < 1e-4);
            checked += 1;
        }
    }
    assert!(checked >= 8, "only {checked} streaks checked");
}

// ── What each quad wears ─────────────────────────────────────────────────────

#[test]
fn each_layer_draws_the_bench_colour_and_alpha_at_birth_mid_life_and_death() {
    let mut h = Harness::new();
    h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[AURA]);
    // (emitter, part, key) -> checked.
    let mut hit: BTreeSet<(usize, bool, usize)> = BTreeSet::new();
    for _ in 0..200 {
        h.tick(1);
        for q in h.quads() {
            let layer = &BENCH[q.emitter];
            let t = q.age / layer.life;
            // Within a thirty-second of a life of a key, the palette draws
            // it (a newborn streak is one 16 ms frame, 0.029, into its life).
            let Some(key) = [0.0, 0.5, 1.0]
                .iter()
                .position(|k| (t - k).abs() < 1.0 / 32.0)
            else {
                continue;
            };
            let streak = q.part == SiphonLifePart::Streak;
            let gain = if streak { STREAK_ALPHA } else { 1.0 };
            assert_eq!(
                rgb(q.material.base_color),
                layer.color[key],
                "emitter {} key {key}",
                q.emitter
            );
            let alpha = q.material.base_color.alpha();
            assert!(
                (alpha - layer.alpha[key] * gain).abs() < 1e-5,
                "emitter {} key {key}: alpha {alpha}",
                q.emitter
            );
            hit.insert((q.emitter, streak, key));
        }
    }
    let want: BTreeSet<(usize, bool, usize)> = [RINGS, FLARES, STREAKS]
        .into_iter()
        .flat_map(|e| (0..3).map(move |k| (e, false, k)))
        .chain((0..3).map(|k| (STREAKS, true, k)))
        .collect();
    assert_eq!(hit, want, "every layer's every key was seen drawn");
}

#[test]
fn the_rings_and_flares_add_and_the_streaks_darken() {
    let mut h = Harness::new();
    h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[AURA]);
    h.tick_secs(1.0);
    let quads = h.quads();
    let adding: BTreeSet<usize> = quads
        .iter()
        .filter(|q| q.material.alpha_mode == AlphaMode::Add)
        .map(|q| q.emitter)
        .collect();
    let darkening: BTreeSet<(usize, SiphonLifePart)> = quads
        .iter()
        .filter(|q| q.material.alpha_mode == AlphaMode::Blend)
        .map(|q| (q.emitter, q.part))
        .collect();
    let bench_adds: BTreeSet<usize> = (0..3).filter(|&i| BENCH[i].additive).collect();
    assert_eq!(adding, bench_adds);
    assert_eq!(
        darkening,
        BTreeSet::from([
            (STREAKS, SiphonLifePart::Sprite),
            (STREAKS, SiphonLifePart::Streak)
        ])
    );
    assert_eq!(
        quads.len(),
        quads
            .iter()
            .filter(|q| matches!(q.material.alpha_mode, AlphaMode::Add | AlphaMode::Blend))
            .count(),
        "no quad is opaque or masked"
    );
    for q in &quads {
        let base = q.material.base_color.to_linear();
        let e = q.material.emissive;
        if q.material.alpha_mode == AlphaMode::Add {
            // Lit-emissive, the glow carrying the base colour's own hue.
            assert!(!q.material.unlit, "unlit would discard the emissive");
            let gain = e.blue / base.blue;
            assert!((2.0..=4.0).contains(&gain), "glow gain {gain}");
            assert!((e.red - base.red * gain).abs() < 1e-5);
            assert!((e.green - base.green * gain).abs() < 1e-5);
        } else {
            // A flat colour that darkens: no light of its own.
            assert!(q.material.unlit);
            assert_eq!((e.red, e.green, e.blue), (0.0, 0.0, 0.0));
        }
        assert!(q.material.double_sided && q.material.cull_mode.is_none());
    }
}

/// Alpha of the texture `material` wears, `fx`, `fy` across it (0..1).
fn texel_alpha(h: &mut Harness, material: &StandardMaterial, fx: f32, fy: f32) -> f32 {
    let handle = material
        .base_color_texture
        .clone()
        .expect("a sprite texture");
    let images = h.app.world().resource::<Assets<Image>>();
    let image = images.get(&handle).unwrap();
    let w = image.width();
    let x = ((fx * w as f32) as u32).min(w - 1);
    let y = ((fy * w as f32) as u32).min(w - 1);
    let data = image.data.as_ref().unwrap();
    data[((y * w + x) * 4 + 3) as usize] as f32 / 255.0
}

#[test]
fn rings_and_streaks_are_soft_glows_and_flares_are_stars_on_a_unit_quad() {
    let mut h = Harness::new();
    h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[AURA]);
    h.tick_secs(1.0);
    let quads = h.quads();
    for emitter in [RINGS, FLARES, STREAKS] {
        let q = quads
            .iter()
            .find(|q| q.emitter == emitter && q.part == SiphonLifePart::Sprite)
            .expect("a sprite of every layer");
        let m = q.material.clone();
        let mesh = h
            .app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&q.mesh)
            .unwrap()
            .clone();
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .unwrap();
        let corners: BTreeSet<(i32, i32, i32)> = positions
            .iter()
            .map(|p| {
                (
                    (p[0] * 10.0) as i32,
                    (p[1] * 10.0) as i32,
                    (p[2] * 10.0) as i32,
                )
            })
            .collect();
        assert_eq!(
            corners,
            BTreeSet::from([(-5, -5, 0), (-5, 5, 0), (5, -5, 0), (5, 5, 0)]),
            "a unit quad, facing +Z"
        );
        let centre = texel_alpha(&mut h, &m, 0.5, 0.5);
        let rim = texel_alpha(&mut h, &m, 0.999, 0.5);
        assert!(centre > 0.97, "emitter {emitter}: centre {centre}");
        assert!(rim < 0.03, "emitter {emitter}: rim {rim}");
        // 0.45 of the way out, along the axis and along the diagonal.
        let r = 0.45 * 0.5;
        let axis = texel_alpha(&mut h, &m, 0.5 + r, 0.5);
        let diagonal = texel_alpha(&mut h, &m, 0.5 + r / 2f32.sqrt(), 0.5 + r / 2f32.sqrt());
        match emitter {
            FLARES => assert!(
                axis > 1.5 * diagonal,
                "a flare is a four-point star: axis {axis}, diagonal {diagonal}"
            ),
            _ => {
                // The bench's glow stops: 1 at the centre, 0.45 at 0.45 out.
                assert!((axis - 0.45).abs() < 0.03, "emitter {emitter}: {axis}");
                assert!(
                    (diagonal - 0.45).abs() < 0.03,
                    "emitter {emitter}: {diagonal}"
                );
            }
        }
    }
}

// ── Held in front of the body ────────────────────────────────────────────────

/// The nearest view depth any point of a combatant's body capsule reaches.
fn body_nearest_depth(victim: Vec3, eye: Vec3, forward: Vec3) -> f32 {
    let ends = [
        victim - Vec3::Y * BODY_HALF_SEGMENT,
        victim + Vec3::Y * BODY_HALF_SEGMENT,
    ];
    ends.iter()
        .map(|e| (*e - eye).dot(forward))
        .fold(f32::MAX, f32::min)
        - BODY_RADIUS
}

/// At the match camera, from every pitch it allows and the nearest and its
/// default zoom, every quad is camera-facing and stands nearer than every
/// point of the body — so none is buried, and each sorts in front of the
/// body and any shroud on its axis — while it holds the bench's place and
/// size on screen.
#[test]
fn every_quad_stands_in_front_of_the_body_where_the_bench_puts_it_on_screen() {
    let mut moved = 0;
    let mut checked = 0;
    let mut streaks = 0;
    for (zoom, pitch, yaw) in [
        (60.0, 38.7f32.to_radians(), 0.0),
        (60.0, 38.7f32.to_radians(), 2.3),
        (20.0, 1.5, 4.0),
        (60.0, 0.1, 1.0),
    ] {
        let mut h = Harness::new();
        let victim = Vec3::new(3.0, COMBATANT_Y, -2.0);
        let cam = match_camera(victim, zoom, pitch, yaw);
        h.camera(cam);
        h.victim(victim, &[AURA]);
        h.tick_secs(1.2);
        let eye = cam.translation;
        let forward = cam.forward().as_vec3();
        let nearest = body_nearest_depth(victim, eye, forward);
        let centre_depth = (victim - eye).dot(forward);

        let world = h.app.world_mut();
        let particles: Vec<(Vec3, Vec3, Vec3, f32, usize, Vec<Entity>)> = world
            .query::<(&SiphonLifeParticle, &GlobalTransform, &Children)>()
            .iter(world)
            .map(|(p, g, c)| {
                (
                    p.offset,
                    g.translation(),
                    p.velocity,
                    p.age,
                    p.emitter,
                    c.iter().collect(),
                )
            })
            .collect();
        for (offset, drawn, velocity, age, emitter, children) in particles {
            let truly = chest(victim) + offset;
            let true_depth = (truly - eye).dot(forward);
            // The hold's ratio, from where the particle is drawn.
            let k = (drawn - eye).dot(forward) / true_depth;
            for child in children {
                let part = *world.get::<SiphonLifePart>(child).unwrap();
                let g = world
                    .get::<GlobalTransform>(child)
                    .unwrap()
                    .compute_transform();
                // Camera-facing: the quad's normal is the view direction, so
                // its whole face sits at one depth.
                let normal = g.rotation * Vec3::Z;
                assert!(
                    normal.dot(-forward) > 0.9999,
                    "a quad turned off the camera"
                );
                let depth = (g.translation - eye).dot(forward);
                assert!(
                    depth < nearest,
                    "a quad at depth {depth} behind the body's nearest point {nearest}"
                );
                assert!(depth < centre_depth);
                if part == SiphonLifePart::Streak {
                    // The streak's screen line, held by the same ratio: its
                    // tail projected into the screen plane, and 0.7 of the
                    // sprite's radius wide.
                    let back = -velocity.normalize();
                    let flat = (back - forward * back.dot(forward)).length();
                    assert!(
                        (g.scale.x - STREAK_LENGTH * flat * k).abs() < 1e-4,
                        "streak length {} vs {}",
                        g.scale.x,
                        STREAK_LENGTH * flat * k
                    );
                    assert!(
                        (g.scale.y - 0.7 * BENCH[STREAKS].scale[0] * k).abs() < 1e-5,
                        "streak width {} vs {}",
                        g.scale.y,
                        0.7 * BENCH[STREAKS].scale[0] * k
                    );
                    streaks += 1;
                    continue;
                }
                // On the true point's view ray, scaled to keep its screen size.
                let ray = (truly - eye).normalize();
                assert!(
                    (g.translation - eye).normalize().dot(ray) > 0.999_999,
                    "a held quad left its screen position"
                );
                assert!((depth / true_depth - k).abs() < 1e-4);
                let layer = &BENCH[emitter];
                let radius = track(layer.scale, age / layer.life);
                assert!(
                    (g.scale.x - 2.0 * radius * k).abs() < 1e-4,
                    "screen size: {} vs {}",
                    g.scale.x,
                    2.0 * radius * k
                );
                if k < 0.999 {
                    // Held only as far as it must be: just in front of the
                    // body, never floated off toward the camera.
                    assert!(
                        depth > nearest - HELD_WITHIN,
                        "held {} in front of the body",
                        nearest - depth
                    );
                    moved += 1;
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 200, "only {checked} sprites checked");
    assert!(moved > 50, "only {moved} sprites needed holding — vacuous");
    assert!(streaks > 30, "only {streaks} streaks checked");
}

/// The hold follows the body: a victim that walks toward the camera after
/// its rig is built keeps every quad in front of where it now stands.
#[test]
fn the_hold_follows_a_victim_walking_toward_the_camera() {
    let mut h = Harness::new();
    let start = Vec3::new(0.0, COMBATANT_Y, -4.0);
    let cam = match_camera(
        Vec3::new(0.0, COMBATANT_Y, 0.0),
        60.0,
        38.7f32.to_radians(),
        0.0,
    );
    h.camera(cam);
    let victim = h.victim(start, &[AURA]);
    h.tick_secs(0.8);
    let eye = cam.translation;
    let forward = cam.forward().as_vec3();
    // Eight yards nearer the camera, in one step.
    let moved = start + Vec3::new(0.0, 0.0, 8.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(victim)
        .unwrap()
        .translation = moved;
    h.tick(1);
    let nearest = body_nearest_depth(moved, eye, forward);
    let world = h.app.world_mut();
    let depths: Vec<f32> = world
        .query::<(&SiphonLifePart, &GlobalTransform)>()
        .iter(world)
        .map(|(_, g)| (g.translation() - eye).dot(forward))
        .collect();
    assert!(depths.len() > 60, "only {} quads", depths.len());
    let held = depths
        .iter()
        .filter(|d| **d > nearest - HELD_WITHIN)
        .count();
    assert!(held > 20, "only {held} quads held at the new body");
    for depth in depths {
        assert!(
            depth < nearest,
            "a quad at {depth} behind the moved body's {nearest}"
        );
    }
}

/// A flare's roll is drawn once, at its birth, and held for its whole life:
/// the bench draws `ang` at spawn and gives its flares no `spin`. With the
/// camera fixed, every flare still alive a few frames on wears the roll it
/// wore before — a spin or a per-frame re-draw would keep the spread uniform
/// and so pass the spread probe below.
#[test]
fn a_flares_roll_is_held_for_its_life() {
    let mut h = Harness::new();
    let victim = Vec3::new(-2.0, COMBATANT_Y, 3.0);
    let cam = match_camera(victim, 60.0, 38.7f32.to_radians(), 2.0);
    h.camera(cam);
    h.victim(victim, &[AURA]);
    // Past one flare life (1.5 s), so the flares are at their steady ~45.
    h.tick_secs(2.0);
    let (right, up) = (cam.rotation * Vec3::X, cam.rotation * Vec3::Y);
    let rolls = |h: &mut Harness| -> Vec<(Entity, f32)> {
        let world = h.app.world_mut();
        let flares: Vec<(Entity, Vec<Entity>)> = world
            .query::<(Entity, &SiphonLifeParticle, &Children)>()
            .iter(world)
            .filter(|(_, p, _)| p.emitter == FLARES)
            .map(|(e, _, c)| (e, c.iter().collect()))
            .collect();
        flares
            .into_iter()
            .filter_map(|(e, children)| {
                children.into_iter().find_map(|c| {
                    (*world.get::<SiphonLifePart>(c)? == SiphonLifePart::Sprite).then(|| {
                        let x = world
                            .get::<GlobalTransform>(c)
                            .unwrap()
                            .compute_transform()
                            .rotation
                            * Vec3::X;
                        (e, x.dot(up).atan2(x.dot(right)))
                    })
                })
            })
            .collect()
    };
    let before = rolls(&mut h);
    h.tick(6);
    let after = rolls(&mut h);
    let mut compared = 0;
    for (e, roll) in &after {
        let Some((_, was)) = before.iter().find(|(b, _)| b == e) else {
            continue;
        };
        let d = (roll - was + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        assert!(
            d.abs() < 1e-4,
            "a flare's roll moved by {d} rad in six frames"
        );
        compared += 1;
    }
    assert!(compared > 30, "only {compared} flares compared");
}

/// Each flare is rolled at random in the camera plane, as the bench draws
/// every star after `g.rotate(ang)`: its in-plane angles (modulo the star's
/// quarter-turn symmetry) spread over the whole quarter turn.
#[test]
fn the_flares_are_rolled_at_random_in_the_camera_plane() {
    for camera in [
        None,
        Some(match_camera(
            Vec3::new(2.0, COMBATANT_Y, 1.0),
            60.0,
            38.7f32.to_radians(),
            1.1,
        )),
    ] {
        let mut h = Harness::new();
        if let Some(cam) = camera {
            h.camera(cam);
        }
        h.victim(Vec3::new(2.0, COMBATANT_Y, 1.0), &[AURA]);
        h.tick_secs(2.0);
        let (right, up) = match camera {
            Some(c) => (c.rotation * Vec3::X, c.rotation * Vec3::Y),
            None => (Vec3::X, Vec3::Y),
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        let angles: Vec<f32> = h
            .quads_with_rotation()
            .into_iter()
            .filter(|(e, part, _)| *e == FLARES && *part == SiphonLifePart::Sprite)
            .map(|(_, _, rotation)| {
                let x = rotation * Vec3::X;
                x.dot(up).atan2(x.dot(right)).rem_euclid(quarter)
            })
            .collect();
        assert!(angles.len() > 30, "only {} flares", angles.len());
        let mut bins = [0usize; 8];
        for a in &angles {
            bins[((a / quarter * 8.0) as usize).min(7)] += 1;
        }
        assert!(
            bins.iter().all(|&n| n > 0),
            "the flares' rolls must fill the quarter turn: {bins:?}"
        );
        let most = *bins.iter().max().unwrap();
        assert!(
            most * 3 < angles.len(),
            "no roll dominates: {bins:?} of {}",
            angles.len()
        );
    }
}

/// At the match camera a streak is the bench's screen line: in the camera's
/// plane at its particle's depth, pointing where the flight's tail projects.
#[test]
fn at_the_camera_a_streak_lies_in_the_screen_plane_behind_its_flight() {
    let mut h = Harness::new();
    let victim = Vec3::new(-1.0, COMBATANT_Y, 4.0);
    let cam = match_camera(victim, 60.0, 38.7f32.to_radians(), 0.7);
    h.camera(cam);
    h.victim(victim, &[AURA]);
    h.tick_secs(1.0);
    let forward = cam.forward().as_vec3();
    let world = h.app.world_mut();
    let rows: Vec<(Vec3, Vec3, Vec<Entity>)> = world
        .query::<(&SiphonLifeParticle, &GlobalTransform, &Children)>()
        .iter(world)
        .filter(|(p, _, _)| p.emitter == STREAKS)
        .map(|(p, g, c)| (p.velocity, g.translation(), c.iter().collect()))
        .collect();
    let mut checked = 0;
    for (velocity, at, children) in rows {
        for child in children {
            if *world.get::<SiphonLifePart>(child).unwrap() != SiphonLifePart::Streak {
                continue;
            }
            let g = world
                .get::<GlobalTransform>(child)
                .unwrap()
                .compute_transform();
            let offset = g.translation - at;
            assert!(
                offset.dot(forward).abs() < 1e-4,
                "the streak left its depth"
            );
            let back = -velocity.normalize();
            let flat = (back - forward * back.dot(forward)).normalize();
            if offset.length() > 1e-3 {
                assert!(
                    offset.normalize().dot(flat) > 0.9999,
                    "the tail points off the flight"
                );
                // The quad's long axis is the tail's screen direction.
                assert!((g.rotation * Vec3::X).dot(flat) > 0.9999);
            }
            checked += 1;
        }
    }
    assert!(checked >= 8, "only {checked} streaks checked");
}

/// A pet's smaller body (`Capsule3d::new(0.35, 0.6)`, hung 0.45 below its
/// sim transform and laid along its facing by `apply_pet_mesh_tilt`) holds
/// its glow in front of it the same way — steep, and looking straight down
/// the body's length, where its near end is nearest.
#[test]
fn a_pets_glow_stands_in_front_of_the_pets_body() {
    for (pitch, yaw) in [(0.3, 0.4), (1.5, 0.0)] {
        a_pets_glow_stands_in_front_of_the_pets_body_from(pitch, yaw);
    }
}

fn a_pets_glow_stands_in_front_of_the_pets_body_from(pitch: f32, yaw: f32) {
    let mut h = Harness::new();
    let at = Vec3::new(-3.0, PET_SIM_Y, 1.0);
    let cam = match_camera(at, 20.0, pitch, yaw);
    h.camera(cam);
    let owner = h.app.world_mut().spawn_empty().id();
    h.app.world_mut().spawn((
        Combatant::new(2, 1, CharacterClass::Hunter),
        Pet {
            owner,
            pet_type: PetType::Felhunter,
        },
        Transform::from_translation(at),
        ActiveAuras {
            auras: vec![Harness::dot(AURA)],
        },
    ));
    h.tick_secs(1.0);
    let eye = cam.translation;
    let forward = cam.forward().as_vec3();
    // The pet faces +Z (an unrotated transform); its capsule lies along it.
    let centre = at + Vec3::Y * PET_BODY_Y;
    let nearest = [centre - Vec3::Z * 0.3, centre + Vec3::Z * 0.3]
        .iter()
        .map(|e| (*e - eye).dot(forward))
        .fold(f32::MAX, f32::min)
        - 0.35;
    let world = h.app.world_mut();
    let depths: Vec<f32> = world
        .query::<(&SiphonLifePart, &GlobalTransform)>()
        .iter(world)
        .map(|(_, g)| (g.translation() - eye).dot(forward))
        .collect();
    assert!(depths.len() > 40, "only {} quads", depths.len());
    // Held against the body's bounding sphere, so at most the capsule's
    // half-segment (0.3) nearer than it must be.
    let held = depths
        .iter()
        .filter(|d| **d > nearest - HELD_WITHIN - 0.3)
        .count();
    assert!(held > 20, "only {held} quads held just in front of the pet");
    for depth in depths {
        assert!(
            depth < nearest,
            "a quad at {depth} behind the pet's {nearest}"
        );
    }
}

#[test]
fn without_a_camera_every_particle_is_drawn_where_it_truly_is() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    h.victim(at, &[AURA]);
    h.tick_secs(0.5);
    let world = h.app.world_mut();
    let rows: Vec<(Vec3, Vec3)> = world
        .query::<(&SiphonLifeParticle, &GlobalTransform)>()
        .iter(world)
        .map(|(p, g)| (p.offset, g.translation()))
        .collect();
    assert_nonempty("particles", &rows);
    for (offset, drawn) in rows {
        assert!(drawn.distance(chest(at) + offset) < 1e-5);
    }
}

// ── Following ────────────────────────────────────────────────────────────────

#[test]
fn the_glow_follows_a_running_victim() {
    let mut h = Harness::new();
    let start = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let victim = h.victim(start, &[AURA]);
    h.tick_secs(0.5);
    let before: Vec<Seen> = h.particles();
    let moved = Vec3::new(6.0, COMBATANT_Y, 2.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(victim)
        .unwrap()
        .translation = moved;
    h.tick(1);
    assert!(h.rigs()[0].2.distance(chest(moved)) < 1e-5);
    let after = h.particles();
    let mut carried = 0;
    for b in &before {
        let Some(a) = after.iter().find(|a| a.entity == b.entity) else {
            continue;
        };
        // Its own flight for one frame, plus the victim's whole move.
        let expected = b.at + b.velocity * TICK_SECS + (moved - start);
        assert!(
            a.at.distance(expected) < 1e-3,
            "a particle was left behind: {} off",
            a.at.distance(expected)
        );
        carried += 1;
    }
    assert!(carried > 40, "only {carried} particles carried");
}

#[test]
fn a_pets_glow_sits_at_its_chest_at_its_stature() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, PET_SIM_Y, 2.0);
    let owner = h.app.world_mut().spawn_empty().id();
    h.app.world_mut().spawn((
        Combatant::new(2, 1, CharacterClass::Hunter),
        Pet {
            owner,
            pet_type: PetType::Felhunter,
        },
        Transform::from_translation(at),
        ActiveAuras {
            auras: vec![Harness::dot(AURA)],
        },
    ));
    h.tick_secs(0.5);
    let pet_chest = at + Vec3::Y * (PET_BODY_Y + CHEST_ABOVE_CENTRE * PET_STATURE);
    assert!(h.rigs()[0].2.distance(pet_chest) < 1e-5);
    let rings: Vec<Seen> = h
        .particles()
        .into_iter()
        .filter(|p| p.emitter == RINGS)
        .collect();
    assert_nonempty("rings", &rings);
    for p in &rings {
        assert!((p.velocity.length() - BENCH[RINGS].speed * PET_STATURE).abs() < 1e-4);
    }
    // The streaks' shell and speed shrink with the pet too.
    let mut births: Vec<f32> = Vec::new();
    for _ in 0..120 {
        h.tick(1);
        for p in h.particles().into_iter().filter(|p| p.emitter == STREAKS) {
            assert!((p.velocity.length() - BENCH[STREAKS].speed * PET_STATURE).abs() < 1e-4);
            births.push((p.at - p.velocity * p.age - pet_chest).length());
        }
    }
    assert!(births.len() > 40, "only {} streaks", births.len());
    for r in births {
        assert!(
            (0.6 * SHELL * PET_STATURE - 1e-3..=SHELL * PET_STATURE + 1e-3).contains(&r),
            "a pet's streak born {r} yd out"
        );
    }
}

// ── Lifecycle, beside Corruption ─────────────────────────────────────────────

fn descends_from(world: &World, mut entity: Entity, ancestor: Entity) -> bool {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if entity == ancestor {
            return true;
        }
    }
    false
}

#[test]
fn siphon_life_lands_corruptions_apply_and_a_refresh_replays_nothing() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA]);
    h.tick(2);
    let corruption_bursts = h.entities::<DotApplyBurst>();
    assert_eq!(corruption_bursts.len(), 1, "Corruption lands its own apply");

    h.add(victim, AURA);
    h.tick(2);
    let bursts = h.entities::<DotApplyBurst>();
    assert_eq!(bursts.len(), 2, "Siphon Life lands kit 117 too");
    let rig = h.rigs()[0].0;

    // A refresh: the same aura back to full duration, in place.
    h.tick_secs(0.5);
    for a in h.auras(victim).auras.iter_mut() {
        if a.ability_name == AURA {
            a.duration = 30.0;
        }
    }
    h.tick(3);
    assert_eq!(h.rigs().len(), 1);
    assert_eq!(h.rigs()[0].0, rig, "a refresh keeps the rig");
    assert_eq!(
        h.built(),
        Built { rigs: 1, bursts: 2 },
        "and builds nothing"
    );
    assert!(
        h.entities::<DotApplyBurst>().is_subset(&bursts),
        "a refresh replays no apply"
    );
}

#[test]
fn removing_siphon_life_ends_its_glow_and_leaves_corruption_standing() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA, AURA]);
    h.tick_secs(1.0);
    assert_eq!(h.rigs().len(), 1);
    assert_eq!(h.count::<CorruptionShroudRig>(), 1);
    assert!(h.count::<SiphonLifeParticle>() > 40);

    // A dispel (or the DoT running out: the same presence-keyed path).
    h.remove(victim, AURA);
    h.tick(1);
    assert!(h.rigs().is_empty(), "the glow ends with its DoT");
    assert_eq!(h.count::<SiphonLifeParticle>(), 0, "nothing lingers");
    assert_eq!(h.count::<SiphonLifePart>(), 0);
    assert_eq!(h.count::<CorruptionShroudRig>(), 1, "Corruption stands");

    // A fresh Siphon Life is a fresh glow, and Corruption's going leaves it.
    h.add(victim, AURA);
    h.tick(2);
    assert_eq!(h.rigs().len(), 1);
    h.remove(victim, CORRUPTION_AURA);
    h.tick(2);
    assert_eq!(h.count::<CorruptionShroudRig>(), 0);
    assert_eq!(h.rigs().len(), 1, "Siphon Life outlives Corruption");
}

#[test]
fn the_glow_ends_when_the_last_aura_expires_and_the_component_goes() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[AURA]);
    h.tick_secs(0.5);
    assert_eq!(h.rigs().len(), 1);
    // `update_auras` removes `ActiveAuras` outright with the last aura.
    h.app.world_mut().entity_mut(victim).remove::<ActiveAuras>();
    h.tick(1);
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<SiphonLifeParticle>(), 0);
}

#[test]
fn death_takes_the_glow_and_the_corpse_wears_nothing() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA, AURA]);
    h.tick_secs(0.3);
    assert_eq!(h.rigs().len(), 1);
    h.app
        .world_mut()
        .get_mut::<Combatant>(victim)
        .unwrap()
        .current_health = 0.0;
    h.tick(1);
    assert!(h.rigs().is_empty(), "nothing on a corpse");
    assert_eq!(h.count::<SiphonLifeParticle>(), 0);
    assert_eq!(h.count::<DotApplyBurst>(), 0, "nor its apply");
    // Auras linger on corpses: the dead victim must never be re-rigged, nor
    // its apply replayed, not even for the frame before a cleanup.
    let built = h.built();
    assert_eq!(built, Built { rigs: 1, bursts: 2 });
    h.tick(5);
    assert!(h.rigs().is_empty());
    assert_eq!(h.built(), built, "a corpse builds nothing");
}

#[test]
fn a_despawned_victim_takes_its_glow() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[AURA]);
    h.tick_secs(0.3);
    h.app.world_mut().despawn(victim);
    h.tick(1);
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<SiphonLifeParticle>(), 0);
}

#[test]
fn a_sheep_wears_nothing_and_its_glow_returns_when_it_lifts() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA, AURA]);
    h.tick_secs(0.3);
    let rig = h.rigs()[0].0;
    let visibility = |h: &mut Harness| *h.app.world().get::<Visibility>(rig).unwrap();
    assert_eq!(visibility(&mut h), Visibility::Inherited);

    h.app
        .world_mut()
        .entity_mut(victim)
        .insert(PolymorphedVisual);
    h.tick(1);
    assert_eq!(visibility(&mut h), Visibility::Hidden);
    let age = h.app.world().get::<SiphonLifeRig>(rig).unwrap().age;
    h.tick_secs(0.5);
    assert!(
        h.app.world().get::<SiphonLifeRig>(rig).unwrap().age > age + 0.4,
        "its clock runs on"
    );

    h.app
        .world_mut()
        .entity_mut(victim)
        .remove::<PolymorphedVisual>();
    h.tick(1);
    assert_eq!(h.rigs()[0].0, rig, "the same rig");
    assert_eq!(visibility(&mut h), Visibility::Inherited);
}

#[test]
fn the_match_ending_mid_dot_leaves_nothing_behind() {
    let mut h = Harness::new();
    h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA, AURA]);
    h.tick_secs(1.0);
    let rig = h.rigs()[0].0;
    assert!(
        h.app.world().get::<PlayMatchEntity>(rig).is_some(),
        "the rig is a match entity"
    );
    let particles = h.entities::<SiphonLifeParticle>();
    let parts = h.entities::<SiphonLifePart>();
    assert!(particles.len() > 40 && parts.len() > particles.len());
    for e in particles.iter().chain(parts.iter()) {
        assert!(
            descends_from(h.app.world(), *e, rig),
            "every particle and quad hangs off the rig"
        );
    }
    h.app
        .world_mut()
        .run_system_once(cleanup_play_match)
        .unwrap();
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<SiphonLifeParticle>(), 0);
    assert_eq!(h.count::<SiphonLifePart>(), 0);
}
