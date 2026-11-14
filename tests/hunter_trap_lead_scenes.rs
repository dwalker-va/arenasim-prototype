//! Scene tests for where the Hunter's Freezing Trap is aimed at a moving
//! enemy healer (AS-493): `hunter_dip::trap_lead_landing` leads the healer
//! along its forecast velocity (`hunter_dip::forecast_velocity`) — at the
//! speed its slows leave it, and not at all while it is rooted or held — so a
//! trap thrown at a rooted or slowed healer lands where that healer will be,
//! not where it would be at its base speed.
//!
//! Each pair is one scene and its control with one condition changed, and
//! both halves assert the trap was THROWN and where it lands, so neither half
//! passes on a trap that never went out.
//!
//! The scene: a Hunter at the origin with an enemy Warrior on it 10yd off
//! (the closing band), so the pressure trap is the Hunter's answer; the enemy
//! Priest stands 20yd off along +X, walking across the throw along +Z at its
//! base speed. The landing is read back from the throw itself
//! (`DecisionView::trap_landings`).

use arenasim::states::match_config::CharacterClass::{Hunter, Priest, Warrior};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::constants::{
    TRAP_ARM_DELAY, TRAP_LAUNCH_SPEED, TRAP_TRIGGER_RADIUS,
};
use arenasim::states::play_match::{AbilityType, Aura, AuraType, TrapType};

use bevy::prelude::Vec3;

const ME: &str = "me";
const WARRIOR: &str = "enemy warrior";
const PRIEST: &str = "enemy priest";

/// How far off the Priest stands, along +X.
const PRIEST_X: f32 = 20.0;

/// How close a landing must be to its expected point. The scene places the
/// units exactly, so this only absorbs float rounding.
const EPS: f32 = 1e-3;

fn base() -> Scene {
    Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .facing(1.0, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(10.0, 0.0)
        .targeting(ME)
        .unit(PRIEST, 2, Priest)
        .at(PRIEST_X, 0.0)
        .facing(0.0, 1.0)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
}

/// The Priest's base speed, the speed an unslowed heading carries.
fn priest_speed() -> f32 {
    base().combatant(PRIEST).base_movement_speed
}

/// The lead along +Z a trap thrown from the origin gives a Priest walking at
/// `speed`: its travel and arm time, capped at three trigger radii.
fn lead_at(speed: f32) -> f32 {
    let lead_time = TRAP_ARM_DELAY + PRIEST_X / TRAP_LAUNCH_SPEED;
    (speed * lead_time).min(TRAP_TRIGGER_RADIUS * 3.0)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; traps {:?}; candidates {:?}",
        view.choices,
        view.trap_landings,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// The Freezing Trap the decision threw on the Priest, and where it lands.
/// Fails unless exactly one Freezing Trap went out, aimed at the Priest.
fn freezing_landing(view: &DecisionView) -> Vec3 {
    assert!(
        view.choices
            .contains(&(AbilityType::FreezingTrap, Some(PRIEST))),
        "the pressure trap is thrown at the Priest: {}",
        describe(view)
    );
    let freezing: Vec<Vec3> = view
        .trap_landings
        .iter()
        .filter(|(t, _)| *t == TrapType::Freezing)
        .map(|(_, at)| *at)
        .collect();
    assert_eq!(freezing.len(), 1, "{}", describe(view));
    freezing[0]
}

fn assert_lands_at(view: &DecisionView, x: f32, z: f32) {
    let at = freezing_landing(view);
    assert!(
        (at.x - x).abs() < EPS && (at.z - z).abs() < EPS,
        "expected the trap to land at ({x}, {z}), it lands at {at:?}: {}",
        describe(view)
    );
}

/// A slow of `magnitude` on the Priest.
fn slowed(scene: Scene, magnitude: f32) -> Scene {
    scene.aura_full(
        PRIEST,
        Aura {
            effect_type: AuraType::MovementSpeedSlow,
            duration: 8.0,
            magnitude,
            ..Default::default()
        },
    )
}

/// The free Priest is led along its heading; rooted, it goes nowhere, and the
/// trap lands on it.
#[test]
fn a_rooted_healer_is_trapped_where_it_stands() {
    let free = base().decide(ME);
    assert_lands_at(&free, PRIEST_X, lead_at(priest_speed()));
    assert!(
        lead_at(priest_speed()) > TRAP_TRIGGER_RADIUS,
        "the free lead must carry the landing off the Priest for this pair to mean anything"
    );

    let rooted = base().aura(PRIEST, AuraType::Root, 4.0).decide(ME);
    assert_lands_at(&rooted, PRIEST_X, 0.0);
}

/// The slowed Priest is led at the speed its slow leaves it: at 40% of its
/// speed the landing is 40% as far ahead as the free Priest's would be
/// uncapped. The free lead may run into the cap; the slowed one does not, so
/// the landing is the slow's.
#[test]
fn a_slowed_healer_is_led_at_its_slowed_speed() {
    let free = base().decide(ME);
    assert_lands_at(&free, PRIEST_X, lead_at(priest_speed()));

    let slowed_lead = lead_at(priest_speed() * 0.4);
    assert!(
        slowed_lead < TRAP_TRIGGER_RADIUS * 3.0 && slowed_lead + 1.0 < lead_at(priest_speed()),
        "the slowed lead must sit under the cap and short of the free one"
    );
    let slow = slowed(base(), 0.4).decide(ME);
    assert_lands_at(&slow, PRIEST_X, slowed_lead);
}
