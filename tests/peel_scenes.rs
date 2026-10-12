//! Scene tests for the Peel play (`situation::Play::Peel`,
//! `class_ai::peel`): one per class with a peel tool, each paired with its
//! control — the same scene with the play withheld from the team
//! (`Scene::withhold`), the one condition removed.
//!
//! The scene: the deciding unit's teammate is dying (30% health) with an
//! enemy Warrior on it, so the team's `ally_in_danger` names the teammate and
//! the Warrior is its attacker. The deciding unit targets the enemy healer,
//! far off, so its own chain has no reason to touch the Warrior.

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Hunter, Mage, Paladin, Priest, Rogue, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::peel::peel_tools;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::{PlayStep, PlayTag};
use arenasim::states::play_match::situation::{PlaySet, PEEL};
use arenasim::states::play_match::AbilityType;

const ME: &str = "me";
const ALLY: &str = "ally";
const ATTACKER: &str = "enemy warrior";
const ENEMY_HEALER: &str = "enemy priest";

/// The scene, with the deciding unit of `class` at `(x, 0)`: the ally at
/// `(3, 0)` at 30% health, the enemy Warrior on it at `(4.5, 0)`, the enemy
/// Priest — the unit's target — 30yd off.
fn scene(class: CharacterClass, x: f32, profile: AiProfile) -> Scene {
    Scene::new()
        .ai_profile(1, profile)
        .unit(ME, 1, class)
        .at(x, 0.0)
        .targeting(ENEMY_HEALER)
        .unit(ALLY, 1, Mage)
        .at(3.0, 0.0)
        .health(0.3)
        .targeting(ENEMY_HEALER)
        .unit(ATTACKER, 2, Warrior)
        .at(4.5, 0.0)
        .targeting(ALLY)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(x, 30.0)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; situation {:?}; candidates {:?}",
        view.chosen,
        view.situation,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// `class`, standing at `x`, peels the Warrior off its dying ally with
/// `tool` (`None` for a self-centred area: the trace names no target) — and,
/// with the play withheld, does not; nor under `Legacy`.
fn peels(class: CharacterClass, x: f32, tool: AbilityType, area: bool) {
    peels_in(|profile| scene(class, x, profile), class, tool, area);
}

/// [`peels`], in the scene `build` makes under a profile.
fn peels_in(
    build: impl Fn(AiProfile) -> Scene,
    class: CharacterClass,
    tool: AbilityType,
    area: bool,
) {
    let target = if area { None } else { Some(ATTACKER) };
    assert!(peel_tools(class).contains(&tool));

    let with = build(AiProfile::TeamPlan).decide(ME);
    let situation = with.situation.expect("a situation");
    assert!(
        situation.ally_in_danger.is_some(),
        "{class:?}: {}",
        describe(&with)
    );
    assert!(
        matches!(situation.play, Some(PlayTag::Peel { .. })),
        "{class:?}: {}",
        describe(&with)
    );
    assert_eq!(
        with.chose(),
        Some((tool, target)),
        "{class:?} peels: {}",
        describe(&with)
    );
    assert_eq!(situation.play_step, Some(PlayStep::Peel), "{class:?}");

    // Control: the play withheld from the team, nothing else changed.
    let without = build(AiProfile::TeamPlan)
        .withhold(1, PlaySet::of(&[PEEL]))
        .decide(ME);
    let situation = without.situation.expect("a situation");
    assert!(situation.ally_in_danger.is_some());
    assert_eq!(situation.play, None, "{class:?}: {}", describe(&without));
    assert_eq!(situation.play_step, None);
    assert_ne!(
        without.chose(),
        Some((tool, target)),
        "{class:?} without the play: {}",
        describe(&without)
    );

    // And under Legacy the play does not exist.
    let legacy = build(AiProfile::Legacy).decide(ME);
    assert_eq!(legacy.situation.and_then(|s| s.play), None);
    assert_eq!(legacy.situation.and_then(|s| s.play_step), None);
}

#[test]
fn a_mage_novas_the_warrior_on_its_dying_ally() {
    peels(Mage, 0.0, AbilityType::FrostNova, true);
}

#[test]
fn a_warlock_death_coils_the_warrior_on_its_dying_ally() {
    // Beyond the Warlock's own Death Coil self-peel radius.
    peels(Warlock, -10.0, AbilityType::DeathCoil, false);
}

#[test]
fn a_paladin_hammers_the_warrior_on_its_dying_ally() {
    peels(Paladin, 0.0, AbilityType::HammerOfJustice, false);
}

/// The Hunter already slows the nearest melee threat. The play makes it the
/// one on the dying ally: an enemy Rogue closing on the Hunter stands nearer.
#[test]
fn a_hunter_concusses_the_warrior_on_its_dying_ally_not_the_nearer_melee() {
    const ROGUE: &str = "enemy rogue";
    // The Rogue out of stealth, 12yd off and closing on the Hunter: nearer
    // than the Warrior on the ally (14.5yd), outside the dead zone.
    let build = |profile| {
        let mut s = scene(Hunter, -10.0, profile)
            .unit(ROGUE, 2, Rogue)
            .at(-10.0, 12.0)
            .targeting(ME);
        s.combatant_mut(ROGUE).stealthed = false;
        s
    };
    let control = build(AiProfile::TeamPlan).withhold(1, PlaySet::of(&[PEEL]));
    assert_eq!(
        control.decide(ME).chose(),
        Some((AbilityType::ConcussiveShot, Some(ROGUE))),
        "without the play the Hunter slows the nearer melee"
    );
    peels_in(build, Hunter, AbilityType::ConcussiveShot, false);
}

#[test]
fn a_rogue_kidney_shots_the_warrior_on_its_dying_ally() {
    // In melee with the Warrior, out of stealth.
    let opened = |profile| {
        let mut s = scene(Rogue, 6.0, profile);
        s.combatant_mut(ME).stealthed = false;
        s
    };
    peels_in(opened, Rogue, AbilityType::KidneyShot, false);
}

#[test]
fn a_druid_cyclones_the_warrior_on_its_dying_ally() {
    peels(Druid, 0.0, AbilityType::Cyclone, false);
}

#[test]
fn a_priest_screams_with_the_warrior_on_its_dying_ally_inside() {
    peels(Priest, 0.0, AbilityType::PsychicScream, true);
}

#[test]
fn a_shaman_frost_shocks_the_warrior_on_its_dying_ally() {
    peels(Shaman, 0.0, AbilityType::FrostShock, false);
}

/// The teammate in danger does not peel for itself: its own defence is its
/// chain's. A Mage in danger, with the Warrior on it, runs its fallback play.
#[test]
fn the_unit_in_danger_does_not_run_the_play() {
    let view = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ME, 1, Mage)
        .health(0.3)
        .targeting(ENEMY_HEALER)
        .unit(ALLY, 1, Priest)
        .at(-6.0, 0.0)
        .targeting(ENEMY_HEALER)
        .unit(ATTACKER, 2, Warrior)
        .at(1.5, 0.0)
        .targeting(ME)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(0.0, 30.0)
        .decide(ME);
    let situation = view.situation.expect("a situation");
    assert!(situation.ally_in_danger.is_some(), "{}", describe(&view));
    assert_eq!(situation.play, None, "{}", describe(&view));
}

/// An attacker the team already holds needs no peel: the Warrior stunned, and
/// the Priest's peel step does not scream.
#[test]
fn an_attacker_the_team_holds_is_not_peeled() {
    let held = scene(Priest, 0.0, AiProfile::TeamPlan).aura(
        ATTACKER,
        arenasim::states::play_match::AuraType::Stun,
        4.0,
    );
    let view = held.decide(ME);
    assert_eq!(
        view.situation.and_then(|s| s.play_step),
        None,
        "{}",
        describe(&view)
    );
    assert_ne!(view.chose(), Some((AbilityType::PsychicScream, None)));
}

/// The Warrior has no peel tool: under the play it runs the chain its
/// fallback orders, as it would without it.
#[test]
fn a_warrior_has_no_peel() {
    assert!(peel_tools(Warrior).is_empty());
    let with = scene(Warrior, 6.0, AiProfile::TeamPlan).decide(ME);
    let without = scene(Warrior, 6.0, AiProfile::TeamPlan)
        .withhold(1, PlaySet::of(&[PEEL]))
        .decide(ME);
    assert_eq!(with.chose(), without.chose(), "{}", describe(&with));
    assert_eq!(with.situation.and_then(|s| s.play_step), None);
}
