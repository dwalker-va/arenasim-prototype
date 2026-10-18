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

use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Rogue, Shaman, Warrior};
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
/// no route to predict, and the hidden lane is no landing the Hunter would
/// throw, so the trap is held — traced `LosBlocked`, as the placement traces a
/// hidden landing.
#[test]
fn a_walker_heading_elsewhere_leaves_the_hidden_lane_held() {
    let view = scene(-route_heading(), true).decide(ME);
    assert_eq!(freezing_landing(&view), None, "{}", describe(&view));
    assert!(
        matches!(
            view.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::LosBlocked)
        ),
        "{}",
        describe(&view)
    );
}

/// Control: with the pillar gone, the Warrior walking away is nobody heading
/// into the lane, and the trap is held for that.
#[test]
fn a_seen_lane_nobody_walks_into_is_held_for_no_springer() {
    let view = scene(-route_heading(), false).decide(ME);
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

/// The Serpent Sting reserves the enemy the lane trap would catch, and that
/// reservation must agree with the throw (AS-361).
///
/// The scene: the Hunter alone (with a teammate, the team would break a trap
/// on the Hunter's own target, and the lane trap would be held for that)
/// against an enemy Shaman, which cannot free a Freezing Trap and is
/// diminished to immunity against it, so it is no off-target, and a Rogue out
/// of stealth, the Hunter's target, 24yd out. The lane toward the Shaman is
/// inside a pillar that the Rogue's path to the Hunter grazes: the Hunter sees
/// past it to the Rogue (so the sting has its target in sight), but the Rogue,
/// body radius and all, has to steer round it.
///
/// - **A seen route** — the Rogue pursuing the Hunter round the pillar: the
///   trap lands on its route, so the sting is reserved and the trap thrown.
/// - **No route** — the same Rogue, facing the same way, with no target to
///   pursue, so no route is predicted. Carried straight on it still walks
///   into the hidden lane, but no throw lands there: the trap is held for
///   sight, and the sting, which waited on that throw before, goes out.
#[test]
fn the_sting_waits_for_a_route_trap_and_not_for_a_hidden_lane() {
    const ROGUE: &str = "enemy rogue";
    const SHAMAN: &str = "enemy shaman";
    // Clear of the Hunter's sightline to the Rogue by 0.3yd, inside the
    // Rogue's body radius of it.
    let grazed = vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(12.0, 2.8),
        radius: 2.5,
        base_y: 0.0,
        height: 10.0,
    }];
    let rogue_at = Vec2::new(24.0, 0.0);
    let shaman_at = Vec2::new(24.0, 3.0);
    let heading = steer_toward_goal(&grazed, rogue_at, Vec2::ZERO, 1.0)
        .expect("the Rogue's body grazes the pillar");
    let eye = |p: Vec2| Vec3::new(p.x, 1.0, p.y);
    assert!(has_line_of_sight(&grazed, eye(Vec2::ZERO), eye(rogue_at)));
    assert!(!has_line_of_sight(
        &grazed,
        eye(Vec2::ZERO),
        eye(shaman_at / 2.0)
    ));
    let mut diminished = DRTracker::default();
    while !diminished.is_immune(DRCategory::Incapacitates) {
        diminished.apply(DRCategory::Incapacitates);
    }
    let build = |pursuing: bool| {
        let s = Scene::new()
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(ROGUE)
            .unit(SHAMAN, 2, Shaman)
            .at(shaman_at.x, shaman_at.y)
            .dr_tracker(SHAMAN, diminished.clone())
            .unit(ROGUE, 2, Rogue)
            .at(rogue_at.x, rogue_at.y)
            .facing(heading.x, heading.y);
        let s = if pursuing { s.targeting(ME) } else { s };
        let mut s = s
            .obstacles(grazed.clone())
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
            .cooldown(ME, AbilityType::AimedShot, 5.0)
            .cooldown(ME, AbilityType::ArcaneShot, 5.0);
        // A Rogue opens stealthed; this one has been seen.
        s.combatant_mut(ROGUE).stealthed = false;
        s
    };
    let reserved = |view: &DecisionView| {
        matches!(
            view.rejected(AbilityType::SerpentSting),
            Some(RejectionReason::PreconditionUnmet { note })
                if note == "trap victim reserved for Freezing Trap"
        )
    };

    let route = build(true).decide(ME);
    assert!(reserved(&route), "seen route: {}", describe(&route));
    assert_eq!(
        route.chose(),
        Some((AbilityType::FreezingTrap, Some(ROGUE))),
        "seen route: {}",
        describe(&route)
    );

    let hidden = build(false).decide(ME);
    assert_eq!(
        hidden.chose(),
        Some((AbilityType::SerpentSting, Some(ROGUE))),
        "no route: {}",
        describe(&hidden)
    );
    // With the sting cooling, the trap itself is reached: held for sight.
    let held = build(false)
        .cooldown(ME, AbilityType::SerpentSting, 5.0)
        .decide(ME);
    assert_eq!(freezing_landing(&held), None, "{}", describe(&held));
    assert!(
        matches!(
            held.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::LosBlocked)
        ),
        "no route, sting cooling: {}",
        describe(&held)
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

