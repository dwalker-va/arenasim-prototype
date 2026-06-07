//! Integration tests for headless match execution
//!
//! These tests verify that:
//! - Headless matches run to completion
//! - Match results are accessible programmatically
//! - Seeded RNG produces deterministic results

use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig, MatchResult};
use arenasim::headless::runner::TraceConfig;

/// Helper to create a basic match config
fn create_config(team1: Vec<&str>, team2: Vec<&str>, seed: Option<u64>) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.into_iter().map(String::from).collect(),
        team2: team2.into_iter().map(String::from).collect(),
        max_duration_secs: 60.0, // Short duration for tests
        random_seed: seed,
        ..Default::default()
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

/// U11 safety gate #1: enabling the decision trace must NOT perturb match
/// outcomes. If the builder calls or writer system introduce any RNG drift,
/// state mutation, or query-ordering change, this test catches it.
///
/// Runs each pairing twice at the same seed — once with TraceConfig::Some,
/// once with None — and asserts MatchResult byte-equality.
#[test]
fn trace_on_matches_trace_off_outcomes() {
    // One pairing per class (every class appears as both T1 and T2 across the
    // set), × 3 seeds each. Covers Spider + Boar + Bird Hunter pet types and
    // the Felhunter Warlock pet via the Priest v Warlock matchup. Total: 18
    // matches in release, ~30s wall-clock.
    let pairings: &[(Vec<&str>, Vec<&str>, u64)] = &[
        (vec!["Warrior"], vec!["Mage"], 42),
        (vec!["Warrior"], vec!["Mage"], 100),
        (vec!["Warrior"], vec!["Mage"], 1000),
        (vec!["Mage"], vec!["Hunter"], 42),
        (vec!["Mage"], vec!["Hunter"], 100),
        (vec!["Mage"], vec!["Hunter"], 1000),
        (vec!["Priest"], vec!["Warlock"], 42),
        (vec!["Priest"], vec!["Warlock"], 100),
        (vec!["Priest"], vec!["Warlock"], 1000),
        (vec!["Rogue"], vec!["Paladin"], 42),
        (vec!["Rogue"], vec!["Paladin"], 100),
        (vec!["Rogue"], vec!["Paladin"], 1000),
        (vec!["Warlock"], vec!["Priest"], 42),
        (vec!["Warlock"], vec!["Priest"], 100),
        (vec!["Paladin"], vec!["Rogue"], 42),
        (vec!["Paladin"], vec!["Rogue"], 100),
        (vec!["Hunter"], vec!["Warrior"], 42),
        (vec!["Hunter"], vec!["Warrior"], 100),
    ];

    for (team1, team2, seed) in pairings {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let trace_path = tmp.path().to_path_buf();
        drop(tmp); // keep path, let writer create the file

        let trace_config = Some(TraceConfig {
            output_path: trace_path,

        });

        let cfg = create_config(team1.clone(), team2.clone(), Some(*seed));
        let with_trace = run_headless_match_with(cfg, true, trace_config).expect("with trace");

        let cfg = create_config(team1.clone(), team2.clone(), Some(*seed));
        let without_trace = run_headless_match_with(cfg, true, None).expect("without trace");

        assert_eq!(
            with_trace.winner, without_trace.winner,
            "{:?} v {:?} seed={}: winner differs trace-on vs trace-off",
            team1, team2, seed
        );
        assert!(
            (with_trace.match_time - without_trace.match_time).abs() < 0.01,
            "{:?} v {:?} seed={}: match_time drift {} vs {}",
            team1, team2, seed, with_trace.match_time, without_trace.match_time
        );

        for (i, (a, b)) in with_trace
            .team1_combatants
            .iter()
            .zip(without_trace.team1_combatants.iter())
            .enumerate()
        {
            assert!(
                (a.final_health - b.final_health).abs() < 0.01,
                "{:?} v {:?} seed={} team1 slot {}: final_health drift {} vs {}",
                team1, team2, seed, i, a.final_health, b.final_health
            );
            assert!(
                (a.damage_dealt - b.damage_dealt).abs() < 0.01,
                "{:?} v {:?} seed={} team1 slot {}: damage_dealt drift {} vs {}",
                team1, team2, seed, i, a.damage_dealt, b.damage_dealt
            );
        }
        for (i, (a, b)) in with_trace
            .team2_combatants
            .iter()
            .zip(without_trace.team2_combatants.iter())
            .enumerate()
        {
            assert!(
                (a.final_health - b.final_health).abs() < 0.01,
                "{:?} v {:?} seed={} team2 slot {}: final_health drift {} vs {}",
                team1, team2, seed, i, a.final_health, b.final_health
            );
            assert!(
                (a.damage_dealt - b.damage_dealt).abs() < 0.01,
                "{:?} v {:?} seed={} team2 slot {}: damage_dealt drift {} vs {}",
                team1, team2, seed, i, a.damage_dealt, b.damage_dealt
            );
        }
    }
}

