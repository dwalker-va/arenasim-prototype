//! Determinism pin — a `cargo test`-visible guard on the byte-identity claim.
//!
//! `scripts/behaviour_baseline.sh` + `tests/baselines/` are the thorough
//! instrument (27 matches, whole-log SHA), but they are a manual gate: nothing
//! runs them unless a person remembers to. Graphical-only features are added to
//! this repo on the standing promise that the simulation does not move, and a
//! promise guarded only by remembering is one broken change away from being
//! false.
//!
//! This pins two fixed-seed matches to exact recorded values so ordinary
//! `cargo test` fails the moment a supposedly-inert change perturbs the sim.
//! It is deliberately narrow — two cells, not twenty-seven — because its job is
//! to catch drift early and cheaply, not to replace the baseline script. When
//! this fails, run the script for the full picture.
//!
//! ON A FAILURE HERE: do not re-record the constants to make it pass. A moved
//! value means simulation behaviour changed; either that was intended (say why,
//! and update `tests/baselines/` too per its README) or it is the regression
//! this file exists to catch.

use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig, MatchResult};

fn config(team1: &[&str], team2: &[&str], seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        random_seed: Some(seed),
        ..Default::default()
    }
}

/// Assert a result against its recorded identity, bit-exact on the float.
///
/// `match_time` is compared by bits rather than by epsilon on purpose: the
/// claim is "nothing changed", and a tolerance band would quietly absorb the
/// small perturbations that are precisely the early symptom of a sim leak.
fn assert_pinned(result: &MatchResult, winner: Option<u8>, time_bits: u32, cell: &str) {
    assert_eq!(result.winner, winner, "{cell}: winner moved");
    assert_eq!(
        result.match_time.to_bits(),
        time_bits,
        "{cell}: match_time moved: {} (bits {}) vs recorded bits {}",
        result.match_time,
        result.match_time.to_bits(),
        time_bits
    );
}

#[test]
fn seeded_2v2_matches_its_recorded_identity() {
    let result = run_headless_match_with(
        config(&["Mage", "Priest"], &["Warrior", "Priest"], 424242),
        true,
        None,
    )
    .expect("2v2 run");
    // 66.86647s — recorded on feat/in-match-kill-call-banter after the branch
    // was confirmed byte-identical to main, so this is main's value too.
    assert_pinned(
        &result,
        Some(1),
        1_116_060_578,
        "2v2 Mage+Priest vs Warrior+Priest @424242",
    );
}

#[test]
fn seeded_1v1_matches_its_recorded_identity() {
    let result =
        run_headless_match_with(config(&["Mage"], &["Warrior"], 99001), true, None).expect("1v1 run");
    // 16.049927s — same provenance as the 2v2 pin above.
    assert_pinned(&result, Some(1), 1_098_933_824, "1v1 Mage vs Warrior @99001");
}