/// `heading` turned `degrees` anticlockwise on the ground plane.
fn turned(heading: Vec2, degrees: f32) -> Vec2 {
    let (s, c) = degrees.to_radians().sin_cos();
    Vec2::new(heading.x * c - heading.y * s, heading.x * s + heading.y * c)
}

/// How closely a walker's heading must follow its route for the route to be
/// believed (`ROUTE_HEADING_AGREEMENT`, cos 30°): a Warrior heading 20° off
/// its route is taken to be walking it, and one heading 45° off is not, so the
/// hidden lane stands and is held.
#[test]
fn a_heading_within_thirty_degrees_of_the_route_follows_it() {
    for degrees in [20.0, -20.0] {
        let view = scene(turned(route_heading(), degrees), true).decide(ME);
        assert_eq!(
            view.chose(),
            Some((AbilityType::FreezingTrap, Some(WARRIOR))),
            "{degrees}° off: {}",
            describe(&view)
        );
    }
    for degrees in [45.0, -45.0] {
        let view = scene(turned(route_heading(), degrees), true).decide(ME);
        assert_eq!(
            freezing_landing(&view),
            None,
            "{degrees}° off: {}",
            describe(&view)
        );
    }
}

/// While an enemy the dip would trap is in view — an off-target, here the
/// enemy Warrior, which the team is not killing and nobody could free — the
/// rotation holds Freezing Trap for the dip and throws no lane trap, whether
/// the lane is open or hidden behind a pillar with a route round it. The
/// control diminishes the Warrior to immunity against incapacitates, so it is
/// no off-target: the same lane is then thrown, at it.
#[test]
fn no_lane_trap_while_an_off_target_waits_for_the_dip() {
    const MAGE: &str = "enemy mage";
    let warrior_at = Vec2::new(36.0, 0.0);
    let pillar = vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(20.0, 0.0),
        radius: 2.5,
        base_y: 0.0,
        height: 10.0,
    }];
    let build = |hidden: bool, off_target: bool| {
        let heading = if hidden {
            steer_toward_goal(&pillar, warrior_at, Vec2::ZERO, 1.0).expect("blocked")
        } else {
            Vec2::new(-1.0, 0.0)
        };
        let mut s = Scene::new()
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(MAGE)
            .unit(WARRIOR, 2, Warrior)
            .at(warrior_at.x, warrior_at.y)
            .facing(heading.x, heading.y)
            .targeting(ME)
            .unit(MAGE, 2, Mage)
            .at(40.0, 0.0)
            .casting(MAGE, AbilityType::Frostbolt)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
            .cooldown(ME, AbilityType::SerpentSting, 5.0)
            .cooldown(ME, AbilityType::AimedShot, 5.0)
            .cooldown(ME, AbilityType::ArcaneShot, 5.0);
        if !off_target {
            let mut diminished = DRTracker::default();
            while !diminished.is_immune(DRCategory::Incapacitates) {
                diminished.apply(DRCategory::Incapacitates);
            }
            s = s.dr_tracker(WARRIOR, diminished);
        }
        if hidden {
            s = s.obstacles(pillar.clone());
        }
        s
    };
    for hidden in [false, true] {
        let control = build(hidden, false).decide(ME);
        assert_eq!(
            control.chose(),
            Some((AbilityType::FreezingTrap, Some(WARRIOR))),
            "control, lane hidden {hidden}: {}",
            describe(&control)
        );
        let view = build(hidden, true).decide(ME);
        assert_eq!(
            freezing_landing(&view),
            None,
            "lane hidden {hidden}: {}",
            describe(&view)
        );
    }
}
