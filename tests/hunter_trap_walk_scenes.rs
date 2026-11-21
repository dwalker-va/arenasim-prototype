//! Scenes for the Hunter's trap walk (AS-498, `hunter_dip::evaluate_trap_walk`):
//! under `TeamPlan` the Hunter walks toward the enemy it will Freezing Trap
//! whenever the walk costs no melee contact that kiting away would have
//! prevented, and throws from range the moment the landing is clean.
//!
//! Each pair is one scene and its control with one condition changed:
//!
//! - the walk: the enemy healer beyond throw range and the enemy Warrior
//!   chasing the Hunter from BEHIND (walking at the healer also runs from the
//!   Warrior) — the Hunter walks; with the Warrior AHEAD, between the Hunter
//!   and the healer, it would run into it, and the Hunter holds;
//! - the teammate rule: the walk needs a DPS teammate attacking someone, who
//!   uses the window the freeze buys; with that teammate attacking nobody, the
//!   Hunter holds;
//! - the outlast trap: the team is killing the enemy Druid, which cannot free
//!   its Warrior from a Freezing Trap, so the Warrior on the Hunter is frozen;
//!   beside a Priest, which can, it is not;
//! - the gate: the walk scene under `Legacy`, and under `TeamPlan` with the
//!   walk withheld (`team1_trap_walk: false`), walks nowhere.

use arenasim::states::match_config::CharacterClass;
use arenasim::states::match_config::CharacterClass::{Druid, Hunter, Priest, Warrior};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::hunter::{
    AIMED_SHOT_HELD_FOR_WALK, FROST_TRAP_HELD_FOR_WALK,
};
use arenasim::states::play_match::class_ai::hunter_dip::{
    TRAP_WALK_MELEE, TRAP_WALK_NO_TEAMMATE, TRAP_WALK_WALKING,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{KitePosture, MovementGoal};
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};

const ME: &str = "me";
const PARTNER: &str = "partner";
const WARRIOR: &str = "enemy warrior";
const HEALER: &str = "enemy healer";

const NOW: f32 = 30.0;

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

/// The trap walk notes the decision recorded on Freezing Trap.
fn trap_notes(view: &DecisionView) -> Vec<String> {
    view.candidates
        .iter()
        .filter(|c| c.ability == AbilityType::FreezingTrap)
        .filter_map(|c| match &c.reason {
            Some(RejectionReason::PreconditionUnmet { note }) if note.starts_with("trap walk") => {
                Some(note.clone())
            }
            _ => None,
        })
        .collect()
}

fn walked(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::TrapWalk)
}

/// The Hunter at the origin under `profile`, its Warrior partner attacking
/// the enemy Warrior (or nobody, with `partner_attacks` false), the enemy
/// Priest 40yd off along +X (beyond throw range), and the enemy Warrior at
/// `warrior_x` on the axis, chasing the Hunter.
fn chase(profile: AiProfile, warrior_x: f32, partner_attacks: bool) -> Scene {
    let scene = Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .kite_posture(ME, KitePosture::new(NOW))
        .unit(PARTNER, 1, Warrior)
        .at(-30.0, 10.0);
    let scene = if partner_attacks {
        scene.targeting(WARRIOR)
    } else {
        scene
    };
    scene
        .unit(WARRIOR, 2, Warrior)
        .at(warrior_x, 0.0)
        .facing(-warrior_x.signum(), 0.0)
        .targeting(ME)
        .unit(HEALER, 2, Priest)
        .at(40.0, 0.0)
}

#[test]
fn the_hunter_walks_at_the_healer_when_the_chaser_is_behind() {
    let view = chase(AiProfile::TeamPlan, -15.0, true).decide(ME);
    assert!(walked(&view), "{}", describe(&view));
    assert!(
        matches!(
            view.directive.map(|d| d.goal),
            Some(MovementGoal::Entity(_))
        ),
        "{}",
        describe(&view)
    );
    assert_eq!(trap_notes(&view), vec![TRAP_WALK_WALKING.to_string()]);
}

#[test]
fn the_hunter_does_not_walk_into_a_chaser_between_it_and_the_healer() {
    let view = chase(AiProfile::TeamPlan, 15.0, true).decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert_eq!(
        trap_notes(&view),
        vec![TRAP_WALK_MELEE.to_string()],
        "{}",
        describe(&view)
    );
}

#[test]
fn the_hunter_holds_when_no_dps_teammate_would_use_the_freeze() {
    let view = chase(AiProfile::TeamPlan, -15.0, false).decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert_eq!(
        trap_notes(&view),
        vec![TRAP_WALK_NO_TEAMMATE.to_string()],
        "{}",
        describe(&view)
    );
}

