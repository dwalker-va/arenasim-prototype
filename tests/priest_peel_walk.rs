//! AS-388 — under `TeamPlan` the Priest's offensive Psychic Scream is the Peel
//! play's: the standalone scream dip is retired, and the Priest walks at an
//! attacker of its teammate in danger to Scream it
//! (`class_ai::priest::peel_walk`), behind AS-383's scream gate
//! (`class_ai::priest::dip_hold`, reading `cc_reach::CcField`).
//!
//! Scenes (tier 1), each paired with its one condition removed:
//!
//! - the walk, against the Peel play withheld and against `Legacy`;
//! - the walk's target: an attacker of the teammate IN DANGER, not a nearer
//!   enemy on a healthy teammate;
//! - an arrival beside the enemy healer comes first, so the Scream takes both;
//! - the gate: an arrival in a ready Fear's reach holds the walk, and is
//!   traced; with the Fear down it walks;
//! - an attacker already inside the Scream needs no walk;
//! - a pressured Priest does not walk; unpressured, it does;
//! - the Peel play's exceptions table governs the walk;
//! - the dip is retired under `TeamPlan` and kept under `Legacy`;
//! - the defensive Scream is untouched.
//!
//! The walk in time — the Priest arrives and the Scream lands on the attacker
//! and the enemy healer beside it — is the staged match in
//! `tests/priest_peel_walk_staged.rs`.

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Mage, Priest, Rogue, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::MovementGoal;
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};
use arenasim::states::play_match::situation::{PlaySet, PEEL};
use arenasim::states::play_match::AuraType;

const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const ATTACKER: &str = "enemy warrior";
const ROGUE: &str = "enemy rogue";
const HEALER: &str = "enemy healer";
const WARLOCK: &str = "enemy warlock";

const NOW: f32 = 30.0;

/// Below `urgency_hp_threshold` (0.5): with a melee on it, in danger.
const DYING: f32 = 0.3;

/// The Priest at (-20, 0); its Warrior teammate at (10, 0), dying, with an
/// enemy Warrior on it at (11.5, 0) — 31.5yd from the Priest, beyond the 8yd
/// Scream and inside the walk's reach (8yd + 6s of walking). An enemy healer
/// of `healer_class` stands far off at (30, 30), out of every reach.
fn scene(profile: AiProfile, healer_class: CharacterClass) -> Scene {
    scene_at(profile, healer_class, (-20.0, 0.0), (30.0, 30.0))
}

/// [`scene`], with the Priest at `priest` and the enemy healer at `healer`.
fn scene_at(
    profile: AiProfile,
    healer_class: CharacterClass,
    priest: (f32, f32),
    healer: (f32, f32),
) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(PRIEST, 1, Priest)
        .at(priest.0, priest.1)
        .unit(WARRIOR, 1, Warrior)
        .at(10.0, 0.0)
        .health(DYING)
        .targeting(ATTACKER)
        .unit(ATTACKER, 2, Warrior)
        .at(11.5, 0.0)
        .targeting(WARRIOR)
        .unit(HEALER, 2, healer_class)
        .at(healer.0, healer.1)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; movement {:?}; directive {:?}; situation {:?}; candidates {:?}",
        view.chosen,
        view.movement,
        view.directive.map(|d| d.goal),
        view.situation,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// Whom of `names` the Priest walks at this tick, if it began a Peel walk:
/// the walk's trace event and the directive it left must agree.
fn walks_among(scene: &Scene, view: &DecisionView, names: &[&'static str]) -> Option<&'static str> {
    let traced = view
        .movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::PeelWalk);
    let goal = match view.directive.map(|d| d.goal) {
        Some(MovementGoal::Entity(e)) => Some(e),
        _ => None,
    };
    match (traced, goal) {
        (true, Some(e)) => Some(
            names
                .iter()
                .copied()
                .find(|who| scene.entity(who) == e)
                .unwrap_or("<someone else>"),
        ),
        (true, None) => panic!("a PeelWalk event with no entity goal: {}", describe(view)),
        (false, _) => None,
    }
}

/// [`walks_among`] the base scene's one attacker.
fn walks_at(scene: &Scene, view: &DecisionView) -> Option<&'static str> {
    walks_among(scene, view, &[ATTACKER])
}

