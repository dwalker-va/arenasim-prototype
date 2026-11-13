//! AS-394, AS-404 — the endgame Hunt play, scene by scene (tier 1).
//!
//! One pillar of radius 6 at the origin. The enemy's last unit, a Priest,
//! hugs its east face at (7, 0); our Mage and Priest stand west of it, out of
//! its sight. Each test is a pair: the scene, and its control with the one
//! condition removed.
//!
//! The hunt's axis is the hider's bearing round the pillar's centre (east,
//! 0°). The Mage, the nearer to the north, takes the north side (counter-
//! clockwise of the hider) and the Priest the south. A blind hunter walks to
//! the nearest spot on its own side that sees the hider clear of the pillar's
//! widened shadow; a hunter that sees the hider stands where it is.

use arenasim::states::match_config::CharacterClass::{Mage, Paladin, Priest, Warlock, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::hunt::{in_shadow, HuntWindow, HUNT_ENTER, HUNT_LEAVE};
use arenasim::states::play_match::class_ai::scene::Scene;
use arenasim::states::play_match::components::MovementGoal;
use arenasim::states::play_match::decision_trace::{MovementTrigger, PlayTag};
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume};
use arenasim::states::play_match::situation::{ClassSet, Play, PlaySet};
use arenasim::states::play_match::AuraType;
use bevy::prelude::{Vec2, Vec3};

const MAGE: &str = "mage";
const WARLOCK: &str = "warlock";
const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const HIDER: &str = "enemy priest";
const ENEMY_WARRIOR: &str = "enemy warrior";

const SOUTH: Vec2 = Vec2::new(0.0, -20.0);

/// Where the hider hugs the pillar's east face.
const HIDER_AT: Vec2 = Vec2::new(7.0, 0.0);

fn pillar() -> ObstacleVolume {
    ObstacleVolume::Cylinder {
        center_xz: Vec2::ZERO,
        radius: 6.0,
        base_y: 0.0,
        height: 10.0,
    }
}

/// Our Mage and Priest west of the pillar; the enemy Priest, alone, hugging
/// its east face.
fn scene() -> Scene {
    Scene::new()
        .obstacles(vec![pillar()])
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(MAGE, 1, Mage)
        .at(-20.0, 2.0)
        .targeting(HIDER)
        .unit(PRIEST, 1, Priest)
        .at(-22.0, -4.0)
        .targeting(HIDER)
        .unit(HIDER, 2, Priest)
        .at(7.0, 0.0)
        .targeting(MAGE)
}

fn hunt(scene: &mut Scene) -> Option<HuntWindow> {
    scene.reclassify().teams[0].hunt.clone()
}

fn station(h: &HuntWindow, scene: &Scene, who: &'static str) -> Option<Vec2> {
    h.station_of(scene.entity(who))
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 0.5
}

fn slot(h: &HuntWindow, scene: &Scene, who: &'static str) -> Option<u8> {
    let unit = scene.entity(who);
    h.stations.iter().find(|s| s.unit == unit).map(|s| s.slot)
}

/// Whether a unit at `p` sees a hider at `hider` clear of the pillar's
/// widened shadow — what a pincer spot must do.
fn sees_clear(p: Vec2, hider: Vec2) -> bool {
    let eye = |q: Vec2| Vec3::new(q.x, 1.0, q.y);
    has_line_of_sight(&[pillar()], eye(p), eye(hider)) && !in_shadow(&[pillar()], 0, p, hider)
}

