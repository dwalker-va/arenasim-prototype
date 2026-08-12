//! Step 1 gate tests: `CcPolicy::Identity` must be byte-identical to the
//! behaviour that shipped before the CC value model existed, and
//! `CcPolicy::Priced` must actually change something.
//!
//! Both halves matter. A gate that changes nothing is a no-op dressed as a
//! feature; a gate that leaks into the default silently invalidates every
//! recorded balance baseline and every fixed-seed movement probe.
//!
//! See `design-docs/cc-value-model.md`.

use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig, MatchResult};

fn config(team1: &[&str], team2: &[&str], seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        random_seed: Some(seed),
        ..Default::default()
    }
}

fn with_policy(mut cfg: HeadlessMatchConfig, policy: &str) -> HeadlessMatchConfig {
    cfg.cc_policy = Some(policy.to_string());
    cfg
}

fn run(cfg: HeadlessMatchConfig) -> MatchResult {
    run_headless_match_with(cfg, true, None).expect("match should run")
}

/// Fingerprint of a match outcome, at the granularity a behaviour change would
/// disturb: who won, when, and every combatant's end state.
fn fingerprint(r: &MatchResult) -> String {
    let mut s = format!("{:?}|{:.4}|{:?}|", r.winner, r.match_time, r.end_reason);
    for c in r.team1_combatants.iter().chain(r.team2_combatants.iter()) {
        s.push_str(&format!(
            "{}:{:.3}:{:.3}:{:.3};",
            c.class_name, c.final_health, c.damage_dealt, c.damage_taken
        ));
    }
    s
}

/// The comps the CC investigation used, plus the case that motivated step 1 —
/// a Warlock whose own kill target IS the enemy healer, where the old identity
/// filter silently disabled the healer lockout entirely.
fn cases() -> Vec<(Vec<&'static str>, Vec<&'static str>, u64)> {
    let mut out = Vec::new();
    for seed in 1..=4u64 {
        out.push((vec!["Warlock", "Warrior"], vec!["Priest", "Mage"], seed));
        out.push((vec!["Warlock", "Mage"], vec!["Mage", "Priest"], seed));
        out.push((vec!["Warlock", "Priest", "Mage"], vec!["Warrior", "Priest", "Rogue"], seed));
    }
    out
}

#[test]
fn identity_is_the_default() {
    // A config that never mentions cc_policy must resolve to Identity, or the
    // default path silently changes for every existing caller.
    let cfg = config(&["Warlock", "Warrior"], &["Priest", "Mage"], 1);
    assert_eq!(
        cfg.cc_policies().unwrap(),
        arenasim::states::play_match::cc_policy::CcPolicies::default()
    );
}

#[test]
fn identity_matches_an_unspecified_config_exactly() {
    // Naming the default explicitly must not change a single bit — this is what
    // makes `Identity` runs comparable to every baseline recorded before the
    // policy axis existed.
    for (t1, t2, seed) in cases() {
        let implicit = fingerprint(&run(config(&t1, &t2, seed)));
        let explicit = fingerprint(&run(with_policy(config(&t1, &t2, seed), "Identity")));
        assert_eq!(
            implicit, explicit,
            "{t1:?} vs {t2:?} seed {seed}: naming Identity changed the outcome"
        );
    }
}

#[test]
fn priced_changes_behaviour_somewhere() {
    // The gate must not be a no-op. Not every seed need diverge — the priced
    // gate only fires when a Warlock is deciding whether to Fear a healer — but
    // across the survey at least one must, or nothing was actually wired up.
    let diverged = cases()
        .into_iter()
        .filter(|(t1, t2, seed)| {
            let identity = fingerprint(&run(with_policy(config(t1, t2, *seed), "Identity")));
            let priced = fingerprint(&run(with_policy(config(t1, t2, *seed), "Priced")));
            identity != priced
        })
        .count();

    assert!(
        diverged > 0,
        "CcPolicy::Priced produced identical outcomes on every case — the gate \
         is wired but inert, which is worse than not shipping it"
    );
}

#[test]
fn priced_is_deterministic_at_a_fixed_seed() {
    // The priced path ranks candidates by a float score, so it needs an explicit
    // deterministic tie-break. Two runs of one seed must agree exactly.
    for (t1, t2, seed) in cases().into_iter().take(4) {
        let a = fingerprint(&run(with_policy(config(&t1, &t2, seed), "Priced")));
        let b = fingerprint(&run(with_policy(config(&t1, &t2, seed), "Priced")));
        assert_eq!(a, b, "{t1:?} vs {t2:?} seed {seed}: Priced is not deterministic");
    }
}