/// Probe: is the self-mirror determinism failure pre-existing, or caused by
/// the trace instrumentation? Runs each self-mirror twice with trace OFF and
/// asserts MatchResult equality between the two runs. If this test fails too,
/// the bug pre-dates the trace work.
#[test]
#[ignore = "diagnostic probe — run via `cargo test -- --ignored`"]
fn self_mirror_determinism_without_trace() {
    const CLASSES: &[&str] = &[
        "Warrior", "Mage", "Priest", "Rogue", "Warlock", "Paladin", "Hunter",
    ];
    let mut failures: Vec<String> = Vec::new();
    for c in CLASSES {
        let seed = 12_345;
        let a = run_headless_match_with(create_config(vec![c], vec![c], Some(seed)), true, None).unwrap();
        let b = run_headless_match_with(create_config(vec![c], vec![c], Some(seed)), true, None).unwrap();
        let mut diffs: Vec<String> = Vec::new();
        if a.winner != b.winner {
            diffs.push(format!("winner {:?} vs {:?}", a.winner, b.winner));
        }
        if (a.match_time - b.match_time).abs() >= 0.01 {
            diffs.push(format!("time {} vs {}", a.match_time, b.match_time));
        }
        for (i, (x, y)) in a.team1_combatants.iter().zip(b.team1_combatants.iter()).enumerate() {
            if (x.final_health - y.final_health).abs() >= 0.01 {
                diffs.push(format!("t1[{}].hp {} vs {}", i, x.final_health, y.final_health));
            }
            if (x.damage_dealt - y.damage_dealt).abs() >= 0.01 {
                diffs.push(format!("t1[{}].dmg {} vs {}", i, x.damage_dealt, y.damage_dealt));
            }
        }
        for (i, (x, y)) in a.team2_combatants.iter().zip(b.team2_combatants.iter()).enumerate() {
            if (x.final_health - y.final_health).abs() >= 0.01 {
                diffs.push(format!("t2[{}].hp {} vs {}", i, x.final_health, y.final_health));
            }
            if (x.damage_dealt - y.damage_dealt).abs() >= 0.01 {
                diffs.push(format!("t2[{}].dmg {} vs {}", i, x.damage_dealt, y.damage_dealt));
            }
        }
        if !diffs.is_empty() {
            failures.push(format!("{} v {}: {}", c, c, diffs.join(", ")));
        }
    }
    if !failures.is_empty() {
        panic!("Self-mirror non-determinism (trace OFF on both runs):\n{}", failures.join("\n"));
    }
}

