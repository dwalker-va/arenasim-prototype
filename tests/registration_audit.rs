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
//! The reading — file discovery, comment blanking, signature scanning,
//! `.add_systems` block extraction, and the type-alias and
//! `SystemParam`-bundle expansion that keeps a parameter visible however it is
//! spelled — is shared with the repo's other lexical audits in
//! `tests/common/source_audit.rs`. What counts as a system, and what counts as
//! a registration, stay here.
//!
//! See `docs/plans/2026-04-26-001-refactor-system-registration-architecture-plan.md`
//! for context. Convention is documented in `CLAUDE.md` under "Adding a New
//! Combat System".

mod common;

use common::source_audit::{
    add_systems_blocks, expanded_param_types, find_fn_body, load_sources, pub_fn_signatures,
    rel_display, repo_path, states_plugin_build, SourceFile, SystemParamBundles, TypeAliases,
};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::PathBuf;

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
    (
        "build",
        "CombatSnapshot::build helper called from decide_abilities",
    ),
];

#[test]
fn audit_combat_system_registration() {
    let core_registered =
        extract_registered_in_function(SYSTEMS_FILE_REL, "add_core_combat_systems")
            .expect("failed to extract core-registered set from systems.rs");
    let graphical_registered = extract_registered_in_states_plugin_build()
        .expect("failed to extract graphical-registered set from states/mod.rs");
    let candidates =
        walk_play_match_fns().expect("failed to walk play_match for candidate pub fn items");

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
            msg.push_str(&format!("  {} at {}:{}\n", name, rel_display(path), line));
        }
        msg.push_str("\nFor each function listed above, do ONE of:\n");
        msg.push_str(
            "  - Register it via add_core_combat_systems in src/states/play_match/systems.rs\n",
        );
        msg.push_str("    (for systems that run in BOTH headless and graphical modes)\n");
        msg.push_str("  - Register it via StatesPlugin::build in src/states/mod.rs\n");
        msg.push_str("    (for systems that run in graphical mode only)\n");
        msg.push_str("  - Add it to ALLOWLIST in tests/registration_audit.rs with a one-line\n");
        msg.push_str("    justification (for helpers that take SystemParam types by value but\n");
        msg.push_str("    are not themselves registered as systems)\n\n");
        msg.push_str(
            "See docs/plans/2026-04-26-001-refactor-system-registration-architecture-plan.md\n",
        );
        msg.push_str("for the rationale.\n");
        panic!("{}", msg);
    }
}

// ---- registered-set extraction ----

fn extract_registered_in_function(
    rel_path: &str,
    fn_name: &str,
) -> std::io::Result<BTreeSet<String>> {
    let file = read_one(rel_path)?;
    let body = find_fn_body(&file.code, fn_name).unwrap_or_default();
    Ok(collect_registered_identifiers(&body, &file.code))
}

fn extract_registered_in_states_plugin_build() -> std::io::Result<BTreeSet<String>> {
    let file = read_one(STATES_MOD_FILE_REL)?;
    let build = states_plugin_build(&file.code).unwrap_or_default();
    Ok(collect_registered_identifiers(&build, &file.code))
}

fn read_one(rel_path: &str) -> std::io::Result<SourceFile> {
    SourceFile::read(&repo_path(rel_path))
}

const SCHEDULE_AND_KEYWORDS: &[&str] = &[
    "chain",
    "in_set",
    "after",
    "before",
    "run_if",
    "in_state",
    "apply_deferred",
    "OnEnter",
    "OnExit",
    "Update",
    "FixedUpdate",
    "Startup",
    "PreUpdate",
    "PostUpdate",
    "PreStartup",
    "PostStartup",
    "GameState",
    "Schedule",
    "self",
    "app",
    "let",
    "if",
    "else",
    "match",
    "for",
    "while",
    "loop",
    "return",
    "fn",
    "use",
    "mut",
    "ref",
    "true",
    "false",
    "Some",
    "None",
    "Ok",
    "Err",
];

/// Within a function body, scan every `.add_systems(...)` call and extract
/// every snake_case identifier registered. Handles three patterns:
///   1. `.add_systems(SCHEDULE, single_system)` — one system
///   2. `.add_systems(SCHEDULE, (a, b, c).chain())` — tuple of systems
///   3. `.add_systems(SCHEDULE, (a, b.after(x), c).chain())` — chained methods
///
/// The line-based extraction is permissive (catches identifiers from anywhere
/// inside the call), filtered by an exclude list of Rust idioms.
fn collect_registered_identifiers(body: &str, file: &str) -> BTreeSet<String> {
    let mut registered: BTreeSet<String> = BTreeSet::new();

    let line_re = Regex::new(r"(?m)^\s*(?:[\w:]+::)?([a-z_][a-z0-9_]*)\s*[,.\(]").unwrap();
    // Single-system shortcut: SCHEDULE, IDENT (e.g. OnEnter(...), play_match::setup_play_match)
    // Operates on the captured block (without the leading .add_systems prefix).
    let single_re =
        Regex::new(r"(?m)^\s*[\w:]+(?:\([^)]*\))?\s*,\s*(?:[\w:]+::)?([a-z_][a-z0-9_]*)").unwrap();

    for block in add_systems_blocks(body, file) {
        for cap in single_re.captures_iter(&block.text) {
            let token = cap[1].split("::").last().unwrap_or("").to_string();
            if !SCHEDULE_AND_KEYWORDS.contains(&token.as_str())
                && !token.is_empty()
                && token
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_lowercase())
                    .unwrap_or(false)
            {
                registered.insert(token);
            }
        }
        for cap in line_re.captures_iter(&block.text) {
            let token = cap[1].to_string();
            if SCHEDULE_AND_KEYWORDS.contains(&token.as_str()) {
                continue;
            }
            registered.insert(token);
        }
    }
    registered
}

