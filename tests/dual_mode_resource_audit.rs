//! Match-scoped resources must be inserted in BOTH modes.
//!
//! `tests/registration_audit.rs` enforces this for *systems*. Nothing enforced
//! it for *resources*, and the gap cost a real bug: `CcPolicies` and
//! `InterruptPolicies` were inserted by the headless runner and never by the
//! graphical path, so the client silently ran every match as `Identity`. A
//! `--replay` of a `Priced` config played identically to an `Identity` one while
//! the headless run of the same seed diverged — the AI change was simply absent,
//! and nothing said so.
//!
//! The same session also hit the component form of this: `RecentDamage` was
//! attached at the two spawn sites in `play_match/mod.rs` and at none of the six
//! in `headless/runner.rs`, which silently zeroed every denial rate.
//!
//! This is a source-level audit, in the style of `registration_audit.rs`: it
//! greps rather than runs, because building the graphical app needs a window and
//! a GPU adapter that CI may not have.

use std::fs;

/// Match-scoped resources that both modes must insert. Add to this list when
/// introducing a new one — that is the point of the test.
const MATCH_SCOPED_RESOURCES: &[&str] = &[
    "AiProfiles",
    "CcPolicies",
    "InterruptPolicies",
    "TeamPlans",
];

/// Where each mode sets up a match.
const HEADLESS_SETUP: &str = "src/headless/runner.rs";
const GRAPHICAL_SETUP: &str = "src/states/play_match/mod.rs";

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

#[test]
fn match_scoped_resources_are_inserted_in_both_modes() {
    let headless = read(HEADLESS_SETUP);
    let graphical = read(GRAPHICAL_SETUP);

    let mut missing = Vec::new();
    for res in MATCH_SCOPED_RESOURCES {
        // The insert may be `insert_resource(x)` where `x` was resolved earlier,
        // so require only that the type is named somewhere in each setup path.
        if !headless.contains(res) {
            missing.push(format!("{res} is never named in {HEADLESS_SETUP}"));
        }
        if !graphical.contains(res) {
            missing.push(format!("{res} is never named in {GRAPHICAL_SETUP}"));
        }
    }

    assert!(
        missing.is_empty(),
        "match-scoped resources missing from a setup path:\n  {}\n\n\
         Both modes must insert every match-scoped resource. A resource present \
         in only one mode does not fail loudly — the other mode falls back to \
         `Default` and silently runs different behaviour, which is exactly how \
         `CcPolicies` made the graphical client ignore the CC policy entirely.",
        missing.join("\n  ")
    );
}

/// The replay path is a third setup route and has its own way of going wrong: it
/// pre-inserts resources so a recorded seed reproduces. Anything resolved from
/// the match config there must be carried, or a replay silently runs defaults.
#[test]
fn the_replay_path_carries_every_config_resolved_axis() {
    let main = read("src/main.rs");
    for resolver in ["ai_profiles()", "cc_policies()", "interrupt_policies()"] {
        assert!(
            main.contains(resolver),
            "src/main.rs never calls `{resolver}`, so `--replay` cannot carry that \
             axis and will silently run its default. This is what made a `Priced` \
             replay play identically to an `Identity` one."
        );
    }
}
