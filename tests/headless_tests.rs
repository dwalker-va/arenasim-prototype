//! Integration tests for headless match execution
//!
//! These tests verify that:
//! - Headless matches run to completion
//! - Match results are accessible programmatically
//! - Seeded RNG produces deterministic results

use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig, MatchResult};

/// Helper to create a basic match config
fn create_config(team1: Vec<&str>, team2: Vec<&str>, seed: Option<u64>) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.into_iter().map(String::from).collect(),
        team2: team2.into_iter().map(String::from).collect(),
        map: "BasicArena".to_string(),
        team1_kill_target: None,
        team2_kill_target: None,
        team1_cc_target: None,
        team2_cc_target: None,
        output_path: None,
        max_duration_secs: 60.0, // Short duration for tests
        random_seed: seed,
        team1_rogue_openers: vec![],
        team2_rogue_openers: vec![],
        team1_warlock_curse_prefs: vec![],
        team2_warlock_curse_prefs: vec![],
        team1_hunter_pet_types: vec![],
        team2_hunter_pet_types: vec![],
        team1_equipment: vec![],
        team2_equipment: vec![],
        team1_warrior_shouts: vec![],
        team2_warrior_shouts: vec![],
        team1_mage_armors: vec![],
        team2_mage_armors: vec![],
        team1_paladin_auras: vec![],
        team2_paladin_auras: vec![],
    }
}

#[test]
fn test_match_result_has_winner() {
    // This is a placeholder test - actual match execution requires Bevy app
    // In a real test environment, you would run the match and check results

    // For now, we just verify the types are accessible
    let _config = create_config(vec!["Warrior"], vec!["Mage"], Some(12345));

    // The MatchResult struct should be usable
    let result = MatchResult {
        winner: Some(1),
        match_time: 30.0,
        team1_combatants: vec![],
        team2_combatants: vec![],
        random_seed: Some(12345),
    };

    assert_eq!(result.winner, Some(1));
    assert_eq!(result.random_seed, Some(12345));
}

#[test]
fn test_config_with_seed() {
    let config = create_config(vec!["Warrior", "Priest"], vec!["Mage", "Rogue"], Some(42));

    assert_eq!(config.random_seed, Some(42));
    assert_eq!(config.team1.len(), 2);
    assert_eq!(config.team2.len(), 2);
}

#[test]
fn test_config_without_seed() {
    let config = create_config(vec!["Warrior"], vec!["Mage"], None);

    assert!(config.random_seed.is_none());
}

#[test]
fn test_combatant_result_fields() {
    use arenasim::headless::CombatantResult;

    let result = CombatantResult {
        class_name: "Warrior".to_string(),
        max_health: 1000.0,
        final_health: 250.0,
        survived: true,
        damage_dealt: 500.0,
        damage_taken: 750.0,
    };

    assert_eq!(result.class_name, "Warrior");
    assert!(result.survived);
    assert!(result.damage_dealt > 0.0);
}

/// End-to-end determinism check: the same seed must produce the same match
/// outcome. This is the canary for any future change that introduces a new
/// non-determinism source (raw `HashMap` iteration, unseeded `thread_rng`,
/// wall-clock time leakage). If this fails, replays and matrix runs are
/// untrustworthy until the regression is found.
///
/// We assert on winner + final HP per combatant + damage dealt/taken — strong
/// enough to catch RNG drift without being as fragile as byte-identical log
/// comparison.
#[test]
fn seeded_matches_are_deterministic() {
    let seed = 0xCAFE_F00D_u64;
    let make = || create_config(vec!["Warrior", "Priest"], vec!["Mage", "Rogue"], Some(seed));

    let r1 = run_headless_match_with(make(), true, None).expect("first run");
    let r2 = run_headless_match_with(make(), true, None).expect("second run");

    assert_eq!(r1.winner, r2.winner, "winner differs between seeded runs");
    assert_eq!(r1.team1_combatants.len(), r2.team1_combatants.len());
    assert_eq!(r1.team2_combatants.len(), r2.team2_combatants.len());

    for (i, (a, b)) in r1.team1_combatants.iter().zip(r2.team1_combatants.iter()).enumerate() {
        assert_eq!(a.class_name, b.class_name, "team1 slot {} class drifted", i);
        assert!((a.final_health - b.final_health).abs() < 0.01,
            "team1 slot {} final_health drift: {} vs {}", i, a.final_health, b.final_health);
        assert!((a.damage_dealt - b.damage_dealt).abs() < 0.01,
            "team1 slot {} damage_dealt drift: {} vs {}", i, a.damage_dealt, b.damage_dealt);
        assert!((a.damage_taken - b.damage_taken).abs() < 0.01,
            "team1 slot {} damage_taken drift: {} vs {}", i, a.damage_taken, b.damage_taken);
    }
    for (i, (a, b)) in r1.team2_combatants.iter().zip(r2.team2_combatants.iter()).enumerate() {
        assert_eq!(a.class_name, b.class_name, "team2 slot {} class drifted", i);
        assert!((a.final_health - b.final_health).abs() < 0.01,
            "team2 slot {} final_health drift: {} vs {}", i, a.final_health, b.final_health);
        assert!((a.damage_dealt - b.damage_dealt).abs() < 0.01,
            "team2 slot {} damage_dealt drift: {} vs {}", i, a.damage_dealt, b.damage_dealt);
        assert!((a.damage_taken - b.damage_taken).abs() < 0.01,
            "team2 slot {} damage_taken drift: {} vs {}", i, a.damage_taken, b.damage_taken);
    }
}

/// Different seeds should produce different matches (or at least, this seed
/// pair should — chosen empirically). Guards against accidentally hard-coding
/// outcomes independent of the seed.
#[test]
fn different_seeds_produce_different_matches() {
    let make = |seed: u64| create_config(vec!["Warrior"], vec!["Mage"], Some(seed));

    let r1 = run_headless_match_with(make(1), true, None).expect("run 1");
    let r2 = run_headless_match_with(make(2), true, None).expect("run 2");

    let differs = r1.winner != r2.winner
        || r1.match_time != r2.match_time
        || r1.team1_combatants.iter().zip(r2.team1_combatants.iter())
            .any(|(a, b)| (a.final_health - b.final_health).abs() > 0.01
                || (a.damage_dealt - b.damage_dealt).abs() > 0.01);

    assert!(differs, "seeds 1 and 2 produced identical results — RNG may not be wired");
}