/// The signed angle from the hider's bearing to `p`'s round the pillar, in
/// degrees: positive is counter-clockwise (the north side here).
fn side_of(p: Vec2, hider: Vec2) -> f32 {
    let d = (p.to_angle() - hider.to_angle() + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    d.to_degrees()
}

#[test]
fn a_hidden_lone_healer_is_hunted_from_opposite_sides() {
    let mut s = scene();
    let h = hunt(&mut s).expect("the team hunts");
    assert_eq!(h.subject, s.entity(HIDER));
    assert_eq!(h.pillar, 0);
    assert_eq!(slot(&h, &s, MAGE), Some(0), "the Mage takes the north side");
    assert_eq!(slot(&h, &s, PRIEST), Some(1), "the Priest the south");
    let mage = station(&h, &s, MAGE).expect("the Mage hunts");
    let priest = station(&h, &s, PRIEST).expect("the Priest hunts");
    assert!(side_of(mage, HIDER_AT) > 0.0, "{mage}: north of the hider");
    assert!(side_of(priest, HIDER_AT) < 0.0, "{priest}: south of it");
    assert!(sees_clear(mage, HIDER_AT) && sees_clear(priest, HIDER_AT));

    // Control: the same team on the healer's side of the pillar sees it, so
    // nothing is hidden and no hunt begins.
    let mut seen = scene();
    seen.set_position(MAGE, 20.0, 2.0);
    seen.set_position(PRIEST, 22.0, -4.0);
    assert_eq!(hunt(&mut seen), None);
}

#[test]
fn the_healer_must_stand_at_a_pillar() {
    let mut s = scene();
    s.set_position(HIDER, 6.0 + HUNT_ENTER + 4.0, 0.0);
    assert_eq!(hunt(&mut s), None, "beyond the enter radius");
    let mut s = scene();
    s.set_position(HIDER, 6.0 + HUNT_ENTER - 0.5, 0.0);
    assert!(hunt(&mut s).is_some(), "inside it");
}

#[test]
fn a_hunt_holds_in_the_pillars_shadow_and_ends_in_the_open() {
    let mut s = scene();
    assert!(hunt(&mut s).is_some());
    s.set_position(HIDER, 6.0 + HUNT_LEAVE - 0.5, 0.0);
    assert!(hunt(&mut s).is_some(), "held between enter and leave");
    // Beyond the leave radius, but behind the pillar from both hunters: the
    // hider still uses it as cover.
    s.set_position(HIDER, 6.0 + HUNT_LEAVE + 0.5, 0.0);
    assert!(hunt(&mut s).is_some(), "held in the shadow");
    // Control: the same spot with the hunters out where they see it clear.
    s.set_position(MAGE, 5.0, 25.0);
    s.set_position(PRIEST, 5.0, -25.0);
    assert_eq!(hunt(&mut s), None, "released in the open");
    // And within the leave radius, in the open, it is still held.
    let mut s = scene();
    assert!(hunt(&mut s).is_some());
    s.set_position(MAGE, 5.0, 25.0);
    s.set_position(PRIEST, 5.0, -25.0);
    s.set_position(HIDER, 6.0 + HUNT_LEAVE - 0.5, 0.0);
    assert!(hunt(&mut s).is_some(), "near the pillar");
}

#[test]
fn a_healer_that_runs_to_another_pillar_is_hunted_there() {
    let second = ObstacleVolume::Cylinder {
        center_xz: Vec2::new(30.0, 0.0),
        radius: 6.0,
        base_y: 0.0,
        height: 10.0,
    };
    let mut s = scene().obstacles(vec![pillar(), second]);
    assert_eq!(hunt(&mut s).unwrap().pillar, 0);
    // On the far face of the second pillar, out of every hunter's sight.
    s.set_position(HIDER, 37.0, 0.0);
    let h = hunt(&mut s).expect("a new hunt");
    assert_eq!(h.pillar, 1);
    assert_eq!(h.stations.len(), 2);
}

#[test]
fn only_a_lone_healer_is_hunted() {
    // A second enemy alive: not an endgame.
    let mut s = scene()
        .unit(ENEMY_WARRIOR, 2, Warrior)
        .at(-25.0, 10.0)
        .targeting(PRIEST);
    assert_eq!(hunt(&mut s), None, "two enemies");
    // The lone enemy is not a healer.
    let mut s = Scene::new()
        .obstacles(vec![pillar()])
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(MAGE, 1, Mage)
        .at(-20.0, 2.0)
        .unit(PRIEST, 1, Priest)
        .at(-22.0, -4.0)
        .unit(HIDER, 2, Warlock)
        .at(7.0, 0.0);
    assert_eq!(hunt(&mut s), None, "a lone Warlock");
    // A lone Paladin is a healer.
    let mut s = Scene::new()
        .obstacles(vec![pillar()])
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(MAGE, 1, Mage)
        .at(-20.0, 2.0)
        .unit(PRIEST, 1, Priest)
        .at(-22.0, -4.0)
        .unit(HIDER, 2, Paladin)
        .at(7.0, 0.0);
    assert!(hunt(&mut s).is_some(), "a lone Paladin");
}

#[test]
fn a_team_of_one_does_not_hunt() {
    let mut s = Scene::new()
        .obstacles(vec![pillar()])
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(MAGE, 1, Mage)
        .at(-20.0, 2.0)
        .unit(HIDER, 2, Priest)
        .at(7.0, 0.0);
    assert_eq!(hunt(&mut s), None);
}

#[test]
fn legacy_classifies_no_hunt_and_an_excepted_team_stations_nobody() {
    let mut legacy = scene().ai_profile(1, AiProfile::Legacy);
    assert_eq!(hunt(&mut legacy), None, "Legacy");
    // The graduated play's control: every class excepted. The hunt is still
    // classified (and traced); nobody on the team takes a station.
    let mut excepted = scene().except_everywhere(1, PlaySet::of(&[Play::Hunt]));
    let h = hunt(&mut excepted).expect("still classified");
    assert!(h.stations.is_empty(), "excepted everywhere: {h:?}");
    let mut other = scene().except_everywhere(2, PlaySet::of(&[Play::Hunt]));
    assert_eq!(
        hunt(&mut other).map(|h| h.stations.len()),
        Some(2),
        "excepted on the other team only"
    );
    let mut bare = scene().obstacles(Vec::new());
    assert_eq!(hunt(&mut bare), None, "no pillar");
}

#[test]
fn two_ranged_dps_take_the_stations_before_the_healer() {
    let mut s = scene().unit(WARLOCK, 1, Warlock).at(-20.0, -2.0);
    let h = hunt(&mut s).unwrap();
    assert!(station(&h, &s, MAGE).is_some());
    assert!(station(&h, &s, WARLOCK).is_some());
    assert_eq!(
        station(&h, &s, PRIEST),
        None,
        "the healer keeps its own solve"
    );
}

#[test]
fn a_melee_takes_no_station_and_one_viewer_is_no_hunt() {
    // Warrior and Priest: the Priest is the only unit that can station, and
    // one viewer cannot out-angle a hugger.
    let mut s = Scene::new()
        .obstacles(vec![pillar()])
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(WARRIOR, 1, Warrior)
        .at(-20.0, 2.0)
        .unit(PRIEST, 1, Priest)
        .at(-22.0, -4.0)
        .unit(HIDER, 2, Priest)
        .at(7.0, 0.0);
    let h = hunt(&mut s).expect("the hunt is classified");
    assert!(h.stations.is_empty(), "{:?}", h.stations);
    // Control: Warrior, Mage and Priest — the Mage and the Priest station.
    let mut s = scene().unit(WARRIOR, 1, Warrior).at(-20.0, 6.0);
    let h = hunt(&mut s).unwrap();
    assert_eq!(station(&h, &s, WARRIOR), None);
    assert!(station(&h, &s, MAGE).is_some() && station(&h, &s, PRIEST).is_some());
    assert_ne!(play_of(&s.decide(WARRIOR)), Some(PlayTag::Hunt));
    assert_eq!(play_of(&s.decide(MAGE)), Some(PlayTag::Hunt));
}

#[test]
fn the_healer_stations_only_while_no_teammate_is_dying() {
    // The Mage below `urgency_hp_threshold`: the Priest owes it its sight,
    // takes no station, and the Mage alone is no hunt.
    let mut s = scene().health_of(MAGE, 0.4);
    let h = hunt(&mut s).unwrap();
    assert!(h.owed);
    assert!(h.stations.is_empty());
    // Hurt but not dying: nothing is owed.
    let mut s = scene().health_of(MAGE, 0.6);
    let h = hunt(&mut s).unwrap();
    assert!(!h.owed);
    assert_eq!(h.stations.len(), 2);
    // Once owed, it stays owed until the Mage is back to `SIGHT_YIELD_HP`.
    let mut s = scene().health_of(MAGE, 0.4);
    assert!(hunt(&mut s).unwrap().owed);
    s.set_health(MAGE, 0.7);
    assert!(hunt(&mut s).unwrap().owed, "the release bar is higher");
    s.set_health(MAGE, 0.85);
    assert!(!hunt(&mut s).unwrap().owed, "released");
    // The healer's own health owes nothing: it heals itself from anywhere.
    let mut s = scene().health_of(PRIEST, 0.4);
    let h = hunt(&mut s).unwrap();
    assert!(!h.owed);
    assert_eq!(h.stations.len(), 2);
}

#[test]
fn the_shipped_table_excepts_every_healer_but_the_priest() {
    use arenasim::states::match_config::CharacterClass::{Druid, Shaman};
    for healer in [Paladin, Druid, Shaman] {
        let team = || {
            Scene::new()
                .obstacles(vec![pillar()])
                .ai_profile(1, AiProfile::TeamPlan)
                .unit(MAGE, 1, Mage)
                .at(-20.0, 2.0)
                .unit(PRIEST, 1, healer)
                .at(-22.0, -4.0)
                .unit(HIDER, 2, Priest)
                .at(7.0, 0.0)
        };
        let mut s = team();
        assert!(hunt(&mut s).unwrap().stations.is_empty(), "{healer:?}");
        // Control: the table lifted, the same healer stations.
        let mut s = team().lift(1, PlaySet::of(&[Play::Hunt]));
        assert_eq!(hunt(&mut s).unwrap().stations.len(), 2, "{healer:?} lifted");
    }
}

#[test]
fn a_side_is_kept_by_the_unit_that_took_it() {
    let mut s = scene();
    hunt(&mut s).unwrap();
    // Swap the two units' sides: the crossed pairing is now the shorter walk,
    // but each keeps the side it took, and walks to a spot on it.
    s.set_position(MAGE, -10.0, -22.0);
    s.set_position(PRIEST, -10.0, 22.0);
    let held = hunt(&mut s).unwrap();
    assert_eq!(slot(&held, &s, MAGE), Some(0));
    assert!(side_of(station(&held, &s, MAGE).unwrap(), HIDER_AT) > 0.0);
    // Control: a fresh hunt from those positions pairs them the other way.
    let mut fresh = scene();
    fresh.set_position(MAGE, -10.0, -22.0);
    fresh.set_position(PRIEST, -10.0, 22.0);
    let other = hunt(&mut fresh).unwrap();
    assert_eq!(slot(&other, &fresh, MAGE), Some(1));
    assert!(side_of(station(&other, &fresh, MAGE).unwrap(), HIDER_AT) < 0.0);
}

#[test]
fn no_hunt_before_the_gates() {
    let mut s = scene().before_gates();
    assert_eq!(hunt(&mut s), None);
}

#[test]
fn a_hunt_with_one_station_left_holds_neither() {
    // A wall across the north half takes every spot the north station could
    // be pulled in or turned to.
    let wall = ObstacleVolume::Aabb {
        min: Vec3::new(-30.0, 0.0, 8.0),
        max: Vec3::new(30.0, 5.0, 23.0),
    };
    let mut s = scene().obstacles(vec![pillar(), wall]);
    let h = hunt(&mut s).expect("the hunt is classified");
    assert!(h.stations.is_empty(), "{:?}", h.stations);
    let mut s = scene();
    assert_eq!(hunt(&mut s).unwrap().stations.len(), 2);
}

#[test]
fn an_excepted_class_takes_no_station() {
    // The Mage excepted: only the Priest is left, so no stations.
    let mut s = scene().exceptions(Play::Hunt, ClassSet::of(&[Mage]));
    assert!(hunt(&mut s).unwrap().stations.is_empty());
    // A class not on the team excepted: as shipped.
    let mut s = scene().exceptions(Play::Hunt, ClassSet::of(&[Warlock]));
    assert_eq!(hunt(&mut s).unwrap().stations.len(), 2);
}

#[test]
fn the_sides_carry_while_the_healer_runs_round_its_pillar() {
    let mut s = scene();
    let first = hunt(&mut s).unwrap();
    s.set_position(HIDER, 5.0, 5.0);
    s.set_now(31.0);
    let held = hunt(&mut s).unwrap();
    assert_eq!(held.axis, first.axis);
    assert_eq!(held.since, first.since, "the onset carries");
    assert_eq!(slot(&held, &s, MAGE), slot(&first, &s, MAGE));
    assert_eq!(slot(&held, &s, PRIEST), slot(&first, &s, PRIEST));
    // The sides move with the hider: each walks to a spot on its own side
    // of the hider's new bearing (45°), from which it sees it.
    let hider = Vec2::new(5.0, 5.0);
    let mage = station(&held, &s, MAGE).unwrap();
    let priest = station(&held, &s, PRIEST).unwrap();
    assert!(side_of(mage, hider) > 0.0 && side_of(priest, hider) < 0.0);
    assert!(sees_clear(mage, hider) && sees_clear(priest, hider));
    // Control: a hunt that begins with the healer on the north-east face takes
    // its axis there.
    let mut fresh = scene();
    fresh.set_position(HIDER, 5.0, 5.0);
    let other = hunt(&mut fresh).unwrap();
    assert!((other.axis - first.axis).abs() > 0.5);
}

#[test]
fn a_hunter_that_sees_stands_and_one_that_is_blind_walks() {
    // The Mage out east, in front of the hider: it sees it, and stands where
    // it is. The Priest, west and blind, walks.
    let mut s = scene();
    s.set_position(MAGE, 20.0, 5.0);
    let h = hunt(&mut s).unwrap();
    assert_eq!(station(&h, &s, MAGE), Some(Vec2::new(20.0, 5.0)), "stands");
    let priest = station(&h, &s, PRIEST).unwrap();
    assert!(priest.distance(Vec2::new(-22.0, -4.0)) > 5.0, "walks");
    // The hider steps round to the west face: the Mage has lost it, and
    // walks too.
    s.set_position(HIDER, -7.0, 0.0);
    let h = hunt(&mut s).unwrap();
    let mage = station(&h, &s, MAGE).unwrap();
    assert!(mage.distance(Vec2::new(20.0, 5.0)) > 5.0, "{mage}: walks");
    assert!(sees_clear(mage, Vec2::new(-7.0, 0.0)));
}

#[test]
fn a_hunter_keeps_standing_until_it_loses_plain_sight() {
    // At (-2, 18) the Mage sees the hider, but inside the pillar's widened
    // shadow: not clear.
    let edge = Vec2::new(-2.0, 18.0);
    let eye = |q: Vec2| Vec3::new(q.x, 1.0, q.y);
    assert!(has_line_of_sight(&[pillar()], eye(edge), eye(HIDER_AT)));
    assert!(in_shadow(&[pillar()], 0, edge, HIDER_AT));
    // Arriving there from where it saw clear, it keeps standing.
    let mut s = scene();
    s.set_position(MAGE, 20.0, 5.0);
    hunt(&mut s).unwrap();
    s.set_position(MAGE, edge.x, edge.y);
    let h = hunt(&mut s).unwrap();
    assert_eq!(station(&h, &s, MAGE), Some(edge), "still stands");
    // Control: a Mage that starts there has not yet seen it clear, and walks
    // on to a spot that does.
    let mut s = scene();
    s.set_position(MAGE, edge.x, edge.y);
    let h = hunt(&mut s).unwrap();
    let mage = station(&h, &s, MAGE).unwrap();
    assert!(
        mage.distance(edge) > 1.0 && sees_clear(mage, HIDER_AT),
        "{mage}"
    );
}

fn play_of(view: &arenasim::states::play_match::class_ai::scene::DecisionView) -> Option<PlayTag> {
    view.situation.as_ref().and_then(|s| s.play)
}

fn point_goal(view: &arenasim::states::play_match::class_ai::scene::DecisionView) -> Option<Vec2> {
    view.directive.and_then(|d| match d.goal {
        MovementGoal::Point(p) => Some(Vec2::new(p.x, p.z)),
        _ => None,
    })
}

#[test]
fn the_mage_walks_round_its_side_toward_sight() {
    let mut s = scene();
    let h = s.reclassify().teams[0].hunt.clone().unwrap();
    let goal = station(&h, &s, MAGE).unwrap();
    let view = s.decide(MAGE);
    assert_eq!(play_of(&view), Some(PlayTag::Hunt));
    assert!(
        point_goal(&view).is_some_and(|p| near(p, goal)),
        "{:?}",
        view.directive
    );
    assert!(side_of(goal, HIDER_AT) > 0.0 && sees_clear(goal, HIDER_AT));
    assert!(view
        .movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::Hunt));
    // Control: withheld, the Mage seeks its own angle instead.
    let mut s = scene().except_everywhere(1, PlaySet::of(&[Play::Hunt]));
    s.reclassify();
    let view = s.decide(MAGE);
    assert_eq!(play_of(&view), None);
    assert!(!point_goal(&view).is_some_and(|p| near(p, goal)));
    assert!(view
        .movement
        .iter()
        .all(|m| m.trigger != MovementTrigger::Hunt));
}