fn dipped(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::DipEnter)
}

/// The note the scream gate traced on Psychic Scream, if any.
fn held(view: &DecisionView) -> Option<String> {
    view.candidates
        .iter()
        .filter(|c| c.ability == AbilityType::PsychicScream)
        .find_map(|c| match &c.reason {
            Some(RejectionReason::PreconditionUnmet { note })
                if note.starts_with("scream walk held") =>
            {
                Some(note.clone())
            }
            _ => None,
        })
}

/// The walk: the Priest walks at the attacker on its dying teammate. With the
/// Peel play withheld from its team it does not, and under `Legacy` there is
/// no play: neither walks, and neither dips (the dying teammate defers the
/// `Legacy` dip).
#[test]
fn the_priest_walks_at_the_attacker_on_its_dying_teammate() {
    let s = scene(AiProfile::TeamPlan, Priest);
    let view = s.decide(PRIEST);
    assert!(
        view.situation.and_then(|t| t.ally_in_danger).is_some(),
        "{}",
        describe(&view)
    );
    assert_eq!(walks_at(&s, &view), Some(ATTACKER), "{}", describe(&view));
    assert!(!dipped(&view));

    let s = scene(AiProfile::TeamPlan, Priest).withhold(1, PlaySet::of(&[PEEL]));
    let view = s.decide(PRIEST);
    assert_eq!(walks_at(&s, &view), None, "withheld: {}", describe(&view));
    assert!(!dipped(&view), "withheld: {}", describe(&view));

    let s = scene(AiProfile::Legacy, Priest);
    let view = s.decide(PRIEST);
    assert_eq!(walks_at(&s, &view), None, "Legacy: {}", describe(&view));
    assert!(!dipped(&view), "Legacy: {}", describe(&view));
}

/// The walk's target is an attacker of the teammate in danger. A healthy
/// Mage teammate at (-20, -10) has an enemy Warrior on it at (-14, -10), 11.7yd
/// from the Priest and far nearer than the attacker on the dying Warrior: the
/// Priest still walks at the dying Warrior's attacker. With the Warrior
/// healthy no teammate is in danger, and it walks at nobody.
#[test]
fn the_walk_is_for_the_teammate_in_danger() {
    let with_mage = |s: Scene| {
        s.unit("mage", 1, Mage)
            .at(-20.0, -10.0)
            .unit("other attacker", 2, Warrior)
            .at(-14.0, -10.0)
            .targeting("mage")
    };
    let both = [ATTACKER, "other attacker"];
    let s = with_mage(scene(AiProfile::TeamPlan, Priest));
    let view = s.decide(PRIEST);
    assert_eq!(
        walks_among(&s, &view, &both),
        Some(ATTACKER),
        "{}",
        describe(&view)
    );

    let mut s = with_mage(scene(AiProfile::TeamPlan, Priest));
    s.set_health(WARRIOR, 0.9);
    let view = s.decide(PRIEST);
    assert!(view.situation.and_then(|t| t.ally_in_danger).is_none());
    assert_eq!(walks_among(&s, &view, &both), None, "{}", describe(&view));
}

