//! Damage-site audit
//!
//! A combatant that reaches 0 health must leave a `[DEATH]` line in the log.
//! There is no single death funnel: each damage site marks its victim dead
//! (`Combatant::is_dead`, the dedup flag), cancels its cast and logs the death
//! with its killer itself. The Unstable Affliction backlash was the one site
//! that did not, so a Priest killed by dispelling it sat in the match report
//! at 0 HP with no `[DEATH]` line (AS-437). This audit keeps every site doing
//! it.
//!
//! Two questions, each answered as SET EQUALITY over named members, so a new
//! member fails loudly instead of passing under a `>=` floor:
//!
//! 1. **Where does health go down?** Every subtractive or zeroing write to
//!    `current_health` in non-test `src/` code, named by (file, enclosing fn).
//!    Damage lands through `combat_core::apply_damage_with_absorb`; the other
//!    members are declared with why they are not damage.
//! 2. **Who deals damage, and does each log its kills?** Every (file,
//!    enclosing fn) that calls `apply_damage_with_absorb` is named in
//!    DAMAGERS, and every one of them also calls `CombatLog::log_death`.
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source. It does not see a
//! health drop spelled without `-` or a zero literal (the clamp when a max
//! health buff ends, `current_health.min(max_health)`, which cannot reach 0),
//! nor one taken in two steps.

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::BTreeSet;

/// The subtractive or zeroing `current_health` writes allowed in `src/`, as
/// (file, fn).
const HEALTH_DROPS: &[(&str, &str)] = &[
    // The one damage site.
    (
        "src/states/play_match/combat_core/damage.rs",
        "apply_damage_with_absorb",
    ),
    // Not damage: a pet whose owner died despawns, and logs its own `[DEATH]`
    // line ("despawns (owner died)") as it does.
    (
        "src/states/play_match/combat_core/death.rs",
        "despawn_pets_of_dead_owners",
    ),
    // Not damage: Life Tap's health-for-mana conversion, floored at 1 health
    // so it never kills.
    (
        "src/states/play_match/class_ai/warlock.rs",
        "convert_health_to_mana",
    ),
];

/// Every system that deals damage through `apply_damage_with_absorb`, as
/// (file, fn). Each must also log the deaths it causes.
const DAMAGERS: &[(&str, &str)] = &[
    // Instant attacks and Frost Nova, queued by the class AI and landed as
    // the dispatch ends.
    ("src/states/play_match/combat_ai.rs", "decide_abilities"),
    // Damage over time ticks.
    ("src/states/play_match/auras.rs", "process_dot_ticks"),
    // Projectile hits (Frostbolt, Shadow Bolt, ...).
    (
        "src/states/play_match/projectiles.rs",
        "process_projectile_hits",
    ),
    // Completed casts with no projectile.
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_casting",
    ),
    // Drain Life's channel ticks.
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_channeling",
    ),
    // Melee swings and Auto Shot.
    (
        "src/states/play_match/combat_core/auto_attack.rs",
        "combat_auto_attack",
    ),
    // Holy Shock's damage.
    (
        "src/states/play_match/effects/holy_shock.rs",
        "process_holy_shock_damage",
    ),
    // What a hit sets off on other units: transferred shares, reflected
    // damage (AS-408).
    ("src/states/play_match/combat_core/follow_ups.rs", "land"),
    // The Unstable Affliction dispel backlash (AS-437).
    (
        "src/states/play_match/effects/backlash.rs",
        "process_backlash",
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

/// Every non-definition call of `name(` in non-test `src/`, as (file, fn).
fn callers(name: &str) -> BTreeSet<(String, String)> {
    let call = Regex::new(&format!(r"(\bfn\s+)?\b{name}\s*\(")).unwrap();
    sites(&call, |definition, _| definition.is_empty())
}

#[test]
fn health_goes_down_only_in_apply_damage_with_absorb() {
    // `current_health -=`, or `current_health =` whose right-hand side
    // subtracts or is a zero literal. The right-hand side runs to the next
    // `;`, which for a closure body is past the closure, so a `->` there is
    // not a subtraction.
    let write = Regex::new(r"\bcurrent_health\s*(-=|=[^=])").unwrap();
    let zero = Regex::new(r"^\s*0(\.0*)?\s*$").unwrap();
    let found = sites(&write, |op, rhs| {
        op == "-=" || rhs.replace("->", "").contains('-') || zero.is_match(rhs)
    });
    assert_same(
        "Health falls outside `combat_core::apply_damage_with_absorb`. Damage must land \
         through it, and its caller must log the death it causes; a drop that is \
         genuinely not damage goes in HEALTH_DROPS with its reason.",
        found,
        named(HEALTH_DROPS),
    );
}

#[test]
fn every_damager_is_declared() {
    assert_same(
        "The set of systems that call `apply_damage_with_absorb` changed. A new damage \
         site names its system in DAMAGERS, and logs the deaths it causes.",
        callers("apply_damage_with_absorb"),
        named(DAMAGERS),
    );
}

#[test]
fn every_damager_logs_the_deaths_it_causes() {
    let loggers = callers("log_death");
    let silent: Vec<_> = named(DAMAGERS).difference(&loggers).cloned().collect();
    assert!(
        silent.is_empty(),
        "These systems deal damage but never call `CombatLog::log_death`, so a unit \
         they kill sits at 0 HP with no [DEATH] line (AS-437). Mark the victim \
         `is_dead`, cancel its cast and log the death with its killer, as \
         `process_dot_ticks` does: {silent:?}"
    );
}
