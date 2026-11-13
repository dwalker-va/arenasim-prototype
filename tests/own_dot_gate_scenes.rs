//! AS-476 — a damage over time is gated on its caster's OWN copy on the
//! target, never on "any DoT" or on the spell's name alone.
//!
//! A periodic effect is per caster: `apply_pending_auras` refuses no
//! `DamageOverTime`, so every caster's copy lands and ticks beside the
//! others'. A teammate's DoT on the target is therefore not this caster's,
//! and must not stop it applying its own (`CombatContext::has_own_dot`).
//!
//! Scenes (tier 1), each a pair — the target carrying a TEAMMATE's DoT (the
//! situation the old gate got wrong), and the control carrying the caster's
//! OWN, the one condition that should hold it:
//!
//! - the Warrior's Rend, against a teammate Warlock's Corruption and a second
//!   Warrior's Rend;
//! - the Warlock's Corruption, Unstable Affliction and Immolate, against a
//!   second Warlock's copy of each — and Unstable Affliction against the
//!   silence its own dispelled copy left, which shares its name and caster;
//! - the Hunter's Serpent Sting, against a second Hunter's sting — in the
//!   chain, and in the opener's "is the sting due" check.

use arenasim::states::match_config::CharacterClass::{
    Hunter, Mage, Priest, Rogue, Warlock, Warrior,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::effects::backlash::dispel_backlash_silence_aura;
use arenasim::states::play_match::{AbilityType, Aura, AuraType};

const ME: &str = "me";
const MATE: &str = "teammate";
const ENEMY: &str = "enemy";

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// A ticking DoT named `name`, cast by `who` in `scene`.
fn dot(scene: &Scene, who: &'static str, name: &str) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 12.0,
        magnitude: 5.0,
        tick_interval: 3.0,
        time_until_next_tick: 3.0,
        break_on_damage_threshold: -1.0,
        caster: Some(scene.entity(who)),
        ability_name: name.to_string(),
        ..Default::default()
    }
}

/// `scene` with `who`'s DoT `name` on the enemy.
fn with_dot(scene: Scene, who: &'static str, name: &str) -> Scene {
    let aura = dot(&scene, who, name);
    scene.aura_full(ENEMY, aura)
}

fn assert_chose(view: &DecisionView, ability: AbilityType) {
    assert_eq!(
        view.chose(),
        Some((ability, Some(ENEMY))),
        "{}",
        describe(view)
    );
}