/// Of the dying teammate's attackers, the one whose arrival puts the enemy
/// healer inside the Scream comes first. An enemy Rogue on the Warrior at
/// (0, 15) — 25yd from the Priest, nearer than the enemy Warrior — and an
/// enemy Druid at (5, 3), 3.4yd from the spot a walk at the enemy Warrior ends
/// on: the Priest walks at the enemy Warrior, to Scream it and the Druid
/// together. The Druid's own crowd control is down (the gate would hold a
/// walk into a ready Cyclone's reach). With the Druid far off, the nearer
/// Rogue is the walk's.
#[test]
fn the_walk_catches_the_enemy_healer_beside_an_attacker() {
    let both = [ATTACKER, ROGUE];
    let build = |druid: (f32, f32)| {
        let mut s = scene_at(AiProfile::TeamPlan, Druid, (-20.0, 0.0), druid)
            .unit(ROGUE, 2, Rogue)
            .at(0.0, 15.0)
            .targeting(WARRIOR)
            .cooldown(HEALER, AbilityType::Cyclone, 30.0)
            .cooldown(HEALER, AbilityType::EntanglingRoots, 30.0);
        s.combatant_mut(ROGUE).stealthed = false;
        s
    };
    let s = build((5.0, 3.0));
    let view = s.decide(PRIEST);
    assert_eq!(
        walks_among(&s, &view, &both),
        Some(ATTACKER),
        "{}",
        describe(&view)
    );

    let s = build((40.0, 40.0));
    let view = s.decide(PRIEST);
    assert_eq!(
        walks_among(&s, &view, &both),
        Some(ROGUE),
        "{}",
        describe(&view)
    );
}

/// The gate (AS-383, kept): the walk at the enemy Warrior ends 8yd short of
/// it, at (3.5, 0). An enemy Warlock at (30, 0) — 26.5yd from there — has its
/// 30yd Fear ready: the walk is held, and the trace says why. With the Fear
/// and Death Coil on cooldown, or the Warlock 38yd from the arrival, the
/// Priest walks.
#[test]
fn an_arrival_in_ready_crowd_control_reach_holds_the_walk() {
    let warlock = |s: Scene, x: f32| s.unit(WARLOCK, 2, Warlock).at(x, 0.0);

    let s = warlock(scene(AiProfile::TeamPlan, Priest), 30.0);
    let view = s.decide(PRIEST);
    assert_eq!(walks_at(&s, &view), None, "Fear ready: {}", describe(&view));
    let note = held(&view).unwrap_or_else(|| panic!("no hold traced: {}", describe(&view)));
    assert!(note.contains("in reach of a ready Fear"), "{note}");

    let s = warlock(scene(AiProfile::TeamPlan, Priest), 30.0)
        .cooldown(WARLOCK, AbilityType::Fear, 20.0)
        .cooldown(WARLOCK, AbilityType::DeathCoil, 20.0);
    let view = s.decide(PRIEST);
    assert_eq!(
        walks_at(&s, &view),
        Some(ATTACKER),
        "cooling: {}",
        describe(&view)
    );
    assert_eq!(held(&view), None);

    let s = warlock(scene(AiProfile::TeamPlan, Priest), 41.5);
    let view = s.decide(PRIEST);
    assert_eq!(
        walks_at(&s, &view),
        Some(ATTACKER),
        "beyond: {}",
        describe(&view)
    );
}

/// An attacker already inside the Scream is the peel step's this tick: the
/// Priest screams where it stands, and does not walk. Beyond the Scream it
/// walks; beyond the walk's reach (8yd + 6s at its speed) it does neither.
#[test]
fn the_walk_runs_only_between_the_scream_and_its_reach() {
    let s = scene_at(AiProfile::TeamPlan, Priest, (5.0, 0.0), (30.0, 30.0));
    let view = s.decide(PRIEST);
    assert_eq!(walks_at(&s, &view), None, "{}", describe(&view));
    assert_eq!(
        view.chose(),
        Some((AbilityType::PsychicScream, None)),
        "{}",
        describe(&view)
    );

    let s = scene_at(AiProfile::TeamPlan, Priest, (-60.0, 0.0), (30.0, 30.0));
    let view = s.decide(PRIEST);
    assert_eq!(walks_at(&s, &view), None, "{}", describe(&view));
    assert_ne!(view.chose(), Some((AbilityType::PsychicScream, None)));
}

