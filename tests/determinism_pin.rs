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
    // Measured on the two TOGETHER: 49.38275s. Neither branch's own figure
    // survives the merge — AS-54 alone recorded 47.982773s and AS-87 alone
    // 58.44928s — which is why this was re-run rather than taken from either
    // side. The winner (Some(2)) is AS-54's; AS-87 moves the duration within
    // that outcome. A single seed's winner is not a balance claim — see
    // `docs/design/balance/2026-09-13-frost-armor-one-debuff-findings.md` and
    // `docs/design/balance/2026-09-14-caster-onehander-findings.md`.
    //
    // Re-recorded for AS-115, the Warrior's Arcanite Reaper re-priced from
    // 4 AP to 11 AP + 3% crit. Took this cell to Some(1) @ 40.69955s from
    // Some(2) @ 49.38275s. The 1v1 pin below did not move. Paired sweep:
    // `docs/design/balance/2026-09-27-as115-two-hander-budget.md`.
    //
    // Re-recorded for AS-167, weapon speed from item data. Took this cell to
    // Some(1) @ 45.966137s from Some(1) @ 40.69955s. The first line that
    // differs from `main` @ 17cb9f0 is the two Priests' opening Wand Shots
    // into each other's shields, 10 -> 13 absorbed: the Staff of Dominance now
    // fires every 1.6s for the same DPS, so each shot hits harder, and every
    // shield after it runs down at a different point. Paired sweep:
    // `docs/design/balance/2026-09-28-as167-weapon-speed.md`.
    //
    // Re-recorded for AS-172, Warriors entering the gates at 0 rage instead of
    // 100. Took this cell to Some(1) @ 45.616142s from Some(1) @ 45.966137s.
    // The first line that differs from `main` @ d5f21c1 is the Warrior's first
    // swing on the Mage: a queued Heroic Strike crit (184) becomes a plain
    // Auto Attack crit (122), because the rage that paid for it was the free
    // starting bar.
    //
    // Re-recorded for AS-187, the Priest's dispel walk. Took this cell to
    // Some(1) @ 47.516113s from Some(1) @ 45.616142s. At 18.50s (log clock) the
    // team-1 Priest's Psychic Scream fears the Warrior 33yd from its own Priest,
    // beyond Dispel Magic's 30yd; that Priest now walks toward it (the trace's
    // only `DispelChase`, at the same tick), and the first line that differs
    // from `main` @ 0a9a0c0 follows at 20.17s: the team-1 Priest's next Wand
    // Shot into the walking Priest lands two frames earlier.
    assert_pinned(
        &result,
        Some(1),
        1_111_363_712,
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
    //
    // Re-recorded for AS-167, weapon speed from item data. Took this cell to
    // Some(2) @ 15.983261s from Some(1) @ 17.549904s. The first line that
    // differs from `main` @ 17cb9f0 is the Mage's first Wand Shot, 7 -> 9: the
    // Wand of Shadows now fires every 1.8s for the same DPS, so each shot hits
    // harder. The Warrior's first swing follows (a 30-damage crit becomes a
    // 122-damage one — the Arcanite Reaper's 3.8s swing at its two-hander DPS).
    //
    // Re-recorded for AS-172, Warriors entering the gates at 0 rage instead of
    // 100. Took this cell to Some(1) @ 17.549904s from Some(2) @ 15.983261s.
    // The first line that differs from `main` @ d5f21c1 is the Warrior's
    // killing blow: a Heroic Strike (72) the starting bar paid for becomes an
    // Auto Attack (61), the Mage survives it and wins. That the Mage's kill
    // lands on the same bits as before AS-167 is the overkill blindness noted
    // above, not a revert: the log's Frostbolt and Wand Shot damage still
    // carry AS-167's numbers.
    assert_pinned(
        &result,
        Some(1),
        1_099_720_244,
        "1v1 Mage vs Warrior @99001",
    );
}
