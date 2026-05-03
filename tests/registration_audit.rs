//! Registration audit
//!
//! Walks `src/states/play_match/**/*.rs` for `pub fn` items whose signatures
//! contain Bevy SystemParam types, then asserts each is registered in either
//! `add_core_combat_systems` (in `src/states/play_match/systems.rs`),
//! `StatesPlugin::build()` (in `src/states/mod.rs`), or the explicit ALLOWLIST
//! below.
//!
//! Closes the historical silent-failure bug class (Divine Shield, Holy Shock,
//! Dispels were each registered in only one of the two paths and silently
//! failed in the other mode).
//!
//! See `docs/plans/2026-04-26-001-refactor-system-registration-architecture-plan.md`
//! for context. Convention is documented in `CLAUDE.md` under "Adding a New
//! Combat System".

use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const PLAY_MATCH_REL: &str = "src/states/play_match";
const SYSTEMS_FILE_REL: &str = "src/states/play_match/systems.rs";
const STATES_MOD_FILE_REL: &str = "src/states/mod.rs";

/// `pub fn` items that match the SystemParam predicate but are intentionally
/// NOT registered as Bevy systems. Each entry must include a one-line
/// justification naming where the function is invoked instead.
///
/// Add new entries here when a helper takes a SystemParam type by value (e.g.
/// `Commands`) but is called manually from within a system body. Most helpers
/// in this codebase take references (`&mut Commands`) and don't reach this
/// list.
const ALLOWLIST: &[(&str, &str)] = &[
    // CombatSnapshot::build takes Bevy queries by reference (not by value) to
    // construct a per-frame view inside `decide_abilities`. Not a Bevy system.
    ("build", "CombatSnapshot::build helper called from decide_abilities"),
];

#[test]
fn audit_combat_system_registration() {
    let core_registered = extract_registered_in_function(SYSTEMS_FILE_REL, "add_core_combat_systems")
        .expect("failed to extract core-registered set from systems.rs");
    let graphical_registered = extract_registered_in_states_plugin_build()
        .expect("failed to extract graphical-registered set from states/mod.rs");
    let candidates = walk_play_match_fns()
        .expect("failed to walk play_match for candidate pub fn items");

    let allowlist: BTreeSet<&str> = ALLOWLIST.iter().map(|(name, _)| *name).collect();

    let mut violations: Vec<(String, PathBuf, usize)> = Vec::new();
    for (name, path, line) in &candidates {
        if core_registered.contains(name.as_str())
            || graphical_registered.contains(name.as_str())
            || allowlist.contains(name.as_str())
        {
            continue;
        }
        violations.push((name.clone(), path.clone(), *line));
    }

    if !violations.is_empty() {
        let mut msg = String::new();
        msg.push_str("\n\nFound Bevy system function(s) not registered in any known location:\n\n");
        for (name, path, line) in &violations {
            let display_path = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(path)
                .display();
            msg.push_str(&format!("  {} at {}:{}\n", name, display_path, line));
        }
        msg.push_str("\nFor each function listed above, do ONE of:\n");
        msg.push_str("  - Register it via add_core_combat_systems in src/states/play_match/systems.rs\n");
        msg.push_str("    (for systems that run in BOTH headless and graphical modes)\n");
        msg.push_str("  - Register it via StatesPlugin::build in src/states/mod.rs\n");
        msg.push_str("    (for systems that run in graphical mode only)\n");
        msg.push_str("  - Add it to ALLOWLIST in tests/registration_audit.rs with a one-line\n");
        msg.push_str("    justification (for helpers that take SystemParam types by value but\n");
        msg.push_str("    are not themselves registered as systems)\n\n");
        msg.push_str("See docs/plans/2026-04-26-001-refactor-system-registration-architecture-plan.md\n");
        msg.push_str("for the rationale.\n");
        panic!("{}", msg);
    }
}

// ---- registered-set extraction ----

fn extract_registered_in_function(rel_path: &str, fn_name: &str) -> std::io::Result<BTreeSet<String>> {
    let text = fs::read_to_string(repo_path(rel_path))?;
    let body = find_fn_body(&text, fn_name).unwrap_or_default();
    Ok(collect_registered_identifiers(&body))
}

