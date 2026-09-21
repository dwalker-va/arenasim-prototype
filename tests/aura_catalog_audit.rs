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
//! justified allowlist, read over the shared comment-blanked source view in
//! `tests/common/source_audit.rs`.
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
//!   plus `AttackSpeedSlow` of the chill it hangs on melee attackers.
//!
//! Keyed on name alone, all six resolved to three entries and the guard passed
//! while the catalog was sending a player from their own gold-bordered buff to
//! the enemy debuff's page — wrong polarity, wrong mechanic, wrong duration,
//! wrong removal rule. The pair key makes each of the six resolvable
//! separately (see `EngineAura`'s collision note), and blocks a seventh.
//!
//! Six auras, but only FIVE entries: the chill's two effects are one COMPOUND
//! debuff, so they share an entry (and a dispel takes both). An entry
//! therefore contributes one pair per effect it covers rather than one pair
//! flat — see `known_pairs`. Six auras resolving to five entries is not the
//! collision bug returning; a collision is two DEBUFFS under one name, and the
//! pair key still separates the chill from the Mage's self-buff.
//!
//! ## Every literal is classified, and nothing is skipped in silence
//!
//! A source scan can only read a name that is written down as a string. The
//! hazard is not that some names are not — it is a scan that shrugs at the ones
//! that are not, because a shrug is indistinguishable from coverage. This guard
//! was bitten by that exact shape once already, one level down, where an
//! unreadable `effect_type` was a loud violation and an unreadable
//! `ability_name` was a silent `continue`. So every `Aura { .. }` literal is
//! now sorted into one of three buckets, and the two unreadable ones have
//! somewhere to be justified:
//!
//! - **A string literal** — `ability_name: "Frost Trap".to_string()`. Checked
//!   against the catalog as a `(name, mechanic)` pair.
//! - **An INDIRECTION** — a name arriving through an expression
//!   (`element.buff_name()`, `def.name`), through the field shorthand, or
//!   through a `..base` update that inherits one. Unreadable, so each site is
//!   named in [`INDIRECT_NAME_SITES`] with the reason the catalog still covers
//!   it. `AuraPending::from_ability_with_name` is the interesting one: its
//!   whole purpose is a name the RON does not carry, it has no production
//!   caller, and `the_custom_name_constructor_has_no_production_caller` is what
//!   keeps that true rather than a comment saying so.
//! - **ANONYMOUS** — no `ability_name` and a `..Default::default()` to supply
//!   it. The aura is nameless, shows nothing on the frames and belongs in no
//!   catalog. A literal that is neither named nor defaulted is a violation: the
//!   scan could not read it, so it cannot vouch for it.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use arenasim::states::encyclopedia::auras::catalog;
use arenasim::states::match_config::RoguePoison;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::shaman::totem_spec;
use arenasim::states::play_match::components::{weapon_poison_marker_aura, TotemElement};
use arenasim::states::play_match::equipment::ItemId;

use common::source_audit::{brace_body, load_sources};

/// The shipped item pool, which the catalog needs for its proc-trinket
/// entries. Loaded rather than mocked, so a proc added to `items.ron`
/// is covered by every guard in this file with no edit here.
fn shipped_items() -> arenasim::states::play_match::equipment::ItemDefinitions {
    arenasim::states::play_match::equipment::load_item_definitions().expect("items.ron must load")
}

/// Every directory holding code that can apply an aura in production. The whole
/// of `src/` rather than `src/states/play_match/`: `src/headless/runner.rs`
/// inserts `ActiveAuras` at match setup, and a literal added there would
/// otherwise be invisible to this guard.
const SCAN_REL: &str = "src";

/// `(ability_name, effect_type)` pairs that are NOT real auras and must not be
/// catalogued. Each entry names why.
const ALLOWLIST: &[(&str, &str)] = &[
    (
        "Test",
        "unit-test fixture in combat_core/mod.rs — never applied in a match",
    ),
    (
        "TestCC",
        "unit-test fixture in auras.rs — never applied in a match",
    ),
];

