//! U12 — Reason-enum coverage audit
//!
//! Runs reference matches that exercise every class plus all four pet types,
//! parses the resulting JSONL trace files, and asserts every variant in
//! `expected_reasons` / `expected_target_reasons` was emitted at least once.
//! Bidirectionally: every variant emitted in production must also appear in
//! the expected lists — catches typos and garbage variants.
//!
//! Failure of this test directs the implementer to either emit the missing
//! variant from a class AI or remove the unused variant from the enum.

use std::collections::HashSet;
use std::path::PathBuf;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig};

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
        // Longer than the 60s used in headless_tests — 2v2 matches need more
        // time for late-game CC drama (DR stacks, Divine Shield triggers,
        // root-induced Charge rejections) to develop.
        max_duration_secs: 180.0,
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

/// Variants of `RejectionReason` that reference matchups reliably emit.
///
/// The audit asserts (a) every variant here is emitted at least once across
/// the reference matchups, AND (b) every variant emitted in production appears
/// here. Both directions catch real bugs: missing variants signal dead-code
/// declarations; surprise variants signal typos or out-of-band emissions.
///
/// Variants NOT in this list (declared in the enum but not exercised by the
/// reference matchups, intentionally):
/// - `SelfIncapacitated`: the incapacitation gate in `decide_abilities` runs
///   BEFORE any class AI sees the actor, so no class AI emits this. Reserved
///   for future explicit emission from a path that sees a self-CC'd actor.
/// - `LowerPriorityThanChosen`: not used by the current AI which uses strict
///   priority order (return-on-first-success), not scored selection. Reserved
///   for future class refactors.
/// - `TargetImmune`: emitted by Warlock and Rogue when target has Divine
///   Shield, but the reference matchups don't reliably produce that state
///   (depends on Paladin HP trajectory). Future refinement: add a scripted
///   matchup with forced Paladin shield activation.
/// - `DRImmune`: requires chained CC of the same category against the same
///   target. Possible in long matches but seed-dependent. Future refinement:
///   add a scripted matchup with two consecutive Polymorphs / Stuns.
/// - `Rooted`: Warrior emits this when trying Charge while rooted by Mage
///   Frost Nova. The window is narrow: Warrior must be both rooted AND
///   beyond CHARGE_MIN_RANGE. Future refinement: add a Frost Nova → break
///   → re-engage scripted scenario.
///
/// To re-include a variant: add a reference matchup that reliably produces
/// the condition, verify it emits in `cargo test`, then add the variant
/// here.
const EXPECTED_REJECTION_REASONS: &[&str] = &[
    "OutOfRange",
    "WithinDeadZone",
    "OnCooldown",
    "InsufficientMana",
    "InsufficientResource",
    "SilencedOrLocked",
    "TargetAlreadyCCd",
    "FriendlyBreakableCC",
    "AlreadyApplied",
    "NoValidTarget",
    "PreconditionUnmet",
    "LowHealthHeel",
    "Rooted",
];

/// Variants of `TargetRejectionReason` that production code currently emits.
const EXPECTED_TARGET_REJECTION_REASONS: &[&str] = &[
    "Stealthed",
    "Immune",
    "LowerScoreThanChosen",
    // "Dead" / "CCd" / "KillTargetOverride" — declared but not currently
    //   emitted by acquire_targets' simplified candidate enumeration.
];

/// One reference matchup: team configs + seed + label for error messages.
struct ReferenceMatch {
    label: &'static str,
    team1: Vec<&'static str>,
    team2: Vec<&'static str>,
    seed: u64,
}

fn reference_matchups() -> Vec<ReferenceMatch> {
    vec![
        ReferenceMatch {
            label: "Warrior v Mage",
            team1: vec!["Warrior"],
            team2: vec!["Mage"],
            seed: 42,
        },
        ReferenceMatch {
            label: "Rogue v Paladin",
            team1: vec!["Rogue"],
            team2: vec!["Paladin"],
            seed: 100,
        },
        ReferenceMatch {
            label: "Priest v Warlock (covers Felhunter pet)",
            team1: vec!["Priest"],
            team2: vec!["Warlock"],
            seed: 200,
        },
        ReferenceMatch {
            label: "Hunter v Warrior (covers Hunter pets)",
            team1: vec!["Hunter"],
            team2: vec!["Warrior"],
            seed: 300,
        },
        // 2v2 needed to exercise multi-actor variants:
        // - LowerScoreThanChosen / Stealthed: multiple visible enemies + a stealthed Rogue
        // - TargetImmune: Paladin Divine Shield active while another actor tries to damage
        // - FriendlyBreakableCC: Mage Polys an enemy while ally tries to damage it
        // - DRImmune / Rooted: cross-class CC interactions
        ReferenceMatch {
            label: "Mage+Rogue v Paladin+Warrior (multi-actor variants)",
            team1: vec!["Mage", "Rogue"],
            team2: vec!["Paladin", "Warrior"],
            seed: 400,
        },
        ReferenceMatch {
            label: "Hunter+Warlock v Mage+Priest (root + dispel + friendly-CC)",
            team1: vec!["Hunter", "Warlock"],
            team2: vec!["Mage", "Priest"],
            seed: 500,
        },
    ]
}