fn assert_already_applied(view: &DecisionView, ability: AbilityType) {
    assert!(
        matches!(
            view.rejected(ability),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{ability:?} should be held by its own copy: {}",
        describe(view)
    );
}

// ---------------------------------------------------------------------------
// Warrior: Rend
// ---------------------------------------------------------------------------

/// A shouted Warrior in melee range of an enemy Priest, with rage for Rend,
/// beside a `mate` teammate standing off.
fn warrior(mate: arenasim::states::match_config::CharacterClass) -> Scene {
    Scene::new()
        .unit(ME, 1, Warrior)
        .rage(80.0)
        .targeting(ENEMY)
        .aura(ME, AuraType::AttackPowerIncrease, 120.0)
        .unit(MATE, 1, mate)
        .at(-15.0, 0.0)
        .targeting(ENEMY)
        .aura(MATE, AuraType::AttackPowerIncrease, 120.0)
        .unit(ENEMY, 2, Priest)
        .at(2.0, 0.0)
}

#[test]
fn a_teammates_corruption_does_not_stand_for_the_warriors_rend() {
    let view = with_dot(warrior(Warlock), MATE, "Corruption").decide(ME);
    assert_chose(&view, AbilityType::Rend);
}

#[test]
fn a_second_warriors_rend_does_not_stand_for_this_warriors_own() {
    let view = with_dot(warrior(Warrior), MATE, "Rend").decide(ME);
    assert_chose(&view, AbilityType::Rend);
}

/// Control: the Warrior's own Rend on the target holds it.
#[test]
fn the_warriors_own_rend_holds_its_rend() {
    let view = with_dot(warrior(Warlock), ME, "Rend").decide(ME);
    assert_already_applied(&view, AbilityType::Rend);
    assert_ne!(
        view.chose(),
        Some((AbilityType::Rend, Some(ENEMY))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Warlock: Corruption, Unstable Affliction, Immolate
// ---------------------------------------------------------------------------

/// A Warlock 25yd from an enemy Mage (no dispeller, so Corruption leads the
/// chain), beside a second Warlock standing off.
fn warlock() -> Scene {
    Scene::new()
        .unit(ME, 1, Warlock)
        .targeting(ENEMY)
        .unit(MATE, 1, Warlock)
        .at(0.0, -10.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Mage)
        .at(25.0, 0.0)
}

#[test]
fn a_second_warlocks_corruption_does_not_stand_for_this_warlocks_own() {
    let view = with_dot(warlock(), MATE, "Corruption").decide(ME);
    assert_chose(&view, AbilityType::Corruption);
}

/// Control: its own Corruption holds it, and the chain moves on to Unstable
/// Affliction.
#[test]
fn the_warlocks_own_corruption_holds_its_corruption() {
    let view = with_dot(warlock(), ME, "Corruption").decide(ME);
    assert_already_applied(&view, AbilityType::Corruption);
    assert_chose(&view, AbilityType::UnstableAffliction);
}

#[test]
fn a_second_warlocks_unstable_affliction_does_not_stand_for_this_warlocks_own() {
    let s = with_dot(warlock(), ME, "Corruption");
    let view = with_dot(s, MATE, "Unstable Affliction").decide(ME);
    assert_chose(&view, AbilityType::UnstableAffliction);
}

#[test]
fn the_warlocks_own_unstable_affliction_holds_it() {
    let s = with_dot(warlock(), ME, "Corruption");
    let view = with_dot(s, ME, "Unstable Affliction").decide(ME);
    assert_already_applied(&view, AbilityType::UnstableAffliction);
}

/// A dispelled Unstable Affliction silences its dispeller with an aura of the
/// same name and the same caster (`dispel_backlash_silence_aura`). That is
/// not the DoT: the Warlock puts Unstable Affliction back on the dispeller.
#[test]
fn the_backlash_silence_does_not_stand_for_the_warlocks_unstable_affliction() {
    let s = with_dot(warlock(), ME, "Corruption");
    let silence = dispel_backlash_silence_aura(Some(s.entity(ME)), 5.0);
    let view = s.aura_full(ENEMY, silence).decide(ME);
    assert_chose(&view, AbilityType::UnstableAffliction);
}

/// Corruption, Unstable Affliction and the curse are all this Warlock's own,
/// so the chain reaches Immolate.
fn warlock_at_immolate() -> Scene {
    let s = with_dot(warlock(), ME, "Corruption");
    let s = with_dot(s, ME, "Unstable Affliction");
    with_dot(s, ME, "Curse of Agony")
}

#[test]
fn a_second_warlocks_immolate_does_not_stand_for_this_warlocks_own() {
    let view = with_dot(warlock_at_immolate(), MATE, "Immolate").decide(ME);
    assert_chose(&view, AbilityType::Immolate);
}

#[test]
fn the_warlocks_own_immolate_holds_it() {
    let view = with_dot(warlock_at_immolate(), ME, "Immolate").decide(ME);
    assert_already_applied(&view, AbilityType::Immolate);
}

// ---------------------------------------------------------------------------
// Hunter: Serpent Sting
// ---------------------------------------------------------------------------

/// A Hunter 25yd from a lone enemy Priest, which cannot stop an Aimed Shot,
/// so the opener is the sting (`against_a_lone_priest_the_opener_is_serpent_sting`),
/// beside a second Hunter standing off.
fn hunter() -> Scene {
    Scene::new()
        .unit(ME, 1, Hunter)
        .targeting(ENEMY)
        .unit(MATE, 1, Hunter)
        .at(0.0, -10.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Priest)
        .at(25.0, 0.0)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
}

#[test]
fn a_second_hunters_sting_does_not_stand_for_this_hunters_own() {
    let view = with_dot(hunter(), MATE, "Serpent Sting").decide(ME);
    assert_chose(&view, AbilityType::SerpentSting);
}

#[test]
fn the_hunters_own_sting_holds_its_sting() {
    let view = with_dot(hunter(), ME, "Serpent Sting").decide(ME);
    assert_already_applied(&view, AbilityType::SerpentSting);
}

/// The opener asks whether the sting is due before it puts Aimed Shot ahead
/// of it: a Rogue 30yd off running in reaches the dead zone after a cast
/// begun now and before one begun after the sting's global cooldown, so a
/// DUE sting yields the opener to Aimed Shot. A second Hunter's sting leaves
/// this Hunter's due; its own does not.
fn hunter_opener() -> Scene {
    let mut s = Scene::new()
        .unit(ME, 1, Hunter)
        .targeting(ENEMY)
        .unit(MATE, 1, Hunter)
        .at(0.0, -10.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Rogue)
        .at(30.0, 0.0)
        .facing(-1.0, 0.0)
        .targeting(ME)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0);
    s.combatant_mut(ENEMY).stealthed = false;
    s
}

fn considered(view: &DecisionView, ability: AbilityType) -> bool {
    view.candidates.iter().any(|c| c.ability == ability)
}

/// The opener's Aimed Shot goes ahead of the due sting: chosen before the
/// chain ever reaches Serpent Sting.
#[test]
fn a_second_hunters_sting_leaves_the_opener_sting_due() {
    let view = with_dot(hunter_opener(), MATE, "Serpent Sting").decide(ME);
    assert_chose(&view, AbilityType::AimedShot);
    assert!(
        !considered(&view, AbilityType::SerpentSting),
        "the opener, not the chain, should have chosen it: {}",
        describe(&view)
    );
}

/// Control: its own sting on the Rogue is not due, so the opener passes and
/// the chain reaches the sting, which its own copy holds.
#[test]
fn the_hunters_own_sting_is_not_due_at_the_opener() {
    let view = with_dot(hunter_opener(), ME, "Serpent Sting").decide(ME);
    assert_already_applied(&view, AbilityType::SerpentSting);
}
