//! Probes for the weapon poison landing (`rendering/effects/poison_landing.rs`):
//! the green cloud `poison_impact_chest.m2` lands on a victim's chest each time
//! any of the four Rogue poisons is applied, and a new application RESTARTS the
//! cloud already playing rather than stacking a second one.
//!
//! Everything is asserted in WORLD space off the propagated `GlobalTransform`s
//! — where the flares are, where the puffs are, whether they ride the victim —
//! and the restart by what is newly born, never by reading the rig's clock.
//!
//! The harness plays the production systems in their production order, one
//! frame per sim tick: a stand-in for `update_auras` (every aura runs down by
//! the tick), then `detect_poison_landings` (FixedUpdate after the sim's last
//! phase in the client), then the Update chain's `update_poison_clouds`,
//! `animate_client_particles` and `billboard_poison_clouds`. An application is
//! written the way the sim writes one: the poison's aura at its full duration.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU.

use std::collections::HashMap;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, ClientParticle, Combatant, PoisonCloud,
};
use arenasim::states::play_match::{
    animate_client_particles, billboard_poison_clouds, detect_poison_landings, lands_poison_cloud,
    poison_landing_emitters, poison_landing_secs, update_poison_clouds, IMPACT_CHEST_Y,
    WEAPON_POISON_AURAS,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(20);
const DT: f32 = 0.02;

/// The burst's launch speed, yd/s, and its spawn cube's side.
const BURST_SPEED: f32 = 3.33;
const BURST_AREA: f32 = 0.28;
/// The cloud's launch speed (±50%) and spawn cube's side.
const CLOUD_SPEED: f32 = 0.28;
const CLOUD_AREA: f32 = 0.56;

/// Whether the stand-in sim runs auras down this frame.
#[derive(Resource)]
struct SimRunning(bool);

/// `update_auras`' duration tick, alone.
fn run_auras_down(running: Res<SimRunning>, time: Res<Time>, mut q: Query<&mut ActiveAuras>) {
    if !running.0 {
        return;
    }
    for mut auras in q.iter_mut() {
        for aura in auras.auras.iter_mut() {
            aura.duration -= time.delta_secs();
        }
        auras.auras.retain(|a| a.duration > 0.0);
    }
}

/// What kind of particle a `ClientParticle` is, by the emitter that made it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Burst,
    Puff,
    Mote,
}

#[derive(Clone, Copy, Debug)]
struct Seen {
    kind: Kind,
    age: f32,
    at: Vec3,
}

/// The victim aura `ability` lands, at its full duration, as the RON defines it.
fn landed(ability: AbilityType) -> Aura {
    let defs = AbilityDefinitions::default();
    let def = defs.get(&ability).unwrap();
    let effect = def.applies_aura.as_ref().unwrap();
    Aura {
        effect_type: effect.aura_type,
        duration: effect.duration,
        ability_name: def.name.clone(),
        ..Default::default()
    }
}

/// The play camera's pose, near enough: up and back, looking at the arena.
fn camera() -> Transform {
    Transform::from_xyz(0.0, 20.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y)
}

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
        app.insert_resource(SimRunning(true));
        app.add_systems(
            Update,
            (
                run_auras_down,
                detect_poison_landings,
                update_poison_clouds,
                animate_client_particles,
                billboard_poison_clouds,
            )
                .chain(),
        );
        app.world_mut().spawn((Camera3d::default(), camera()));
        // Prime `Time` so the first measured frame has a real delta.
        app.update();
        Harness { app }
    }

    fn spawn_unit(&mut self, at: Vec3, auras: Vec<Aura>) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                Combatant::new(2, 0, CharacterClass::Warrior),
                ActiveAuras { auras },
            ))
            .id()
    }

    /// Land `aura` on `e` the way the sim does: a fresh one is pushed, a
    /// present one (same name and caster) goes back to its full duration.
    fn apply(&mut self, e: Entity, aura: Aura) {
        let mut auras = self.app.world_mut().get_mut::<ActiveAuras>(e).unwrap();
        match auras
            .auras
            .iter_mut()
            .find(|a| a.ability_name == aura.ability_name && a.caster == aura.caster)
        {
            Some(existing) => existing.duration = aura.duration,
            None => auras.auras.push(aura),
        }
    }

    fn run(&mut self, secs: f32) {
        for _ in 0..(secs / DT).round() as usize {
            self.app.update();
        }
    }

    fn pos(&self, e: Entity) -> Vec3 {
        self.app.world().get::<Transform>(e).unwrap().translation
    }

    fn chest(&self, e: Entity) -> Vec3 {
        self.pos(e) + Vec3::Y * IMPACT_CHEST_Y
    }

    fn rigs(&mut self) -> Vec<(PoisonCloud, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&PoisonCloud, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(c, t)| (*c, t.translation()))
            .collect()
    }

    /// Every LIVE cloud particle, by entity, in world space.
    fn particles(&mut self) -> HashMap<Entity, Seen> {
        let burst_life = poison_landing_emitters()[0].life;
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &ClientParticle, &GlobalTransform)>();
        q.iter(self.app.world())
            .filter(|(_, p, _)| p.age < p.life)
            .map(|(e, p, t)| {
                let kind = if (p.life - burst_life).abs() < 1e-4 {
                    Kind::Burst
                } else if p.size[0] == p.size[2] {
                    Kind::Mote
                } else {
                    Kind::Puff
                };
                (
                    e,
                    Seen {
                        kind,
                        age: p.age,
                        at: t.translation(),
                    },
                )
            })
            .collect()
    }

    /// Every particle entity, live or spent.
    fn particle_entities(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&ClientParticle>();
        q.iter(self.app.world()).count()
    }

    fn of(&mut self, kind: Kind) -> Vec<Seen> {
        self.particles()
            .into_values()
            .filter(|s| s.kind == kind)
            .collect()
    }
}

