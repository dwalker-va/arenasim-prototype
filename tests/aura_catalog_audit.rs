//! Aura catalog audit — the drift guard for the encyclopedia's named-aura list.
//!
//! The Buffs & Debuffs section enumerates NAMED auras: one entry per ability
//! carrying an `applies_aura` block in `abilities.ron`, plus a short explicit
//! registry (`encyclopedia::auras::EngineAura`) for the handful the engine
//! applies from code with a hardcoded name.
//!
//! The RON half is self-maintaining — entry N+1 appears the moment the RON
//! gains an `applies_aura`. The engine half is not, and that is the whole
//! hazard: someone adds `ability_name: "Shattered Guard"` at a new apply site,
//! the buff bar dutifully shows "Shattered Guard" to the player, and the
//! encyclopedia has never heard of it. Nothing else in this repo observes an
//! aura the catalog is missing.
//!
//! So this walks `src/states/play_match/**/*.rs` for every `ability_name:`
//! STRING LITERAL and fails unless it resolves to a catalog entry. Same idiom
//! as `tests/registration_audit.rs`: a regex source scan plus an explicit,
//! justified allowlist.
//!
//! ## What this does NOT catch
//!
//! Two blind spots, both known:
//!
//! - A name that arrives through a FUNCTION rather than a literal
//!   (`TotemElement::buff_name()`, `RoguePoison::name()`). Those known
//!   indirections get their own direct assertion below.
//! - A name REUSED across mechanics. The guard asks only "is this name
//!   catalogued", so Frost Armor's on-hit proc — which hangs a movement slow
//!   and an attack-speed slow on the attacker, both named "Frost Armor" —
//!   resolves to the Frost Armor BUFF's entry and passes. Three distinct auras
//!   sharing one name is a naming problem in the engine, not something the
//!   catalog can present its way out of: three "Frost Armor" rows would be
//!   worse than one. Tightening this guard to name-plus-mechanic is only worth
//!   doing once those auras have names of their own.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use arenasim::states::encyclopedia::auras::catalog;
use arenasim::states::match_config::RoguePoison;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::components::TotemElement;

const PLAY_MATCH_REL: &str = "src/states/play_match";

/// `ability_name:` literals that are NOT real auras and must not be catalogued.
/// Each entry names why.
const ALLOWLIST: &[(&str, &str)] = &[
    ("Test", "unit-test fixture in combat_core/mod.rs — never applied in a match"),
    ("TestCC", "unit-test fixture in auras.rs — never applied in a match"),
];

#[test]
fn every_named_aura_in_the_engine_has_a_catalog_entry() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");

    // A name resolves only if the CATALOG has an entry for it. That covers both
    // halves at once — RON-derived entries carry the ability's `name`, engine
    // registry entries carry their hardcoded one — and nothing else, which is
    // the point: matching against `abilities.ron` names instead would let an
    // aura named after an ability that applies no aura slip through with no
    // page behind it. (Frost Trap and the four totems are exactly that shape,
    // and they are in the registry for exactly that reason.)
    let known: BTreeSet<String> =
        catalog(&abilities).into_iter().map(|entry| entry.name).collect();
    let allowed: BTreeSet<&str> = ALLOWLIST.iter().map(|(name, _)| *name).collect();

    let literals = scan_ability_name_literals().expect("failed to walk play_match sources");
    assert!(
        !literals.is_empty(),
        "the scan found no `ability_name:` literals at all — the regex or the path is wrong, \
         and a guard that matches nothing guards nothing"
    );

    let mut violations: Vec<(String, PathBuf, usize)> = Vec::new();
    for (name, path, line) in &literals {
        if known.contains(name.as_str()) || allowed.contains(name.as_str()) {
            continue;
        }
        violations.push((name.clone(), path.clone(), *line));
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\n\nAura name(s) applied in code that the encyclopedia's catalog does not know:\n\n",
        );
        for (name, path, line) in &violations {
            let display = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(path).display();
            msg.push_str(&format!("  \"{}\" at {}:{}\n", name, display, line));
        }
        msg.push_str(
            "\nPlayers see this name on the actor frames, so the encyclopedia must have a page \
             for it. Do ONE of:\n\
             \x20 - Give the applying ability an `applies_aura` block in assets/config/abilities.ron\n\
             \x20   (preferred — the catalog entry then derives itself)\n\
             \x20 - Add a variant to `EngineAura` in src/states/encyclopedia/auras.rs and resolve\n\
             \x20   it in `engine_entry`, reading its values from the constant the apply site reads\n\
             \x20 - Add it to ALLOWLIST in tests/aura_catalog_audit.rs with a one-line\n\
             \x20   justification (test fixtures only)\n\n",
        );
        panic!("{}", msg);
    }
}

/// Aura names that reach `Aura::ability_name` through a function rather than a
/// literal, so the source scan above is blind to them. Kept short on purpose —
/// if this list grows, the scan needs to get smarter rather than the list
/// longer.
#[test]
fn named_auras_built_from_helpers_also_resolve() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let known: BTreeSet<String> =
        catalog(&abilities).into_iter().map(|entry| entry.name).collect();

    // `totems.rs` names each pulsed buff via `TotemElement::buff_name()`.
    for element in TotemElement::ALL {
        let name = element.buff_name();
        assert!(
            known.contains(name),
            "{:?}'s totem buff is named \"{}\" on the actor frames but has no catalog entry",
            element,
            name
        );
    }

    // `Combatant::weapon_poison_self_buff` names the Rogue's coating marker via
    // `RoguePoison::name()`.
    let poison = RoguePoison::default();
    assert!(
        known.contains(poison.name()),
        "the Rogue's \"{}\" marker buff has no catalog entry",
        poison.name()
    );
}

/// Every allowlist entry must still correspond to a literal in the sources —
/// otherwise it is a stale exemption quietly widening the guard.
#[test]
fn the_allowlist_has_no_stale_entries() {
    let literals = scan_ability_name_literals().expect("failed to walk play_match sources");
    let found: BTreeSet<&str> = literals.iter().map(|(name, _, _)| name.as_str()).collect();
    for (name, justification) in ALLOWLIST {
        assert!(
            found.contains(name),
            "ALLOWLIST entry \"{}\" ({}) matches no `ability_name:` literal any more — remove it",
            name,
            justification
        );
    }
}

// ---- source scan ----

/// Every `ability_name: "..."` literal under `src/states/play_match`, as
/// (name, file, 1-based line).
fn scan_ability_name_literals() -> std::io::Result<Vec<(String, PathBuf, usize)>> {
    let re = Regex::new(r#"ability_name:\s*"([^"]*)""#).unwrap();
    let mut out = Vec::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PLAY_MATCH_REL);
    for path in rust_files(&root)? {
        let text = fs::read_to_string(&path)?;
        for (index, line) in text.lines().enumerate() {
            for caps in re.captures_iter(line) {
                out.push((caps[1].to_string(), path.clone(), index + 1));
            }
        }
    }
    Ok(out)
}

fn rust_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(rust_files(&path)?);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}