#[test]
fn the_priest_walks_round_its_side_toward_sight() {
    let mut s = scene();
    let h = s.reclassify().teams[0].hunt.clone().unwrap();
    let goal = station(&h, &s, PRIEST).unwrap();
    let view = s.decide(PRIEST);
    assert_eq!(play_of(&view), Some(PlayTag::Hunt));
    assert!(
        point_goal(&view).is_some_and(|p| near(p, goal)),
        "{:?}",
        view.directive
    );
    assert!(side_of(goal, HIDER_AT) < 0.0 && sees_clear(goal, HIDER_AT));
    let mut s = scene().except_everywhere(1, PlaySet::of(&[Play::Hunt]));
    s.reclassify();
    let view = s.decide(PRIEST);
    assert!(!point_goal(&view).is_some_and(|p| near(p, goal)));
}

#[test]
fn the_warlock_walks_round_its_side_toward_sight() {
    let warlock = || {
        Scene::new()
            .obstacles(vec![pillar()])
            .ai_profile(1, AiProfile::TeamPlan)
            .unit(WARLOCK, 1, Warlock)
            .at(-20.0, 2.0)
            .targeting(HIDER)
            .unit(PRIEST, 1, Priest)
            .at(-22.0, -4.0)
            .unit(HIDER, 2, Priest)
            .at(7.0, 0.0)
    };
    let mut s = warlock();
    let h = s.reclassify().teams[0].hunt.clone().unwrap();
    let goal = station(&h, &s, WARLOCK).unwrap();
    let view = s.decide(WARLOCK);
    assert!(
        point_goal(&view).is_some_and(|p| near(p, goal)),
        "{:?}",
        view.directive
    );
    assert!(side_of(goal, HIDER_AT) > 0.0 && sees_clear(goal, HIDER_AT));
    let mut s = warlock().except_everywhere(1, PlaySet::of(&[Play::Hunt]));
    s.reclassify();
    assert!(s.decide(WARLOCK).directive.is_none());
}