/// Apply sites whose `ability_name` the scan cannot read, keyed on the file and
/// the binding exactly as written. Each entry says what covers the name
/// INSTEAD, because "the scan cannot see it" is a reason to look somewhere
/// else, not a reason to stop looking.
///
/// Kept short on purpose. If this grows past the handful of shared constructors
/// below, the scan needs to get smarter rather than the list longer.
const INDIRECT_NAME_SITES: &[(&str, &str, &str)] = &[
    (
        "src/states/play_match/class_ai/paladin.rs",
        "ability_name: def.name.to_string()",
        "Hammer of Justice's stun, named after its own abilities.ron entry — which carries an \
         `applies_aura` block, so the catalog's RON half already has the page.",
    ),
    (
        "src/states/play_match/combat_core/damage.rs",
        "ability_name: abilities.get_unchecked(&interrupt.ability).name.clone()",
        "the school lockout a successful interrupt leaves behind. \
         `EngineAura::InterruptLockout` is derived from the same interrupt flags this site \
         reads, so interrupt N+1 is catalogued with no code in either place.",
    ),
    (
        "src/states/play_match/combat_core/mod.rs",
        "ability_name: ability_name.to_string()",
        "`create_absorb_aura`, a #[cfg(test)] fixture in this file's own unit tests — never \
         applied in a match.",
    ),
    (
        "src/states/play_match/components/auras.rs",
        "ability_name: ability_def.name.clone()",
        "`AuraPending::from_ability_scaled` and `::from_ability_dot`, the constructors every \
         RON-defined aura passes through. The catalog's RON half reads the same `applies_aura` \
         blocks, so these two are covered wholesale.",
    ),
    (
        "src/states/play_match/components/auras.rs",
        "ability_name,",
        "`AuraPending::from_ability_with_name` — the one constructor that takes a name the RON \
         does not carry. It has NO production caller, and \
         `the_custom_name_constructor_has_no_production_caller` is what keeps that true.",
    ),
    (
        "src/states/play_match/components/combatant.rs",
        "ability_name: poison.name().to_string()",
        "the Rogue's weapon-coating marker; asserted per poison by \
         `named_auras_built_from_helpers_also_resolve`.",
    ),
    (
        "src/states/play_match/totems.rs",
        "ability_name: buff_name.to_string()",
        "a totem's pulsed buff; asserted per element by \
         `named_auras_built_from_helpers_also_resolve`.",
    ),
    (
        "src/states/play_match/proc_trinkets.rs",
        "ability_name: source_name.to_string()",
        "a proc trinket's buff, named after the trinket. `EngineAura::ProcTrinketBuff` is \
         derived from the `proc:` blocks in items.ron, so trinket N+1 is catalogued with no \
         code in either place; asserted per shipped trinket by \
         `named_auras_built_from_helpers_also_resolve`.",
    ),
];

/// Files that must each still yield at least one `ability_name:` STRING
/// LITERAL, with the aura that makes them load-bearing.
///
/// `!scanned.is_empty()` is not an anti-vacuity assertion, it is the appearance
/// of one: a regex or a path that silently cut this scan from sixteen sites to
/// one would sail past it, and under-scanning is the failure this guard has
/// already been bitten by twice. A per-file floor fails on the first file that
/// drops out and names it — and unlike a bare count, it does not have to be
/// revised every time the engine legitimately grows another apply site.
const MUST_CONTRIBUTE: &[(&str, &str)] = &[
    (
        "src/states/play_match/class_ai/priest.rs",
        "Weakened Soul, stamped by the Power Word: Shield cast",
    ),
    (
        "src/states/play_match/combat_core/auto_attack.rs",
        "the Frost Armor proc(s) a melee attacker takes",
    ),
    (
        "src/states/play_match/combat_core/mod.rs",
        "Frostbolt's slow, Weakened Soul, Divine Shield, Shadow Resistance Aura",
    ),
    (
        "src/states/play_match/effects/backlash.rs",
        "Unstable Affliction's dispel-backlash silence",
    ),
    (
        "src/states/play_match/effects/berserker_rage.rs",
        "Berserker Rage's fear immunity",
    ),
    (
        "src/states/play_match/effects/divine_shield.rs",
        "Divine Shield's damage immunity",
    ),
    (
        "src/states/play_match/shadow_sight.rs",
        "the Shadow Sight orb buff",
    ),
    (
        "src/states/play_match/traps.rs",
        "Freezing Trap's incapacitate and the Frost Trap zone's slow",
    ),
];