// =============================================================================
// Routing
// =============================================================================

/// The table names each poison's victim aura exactly as `abilities.ron` lands
/// it — so a renamed or retyped poison aura fails here, not silently in play.
#[test]
fn the_table_is_the_four_poisons_as_the_config_lands_them() {
    let poisons: Vec<AbilityType> = WEAPON_POISON_AURAS.iter().map(|r| r.0).collect();
    assert_eq!(
        poisons,
        vec![
            AbilityType::CripplingPoison,
            AbilityType::DeadlyPoison,
            AbilityType::WoundPoison,
            AbilityType::MindNumbingPoison,
        ]
    );
    for (ability, aura_type, name) in WEAPON_POISON_AURAS {
        let aura = landed(ability);
        assert_eq!(
            (aura.effect_type, aura.ability_name.as_str()),
            (aura_type, name)
        );
        assert!(lands_poison_cloud(ability));
    }
    assert!(!lands_poison_cloud(AbilityType::Frostbolt));
}

// =============================================================================
// The landing
// =============================================================================

#[test]
fn every_poison_lands_the_cloud_on_its_victims_chest() {
    for (ability, _, _) in WEAPON_POISON_AURAS {
        let mut h = Harness::new();
        let victim = h.spawn_unit(Vec3::new(3.0, 1.0, -2.0), vec![]);
        h.run(0.1);
        h.apply(victim, landed(ability));
        h.run(0.3);

        let rigs = h.rigs();
        assert_eq!(rigs.len(), 1, "{ability:?}: one cloud");
        assert_eq!(rigs[0].0.victim, victim);
        let chest = h.chest(victim);
        assert!(
            rigs[0].1.distance(chest) < 1e-4,
            "{ability:?}: rig at the chest"
        );

        // The burst: about 25 flares, each flying out from the chest at
        // 3.33 yd/s from a 0.28 yd cube about it.
        let burst = h.of(Kind::Burst);
        assert!(
            (24..=26).contains(&burst.len()),
            "{ability:?}: {} flares",
            burst.len()
        );
        let slack = BURST_AREA * 0.87 + 0.05;
        for f in &burst {
            let d = f.at.distance(chest);
            assert!(
                (d - BURST_SPEED * f.age).abs() < slack,
                "{ability:?}: a flare {d:.2} yd out at age {:.2}",
                f.age
            );
        }
        // At least one flare has already flown clear of the body (0.5 yd).
        assert!(burst.iter().any(|f| f.at.distance(chest) > 0.8));

        // The cloud: puffs and motes drifting slowly about the chest.
        for kind in [Kind::Puff, Kind::Mote] {
            let cloud = h.of(kind);
            assert!(cloud.len() >= 4, "{ability:?}: {kind:?} x{}", cloud.len());
            for p in &cloud {
                let reach = CLOUD_AREA * 0.87 + 1.5 * CLOUD_SPEED * p.age + 0.01;
                assert!(
                    p.at.distance(chest) < reach,
                    "{ability:?}: a {kind:?} {:.2} yd from the chest",
                    p.at.distance(chest)
                );
            }
        }
    }
}

