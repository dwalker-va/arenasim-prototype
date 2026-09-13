//! Lazy-loader registration audit — closes the "empty resource on a cold
//! entrance" bug class.
//!
//! ## The class
//!
//! Several resources in this app are filled LAZILY, by a loader system that
//! self-guards on an internal `loaded` flag and is registered under whichever
//! states happen to need it. Nothing in the type system connects a loader to
//! the screens that read what it loads: a system taking `Res<ClassIcons>`
//! compiles, ticks, and renders — just with an empty map, so every icon is a
//! blank tile. The failure is invisible to the compiler, to unit tests, and to
//! the pure-draw snapshot harnesses (which build their data by hand and never
//! touch the ECS). Only a human looking at the screen sees it.
//!
//! It has now happened three times, all with the same root cause — the loader
//! was registered under the state where it was first needed, and a later screen
//! read the resource from a different state:
//!
//! 1. RESULTS × `AbilityIcons` — ability bars drew placeholder tiles unless the
//!    reader had first opened View Combatant or the encyclopedia.
//! 2. RESULTS × `ClassIcons`, then PLAYMATCH × `ClassIcons` — `load_class_icons`
//!    ran only under ConfigureMatch, and `--replay` boots straight into
//!    `PlayMatch`, skipping it. A replay ran the whole match with class-less
//!    team frames and speech bubbles.
//! 3. VIEWCOMBATANT × `ClassIcons` — drew icons only because ConfigureMatch,
//!    its sole entrance, had already filled them.
//!
//! ## The invariant
//!
//! **A state that reads a lazily-loaded resource must register that resource's
//! loader itself.** Not "some state earlier on the path does" — that is the
//! assumption each of the three bugs above was built on, and it is exactly what
//! a new entrance (a replay boot, a deep link, a new screen) breaks. The loaders
//! self-guard, so an extra registration costs one line and nothing at run time.
//!
//! This audit discovers both sides from the source rather than from a list: the
//! lazy resources by their `loaded: bool` self-guard field, the loaders and
//! consumers by their system signatures, and the state each is registered under
//! by reading `StatesPlugin::build`. A screen nobody has written yet is covered
//! the day it takes a `Res<…Icons>` parameter.
//!
//! Same shape, and the same ALLOWLIST escape hatch, as
//! `tests/registration_audit.rs`.

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SRC_REL: &str = "src";
const STATES_MOD_FILE_REL: &str = "src/states/mod.rs";

/// Deliberate exceptions: a consumer that takes a lazily-loaded resource as a
/// parameter under a state whose chain does NOT run its loader, because it
/// provably never paints with it. Each entry must say why.
///
/// `(consumer system, resource, state, justification)`
const ALLOWLIST: &[(&str, &str, &str, &str)] = &[(
    "results_ui",
    "ItemIcons",
    "Results",
    "Results resolves only Topic::Class and Topic::Ability, so it never \
         paints an item icon; the parameter is handed to EncyclopediaData and \
         goes unread. Register load_item_icons in the Results chain if that \
         ever changes.",
)];

