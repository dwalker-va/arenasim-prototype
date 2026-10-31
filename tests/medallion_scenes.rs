//! AS-305 — when a held wearer breaks free with the Medallion
//! (`class_ai::on_use`, the crowd-control break's one AI rule). Each scene
//! runs the real held-unit dispatch (`combat_ai::dispatch_while_held`, the
//! entry point `decide_abilities` runs for a stunned, feared, polymorphed or
//! incapacitated unit) for one tick, paired with its control: the same scene
//! with the trigger's one condition removed.

use arenasim::states::match_config::CharacterClass::{
    Mage, Paladin, Priest, Rogue, Warlock, Warrior,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::equipment::ItemId::MedallionOfTheAlliance as MEDALLION;
use arenasim::states::play_match::{AbilityType, AuraType};

const WEARER: &str = "wearer";
const PRIEST: &str = "priest";
const ALLY: &str = "ally";
const ROGUE: &str = "rogue";
const WARLOCK: &str = "warlock";

/// A Warrior wearing the Medallion, stunned for `stun` seconds beside its
/// Priest at `priest_hp`, against a Rogue and a Warlock.
fn teammate_scene(stun: f32, priest_hp: f32) -> Scene {
    Scene::new()
        .unit(WEARER, 1, Warrior)
        .on_use_item(WEARER, MEDALLION)
        .aura(WEARER, AuraType::Stun, stun)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 10.0)
        .health(priest_hp)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
        .unit(WARLOCK, 2, Warlock)
        .at(0.0, -25.0)
}

fn used(view: &DecisionView) -> bool {
    view.items_used == vec![(WEARER, MEDALLION)]
        && matches!(view.item_decisions.as_slice(), [(MEDALLION, None)])
}

fn held_for_precondition(view: &DecisionView) -> bool {
    view.items_used.is_empty()
        && matches!(
            view.item_decisions.as_slice(),
            [(MEDALLION, Some(RejectionReason::PreconditionUnmet { .. }))]
        )
}

#[test]
fn a_low_teammate_breaks_a_long_hold() {
    let view = teammate_scene(4.0, 0.3).decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    assert!(view.acted);
    // Control: the teammate is healthy.
    let view = teammate_scene(4.0, 0.9).decide_held(WEARER);
    assert!(held_for_precondition(&view), "{view:?}");
    assert!(!view.acted);
}

#[test]
fn a_hold_about_to_end_is_not_worth_the_break() {
    // 2.0s is the floor, inclusive.
    assert!(used(&teammate_scene(2.0, 0.3).decide_held(WEARER)));
    let view = teammate_scene(1.9, 0.3).decide_held(WEARER);
    assert!(held_for_precondition(&view), "{view:?}");
}

#[test]
fn every_hold_counts_toward_the_floor_but_a_root_alone_does_not() {
    // A 1s stun under a 6s polymorph: the polymorph is the hold left.
    let view = teammate_scene(1.0, 0.3)
        .aura(WEARER, AuraType::Polymorph, 6.0)
        .decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    // A 1s stun beside a 6s root: the root holds nothing, so 1s is left.
    let view = teammate_scene(1.0, 0.3)
        .aura(WEARER, AuraType::Root, 6.0)
        .decide_held(WEARER);
    assert!(held_for_precondition(&view), "{view:?}");
}

/// A Warrior wearing the Medallion at `hp`, stunned with 0.5s left, beside a
/// healthy Mage (no healer to save it), against a Rogue and a Warlock.
fn kill_target_scene(hp: f32) -> Scene {
    Scene::new()
        .unit(WEARER, 1, Warrior)
        .health(hp)
        .on_use_item(WEARER, MEDALLION)
        .aura(WEARER, AuraType::Stun, 0.5)
        .unit(ALLY, 1, Mage)
        .at(0.0, 20.0)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
        .unit(WARLOCK, 2, Warlock)
        .at(0.0, -25.0)
        .targeting(WEARER)
}

#[test]
fn the_enemys_kill_target_breaks_whatever_is_left() {
    // Below `kill_hp_threshold` (0.35) with no healer of its own: the enemy's
    // kill window names it.
    let scene = kill_target_scene(0.2);
    let view = scene.decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    // Control: healthy, so no kill window, and its teammate is healthy too.
    let view = kill_target_scene(0.9).decide_held(WEARER);
    assert!(held_for_precondition(&view), "{view:?}");
}

#[test]
fn a_healer_that_can_reach_it_closes_the_kill_window() {
    // The same low Warrior, with a Priest in reach: it can be healed, so it is
    // not the enemy's kill target — and the Priest is healthy, so nothing
    // fires. Pins that the trigger is the enemy's window, not low health.
    let view = Scene::new()
        .unit(WEARER, 1, Warrior)
        .health(0.2)
        .on_use_item(WEARER, MEDALLION)
        .aura(WEARER, AuraType::Stun, 0.5)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 10.0)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
        .unit(WARLOCK, 2, Warlock)
        .at(0.0, -25.0)
        .decide_held(WEARER);
    assert!(held_for_precondition(&view), "{view:?}");
}

#[test]
fn a_spent_medallion_is_not_asked_again() {
    let mut scene = teammate_scene(4.0, 0.3);
    scene.combatant_mut(WEARER).on_use_items[0].spent = true;
    let view = scene.decide_held(WEARER);
    assert!(view.items_used.is_empty(), "{view:?}");
    assert!(view.item_decisions.is_empty(), "{view:?}");
    assert!(!view.acted);
}

#[test]
fn a_unit_without_one_decides_nothing_about_items() {
    let view = Scene::new()
        .unit(WEARER, 1, Warrior)
        .aura(WEARER, AuraType::Stun, 4.0)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 10.0)
        .health(0.3)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .decide_held(WEARER);
    assert!(view.item_decisions.is_empty() && view.items_used.is_empty());
}

/// A Paladin, stunned 4s beside a Priest at 30%: Divine Shield's teammate
/// trigger holds too.
fn paladin_scene(medallion: bool) -> Scene {
    let scene = Scene::new().unit(WEARER, 1, Paladin);
    let scene = if medallion {
        scene.on_use_item(WEARER, MEDALLION)
    } else {
        scene
    };
    scene
        .aura(WEARER, AuraType::Stun, 4.0)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 10.0)
        .health(0.3)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
}

#[test]
fn the_medallion_goes_before_a_class_break() {
    // Wearing it, the Paladin breaks free with the Medallion and keeps its
    // bubble...
    let view = paladin_scene(true).decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    assert_eq!(view.chose(), None, "{view:?}");
    // ...and without it, the same hold is broken by Divine Shield.
    let view = paladin_scene(false).decide_held(WEARER);
    assert_eq!(view.chose().map(|c| c.0), Some(AbilityType::DivineShield));
    assert!(view.items_used.is_empty());
}