/// The standalone dip is retired under `TeamPlan`: a healthy Warrior teammate
/// attacking nobody, an enemy Shaman 35yd east that nothing holds — the
/// `Legacy` Priest dips at it, the `TeamPlan` Priest neither dips nor walks.
/// The walk is FREE's: a pressured Priest does not walk into the enemy. An
/// enemy Warrior 10yd north of the Priest targets it — inside the 12yd danger
/// radius, so the Priest is PRESSURED, and beyond the 8yd Scream, so the
/// defensive Scream cannot fire either; its Charge is down, so it holds no
/// crowd control that would gate the walk on its own. The Priest neither
/// begins a Peel walk nor holds an `Entity` goal on the attacker. With that
/// Warrior on the dying Warrior instead (31.6yd from it, so not one of its
/// attackers), the Priest is FREE and walks.
#[test]
fn a_pressured_priest_does_not_walk() {
    const BRUISER: &str = "enemy bruiser";
    let with_bruiser = |on: &'static str| {
        scene(AiProfile::TeamPlan, Priest)
            .unit(BRUISER, 2, Warrior)
            .at(-20.0, 10.0)
            .targeting(on)
            .cooldown(BRUISER, AbilityType::Charge, 30.0)
    };
    let s = with_bruiser(PRIEST);
    let view = s.decide(PRIEST);
    assert_eq!(walks_at(&s, &view), None, "pressured: {}", describe(&view));
    assert_ne!(
        view.directive.map(|d| d.goal),
        Some(MovementGoal::Entity(s.entity(ATTACKER))),
        "pressured: {}",
        describe(&view)
    );
    assert_ne!(
        view.chose(),
        Some((AbilityType::PsychicScream, None)),
        "pressured: {}",
        describe(&view)
    );

    let s = with_bruiser(WARRIOR);
    let view = s.decide(PRIEST);
    assert_eq!(
        walks_at(&s, &view),
        Some(ATTACKER),
        "unpressured: {}",
        describe(&view)
    );
}

/// The walk is the Peel play's, so the play's exceptions govern it: with the
/// Peel play's table replaced by one naming the Priest peeling for a Warrior,
/// the Priest does not walk for its dying Warrior; with a table naming another
/// context (a Priest peeling for a Mage), it does.
#[test]
fn the_peel_plays_exceptions_govern_the_walk() {
    use arenasim::states::play_match::situation::{ExceptionKey, ExceptionSet};
    let peel = PEEL;
    let s = scene(AiProfile::TeamPlan, Priest).exceptions(
        peel,
        ExceptionSet::of(&[ExceptionKey::in_context(Priest, Warrior)]),
    );
    let view = s.decide(PRIEST);
    assert_eq!(walks_at(&s, &view), None, "excepted: {}", describe(&view));

    let s = scene(AiProfile::TeamPlan, Priest).exceptions(
        peel,
        ExceptionSet::of(&[ExceptionKey::in_context(Priest, Mage)]),
    );
    let view = s.decide(PRIEST);
    assert_eq!(
        walks_at(&s, &view),
        Some(ATTACKER),
        "another context: {}",
        describe(&view)
    );
}

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
    assert_eq!(walks_at(&s, &view), None);

    let view = dip_scene(AiProfile::Legacy).decide(PRIEST);
    assert!(dipped(&view), "Legacy: {}", describe(&view));
}

/// The defensive Scream is untouched: a `TeamPlan` Priest pressured by an
/// enemy Warrior on it screams in its own defence, with an enemy Warlock's
/// Fear in reach.
#[test]
fn the_defensive_scream_is_not_held() {
    let view = Scene::new()
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
        .targeting(PRIEST)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 60.0)
        .aura(WARRIOR, AuraType::MaxHealthIncrease, 60.0)
        .decide(PRIEST);
    assert_eq!(
        view.chose(),
        Some((AbilityType::PsychicScream, None)),
        "{}",
        describe(&view)
    );
}
