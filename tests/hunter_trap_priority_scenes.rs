//! Scene tests for the Hunter's trap priority (AS-243): the Frost Trap peel
//! is held for the shared trap cooldown only while the pressure trap would
//! freeze the enemy healer within `HEALER_FREEZE_HOLD_SECS`
//! (`hunter::frost_trap_held_for_healer`).
//!
//! Each pair is one scene and its control with one condition changed, so
//! neither half passes because the Hunter had no reason to peel, or no freeze
//! to wait for.
//!
//! The base scene: a Hunter at the origin, targeting an enemy Warrior on it
//! 10yd off (the closing band), Concussive Shot cooling so the Frost Trap is
//! the peel it reaches. The enemy Priest is planted (casting) just beyond
//! Freezing Trap's range, and the Hunter walks toward it: the pressure trap
//! holds this tick (out of throw range) and would land about a second from
//! now. Every unit not given a facing walks +Z at its base speed, as a scene
//! estimates it.

use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Paladin, Priest, Warrior};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::hunter::{
    FROST_TRAP_HELD_FOR_HEALER, HEALER_FREEZE_HOLD_SECS,
};
use arenasim::states::play_match::class_ai::hunter_dip::{
    PRESSURE_TRAP_LEAD_OUT_OF_RANGE, PRESSURE_TRAP_OUT_OF_RANGE,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::AbilityType;

use bevy::prelude::Vec2;

const ME: &str = "me";
const WARRIOR: &str = "enemy warrior";
const PRIEST: &str = "enemy priest";
const OTHER: &str = "other enemy";

/// Freezing Trap's configured throw range.
fn trap_range() -> f32 {
    AbilityDefinitions::default()
        .get_unchecked(&AbilityType::FreezingTrap)
        .range
}

/// The Hunter's base speed, the rate it closes on a planted healer.
fn hunter_speed() -> f32 {
    Scene::new()
        .unit(ME, 1, Hunter)
        .combatant(ME)
        .base_movement_speed
}

/// Where the planted Priest stands for the pressure trap to come into range
/// `secs` from now, the Hunter walking straight at it.
fn priest_x(secs: f32) -> f32 {
    trap_range() + hunter_speed() * secs
}

/// The base scene with the Priest `priest_x` yards off along +X, the Hunter
/// walking along `(dir_x, 0)`, and the melee on the Hunter at `melee_x`.
fn scene_with(priest_at: f32, dir_x: f32, melee_x: f32) -> Scene {
    Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .facing(dir_x, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(melee_x, 0.0)
        .targeting(ME)
        .unit(PRIEST, 2, Priest)
        .at(priest_at, 0.0)
        .casting(PRIEST, AbilityType::FlashHeal)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
}

/// The base scene: the freeze is a second away.
fn reachable() -> Scene {
    scene_with(priest_x(1.0), 1.0, 10.0)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.choices,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn held_for_healer(view: &DecisionView) -> bool {
    matches!(
        view.rejected(AbilityType::FrostTrap),
        Some(RejectionReason::PreconditionUnmet { note }) if note == FROST_TRAP_HELD_FOR_HEALER
    )
}

fn peeled(view: &DecisionView) -> bool {
    view.choices
        .iter()
        .any(|(a, _)| *a == AbilityType::FrostTrap)
}

/// The held half of every pair: the Frost Trap is refused with the note, and
/// nothing else spends the trap cooldown.
fn assert_held(view: &DecisionView) {
    assert!(held_for_healer(view), "{}", describe(view));
    assert!(!peeled(view), "{}", describe(view));
    assert!(
        !view
            .choices
            .iter()
            .any(|(a, _)| *a == AbilityType::FreezingTrap),
        "the pressure trap is out of range this tick: {}",
        describe(view)
    );
}

/// The peeled half: the Frost Trap goes out, and the hold is not traced.
fn assert_peeled(view: &DecisionView) {
    assert!(peeled(view), "{}", describe(view));
    assert!(!held_for_healer(view), "{}", describe(view));
}

/// The healer comes into throw range within the horizon: the peel is held.
/// Walking away from it, it never does: the peel goes out.
#[test]
fn peel_is_held_only_while_the_hunter_closes_on_the_healer() {
    let view = reachable().decide(ME);
    assert!(
        matches!(
            view.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::PreconditionUnmet { note }) if note == PRESSURE_TRAP_OUT_OF_RANGE
        ),
        "the pressure trap holds this tick: {}",
        describe(&view)
    );
    assert_held(&view);

    let away = scene_with(priest_x(1.0), -1.0, 10.0).decide(ME);
    assert_peeled(&away);
    assert_eq!(away.chose(), Some((AbilityType::FrostTrap, Some(WARRIOR))));
}

/// A freeze that comes into range a second inside the horizon holds the peel;
/// one a second past it does not.
#[test]
fn peel_is_not_held_for_a_freeze_past_the_horizon() {
    assert_held(&scene_with(priest_x(HEALER_FREEZE_HOLD_SECS - 1.0), 1.0, 10.0).decide(ME));
    assert_peeled(&scene_with(priest_x(HEALER_FREEZE_HOLD_SECS + 1.0), 1.0, 10.0).decide(ME));
}

/// Another enemy beside the healer would spring the trap first: the freeze
/// would not land cleanly, so the peel goes out. The same enemy well clear of
/// the healer holds it.
#[test]
fn peel_is_not_held_for_a_healer_that_would_not_spring_it_cleanly() {
    let with_other = |z: f32| {
        reachable()
            .unit(OTHER, 2, Mage)
            .at(priest_x(1.0), z)
            .casting(OTHER, AbilityType::Frostbolt)
    };
    assert_held(&with_other(25.0).decide(ME));
    assert_peeled(&with_other(2.0).decide(ME));
}

/// A pillar hides every landing the Hunter reaches within the horizon: the
/// placement would refuse the throw, so the peel goes out. The same pillar off
/// the line of the throw holds it.
#[test]
fn peel_is_not_held_for_a_freeze_whose_landing_is_hidden() {
    let with_pillar = |z: f32| {
        reachable().obstacles(vec![ObstacleVolume::Cylinder {
            center_xz: Vec2::new(priest_x(1.0) - 4.0, z),
            radius: 1.5,
            base_y: 0.0,
            height: 10.0,
        }])
    };
    assert_held(&with_pillar(10.0).decide(ME));
    assert_peeled(&with_pillar(0.0).decide(ME));
}

/// A second enemy healer would free the frozen one: the pressure trap passes
/// over it, so the peel is not held for it. A second enemy that cannot free it
/// holds the peel.
#[test]
fn peel_is_not_held_for_a_healer_its_team_would_free() {
    let with_other = |class| reachable().unit(OTHER, 2, class).at(0.0, 60.0);
    assert_held(&with_other(Mage).decide(ME));
    assert_peeled(&with_other(Paladin).decide(ME));
}

/// The pressure trap answers a melee on the Hunter. With a caster on it
/// instead, no pressure trap is coming, so the peel (on the caster) goes out.
#[test]
fn peel_is_not_held_without_a_melee_for_the_pressure_trap() {
    let caster = Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .facing(1.0, 0.0)
        .targeting(OTHER)
        .unit(OTHER, 2, Mage)
        .at(10.0, 0.0)
        .targeting(ME)
        .unit(PRIEST, 2, Priest)
        .at(priest_x(1.0), 0.0)
        .casting(PRIEST, AbilityType::FlashHeal)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0);
    let view = caster.decide(ME);
    assert_peeled(&view);
    assert_eq!(view.chose(), Some((AbilityType::FrostTrap, Some(OTHER))));
    assert_held(&reachable().decide(ME));
}

/// A Freezing Trap that cannot be cast (a scene cools each trap on its own)
/// is no freeze to wait for: the peel goes out.
#[test]
fn peel_is_not_held_while_freezing_trap_cannot_be_cast() {
    assert_held(&reachable().decide(ME));
    assert_peeled(
        &reachable()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .decide(ME),
    );
}

/// A Frost Trap on cooldown is traced `OnCooldown`, never the hold.
#[test]
fn a_cooling_frost_trap_is_traced_on_cooldown() {
    let view = reachable()
        .cooldown(ME, AbilityType::FrostTrap, 5.0)
        .decide(ME);
    assert!(
        matches!(
            view.rejected(AbilityType::FrostTrap),
            Some(RejectionReason::OnCooldown { .. })
        ),
        "{}",
        describe(&view)
    );
}

/// The dead zone's Frost Trap at the Hunter's feet waits for the same freeze:
/// Disengage cooling, the melee 5yd off. Walking away from the healer, the
/// trap goes down.
#[test]
fn dead_zone_frost_trap_is_held_for_a_reachable_freeze() {
    let build = |dir_x: f32| {
        scene_with(priest_x(1.0), dir_x, 5.0).cooldown(ME, AbilityType::Disengage, 5.0)
    };
    assert_held(&build(1.0).decide(ME));
    let away = build(-1.0).decide(ME);
    assert_peeled(&away);
}

/// The forecast is relative: a healer walking away from the Hunter as fast as
/// the Hunter walks after it stays out of reach (its lead lands beyond throw
/// range), so the peel goes out. The same healer walking across the Hunter's
/// path is caught up with, and holds it.
#[test]
fn peel_is_not_held_for_a_healer_walking_away_as_fast_as_the_hunter_closes() {
    let build = |priest_dir: (f32, f32)| {
        Scene::new()
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .facing(1.0, 0.0)
            .targeting(WARRIOR)
            .unit(WARRIOR, 2, Warrior)
            .at(10.0, 0.0)
            .targeting(ME)
            .unit(PRIEST, 2, Priest)
            .at(trap_range() - 2.0, 0.0)
            .facing(priest_dir.0, priest_dir.1)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
    };
    let walking = build((1.0, 0.0)).decide(ME);
    assert!(
        matches!(
            walking.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::PreconditionUnmet { note }) if note == PRESSURE_TRAP_LEAD_OUT_OF_RANGE
        ),
        "the healer's lead carries the landing out of range: {}",
        describe(&walking)
    );
    assert_peeled(&walking);
    assert_held(&build((0.0, 1.0)).decide(ME));
}
