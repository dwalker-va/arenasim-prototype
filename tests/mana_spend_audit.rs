//! Mana-spend audit
//!
//! A unit's resource (`Combatant::current_mana`: mana, and a Warrior's rage
//! and a Rogue's energy) must never go below zero. Nothing clamps it, by
//! design: a clamp would hide the spend that skipped its affordability check.
//! Aimed Shot was charged twice, as its cast began and again as it landed, so
//! a Hunter that could afford it once ended the match at negative mana
//! (AS-438). This audit keeps every spend in view.
//!
//! Two questions, each answered as SET EQUALITY over named members, so a new
//! member fails loudly instead of passing under a `>=` floor:
//!
//! 1. **Where does the resource go down?** Every subtractive or zeroing write
//!    to `current_mana` in non-test `src/` code, named by (file, enclosing fn)
//!    in SPENDS or DRAINS. A new member is named with how it knows the unit
//!    can pay.
//! 2. **Does a cast with a cast bar pay once?** A cast pays as it LANDS, in
//!    `process_casting` (CLAUDE.md, *Combat Flow*). No function that begins a
//!    cast (`CastingState::new`) may also lower the resource, or the cast
//!    pays twice.
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source. It does not see a
//! drop spelled without `-` or a zero literal (`current_mana.min(max_mana)`
//! when a mana buff ends, which cannot go below zero, and the pre-match
//! resets to `pre_combat_resource()`), nor one taken in two steps.

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::BTreeSet;

const CLASS_AI: &str = "src/states/play_match/class_ai";

/// The class AI's spends: an instant, a channel or a shapeshift paid at the
/// decision that takes it, after `cast_gate` cleared it — the gate refuses a
/// cast the unit cannot pay for (`RejectionReason::InsufficientMana` and its
/// rage and energy kin). `decide_hunter_action` is Disengage; `fire_interrupt`,
/// `fire_psychic_scream`, `cast_hammer_of_justice`, `cast_shadow_word_pain`
/// and `channel_mind_flay` are the paying halves of decisions gated by their
/// callers.
const SPENDS: &[(&str, &str)] = &[
    ("druid.rs", "try_mark_of_the_wild"),
    ("druid.rs", "try_travel_form"),
    ("hunter.rs", "apply_flare"),
    ("hunter.rs", "decide_hunter_action"),
    ("hunter.rs", "try_arcane_shot"),
    ("hunter.rs", "try_concussive_shot"),
    ("hunter.rs", "try_place_trap_at"),
    ("hunter.rs", "try_serpent_sting"),
    ("interrupts.rs", "fire_interrupt"),
    ("mage.rs", "try_arcane_intellect"),
    ("mage.rs", "try_frost_nova"),
    ("mage.rs", "try_ice_barrier"),
    ("mage.rs", "try_mage_armor"),
    ("mod.rs", "try_dispel_ally"),
    ("mod.rs", "try_purge_enemy"),
    ("paladin.rs", "cast_hammer_of_justice"),
    ("paladin.rs", "try_holy_shock_damage"),
    ("paladin.rs", "try_holy_shock_heal"),
    ("paladin_holy.rs", "try_aura_mastery"),
    ("paladin_holy.rs", "try_divine_favor"),
    ("paladin_protection.rs", "try_blessing_of_sacrifice"),
    ("paladin_protection.rs", "try_divine_protection"),
    ("paladin_protection.rs", "try_sacred_shield"),
    ("paladin_retribution.rs", "try_crusader_strike"),
    ("paladin_retribution.rs", "try_repentance"),
    ("priest.rs", "cast_shadow_word_pain"),
    ("priest.rs", "fire_psychic_scream"),
    ("priest.rs", "try_fortitude"),
    ("priest.rs", "try_power_word_shield"),
    ("priest.rs", "try_shadowform"),
    ("priest_discipline.rs", "try_pain_suppression"),
    ("priest_discipline.rs", "try_power_infusion"),
    ("priest_holy.rs", "try_guardian_spirit"),
    ("priest_holy.rs", "try_holy_nova"),
    ("priest_shadow.rs", "channel_mind_flay"),
    ("priest_shadow.rs", "try_silence"),
    ("priest_shadow.rs", "try_vampiric_embrace"),
    ("rogue.rs", "try_ambush"),
    ("rogue.rs", "try_cheap_shot"),
    ("rogue.rs", "try_expose_armor"),
    ("rogue.rs", "try_hemorrhage"),
    ("rogue.rs", "try_kidney_shot"),
    ("rogue.rs", "try_rupture"),
    ("rogue.rs", "try_shadow_dance"),
    ("rogue.rs", "try_sinister_strike"),
    ("shaman.rs", "try_totem"),
    ("warlock.rs", "try_amplify_curse"),
    ("warlock.rs", "try_cast_curse"),
    ("warlock.rs", "try_corruption"),
    ("warlock.rs", "try_death_coil"),
    ("warlock.rs", "try_drain_life"),
    ("warlock.rs", "try_siphon_life"),
    ("warrior.rs", "try_battle_shout"),
    ("warrior.rs", "try_commanding_shout"),
    ("warrior.rs", "try_demoralizing_shout"),
    ("warrior.rs", "try_heroic_strike"),
    ("warrior.rs", "try_mortal_strike"),
    ("warrior.rs", "try_rend"),
];

/// The other lowering sites, as (file, fn), each with how it stays at or
/// above zero.
const DRAINS: &[(&str, &str, &str)] = &[
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_casting",
        "a cast with a cast bar pays as it lands; one its caster can no longer \
         pay for fails instead",
    ),
    (
        "src/states/play_match/effects/mana_burn.rs",
        "process_mana_burn",
        "burns at most what its target has",
    ),
];

