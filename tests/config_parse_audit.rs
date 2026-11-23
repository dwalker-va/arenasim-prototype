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
//! the format crate itself? Named by (file, canonical construct). A parse is
//! any of:
//!
//! - a parse function: `from_str`, `from_bytes`, `from_reader`, `from_slice`,
//!   at the crate root or under `de` (`ron::de::from_str`);
//! - a deserializer built from text: `Deserializer`, `StreamDeserializer`,
//!   `ron::Options`;
//! - the `FromStr` of either crate's `Value`: `Value::from_str(t)` and
//!   `t.parse::<Value>()`. (`serde_json::from_value` is not a parse of text:
//!   it reads a `Value` that a parse already built.)
//!
//! however each is spelled in its file: fully qualified, through a crate alias
//! (`use serde_json as sj; sj::from_str`), through an imported module
//! (`use ron::de; de::from_str`), through an imported or renamed `Value`
//! (`use serde_json::Value as Json; Json::from_str`), or imported by name
//! (`use ron::from_str`, the import itself is the hit, since nothing imports a
//! parse function but to call it). A glob import of either crate
//! (`use serde_json::*`) is a hit too, because what it brings in cannot be
//! told apart. `use` trees are expanded, braces and `self` included.
//!
//! **Known limits.** The audit is lexical, like the others built on
//! `tests/common/source_audit.rs`: it reads comment-, string- and
//! test-module-blanked source, one file at a time. It cannot see:
//! - a parse reached through a macro;
//! - an alias or re-export made in ANOTHER module (`pub use serde_json as
//!   json;` in one file, `crate::a::json::from_str` in the next), since it
//!   resolves names per file;
//! - a `Value` parsed with its type left to inference (`let v: Value =
//!   t.parse()?`, `FromStr::from_str(t)`), since the parse never names it.

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

const CRATES: &[&str] = &["ron", "serde_json"];

/// The parse functions and text deserializers, by their last path segment.
const PARSE_ITEMS: &[&str] = &[
    "from_str",
    "from_bytes",
    "from_reader",
    "from_slice",
    "Deserializer",
    "StreamDeserializer",
    "Options",
];

/// One `use` binding: the full path it names, and the name it binds (`*` for
/// a glob).
struct Binding {
    path: Vec<String>,
    name: String,
}

/// Split at `sep` outside braces.
fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

fn path_segments(path: &str) -> Vec<String> {
    path.split("::")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Expand one `use` tree (`a::{b, c::d as e, self}`) under `prefix`.
fn expand_use(tree: &str, prefix: &[String], out: &mut Vec<Binding>) {
    for item in split_top_level(tree, ',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let (Some(open), Some(close)) = (item.find('{'), item.rfind('}')) {
            let mut path = prefix.to_vec();
            path.extend(path_segments(&item[..open]));
            expand_use(&item[open + 1..close], &path, out);
            continue;
        }
        let (path_text, alias) = match item.split_once(" as ") {
            Some((p, a)) => (p, Some(a.trim().to_string())),
            None => (item, None),
        };
        let mut path = prefix.to_vec();
        path.extend(path_segments(path_text));
        if path.last().map(String::as_str) == Some("self") {
            path.pop();
        }
        let Some(last) = path.last().cloned() else {
            continue;
        };
        let name = alias.unwrap_or(last);
        out.push(Binding { path, name });
    }
}

/// How each name a file binds resolves into the two format crates.
#[derive(Default)]
struct Names {
    /// A name that resolves to a crate or one of its modules, to that path
    /// (`ron`, `sj` -> `serde_json`, `de` -> `ron::de`).
    modules: BTreeMap<String, String>,
    /// A name bound to a crate's `Value`, to its canonical path.
    values: BTreeMap<String, String>,
}

/// One parse found: its canonical construct, the line, and how it was written.
struct Hit {
    construct: String,
    line: usize,
    written: String,
}