#[test]
fn every_state_that_reads_a_lazy_resource_registers_its_loader() {
    let files = rust_files(&repo_path(SRC_REL)).expect("walk src/");
    let lazy = lazy_resources(&files).expect("scan for lazily-loaded resources");

    // Non-vacuity: a parser that silently stops finding anything would make
    // this test pass forever. Pin the resources the class is known to contain.
    for expected in [
        "ClassIcons",
        "SpellIcons",
        "EmojiIcons",
        "AbilityIcons",
        "ItemIcons",
        "HunterPetIcons",
    ] {
        assert!(
            lazy.contains(expected),
            "the lazy-resource scan lost {expected} — it is a `Resource` with a \
             `loaded: bool` self-guard, so either the idiom changed or this \
             audit stopped parsing. Found: {lazy:?}"
        );
    }

    let systems = system_signatures(&files, &lazy).expect("scan for system signatures");
    // resource -> loader system names
    let mut loaders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // (consumer, resource) pairs
    let mut consumers: Vec<(String, String)> = Vec::new();
    for sys in &systems {
        for r in &sys.writes {
            loaders
                .entry(r.clone())
                .or_default()
                .insert(sys.name.clone());
        }
        for r in &sys.reads {
            consumers.push((sys.name.clone(), r.clone()));
        }
    }

    let registrations = state_registrations().expect("parse StatesPlugin::build");

    let allowed: BTreeSet<(&str, &str, &str)> =
        ALLOWLIST.iter().map(|(c, r, s, _)| (*c, *r, *s)).collect();

    let mut checked = 0usize;
    let mut checked_class_icons_in_play_match = false;
    let mut violations: Vec<String> = Vec::new();

    for (consumer, resource) in &consumers {
        let Some(loader_names) = loaders.get(resource) else {
            // Nothing loads it lazily — not this bug class.
            continue;
        };
        for state in registrations.states_for(consumer) {
            checked += 1;
            if resource == "ClassIcons" && state == "PlayMatch" {
                checked_class_icons_in_play_match = true;
            }
            if allowed.contains(&(consumer.as_str(), resource.as_str(), state.as_str())) {
                continue;
            }
            let loaded_here = loader_names
                .iter()
                .any(|loader| registrations.states_for(loader).contains(&state));
            if !loaded_here {
                violations.push(format!(
                    "  {consumer} reads {resource} under GameState::{state}, but none of \
                     {loader_names:?} is registered under that state"
                ));
            }
        }
    }

    assert!(
        checked > 0,
        "the audit checked nothing — the StatesPlugin::build parse found no \
         registrations for any icon consumer"
    );
    assert!(
        checked_class_icons_in_play_match,
        "the audit no longer sees any PlayMatch consumer of ClassIcons — that \
         is the exact pair this file was written for (the team frames and the \
         speech bubbles), so the parse has drifted"
    );

    if !violations.is_empty() {
        panic!(
            "\n\nLazily-loaded resource read under a state that does not load it:\n\n{}\n\n\
             A loader self-guards on its `loaded` flag, so the fix is one line: add it to \
             that state's chain in src/states/mod.rs, e.g.\n\n    \
             configure_match_ui::load_class_icons,\n\n\
             Do NOT rely on an earlier state having filled it — `--replay` boots straight \
             into PlayMatch and skips ConfigureMatch entirely, and the next new entrance \
             will skip something else.\n\n\
             If the consumer provably never paints with the resource, add it to ALLOWLIST \
             in tests/icon_loader_registration_audit.rs with a justification.\n",
            violations.join("\n")
        );
    }
}

// ---- discovery: lazily-loaded resources ----

/// A `#[derive(… Resource …)] struct X { … loaded: bool … }` — the self-guard
/// idiom every lazy loader in this codebase uses.
fn lazy_resources(files: &[PathBuf]) -> std::io::Result<BTreeSet<String>> {
    let re = Regex::new(
        r"(?m)#\[derive\([^)]*\bResource\b[^)]*\)\]\s*(?:pub\s+)?struct\s+(\w+)\s*\{([^}]*)\}",
    )
    .unwrap();
    let loaded_re = Regex::new(r"\bloaded\s*:\s*bool\b").unwrap();
    let mut out = BTreeSet::new();
    for path in files {
        let text = strip_comments(&fs::read_to_string(path)?);
        for cap in re.captures_iter(&text) {
            if loaded_re.is_match(&cap[2]) {
                out.insert(cap[1].to_string());
            }
        }
    }
    Ok(out)
}

// ---- discovery: system signatures ----

struct SystemSig {
    name: String,
    /// Lazy resources taken as `ResMut<R>` — this system fills R.
    writes: BTreeSet<String>,
    /// Lazy resources taken as `Res<R>` or `Option<Res<R>>` — this system reads R.
    reads: BTreeSet<String>,
}

fn system_signatures(
    files: &[PathBuf],
    lazy: &BTreeSet<String>,
) -> std::io::Result<Vec<SystemSig>> {
    let pub_fn_re = Regex::new(r"(?m)^[ \t]*pub\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(").unwrap();
    let res_re = Regex::new(r"\bRes<\s*(?:[\w]+\s*::\s*)*(\w+)\s*>").unwrap();
    let res_mut_re = Regex::new(r"\bResMut<\s*(?:[\w]+\s*::\s*)*(\w+)\s*>").unwrap();

    let mut out = Vec::new();
    for path in files {
        let text = strip_comments(&fs::read_to_string(path)?);
        for m in pub_fn_re.captures_iter(&text) {
            let name = m[1].to_string();
            let Some(params) = param_list(&text, m.get(0).unwrap().end() - 1) else {
                continue;
            };
            let mut writes = BTreeSet::new();
            let mut reads = BTreeSet::new();
            for param in split_params(&params) {
                // A parameter behind a reference is a HELPER argument, not a
                // Bevy system parameter — the helper's caller is the system.
                if param.contains('&') {
                    continue;
                }
                for cap in res_mut_re.captures_iter(&param) {
                    if lazy.contains(&cap[1]) {
                        writes.insert(cap[1].to_string());
                    }
                }
                for cap in res_re.captures_iter(&param) {
                    // `ResMut<…>` also contains `Res` — but not as a word
                    // boundary match of `Res<`, so the two regexes are disjoint.
                    if lazy.contains(&cap[1]) {
                        reads.insert(cap[1].to_string());
                    }
                }
            }
            if !writes.is_empty() || !reads.is_empty() {
                out.push(SystemSig {
                    name,
                    writes,
                    reads,
                });
            }
        }
    }
    Ok(out)
}