#[test]
fn the_hunt_stands_down_while_the_healer_is_locked() {
    let goal = {
        let mut s = scene();
        let h = hunt(&mut s).unwrap();
        station(&h, &s, MAGE).unwrap()
    };
    let mut s = scene().aura(HIDER, AuraType::Stun, 4.0);
    s.reclassify();
    let view = s.decide(MAGE);
    assert_ne!(play_of(&view), Some(PlayTag::Hunt));
    assert!(!point_goal(&view).is_some_and(|p| near(p, goal)));
    // Control: a Root holds its legs but not its casts — not a lock.
    let mut s = scene().aura(HIDER, AuraType::Root, 4.0);
    s.reclassify();
    assert_eq!(play_of(&s.decide(MAGE)), Some(PlayTag::Hunt));
}

#[test]
fn a_stale_station_walk_is_renewed_and_a_live_one_is_left_alone() {
    use arenasim::states::play_match::components::MovementDirective;
    let goal = {
        let mut s = scene();
        let h = hunt(&mut s).unwrap();
        station(&h, &s, MAGE).unwrap()
    };
    let walk = |expires: f32| MovementDirective {
        goal: MovementGoal::Point(Vec3::new(goal.x, 1.0, goal.y)),
        expires,
        committed_until: expires,
    };
    // Expired: renewed past now.
    let mut s = scene().directive(MAGE, walk(29.0));
    s.reclassify();
    let view = s.decide(MAGE);
    assert!(view.directive.is_some_and(|d| d.expires > 30.0));
    assert!(
        view.movement
            .iter()
            .all(|m| m.trigger != MovementTrigger::Hunt),
        "same point: no new walk traced"
    );
    // Live: left as it is.
    let mut s = scene().directive(MAGE, walk(30.4));
    s.reclassify();
    let view = s.decide(MAGE);
    assert!(view.directive.is_some_and(|d| d.expires == 30.4));
}