// ---- candidate scan ----

/// SystemParam tokens (Bevy 0.16) that mark a function as a Bevy system.
/// Extend this list when a new SystemParam shape is adopted (e.g. a Bevy
/// upgrade introduces a new param type).
///
/// Written whitespace-tolerantly (`\s*` before every `<`) for the same reason
/// the shared reader is: a spelling rustfmt would never produce must still be
/// read, or the audit's coverage depends on the formatter.
const SYSTEM_PARAM_TOKENS: &[&str] = &[
    r"\bQuery\s*<",
    r"\bRes\s*<",
    r"\bResMut\s*<",
    r"\bCommands\b",
    r"\bLocal\s*<",
    r"\bEventReader\s*<",
    r"\bEventWriter\s*<",
    r"\bTime\b",
    r"\bTime\s*<",
    r"\bAssets\s*<",
    r"\bAssetServer\b",
    r"\bEguiContexts\b",
    r"\bGizmos\b",
    r"\bTrigger\s*<",
    r"\bIn\s*<",
    r"\bSingle\s*<",
    r"\bPopulated\s*<",
    r"\bNonSend\s*<",
    r"\bNonSendMut\s*<",
    r"\bRemovedComponents\s*<",
    r"\bParamSet\s*<",
];

/// Walk play_match for `pub fn` items with system signatures.
/// Returns Vec of (name, file_path, line_number).
fn walk_play_match_fns() -> std::io::Result<Vec<(String, PathBuf, usize)>> {
    let sys_param_re = Regex::new(&SYSTEM_PARAM_TOKENS.join("|")).unwrap();

    let files = load_sources(&[PLAY_MATCH_REL])?;
    let bundles = SystemParamBundles::scan(&files);

    let mut out = Vec::new();
    for file in &files {
        // Test modules take SystemParam types in harness signatures; excluding
        // them prevents false positives.
        let code = file.code_without_test_modules();
        let aliases = TypeAliases::from_source(&code);
        for sig in pub_fn_signatures(&code) {
            // Parameters taken by reference are helper arguments, not system
            // parameters; aliases and SystemParam bundles are expanded so a
            // system is recognised however its parameters are spelled.
            let params = expanded_param_types(&sig.params, &aliases, &bundles).join(", ");
            if !sys_param_re.is_match(&params) {
                continue;
            }
            out.push((sig.name, file.path.clone(), sig.line));
        }
    }
    Ok(out)
}

/// The detector itself, pinned: a signature spelled the way rustfmt would never
/// write it — extra whitespace, a type alias, a `SystemParam` bundle — is still
/// a system. Each of these was a real miss in one of the repo's three lexical
/// audits before the reading moved into `tests/common/source_audit.rs`.
#[test]
fn the_param_detector_reads_hostile_spellings() {
    let sys_param_re = Regex::new(&SYSTEM_PARAM_TOKENS.join("|")).unwrap();
    let bundles = SystemParamBundles::scan(&[SourceFile {
        path: PathBuf::from("synthetic.rs"),
        raw: String::new(),
        code: r#"
            #[derive(SystemParam)]
            pub struct Extras<'w> {
                clock: Res<'w, Time>,
            }
        "#
        .to_string(),
    }]);
    let aliases = TypeAliases::from_source("type Clock<'w> = Res<'w, Time>;\nuse x::Query as Q;");

    for (label, params) in [
        ("plain", "time: Res<Time>"),
        ("spaced", "time: Res < Time >"),
        ("lifetime", "time: Res<'w, Time>"),
        ("type alias", "clock: Clock"),
        ("use rename", "q: Q<'w, 's, &'static Transform>"),
        ("SystemParam bundle", "extras: Extras"),
        ("borrowing query", "q: Query<&mut Transform>"),
    ] {
        let expanded = expanded_param_types(params, &aliases, &bundles).join(", ");
        assert!(
            sys_param_re.is_match(&expanded),
            "`{params}` ({label}) must read as a system parameter, got `{expanded}`"
        );
    }

    // A helper argument is still not a system parameter.
    for params in ["commands: &mut Commands", "icons: &Res<ClassIcons>"] {
        let expanded = expanded_param_types(params, &aliases, &bundles).join(", ");
        assert!(
            !sys_param_re.is_match(&expanded),
            "`{params}` is a helper argument, not a system parameter"
        );
    }
}
