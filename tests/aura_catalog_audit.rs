//! Aura catalog audit — the drift guard for the encyclopedia's named-aura list.
//!
//! The Buffs & Debuffs section enumerates NAMED auras: one entry per ability
//! carrying an `applies_aura` block in `abilities.ron`, plus a short explicit
//! registry (`encyclopedia::auras::EngineAura`) for the ones the engine applies
//! from code with a hardcoded name.
//!
//! The RON half is self-maintaining — entry N+1 appears the moment the RON
//! gains an `applies_aura`. The engine half is not, and that is the whole
//! hazard: someone adds `ability_name: "Shattered Guard"` at a new apply site,
//! the buff bar dutifully shows "Shattered Guard" to the player, and the
//! encyclopedia has never heard of it. Nothing else in this repo observes an
//! aura the catalog is missing.
//!
//! So this walks `src/**/*.rs` for every `Aura { .. }` literal and fails unless
//! its `(ability_name, effect_type)` pair resolves to a catalog entry. Same
//! idiom as `tests/registration_audit.rs`: a source scan plus an explicit,
//! justified allowlist.
//!
//! ## Why the key is a PAIR, not just the name
//!
//! A name-keyed guard walks straight past NAME REUSE, and the engine reuses
//! three names across six distinct auras:
//!
//! - "Crippling Poison" — the Rogue's own coating marker (a `WeaponPoison`
//!   SELF-BUFF) and the `MovementSpeedSlow` that coating puts on its victim.
//! - "Unstable Affliction" — an 18-second `DamageOverTime` and the `Silence`
//!   its dispel backlash inflicts.
//! - "Frost Armor" — the Mage's `FrostArmorBuff` and the `MovementSpeedSlow`
//!   plus `AttackSpeedSlow` it hangs on melee attackers.
//!
//! Keyed on name alone, all six resolved to three entries and the guard passed
//! while the catalog was sending a player from their own gold-bordered buff to
//! the enemy debuff's page — wrong polarity, wrong mechanic, wrong duration,
//! wrong removal rule. The pair key makes each of the six an entry of its own
//! (see `EngineAura`'s collision note), and blocks a seventh.
//!
//! ## What this does NOT catch
//!
//! A name that arrives through a FUNCTION rather than a literal
//! (`TotemElement::buff_name()`, `RoguePoison::name()`) is invisible to a
//! source scan. Those known indirections get their own pair assertion below,
//! built from the same functions the engine calls.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use arenasim::states::encyclopedia::auras::catalog;
use arenasim::states::match_config::RoguePoison;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::shaman::totem_spec;
use arenasim::states::play_match::components::{weapon_poison_marker_aura, TotemElement};

/// Every directory holding code that can apply an aura in production. The whole
/// of `src/` rather than `src/states/play_match/`: `src/headless/runner.rs`
/// inserts `ActiveAuras` at match setup, and a literal added there would
/// otherwise be invisible to this guard.
const SCAN_REL: &str = "src";

/// `(ability_name, effect_type)` pairs that are NOT real auras and must not be
/// catalogued. Each entry names why.
const ALLOWLIST: &[(&str, &str)] = &[
    ("Test", "unit-test fixture in combat_core/mod.rs — never applied in a match"),
    ("TestCC", "unit-test fixture in auras.rs — never applied in a match"),
];

/// One `Aura { .. }` literal found in the sources.
#[derive(Debug)]
struct ScannedAura {
    /// The `ability_name:` string literal.
    name: String,
    /// The `effect_type:` variant, as written (`AuraType::Stun` -> `Stun`).
    /// `None` when it comes from a variable, which is the test fixtures.
    mechanic: Option<String>,
    path: PathBuf,
    line: usize,
}