#[test]
fn per_team_policies_are_independent() {
    // Head-to-head measurement depends on this: a uniform A/B compares two
    // internally consistent worlds and cannot answer "is the new model better".
    let mut cfg = config(&["Warlock", "Warrior"], &["Warlock", "Priest"], 1);
    cfg.team1_cc_policy = Some("Priced".to_string());
    cfg.team2_cc_policy = Some("Identity".to_string());
    let policies = cfg.cc_policies().unwrap();
    assert!(policies.is_head_to_head());
    assert!(policies.for_team(1).is_priced());
    assert!(!policies.for_team(2).is_priced());
}

#[test]
fn an_unknown_policy_is_rejected_at_load() {
    let mut cfg = config(&["Warlock"], &["Priest"], 1);
    cfg.cc_policy = Some("Nonsense".to_string());
    assert!(cfg.cc_policies().is_err());
}

// ---------------------------------------------------------------------------
// The two axes are independent
// ---------------------------------------------------------------------------
//
// Split per DECISION SITE, not per migration step: `cc_policy` chooses which
// enemy to crowd-control, `interrupt_policy` chooses which cast to interrupt.
// They measured with opposite signs (+10pt vs -5pt), so one flag would force
// shipping the loss to get the win. These tests pin that the split is real —
// that setting one axis cannot move the other.

fn with_interrupt(mut cfg: HeadlessMatchConfig, policy: &str) -> HeadlessMatchConfig {
    cfg.interrupt_policy = Some(policy.to_string());
    cfg
}

#[test]
fn interrupt_policy_defaults_to_identity_and_is_independent() {
    let cfg = config(&["Shaman", "Warrior"], &["Priest", "Warlock"], 1);
    assert_eq!(
        cfg.interrupt_policies().unwrap(),
        arenasim::states::play_match::cc_policy::InterruptPolicies::default()
    );
    // Turning the CC axis on must NOT turn the interrupt axis on.
    let cc_on = with_policy(config(&["Shaman", "Warrior"], &["Priest", "Warlock"], 1), "Priced");
    assert!(cc_on.cc_policies().unwrap().for_team(1).is_priced());
    assert!(!cc_on.interrupt_policies().unwrap().for_team(1).is_priced());
}

#[test]
fn naming_identity_on_the_interrupt_axis_changes_nothing() {
    for (t1, t2, seed) in cases() {
        let implicit = fingerprint(&run(config(&t1, &t2, seed)));
        let explicit = fingerprint(&run(with_interrupt(config(&t1, &t2, seed), "Identity")));
        assert_eq!(
            implicit, explicit,
            "{t1:?} vs {t2:?} seed {seed}: naming interrupt Identity changed the outcome"
        );
    }
}

#[test]
fn priced_interrupts_change_behaviour_somewhere() {
    // The Shaman is the only interrupter with a real choice — Pummel and Kick
    // are 2.5yd, so a melee can only ever reach the caster it is standing on.
    // Wind Shear is 30yd, which is why this comp is the one that can diverge.
    let diverged = (1..=6u64)
        .filter(|seed| {
            let t1 = vec!["Shaman", "Warrior"];
            let t2 = vec!["Priest", "Warlock"];
            let identity = fingerprint(&run(with_interrupt(config(&t1, &t2, *seed), "Identity")));
            let priced = fingerprint(&run(with_interrupt(config(&t1, &t2, *seed), "Priced")));
            identity != priced
        })
        .count();
    assert!(diverged > 0, "priced interrupt selection is wired but inert");
}

#[test]
fn the_cc_axis_alone_does_not_alter_interrupt_choices() {
    // A comp with NO class on the priced CC model, so the CC axis has nothing to
    // change: flipping it must leave the match bit-identical. If this fails, the
    // axes leak.
    //
    // The fixture was `Priest+Mage` until the Mage joined the priced model
    // (2026-08-09) — at which point flipping the CC axis legitimately changed
    // that comp and this test failed for the right reason. Kept pointed at a
    // roster with neither Warlock nor Mage; extend the exclusion as more classes
    // are migrated.
    for seed in 1..=6u64 {
        let t1 = vec!["Shaman", "Warrior"];
        let t2 = vec!["Priest", "Rogue"];
        let off = fingerprint(&run(with_policy(config(&t1, &t2, seed), "Identity")));
        let on = fingerprint(&run(with_policy(config(&t1, &t2, seed), "Priced")));
        assert_eq!(
            off, on,
            "seed {seed}: the CC axis moved a match with no Warlock in it — the \
             axes are not independent"
        );
    }
}
