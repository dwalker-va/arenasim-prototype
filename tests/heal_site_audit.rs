//! Heal-site audit
//!
//! Every effect that raises a combatant's health must land through
//! `combat_core::apply_healing`, the one function that applies healing
//! reduction (Mortal Strike), the Mortal Wounds tell and arena dampening. Heal
//! over time ticks once skipped the reduction because each heal site carried
//! its own copy of those rules and one copy was missing a step; this audit is
//! what keeps there being one copy.
//!
//! Two questions, each answered as SET EQUALITY over named members, so a new
//! member fails loudly instead of passing under a `>=` floor:
//!
//! 1. **Where does health go up?** Every additive write to `current_health` in
//!    non-test `src/` code, named by (file, enclosing fn). The only members are
//!    `apply_healing` itself and one declared non-heal: a max-health buff that
//!    raises current health with the maximum. A raw `current_health +=` or
//!    `current_health = (current_health + x)...` anywhere else is a heal site
//!    that bypasses the rules.
//! 2. **Who heals?** Every (file, enclosing fn) that calls `apply_healing`. A
//!    heal system that stops calling it (and so stops healing, or heals some
//!    other way) changes this set.
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source. It does not see a
//! health raise spelled without `+` (`current_health = max_health`, the spawn
//! and reset sites), which is why the reset sites are not heals to it.

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::BTreeSet;

/// The additive `current_health` writes allowed in `src/`, as (file, fn).
const HEALTH_RAISES: &[(&str, &str)] = &[
    // The one heal site.
    (
        "src/states/play_match/combat_core/healing.rs",
        "apply_healing",
    ),
    // Not a heal: a MaxHealthIncrease buff (Power Word: Fortitude) raises
    // current health WITH the maximum as it lands, so the buff does not read
    // as missing health. Healing reduction must not cut it.
    ("src/states/play_match/auras.rs", "apply_pending_auras"),
];

/// Every system that lands healing, as (file, fn).
const HEALERS: &[(&str, &str)] = &[
    // Direct heals (Flash Heal, Healing Touch, Swiftmend, ...).
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_casting",
    ),
    // Drain Life's heal to its channeler.
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_channeling",
    ),
    // Heal over time ticks (Renew, Rejuvenation, Lifebloom, Healing Stream).
    ("src/states/play_match/auras.rs", "process_hot_ticks"),
    // The Lifebloom bloom.
    ("src/states/play_match/effects/bloom.rs", "process_blooms"),
    // Holy Shock's heal.
    (
        "src/states/play_match/effects/holy_shock.rs",
        "process_holy_shock_heals",
    ),
    // Death Coil's lifesteal.
    (
        "src/states/play_match/projectiles.rs",
        "process_projectile_hits",
    ),
    // Devour Magic's heal to the Felhunter.
    (
        "src/states/play_match/effects/dispels.rs",
        "process_dispels",
    ),
];

/// Every (file, enclosing fn) in non-test `src/` code where `pattern` matches
/// and `keep(first capture group, rest of the statement)` holds.
fn sites(pattern: &Regex, keep: impl Fn(&str, &str) -> bool) -> BTreeSet<(String, String)> {
    let fn_item = Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let mut found = BTreeSet::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for m in pattern.captures_iter(&code) {
            let whole = m.get(0).unwrap();
            let statement_end = code[whole.end()..]
                .find(';')
                .map_or(code.len(), |i| whole.end() + i);
            let group = m.get(1).map_or("", |g| g.as_str());
            if !keep(group, &code[whole.end()..statement_end]) {
                continue;
            }
            let enclosing = fn_item
                .captures_iter(&code[..whole.start()])
                .last()
                .map_or_else(|| "<module>".to_string(), |c| c[1].to_string());
            found.insert((file.rel_display(), enclosing));
        }
    }
    found
}

fn named(members: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    members
        .iter()
        .map(|(file, f)| (file.to_string(), f.to_string()))
        .collect()
}

fn assert_same(
    label: &str,
    found: BTreeSet<(String, String)>,
    expected: BTreeSet<(String, String)>,
) {
    let extra: Vec<_> = found.difference(&expected).collect();
    let missing: Vec<_> = expected.difference(&found).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "{label}\n  found but not declared: {extra:?}\n  declared but not found: {missing:?}"
    );
}

#[test]
fn health_goes_up_only_in_apply_healing() {
    // `current_health +=`, or `current_health =` whose right-hand side adds.
    let write = Regex::new(r"\bcurrent_health\s*(\+=|=[^=])").unwrap();
    let found = sites(&write, |op, rhs| op == "+=" || rhs.contains('+'));
    assert_same(
        "Health rises outside `combat_core::apply_healing`. A heal must land through it, \
         so healing reduction, the Mortal Wounds tell and arena dampening apply; a raise \
         that is genuinely not a heal goes in HEALTH_RAISES with its reason.",
        found,
        named(HEALTH_RAISES),
    );
}

#[test]
fn every_healer_lands_through_apply_healing() {
    // A call, not the definition's own `fn apply_healing(`.
    let call = Regex::new(r"(\bfn\s+)?\bapply_healing\s*\(").unwrap();
    let found = sites(&call, |definition, _| definition.is_empty());
    assert_same(
        "The set of systems that call `apply_healing` changed. A new heal names its \
         system in HEALERS; a system that stopped calling it has a heal that no longer \
         lands through the shared rules.",
        found,
        named(HEALERS),
    );
}
