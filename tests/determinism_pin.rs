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
    // Re-recorded twice in short order, for two INTENDED simulation changes
    // that landed together. Both pinned cells put a Mage against a Warrior,
    // which is inside both blast radii.
    //
    //   AS-54 — Frost Armor's chill became ONE debuff, so its two effects
    //   land, diminish and come off together. Took this cell to Some(2) @
    //   47.982773s from Some(1) @ 66.86647s.
    //   AS-87 — the Mage, the Warlock and the Priest gained a caster main hand
    //   (+5 spell power, +6 mana) where all three wore an empty socket. Both
    //   teams here field one of those classes, so a pin still passing would
    //   mean the change never reached the sim.
    //
    // The value below is measured on the two TOGETHER; neither branch's own
    // figure survives the merge, which is why it was re-run rather than taken
    // from either side. A single seed's winner is not a balance claim — see
    // `docs/design/balance/2026-09-13-frost-armor-one-debuff-findings.md` and
    // `docs/design/balance/2026-09-14-caster-onehander-findings.md`.
    assert_pinned(
        &result,
        Some(2),
        1_111_486_044,
        "2v2 Mage+Priest vs Warrior+Priest @424242",
    );
}

#[test]
fn seeded_1v1_matches_its_recorded_identity() {
    let result = run_headless_match_with(config(&["Mage"], &["Warrior"], 99001), true, None)
        .expect("1v1 run");
    // Re-recorded with the 2v2 pin above, for the same two changes (was
    // 16.049927s on the base both branched from).
    //
    // Worth knowing what this pin can and cannot see: it asserts winner and
    // match_time only. Measured against a pre-AS-87 binary, AS-87 alone moved
    // every Frostbolt in this cell (83->86, 84->87, 87->90) and Frost Nova by
    // 1, yet left match_time untouched, because the killing blow was already
    // an overkill — its logged damage fell 106->96, exactly the 10 points the
    // earlier hits had already removed. So a passing pin here is not evidence
    // that a change did nothing; AS-87 carries its non-vacuity in a paired
    // control sweep instead.
    assert_pinned(
        &result,
        Some(1),
        1_099_720_244,
        "1v1 Mage vs Warrior @99001",
    );
}