/// The flares all fly at 3.33 yd/s; the cloud drifts at 0.28 yd/s ±50%, the
/// spread the client gives it, not one speed.
#[test]
fn the_cloud_drifts_at_the_clients_speeds() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::ZERO, vec![]);
    h.apply(victim, landed(AbilityType::DeadlyPoison));
    h.run(0.3);
    let burst_life = poison_landing_emitters()[0].life;
    let mut q = h.app.world_mut().query::<&ClientParticle>();
    let (mut burst, mut cloud) = (Vec::new(), Vec::new());
    for p in q.iter(h.app.world()) {
        let speed = p.velocity.length();
        if (p.life - burst_life).abs() < 1e-4 {
            burst.push(speed);
        } else {
            cloud.push(speed);
        }
    }
    assert!(burst.len() >= 24 && cloud.len() >= 8, "guard");
    assert!(
        burst.iter().all(|s| (s - BURST_SPEED).abs() < 1e-3),
        "{burst:?}"
    );
    let lo = CLOUD_SPEED * 0.5 - 1e-3;
    let hi = CLOUD_SPEED * 1.5 + 1e-3;
    assert!(cloud.iter().all(|s| (lo..hi).contains(s)), "{cloud:?}");
    let min = cloud.iter().copied().fold(f32::MAX, f32::min);
    let max = cloud.iter().copied().fold(0.0, f32::max);
    assert!(
        min < CLOUD_SPEED * 0.8 && max > CLOUD_SPEED * 1.2,
        "the cloud drifts at one speed: {min:.3}..{max:.3}"
    );
}

/// Every puff, mote and flare is a camera-facing sprite: its world rotation is
/// the camera's.
#[test]
fn the_cloud_faces_the_camera() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::new(2.0, 1.0, 1.0), vec![]);
    h.apply(victim, landed(AbilityType::DeadlyPoison));
    h.run(0.3);
    let cam = camera().rotation;
    let mut q = h
        .app
        .world_mut()
        .query::<(&ClientParticle, &GlobalTransform)>();
    let mut n = 0;
    for (_, t) in q.iter(h.app.world()) {
        let (_, rotation, _) = t.to_scale_rotation_translation();
        assert!(rotation.angle_between(cam) < 1e-3, "{rotation:?}");
        n += 1;
    }
    assert!(n >= 30, "guard: {n} particles");
}

/// A slow that is not a poison, and the Rogue's own weapon coating (a
/// `WeaponPoison` aura that carries the poison's name), land no cloud.
#[test]
fn nothing_but_a_poison_lands_the_cloud() {
    let mut h = Harness::new();
    h.spawn_unit(Vec3::ZERO, vec![]);
    let rogue = h.spawn_unit(Vec3::new(4.0, 1.0, 0.0), vec![]);
    let frosted = h.spawn_unit(Vec3::new(-4.0, 1.0, 0.0), vec![]);
    h.run(0.1);
    h.apply(
        rogue,
        Aura {
            effect_type: AuraType::WeaponPoison,
            duration: 3600.0,
            ability_name: "Deadly Poison".to_string(),
            ..Default::default()
        },
    );
    h.apply(frosted, landed(AbilityType::Frostbolt));
    h.run(0.5);
    assert!(h.rigs().is_empty(), "{:?}", h.rigs());
    assert!(h.particles().is_empty());
}

#[test]
fn the_cloud_rides_its_victim() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![]);
    h.apply(victim, landed(AbilityType::DeadlyPoison));
    h.run(1.0);
    let before = h.particles();
    assert!(before.len() >= 20, "guard: a cloud to move");

    // The victim steps 5 yd east in one tick.
    h.app
        .world_mut()
        .get_mut::<Transform>(victim)
        .unwrap()
        .translation
        .x += 5.0;
    h.run(DT);
    let after = h.particles();
    let shifts: Vec<Vec3> = before
        .iter()
        .filter_map(|(e, b)| after.get(e).map(|a| a.at - b.at))
        .collect();
    assert!(
        shifts.len() >= 20,
        "guard: {} survived the tick",
        shifts.len()
    );
    for s in &shifts {
        assert!(
            (s.x - 5.0).abs() < 0.1 && s.z.abs() < 0.1,
            "a particle moved {s:?}, not with its victim"
        );
    }
    assert!(h.rigs()[0].1.distance(h.chest(victim)) < 1e-4);
}

// =============================================================================
// Every application, and the restart
// =============================================================================

/// One application: the burst plays once, the cloud emits for 3 s, and by
/// 5.5 s everything it made is gone.
#[test]
fn a_single_landing_plays_out_and_ends() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::ZERO, vec![]);
    h.apply(victim, landed(AbilityType::DeadlyPoison));
    h.run(2.1);
    assert!(
        h.of(Kind::Burst).is_empty(),
        "no second burst without a second application"
    );
    h.run(2.4); // t = 4.5
    let youngest = h
        .of(Kind::Puff)
        .iter()
        .map(|p| p.age)
        .fold(f32::MAX, f32::min);
    assert!(
        youngest > 1.4,
        "the cloud stops emitting at 3.0 s, yet a puff is {youngest:.2} s old"
    );
    // The aura is still on (12 s) and running down: that is not a landing.
    h.run(poison_landing_secs() - 4.5 + 0.1);
    assert!(h.rigs().is_empty(), "the rig outlived its last particle");
    assert_eq!(h.particle_entities(), 0);
}

