//! Probes for Shadow Word: Pain's landing (`rendering/effects/shadow_word_pain.rs`,
//! AS-342): the client's `shadowwordpain_chest.m2` hit on the victim's chest,
//! the spell's whole visual.
//!
//! Two halves:
//!
//! - **The sim's marker**, driven through `apply_pending_auras` on a bare
//!   `World`: a Shadow Word: Pain that lands marks one hit on its victim, and a
//!   DoT with no landing (Corruption) marks none.
//! - **The renderer**, on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`
//!   with the chains `states/mod.rs` registers (the Druid's too, which shares
//!   the marker type), asserting WORLD geometry: the violet streaks converge on
//!   the chest for the first 167 ms; at 233 ms the burst leaves the chest
//!   outward; the dark layers darken (alpha blend) and the violet ones add; the
//!   smoke drags to a near stop; the streaks trail back along the flight; the
//!   hit tracks a moving victim; and it retires once its smoke has died.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::components::{
    ArenaDampening, AuraLanding, AuraLandingKind, AuraPending, Combatant, DruidEffectRig, GameRng,
    ShadowPainHit, ShadowPainParticle, ShadowPainParticlePart, SHADOW_WORD_PAIN_AURA,
};
use arenasim::states::play_match::{
    animate_druid_effects, animate_druid_particles, animate_shadow_pain_hits,
    animate_shadow_pain_particles, billboard_druid_particles, billboard_shadow_pain_particles,
    cleanup_druid_effects, spawn_druid_effects, spawn_shadow_pain_hits, swp_one_shot_life,
    swp_streak_length, AbilityDefinitions, AbilityType, BODY_CENTRE_ABOVE_FLOOR,
    SWP_CHEST_ABOVE_FLOOR, SWP_EMITTERS,
};
use arenasim::CharacterClass;

// ── the sim's marker ─────────────────────────────────────────────────────────

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(Time::<()>::default());
    world
}

fn landings(world: &mut World) -> Vec<(Entity, AuraLandingKind)> {
    let mut q = world.query::<&AuraLanding>();
    q.iter(world).map(|l| (l.target, l.kind)).collect()
}

fn land(world: &mut World, ability: AbilityType, target: Entity, caster: Entity) {
    let defs = AbilityDefinitions::default();
    let pending = AuraPending::from_ability(target, caster, defs.get_unchecked(&ability))
        .expect("the ability applies an aura");
    world.spawn(pending);
    world.run_system_once(apply_pending_auras).unwrap();
}

/// The router's name is the RON name: a rename in `abilities.ron` must not
/// silently drop Shadow Word: Pain off its landing.
#[test]
fn the_routed_name_is_the_config_name() {
    let defs = AbilityDefinitions::default();
    assert_eq!(
        defs.get_unchecked(&AbilityType::ShadowWordPain).name,
        SHADOW_WORD_PAIN_AURA
    );
}

#[test]
fn shadow_word_pain_marks_its_hit_and_corruption_marks_none() {
    let mut world = world();
    let priest = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Priest),
            Transform::default(),
        ))
        .id();
    let victim = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::default(),
        ))
        .id();
    land(&mut world, AbilityType::Corruption, victim, priest);
    assert!(
        landings(&mut world).is_empty(),
        "a DoT with no landing marks nothing"
    );
    land(&mut world, AbilityType::ShadowWordPain, victim, priest);
    assert_eq!(
        landings(&mut world),
        vec![(victim, AuraLandingKind::ShadowWordPainHit)]
    );
}

// ── the renderer ─────────────────────────────────────────────────────────────

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;

/// Emitter indices in `SWP_EMITTERS`.
const INWARD: usize = 0;
const SMOKE: usize = 1;
const BLOBS: usize = 2;
const VIOLET: usize = 3;
const DARK: usize = 4;
const STARS: usize = 5;
const LATE: usize = 6;

struct Harness {
    app: App,
}

/// One live particle, read back from the world.
struct Seen {
    emitter: usize,
    at: Vec3,
    velocity: Vec3,
    age: f32,
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
        // The chains `states/mod.rs` registers. The Druid's consumes the same
        // marker type, so it runs here too.
        app.add_systems(
            Update,
            (
                spawn_druid_effects,
                animate_druid_effects,
                animate_druid_particles,
                billboard_druid_particles,
                cleanup_druid_effects,
            )
                .chain(),
        );
        app.add_systems(
            Update,
            (
                spawn_shadow_pain_hits,
                animate_shadow_pain_hits,
                animate_shadow_pain_particles,
                billboard_shadow_pain_particles,
            )
                .chain(),
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

    /// Run until `secs` after the hit (frames are counted from the marker).
    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / TICK_SECS).ceil() as u32);
    }

    fn victim(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(2, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
            ))
            .id()
    }

    fn hit(&mut self, target: Entity) {
        self.app.world_mut().spawn(AuraLanding {
            target,
            kind: AuraLandingKind::ShadowWordPainHit,
        });
    }

    fn rigs(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&ShadowPainHit>().iter(world).count()
    }

    fn particles(&mut self) -> Vec<Seen> {
        let world = self.app.world_mut();
        let mut q = world.query::<(&ShadowPainParticle, &GlobalTransform)>();
        q.iter(world)
            .map(|(p, g)| Seen {
                emitter: p.emitter,
                at: g.translation(),
                velocity: p.velocity,
                age: p.age,
            })
            .collect()
    }

    fn emitters_live(&mut self) -> BTreeSet<usize> {
        self.particles().iter().map(|p| p.emitter).collect()
    }
}

