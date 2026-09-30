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
    add_systems_blocks, blank_comments_and_strings, expanded_param_types, find_fn_body,
    load_sources, pub_fn_signatures, rel_display, repo_path, states_plugin_build, SourceFile,
    SystemParamBundles, TypeAliases,
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

/// A candidate system: its name, and where it is defined.
type Candidate = (String, PathBuf, usize);

#[test]
fn audit_combat_system_registration() {
    let core_registered =
        extract_registered_in_function(SYSTEMS_FILE_REL, "add_core_combat_systems")
            .unwrap_or_else(|e| panic!("reading add_core_combat_systems: {e}"));
    let graphical_registered = extract_registered_in_states_plugin_build()
        .unwrap_or_else(|e| panic!("reading StatesPlugin::build: {e}"));
    let candidates =
        walk_play_match_fns().expect("failed to walk play_match for candidate pub fn items");

    // Every set the verdict rests on actually read something: a named member
    // of each, not a size floor. A reader that silently came back empty would
    // otherwise pass (no candidates) or fail for the wrong reason.
    let candidate_names: BTreeSet<&str> = candidates.iter().map(|(n, ..)| n.as_str()).collect();
    for (set, label, member) in [
        (&candidate_names, "candidates", "decide_abilities"),
        (&candidate_names, "candidates", "plan_celebration"),
    ] {
        assert!(set.contains(member), "{label} must include `{member}`");
    }
    for (set, label, member) in [
        (
            &core_registered,
            "add_core_combat_systems",
            "decide_abilities",
        ),
        (
            &graphical_registered,
            "StatesPlugin::build",
            "plan_celebration",
        ),
        (
            &graphical_registered,
            "StatesPlugin::build",
            "step_celebration",
        ),
    ] {
        assert!(set.contains(member), "{label} must register `{member}`");
    }

    let violations = unregistered(&candidates, &[&core_registered, &graphical_registered]);

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

/// The candidates no registered set and no ALLOWLIST entry accounts for.
fn unregistered<'a>(
    candidates: &'a [Candidate],
    registered: &[&BTreeSet<String>],
) -> Vec<&'a Candidate> {
    let allowlist: BTreeSet<&str> = ALLOWLIST.iter().map(|(name, _)| *name).collect();
    candidates
        .iter()
        .filter(|(name, ..)| {
            !allowlist.contains(name.as_str()) && !registered.iter().any(|set| set.contains(name))
        })
        .collect()
}

// ---- registered-set extraction ----

fn extract_registered_in_function(
    rel_path: &str,
    fn_name: &str,
) -> Result<BTreeSet<String>, String> {
    let file = read_one(rel_path)?;
    let body = find_fn_body(&file.code, fn_name).unwrap_or_default();
    collect_registered_identifiers(&body, &file.code)
}

fn extract_registered_in_states_plugin_build() -> Result<BTreeSet<String>, String> {
    let file = read_one(STATES_MOD_FILE_REL)?;
    let build = states_plugin_build(&file.code).unwrap_or_default();
    collect_registered_identifiers(&build, &file.code)
}

fn read_one(rel_path: &str) -> Result<SourceFile, String> {
    SourceFile::read(&repo_path(rel_path)).map_err(|e| format!("{rel_path}: {e}"))
}

/// Every system the `.add_systems(…)` calls inside `body` register.
///
/// Each call is PARSED rather than scanned line by line: its second argument
/// is read as a system expression, so a registration is found however it is
/// laid out — one line or many, a bare path or a tuple, tuples nested in
/// tuples, any `.chain()` / `.after(…)` / `.run_if(…)` suffix. Only the
/// systems themselves count: a name that appears as an ARGUMENT to one of
/// those methods is an ordering edge or a run condition, not a registration.
///
/// A call whose shape the parser does not know is an error, never a skip —
/// the audit fails loudly on code it cannot read instead of guessing at it.
fn collect_registered_identifiers(body: &str, file: &str) -> Result<BTreeSet<String>, String> {
    let mut registered = BTreeSet::new();
    for block in add_systems_blocks(body, file) {
        registered.extend(registered_by_call(&block.text)?);
    }
    Ok(registered)
}