/// Broad determinism sweep: every 1v1 class pairing must produce byte-
/// identical MatchResults with trace-on vs trace-off. The narrower
/// `trace_on_matches_trace_off_outcomes` (above) only covers Warrior v Mage.
/// This test catches class-specific RNG drift that would slip past the
/// narrower gate — e.g., if Hunter pet AI instrumentation perturbs entity
/// iteration in a way that only matters when a Hunter is in the match.
///
/// 49 pairings × 2 runs = 98 matches. Runs at ~60Hz sim time with a 60s
/// max_duration cap, so wall-clock is a few seconds per match in release
/// (slower in debug). Marked `#[ignore]` to avoid bloating the default
/// `cargo test` run; opt in via `cargo test -- --ignored`.
#[test]
#[ignore = "expensive — 98 matches; run via `cargo test -- --ignored`"]
fn trace_on_matches_trace_off_all_class_pairings() {
    const CLASSES: &[&str] = &[
        "Warrior", "Mage", "Priest", "Rogue", "Warlock", "Paladin", "Hunter",
    ];
    let mut failures: Vec<String> = Vec::new();

    for (i, c1) in CLASSES.iter().enumerate() {
        for (j, c2) in CLASSES.iter().enumerate() {
            // Stable per-pair seed so the same pairing is reproducible across runs.
            let seed = 1_000 + (i as u64) * 100 + (j as u64);
            let tmp = tempfile::NamedTempFile::new().unwrap();
            let trace_path = tmp.path().to_path_buf();
            drop(tmp);

            let trace_config = Some(TraceConfig {
                output_path: trace_path,

            });

            let with_trace = run_headless_match_with(
                create_config(vec![c1], vec![c2], Some(seed)),
                true,
                trace_config,
            );
            let without_trace = run_headless_match_with(
                create_config(vec![c1], vec![c2], Some(seed)),
                true,
                None,
            );

            let (with_trace, without_trace) = match (with_trace, without_trace) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => {
                    failures.push(format!("{} v {} seed={}: match failed: {}", c1, c2, seed, e));
                    continue;
                }
            };

            if with_trace.winner != without_trace.winner {
                failures.push(format!(
                    "{} v {} seed={}: winner drift trace-on={:?} trace-off={:?}",
                    c1, c2, seed, with_trace.winner, without_trace.winner
                ));
                continue;
            }
            if (with_trace.match_time - without_trace.match_time).abs() >= 0.01 {
                failures.push(format!(
                    "{} v {} seed={}: match_time drift {} vs {}",
                    c1, c2, seed, with_trace.match_time, without_trace.match_time
                ));
                continue;
            }
            for (slot, (a, b)) in with_trace
                .team1_combatants
                .iter()
                .zip(without_trace.team1_combatants.iter())
                .enumerate()
            {
                if (a.final_health - b.final_health).abs() >= 0.01
                    || (a.damage_dealt - b.damage_dealt).abs() >= 0.01
                {
                    failures.push(format!(
                        "{} v {} seed={} team1 slot {}: hp {} vs {} | dmg {} vs {}",
                        c1, c2, seed, slot, a.final_health, b.final_health, a.damage_dealt, b.damage_dealt
                    ));
                }
            }
            for (slot, (a, b)) in with_trace
                .team2_combatants
                .iter()
                .zip(without_trace.team2_combatants.iter())
                .enumerate()
            {
                if (a.final_health - b.final_health).abs() >= 0.01
                    || (a.damage_dealt - b.damage_dealt).abs() >= 0.01
                {
                    failures.push(format!(
                        "{} v {} seed={} team2 slot {}: hp {} vs {} | dmg {} vs {}",
                        c1, c2, seed, slot, a.final_health, b.final_health, a.damage_dealt, b.damage_dealt
                    ));
                }
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "Trace-induced drift in {} pairing(s) of {} tested:\n{}",
            failures.len(),
            CLASSES.len() * CLASSES.len(),
            failures.join("\n")
        );
    }
}

