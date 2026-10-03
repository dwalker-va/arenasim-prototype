//! Cooldown-site audit
//!
//! An ability's cooldown starts in ONE place, `Combatant::start_cooldown`,
//! because that is where a shared cooldown category (`cooldown_category` in
//! `abilities.ron` — Freezing Trap and Frost Trap share the `Trap` cooldown)
//! is applied. A cast path that wrote `ability_cooldowns` itself would start
//! the ability's own cooldown and silently skip the category's.
//!
//! The question, answered as SET EQUALITY over named members so a new member
//! fails loudly instead of passing under a `>=` floor: where in non-test `src/`
//! code is a key ADDED to `ability_cooldowns` (`insert`, `entry`, `extend`)?
//! Named by (file, enclosing fn). The tick that counts cooldowns down and
//! removes the expired ones only shortens them, and is not a member.
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source.

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::BTreeSet;

/// The places a key is added to `ability_cooldowns`, as (file, fn).
const COOLDOWN_STARTS: &[(&str, &str)] = &[
    // The one place a cooldown starts.
    (
        "src/states/play_match/components/combatant.rs",
        "start_cooldown",
    ),
    // Not a cooldown: a Druid leaving Travel Form to cast holds the re-shift
    // back for a global cooldown's length, as a cast would. Travel Form has no
    // cooldown category.
    (
        "src/states/play_match/class_ai/druid.rs",
        "decide_druid_action",
    ),
];

#[test]
fn cooldowns_start_only_in_start_cooldown() {
    let write = Regex::new(r"\.\s*ability_cooldowns\s*\.\s*(insert|entry|extend)\s*\(").unwrap();
    let fn_item = Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let mut found = BTreeSet::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for m in write.find_iter(&code) {
            let enclosing = fn_item
                .captures_iter(&code[..m.start()])
                .last()
                .map_or_else(|| "<module>".to_string(), |c| c[1].to_string());
            found.insert((file.rel_display(), enclosing));
        }
    }
    let expected: BTreeSet<(String, String)> = COOLDOWN_STARTS
        .iter()
        .map(|(file, f)| (file.to_string(), f.to_string()))
        .collect();
    let extra: Vec<_> = found.difference(&expected).collect();
    let missing: Vec<_> = expected.difference(&found).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "A cooldown is started outside `Combatant::start_cooldown`, which is the one place \
         a shared cooldown category applies. Call it instead; a write that is genuinely not \
         a cooldown start goes in COOLDOWN_STARTS with its reason.\n  \
         found but not declared: {extra:?}\n  declared but not found: {missing:?}"
    );
}