/// The systems one `.add_systems(SCHEDULE, SYSTEMS)` call registers, given the
/// text between its parentheses.
fn registered_by_call(call: &str) -> Result<Vec<String>, String> {
    let code = blank_comments_and_strings(call);
    let args = split_top_level(&code);
    let [_schedule, systems] = args.as_slice() else {
        return Err(format!(
            "expected `.add_systems(SCHEDULE, SYSTEMS)`, found {} argument(s) in:\n{call}",
            args.len()
        ));
    };
    let mut out = Vec::new();
    parse_system_expr(systems, &mut out).map_err(|e| format!("{e} in:\n{call}"))?;
    Ok(out)
}

/// A system expression is a path (`sys`, `module::sys`, `sys::<T>`) or a
/// parenthesised tuple of system expressions, followed by any chain of method
/// calls. A path's last segment is the registered name.
fn parse_system_expr(expr: &str, out: &mut Vec<String>) -> Result<(), String> {
    let expr = expr.trim();
    let rest = if expr.starts_with('(') {
        let close = matching_close(expr, 0).ok_or_else(|| format!("unbalanced tuple `{expr}`"))?;
        for member in split_top_level(&expr[1..close]) {
            parse_system_expr(&member, out)?;
        }
        &expr[close + 1..]
    } else {
        let (name, rest) =
            parse_path(expr).ok_or_else(|| format!("unrecognised system expression `{expr}`"))?;
        out.push(name.to_string());
        rest
    };
    skip_method_chain(rest).ok_or_else(|| format!("unrecognised suffix `{}`", rest.trim()))
}

/// `ident(::ident)*`, optionally ending in a turbofish. Returns the last
/// identifier and the text after the path.
fn parse_path(text: &str) -> Option<(&str, &str)> {
    let (mut name, mut rest) = parse_ident(text)?;
    while let Some(after) = rest.trim_start().strip_prefix("::") {
        let after = after.trim_start();
        if after.starts_with('<') {
            rest = &after[matching_close(after, 0)? + 1..];
            break;
        }
        (name, rest) = parse_ident(after)?;
    }
    Some((name, rest))
}

fn parse_ident(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    let first = text.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    Some((&text[..end], &text[end..]))
}

/// Consume `(.method[::<…>](…))*` through to the end of `text`; `None` if
/// anything else is left.
fn skip_method_chain(mut text: &str) -> Option<()> {
    loop {
        text = text.trim_start();
        if text.is_empty() {
            return Some(());
        }
        let (_, rest) = parse_ident(text.strip_prefix('.')?)?;
        let mut rest = rest.trim_start();
        if let Some(turbofish) = rest.strip_prefix("::") {
            let turbofish = turbofish.trim_start();
            if !turbofish.starts_with('<') {
                return None;
            }
            rest = turbofish[matching_close(turbofish, 0)? + 1..].trim_start();
        }
        if !rest.starts_with('(') {
            return None;
        }
        text = &rest[matching_close(rest, 0)? + 1..];
    }
}

/// Index of the delimiter closing the one at `open` (`(`, `[`, `{` or `<`).
/// Brackets nest; angle brackets count only inside an angle-bracketed span, so
/// a `>` in an argument (`a > b`, `->`) never closes anything.
fn matching_close(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let angled = bytes[open] == b'<';
    let mut depth = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b'<' if angled => depth += 1,
            b')' | b']' | b'}' => depth = depth.checked_sub(1)?,
            b'>' if angled => depth = depth.checked_sub(1)?,
            _ => {}
        }
        if depth == 0 {
            return Some(i);
        }
    }
    None
}

