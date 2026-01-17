//! Unit tests for combat log query and aggregation methods
//!
//! These tests verify that the CombatLog correctly:
//! - Aggregates damage by ability
//! - Counts killing blows
//! - Tracks CC duration
//! - Identifies surviving combatants

use arenasim::combat::log::{
    CombatLog, CombatLogEventType, CombatantId, StructuredEventData,
};

fn create_test_log() -> CombatLog {
    CombatLog::default()
}

// =============================================================================
// Damage Aggregation Tests
// =============================================================================

#[test]
fn test_damage_by_ability_empty_log() {
    let log = create_test_log();
    let damage = log.damage_by_ability("Team 1 Warrior");
    assert!(damage.is_empty(), "Empty log should return empty damage map");
}

#[test]
fn test_damage_by_ability_single_source() {
    let mut log = create_test_log();

    // Log some damage from a single source
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        50.0,
        false,
        "Test message".to_string(),
    );
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        45.0,
        false,
        "Test message".to_string(),
    );
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Auto Attack".to_string(),
        20.0,
        false,
        "Test message".to_string(),
    );

    let damage = log.damage_by_ability("Team 1 Warrior");

    assert_eq!(damage.len(), 2, "Should have 2 different abilities");
    assert_eq!(damage.get("Mortal Strike"), Some(&95.0), "Mortal Strike should total 95 damage");
    assert_eq!(damage.get("Auto Attack"), Some(&20.0), "Auto Attack should be 20 damage");
}

#[test]
fn test_damage_by_ability_multiple_sources() {
    let mut log = create_test_log();

    // Both warriors deal damage
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        50.0,
        false,
        "Test".to_string(),
    );
    log.log_damage(
        "Team 2 Warrior".to_string(),
        "Team 1 Mage".to_string(),
        "Mortal Strike".to_string(),
        60.0,
        false,
        "Test".to_string(),
    );

    let team1_damage = log.damage_by_ability("Team 1 Warrior");
    let team2_damage = log.damage_by_ability("Team 2 Warrior");

    assert_eq!(team1_damage.get("Mortal Strike"), Some(&50.0));
    assert_eq!(team2_damage.get("Mortal Strike"), Some(&60.0));
}

#[test]
fn test_total_damage_dealt() {
    let mut log = create_test_log();

    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        50.0,
        false,
        "Test".to_string(),
    );
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Auto Attack".to_string(),
        20.0,
        false,
        "Test".to_string(),
    );
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Priest".to_string(),
        "Rend".to_string(),
        30.0,
        false,
        "Test".to_string(),
    );

    let total = log.total_damage_dealt("Team 1 Warrior");
    assert_eq!(total, 100.0, "Total damage should be 100");
}

#[test]
fn test_total_damage_taken() {
    let mut log = create_test_log();

    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Frostbolt".to_string(),
        40.0,
        false,
        "Test".to_string(),
    );
    log.log_damage(
        "Team 1 Rogue".to_string(),
        "Team 2 Mage".to_string(),
        "Ambush".to_string(),
        80.0,
        false,
        "Test".to_string(),
    );

    let taken = log.total_damage_taken("Team 2 Mage");
    assert_eq!(taken, 120.0, "Mage should have taken 120 damage");
}

// =============================================================================
// Healing Aggregation Tests
// =============================================================================

#[test]
fn test_healing_by_ability() {
    let mut log = create_test_log();

    log.log_healing(
        "Team 1 Priest".to_string(),
        "Team 1 Warrior".to_string(),
        "Flash Heal".to_string(),
        50.0,
        "Test".to_string(),
    );
    log.log_healing(
        "Team 1 Priest".to_string(),
        "Team 1 Warrior".to_string(),
        "Flash Heal".to_string(),
        45.0,
        "Test".to_string(),
    );
    log.log_healing(
        "Team 1 Priest".to_string(),
        "Team 1 Priest".to_string(),
        "Flash Heal".to_string(),
        30.0,
        "Test".to_string(),
    );

    let healing = log.healing_by_ability("Team 1 Priest");
    assert_eq!(healing.get("Flash Heal"), Some(&125.0));
}