/// Every parse of RON or JSON text in one file's code (comments, strings and
/// test modules already blanked).
fn parses_in(code: &str) -> Vec<Hit> {
    let line_of = |at: usize| 1 + code[..at].matches('\n').count();
    let mut hits = Vec::new();
    let mut names = Names::default();
    for krate in CRATES {
        names.modules.insert(krate.to_string(), krate.to_string());
    }

    let extern_alias = Regex::new(r"\bextern\s+crate\s+(ron|serde_json)\s+as\s+(\w+)").unwrap();
    for c in extern_alias.captures_iter(code) {
        names.modules.insert(c[2].to_string(), c[1].to_string());
    }
    let use_stmt = Regex::new(r"\buse\s+([^;]*);").unwrap();
    for stmt in use_stmt.captures_iter(code) {
        let at = stmt.get(0).unwrap().start();
        let mut bindings = Vec::new();
        expand_use(stmt[1].trim_start_matches("::"), &[], &mut bindings);
        for Binding { path, name } in bindings {
            if !CRATES.contains(&path[0].as_str()) {
                continue;
            }
            let canonical = path.join("::");
            let last = path.last().unwrap().as_str();
            let written = format!("use {}", stmt[1].trim());
            // A glob brings in what cannot be told apart, and nothing
            // imports a parse function but to call it.
            if last == "*" || PARSE_ITEMS.contains(&last) {
                hits.push(Hit {
                    construct: canonical,
                    line: line_of(at),
                    written,
                });
            } else if last == "Value" {
                names.values.insert(name, canonical);
            } else if last.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                names.modules.insert(name, canonical);
            }
        }
    }

    // A parse item reached through a crate or module name, with an optional
    // `de` hop: `serde_json::from_str`, `sj::de::from_str`, `de::from_str`.
    for (name, path) in &names.modules {
        let item = Regex::new(&format!(
            r"\b{}\s*::\s*(?:(de)\s*::\s*)?({})\b",
            regex::escape(name),
            PARSE_ITEMS.join("|")
        ))
        .unwrap();
        for c in item.captures_iter(code) {
            let m = c.get(0).unwrap();
            // `serde::de::from_str` is not `ron::de::from_str`: a bound name
            // must stand at the start of its path.
            if code[..m.start()].trim_end().ends_with("::") {
                continue;
            }
            let construct = match c.get(1) {
                Some(_) => format!("{path}::de::{}", &c[2]),
                None => format!("{path}::{}", &c[2]),
            };
            hits.push(Hit {
                construct,
                line: line_of(m.start()),
                written: m.as_str().to_string(),
            });
        }
        // `serde_json::Value::from_str`, `ron::value::Value::from_str`.
        let value = Regex::new(&format!(
            r"\b{}\s*::\s*(?:value\s*::\s*)?Value\s*::\s*from_str\b",
            regex::escape(name)
        ))
        .unwrap();
        for m in value.find_iter(code) {
            if code[..m.start()].trim_end().ends_with("::") {
                continue;
            }
            hits.push(Hit {
                construct: format!("{path}::Value::from_str"),
                line: line_of(m.start()),
                written: m.as_str().to_string(),
            });
        }
    }
    // A bound `Value`: `Value::from_str(t)`, `Json::from_str(t)`.
    for (name, path) in &names.values {
        let value = Regex::new(&format!(r"\b{}\s*::\s*from_str\b", regex::escape(name))).unwrap();
        for m in value.find_iter(code) {
            if code[..m.start()].trim_end().ends_with("::") {
                continue;
            }
            hits.push(Hit {
                construct: format!("{path}::from_str"),
                line: line_of(m.start()),
                written: m.as_str().to_string(),
            });
        }
    }
    // `t.parse::<serde_json::Value>()`, `t.parse::<Json>()`.
    let parse = Regex::new(r"\bparse\s*::\s*<\s*((?:\w+\s*::\s*)*\w+)\s*>").unwrap();
    for c in parse.captures_iter(code) {
        let segments = path_segments(&c[1].trim_start_matches("::").replace(' ', ""));
        let resolved = match segments.as_slice() {
            [one] => names.values.get(one).cloned(),
            [head @ .., last] if last == "Value" => names
                .modules
                .get(&head[0])
                .map(|root| format!("{root}::Value")),
            _ => None,
        };
        if let Some(value) = resolved {
            let m = c.get(0).unwrap();
            hits.push(Hit {
                construct: format!("parse::<{value}>"),
                line: line_of(m.start()),
                written: m.as_str().to_string(),
            });
        }
    }
    hits
}

