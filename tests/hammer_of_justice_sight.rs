//! Scene tests: Hammer of Justice is cast only on a target the Paladin can
//! SEE. Both paths that cast it on a chosen enemy — the rotation's
//! healer-preferring pick and the dip's committed enemy healer — ask range,
//! then sight (`class_ai::cast_reach`, the cast gate's own two checks), so a
//! pillar between the Paladin and its target keeps the stun off it.
//!
//! Every scene is a pair: the pillar between, and the same scene without it.

use arenasim::states::match_config::CharacterClass::{Paladin, Priest, Warrior};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{
    Aura, AuraType, HealerPosture, MovementDirective, MovementGoal, Posture,
};
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::AbilityType;
use bevy::prelude::Vec2;

const ME: &str = "paladin";
const ALLY: &str = "ally";
const WARRIOR: &str = "enemy warrior";
const HEALER: &str = "enemy priest";

/// A 1.5yd cylinder at (5, 0): squarely between a Paladin at the origin and
/// a unit at (9.9, 0), which stands inside Hammer of Justice's 10yd range.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(5.0, 0.0),
        radius: 1.5,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// The Paladin's Devotion Aura, already up, so the aura is not its first
/// choice.
fn devotion_aura() -> Aura {
    Aura {
        effect_type: AuraType::DamageTakenReduction,
        duration: 600.0,
        magnitude: 0.1,
        break_on_damage_threshold: -1.0,
        ability_name: "Devotion Aura".into(),
        ..Default::default()
    }
}

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

/// The rotation: a Paladin with no enemy healer to reserve the stun for,
/// facing an enemy Warrior 9.9yd off.
fn rotation_scene(obstacles: Vec<ObstacleVolume>) -> Scene {
    Scene::new()
        .unit(ME, 1, Paladin)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(9.9, 0.0)
        .targeting(ME)
        .aura_full(ME, devotion_aura())
        .obstacles(obstacles)
}

#[test]
fn rotation_hammer_of_justice_needs_sight() {
    let open = rotation_scene(Vec::new()).decide(ME);
    assert_eq!(
        open.chose(),
        Some((AbilityType::HammerOfJustice, Some(WARRIOR))),
        "in range and in sight: {}",
        describe(&open)
    );

    let blocked = rotation_scene(pillar()).decide(ME);
    assert_ne!(
        blocked.chose().map(|(ability, _)| ability),
        Some(AbilityType::HammerOfJustice),
        "a pillar between: {}",
        describe(&blocked)
    );
    assert!(
        matches!(
            blocked.rejected(AbilityType::HammerOfJustice),
            Some(RejectionReason::LosBlocked)
        ),
        "{}",
        describe(&blocked)
    );
}

/// The dip: the Paladin is mid-dip on the enemy Priest, 9.9yd off — in
/// range. An ally is with it, healthy, so nothing aborts the dip. `walking`:
/// it carries its dip walk (the `Entity` directive the dip issued) into the
/// tick; otherwise it comes in without one, as after a short CC killed it.
fn dip_scene(obstacles: Vec<ObstacleVolume>, walking: bool) -> Scene {
    let now = 20.0;
    let dip_until = now + 4.0;
    let scene = Scene::new()
        .now(now)
        .unit(ME, 1, Paladin)
        .targeting(HEALER)
        .unit(ALLY, 1, Warrior)
        .at(-3.0, 0.0)
        .targeting(HEALER)
        .unit(HEALER, 2, Priest)
        .at(9.9, 0.0)
        .unit(WARRIOR, 2, Warrior)
        .at(-20.0, 0.0)
        .targeting(ALLY)
        .aura_full(ME, devotion_aura())
        .obstacles(obstacles);
    let healer = scene.entity(HEALER);
    let mut posture = HealerPosture::new(now - 1.0);
    posture.posture = Posture::Dip;
    posture.dip_target = Some(healer);
    posture.dip_until = dip_until;
    let scene = scene.healer_posture(ME, posture);
    if walking {
        scene.directive(
            ME,
            MovementDirective {
                goal: MovementGoal::Entity(healer),
                expires: dip_until,
                committed_until: dip_until,
            },
        )
    } else {
        scene
    }
}

