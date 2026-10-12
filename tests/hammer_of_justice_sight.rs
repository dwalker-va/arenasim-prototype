//! Scene tests: Hammer of Justice is cast only on a target the Paladin can
//! SEE. Both paths that cast it on a chosen enemy — the rotation's
//! healer-preferring pick and the dip's committed enemy healer — ask range,
//! then sight (`class_ai::cast_reach`, the cast gate's own two checks), so a
//! pillar between the Paladin and its target keeps the stun off it.
//!
//! Every scene is a pair: the pillar between, and the same scene without it.

use arenasim::states::match_config::CharacterClass::{Paladin, Priest, Warrior};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{Aura, AuraType, HealerPosture, Posture};
use arenasim::states::play_match::decision_trace::RejectionReason;
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
/// range. An ally is with it, healthy, so nothing aborts the dip.
fn dip_scene(obstacles: Vec<ObstacleVolume>) -> Scene {
    let now = 20.0;
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
    let mut posture = HealerPosture::new(now - 1.0);
    posture.posture = Posture::Dip;
    posture.dip_target = Some(scene.entity(HEALER));
    posture.dip_until = now + 4.0;
    scene.healer_posture(ME, posture)
}

#[test]
fn dip_hammer_of_justice_needs_sight() {
    let open = dip_scene(Vec::new()).decide(ME);
    assert_eq!(
        open.chose(),
        Some((AbilityType::HammerOfJustice, Some(HEALER))),
        "the dip has reached the healer: {}",
        describe(&open)
    );

    // In range behind the pillar, the dip has not arrived: the stun stays
    // reserved for it and the walk goes on round the pillar, rather than the
    // dip handing over a cast that cannot land.
    let blocked = dip_scene(pillar()).decide(ME);
    assert_ne!(
        blocked.chose().map(|(ability, _)| ability),
        Some(AbilityType::HammerOfJustice),
        "a pillar between: {}",
        describe(&blocked)
    );
    assert!(
        matches!(
            blocked.rejected(AbilityType::HammerOfJustice),
            Some(RejectionReason::PreconditionUnmet { note })
                if note == "HoJ reserved for enemy-healer dip"
        ),
        "the dip is still walking: {}",
        describe(&blocked)
    );
    assert!(
        blocked
            .movement
            .iter()
            .all(|m| m.posture == arenasim::states::play_match::decision_trace::Posture::Dip),
        "the dip neither completes nor aborts: {:?}",
        blocked.movement
    );
}