fn extract_registered_in_states_plugin_build() -> std::io::Result<BTreeSet<String>> {
    let text = fs::read_to_string(repo_path(STATES_MOD_FILE_REL))?;
    let impl_body = find_impl_body(&text, "Plugin", "StatesPlugin").unwrap_or_default();
    let build_body = find_fn_body(&impl_body, "build").unwrap_or_default();
    Ok(collect_registered_identifiers(&build_body))
}

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Find body of `fn FN_NAME(...) [-> ...] { ... }` (without surrounding braces).
/// Handles generics, multi-line parameter lists, and return types.
fn find_fn_body(text: &str, fn_name: &str) -> Option<String> {
    let pattern = format!(r"\bfn\s+{}\b", regex::escape(fn_name));
    let re = Regex::new(&pattern).ok()?;
    let m = re.find(text)?;
    let bytes = text.as_bytes();
    let mut i = m.end();

    if i < bytes.len() && bytes[i] == b'<' {
        let mut depth = 1;
        i += 1;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'<' => depth += 1,
                b'>' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
    }
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'(' {
        let mut depth = 1;
        i += 1;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
    }
    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let body_start = i + 1;
    let mut depth = 1;
    let mut j = body_start;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        j += 1;
    }
    Some(text[body_start..j.saturating_sub(1)].to_string())
}

/// Find body of `impl TRAIT for TYPE { ... }`.
fn find_impl_body(text: &str, trait_name: &str, type_name: &str) -> Option<String> {
    let pattern = format!(
        r"\bimpl\s+{}\s+for\s+{}\b",
        regex::escape(trait_name),
        regex::escape(type_name)
    );
    let re = Regex::new(&pattern).ok()?;
    let m = re.find(text)?;
    let bytes = text.as_bytes();
    let mut i = m.end();
    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let body_start = i + 1;
    let mut depth = 1;
    let mut j = body_start;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        j += 1;
    }
    Some(text[body_start..j.saturating_sub(1)].to_string())
}

const SCHEDULE_AND_KEYWORDS: &[&str] = &[
    "chain", "in_set", "after", "before", "run_if", "in_state",
    "apply_deferred", "OnEnter", "OnExit", "Update", "FixedUpdate",
    "Startup", "PreUpdate", "PostUpdate", "PreStartup", "PostStartup",
    "GameState", "Schedule", "self", "app", "let", "if", "else", "match",
    "for", "while", "loop", "return", "fn", "use", "mut", "ref", "true",
    "false", "Some", "None", "Ok", "Err",
];