/// How one scanned `Aura { .. }` literal names itself.
#[derive(Debug, PartialEq, Eq)]
enum NameBinding {
    /// `ability_name: "Frost Trap".to_string()` — the scan can read the name.
    Literal(String),
    /// A name the scan cannot read, rendered as written so it can key
    /// [`INDIRECT_NAME_SITES`]: an expression, the field shorthand
    /// (`ability_name,`), or a `..base` update inheriting a name.
    Indirect(String),
    /// No name and a `..Default::default()` to supply one. Nameless auras show
    /// nothing on the frames, so there is no page owed.
    Anonymous,
}

/// One `Aura { .. }` literal found in the sources.
#[derive(Debug)]
struct ScannedAura {
    binding: NameBinding,
    /// The `effect_type:` variant, as written (`AuraType::Stun` -> `Stun`).
    /// `None` when it comes from a variable, which is the test fixtures.
    mechanic: Option<String>,
    /// The file, relative to the crate root.
    file: String,
    line: usize,
}

/// Every `(frame name, mechanic)` pair the catalog accounts for.
///
/// An entry contributes ONE pair per effect it covers, not one per entry. Most
/// entries are one aura doing one thing; a COMPOUND debuff is several auras
/// bound into one debuff (`CompoundDebuff`), gets one catalog entry, and still
/// has to resolve each of its apply sites — the Frost Armor chill writes both
/// a `MovementSpeedSlow` and an `AttackSpeedSlow` under the name "Frost Armor".
/// Keying only on `mechanic` here would leave the rider's apply site
/// unresolvable and this guard would fail an aura the catalog really does
/// cover; keying on the NAME alone is the collision bug in the module doc
/// above. So: name plus each mechanic.
fn known_pairs(
    abilities: &arenasim::states::play_match::ability_config::AbilityDefinitions,
) -> BTreeSet<(String, String)> {
    catalog(abilities, &shipped_items())
        .into_iter()
        .flat_map(|entry| {
            entry
                .mechanics()
                .into_iter()
                .map(|mechanic| (entry.frame_name.clone(), format!("{:?}", mechanic)))
                .collect::<Vec<_>>()
        })
        .collect()
}

impl ScannedAura {
    /// The name this site writes down, or `None` for the two bindings the scan
    /// cannot read.
    fn literal_name(&self) -> Option<&str> {
        match &self.binding {
            NameBinding::Literal(name) => Some(name.as_str()),
            _ => None,
        }
    }
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
    let known = known_pairs(&abilities);
    let allowed: BTreeSet<&str> = ALLOWLIST.iter().map(|(name, _)| *name).collect();

    let scanned = scan_aura_literals();
    assert!(
        !scanned.is_empty(),
        "the scan found no `Aura {{ .. }}` literals at all — the regex or the path is wrong, \
         and a guard that matches nothing guards nothing"
    );