/// A second application at 2.0 s restarts the cloud: a fresh burst, and the
/// cloud still emitting at 4.5 s — 2.5 s after the restart — on the one rig.
#[test]
fn a_reapplication_restarts_the_cloud() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::ZERO, vec![]);
    h.apply(victim, landed(AbilityType::WoundPoison));
    h.run(2.0);
    let puff_life = poison_landing_emitters()[1].life;
    let living: Vec<Entity> = h
        .particles()
        .into_iter()
        .filter(|(_, s)| s.kind == Kind::Puff && s.age + 0.3 < puff_life)
        .map(|(e, _)| e)
        .collect();
    assert!(living.len() >= 30, "guard: a cloud in flight");
    h.apply(victim, landed(AbilityType::WoundPoison));
    // The burst's whole 167 ms window, and no more.
    h.run(0.2);

    // The puffs already in flight finish their lives: a restart is not a wipe.
    let after = h.particles();
    assert!(living.iter().all(|e| after.contains_key(e)));

    let burst = h.of(Kind::Burst);
    assert!(
        (24..=26).contains(&burst.len()) && burst.iter().all(|f| f.age < 0.25),
        "the restart's burst: {burst:?}"
    );
    assert_eq!(h.rigs().len(), 1, "a restart, never a second cloud");

    h.run(2.3); // t = 4.5
    let youngest = h
        .of(Kind::Puff)
        .iter()
        .map(|p| p.age)
        .fold(f32::MAX, f32::min);
    assert!(
        youngest < 0.1,
        "the restarted cloud is still emitting; youngest puff {youngest:.2} s"
    );
    assert_eq!(h.rigs().len(), 1);
}

/// A Rogue landing a proc every half-second for 8 s keeps ONE steady cloud:
/// never more puffs alive than one timeline can hold (20/s x 2.5 s), and no
/// spent particle left behind under the rig.
#[test]
fn procs_keep_one_steady_cloud_that_never_thickens() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::ZERO, vec![]);
    let mut most = 0;
    let mut most_entities = 0;
    for _ in 0..16 {
        h.apply(victim, landed(AbilityType::DeadlyPoison));
        for _ in 0..25 {
            h.run(DT);
            most = most.max(h.of(Kind::Puff).len());
            most_entities = most_entities.max(h.particle_entities());
            assert_eq!(h.rigs().len(), 1);
        }
    }
    assert!(
        most <= 51,
        "{most} puffs alive at once: the cloud thickened"
    );
    assert!(
        most >= 45,
        "guard: the cloud reached its steady state ({most})"
    );
    // Alive bounds: 51 puffs + 51 motes, and up to two bursts of 26 flares
    // (a burst lives 0.6 s, longer than the 0.5 s between procs), plus the
    // few that died this frame and go next. Spent ones kept under the rig
    // would pile far past it (16 landings emit over 1000).
    assert!(
        most_entities <= 170,
        "{most_entities} particle entities: spent ones were not retired"
    );
}

/// A duration that stops running down — the sim frozen at a decided match —
/// is not a refresh.
#[test]
fn a_frozen_aura_does_not_restart_the_cloud() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::ZERO, vec![]);
    h.apply(victim, landed(AbilityType::MindNumbingPoison));
    h.run(0.1);
    h.app.insert_resource(SimRunning(false));
    h.run(2.0);
    assert!(
        h.of(Kind::Burst).is_empty(),
        "a frozen aura replayed the burst"
    );
    h.run(poison_landing_secs());
    assert!(h.rigs().is_empty());
}

/// A poison that lapses and lands again is a fresh landing.
#[test]
fn a_lapsed_poison_that_lands_again_plays_again() {
    let mut h = Harness::new();
    let victim = h.spawn_unit(Vec3::ZERO, vec![]);
    let mut short = landed(AbilityType::CripplingPoison);
    short.duration = 0.2;
    h.apply(victim, short);
    h.run(1.0);
    assert!(
        h.app
            .world()
            .get::<ActiveAuras>(victim)
            .unwrap()
            .auras
            .is_empty(),
        "guard: the poison lapsed"
    );
    h.apply(victim, landed(AbilityType::CripplingPoison));
    h.run(0.2);
    let burst = h.of(Kind::Burst);
    assert!(
        (24..=26).contains(&burst.len()) && burst.iter().all(|f| f.age < 0.25),
        "{burst:?}"
    );
}