#[test]
fn legacy_has_no_trap_walk() {
    let view = chase(AiProfile::Legacy, -15.0, true).decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert!(trap_notes(&view).is_empty(), "{}", describe(&view));
}

#[test]
fn a_team_with_the_walk_withheld_has_no_trap_walk() {
    let view = chase(AiProfile::TeamPlan, -15.0, true)
        .without_trap_walk(1)
        .decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert!(trap_notes(&view).is_empty(), "{}", describe(&view));
}

/// The Hunter at the origin under `TeamPlan` with the enemy Warrior on it,
/// its Warrior partner killing the enemy `healer` 30yd off.
fn outlast(healer: CharacterClass) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, AiProfile::TeamPlan)
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

/// The notes `ability` was held with.
fn notes(view: &DecisionView, ability: AbilityType) -> Vec<String> {
    view.candidates
        .iter()
        .filter(|c| c.ability == ability)
        .filter_map(|c| match &c.reason {
            Some(RejectionReason::PreconditionUnmet { note }) => Some(note.clone()),
            _ => None,
        })
        .collect()
}

/// The walk keeps the shared trap cooldown: with the Warrior chasing inside
/// the closing band and Concussive Shot down, the Frost Trap peel waits while
/// the Hunter walks, and is not held when the walk is withheld.
#[test]
fn the_walk_keeps_the_trap_cooldown_from_the_frost_trap() {
    let scene = |withheld: bool| {
        let s = chase(AiProfile::TeamPlan, -15.0, true).cooldown(
            ME,
            AbilityType::ConcussiveShot,
            5.0,
        );
        if withheld {
            s.without_trap_walk(1)
        } else {
            s
        }
    };
    let walking = scene(false).decide(ME);
    assert!(walked(&walking), "{}", describe(&walking));
    assert!(
        notes(&walking, AbilityType::FrostTrap).contains(&FROST_TRAP_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&walking)
    );
    let withheld = scene(true).decide(ME);
    assert!(
        !notes(&withheld, AbilityType::FrostTrap)
            .contains(&FROST_TRAP_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&withheld)
    );
    assert!(
        withheld
            .candidates
            .iter()
            .any(|c| c.ability == AbilityType::FrostTrap),
        "the control must reach the Frost Trap: {}",
        describe(&withheld)
    );
}

/// The walk does not plant for Aimed Shot: with the chaser 25yd behind (the
/// safe-range rotation) and the shots above it down, Aimed Shot waits while the Hunter walks, and is not
/// held when the walk is withheld.
#[test]
fn the_walk_does_not_stop_for_aimed_shot() {
    let safe = || {
        chase(AiProfile::TeamPlan, -25.0, true)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
            .cooldown(ME, AbilityType::SerpentSting, 5.0)
    };
    let walking = safe().decide(ME);
    assert!(walked(&walking), "{}", describe(&walking));
    assert!(
        notes(&walking, AbilityType::AimedShot).contains(&AIMED_SHOT_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&walking)
    );
    let withheld = safe().without_trap_walk(1).decide(ME);
    assert!(
        !notes(&withheld, AbilityType::AimedShot)
            .contains(&AIMED_SHOT_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&withheld)
    );
    assert!(
        withheld
            .candidates
            .iter()
            .any(|c| c.ability == AbilityType::AimedShot),
        "the control must reach Aimed Shot: {}",
        describe(&withheld)
    );
}

/// The walk throws from range: with no melee near and the enemy healer 22yd
/// off, the clean landing is taken at once under `TeamPlan`; the old dip
/// (`Legacy`) walks to plant range instead and throws nothing now.
#[test]
fn the_walk_throws_from_range_where_the_old_dip_walks_in() {
    let scene = |profile: AiProfile| {
        Scene::new()
            .now(NOW)
            .ai_profile(1, profile)
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(WARRIOR)
            .kite_posture(ME, KitePosture::new(NOW))
            .unit(PARTNER, 1, Warrior)
            .at(-45.0, 0.0)
            .targeting(WARRIOR)
            .unit(WARRIOR, 2, Warrior)
            .at(-40.0, 0.0)
            .targeting(PARTNER)
            .unit(HEALER, 2, Priest)
            .at(22.0, 0.0)
    };
    let teamplan = scene(AiProfile::TeamPlan).decide(ME);
    assert!(
        teamplan
            .choices
            .contains(&(AbilityType::FreezingTrap, Some(HEALER))),
        "{}",
        describe(&teamplan)
    );
    let legacy = scene(AiProfile::Legacy).decide(ME);
    assert!(
        !legacy
            .choices
            .iter()
            .any(|(a, _)| *a == AbilityType::FreezingTrap),
        "{}",
        describe(&legacy)
    );
    assert!(
        legacy
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::DipEnter),
        "the control is the old dip walking in: {}",
        describe(&legacy)
    );
}