    let mut violations: Vec<(&ScannedAura, &'static str)> = Vec::new();
    for aura in &scanned {
        // The two unreadable bindings are owned by
        // `every_unreadable_aura_name_is_accounted_for`, which is where they
        // are justified rather than shrugged at.
        let Some(name) = aura.literal_name() else {
            continue;
        };
        if allowed.contains(name) {
            continue;
        }
        match &aura.mechanic {
            // The pair resolves, or it does not. No name-only fallback — that
            // fallback is the bug this guard was rewritten to close.
            Some(mechanic) => {
                if !known.contains(&(name.to_string(), mechanic.clone())) {
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
            msg.push_str(&format!(
                "  \"{}\" ({}) at {}:{}\n      {}\n",
                aura.literal_name().unwrap_or("<unreadable>"),
                aura.mechanic.as_deref().unwrap_or("mechanic unknown"),
                aura.file,
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

/// Every FILE that hardcodes an aura name still hardcodes one.
///
/// The scan's own non-emptiness check can only say "something matched". This
/// says WHICH things matched, so a regex narrowed by one character, or a
/// `SCAN_REL` pointed one directory too deep, fails on the files that went
/// quiet instead of passing on the one that did not.
#[test]
fn every_file_that_hardcodes_an_aura_name_still_contributes() {
    let scanned = scan_aura_literals();
    let contributing: BTreeSet<&str> = scanned
        .iter()
        .filter(|aura| aura.literal_name().is_some())
        .map(|aura| aura.file.as_str())
        .collect();

    let missing: Vec<String> = MUST_CONTRIBUTE
        .iter()
        .filter(|(file, _)| !contributing.contains(file))
        .map(|(file, what)| format!("  {file}\n      expected at least: {what}"))
        .collect();

    assert!(
        missing.is_empty(),
        "\n\n{} file(s) that hardcode an aura name yielded no `ability_name:` string literal:\n\n\
         {}\n\n\
         Either the scan broke (a narrowed regex, a wrong SCAN_REL, a brace matcher that stopped \
         closing these literals) — in which case fix the scan, not this list — or the apply site \
         genuinely moved, in which case update MUST_CONTRIBUTE to name where it moved to.\n\n\
         The scan found {} string-literal site(s) in total, across: {:?}\n\n",
        missing.len(),
        missing.join("\n"),
        scanned
            .iter()
            .filter(|aura| aura.literal_name().is_some())
            .count(),
        contributing
    );
}

/// Every `Aura { .. }` literal whose name the scan CANNOT read is justified.
///
/// This is the asymmetry the round-1 defect had one level down: an unreadable
/// `effect_type` was a loud violation while an unreadable `ability_name` was a
/// silent `continue` — so the single construction path whose entire purpose is
/// a custom hardcoded name was the one path the guard could not see.
#[test]
fn every_unreadable_aura_name_is_accounted_for() {
    let scanned = scan_aura_literals();
    let justified: BTreeSet<(&str, &str)> = INDIRECT_NAME_SITES
        .iter()
        .map(|(file, binding, _)| (*file, *binding))
        .collect();

    let unjustified: Vec<String> = scanned
        .iter()
        .filter_map(|aura| match &aura.binding {
            NameBinding::Indirect(binding)
                if !justified.contains(&(aura.file.as_str(), binding.as_str())) =>
            {
                Some(format!(
                    "  {}:{}\n      `{}`",
                    aura.file, aura.line, binding
                ))
            }
            _ => None,
        })
        .collect();

    assert!(
        unjustified.is_empty(),
        "\n\n{} `Aura {{ .. }}` literal(s) name themselves in a way this scan cannot read, and \
         nothing says what covers them:\n\n{}\n\n\
         A name the scan cannot read is a name the catalog cannot be checked against. Do ONE of:\n\
         \x20 - Write the name as a string literal at the apply site, so the guard reads it\n\
         \x20 - Add the site to INDIRECT_NAME_SITES in tests/aura_catalog_audit.rs, saying what\n\
         \x20   DOES cover the names it produces (a derived `EngineAura` variant, the RON half,\n\
         \x20   or an explicit assertion in `named_auras_built_from_helpers_also_resolve`)\n\n",
        unjustified.len(),
        unjustified.join("\n"),
    );

    // The staleness rule the ALLOWLIST already gets: an entry matching nothing
    // is a justification for a site that no longer exists, quietly standing
    // ready to excuse whatever moves in next.
    let seen: BTreeSet<(&str, &str)> = scanned
        .iter()
        .filter_map(|aura| match &aura.binding {
            NameBinding::Indirect(binding) => Some((aura.file.as_str(), binding.as_str())),
            _ => None,
        })
        .collect();
    for (file, binding, why) in INDIRECT_NAME_SITES {
        assert!(
            seen.contains(&(*file, *binding)),
            "INDIRECT_NAME_SITES entry `{}` in {} ({}) matches no `Aura {{ .. }}` literal any \
             more — remove it",
            binding,
            file,
            why
        );
    }
}

/// `AuraPending::from_ability_with_name` stays unused in production.
///
/// It is a public constructor whose entire purpose is an aura name that
/// `abilities.ron` does not carry — precisely the name the catalog cannot
/// derive and the source scan cannot read. Today it has no production caller,
/// so the hole is theoretical; the day it gets one, the buff bar shows a name
/// the encyclopedia has never heard of and the scan above has nothing to say
/// about it. This is the tripwire for that day.
#[test]
fn the_custom_name_constructor_has_no_production_caller() {
    let call = Regex::new(r"(?:Self|AuraPending)\s*::\s*from_ability_with_name\s*\(").unwrap();
    let mut callers = Vec::new();
    for file in load_sources(&[SCAN_REL]).expect("failed to walk the sources") {
        let code = file.code_without_test_modules();
        for m in call.find_iter(&code) {
            callers.push(format!(
                "  {}:{}",
                file.rel_display(),
                code[..m.start()].matches('\n').count() + 1
            ));
        }
    }

    assert!(
        callers.is_empty(),
        "\n\n`AuraPending::from_ability_with_name` now has {} production caller(s):\n\n{}\n\n\
         It stamps an aura with a name that is in NO config and NO registry, so the encyclopedia \
         cannot derive a page for it and this file's source scan cannot read it. Before keeping \
         the call: add an `EngineAura` variant for the name it produces, say so in \
         INDIRECT_NAME_SITES (the `ability_name,` entry), and relax this test to allow that \
         caller.\n\n",
        callers.len(),
        callers.join("\n"),
    );
}

/// Auras that reach `Aura::ability_name` through a function rather than a
/// literal, so the source scan above cannot read them. Kept short on purpose —
/// if this list grows, the scan needs to get smarter rather than the list
/// longer.
///
/// Asserted as PAIRS, like the scan: the Rogue's coating marker shares its name
/// with the debuff its poison applies, and a name-only assertion here certified
/// that wrong mapping as covered.
#[test]
fn named_auras_built_from_helpers_also_resolve() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let known = known_pairs(&abilities);

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

    // `proc_trinkets::ProcConfig::aura` names a proc's buff after the trinket.
    // Built through the SIMULATION's own constructor — the one `roll_procs`
    // calls — so a trinket whose buff the catalog does not cover fails here
    // rather than showing the player a frame with no page behind it.
    //
    // Two claims, deliberately separate: this walks every shipped proc and
    // checks each resolves; `at_least_one_proc_trinket_ships` below is what
    // stops the loop going vacuous if the pool ever empties.
    let items = shipped_items();
    for id in ItemId::all() {
        let Some(item) = items.get(id) else { continue };
        let Some(proc) = item.proc.as_ref() else {
            continue;
        };
        let sample = proc.aura(&item.name);
        let mechanic = format!("{:?}", sample.effect_type);
        assert!(
            known.contains(&(sample.ability_name.clone(), mechanic.clone())),
            "the proc trinket \"{}\" grants a ({}) buff the frames call \"{}\", and the \
             catalog has no entry for it",
            item.name,
            mechanic,
            sample.ability_name
        );
    }
}

/// The companion claim to the proc-trinket loop above: it walks a pool, so an
/// empty pool would pass it silently. This says the pool is not empty and names
/// what it should contain — the proof set AS-140 shipped, one per trigger.
#[test]
fn at_least_one_proc_trinket_ships() {
    let items = shipped_items();
    let mut with_procs: Vec<&str> = ItemId::all()
        .iter()
        .filter_map(|id| items.get(id))
        .filter(|item| item.proc.is_some())
        .map(|item| item.name.as_str())
        .collect();
    with_procs.sort_unstable();
    assert_eq!(
        with_procs,
        vec![
            "Dragonspine Trophy",
            "Reliquary of Renewal",
            "Sigil of Arcane Surge",
            "Whetstone of Fury",
        ],
        "the shipped proc-trinket set changed — extend this list deliberately, and check \
         `named_auras_built_from_helpers_also_resolve` still covers every member"
    );
}

/// Every allowlist entry must still correspond to a literal in the sources —
/// otherwise it is a stale exemption quietly widening the guard.
#[test]
fn the_allowlist_has_no_stale_entries() {
    let scanned = scan_aura_literals();
    let found: BTreeSet<&str> = scanned
        .iter()
        .filter_map(|aura| aura.literal_name())
        .collect();
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
    let entries = catalog(&abilities, &shipped_items());

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

/// No two catalog entries share a DISPLAY name.
///
/// The `(frame name, mechanic)` key that separates the six colliding entries
/// guarantees a seventh collision gets its own entry — but an entry is not a
/// distinct ROW. Two entries whose `name` happens to match ship two
/// identical-looking lines in the index and two identical search hits, which is
/// the player-visible confusion the parenthetical disambiguators exist to
/// prevent. The test above pins the three collisions we know about; this one
/// holds for the fourth nobody has thought of.
#[test]
fn catalog_display_names_are_unique() {
    let abilities = load_ability_definitions().expect("abilities.ron must load");
    let entries = catalog(&abilities, &shipped_items());

    let mut by_name: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for entry in &entries {
        by_name
            .entry(entry.name.as_str())
            .or_default()
            .push(format!(
                "frame name \"{}\" ({:?})",
                entry.frame_name, entry.mechanic
            ));
    }

    let collisions: Vec<String> = by_name
        .into_iter()
        .filter(|(_, sharing)| sharing.len() > 1)
        .map(|(name, sharing)| format!("  \"{}\"\n      {}", name, sharing.join("\n      ")))
        .collect();

    assert!(
        collisions.is_empty(),
        "\n\n{} catalog name(s) are carried by more than one entry:\n\n{}\n\n\
         The index rows, the search results and the page headers all print this name, so two \
         entries sharing one are two rows a player cannot tell apart. Give each a disambiguating \
         parenthetical in its `EngineAura` entry, and set `frame_name` to what the actor frames \
         actually show.\n\n",
        collisions.len(),
        collisions.join("\n"),
    );
}

// ---- source scan ----

/// Every `Aura { .. }` struct literal under `src/`, classified by how it names
/// itself, with the `effect_type:` written beside it.
///
/// Brace-matched rather than line-matched: the two fields sit several lines
/// apart inside the same literal, and pairing them is the entire point. Read
/// over the shared comment-blanked source view, so a doc comment that writes
/// out `Aura { .. }` — this module's own docs do — is not mistaken for code.
fn scan_aura_literals() -> Vec<ScannedAura> {
    let open = Regex::new(r"\bAura\s*\{").unwrap();
    let literal_re = Regex::new(r#"ability_name:\s*"([^"]*)""#).unwrap();
    // Anything else bound to `ability_name`: an expression, or the field
    // shorthand. `[^,\n]*` stops at the field separator, which is what makes
    // the captured text a stable allowlist key.
    let indirect_re = Regex::new(r"ability_name\s*(?::\s*([^,\n]*)|,)").unwrap();
    let update_re = Regex::new(r"\.\.\s*([A-Za-z_][\w:]*(?:\s*\(\s*\))?)").unwrap();
    let mechanic_re = Regex::new(r"effect_type:\s*(?:\w+::)*AuraType::(\w+)").unwrap();

    let mut out = Vec::new();
    for file in load_sources(&[SCAN_REL]).expect("failed to walk the sources") {
        let text = &file.code;
        let rel = file.rel_display();
        for open_match in open.find_iter(text) {
            // `struct Aura {`, `impl Aura {` and `-> Aura {` are not
            // construction sites. The last one also matters for counting: its
            // braces enclose the whole function body, so without this the
            // literal inside is scanned twice.
            let line_start = text[..open_match.start()].rfind('\n').map_or(0, |i| i + 1);
            if is_declaration_prefix(&text[line_start..open_match.start()]) {
                continue;
            }
            let brace = open_match.end() - 1;
            let Some(body) = brace_body(text, brace) else {
                continue;
            };
            let line = text[..brace].matches('\n').count() + 1;

            let binding = if let Some(c) = literal_re.captures(&body) {
                NameBinding::Literal(c[1].to_string())
            } else if let Some(c) = indirect_re.captures(&body) {
                NameBinding::Indirect(match c.get(1) {
                    Some(expr) => format!("ability_name: {}", expr.as_str().trim()),
                    None => "ability_name,".to_string(),
                })
            } else {
                match update_re.captures(&body).map(|c| c[1].to_string()) {
                    // `..Default::default()` leaves `ability_name` empty: a
                    // nameless aura, nothing on the frames, no page owed.
                    Some(base) if strip_ws(&base) == "Default::default()" => NameBinding::Anonymous,
                    // `..some_other_aura` INHERITS a name from elsewhere; and a
                    // literal with neither an `ability_name` nor a `..base` is
                    // one this scan mis-read. Both are unreadable, so both go
                    // through the justification list rather than past it.
                    Some(base) => NameBinding::Indirect(format!("..{}", strip_ws(&base))),
                    None => NameBinding::Indirect("<no ability_name, no ..base>".to_string()),
                }
            };

            out.push(ScannedAura {
                binding,
                mechanic: mechanic_re.captures(&body).map(|c| c[1].to_string()),
                file: rel.clone(),
                line,
            });
        }
    }
    out
}

fn strip_ws(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Does the text between the start of a line and an `Aura {` match end in
/// `struct`, `impl`, or a `->` return arrow (with any module path in between)?
fn is_declaration_prefix(prefix: &str) -> bool {
    // Strip any `some::path::` qualifier written between the keyword and the
    // type name, so `-> super::Aura {` reads the same as `-> Aura {`. Only a
    // path is stripped — taking the identifier too would eat the very keyword
    // being looked for.
    let mut head = prefix.trim_end();
    while let Some(rest) = head.strip_suffix("::") {
        head = rest
            .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
            .trim_end();
    }
    head.ends_with("struct") || head.ends_with("impl") || head.ends_with("->")
}