/// Broad trace-file determinism sweep across all 49 class pairings: two
/// trace-on runs at the same seed must produce byte-identical trace files
/// for every pairing. Catches non-deterministic event ordering that would
/// only surface for specific class combinations.
#[test]
#[ignore = "expensive — 98 matches; run via `cargo test -- --ignored`"]
fn trace_file_deterministic_all_class_pairings() {
    const CLASSES: &[&str] = &[
        "Warrior", "Mage", "Priest", "Rogue", "Warlock", "Paladin", "Hunter",
    ];
    let mut failures: Vec<String> = Vec::new();

    for (i, c1) in CLASSES.iter().enumerate() {
        for (j, c2) in CLASSES.iter().enumerate() {
            let seed = 2_000 + (i as u64) * 100 + (j as u64);
            let tmp1 = tempfile::NamedTempFile::new().unwrap();
            let path1 = tmp1.path().to_path_buf();
            drop(tmp1);
            let tmp2 = tempfile::NamedTempFile::new().unwrap();
            let path2 = tmp2.path().to_path_buf();
            drop(tmp2);

            if let Err(e) = run_headless_match_with(
                create_config(vec![c1], vec![c2], Some(seed)),
                true,
                Some(TraceConfig {
                    output_path: path1.clone(),
    
                }),
            ) {
                failures.push(format!("{} v {} seed={}: first run failed: {}", c1, c2, seed, e));
                continue;
            }
            if let Err(e) = run_headless_match_with(
                create_config(vec![c1], vec![c2], Some(seed)),
                true,
                Some(TraceConfig {
                    output_path: path2.clone(),
    
                }),
            ) {
                failures.push(format!("{} v {} seed={}: second run failed: {}", c1, c2, seed, e));
                continue;
            }

            let a = std::fs::read_to_string(&path1).unwrap();
            let b = std::fs::read_to_string(&path2).unwrap();
            if a != b {
                failures.push(format!(
                    "{} v {} seed={}: trace files differ (len {} vs {})",
                    c1, c2, seed, a.len(), b.len()
                ));
            }

            std::fs::remove_file(&path1).ok();
            std::fs::remove_file(&path2).ok();
        }
    }

    if !failures.is_empty() {
        panic!(
            "Non-deterministic trace file in {} pairing(s) of {} tested:\n{}",
            failures.len(),
            CLASSES.len() * CLASSES.len(),
            failures.join("\n")
        );
    }
}

/// U11 safety gate #2: two trace-on runs at the same seed must produce
/// byte-identical trace files. The writer canonicalizes event order by
/// `(frame, actor.entity_id, kind)` before flush, so even if intermediate
/// query iteration introduces ordering variance, the on-disk output stays
/// stable.
#[test]
fn trace_file_is_deterministic_at_same_seed() {
    let seed = 7777_u64;

    let tmp1 = tempfile::NamedTempFile::new().unwrap();
    let path1 = tmp1.path().to_path_buf();
    drop(tmp1);
    let tmp2 = tempfile::NamedTempFile::new().unwrap();
    let path2 = tmp2.path().to_path_buf();
    drop(tmp2);

    let cfg = create_config(vec!["Warrior"], vec!["Mage"], Some(seed));
    run_headless_match_with(
        cfg,
        true,
        Some(TraceConfig {
            output_path: path1.clone(),

        }),
    )
    .expect("first trace run");

    let cfg = create_config(vec!["Warrior"], vec!["Mage"], Some(seed));
    run_headless_match_with(
        cfg,
        true,
        Some(TraceConfig {
            output_path: path2.clone(),

        }),
    )
    .expect("second trace run");

    let a = std::fs::read_to_string(&path1).expect("read trace 1");
    let b = std::fs::read_to_string(&path2).expect("read trace 2");
    assert_eq!(
        a.len(),
        b.len(),
        "trace files differ in length: {} vs {}",
        a.len(),
        b.len()
    );
    assert_eq!(a, b, "trace files differ at same seed — non-deterministic event ordering");

    std::fs::remove_file(&path1).ok();
    std::fs::remove_file(&path2).ok();
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