#[test]
fn every_named_aura_in_the_engine_has_a_catalog_entry() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");

    // A pair resolves only if the CATALOG has an entry for it. That covers both
    // halves at once — RON-derived entries carry the ability's `name`, engine
    // registry entries carry the hardcoded one their apply site writes — and
    // nothing else, which is the point: matching against `abilities.ron` names
    // instead would let an aura named after an ability that applies no aura
    // slip through with no page behind it. (Frost Trap and the four totems are
    // exactly that shape, and they are in the registry for exactly that reason.)
    //
    // Keyed on FRAME name: the name the engine writes, not the catalog's
    // disambiguated one. That is what an apply site can be compared against.
    let known: BTreeSet<(String, String)> = catalog(&abilities)
        .into_iter()
        .map(|entry| (entry.frame_name, format!("{:?}", entry.mechanic)))
        .collect();
    let allowed: BTreeSet<&str> = ALLOWLIST.iter().map(|(name, _)| *name).collect();

    let scanned = scan_aura_literals().expect("failed to walk the sources");
    assert!(
        !scanned.is_empty(),
        "the scan found no `Aura {{ .. }}` literals with an `ability_name:` at all — the regex \
         or the path is wrong, and a guard that matches nothing guards nothing"
    );

    let mut violations: Vec<(&ScannedAura, &'static str)> = Vec::new();
    for aura in &scanned {
        if allowed.contains(aura.name.as_str()) {
            continue;
        }
        match &aura.mechanic {
            // The pair resolves, or it does not. No name-only fallback — that
            // fallback is the bug this guard was rewritten to close.
            Some(mechanic) => {
                if !known.contains(&(aura.name.clone(), mechanic.clone())) {
                    violations.push((aura, "no catalog entry for this (name, mechanic) pair"));
                }
            }
            // An `effect_type` that is not a plain `AuraType::` variant, on an
            // aura whose name IS a literal. Nothing in the sources does this
            // outside the allowlisted test fixtures; if something starts to,
            // the guard cannot check it and must say so rather than pass.
            None => violations.push((
                aura,
                "its `effect_type` is not a literal `AuraType::` variant, so the mechanic \
                 cannot be read from the source",
            )),
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\n\nAura(s) applied in code that the encyclopedia's catalog does not know:\n\n",
        );
        for (aura, why) in &violations {
            let display = aura
                .path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&aura.path)
                .display();
            msg.push_str(&format!(
                "  \"{}\" ({}) at {}:{}\n      {}\n",
                aura.name,
                aura.mechanic.as_deref().unwrap_or("mechanic unknown"),
                display,
                aura.line,
                why
            ));
        }
        msg.push_str(
            "\nPlayers see this name on the actor frames, so the encyclopedia must have a page \
             for it. Do ONE of:\n\
             \x20 - Give the applying ability an `applies_aura` block in assets/config/abilities.ron\n\
             \x20   (preferred — the catalog entry then derives itself)\n\
             \x20 - Add a variant to `EngineAura` in src/states/encyclopedia/auras.rs and resolve\n\
             \x20   it in `engine_entry`, reading its values from the constant or shared\n\
             \x20   constructor the apply site reads. If the name is already taken by another\n\
             \x20   aura, give the entry a disambiguated `name` and set `frame_name` to what the\n\
             \x20   frames show.\n\
             \x20 - Add it to ALLOWLIST in tests/aura_catalog_audit.rs with a one-line\n\
             \x20   justification (test fixtures only)\n\n",
        );
        panic!("{}", msg);
    }
}

/// Auras that reach `Aura::ability_name` through a function rather than a
/// literal, so the source scan above is blind to them. Kept short on purpose —
/// if this list grows, the scan needs to get smarter rather than the list
/// longer.
///
/// Asserted as PAIRS, like the scan: the Rogue's coating marker shares its name
/// with the debuff its poison applies, and a name-only assertion here certified
/// that wrong mapping as covered.
#[test]
fn named_auras_built_from_helpers_also_resolve() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let known: BTreeSet<(String, String)> = catalog(&abilities)
        .into_iter()
        .map(|entry| (entry.frame_name, format!("{:?}", entry.mechanic)))
        .collect();

    // `totems.rs` names each pulsed buff via `TotemElement::buff_name()`, and
    // takes its mechanic from `totem_spec`.
    for element in TotemElement::ALL {
        let name = element.buff_name().to_string();
        let mechanic = format!("{:?}", totem_spec(element).1);
        assert!(
            known.contains(&(name.clone(), mechanic.clone())),
            "{:?}'s totem buff is \"{}\" ({}) on the actor frames but has no catalog entry",
            element,
            name,
            mechanic
        );
    }

    // `Combatant::weapon_poison_self_buff` builds the Rogue's coating marker
    // through `weapon_poison_marker_aura`. Its name is the poison's, which the
    // poison's DEBUFF also carries — so the mechanic half of this pair is the
    // only thing separating the buff a Rogue sees on itself from the slow it
    // puts on its victim.
    for poison in RoguePoison::ALL {
        let sample = weapon_poison_marker_aura(poison);
        let mechanic = format!("{:?}", sample.effect_type);
        assert!(
            known.contains(&(sample.ability_name.clone(), mechanic.clone())),
            "the Rogue's \"{}\" ({}) marker buff has no catalog entry of its own — a name-only \
             match would have resolved it to the {} debuff's page",
            sample.ability_name,
            mechanic,
            sample.ability_name
        );
    }
}

