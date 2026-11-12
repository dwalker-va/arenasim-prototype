//! Role audit (AS-413): a unit's part is asked of the UNIT, never its class.
//!
//! A per-match choice can change the part a unit plays — a Shadowform Priest
//! is a ranged damage dealer — so every place that asks "is this a healer?",
//! "is this a DPS?" reads `Combatant::role` / `CombatantInfo::role` (and their
//! `is_healer`). Asking the class instead compiles, reads naturally, and is
//! wrong for exactly one unit, which no baseline contains: a revert to it is
//! invisible to every byte-identity check. So this audit reads production
//! `src/` (test modules blanked) and fails on either spelling of the class
//! question:
//!
//! - `<class>.is_healer()` — `CharacterClass::is_healer` on anything whose
//!   receiver names a class (`c.class.is_healer()`, `class_of(e).is_healer()`);
//! - `Role::of(` — a class's DEFAULT role, which is only right where no unit
//!   exists yet.
//!
//! The exceptions are named, each with why, and checked by set equality, so a
//! new site fails until it is fixed or argued for here, and a removed one
//! fails until its row goes.

mod common;

use std::collections::BTreeSet;

use common::source_audit::load_sources;
use regex::Regex;

/// `(file, what)` for every class-keyed role question production code may ask.
const ALLOWED: &[(&str, &str)] = &[
    // `Role::of` itself: the class's default role is defined from the class.
    ("src/states/play_match/situation.rs", "class.is_healer()"),
    // `Combatant::role`: the default, for a unit whose choices change nothing.
    ("src/states/play_match/components/combatant.rs", "Role::of("),
    // Banter's kill-call flavour: its lineup carries classes only, no unit,
    // and a line naming a Shadowform Priest "the healer" is harmless.
    (
        "src/states/play_match/banter/resolver.rs",
        "class.is_healer()",
    ),
];

/// Every class-keyed role question in `code`, as `what` strings, one per
/// occurrence.
fn class_questions(code: &str) -> Vec<&'static str> {
    let healer = Regex::new(r"([A-Za-z_][A-Za-z_0-9]*)\s*(\([^()]*\))?\s*\.\s*is_healer\s*\(\s*\)")
        .expect("pattern");
    let role_of = Regex::new(r"\bRole\s*::\s*of\s*\(").expect("pattern");
    let mut found = Vec::new();
    for c in healer.captures_iter(code) {
        if c[1].contains("class") {
            found.push("class.is_healer()");
        }
    }
    found.extend(role_of.find_iter(code).map(|_| "Role::of("));
    found
}

#[test]
fn every_role_question_asks_the_unit() {
    let files = load_sources(&["src"]).expect("src reads");
    assert!(files.len() > 100, "the scan read {} files", files.len());
    let mut seen: BTreeSet<(String, &str)> = BTreeSet::new();
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file.rel_display();
        let code = file.code_without_test_modules();
        for what in class_questions(&code) {
            if !seen.insert((rel.clone(), what)) {
                offenders.push(format!("{rel}: a second {what}"));
            }
        }
    }
    let allowed: BTreeSet<(String, &str)> =
        ALLOWED.iter().map(|(f, w)| (f.to_string(), *w)).collect();
    for site in seen.difference(&allowed) {
        offenders.push(format!(
            "{}: {} — ask the unit's role (`.role` / `.is_healer()` on the unit) instead",
            site.0, site.1
        ));
    }
    for site in allowed.difference(&seen) {
        offenders.push(format!(
            "{}: the allowed {} is gone — drop its row",
            site.0, site.1
        ));
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// The predicate, on planted offenders: each spelling of the class question is
/// caught, and a unit's own question is not.
#[test]
fn the_audit_sees_each_spelling_of_the_class_question() {
    assert_eq!(
        class_questions("let h = info.class.is_healer();"),
        vec!["class.is_healer()"]
    );
    assert_eq!(
        class_questions("if class_of(d).is_healer() {}"),
        vec!["class.is_healer()"]
    );
    assert_eq!(
        class_questions("role: Role::of(combatant.class),"),
        vec!["Role::of("]
    );
    assert!(class_questions("if info.is_healer() && c.is_healer() {}").is_empty());
}
