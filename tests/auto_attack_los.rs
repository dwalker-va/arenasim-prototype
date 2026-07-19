//! Durable guard: the ranged auto-attack line-of-sight gate in
//! `combat_auto_attack` suppresses swings when an obstacle occludes the target,
//! while melee auto-attacks are exempt.
//!
//! The gate under test is:
//! ```ignore
//! if !attacker_is_melee
//!     && !has_line_of_sight(&map_geometry.volumes, my_pos, target_pos)
//! {
//!     continue;
//! }
//! ```
//! in `src/states/play_match/combat_core/auto_attack.rs`. This drives the real
//! system in a minimal Bevy App (MinimalPlugins clock, gates forced open, only
//! `combat_auto_attack` registered) so it pins the wiring — the resource read,
//! the melee exemption, and the "occlusion skips the swing" behavior — not just
//! the underlying `has_line_of_sight` helper (which is covered in
//! `map_geometry`'s unit tests).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::MinimalPlugins;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::combat_core::combat_auto_attack;
use arenasim::states::play_match::components::{Combatant, GameRng, MatchCountdown};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::AbilityDefinitions;
use arenasim::CharacterClass;

/// Minimal App running only `combat_auto_attack`, with gates open, a manual
/// 1/60s clock (same strategy as the headless runner), and `obstacles` as the
/// active map geometry.
fn harness_app(obstacles: Vec<ObstacleVolume>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(0))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ActiveMapGeometry {
            volumes: obstacles,
            cover_anchors: Vec::new(),
        })
        .add_systems(Update, combat_auto_attack);
    app
}

/// Spawn a combatant of `class` at `pos` with a fast attack speed (so several
/// swings resolve within the short test window) and return its entity.
fn spawn_combatant(app: &mut App, class: CharacterClass, pos: Vec3) -> Entity {
    let mut combatant = Combatant::new(1, 0, class);
    // Speed up swings so the ~1s window covers several attack intervals.
    combatant.attack_speed = 5.0;
    app.world_mut()
        .spawn((Transform::from_translation(pos), combatant))
        .id()
}

fn set_target(app: &mut App, attacker: Entity, target: Entity) {
    app.world_mut()
        .get_mut::<Combatant>(attacker)
        .unwrap()
        .target = Some(target);
}

fn health(app: &App, entity: Entity) -> f32 {
    app.world().get::<Combatant>(entity).unwrap().current_health
}

fn max_health(app: &App, entity: Entity) -> f32 {
    app.world().get::<Combatant>(entity).unwrap().max_health
}

fn damage_dealt(app: &App, entity: Entity) -> f32 {
    app.world().get::<Combatant>(entity).unwrap().damage_dealt
}

/// A full-height pillar centered on the origin (the shipped PillaredArena pillar
/// radius), straddling an attacker→target segment that runs along Z through the
/// origin.
fn blocking_pillar() -> ObstacleVolume {
    ObstacleVolume::Cylinder {
        center_xz: Vec2::new(0.0, 0.0),
        radius: 2.5,
        base_y: 0.0,
        height: 5.0,
    }
}

/// (a) A ranged attacker (Hunter Auto Shot) with a pillar between it and its
/// target deals NO auto damage over several attack intervals — the LoS gate
/// skips every occluded swing.
#[test]
fn ranged_auto_blocked_by_obstacle_deals_no_damage() {
    let mut app = harness_app(vec![blocking_pillar()]);
    // Distance 24: within AUTO_SHOT_RANGE (35) and beyond the Hunter dead zone
    // (8), so range/dead-zone are NOT the reason a swing is skipped.
    let hunter = spawn_combatant(&mut app, CharacterClass::Hunter, Vec3::new(0.0, 1.0, -12.0));
    let victim = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 12.0));
    set_target(&mut app, hunter, victim);

    for _ in 0..60 {
        app.update();
    }

    assert_eq!(
        health(&app, victim),
        max_health(&app, victim),
        "an occluded ranged auto must deal no damage"
    );
    assert_eq!(
        damage_dealt(&app, hunter),
        0.0,
        "the occluded attacker must record zero damage dealt"
    );
}

/// (b) Harness sanity: the SAME setup with NO obstacle deals damage — proving
/// the test can distinguish blocked from clear (i.e. it can fail).
#[test]
fn ranged_auto_with_clear_los_deals_damage() {
    let mut app = harness_app(Vec::new());
    let hunter = spawn_combatant(&mut app, CharacterClass::Hunter, Vec3::new(0.0, 1.0, -12.0));
    let victim = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 12.0));
    set_target(&mut app, hunter, victim);

    for _ in 0..60 {
        app.update();
    }

    assert!(
        health(&app, victim) < max_health(&app, victim),
        "a clear-LoS ranged auto must deal damage (got {} / {})",
        health(&app, victim),
        max_health(&app, victim)
    );
    assert!(
        damage_dealt(&app, hunter) > 0.0,
        "the attacker must record damage dealt with clear LoS"
    );
}

/// (c) Melee exemption: a melee attacker adjacent to its target with a thin box
/// straddling the line between them STILL swings — the LoS gate applies only to
/// ranged autos, so two melee units flanking a thin obstacle edge trade hits.
#[test]
fn melee_auto_swings_through_thin_obstacle() {
    // A thin wall on the z=0 plane between the two adjacent melee units. It
    // occludes the attacker→target segment, but melee autos are LoS-exempt.
    let thin_wall = ObstacleVolume::Aabb {
        min: Vec3::new(-5.0, 0.0, -0.2),
        max: Vec3::new(5.0, 2.0, 0.2),
    };
    let mut app = harness_app(vec![thin_wall]);
    // Distance 2.0: within MELEE_RANGE (2.5), on opposite sides of the wall.
    let warrior = spawn_combatant(&mut app, CharacterClass::Warrior, Vec3::new(0.0, 1.0, -1.0));
    let victim = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 1.0));
    set_target(&mut app, warrior, victim);

    for _ in 0..60 {
        app.update();
    }

    assert!(
        health(&app, victim) < max_health(&app, victim),
        "a melee auto must swing through a thin obstacle (got {} / {})",
        health(&app, victim),
        max_health(&app, victim)
    );
    assert!(
        damage_dealt(&app, warrior) > 0.0,
        "the melee attacker must record damage dealt despite the occluding wall"
    );
}
