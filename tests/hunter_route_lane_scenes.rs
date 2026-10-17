//! Scene tests for the lane Freezing Trap round cover (AS-295). When a pillar
//! hides the lane — the midpoint toward the enemy healer, else the kill
//! target — the trap lands instead on an enemy's route round the pillar, at
//! the first point of it the Hunter can see; a throw still never lands out of
//! sight. Each scene is paired with its controls, the same scene with one
//! condition removed.
//!
//! The scene: a Hunter at the origin and an enemy Warrior 24yd out on +x,
//! coming for it round a pillar halfway between them — the lane is inside the
//! pillar. The Warrior faces the way its pursuit steers (the tangent of the
//! pillar), which is how a walking unit's heading reads in the snapshot. The
//! Hunter's shots are cooling and the Warrior's rage rules out the sting, so
//! the trap is the Hunter's one cast.

use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Warrior};
use arenasim::states::play_match::class_ai::hunter::TRAP_HELD_NO_SPRINGER;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{DRCategory, DRTracker, TrapType};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::map_geometry::{
    has_line_of_sight, steer_toward_goal, ObstacleVolume,
};
use arenasim::states::play_match::{AbilityType, Aura, AuraType};

use bevy::prelude::{Vec2, Vec3};

const ME: &str = "me";
const WARRIOR: &str = "enemy warrior";

const PILLAR: Vec2 = Vec2::new(12.0, 0.0);
const WARRIOR_AT: Vec2 = Vec2::new(24.0, 0.0);

fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: PILLAR,
        radius: 2.5,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// The way the Warrior's pursuit of the Hunter steers round the pillar.
fn route_heading() -> Vec2 {
    steer_toward_goal(&pillar(), WARRIOR_AT, Vec2::ZERO, 1.0).expect("the pillar is in the way")
}

/// The scene with the Warrior facing `heading`, and the pillar when `pillared`.
fn scene(heading: Vec2, pillared: bool) -> Scene {
    let s = Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(WARRIOR_AT.x, WARRIOR_AT.y)
        .facing(heading.x, heading.y)
        .targeting(ME)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
        .cooldown(ME, AbilityType::AimedShot, 5.0)
        .cooldown(ME, AbilityType::ArcaneShot, 5.0);
    if pillared {
        s.obstacles(pillar())
    } else {
        s
    }
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

fn freezing_landing(view: &DecisionView) -> Option<Vec3> {
    view.trap_landings
        .iter()
        .find(|(t, _)| *t == TrapType::Freezing)
        .map(|(_, at)| *at)
}

/// The points the Warrior's steered pursuit passes through, walked as
/// movement walks it, a body radius apart at most.
fn warrior_route() -> Vec<Vec2> {
    let mut pos = WARRIOR_AT;
    let mut route = vec![pos];
    for _ in 0..400 {
        if pos.length() <= 2.0 {
            break;
        }
        let dir = steer_toward_goal(&pillar(), pos, Vec2::ZERO, 1.0)
            .unwrap_or_else(|| (-pos).normalize());
        pos += dir * 0.1;
        route.push(pos);
    }
    route
}

/// The card's rule: the lane is hidden, so the trap lands on the Warrior's
/// route round the pillar, where the Hunter can see it — aimed at the
/// Warrior, off the hidden lane, inside the throw's range.
#[test]
fn a_hidden_lane_trap_lands_on_the_route_round_the_pillar_in_sight() {
    let view = scene(route_heading(), true).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FreezingTrap, Some(WARRIOR))),
        "{}",
        describe(&view)
    );
    let at = freezing_landing(&view).expect("a Freezing Trap thrown");
    let me = Vec3::new(0.0, 1.0, 0.0);
    assert!(
        has_line_of_sight(&pillar(), me, Vec3::new(at.x, 1.0, at.z)),
        "landed out of sight at {at:?}"
    );
    let on_route = warrior_route()
        .iter()
        .map(|p| p.distance(Vec2::new(at.x, at.z)))
        .fold(f32::INFINITY, f32::min);
    assert!(on_route < 0.5, "{at:?} is {on_route}yd off the route");
    assert!(
        Vec2::new(at.x, at.z).distance(PILLAR) > 2.5,
        "landed in the pillar at {at:?}"
    );
    assert!(
        Vec2::new(at.x, at.z).length() <= 30.0,
        "beyond range: {at:?}"
    );
}

/// Control: the same Warrior not walking the route (facing away from it) has
/// no route to predict, so the hidden lane stands, decided as it always was:
/// nobody is heading into it, and the trap is held.
#[test]
fn a_walker_heading_elsewhere_leaves_the_hidden_lane_held() {
    let view = scene(-route_heading(), true).decide(ME);
    assert_eq!(freezing_landing(&view), None, "{}", describe(&view));
    assert!(
        matches!(
            view.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::PreconditionUnmet { note }) if note == TRAP_HELD_NO_SPRINGER
        ),
        "{}",
        describe(&view)
    );
}

/// Control: a Warrior heading along its route but planted there by a root
/// does not walk it, so it predicts no route either.
#[test]
fn a_rooted_walker_has_no_route() {
    let view = scene(route_heading(), true)
        .aura(WARRIOR, AuraType::Root, 5.0)
        .decide(ME);
    assert_eq!(freezing_landing(&view), None, "{}", describe(&view));
}