/// The chest the bench anchors the model on, for a victim at `at`.
fn chest(at: Vec3) -> Vec3 {
    at + Vec3::Y * (SWP_CHEST_ABOVE_FLOOR - BODY_CENTRE_ABOVE_FLOOR)
}

#[test]
fn the_violet_streaks_converge_on_the_chest_before_the_burst() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, COMBATANT_Y, -2.0);
    let victim = h.victim(at);
    h.hit(victim);
    // Early enough that no streak has reached the chest yet: born at least
    // 0.54 yd out, at 4.58 yd/s none arrives before 0.118 s (the bench's
    // streaks then fly on through the body, fading).
    h.tick_secs(0.08);

    let seen = h.particles();
    assert!(
        seen.len() >= 6,
        "104/s for 0.08 s should be ~8 streaks, saw {}",
        seen.len()
    );
    assert_eq!(
        h.emitters_live(),
        BTreeSet::from([INWARD]),
        "only the inward streaks play before the burst"
    );
    let centre = chest(at);
    for p in &seen {
        let to_chest = centre - p.at;
        // Born on a shell at most 0.9 yd out, and travelling straight in.
        assert!(
            to_chest.length() <= 0.9 + 1e-3,
            "a streak {} yd from the chest",
            to_chest.length()
        );
        let along = to_chest.normalize().dot(p.velocity.normalize());
        assert!(
            along > 0.999,
            "a streak travels off the chest line ({along})"
        );
        assert!((p.velocity.length() - 4.58).abs() < 1e-3);
    }
}

#[test]
fn the_burst_leaves_the_chest_at_233_ms_and_the_late_sparks_follow() {
    let mut h = Harness::new();
    let at = Vec3::new(-4.0, COMBATANT_Y, 6.0);
    let victim = h.victim(at);
    h.hit(victim);
    h.tick_secs(0.20);
    assert!(
        h.particles().iter().all(|p| p.emitter == INWARD),
        "nothing bursts before 233 ms"
    );

    h.tick_secs(0.10);
    assert_eq!(
        h.emitters_live(),
        BTreeSet::from([INWARD, SMOKE, BLOBS, VIOLET, DARK, STARS]),
        "at 0.3 s the burst is out and the last inward streaks still live"
    );
    let centre = chest(at);
    for p in h.particles().iter().filter(|p| p.emitter != INWARD) {
        let out = p.at - centre;
        let e = &SWP_EMITTERS[p.emitter];
        // Born AT the chest and flying straight out: never further than the
        // launch speed carries it in its age.
        assert!(
            out.length() <= e.speed * p.age + 1e-3,
            "{} is {} yd out at age {}",
            e.name,
            out.length(),
            p.age
        );
        if out.length() > 1e-3 && p.velocity.length() > 1e-3 {
            let along = out.normalize().dot(p.velocity.normalize());
            assert!(along > 0.999, "{} flies off its ray ({along})", e.name);
        }
    }

    h.tick_secs(0.50);
    assert!(
        h.emitters_live().contains(&LATE),
        "the late dark sparks play from 700 ms"
    );
}

#[test]
fn the_dark_layers_darken_and_the_violet_layers_add() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.hit(victim);
    h.tick_secs(0.75);

    let world = h.app.world_mut();
    let mut q = world.query::<(&ShadowPainParticle, &Children)>();
    let mut parts = world.query::<(&ShadowPainParticlePart, &MeshMaterial3d<StandardMaterial>)>();
    let mut modes: Vec<(usize, AlphaMode)> = Vec::new();
    let pairs: Vec<(usize, Vec<Entity>)> = q
        .iter(world)
        .map(|(p, c)| (p.emitter, c.iter().collect()))
        .collect();
    for (emitter, children) in pairs {
        for child in children {
            let (_, material) = parts.get(world, child).unwrap();
            let handle = material.0.clone();
            let mode = world
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .unwrap()
                .alpha_mode;
            modes.push((emitter, mode));
        }
    }
    let darkening: BTreeSet<usize> = modes
        .iter()
        .filter(|(_, m)| *m == AlphaMode::Blend)
        .map(|(e, _)| *e)
        .collect();
    let adding: BTreeSet<usize> = modes
        .iter()
        .filter(|(_, m)| *m == AlphaMode::Add)
        .map(|(e, _)| *e)
        .collect();
    assert_eq!(darkening, BTreeSet::from([SMOKE, BLOBS, DARK, LATE]));
    assert_eq!(adding, BTreeSet::from([VIOLET, STARS]));
    assert_eq!(modes.len(), {
        let world = h.app.world_mut();
        world.query::<&ShadowPainParticlePart>().iter(world).count()
    });
}

