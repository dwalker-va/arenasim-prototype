//! Scene tests for the four decisions that read a situation
//! (`src/states/play_match/situation.rs`): each runs the real class dispatch
//! for one tick in a constructed scene, paired with its control — the same
//! scene with the situation's one condition removed.
//!
//! These pin what the situation DOES to each decision, which no seed probe
//! can: a situational rule that reorders a priority chain usually leaves a
//! match byte-identical, so neither the match-level suites nor a sweep notice
//! when it is deleted. Each pair below kills exactly that deletion.

use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Priest, Warrior};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::MovementTrigger;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::situation::Tempo;
use arenasim::states::play_match::{AbilityType, AuraType};

const WARRIOR: &str = "warrior";
const HUNTER: &str = "hunter";
const PRIEST: &str = "priest";
const ALLY: &str = "ally";
const MAGE: &str = "mage";
const ENEMY_WARRIOR: &str = "enemy warrior";
const ENEMY_PRIEST: &str = "enemy priest";

/// Battle Shout's buff, already up, so the Warrior's first priority (shout an
/// unbuffed ally) is out of the way and the damage chain is what decides.
fn shouted(scene: Scene, who: &'static str) -> Scene {
    scene.aura(who, AuraType::AttackPowerIncrease, 120.0)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}; movement {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>(),
        view.movement
    )
}

// ---------------------------------------------------------------------------
// Warrior: the Burst play puts Mortal Strike ahead of Rend.
// ---------------------------------------------------------------------------

fn warrior_scene() -> Scene {
    let scene = Scene::new()
        .unit(WARRIOR, 1, Warrior)
        .rage(80.0)
        .targeting(MAGE)
        .unit(MAGE, 2, Mage)
        .at(2.0, 0.0)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 20.0)
        .aura(ENEMY_PRIEST, AuraType::Polymorph, 6.0);
    shouted(scene, WARRIOR)
}

#[test]
fn warrior_bursts_mortal_strike_while_the_enemy_healer_is_locked() {
    let mut scene = warrior_scene();
    let locked = scene.decide(WARRIOR);
    assert!(
        locked
            .situation
            .is_some_and(|s| s.enemy_healer_locked.is_some()),
        "the scene's situation: {:?}",
        locked.situation
    );
    assert_eq!(
        locked.chose(),
        Some((AbilityType::MortalStrike, Some(MAGE))),
        "{}",
        describe(&locked)
    );

    // Control: the healer free, everything else the same.
    scene.clear_auras(ENEMY_PRIEST);
    let free = scene.decide(WARRIOR);
    assert!(free
        .situation
        .is_some_and(|s| s.enemy_healer_locked.is_none()));
    assert_eq!(
        free.chose(),
        Some((AbilityType::Rend, Some(MAGE))),
        "{}",
        describe(&free)
    );
}

#[test]
fn a_rooted_healer_is_not_locked() {
    let mut scene = warrior_scene();
    scene.clear_auras(ENEMY_PRIEST);
    let scene = scene.aura(ENEMY_PRIEST, AuraType::Root, 6.0);
    let view = scene.decide(WARRIOR);
    assert!(view
        .situation
        .is_some_and(|s| s.enemy_healer_locked.is_none()));
    assert_eq!(view.chose(), Some((AbilityType::Rend, Some(MAGE))));
}

// ---------------------------------------------------------------------------
// Hunter: the Burst play starts Aimed Shot at safe range — unless the locked
// healer is the Hunter's own target.
// ---------------------------------------------------------------------------

fn hunter_scene(target: &'static str) -> Scene {
    let mut scene = Scene::new()
        .unit(HUNTER, 1, Hunter)
        .unit(MAGE, 2, Mage)
        .at(30.0, 0.0)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 30.0)
        .aura(ENEMY_PRIEST, AuraType::Polymorph, 6.0);
    let target = scene.entity(target);
    let hunter = scene.combatant_mut(HUNTER);
    hunter.target = Some(target);
    // Past the opener, so the ordinary rotation (not the opener's own Aimed
    // Shot rule) is what runs.
    hunter.hunter_opened = true;
    scene
}