#[test]
fn dip_hammer_of_justice_needs_sight() {
    for walking in [true, false] {
        let open = dip_scene(Vec::new(), walking).decide(ME);
        assert_eq!(
            open.chose(),
            Some((AbilityType::HammerOfJustice, Some(HEALER))),
            "walking={walking}: the dip has reached the healer: {}",
            describe(&open)
        );
        assert!(
            open.movement
                .iter()
                .any(|m| m.trigger == MovementTrigger::DipComplete),
            "walking={walking}: the stun completes the dip: {:?}",
            open.movement
        );
        assert!(
            open.directive.is_none(),
            "walking={walking}: a completed dip stops its walk: {:?}",
            open.directive
        );

        // In range behind the pillar, the dip has not arrived: the stun stays
        // reserved for it, and the Paladin leaves the tick still walking AT
        // the healer (the walk is tangent-steered round the pillar) — not
        // holding in range facing cover, and not handing over a cast that
        // cannot land.
        let blocked_scene = dip_scene(pillar(), walking);
        let healer = blocked_scene.entity(HEALER);
        let blocked = blocked_scene.decide(ME);
        assert_ne!(
            blocked.chose().map(|(ability, _)| ability),
            Some(AbilityType::HammerOfJustice),
            "walking={walking}: a pillar between: {}",
            describe(&blocked)
        );
        assert!(
            matches!(
                blocked.rejected(AbilityType::HammerOfJustice),
                Some(RejectionReason::PreconditionUnmet { note })
                    if note == "HoJ reserved for enemy-healer dip"
            ),
            "walking={walking}: the dip has not arrived: {}",
            describe(&blocked)
        );
        assert_eq!(
            blocked.directive.map(|d| d.goal),
            Some(MovementGoal::Entity(healer)),
            "walking={walking}: the dip walks on at the healer"
        );
    }
}

/// The rotation's fallback: a PRESSURED Paladin (pressure releases the stun
/// from the dip reservation) with the enemy Priest 9yd off and an enemy
/// Warrior on it 6yd off, both in range. With nothing between, the stun goes
/// to the healer; with a pillar between the Paladin and the healer, it falls
/// back to the Warrior it can see rather than being refused.
fn pressured_scene(obstacles: Vec<ObstacleVolume>) -> Scene {
    let now = 20.0;
    let mut posture = HealerPosture::new(now - 0.5);
    posture.posture = Posture::Pressured;
    posture.hold_until = now + 1.0;
    Scene::new()
        .now(now)
        .unit(ME, 1, Paladin)
        .targeting(WARRIOR)
        .unit(ALLY, 1, Warrior)
        .at(-3.0, 0.0)
        .targeting(WARRIOR)
        .unit(HEALER, 2, Priest)
        .at(9.0, 0.0)
        .unit(WARRIOR, 2, Warrior)
        .at(0.0, 6.0)
        .targeting(ME)
        .aura_full(ME, devotion_aura())
        .healer_posture(ME, posture)
        .obstacles(obstacles)
}

#[test]
fn rotation_hammer_of_justice_falls_back_to_an_enemy_in_sight() {
    let open = pressured_scene(Vec::new()).decide(ME);
    assert_eq!(
        open.chose(),
        Some((AbilityType::HammerOfJustice, Some(HEALER))),
        "nothing between: the healer first: {}",
        describe(&open)
    );

    let blocked = pressured_scene(pillar()).decide(ME);
    assert_eq!(
        blocked.chose(),
        Some((AbilityType::HammerOfJustice, Some(WARRIOR))),
        "the healer behind a pillar: the Warrior in sight: {}",
        describe(&blocked)
    );
}