/// Text between the parens of a parameter list starting at `open` (the `(`).
fn param_list(text: &str, open: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[open + 1..i].to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split a parameter list on TOP-LEVEL commas (generics and tuples nest).
fn split_params(params: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in params.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

// ---- discovery: which state each system is registered under ----

struct StateRegistrations {
    /// system name -> states it is registered under.
    map: BTreeMap<String, BTreeSet<String>>,
}

impl StateRegistrations {
    fn states_for(&self, system: &str) -> BTreeSet<String> {
        self.map.get(system).cloned().unwrap_or_default()
    }
}

/// Parse every `.add_systems(...)` call in `StatesPlugin::build`, pairing the
/// systems it registers with the state(s) it gates them on.
///
/// Three gate shapes appear in that function:
///   - `run_if(in_state(GameState::X))`      -> {X}
///   - `OnEnter(GameState::X)` / `OnExit(..)` -> {X}
///   - `run_if(<fn>)` where `<fn>` is a plain `-> bool` state predicate
///     (`in_combat_scene`) -> the states named in that function's body.
fn state_registrations() -> std::io::Result<StateRegistrations> {
    let text = strip_comments(&fs::read_to_string(repo_path(STATES_MOD_FILE_REL))?);
    let build = find_states_plugin_build(&text).expect("StatesPlugin::build body");

    let in_state_re = Regex::new(r"in_state\s*\(\s*GameState::(\w+)\s*\)").unwrap();
    let on_enter_exit_re = Regex::new(r"On(?:Enter|Exit)\s*\(\s*GameState::(\w+)\s*\)").unwrap();
    let run_if_fn_re = Regex::new(r"run_if\s*\(\s*([a-z_][a-z0-9_]*)\s*\)").unwrap();
    let ident_re = Regex::new(r"(?:([a-z_][a-z0-9_]*)\s*::\s*)?([a-z_][a-z0-9_]*)").unwrap();
    let game_state_re = Regex::new(r"GameState::(\w+)").unwrap();
    let add_systems_re = Regex::new(r"\.add_systems\s*\(").unwrap();

    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let bytes = build.as_bytes();
    let mut i = 0usize;
    while let Some(m) = add_systems_re.find(&build[i..]) {
        let open = i + m.end() - 1;
        let Some(block) = param_list(&build, open) else {
            break;
        };
        let block_end = open + block.len() + 2;

        let mut states: BTreeSet<String> = BTreeSet::new();
        for cap in in_state_re.captures_iter(&block) {
            states.insert(cap[1].to_string());
        }
        for cap in on_enter_exit_re.captures_iter(&block) {
            states.insert(cap[1].to_string());
        }
        for cap in run_if_fn_re.captures_iter(&block) {
            // A named predicate function: read the states out of its body
            // rather than hardcoding what `in_combat_scene` covers today.
            if let Some(body) = find_fn_body(&text, &cap[1]) {
                for c in game_state_re.captures_iter(&body) {
                    states.insert(c[1].to_string());
                }
            }
        }

        for cap in ident_re.captures_iter(&block) {
            let name = cap[2].to_string();
            map.entry(name).or_default().extend(states.iter().cloned());
        }

        i = block_end.min(bytes.len());
    }
    Ok(StateRegistrations { map })
}

fn find_states_plugin_build(text: &str) -> Option<String> {
    let re = Regex::new(r"\bimpl\s+Plugin\s+for\s+StatesPlugin\b").unwrap();
    let m = re.find(text)?;
    let impl_body = brace_body(text, m.end())?;
    find_fn_body(&impl_body, "build")
}

/// Body of `fn NAME(...) [-> T] { ... }`, braces excluded.
fn find_fn_body(text: &str, fn_name: &str) -> Option<String> {
    let re = Regex::new(&format!(r"\bfn\s+{}\s*\(", regex::escape(fn_name))).ok()?;
    let m = re.find(text)?;
    let after_params = {
        let open = m.end() - 1;
        let params = param_list(text, open)?;
        open + params.len() + 2
    };
    brace_body(text, after_params)
}

/// Body of the next `{ ... }` at or after `from`, braces excluded.
fn brace_body(text: &str, from: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let start = i + 1;
    let mut depth = 1;
    let mut j = start;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        j += 1;
    }
    Some(text[start..j.saturating_sub(1)].to_string())
}

// ---- plumbing ----

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Blank out `//` comments (keeping newlines) so prose mentioning
/// `GameState::X` or a system name cannot be parsed as a registration.
fn strip_comments(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            Some(idx) => line[..idx].to_string(),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn rust_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}
