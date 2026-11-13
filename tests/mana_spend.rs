//! A unit pays for a cast once, and only with mana it has (AS-438).
//!
//! A cast with a cast bar pays in `process_casting` when it lands (CLAUDE.md,
//! *Combat Flow*). Two spends took mana a unit did not have:
//!
//! - Aimed Shot also paid as it began, so every shot cost twice its price,
//!   and a Hunter that could afford one shot ended below zero mana. The scene
//!   pair pins the decision: beginning Aimed Shot leaves the Hunter's mana
//!   where it was, where an instant shot (the control) pays at once. The
//!   staged match pins the whole cast: one shot costs exactly its price.
//! - The landing charge never asked whether the caster could still pay, so a
//!   Mana Burn landing mid-cast drove a healer below zero as its heal landed.
//!   Now the cast fails, as in TBC. The staged pair puts a Paladin mid-cast
//!   with less mana than Flash of Light costs, and its control with more.

mod common;

use arenasim::headless::stage::{Stage, Unit};
use arenasim::headless::HeadlessMatchConfig;
use arenasim::headless::{FrameObservation, ObservedCombatant};
use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Priest};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::CastingState;
use arenasim::states::play_match::{AbilityType, AuraType};

const HUNTER: &str = "hunter";
const MAGE: &str = "mage";
const ENEMY_PRIEST: &str = "enemy priest";

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

/// The Hunter on the Mage while the enemy Priest is polymorphed: the Burst
/// play starts Aimed Shot (`tests/situation_scenes.rs`). Its mana covers ONE
/// Aimed Shot and not two.
fn hunter_scene() -> Scene {
    let mut scene = Scene::new()
        .unit(HUNTER, 1, Hunter)
        .unit(MAGE, 2, Mage)
        .at(30.0, 0.0)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 30.0)
        .aura(ENEMY_PRIEST, AuraType::Polymorph, 6.0);
    let target = scene.entity(MAGE);
    let hunter = scene.combatant_mut(HUNTER);
    hunter.target = Some(target);
    hunter.hunter_opened = true;
    let cost = hunter
        .book()
        .get_unchecked(&AbilityType::AimedShot)
        .mana_cost;
    hunter.current_mana = cost * 1.5;
    scene
}

#[test]
fn beginning_aimed_shot_pays_nothing() {
    let scene = hunter_scene();
    let before = scene.combatant(HUNTER).current_mana;
    let (view, after) = scene.decide_keeping(HUNTER);
    assert_eq!(
        view.chose(),
        Some((AbilityType::AimedShot, Some(MAGE))),
        "{}",
        describe(&view)
    );
    assert_eq!(
        after.current_mana, before,
        "Aimed Shot pays as it lands, not as it begins"
    );
}

#[test]
fn an_instant_shot_pays_as_it_is_taken() {
    // The control: with Aimed Shot on cooldown the Hunter takes an instant,
    // which pays at the decision, so the scene does read what a decision paid.
    let scene = hunter_scene().cooldown(HUNTER, AbilityType::AimedShot, 8.0);
    let before = scene.combatant(HUNTER).current_mana;
    let (view, after) = scene.decide_keeping(HUNTER);
    let (ability, _) = view
        .chose()
        .unwrap_or_else(|| panic!("{}", describe(&view)));
    let cost = after.book().get_unchecked(&ability).mana_cost;
    assert!(cost > 0.0, "{ability:?} costs nothing: {}", describe(&view));
    assert_eq!(
        after.current_mana,
        before - cost,
        "{ability:?} is an instant: it pays as it is taken"
    );
}

/// A Hunter at range of a Priest, holding enough mana for one Aimed Shot and
/// not two, opens with one. Across the cast its mana falls once, by the
/// shot's price, as the shot leaves; it never goes below zero.
#[test]
fn one_aimed_shot_costs_its_price_once() {
    const HUNTER_UNIT: Unit = Unit::new(1, 0);
    const PRIEST_UNIT: Unit = Unit::new(2, 0);
    let config = HeadlessMatchConfig {
        team1: vec!["Hunter".into()],
        team2: vec!["Priest".into()],
        random_seed: Some(7),
        max_duration_secs: 30.0,
        ..Default::default()
    };
    let cost = arenasim::states::play_match::load_ability_definitions()
        .expect("abilities.ron")
        .get_unchecked(&AbilityType::AimedShot)
        .mana_cost;
    let stage = Stage::new()
        .place(HUNTER_UNIT, 0.0, 0.0)
        .place(PRIEST_UNIT, 30.0, 0.0)
        .target(HUNTER_UNIT, PRIEST_UNIT)
        .resource(HUNTER_UNIT, cost * 1.5);
    let played = common::staged::play(config, stage);

    let hunter = |f: &FrameObservation| {
        f.combatants
            .values()
            .find(|c| c.team == 1 && !c.is_pet)
            .cloned()
            .expect("the Hunter")
    };
    // Every frame, so the one before the cast begins is there even when the
    // Hunter begins it on the frame the stage lands.
    let frames: Vec<_> = played.frames.iter().map(hunter).collect();
    let landed = played.frames.len() - played.staged_frames().count();
    let first = landed
        + frames[landed..]
            .iter()
            .position(|c| c.casting == Some(AbilityType::AimedShot))
            .expect("the Hunter begins an Aimed Shot");
    let last = first
        + frames[first..]
            .iter()
            .position(|c| c.casting != Some(AbilityType::AimedShot))
            .expect("the Aimed Shot ends");
    assert!(
        played.log.contains("Aimed Shot hits") || played.log.contains("Aimed Shot CRITS"),
        "the Aimed Shot lands"
    );
    // Beginning the cast pays nothing: the mana only regenerates.
    assert!(
        frames[first].current_mana >= frames[first - 1].current_mana,
        "Aimed Shot paid as it began: {} -> {}",
        frames[first - 1].current_mana,
        frames[first].current_mana
    );
    // Landing pays the price, less one frame's regeneration.
    let paid = frames[last - 1].current_mana - frames[last].current_mana;
    assert!(
        paid <= cost && paid > cost - 1.0,
        "Aimed Shot paid {paid} as it landed, its price is {cost}"
    );
    let lowest = frames[landed..]
        .iter()
        .map(|c| c.current_mana)
        .fold(f32::INFINITY, f32::min);
    assert!(lowest >= 0.0, "the Hunter's mana went to {lowest}");
}

