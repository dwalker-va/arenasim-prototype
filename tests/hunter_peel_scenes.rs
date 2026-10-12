//! Scene tests for the Hunter's snares and trap throws (AS-281): a peel or a
//! snare is never spent on an immune unit, and a trap is thrown only where the
//! Hunter can see it land. Each scene is paired with its control — the same
//! scene with the one condition removed — so a test cannot pass because the
//! Hunter had no reason to cast at all.
//!
//! The scene: a Hunter at the origin targets the enemy Priest 25yd off; an
//! enemy Warrior 10yd off is on the Hunter (the closing band, outside the dead
//! zone).

use arenasim::states::match_config::CharacterClass::{Hunter, Priest, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::TrapType;
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::{AbilityType, Aura, AuraType};

use bevy::prelude::Vec2;

const ME: &str = "me";
const WARRIOR: &str = "enemy warrior";
const SECOND: &str = "second warrior";
const PRIEST: &str = "enemy priest";
const ALLY: &str = "ally";

/// The base scene. The Hunter's traps share one cooldown, but readiness is
/// read per ability, so a scene cools the one it wants out of the way.
fn scene() -> Scene {
    Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(PRIEST)
        .unit(WARRIOR, 2, Warrior)
        .at(10.0, 0.0)
        .targeting(ME)
        .unit(PRIEST, 2, Priest)
        .at(25.0, 0.0)
}

/// Divine Shield on `who`: immune to damage and to every hostile aura.
fn shielded(scene: Scene, who: &'static str) -> Scene {
    scene.aura_full(
        who,
        Aura {
            effect_type: AuraType::DamageImmunity,
            duration: 8.0,
            break_on_damage_threshold: -1.0,
            ..Default::default()
        },
    )
}

/// A pillar centred at `(x, z)`.
fn pillar(scene: Scene, x: f32, z: f32) -> Scene {
    scene.obstacles(vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(x, z),
        radius: 1.5,
        base_y: 0.0,
        height: 10.0,
    }])
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

/// Whether the decision spent anything on `who`.
fn aimed_at(view: &DecisionView, who: &str) -> bool {
    view.choices.iter().any(|(_, t)| *t == Some(who))
}

// ---------------------------------------------------------------------------
// Immunity
// ---------------------------------------------------------------------------

/// The card's scene: the traps cooling, a Divine-Shielded Warrior on the
/// Hunter. A Concussive Shot on it is blocked on arrival, so it is not fired.
#[test]
fn concussive_shot_is_not_fired_at_a_shielded_melee() {
    let build = || {
        scene()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .cooldown(ME, AbilityType::FrostTrap, 5.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::ConcussiveShot, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );

    let view = shielded(build(), WARRIOR).decide(ME);
    assert!(!aimed_at(&view, WARRIOR), "{}", describe(&view));
}

/// Concussive Shot cooling: the Frost Trap peel aims at the melee on the
/// Hunter — but not at a shielded one, which the slow would not take.
#[test]
fn frost_trap_is_not_thrown_at_a_shielded_melee() {
    let build = || {
        scene()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FrostTrap, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );

    let view = shielded(build(), WARRIOR).decide(ME);
    assert!(
        !view
            .choices
            .iter()
            .any(|(a, _)| *a == AbilityType::FrostTrap),
        "{}",
        describe(&view)
    );
    assert!(!aimed_at(&view, WARRIOR), "{}", describe(&view));
}

/// A second Warrior, farther off and not shielded, beside a shielded one
/// nearer: the snare goes to the one it would land on.
fn two_warriors() -> Scene {
    shielded(
        scene().unit(SECOND, 2, Warrior).at(14.0, 4.0).targeting(ME),
        WARRIOR,
    )
}

#[test]
fn concussive_shot_peels_the_melee_that_is_not_shielded() {
    let view = two_warriors()
        .cooldown(ME, AbilityType::FreezingTrap, 5.0)
        .cooldown(ME, AbilityType::FrostTrap, 5.0)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::ConcussiveShot, Some(SECOND))),
        "{}",
        describe(&view)
    );
}

#[test]
fn frost_trap_peels_the_melee_that_is_not_shielded() {
    let view = two_warriors()
        .cooldown(ME, AbilityType::FreezingTrap, 5.0)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FrostTrap, Some(SECOND))),
        "{}",
        describe(&view)
    );
}

/// The Peel play (TeamPlan) on a dying ally's attackers: the shielded Warrior
/// is passed over and the other one is slowed.
#[test]
fn the_peel_play_snares_the_attacker_that_is_not_shielded() {
    let view = two_warriors()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ALLY, 1, Priest)
        .at(12.0, 2.0)
        .health(0.3)
        .targeting(PRIEST)
        .target(WARRIOR, ALLY)
        .target(SECOND, ALLY)
        .cooldown(ME, AbilityType::FreezingTrap, 5.0)
        .decide(ME);
    assert!(
        view.situation.is_some_and(|s| s.ally_in_danger.is_some()),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::ConcussiveShot, Some(SECOND))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Sight of the landing
// ---------------------------------------------------------------------------

/// A pillar on the midpoint between the Hunter and the Warrior: the Frost Trap
/// peel would land inside it. The throw is held, traced `LosBlocked`.
#[test]
fn no_frost_trap_is_thrown_into_a_pillar() {
    let build = || {
        scene()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FrostTrap, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );

    let view = pillar(build(), 5.0, 0.0).decide(ME);
    assert!(view.trap_landings.is_empty(), "{}", describe(&view));
    assert!(
        matches!(
            view.rejected(AbilityType::FrostTrap),
            Some(RejectionReason::LosBlocked)
        ),
        "{}",
        describe(&view)
    );
}

/// The pressure trap: a Freezing Trap thrown onto the enemy healer while a
/// melee is on the Hunter. A pillar between the Hunter and the healer hides
/// the landing, and the throw is held.
#[test]
fn no_freezing_trap_is_thrown_behind_a_pillar() {
    let build = || {
        Scene::new()
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(WARRIOR)
            .unit(WARRIOR, 2, Warrior)
            .at(10.0, 0.0)
            .targeting(ME)
            .unit(PRIEST, 2, Priest)
            .at(0.0, 12.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FreezingTrap, Some(PRIEST))),
        "control: {}",
        describe(&control)
    );
    assert!(
        control
            .trap_landings
            .iter()
            .any(|(t, _)| *t == TrapType::Freezing),
        "control: {}",
        describe(&control)
    );

    let view = pillar(build(), 0.0, 6.0).decide(ME);
    assert!(
        !view
            .trap_landings
            .iter()
            .any(|(t, _)| *t == TrapType::Freezing),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::LosBlocked)
        ),
        "{}",
        describe(&view)
    );
}