/// Every allowlist entry must still correspond to a literal in the sources —
/// otherwise it is a stale exemption quietly widening the guard.
#[test]
fn the_allowlist_has_no_stale_entries() {
    let scanned = scan_aura_literals().expect("failed to walk the sources");
    let found: BTreeSet<&str> = scanned.iter().map(|aura| aura.name.as_str()).collect();
    for (name, justification) in ALLOWLIST {
        assert!(
            found.contains(name),
            "ALLOWLIST entry \"{}\" ({}) matches no `ability_name:` literal any more — remove it",
            name,
            justification
        );
    }
}

/// The reused names are still reused. If an engine rename ever makes one
/// unique, this fails and the catalog's parenthetical disambiguator can be
/// dropped along with it — the doc above stops being true silently otherwise.
#[test]
fn the_catalog_disambiguates_every_reused_engine_name() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let entries = catalog(&abilities);

    for reused in ["Crippling Poison", "Unstable Affliction", "Frost Armor"] {
        let sharing: Vec<&str> = entries
            .iter()
            .filter(|e| e.frame_name == reused)
            .map(|e| e.name.as_str())
            .collect();
        assert!(
            sharing.len() > 1,
            "\"{}\" is documented as a reused engine name but only {} entry(s) carry it: {:?}. \
             If the engine renamed one, update the module doc and EngineAura's collision note.",
            reused,
            sharing.len(),
            sharing
        );
        // Distinct catalog names, or the index shows identical rows.
        let unique: BTreeSet<&str> = sharing.iter().copied().collect();
        assert_eq!(
            unique.len(),
            sharing.len(),
            "entries sharing the frame name \"{}\" must have distinct catalog names: {:?}",
            reused,
            sharing
        );
    }
}

// ---- source scan ----

/// Every `Aura { .. }` struct literal under `src/` that sets `ability_name:` to
/// a string literal, with the `effect_type:` written beside it.
///
/// Brace-matched rather than line-matched: the two fields sit several lines
/// apart inside the same literal, and pairing them is the entire point.
fn scan_aura_literals() -> std::io::Result<Vec<ScannedAura>> {
    let open = Regex::new(r"\bAura\s*\{").unwrap();
    let name_re = Regex::new(r#"ability_name:\s*"([^"]*)""#).unwrap();
    let mechanic_re =
        Regex::new(r"effect_type:\s*(?:\w+::)*AuraType::(\w+)").unwrap();

    let mut out = Vec::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SCAN_REL);
    for path in rust_files(&root)? {
        let text = fs::read_to_string(&path)?;
        for open_match in open.find_iter(&text) {
            let start = open_match.end() - 1; // the `{`
            let Some(end) = matching_brace(&text, start) else { continue };
            let body = &text[start..=end];
            let Some(name) = name_re.captures(body) else { continue };
            out.push(ScannedAura {
                name: name[1].to_string(),
                mechanic: mechanic_re.captures(body).map(|c| c[1].to_string()),
                path: path.clone(),
                line: text[..start].matches('\n').count() + 1,
            });
        }
    }
    Ok(out)
}

/// Index of the `}` closing the `{` at `open`. Brace counting is enough here:
/// these literals hold no string or comment containing an unbalanced brace, and
/// the scan skips any literal it cannot close.
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
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