const PALADIN_UNIT: Unit = Unit::new(1, 0);
const WARRIOR_UNIT: Unit = Unit::new(2, 0);

/// A Paladin half a second from the end of a Flash of Light on itself, its
/// mana `margin` points either side of the spell's price, with the enemy
/// Warrior far out of reach. Returns the Paladin's frames from the stage on,
/// the price, and the combat log.
fn flash_of_light_landing(margin: f32) -> (Vec<ObservedCombatant>, f32, String) {
    let config = HeadlessMatchConfig {
        team1: vec!["Paladin".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(7),
        max_duration_secs: 30.0,
        ..Default::default()
    };
    let cost = arenasim::states::play_match::load_ability_definitions()
        .expect("abilities.ron")
        .get_unchecked(&AbilityType::FlashOfLight)
        .mana_cost;
    let stage = Stage::new()
        .place(PALADIN_UNIT, -40.0, 0.0)
        .place(WARRIOR_UNIT, 40.0, 0.0)
        .health(PALADIN_UNIT, 0.5)
        .resource(PALADIN_UNIT, cost + margin)
        .world(|world| {
            let paladin = PALADIN_UNIT.entity(world);
            world.entity_mut(paladin).insert(CastingState::new(
                AbilityType::FlashOfLight,
                paladin,
                0.5,
            ));
        });
    let played = common::staged::play(config, stage);
    let paladin = |f: &FrameObservation| {
        f.combatants
            .values()
            .find(|c| c.team == 1 && !c.is_pet)
            .cloned()
            .expect("the Paladin")
    };
    let frames = played.staged_frames().map(paladin).collect();
    (frames, cost, played.log)
}

/// The frame the staged Flash of Light ended on, and the one before it.
fn cast_end(frames: &[ObservedCombatant]) -> usize {
    assert_eq!(
        frames[0].casting,
        Some(AbilityType::FlashOfLight),
        "the stage put the Paladin mid-cast"
    );
    frames
        .iter()
        .position(|c| c.casting != Some(AbilityType::FlashOfLight))
        .expect("the Flash of Light ends")
}

#[test]
fn a_cast_its_caster_can_no_longer_pay_for_fails() {
    let (frames, _, log) = flash_of_light_landing(-1.0);
    let end = cast_end(&frames);
    assert!(
        log.contains("Team 1 Paladin #1 fails to cast Flash of Light: not enough mana"),
        "the cast fails for mana"
    );
    assert!(
        frames[end].current_mana >= frames[end - 1].current_mana,
        "a failed cast pays nothing: {} -> {}",
        frames[end - 1].current_mana,
        frames[end].current_mana
    );
    assert_eq!(
        frames[end].current_health,
        frames[end - 1].current_health,
        "a failed heal lands nothing"
    );
    let lowest = frames
        .iter()
        .map(|c| c.current_mana)
        .fold(f32::INFINITY, f32::min);
    assert!(lowest >= 0.0, "the Paladin's mana went to {lowest}");
}

#[test]
fn a_cast_its_caster_can_pay_for_lands_and_pays() {
    // The control: one point more than the price, and the same cast lands.
    let (frames, cost, log) = flash_of_light_landing(1.0);
    let end = cast_end(&frames);
    assert!(!log.contains("not enough mana"), "the cast does not fail");
    let paid = frames[end - 1].current_mana - frames[end].current_mana;
    assert!(
        paid <= cost && paid > cost - 1.0,
        "Flash of Light paid {paid} as it landed, its price is {cost}"
    );
    assert!(
        frames[end].current_health > frames[end - 1].current_health,
        "the heal lands"
    );
}