/// Control: with no pillar the lane is seen, and the trap lands on it — the
/// midpoint — exactly as it always has.
#[test]
fn a_seen_lane_is_thrown_where_it_always_was() {
    let view = scene(route_heading(), false).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FreezingTrap, Some(WARRIOR))),
        "{}",
        describe(&view)
    );
    let at = freezing_landing(&view).expect("a Freezing Trap thrown");
    assert!(
        (at.x - 12.0).abs() < 0.01 && at.z.abs() < 0.01,
        "landed at {at:?}"
    );
}

/// A route is asked for only when the lane is hidden. Here a narrower pillar
/// stands near the Warrior, off the lane: its pursuit still steers round it
/// (a route exists), but the Hunter sees the lane, and the trap lands there.
#[test]
fn a_seen_lane_is_kept_though_a_route_exists() {
    let near_pillar = vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(17.0, 0.0),
        radius: 1.5,
        base_y: 0.0,
        height: 10.0,
    }];
    let heading = steer_toward_goal(&near_pillar, WARRIOR_AT, Vec2::ZERO, 1.0)
        .expect("the pillar is in the Warrior's way");
    let view = scene(heading, false)
        .obstacles(near_pillar.clone())
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FreezingTrap, Some(WARRIOR))),
        "{}",
        describe(&view)
    );
    let at = freezing_landing(&view).expect("a Freezing Trap thrown");
    assert!(
        (at.x - 12.0).abs() < 0.01 && at.z.abs() < 0.01,
        "landed off the lane at {at:?}"
    );
}

/// Who springs a route landing is predicted along the route, not straight
/// along the walker's heading. A Warrior tucked against the far side of a
/// pillar sets off almost sideways to round it; carried straight on, that
/// heading never comes near where its route leaves the pillar's shadow, so
/// only the route predicts it springs the trap there. The lane toward the
/// Hunter's target, a Mage beyond the pillar, is hidden by the same pillar.
#[test]
fn a_route_landing_is_sprung_along_the_route_not_the_heading() {
    const MAGE: &str = "enemy mage";
    let pillar = vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(20.0, 0.0),
        radius: 2.5,
        base_y: 0.0,
        height: 10.0,
    }];
    let tucked = Vec2::new(24.5, 0.0);
    let heading = steer_toward_goal(&pillar, tucked, Vec2::ZERO, 1.0).expect("blocked");
    let mut diminished = DRTracker::default();
    while !diminished.is_immune(DRCategory::Incapacitates) {
        diminished.apply(DRCategory::Incapacitates);
    }
    let view = Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(MAGE)
        .unit(WARRIOR, 2, Warrior)
        .at(tucked.x, tucked.y)
        .facing(heading.x, heading.y)
        .targeting(ME)
        .unit(MAGE, 2, Mage)
        .at(40.0, 0.0)
        .casting(MAGE, AbilityType::Frostbolt)
        .dr_tracker(WARRIOR, diminished)
        .obstacles(pillar.clone())
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
        .cooldown(ME, AbilityType::SerpentSting, 5.0)
        .cooldown(ME, AbilityType::AimedShot, 5.0)
        .cooldown(ME, AbilityType::ArcaneShot, 5.0)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FreezingTrap, Some(WARRIOR))),
        "{}",
        describe(&view)
    );
    let at = freezing_landing(&view).expect("a Freezing Trap thrown");
    // Straight on from where it stands, the Warrior's heading passes the
    // landing no nearer than the trigger radius.
    let ray = Vec2::new(heading.x, heading.y).normalize();
    let to = Vec2::new(at.x, at.z) - tucked;
    let off_ray = (to - ray * to.dot(ray).max(0.0)).length();
    assert!(off_ray > 5.0, "the heading passes {off_ray}yd from {at:?}");
}

/// The route lane decides on who would spring it, as every lane does: a
/// Warrior immune to the trap (Divine Shield) holds it — by the lane's own
/// pick, traced `TargetImmune`, ahead of the friendly-DoT guard the Hunter's
/// Serpent Sting on it would otherwise trip, so the sting's reservation of
/// the trap's victim agrees with the throw. The Hunter's target is an enemy
/// Mage behind the pillar (an immune target stops the whole rotation), so the
/// lane toward it is hidden too.
#[test]
fn a_route_lane_trap_is_held_for_an_immune_walker() {
    const MAGE: &str = "enemy mage";
    // Diminished to immunity against incapacitates, so the Warrior is no
    // off-target the rotation would hold the trap for: the lane is the one
    // path left (as the AS-281 lane scenes stage it).
    let mut diminished = DRTracker::default();
    while !diminished.is_immune(DRCategory::Incapacitates) {
        diminished.apply(DRCategory::Incapacitates);
    }
    let build = || {
        scene(route_heading(), true)
            .dr_tracker(WARRIOR, diminished.clone())
            .unit(MAGE, 2, Mage)
            .at(26.0, 0.5)
            .target(ME, MAGE)
            .cooldown(ME, AbilityType::SerpentSting, 5.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FreezingTrap, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );

    let s = build();
    let shield = Aura {
        effect_type: AuraType::DamageImmunity,
        duration: 8.0,
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let sting = Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 10.0,
        magnitude: 5.0,
        ability_name: "Serpent Sting".into(),
        caster: Some(s.entity(ME)),
        ..Default::default()
    };
    let view = s
        .aura_full(WARRIOR, shield)
        .aura_full(WARRIOR, sting)
        .decide(ME);
    assert_eq!(freezing_landing(&view), None, "{}", describe(&view));
    assert!(
        matches!(
            view.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::TargetImmune)
        ),
        "{}",
        describe(&view)
    );
}