#[test]
fn test_total_healing_done() {
    let mut log = create_test_log();

    log.log_healing(
        "Team 1 Priest".to_string(),
        "Team 1 Warrior".to_string(),
        "Flash Heal".to_string(),
        50.0,
        "Test".to_string(),
    );
    log.log_healing(
        "Team 1 Priest".to_string(),
        "Team 1 Priest".to_string(),
        "Flash Heal".to_string(),
        30.0,
        "Test".to_string(),
    );

    let total = log.total_healing_done("Team 1 Priest");
    assert_eq!(total, 80.0);
}

// =============================================================================
// Killing Blow Tests
// =============================================================================

#[test]
fn test_killing_blows_none() {
    let mut log = create_test_log();

    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        50.0,
        false, // Not a killing blow
        "Test".to_string(),
    );

    assert_eq!(log.killing_blows("Team 1 Warrior"), 0);
}

#[test]
fn test_killing_blows_counted() {
    let mut log = create_test_log();

    // First hit - not a killing blow
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        50.0,
        false,
        "Test".to_string(),
    );

    // Killing blow!
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        100.0,
        true, // Killing blow
        "Test".to_string(),
    );

    // Another kill
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Priest".to_string(),
        "Auto Attack".to_string(),
        20.0,
        true, // Killing blow
        "Test".to_string(),
    );

    assert_eq!(log.killing_blows("Team 1 Warrior"), 2);
}

#[test]
fn test_killing_blows_per_combatant() {
    let mut log = create_test_log();

    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        100.0,
        true,
        "Test".to_string(),
    );
    log.log_damage(
        "Team 1 Rogue".to_string(),
        "Team 2 Priest".to_string(),
        "Ambush".to_string(),
        150.0,
        true,
        "Test".to_string(),
    );
    log.log_damage(
        "Team 1 Rogue".to_string(),
        "Team 2 Warrior".to_string(),
        "Eviscerate".to_string(),
        80.0,
        true,
        "Test".to_string(),
    );

    assert_eq!(log.killing_blows("Team 1 Warrior"), 1);
    assert_eq!(log.killing_blows("Team 1 Rogue"), 2);
    assert_eq!(log.killing_blows("Team 2 Mage"), 0);
}

// =============================================================================
// CC Duration Tests
// =============================================================================

#[test]
fn test_cc_done_seconds() {
    let mut log = create_test_log();

    log.log_crowd_control(
        "Team 1 Mage".to_string(),
        "Team 2 Warrior".to_string(),
        "Frost Nova".to_string(),
        6.0, // 6 second root
        "Test".to_string(),
    );
    log.log_crowd_control(
        "Team 1 Mage".to_string(),
        "Team 2 Priest".to_string(),
        "Polymorph".to_string(),
        8.0, // 8 second sheep
        "Test".to_string(),
    );

    let cc_done = log.cc_done_seconds("Team 1 Mage");
    assert_eq!(cc_done, 14.0);
}

#[test]
fn test_cc_received_seconds() {
    let mut log = create_test_log();

    log.log_crowd_control(
        "Team 1 Mage".to_string(),
        "Team 2 Warrior".to_string(),
        "Frost Nova".to_string(),
        6.0,
        "Test".to_string(),
    );
    log.log_crowd_control(
        "Team 1 Rogue".to_string(),
        "Team 2 Warrior".to_string(),
        "Kidney Shot".to_string(),
        5.0,
        "Test".to_string(),
    );

    let cc_received = log.cc_received_seconds("Team 2 Warrior");
    assert_eq!(cc_received, 11.0);
}

// =============================================================================
// Survival/Death Tests
// =============================================================================

#[test]
fn test_combatant_survived_no_deaths() {
    let mut log = create_test_log();

    // Only damage, no deaths logged
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Mortal Strike".to_string(),
        50.0,
        false,
        "Test".to_string(),
    );

    assert!(log.combatant_survived("Team 1 Warrior"));
    assert!(log.combatant_survived("Team 2 Mage"));
}

#[test]
fn test_combatant_survived_with_death() {
    let mut log = create_test_log();

    log.log_death(
        "Team 2 Mage".to_string(),
        Some("Team 1 Warrior".to_string()),
        "Team 2 Mage died".to_string(),
    );

    assert!(log.combatant_survived("Team 1 Warrior"), "Killer should survive");
    assert!(!log.combatant_survived("Team 2 Mage"), "Dead combatant should not survive");
}

// =============================================================================
// All Combatants Tests
// =============================================================================

