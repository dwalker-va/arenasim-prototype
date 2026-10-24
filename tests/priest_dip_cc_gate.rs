//! AS-383 — under `TeamPlan`, the Priest's Psychic Scream dip is held back
//! when it would walk into enemy crowd control, and the trace says why.
//!
//! Two clauses (`class_ai::priest::dip_hold`), each a scene paired with its
//! one condition removed:
//!
//! - the dip target already targets the Priest from within
//!   `threat_intent_radius` (the 12yd pressure onset would end the dip short
//!   of scream range);
//! - the spot the dip ends on, scream range short of the target, lies in the
//!   reach of an enemy crowd control ready to land on the Priest — any
//!   source's but the dip target's own.
//!
//! Under `Legacy` the same scenes dip as before.

use arenasim::states::match_config::CharacterClass::{Priest, Shaman, Warlock, Warrior};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};

const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const HEALER: &str = "enemy healer";
const WARLOCK: &str = "enemy warlock";

const NOW: f32 = 30.0;

/// The Priest at (-20, 0) on team 1 with a Warrior teammate beside it that
/// attacks nobody, and an enemy healer of `healer_class` at `healer`.
fn scene(profile: AiProfile, healer_class: arenasim::CharacterClass, healer: (f32, f32)) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(PRIEST, 1, Priest)
        .at(-20.0, 0.0)
        .unit(WARRIOR, 1, Warrior)
        .at(-24.0, 0.0)
        .unit(HEALER, 2, healer_class)
        .at(healer.0, healer.1)
}

fn dipped(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::DipEnter)
}

/// The note the gate traced on Psychic Scream, if any.
fn held(view: &DecisionView) -> Option<String> {
    view.candidates
        .iter()
        .filter(|c| c.ability == AbilityType::PsychicScream)
        .filter_map(|c| match &c.reason {
            Some(RejectionReason::PreconditionUnmet { note })
                if note.starts_with("scream dip held") =>
            {
                Some(note.clone())
            }
            _ => None,
        })
        .next()
}

fn describe(view: &DecisionView) -> String {
    format!(
        "movement {:?}; candidates {:?}",
        view.movement, view.candidates
    )
}

fn assert_dips(view: &DecisionView, what: &str) {
    assert!(
        dipped(view) && held(view).is_none(),
        "{what}: {}",
        describe(view)
    );
}

fn assert_held(view: &DecisionView, note: &str, what: &str) {
    assert!(!dipped(view), "{what}: {}", describe(view));
    let traced = held(view).unwrap_or_else(|| panic!("{what}: no hold traced: {}", describe(view)));
    assert!(traced.contains(note), "{what}: {traced}");
}

/// An enemy Shaman 25yd off, inside the 30yd intent radius but beyond the 12yd
/// danger radius, so it pressures nothing: targeting the Priest it holds the
/// dip back; targeting the Warrior, or the Priest from 35yd, it does not.
#[test]
fn a_dip_target_already_on_the_priest_holds_the_dip() {
    let on_priest = scene(AiProfile::TeamPlan, Shaman, (5.0, 0.0))
        .target(HEALER, PRIEST)
        .decide(PRIEST);
    assert_held(
        &on_priest,
        "already targets the Priest",
        "on the Priest at 25yd",
    );

    let elsewhere = scene(AiProfile::TeamPlan, Shaman, (5.0, 0.0))
        .target(HEALER, WARRIOR)
        .decide(PRIEST);
    assert_dips(&elsewhere, "on the Warrior");

    let far = scene(AiProfile::TeamPlan, Shaman, (15.0, 0.0))
        .target(HEALER, PRIEST)
        .decide(PRIEST);
    assert_dips(&far, "on the Priest from 35yd");
}

/// The dip at an enemy Shaman 35yd east ends 8yd short of it, at (7, 0). An
/// enemy Warlock at (30, 0) — 23yd from there, 50yd from the Priest — has its
/// 30yd Fear ready: the dip is held. With the Fear (and the Warlock's other
/// hold, Death Coil) on cooldown, or the Warlock 38yd from the arrival, it is
/// not.
#[test]
fn an_arrival_in_ready_crowd_control_reach_holds_the_dip() {
    let warlock = |s: Scene, x: f32| s.unit(WARLOCK, 2, Warlock).at(x, 0.0);

    let ready = warlock(scene(AiProfile::TeamPlan, Shaman, (15.0, 0.0)), 30.0).decide(PRIEST);
    assert_held(&ready, "in reach of a ready Fear", "Fear ready");

    let cooling = warlock(scene(AiProfile::TeamPlan, Shaman, (15.0, 0.0)), 30.0)
        .cooldown(WARLOCK, AbilityType::Fear, 20.0)
        .cooldown(WARLOCK, AbilityType::DeathCoil, 20.0)
        .decide(PRIEST);
    assert_dips(&cooling, "Fear and Death Coil on cooldown");

    let beyond = warlock(scene(AiProfile::TeamPlan, Shaman, (15.0, 0.0)), 45.0).decide(PRIEST);
    assert_dips(&beyond, "the arrival beyond the Fear's reach");
}

/// The dip target's own crowd control does not hold it: an enemy Priest's
/// Psychic Scream covers every spot the dip can end on, and the dip is the
/// walk into it.
#[test]
fn the_dip_targets_own_crowd_control_does_not_hold_it() {
    let view = scene(AiProfile::TeamPlan, Priest, (15.0, 0.0)).decide(PRIEST);
    assert_dips(&view, "an enemy Priest's own Scream");
}

/// `Legacy` dips in every scene the gate holds under `TeamPlan`.
#[test]
fn legacy_dips_as_before() {
    let on_priest = scene(AiProfile::Legacy, Shaman, (5.0, 0.0))
        .target(HEALER, PRIEST)
        .decide(PRIEST);
    assert_dips(&on_priest, "Legacy, target on the Priest");

    let fear = scene(AiProfile::Legacy, Shaman, (15.0, 0.0))
        .unit(WARLOCK, 2, Warlock)
        .at(30.0, 0.0)
        .decide(PRIEST);
    assert_dips(&fear, "Legacy, arrival in a ready Fear's reach");
}