/// Every (file, enclosing fn) in non-test `src/` code where `pattern` matches
/// and `keep(first capture group, rest of the statement)` holds.
fn sites(pattern: &Regex, keep: impl Fn(&str, &str) -> bool) -> BTreeSet<(String, String)> {
    let fn_item = Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let mut found = BTreeSet::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for m in pattern.captures_iter(&code) {
            let whole = m.get(0).unwrap();
            let statement_end = code[whole.end()..]
                .find(';')
                .map_or(code.len(), |i| whole.end() + i);
            let group = m.get(1).map_or("", |g| g.as_str());
            if !keep(group, &code[whole.end()..statement_end]) {
                continue;
            }
            let enclosing = fn_item
                .captures_iter(&code[..whole.start()])
                .last()
                .map_or_else(|| "<module>".to_string(), |c| c[1].to_string());
            found.insert((file.rel_display(), enclosing));
        }
    }
    found
}

fn declared() -> BTreeSet<(String, String)> {
    let spends = SPENDS
        .iter()
        .map(|(file, f)| (format!("{CLASS_AI}/{file}"), f.to_string()));
    let others = DRAINS
        .iter()
        .map(|(file, f, _)| (file.to_string(), f.to_string()));
    spends.chain(others).collect()
}

/// Every subtractive or zeroing write to `current_mana` in non-test `src/`.
fn lowering_sites() -> BTreeSet<(String, String)> {
    // `current_mana -=`, or `current_mana =` whose right-hand side subtracts
    // or is a zero literal. The right-hand side runs to the next `;`, which
    // for a closure body is past the closure, so a `->` there is not a
    // subtraction.
    let write = Regex::new(r"\bcurrent_mana\s*(-=|=[^=])").unwrap();
    let zero = Regex::new(r"^\s*0(\.0*)?\s*$").unwrap();
    sites(&write, |op, rhs| {
        op == "-=" || rhs.replace("->", "").contains('-') || zero.is_match(rhs)
    })
}

#[test]
fn the_resource_goes_down_only_at_named_sites() {
    let found = lowering_sites();
    let expected = declared();
    let extra: Vec<_> = found.difference(&expected).collect();
    let missing: Vec<_> = expected.difference(&found).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "The set of sites that lower `current_mana` changed. A new spend names itself \
         in SPENDS (a class AI decision cleared by `cast_gate`) or DRAINS (with how it \
         stays at or above zero); never clamp the resource to hide one (AS-438).\n  \
         found but not declared: {extra:?}\n  declared but not found: {missing:?}"
    );
}

/// Every function in the class AI that begins a cast with a cast bar
/// (`CastingState::new`), as (file, fn). The scene builder's `casts` stages
/// one for a test.
const CAST_STARTERS: &[(&str, &str)] = &[
    ("druid.rs", "cast"),
    ("hunter.rs", "try_aimed_shot"),
    ("mage.rs", "try_frostbolt_on"),
    ("mage.rs", "try_polymorph_on"),
    ("paladin.rs", "try_flash_of_light"),
    ("paladin.rs", "try_holy_light"),
    ("priest.rs", "cast_mana_burn"),
    ("priest.rs", "try_flash_heal"),
    ("priest.rs", "try_mind_blast"),
    ("scene.rs", "casts"),
    ("shaman.rs", "frost_shock_on"),
    ("shaman.rs", "try_lesser_healing_wave"),
    ("shaman.rs", "try_lightning_bolt"),
    ("warlock.rs", "cast_instant_hit"),
    ("warlock.rs", "try_fear"),
    ("warlock.rs", "try_immolate"),
    ("warlock.rs", "try_shadowbolt"),
    ("warlock.rs", "try_unstable_affliction"),
];

/// Cast starters outside the class AI: the animation sandbox plays a cast's
/// visuals on a dummy, outside any match, and pays nothing.
const OTHER_CAST_STARTERS: &[(&str, &str)] = &[
    ("src/states/animation_sandbox/playback.rs", "drive_playback"),
    ("src/states/animation_sandbox/playback.rs", "start_entry"),
];

#[test]
fn a_cast_with_a_cast_bar_pays_only_as_it_lands() {
    let begins = Regex::new(r"\bCastingState\s*::\s*new\s*\(").unwrap();
    let starters = sites(&begins, |_, _| true);
    let named: BTreeSet<(String, String)> = CAST_STARTERS
        .iter()
        .map(|(file, f)| (format!("{CLASS_AI}/{file}"), f.to_string()))
        .chain(
            OTHER_CAST_STARTERS
                .iter()
                .map(|(file, f)| (file.to_string(), f.to_string())),
        )
        .collect();
    let extra: Vec<_> = starters.difference(&named).collect();
    let missing: Vec<_> = named.difference(&starters).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "The set of functions that begin a cast changed; name it in CAST_STARTERS, and \
         let it pay nothing: the cast pays as it lands.\n  \
         found but not declared: {extra:?}\n  declared but not found: {missing:?}"
    );
    let lowering = lowering_sites();
    let both: Vec<_> = starters.intersection(&lowering).collect();
    assert!(
        both.is_empty(),
        "These functions begin a cast with a cast bar AND lower the caster's resource, \
         so the cast pays twice: as it begins, and again as it lands in \
         `process_casting` (AS-438). Drop the charge at the cast's start: {both:?}"
    );
}