/// Within a function body, scan every `.add_systems(...)` call and extract
/// every snake_case identifier registered. Handles three patterns:
///   1. `.add_systems(SCHEDULE, single_system)` — one system
///   2. `.add_systems(SCHEDULE, (a, b, c).chain())` — tuple of systems
///   3. `.add_systems(SCHEDULE, (a, b.after(x), c).chain())` — chained methods
/// The line-based extraction is permissive (catches identifiers from anywhere
/// inside the call), filtered by an exclude list of Rust idioms.
fn collect_registered_identifiers(body: &str) -> BTreeSet<String> {
    let mut registered: BTreeSet<String> = BTreeSet::new();
    let bytes = body.as_bytes();

    let add_systems_re = Regex::new(r"\.add_systems\s*\(").unwrap();
    let line_re = Regex::new(r"(?m)^\s*(?:[\w:]+::)?([a-z_][a-z0-9_]*)\s*[,.\(]").unwrap();
    // Single-system shortcut: SCHEDULE, IDENT (e.g. OnEnter(...), play_match::setup_play_match)
    // Operates on the captured block (without the leading .add_systems prefix).
    let single_re = Regex::new(
        r"(?m)^\s*[\w:]+(?:\([^)]*\))?\s*,\s*(?:[\w:]+::)?([a-z_][a-z0-9_]*)",
    )
    .unwrap();

    let mut i = 0usize;
    while let Some(m) = add_systems_re.find(&body[i..]) {
        let start = i + m.end() - 1; // at `(`
        let mut depth = 1;
        let mut j = start + 1;
        while j < bytes.len() && depth > 0 {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        if j > bytes.len() {
            break;
        }
        let block = &body[start + 1..j.saturating_sub(1)];

        for cap in single_re.captures_iter(block) {
            let token = cap[1].split("::").last().unwrap_or("").to_string();
            if !SCHEDULE_AND_KEYWORDS.contains(&token.as_str())
                && !token.is_empty()
                && token.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
            {
                registered.insert(token);
            }
        }
        for cap in line_re.captures_iter(block) {
            let token = cap[1].to_string();
            if SCHEDULE_AND_KEYWORDS.contains(&token.as_str()) {
                continue;
            }
            registered.insert(token);
        }

        i = j;
    }
    registered
}

// ---- candidate scan ----

/// SystemParam tokens (Bevy 0.15) that mark a function as a Bevy system.
/// Extend this list when a new SystemParam shape is adopted (e.g. a Bevy
/// upgrade introduces a new param type).
const SYSTEM_PARAM_TOKENS: &[&str] = &[
    r"\bQuery<",
    r"\bRes<",
    r"\bResMut<",
    r"\bCommands\b",
    r"\bLocal<",
    r"\bEventReader<",
    r"\bEventWriter<",
    r"\bTime\b",
    r"\bTime<",
    r"\bAssets<",
    r"\bAssetServer\b",
    r"\bEguiContexts\b",
    r"\bGizmos\b",
    r"\bTrigger<",
    r"\bIn<",
    r"\bSingle<",
    r"\bPopulated<",
    r"\bNonSend<",
    r"\bNonSendMut<",
    r"\bRemovedComponents<",
    r"\bParamSet<",
];

/// Walk play_match for `pub fn` items with system signatures.
/// Returns Vec of (name, file_path, line_number).
fn walk_play_match_fns() -> std::io::Result<Vec<(String, PathBuf, usize)>> {
    let mut out = Vec::new();
    let pub_fn_re = Regex::new(r"(?m)^[ \t]*pub\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(").unwrap();
    let helper_ref_re = Regex::new(r"&mut\s+(?:Commands|Assets<)").unwrap();
    let sys_param_re = Regex::new(&SYSTEM_PARAM_TOKENS.join("|")).unwrap();

    let dir = repo_path(PLAY_MATCH_REL);
    walk_dir(&dir, &mut |path| {
        let text = fs::read_to_string(path)?;
        let stripped = strip_test_blocks(&text);
        for m in pub_fn_re.captures_iter(&stripped) {
            let name = m[1].to_string();
            let m0 = m.get(0).unwrap();
            let bytes = stripped.as_bytes();
            let start_paren = m0.end() - 1;
            let mut depth = 0;
            let mut i = start_paren;
            while i < bytes.len() {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if i >= bytes.len() {
                continue;
            }
            let params = &stripped[start_paren + 1..i];
            let params_clean = helper_ref_re.replace_all(params, "");
            if !sys_param_re.is_match(&params_clean) {
                continue;
            }
            let line_no = stripped[..m0.start()].matches('\n').count() + 1;
            out.push((name, path.to_path_buf(), line_no));
        }
        Ok(())
    })?;
    Ok(out)
}

fn walk_dir<F: FnMut(&Path) -> std::io::Result<()>>(
    dir: &Path,
    visitor: &mut F,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, visitor)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            visitor(&path)?;
        }
    }
    Ok(())
}

/// Remove `#[cfg(test)] mod tests { ... }` blocks from text. Test functions
/// frequently take SystemParam types in test harness signatures; excluding
/// them prevents false positives.
fn strip_test_blocks(text: &str) -> String {
    let mod_tests_re = Regex::new(r"#\[cfg\(test\)\][\s\S]*?\bmod\s+tests\s*\{").unwrap();
    let mut out = String::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        let slice = &text[i..];
        if let Some(m) = mod_tests_re.find(slice) {
            out.push_str(&text[i..i + m.start()]);
            let mut depth = 1;
            let mut j = i + m.end();
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            i = j;
        } else {
            out.push_str(&text[i..]);
            break;
        }
    }
    out
}