#[test]
fn a_healer_whose_hunt_ends_gives_its_station_walk_back() {
    use arenasim::states::play_match::components::{HealerPosture, MovementDirective, Posture};
    let walk = MovementDirective {
        goal: MovementGoal::Point(Vec3::new(SOUTH.x, 1.0, SOUTH.y)),
        expires: 60.0,
        committed_until: 60.0,
    };
    // Our Priest PRESSURED by the enemy Priest beside it: a posture that
    // keeps a committed walk until its window runs out.
    let posture = |station: Option<Vec2>| {
        let mut p = HealerPosture::new(0.0);
        p.posture = Posture::Pressured;
        p.hold_until = 60.0;
        p.hunt_station = station;
        p
    };
    let pressed = |station: Option<Vec2>| {
        let mut s = scene()
            .except_everywhere(1, PlaySet::of(&[Play::Hunt]))
            .healer_posture(PRIEST, posture(station))
            .directive(PRIEST, walk);
        s.set_position(PRIEST, 12.0, -2.0);
        s.set_target(HIDER, PRIEST);
        s.reclassify();
        point_goal(&s.decide(PRIEST)).is_some_and(|p| near(p, SOUTH))
    };
    // The hunt over (withheld): a Priest that remembers its station drops
    // the walk.
    assert!(!pressed(Some(SOUTH)));
    // Control: a walk the hunt never gave is the posture's own, and is kept.
    assert!(pressed(None));
}