#[test]
fn test_all_combatants_from_registration() {
    let mut log = create_test_log();

    log.register_combatant("Team 1 Warrior".to_string());
    log.register_combatant("Team 1 Priest".to_string());
    log.register_combatant("Team 2 Mage".to_string());

    let combatants = log.all_combatants();
    assert_eq!(combatants.len(), 3);
    assert!(combatants.contains(&"Team 1 Warrior".to_string()));
    assert!(combatants.contains(&"Team 1 Priest".to_string()));
    assert!(combatants.contains(&"Team 2 Mage".to_string()));
}

#[test]
fn test_all_combatants_no_duplicates() {
    let mut log = create_test_log();

    log.register_combatant("Team 1 Warrior".to_string());
    log.register_combatant("Team 1 Warrior".to_string()); // Duplicate

    let combatants = log.all_combatants();
    assert_eq!(combatants.len(), 1);
}

// =============================================================================
// Ability Cast Timeline Tests
// =============================================================================

#[test]
fn test_ability_casts_for_combatant() {
    let mut log = create_test_log();
    log.match_time = 5.0;

    log.log_ability_cast(
        "Team 1 Mage".to_string(),
        "Frostbolt".to_string(),
        Some("Team 2 Warrior".to_string()),
        "Test".to_string(),
    );

    log.match_time = 8.0;
    log.log_ability_cast(
        "Team 1 Mage".to_string(),
        "Frost Nova".to_string(),
        None,
        "Test".to_string(),
    );

    let casts = log.ability_casts_for("Team 1 Mage");
    assert_eq!(casts.len(), 2);
    assert_eq!(casts[0], (5.0, "Frostbolt", false));
    assert_eq!(casts[1], (8.0, "Frost Nova", false));
}

#[test]
fn test_mark_cast_interrupted() {
    let mut log = create_test_log();
    log.match_time = 5.0;

    log.log_ability_cast(
        "Team 1 Mage".to_string(),
        "Frostbolt".to_string(),
        Some("Team 2 Warrior".to_string()),
        "Test".to_string(),
    );

    // Mark it as interrupted
    log.mark_cast_interrupted("Team 1 Mage", "Frostbolt");

    let casts = log.ability_casts_for("Team 1 Mage");
    assert_eq!(casts.len(), 1);
    assert_eq!(casts[0], (5.0, "Frostbolt", true)); // interrupted = true
}

// =============================================================================
// Filter Tests
// =============================================================================

#[test]
fn test_filter_by_type() {
    let mut log = create_test_log();

    log.log(CombatLogEventType::MatchEvent, "Match started".to_string());
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Test".to_string(),
        50.0,
        false,
        "Test".to_string(),
    );
    log.log_healing(
        "Team 1 Priest".to_string(),
        "Team 1 Warrior".to_string(),
        "Test".to_string(),
        30.0,
        "Test".to_string(),
    );

    let damage_events = log.filter_by_type(CombatLogEventType::Damage);
    assert_eq!(damage_events.len(), 1);

    let healing_events = log.filter_by_type(CombatLogEventType::Healing);
    assert_eq!(healing_events.len(), 1);

    let match_events = log.filter_by_type(CombatLogEventType::MatchEvent);
    assert_eq!(match_events.len(), 1);
}

#[test]
fn test_hp_changes_only() {
    let mut log = create_test_log();

    log.log(CombatLogEventType::MatchEvent, "Match started".to_string());
    log.log_damage(
        "Team 1 Warrior".to_string(),
        "Team 2 Mage".to_string(),
        "Test".to_string(),
        50.0,
        false,
        "Test".to_string(),
    );
    log.log_healing(
        "Team 1 Priest".to_string(),
        "Team 1 Warrior".to_string(),
        "Test".to_string(),
        30.0,
        "Test".to_string(),
    );
    log.log(CombatLogEventType::AuraApplied, "Buff applied".to_string());

    let hp_changes = log.hp_changes_only();
    assert_eq!(hp_changes.len(), 2, "Should only include damage and healing events");
}

#[test]
fn test_recent_entries() {
    let mut log = create_test_log();

    for i in 0..10 {
        log.log(CombatLogEventType::MatchEvent, format!("Event {}", i));
    }

    let recent = log.recent(3);
    assert_eq!(recent.len(), 3);
    assert_eq!(recent[0].message, "Event 7");
    assert_eq!(recent[1].message, "Event 8");
    assert_eq!(recent[2].message, "Event 9");
}
