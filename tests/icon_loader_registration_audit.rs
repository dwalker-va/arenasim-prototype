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
//! The reading — file discovery, comment blanking, signature and
//! `.add_systems` parsing, and the type-alias and `SystemParam`-bundle
//! expansion that keeps a `Res<'w, ClassIcons>` visible wherever it is spelled
//! — is shared with the repo's other lexical audits in
//! `tests/common/source_audit.rs`. The judging below is this audit's own.
//!
//! Same shape, and the same ALLOWLIST escape hatch, as
//! `tests/registration_audit.rs`.

mod common;

use common::source_audit::{
    add_systems_blocks, load_sources, pub_fn_signatures, repo_path, resource_uses,
    states_plugin_build, SourceFile, SystemParamBundles, TypeAliases,
};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

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
    let files = load_sources(&[SRC_REL]).expect("walk src/");
    let lazy = lazy_resources(&files);

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

    let bundles = SystemParamBundles::scan(&files);
    // Non-vacuity for the bundle expansion specifically: a system takes its
    // resources through a `#[derive(SystemParam)]` struct to stay under Bevy's
    // 16-parameter limit, and the fields of that struct are the blind spot this
    // expansion exists to close. If the scan finds no bundle at all, a future
    // `Res<'w, ClassIcons>` field would go unread and this audit would not say
    // so. `AbilityDispatchExtras` is the tree's bundle today; when it goes, put
    // its replacement here rather than deleting the check.
    let expanded = bundles.expand("extras: AbilityDispatchExtras");
    assert!(
        expanded.iter().any(|ty| ty.contains("Res<")),
        "the SystemParam-bundle scan no longer reads any `Res` field out of \
         AbilityDispatchExtras (found {expanded:?}) — a resource held inside a \
         bundle is invisible to a signature scan, so this audit would stop \
         seeing it. Bundles discovered: {:?}",
        bundles.names().collect::<Vec<_>>()
    );

    let systems = system_signatures(&files, &lazy, &bundles);
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

    let registrations = state_registrations(&files);

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
fn lazy_resources(files: &[SourceFile]) -> BTreeSet<String> {
    let re = Regex::new(
        r"(?m)#\[derive\([^)]*\bResource\b[^)]*\)\]\s*(?:pub\s+)?struct\s+(\w+)\s*\{([^}]*)\}",
    )
    .unwrap();
    let loaded_re = Regex::new(r"\bloaded\s*:\s*bool\b").unwrap();
    let mut out = BTreeSet::new();
    for file in files {
        for cap in re.captures_iter(&file.code) {
            if loaded_re.is_match(&cap[2]) {
                out.insert(cap[1].to_string());
            }
        }
    }
    out
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
    files: &[SourceFile],
    lazy: &BTreeSet<String>,
    bundles: &SystemParamBundles,
) -> Vec<SystemSig> {
    let mut out = Vec::new();
    for file in files {
        let aliases = TypeAliases::from_source(&file.code);
        for sig in pub_fn_signatures(&file.code) {
            let uses = resource_uses(&sig.params, &aliases, bundles);
            let writes: BTreeSet<String> = uses.writes.intersection(lazy).cloned().collect();
            let reads: BTreeSet<String> = uses.reads.intersection(lazy).cloned().collect();
            if !writes.is_empty() || !reads.is_empty() {
                out.push(SystemSig {
                    name: sig.name,
                    writes,
                    reads,
                });
            }
        }
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

/// Pair every system registered in `StatesPlugin::build` with the state(s) the
/// `.add_systems` call that registers it is gated on.
fn state_registrations(files: &[SourceFile]) -> StateRegistrations {
    let states_mod = repo_path(STATES_MOD_FILE_REL);
    let file = files
        .iter()
        .find(|f| f.path == states_mod)
        .expect("src/states/mod.rs must be among the scanned sources");
    let build = states_plugin_build(&file.code).expect("StatesPlugin::build body");
    let ident_re = Regex::new(r"(?:([a-z_][a-z0-9_]*)\s*::\s*)?([a-z_][a-z0-9_]*)").unwrap();

    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for block in add_systems_blocks(&build, &file.code) {
        for cap in ident_re.captures_iter(&block.text) {
            map.entry(cap[2].to_string())
                .or_default()
                .extend(block.states.iter().cloned());
        }
    }
    StateRegistrations { map }
}
