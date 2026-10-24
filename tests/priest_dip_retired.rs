//! AS-388 — under `TeamPlan` the Priest's offensive Psychic Scream dip is
//! retired. The offensive Scream is the Peel play's step alone: it fears an
//! attacker of the teammate in danger that is already inside the Scream, and
//! the Priest never walks to bring one into it. `Legacy` keeps the dip.
//!
//! Scenes (tier 1), each paired with its one condition removed:
//!
//! - the dip: `Legacy` dips at an unheld enemy healer, `TeamPlan` does not;
//! - the Peel step: an attacker inside the Scream is screamed where the
//!   Priest stands; beyond it, the Priest neither screams nor walks at it;
//! - the defensive Scream fires under `TeamPlan` for a Priest with a melee
//!   on it, and not with the melee elsewhere.
//!
//! The dip's dying-teammate abort (AS-371) is `Legacy`'s and is pinned in
//! `tests/priest_dip_abort_band.rs`.

use arenasim::states::match_config::CharacterClass::{Priest, Shaman, Warlock, Warrior};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::MovementGoal;
use arenasim::states::play_match::decision_trace::MovementTrigger;
use arenasim::states::play_match::AuraType;

const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const ATTACKER: &str = "enemy warrior";
const HEALER: &str = "enemy healer";
const WARLOCK: &str = "enemy warlock";

const NOW: f32 = 30.0;

/// Below `urgency_hp_threshold` (0.5): with a melee on it, in danger.
const DYING: f32 = 0.3;

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

fn dipped(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::DipEnter)
}

/// The standalone dip is retired under `TeamPlan`: a healthy Warrior teammate
/// attacking nobody, an enemy Shaman 35yd east that nothing holds — the
/// `Legacy` Priest dips at it, the `TeamPlan` Priest does not.
#[test]
fn the_dip_is_retired_under_team_plan() {
    let dip_scene = |profile: AiProfile| {
        Scene::new()
            .now(NOW)
            .ai_profile(1, profile)
            .unit(PRIEST, 1, Priest)
            .at(-20.0, 0.0)
            .unit(WARRIOR, 1, Warrior)
            .at(-24.0, 0.0)
            .unit(HEALER, 2, Shaman)
            .at(15.0, 0.0)
    };
    let s = dip_scene(AiProfile::TeamPlan);
    let view = s.decide(PRIEST);
    assert!(!dipped(&view), "TeamPlan: {}", describe(&view));
    assert_ne!(
        view.directive.map(|d| d.goal),
        Some(MovementGoal::Entity(s.entity(HEALER))),
        "TeamPlan walks at the healer: {}",
        describe(&view)
    );

    let view = dip_scene(AiProfile::Legacy).decide(PRIEST);
    assert!(dipped(&view), "Legacy: {}", describe(&view));
}

/// The Priest at `priest`; its Warrior teammate at (10, 0), dying, with an
/// enemy Warrior on it at (11.5, 0). An enemy Shaman stands far off at
/// (30, 30), out of every reach.
fn peel_scene(priest: (f32, f32)) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(PRIEST, 1, Priest)
        .at(priest.0, priest.1)
        .unit(WARRIOR, 1, Warrior)
        .at(10.0, 0.0)
        .health(DYING)
        .targeting(ATTACKER)
        .unit(ATTACKER, 2, Warrior)
        .at(11.5, 0.0)
        .targeting(WARRIOR)
        .unit(HEALER, 2, Shaman)
        .at(30.0, 30.0)
}

/// The Peel step's Scream is kept: 6.5yd from the attacker on its dying
/// teammate, the `TeamPlan` Priest screams where it stands. 31.5yd from it,
/// it neither screams nor walks at it.
#[test]
fn the_peel_step_screams_only_an_attacker_in_range() {
    let view = peel_scene((5.0, 0.0)).decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((AbilityType::PsychicScream, None)),
        "in range: {}",
        describe(&view)
    );

    let s = peel_scene((-20.0, 0.0));
    let view = s.decide(PRIEST);
    assert_ne!(
        view.chose(),
        Some((AbilityType::PsychicScream, None)),
        "beyond: {}",
        describe(&view)
    );
    assert_ne!(
        view.directive.map(|d| d.goal),
        Some(MovementGoal::Entity(s.entity(ATTACKER))),
        "beyond, it walks at the attacker: {}",
        describe(&view)
    );
    assert!(!dipped(&view), "beyond: {}", describe(&view));
}

/// The defensive Scream is untouched: a `TeamPlan` Priest with an enemy
/// Warrior on it screams in its own defence, with an enemy Warlock's Fear in
/// reach. With that Warrior on its teammate, 6yd off, it does not.
#[test]
fn the_defensive_scream_still_fires_under_team_plan() {
    let defence = |on: &'static str| {
        Scene::new()
            .now(NOW)
            .ai_profile(1, AiProfile::TeamPlan)
            .unit(PRIEST, 1, Priest)
            .at(-20.0, 0.0)
            .unit(WARRIOR, 1, Warrior)
            .at(-24.0, 0.0)
            .unit(HEALER, 2, Shaman)
            .at(5.0, 0.0)
            .targeting(PRIEST)
            .unit(WARLOCK, 2, Warlock)
            .at(5.0, 5.0)
            .unit(ATTACKER, 2, Warrior)
            .at(-18.0, 0.0)
            .targeting(on)
            .aura(PRIEST, AuraType::MaxHealthIncrease, 60.0)
            .aura(WARRIOR, AuraType::MaxHealthIncrease, 60.0)
            .decide(PRIEST)
    };
    let view = defence(PRIEST);
    assert_eq!(
        view.chose(),
        Some((AbilityType::PsychicScream, None)),
        "{}",
        describe(&view)
    );
    let view = defence(WARRIOR);
    assert_ne!(
        view.chose(),
        Some((AbilityType::PsychicScream, None)),
        "{}",
        describe(&view)
    );
}