#[test]
fn hunter_bursts_aimed_shot_while_the_enemy_healer_is_locked() {
    let mut scene = hunter_scene(MAGE);
    let locked = scene.decide(HUNTER);
    assert_eq!(
        locked.chose(),
        Some((AbilityType::AimedShot, Some(MAGE))),
        "{}",
        describe(&locked)
    );

    scene.clear_auras(ENEMY_PRIEST);
    let free = scene.decide(HUNTER);
    let control = free.chose();
    assert!(
        control.is_some_and(|(ability, _)| ability != AbilityType::AimedShot),
        "{}",
        describe(&free)
    );
}

#[test]
fn hunter_does_not_burst_the_locked_healer_itself() {
    // The healer is the target: bursting it would break its own breakable CC.
    let scene = hunter_scene(ENEMY_PRIEST);
    let view = scene.decide(HUNTER);
    assert!(view
        .situation
        .is_some_and(|s| s.enemy_healer_locked.is_some()));
    assert_ne!(
        view.chose().map(|(ability, _)| ability),
        Some(AbilityType::AimedShot),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Priest: a locked enemy healer lowers Mana Burn's health floor from 0.90 to
// 0.70, so a team at 80% burns instead of healing.
// ---------------------------------------------------------------------------

/// Fortitude already up on both allies, Weakened Soul on both (no Shield to
/// spend), and Psychic Scream on cooldown (so no offensive dip walks the
/// Priest at the enemy healer): the heal-or-burn rungs are what decides.
fn priest_scene() -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
        .cooldown(PRIEST, AbilityType::PsychicScream, 20.0)
        .unit(ALLY, 1, Warrior)
        .at(5.0, 0.0)
        .health(0.8)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .aura(ALLY, AuraType::WeakenedSoul, 15.0)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 28.0)
        .aura(ENEMY_PRIEST, AuraType::Polymorph, 6.0)
        .unit(MAGE, 2, Mage)
        .at(-28.0, 0.0)
}

#[test]
fn priest_burns_mana_on_a_locked_healer_and_heals_otherwise() {
    let mut scene = priest_scene();
    let locked = scene.decide(PRIEST);
    assert_eq!(
        locked.chose(),
        Some((AbilityType::ManaBurn, Some(ENEMY_PRIEST))),
        "{}",
        describe(&locked)
    );

    scene.clear_auras(ENEMY_PRIEST);
    let free = scene.decide(PRIEST);
    assert_eq!(
        free.chose(),
        Some((AbilityType::FlashHeal, Some(ALLY))),
        "{}",
        describe(&free)
    );
}

// ---------------------------------------------------------------------------
// Healer posture: a team pressing its lead stops hiding — `cover_pull` is
// zeroed while the tempo is Pressing.
// ---------------------------------------------------------------------------

/// A Priest an enemy Warrior is closing on, with a pillar between them: every
/// step straight away from the Warrior is out of its sight, so cover is on
/// offer. `enemy_health` sets the tempo.
fn pressured_priest_scene(enemy_health: f32) -> Scene {
    let pillar = ObstacleVolume::Cylinder {
        center_xz: bevy::prelude::Vec2::new(-4.0, 0.0),
        radius: 1.5,
        base_y: 0.0,
        height: 10.0,
    };
    Scene::new()
        .obstacles(vec![pillar])
        .unit(PRIEST, 1, Priest)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .unit(ALLY, 1, Warrior)
        .at(0.0, -10.0)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .unit(ENEMY_WARRIOR, 2, Warrior)
        .at(-8.0, 0.0)
        .health(enemy_health)
        .targeting(PRIEST)
        .unit(MAGE, 2, Mage)
        .at(-30.0, 10.0)
        .health(enemy_health)
}

#[test]
fn a_pressing_healer_stops_pulling_toward_cover() {
    let pressing = pressured_priest_scene(0.6).decide(PRIEST);
    let even = pressured_priest_scene(1.0).decide(PRIEST);
    assert_eq!(pressing.situation.map(|s| s.tempo), Some(Tempo::Pressing));
    assert_eq!(even.situation.map(|s| s.tempo), Some(Tempo::Even));
    for view in [&pressing, &even] {
        assert!(
            view.movement
                .iter()
                .any(|m| m.trigger == MovementTrigger::PressuredEnter),
            "the scene pressures the Priest: {}",
            describe(view)
        );
    }
    assert!(
        even.scorer_term("cover_pull").is_some_and(|v| v > 0.0),
        "control: the level Priest steps into cover — {}",
        describe(&even)
    );
    assert_eq!(
        pressing.scorer_term("cover_pull"),
        Some(0.0),
        "{}",
        describe(&pressing)
    );
}