/// Split on the commas outside every bracket and turbofish; empty pieces (a
/// trailing comma) are dropped.
fn split_top_level(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => i = matching_close(text, i).unwrap_or(bytes.len()),
            b'<' if text[..i].trim_end().ends_with("::") => {
                i = matching_close(text, i).unwrap_or(bytes.len())
            }
            b',' => {
                pieces.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    pieces.push(&text[start.min(bytes.len())..]);
    pieces
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(str::to_string)
        .collect()
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
fn walk_play_match_fns() -> std::io::Result<Vec<Candidate>> {
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

// ---- the registration reader, pinned ----

/// Two systems registered as a tuple written on ONE line — the shape the old
/// line-by-line reader could not see.
const ONE_LINE_TUPLE: &str = "app.add_systems(FixedUpdate, (plan_celebration, play_match::step_celebration).chain().run_if(in_state(GameState::PlayMatch)));";

fn registered_in(body: &str) -> BTreeSet<String> {
    collect_registered_identifiers(body, "").unwrap_or_else(|e| panic!("{e}"))
}

fn names<'a>(items: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    items.into_iter().map(str::to_string).collect()
}

fn fixture_candidates(names: &[&str]) -> Vec<Candidate> {
    names
        .iter()
        .map(|n| (n.to_string(), PathBuf::from("fixture.rs"), 1))
        .collect()
}

fn unregistered_names(candidates: &[Candidate], body: &str) -> Vec<String> {
    unregistered(candidates, &[&registered_in(body)])
        .into_iter()
        .map(|(name, ..)| name.clone())
        .collect()
}

#[test]
fn a_one_line_tuple_registers_every_member() {
    assert_eq!(
        registered_in(ONE_LINE_TUPLE),
        names(["plan_celebration", "step_celebration"])
    );
    let candidates = fixture_candidates(&["plan_celebration", "step_celebration"]);
    assert_eq!(
        unregistered_names(&candidates, ONE_LINE_TUPLE),
        Vec::<String>::new()
    );
}

#[test]
fn a_system_removed_from_a_one_line_tuple_is_named() {
    let removed = ONE_LINE_TUPLE.replace(", play_match::step_celebration", "");
    assert_ne!(
        removed, ONE_LINE_TUPLE,
        "the fixture edit must remove something"
    );

    let candidates = fixture_candidates(&["plan_celebration", "step_celebration"]);
    assert_eq!(
        unregistered_names(&candidates, &removed),
        vec!["step_celebration".to_string()]
    );
}

/// Every layout the reader must see through, and the one thing it must not
/// count: a name that appears only as an ordering edge or a run condition.
#[test]
fn every_registration_shape_is_read_and_nothing_else_is() {
    let body = r#"
        app.add_systems(Startup, setup_scene)
            .add_systems(
                OnEnter(GameState::PlayMatch),
                (
                    spawn_a, // a trailing comment, with a comma
                    (nested_b, nested_c.after(spawn_a)).chain(),
                    load_icons::<ClassIcons, SpellIcons>.run_if(|f: Res<Flag>| f.0 > 1),
                    ApplyDeferred,
                )
                    .chain()
                    .in_set(Phase::One)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            .add_systems(
                Update,
                play_match::single
                    .after(
                        ordering_only,
                    )
                    .run_if(condition_only),
            );
    "#;
    assert_eq!(
        registered_in(body),
        names([
            "setup_scene",
            "spawn_a",
            "nested_b",
            "nested_c",
            "load_icons",
            "ApplyDeferred",
            "single",
        ])
    );

    // An ordering edge names a system; it does not register one.
    let candidates = fixture_candidates(&["single", "ordering_only", "condition_only"]);
    assert_eq!(
        unregistered_names(&candidates, body),
        vec!["ordering_only".to_string(), "condition_only".to_string()]
    );
}

/// A registration the reader does not understand stops the audit, naming the
/// call, rather than being skipped.
#[test]
fn an_unreadable_registration_is_an_error() {
    for body in [
        "app.add_systems(Update, IntoSystem::into_system(foo));",
        "app.add_systems(Update, move || foo());",
        "app.add_systems(Update);",
        "app.add_systems(Update, (a, b).chain() + c);",
    ] {
        let err = collect_registered_identifiers(body, "")
            .expect_err(&format!("`{body}` must not be read as a registration"));
        assert!(err.contains("add_systems") || err.contains("in:"), "{err}");
    }
}