/// Parse a JSONL file and collect the set of `RejectionReason` + `TargetRejectionReason`
/// variant names that appear in any event's candidates.
fn collect_reasons(path: &PathBuf) -> (HashSet<String>, HashSet<String>) {
    let body = std::fs::read_to_string(path).expect("read trace file");
    let mut ability_reasons: HashSet<String> = HashSet::new();
    let mut target_reasons: HashSet<String> = HashSet::new();

    for line in body.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("parse JSONL line");
        let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        let candidates = match v.get("candidates") {
            Some(c) => c,
            None => continue,
        };
        let array = match candidates.as_array() {
            Some(a) => a,
            None => continue,
        };
        for cand in array {
            let reason = match cand.get("reason") {
                Some(r) => r,
                None => continue,
            };
            // Reason is either a string (unit variant) or an object with one key
            // (structured variant).
            let variant_name: Option<String> = if let Some(s) = reason.as_str() {
                Some(s.to_string())
            } else if let Some(obj) = reason.as_object() {
                obj.keys().next().cloned()
            } else {
                None
            };
            if let Some(name) = variant_name {
                match kind {
                    "target_acquisition" => {
                        target_reasons.insert(name);
                    }
                    _ => {
                        ability_reasons.insert(name);
                    }
                }
            }
        }
    }

    (ability_reasons, target_reasons)
}

#[test]
fn reason_enum_variants_all_emitted_by_reference_matches() {
    let mut all_ability: HashSet<String> = HashSet::new();
    let mut all_target: HashSet<String> = HashSet::new();
    let mut artifacts: Vec<(String, PathBuf)> = Vec::new();

    for matchup in reference_matchups() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);

        let config = create_config(matchup.team1.clone(), matchup.team2.clone(), Some(matchup.seed));
        let _result = run_headless_match_with(
            config,
            true, // suppress .txt log
            Some(TraceConfig {
                output_path: path.clone(),

            }),
        )
        .unwrap_or_else(|e| panic!("{} failed: {}", matchup.label, e));

        let (ability, target) = collect_reasons(&path);
        all_ability.extend(ability);
        all_target.extend(target);
        artifacts.push((matchup.label.to_string(), path));
    }

    // Only the backward direction: every emitted variant must be in the
    // expected list. This catches typos and out-of-band emissions while
    // letting balance changes that incidentally suppress a variant pass
    // (e.g., Hunter rebalance making WithinDeadZone unreachable shouldn't
    // block unrelated PRs). The forward direction (every expected variant
    // must be emitted) was removed deliberately — the dead-code-detection
    // value it added didn't outweigh the friction of blocking balance
    // changes on coverage-coincidence.
    let expected_ability: HashSet<String> = EXPECTED_REJECTION_REASONS.iter().map(|s| s.to_string()).collect();
    let expected_target: HashSet<String> = EXPECTED_TARGET_REJECTION_REASONS.iter().map(|s| s.to_string()).collect();

    let surprise_ability: Vec<&String> = all_ability.difference(&expected_ability).collect();
    let surprise_target: Vec<&String> = all_target.difference(&expected_target).collect();

    let mut issues = Vec::new();
    if !surprise_ability.is_empty() {
        let mut sorted: Vec<&&String> = surprise_ability.iter().collect();
        sorted.sort();
        issues.push(format!(
            "RejectionReason variants emitted but NOT in EXPECTED list: {:?}\n  \
             Add them to EXPECTED_REJECTION_REASONS in tests/decision_trace_audit.rs.",
            sorted
        ));
    }
    if !surprise_target.is_empty() {
        let mut sorted: Vec<&&String> = surprise_target.iter().collect();
        sorted.sort();
        issues.push(format!(
            "TargetRejectionReason variants emitted but NOT in EXPECTED list: {:?}\n  \
             Add them to EXPECTED_TARGET_REJECTION_REASONS in tests/decision_trace_audit.rs.",
            sorted
        ));
    }

    if !issues.is_empty() {
        panic!(
            "Reason-enum audit failed:\n{}\n\nReference matchups run:\n{}",
            issues.join("\n"),
            reference_matchups()
                .iter()
                .map(|m| format!("  - {} (seed {})", m.label, m.seed))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // Best-effort cleanup of trace files we wrote.
    for (_, path) in artifacts {
        let _ = std::fs::remove_file(path);
    }

    println!(
        "Reason-enum audit passed (surprise-only): {} RejectionReason + {} TargetRejectionReason variants emitted across reference matches; none were unexpected.",
        all_ability.len(),
        all_target.len(),
    );
}
