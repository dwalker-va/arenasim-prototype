//! AS-394 — the endgame Hunt play, scene by scene (tier 1).
//!
//! One pillar of radius 6 at the origin. The enemy's last unit, a Priest,
//! hugs its east face at (7, 0); our Mage and Priest stand west of it, out of
//! its sight. Each test is a pair: the scene, and its control with the one
//! condition removed.
//!
//! The hunt's axis is the hider's bearing round the pillar's centre (east,
//! 0°), so its two stations stand 14yd off the skin at +90° (north, z = 20)
//! and -90° (south, z = -20); the Mage, the nearer to the north one, takes
//! it, and the Priest takes the south.

use arenasim::states::match_config::CharacterClass::{Mage, Paladin, Priest, Warlock, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::hunt::{HuntWindow, HUNT_ENTER, HUNT_LEAVE};
use arenasim::states::play_match::class_ai::scene::Scene;
use arenasim::states::play_match::components::MovementGoal;
use arenasim::states::play_match::decision_trace::{MovementTrigger, PlayTag};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::situation::{ClassSet, Play, PlaySet};
use arenasim::states::play_match::AuraType;
use bevy::prelude::Vec2;

const MAGE: &str = "mage";
const WARLOCK: &str = "warlock";
const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const HIDER: &str = "enemy priest";
const ENEMY_WARRIOR: &str = "enemy warrior";

const NORTH: Vec2 = Vec2::new(0.0, 20.0);
const SOUTH: Vec2 = Vec2::new(0.0, -20.0);

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

#[test]
fn a_hidden_lone_healer_is_hunted_from_opposite_sides() {
    let mut s = scene();
    let h = hunt(&mut s).expect("the team hunts");
    assert_eq!(h.subject, s.entity(HIDER));
    assert_eq!(h.pillar, 0);
    let mage = station(&h, &s, MAGE).expect("the Mage holds a station");
    let priest = station(&h, &s, PRIEST).expect("the Priest holds a station");
    assert!(near(mage, NORTH), "{mage}");
    assert!(near(priest, SOUTH), "{priest}");
    assert!(
        (mage.to_angle() - priest.to_angle()).abs() > 3.1,
        "half a turn apart"
    );

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
fn a_hunt_is_held_out_to_the_leave_radius_and_ends_beyond_it() {
    let mut s = scene();
    assert!(hunt(&mut s).is_some());
    s.set_position(HIDER, 6.0 + HUNT_LEAVE - 0.5, 0.0);
    assert!(hunt(&mut s).is_some(), "held between enter and leave");
    s.set_position(HIDER, 6.0 + HUNT_LEAVE + 0.5, 0.0);
    assert_eq!(hunt(&mut s), None, "released beyond leave");
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
fn legacy_and_a_withheld_hunt_classify_none() {
    let mut legacy = scene().ai_profile(1, AiProfile::Legacy);
    assert_eq!(hunt(&mut legacy), None, "Legacy");
    let mut withheld = scene().withhold(1, PlaySet::of(&[Play::Hunt]));
    assert_eq!(hunt(&mut withheld), None, "withheld");
    let mut other = scene().withhold(2, PlaySet::of(&[Play::Hunt]));
    assert!(
        hunt(&mut other).is_some(),
        "withheld from the other team only"
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
fn a_station_is_kept_by_the_unit_that_took_it() {
    let mut s = scene();
    let first = hunt(&mut s).unwrap();
    // Swap the two units' sides: the crossed pairing is now the shorter walk,
    // but each keeps the station it took.
    s.set_position(MAGE, -10.0, -22.0);
    s.set_position(PRIEST, -10.0, 22.0);
    let held = hunt(&mut s).unwrap();
    assert_eq!(station(&held, &s, MAGE), station(&first, &s, MAGE));
    // Control: a fresh hunt from those positions pairs them the other way.
    let mut fresh = scene();
    fresh.set_position(MAGE, -10.0, -22.0);
    fresh.set_position(PRIEST, -10.0, 22.0);
    let other = hunt(&mut fresh).unwrap();
    assert!(near(station(&other, &fresh, MAGE).unwrap(), SOUTH));
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
        min: bevy::prelude::Vec3::new(-30.0, 0.0, 8.0),
        max: bevy::prelude::Vec3::new(30.0, 5.0, 23.0),
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
fn the_axis_carries_while_the_healer_runs_round_its_pillar() {
    let mut s = scene();
    let first = hunt(&mut s).unwrap();
    s.set_position(HIDER, 5.0, 5.0);
    s.set_now(31.0);
    let held = hunt(&mut s).unwrap();
    assert_eq!(held.axis, first.axis);
    assert_eq!(held.since, first.since, "the onset carries");
    assert_eq!(held.stations, first.stations);
    // Control: a hunt that begins with the healer on the north-east face takes
    // its axis there.
    let mut fresh = scene();
    fresh.set_position(HIDER, 5.0, 5.0);
    let other = hunt(&mut fresh).unwrap();
    assert!((other.axis - first.axis).abs() > 0.5);
}

/// Two small blockers that hide the hider from both stations.
fn shields() -> Vec<ObstacleVolume> {
    [Vec2::new(3.5, 10.0), Vec2::new(3.5, -10.0)]
        .into_iter()
        .map(|c| ObstacleVolume::Cylinder {
            center_xz: c,
            radius: 1.5,
            base_y: 0.0,
            height: 10.0,
        })
        .collect()
}

#[test]
fn two_units_on_their_stations_that_both_lose_the_healer_give_them_up() {
    let mut obstacles = vec![pillar()];
    obstacles.extend(shields());
    let mut s = scene().obstacles(obstacles);
    s.set_position(MAGE, NORTH.x, NORTH.y);
    s.set_position(PRIEST, SOUTH.x, SOUTH.y);
    let h = hunt(&mut s).unwrap();
    assert_eq!(h.stations.len(), 2);
    assert!(h.stations.iter().all(|st| st.arrived && !st.hold));
    assert_eq!(station(&h, &s, MAGE), None);
    // Leaving the station for sight does not make it walk back: the arrival
    // is latched.
    s.set_position(MAGE, -5.0, 25.0);
    let h = hunt(&mut s).unwrap();
    assert_eq!(station(&h, &s, MAGE), None);

    // Control: one blocker only — the south station sees the healer, so both
    // hold.
    let mut obstacles = vec![pillar()];
    obstacles.push(shields()[0]);
    let mut s = scene().obstacles(obstacles);
    s.set_position(MAGE, NORTH.x, NORTH.y);
    s.set_position(PRIEST, SOUTH.x, SOUTH.y);
    let h = hunt(&mut s).unwrap();
    assert!(h.stations.iter().all(|st| st.hold));
    // And a unit still on its way holds, whoever sees.
    let mut obstacles = vec![pillar()];
    obstacles.extend(shields());
    let mut s = scene().obstacles(obstacles);
    let h = hunt(&mut s).unwrap();
    assert!(h.stations.iter().all(|st| !st.arrived && st.hold));
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
fn the_mage_walks_to_its_station() {
    let mut s = scene();
    s.reclassify();
    let view = s.decide(MAGE);
    assert_eq!(play_of(&view), Some(PlayTag::Hunt));
    assert!(
        point_goal(&view).is_some_and(|p| near(p, NORTH)),
        "{:?}",
        view.directive
    );
    assert!(view
        .movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::Hunt));
    // Control: withheld, the Mage seeks its own angle instead.
    let mut s = scene().withhold(1, PlaySet::of(&[Play::Hunt]));
    s.reclassify();
    let view = s.decide(MAGE);
    assert_eq!(play_of(&view), None);
    assert!(!point_goal(&view).is_some_and(|p| near(p, NORTH)));
    assert!(view
        .movement
        .iter()
        .all(|m| m.trigger != MovementTrigger::Hunt));
}

#[test]
fn the_priest_walks_to_its_station() {
    let mut s = scene();
    s.reclassify();
    let view = s.decide(PRIEST);
    assert_eq!(play_of(&view), Some(PlayTag::Hunt));
    assert!(
        point_goal(&view).is_some_and(|p| near(p, SOUTH)),
        "{:?}",
        view.directive
    );
    let mut s = scene().withhold(1, PlaySet::of(&[Play::Hunt]));
    s.reclassify();
    let view = s.decide(PRIEST);
    assert!(!point_goal(&view).is_some_and(|p| near(p, SOUTH)));
}

#[test]
fn the_warlock_walks_to_its_station() {
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
    s.reclassify();
    let view = s.decide(WARLOCK);
    assert!(
        point_goal(&view).is_some_and(|p| near(p, NORTH)),
        "{:?}",
        view.directive
    );
    let mut s = warlock().withhold(1, PlaySet::of(&[Play::Hunt]));
    s.reclassify();
    assert!(s.decide(WARLOCK).directive.is_none());
}

#[test]
fn the_hunt_stands_down_while_the_healer_is_locked() {
    let mut s = scene().aura(HIDER, AuraType::Stun, 4.0);
    s.reclassify();
    let view = s.decide(MAGE);
    assert_ne!(play_of(&view), Some(PlayTag::Hunt));
    assert!(!point_goal(&view).is_some_and(|p| near(p, NORTH)));
    // Control: a Root holds its legs but not its casts — not a lock.
    let mut s = scene().aura(HIDER, AuraType::Root, 4.0);
    s.reclassify();
    assert_eq!(play_of(&s.decide(MAGE)), Some(PlayTag::Hunt));
}

#[test]
fn a_stale_station_walk_is_renewed_and_a_live_one_is_left_alone() {
    use arenasim::states::play_match::components::MovementDirective;
    let walk = |expires: f32| MovementDirective {
        goal: MovementGoal::Point(bevy::prelude::Vec3::new(NORTH.x, 1.0, NORTH.y)),
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
        "same station: no new walk traced"
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
        goal: MovementGoal::Point(bevy::prelude::Vec3::new(SOUTH.x, 1.0, SOUTH.y)),
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
            .withhold(1, PlaySet::of(&[Play::Hunt]))
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