#[test]
fn the_smoke_drags_to_a_near_stop_at_the_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(1.0, COMBATANT_Y, 1.0);
    let victim = h.victim(at);
    h.hit(victim);
    h.tick_secs(1.6);

    let smoke: Vec<Seen> = h
        .particles()
        .into_iter()
        .filter(|p| p.emitter == SMOKE && p.age >= 0.6)
        .collect();
    assert!(smoke.len() >= 10, "the probe went vacuous: {}", smoke.len());
    let centre = chest(at);
    for p in &smoke {
        // Undragged, 1.1 yd/s for 0.6 s would carry it 0.66 yd; drag 2 stops
        // it within about 0.2 yd.
        let out = p.at.distance(centre);
        assert!(out < 0.3, "smoke drifted {out} yd at age {}", p.age);
    }
}

#[test]
fn a_streak_trails_back_along_its_flight() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(2.0, COMBATANT_Y, -1.0));
    h.hit(victim);
    h.tick_secs(0.33);

    let world = h.app.world_mut();
    let mut q = world.query::<(&ShadowPainParticle, &GlobalTransform, &Children)>();
    let mut parts = world.query::<(&ShadowPainParticlePart, &GlobalTransform)>();
    let particles: Vec<(usize, Vec3, Vec3, Vec<Entity>)> = q
        .iter(world)
        .map(|(p, g, c)| (p.emitter, g.translation(), p.velocity, c.iter().collect()))
        .collect();
    let mut checked = 0;
    for (emitter, at, velocity, children) in particles {
        let e = &SWP_EMITTERS[emitter];
        let Some(length) = swp_streak_length(e, velocity.length(), 1.0) else {
            continue;
        };
        for child in children {
            let (part, g) = parts.get(world, child).unwrap();
            if *part != ShadowPainParticlePart::Streak {
                continue;
            }
            let offset = g.translation() - at;
            assert!(
                (offset.length() - length * 0.5).abs() < 1e-3,
                "{}: the streak's middle sits {} back, not {}",
                e.name,
                offset.length(),
                length * 0.5
            );
            assert!(
                offset.normalize().dot(velocity.normalize()) < -0.999,
                "{}: the streak must trail BEHIND the flight",
                e.name
            );
            checked += 1;
        }
    }
    assert!(checked >= 10, "the probe went vacuous: {checked} streaks");
}

#[test]
fn the_hit_tracks_a_moving_victim() {
    let mut h = Harness::new();
    let start = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let victim = h.victim(start);
    h.hit(victim);
    h.tick_secs(0.2);
    let moved = Vec3::new(6.0, COMBATANT_Y, 2.0);
    h.app
        .world_mut()
        .entity_mut(victim)
        .get_mut::<Transform>()
        .unwrap()
        .translation = moved;
    h.tick_secs(0.1);

    let burst: Vec<Seen> = h
        .particles()
        .into_iter()
        .filter(|p| p.emitter != INWARD)
        .collect();
    assert!(!burst.is_empty(), "the probe went vacuous");
    for p in &burst {
        assert!(
            p.at.distance(chest(moved)) < 1.0,
            "the burst plays on the victim where it is, not where it was hit"
        );
    }
}

#[test]
fn the_hit_retires_once_its_smoke_has_died() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.hit(victim);
    h.tick_secs(2.0);
    assert_eq!(h.rigs(), 1);
    assert!(
        h.particles().iter().any(|p| p.emitter == SMOKE),
        "the smoke still trails at 2.0 s"
    );
    h.tick_secs(swp_one_shot_life() - 2.0 + 0.1);
    assert_eq!(h.rigs(), 0, "the hit retires");
    assert!(h.particles().is_empty(), "and leaves no particle behind");
}

/// The two renderers share the `AuraLanding` marker; each plays only its own.
#[test]
fn the_druid_and_the_priest_landings_stay_apart() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.hit(victim);
    h.tick(2);
    assert_eq!(h.rigs(), 1);
    let druid_rigs = {
        let world = h.app.world_mut();
        world.query::<&DruidEffectRig>().iter(world).count()
    };
    assert_eq!(druid_rigs, 0, "Shadow Word: Pain plays no Druid effect");

    let ally = h.victim(Vec3::new(5.0, COMBATANT_Y, 0.0));
    h.app.world_mut().spawn(AuraLanding {
        target: ally,
        kind: AuraLandingKind::RejuvenationSwirl,
    });
    h.tick(2);
    let world = h.app.world_mut();
    let targets: Vec<Entity> = world
        .query::<&ShadowPainHit>()
        .iter(world)
        .map(|r| r.target)
        .collect();
    assert_eq!(targets, vec![victim], "Rejuvenation plays no shadow hit");
    assert_eq!(world.query::<&DruidEffectRig>().iter(world).count(), 1);
    assert!(
        world.query::<&AuraLanding>().iter(world).next().is_none(),
        "every marker is consumed by its own renderer"
    );
}
