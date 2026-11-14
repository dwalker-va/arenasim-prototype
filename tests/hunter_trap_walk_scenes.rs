//! AS-498 prototype scenes: the Hunter's trap walk (`hunter_dip::
//! evaluate_trap_walk`), switched on by `hunter.trap_walk_worth` (here 0.0:
//! a walk may cost no melee contact the kite would have prevented).
//!
//! Each pair is one scene and its control with one condition changed:
//!
//! - the walk: an enemy healer beyond throw range, the enemy Warrior chasing
//!   the Hunter from BEHIND (the walk runs away from it too) — the Hunter
//!   walks; with the Warrior AHEAD, between the Hunter and the healer, the
//!   walk would run into it, and the Hunter does not walk;
//! - the outlast trap: the team is killing the enemy Druid, which cannot free
//!   its Warrior from a Freezing Trap, so the Warrior on the Hunter is frozen;
//!   against a Priest, which can, it is not.

use arenasim::states::match_config::CharacterClass;
use arenasim::states::match_config::CharacterClass::{Druid, Hunter, Priest, Warrior};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{KitePosture, MovementGoal};
use arenasim::states::play_match::decision_trace::MovementTrigger;
use arenasim::states::play_match::movement_config::load_movement_config;

const ME: &str = "me";
const PARTNER: &str = "partner";
const WARRIOR: &str = "enemy warrior";
const HEALER: &str = "enemy healer";

const NOW: f32 = 30.0;

fn walk_scene() -> Scene {
    let mut movement = load_movement_config().expect("movement.ron loads");
    movement.hunter.trap_walk_worth = 0.0;
    Scene::with_config(
        load_ability_definitions().expect("abilities.ron loads"),
        movement,
    )
    .now(NOW)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; movement {:?}; directive {:?}; candidates {:?}",
        view.chosen,
        view.movement,
        view.directive.map(|d| d.goal),
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// The Hunter at the origin, its Warrior partner on the enemy Warrior, the
/// enemy Priest 40yd off along +X (beyond throw range), and the enemy
/// Warrior at `warrior_x` on the axis, chasing the Hunter.
fn chase(warrior_x: f32) -> Scene {
    walk_scene()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .kite_posture(ME, KitePosture::new(NOW))
        .unit(PARTNER, 1, Warrior)
        .at(-30.0, 10.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(warrior_x, 0.0)
        .facing(-warrior_x.signum(), 0.0)
        .targeting(ME)
        .unit(HEALER, 2, Priest)
        .at(40.0, 0.0)
}

fn walked(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::DipEnter)
        && matches!(
            view.directive.map(|d| d.goal),
            Some(MovementGoal::Entity(_))
        )
}

#[test]
fn the_hunter_walks_at_the_healer_when_the_chaser_is_behind() {
    let view = chase(-15.0).decide(ME);
    assert!(walked(&view), "{}", describe(&view));
}

#[test]
fn the_hunter_does_not_walk_into_a_chaser_between_it_and_the_healer() {
    let view = chase(15.0).decide(ME);
    assert!(
        !view
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::DipEnter),
        "{}",
        describe(&view)
    );
}

/// The Hunter at the origin with the enemy Warrior on it, its partner killing
/// the enemy `healer` 30yd off.
fn outlast(healer: CharacterClass) -> Scene {
    walk_scene()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .kite_posture(ME, KitePosture::new(NOW))
        .unit(PARTNER, 1, Warrior)
        .at(28.0, 8.0)
        .targeting(HEALER)
        .unit(WARRIOR, 2, Warrior)
        .at(6.0, 0.0)
        .facing(-1.0, 0.0)
        .targeting(ME)
        .unit(HEALER, 2, healer)
        .at(30.0, 8.0)
        .targeting(PARTNER)
}

fn froze_warrior(view: &DecisionView) -> bool {
    view.choices
        .contains(&(AbilityType::FreezingTrap, Some(WARRIOR)))
}

#[test]
fn the_outlast_trap_freezes_a_warrior_its_druid_cannot_free() {
    let view = outlast(Druid).decide(ME);
    assert!(froze_warrior(&view), "{}", describe(&view));
}

#[test]
fn the_outlast_trap_leaves_a_warrior_its_priest_would_free() {
    let view = outlast(Priest).decide(ME);
    assert!(view.acted, "{}", describe(&view));
    assert!(!froze_warrior(&view), "{}", describe(&view));
}