/// Every parse of RON or JSON text in non-test `src/` code, as (file,
/// construct) to where it is written.
fn parses() -> BTreeMap<(String, String), Vec<(usize, String)>> {
    let mut found: BTreeMap<(String, String), Vec<(usize, String)>> = BTreeMap::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for hit in parses_in(&code) {
            found
                .entry((file.rel_display(), hit.construct))
                .or_default()
                .push((hit.line, hit.written));
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
        .flat_map(|((file, construct), sites)| {
            sites
                .iter()
                .map(move |(line, written)| format!("{file}:{line}: {construct} (`{written}`)"))
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

/// The reader catches every spelling its doc names, and passes what is not a
/// parse. Each line of the fixture is one spelling; the set of constructs
/// found must be exactly the set named.
#[test]
fn the_audit_reads_every_spelling_of_a_parse() {
    let fixture = r#"
use ron::de;
use serde_json as sj;
use serde_json::Value as Json;
use ron::{from_bytes, value::Value as RonValue};
use serde_json::{self as js, de as jde};
use ron::*;
extern crate serde_json as ext;
use serde::de::Error;
fn a(t: &str) {
    let _ = ron::from_str::<u8>(t);
    let _ = ron::de::from_reader(t.as_bytes());
    let _ = serde_json::from_slice::<u8>(t.as_bytes());
    let _ = serde_json::StreamDeserializer::new(t);
    let _ = ron::Options::default();
    let _ = de::from_str::<u8>(t);
    let _ = sj::from_str::<u8>(t);
    let _ = js::from_reader(t.as_bytes());
    let _ = jde::from_str::<u8>(t);
    let _ = ext::from_str::<u8>(t);
    let _ = t.parse::<serde_json::Value>();
    let _ = t.parse::< sj :: Value >();
    let _ = t.parse::<Json>();
    let _ = ron::Value::from_str(t);
    let _ = Json::from_str(t);
    let _ = RonValue::from_str(t);
    // Not parses of text:
    let _ = serde_json::from_value::<u8>(serde_json::Value::Null);
    let _ = serde_json::to_string(&1);
    let _ = serde::de::value::Error::custom("x");
    let _ = t.parse::<u8>();
    let _ = other::de::from_str(t);
    let _ = other::Json::from_str(t);
}
"#;
    let found: BTreeSet<String> = parses_in(fixture)
        .into_iter()
        .map(|hit| format!("{}: {}", hit.line, hit.construct))
        .collect();
    let expected: BTreeSet<String> = [
        "5: ron::from_bytes",
        "7: ron::*",
        "11: ron::from_str",
        "12: ron::de::from_reader",
        "13: serde_json::from_slice",
        "14: serde_json::StreamDeserializer",
        "15: ron::Options",
        "16: ron::de::from_str",
        "17: serde_json::from_str",
        "18: serde_json::from_reader",
        "19: serde_json::de::from_str",
        "20: serde_json::from_str",
        "21: parse::<serde_json::Value>",
        "22: parse::<serde_json::Value>",
        "23: parse::<serde_json::Value>",
        "24: ron::Value::from_str",
        "25: serde_json::Value::from_str",
        "26: ron::value::Value::from_str",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let extra: Vec<_> = found.difference(&expected).collect();
    let missing: Vec<_> = expected.difference(&found).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "found but not expected: {extra:?}\n  expected but not found: {missing:?}"
    );
}
