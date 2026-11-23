//! Config-parse audit (AS-504)
//!
//! RON and JSON keep the last value of a repeated map key and say nothing.
//! `src/config_parse.rs` is the one parser that refuses the repeat, so a
//! config parsed anywhere else is a config whose duplicates pass silently.
//! This audit is the stop site: it fails on any non-test parse of RON or JSON
//! text under `src/` that does not go through `config_parse`, naming its file
//! and line.
//!
//! The question, answered as SET EQUALITY over named members so a new parse
//! fails loudly: where in non-test `src/` code is RON or JSON text parsed by
//! the format crate itself (`ron::from_str`, `ron::de::from_*`,
//! `ron::Options`, a format `Deserializer`, `serde_json::from_str` / `_slice`
//! / `_reader`, or a `use` that imports one of those to call it bare)? Named
//! by (file, construct). `serde_json::from_value` is not a parse of text: it
//! reads a `Value` that a parse already built (and, under `config_parse`,
//! already checked).
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source, so it cannot see a
//! parse reached through a macro or a re-export under another name.

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

/// The parses allowed outside `config_parse`'s entry points, as (file,
/// construct), each with its reason.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "src/config_parse.rs",
        "ron::de::Deserializer",
        "the strict parser itself: wraps RON's deserializer in the duplicate check",
    ),
    (
        "src/config_parse.rs",
        "serde_json::Deserializer",
        "the strict parser itself: wraps serde_json's deserializer in the duplicate check",
    ),
    (
        "src/settings.rs",
        "ron::from_str",
        "the player's own settings.ron: a file the player edits, which must keep loading \
         (a parse error there falls back to defaults rather than refusing to start)",
    ),
];

/// Every parse of RON or JSON text in non-test `src/` code, as (file,
/// construct) to the lines it appears on.
fn parses() -> BTreeMap<(String, String), Vec<usize>> {
    let qualified = Regex::new(
        r"\b(ron|serde_json)\s*::\s*(?:(de)\s*::\s*)?(from_str|from_bytes|from_reader|from_slice|Deserializer|Options)\b",
    )
    .unwrap();
    let imported = Regex::new(
        r"\buse\s+(ron|serde_json)\s*::[^;]*\b(from_str|from_bytes|from_reader|from_slice|Deserializer|Options)\b",
    )
    .unwrap();
    let mut found: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        let line_of = |at: usize| 1 + code[..at].matches('\n').count();
        for c in qualified.captures_iter(&code) {
            let construct = match c.get(2) {
                Some(_) => format!("{}::de::{}", &c[1], &c[3]),
                None => format!("{}::{}", &c[1], &c[3]),
            };
            found
                .entry((file.rel_display(), construct))
                .or_default()
                .push(line_of(c.get(0).unwrap().start()));
        }
        for c in imported.captures_iter(&code) {
            found
                .entry((file.rel_display(), format!("use {}::{}", &c[1], &c[2])))
                .or_default()
                .push(line_of(c.get(0).unwrap().start()));
        }
    }
    found
}

#[test]
fn configs_are_parsed_only_through_config_parse() {
    let found = parses();
    let expected: BTreeSet<(String, String)> = ALLOWED
        .iter()
        .map(|(file, construct, _)| (file.to_string(), construct.to_string()))
        .collect();
    let extra: Vec<String> = found
        .iter()
        .filter(|(key, _)| !expected.contains(*key))
        .flat_map(|((file, construct), lines)| {
            lines
                .iter()
                .map(move |line| format!("{file}:{line}: {construct}"))
        })
        .collect();
    let missing: Vec<_> = expected
        .iter()
        .filter(|key| !found.contains_key(*key))
        .collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "RON or JSON text is parsed outside `arenasim::config_parse`, so a map that names a \
         key twice would silently keep the last. Parse it with `config_parse::ron_from_str` \
         or `config_parse::json_from_str`; a parse that genuinely must not refuse one goes \
         in ALLOWED with its reason.\n  found but not allowed:\n    {}\n  allowed but not \
         found: {missing:?}",
        extra.join("\n    ")
    );
}
