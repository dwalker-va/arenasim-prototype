//! AS-209 — under `TeamPlan` the Paladin's band-hold parks it only where the
//! heal reaches a dying teammate.
//!
//! A PRESSURED Paladin whose threats are all at or beyond `fallback_range`
//! band-holds: it parks where it stands to heal. Under `TeamPlan` that park
//! ran before the team solve, so the solve's dying-teammate reach (`C_DYING`)
//! never applied to the Paladin: it parked 45yd from a teammate below
//! `urgency_hp_threshold`, or with a pillar between them. The rule: while a
//! dying teammate is out of reach from where the Paladin stands (beyond heal
//! range less `HEAL_REACH_MARGIN`, or out of sight — `C_DYING`, asked through
//! `team_solve::dying_unreached`), the band-hold steps aside and the solve
//! walks the Paladin into reach, as it already does for an owed dispel. Once
//! the teammate is reached the park resumes.
//!
//! Each scene is a pair with its one condition removed. `Legacy` keeps its
//! park.

use arenasim::states::match_config::CharacterClass::{Mage, Paladin, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{HealerPosture, MovementGoal, Posture};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use bevy::prelude::Vec2;

const PALADIN: &str = "paladin";
const WARRIOR: &str = "warrior";
const ENEMY: &str = "enemy";

/// Below `urgency_hp_threshold` (0.5).
const DYING: f32 = 0.45;
/// Above it: hurt, not dying.
const HURT: f32 = 0.55;

const NOW: f32 = 30.0;

/// 45yd east of the Paladin: in sight, beyond the 40yd heal range.
const OUT_OF_RANGE: (f32, f32) = (45.0, 0.0);
/// 30yd east: in heal range (and, with no pillar, in sight).
const IN_RANGE: (f32, f32) = (30.0, 0.0);

/// 39.5yd east: inside the 40yd heal range, outside its 1yd
/// `HEAL_REACH_MARGIN` — not reached, by the solve's own measure.
const INSIDE_RANGE_OUTSIDE_MARGIN: (f32, f32) = (39.5, 0.0);
/// 38.5yd east: inside the margin — reached.
const INSIDE_MARGIN: (f32, f32) = (38.5, 0.0);

/// A 6yd pillar at (15, 0), squarely between the Paladin at the origin and
/// the Warrior [`IN_RANGE`].
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(15.0, 0.0),
        radius: 6.0,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// A 2yd pillar at (-5, 6), off both the Paladin's line to the Warrior and its
/// line to the enemy Mage: cover the solve would walk to, out of the Mage's
/// sight, were it consulted. It is what tells a band-hold park from a solve
/// that is merely satisfied where it stands.
fn cover() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(-5.0, 6.0),
        radius: 2.0,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// The Paladin in PRESSURED, held there by its hysteresis floor.
fn pressured() -> HealerPosture {
    HealerPosture {
        posture: Posture::Pressured,
        hold_until: NOW + 10.0,
        ..HealerPosture::new(NOW - 1.0)
    }
}

/// A PRESSURED Paladin at the origin on team 1 under `profile`, its teammate
/// Warrior at `warrior` and `warrior_hp`, and an enemy Mage 20yd west —
/// beyond the 15yd `fallback_range`, so the band-hold's own condition holds —
/// targeting the Paladin.
fn scene(
    profile: AiProfile,
    warrior: (f32, f32),
    warrior_hp: f32,
    obstacles: Vec<ObstacleVolume>,
) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(PALADIN, 1, Paladin)
        .unit(WARRIOR, 1, Warrior)
        .at(warrior.0, warrior.1)
        .health(warrior_hp)
        .unit(ENEMY, 2, Mage)
        .at(-20.0, 0.0)
        .targeting(PALADIN)
        .healer_posture(PALADIN, pressured())
        .obstacles(obstacles)
}

fn goal(view: &DecisionView) -> Option<Vec2> {
    match view.directive.map(|d| d.goal) {
        Some(MovementGoal::Point(p)) => Some(Vec2::new(p.x, p.z)),
        _ => None,
    }
}

/// The band-hold: a Point goal where the Paladin stands.
fn parked(view: &DecisionView) -> bool {
    goal(view).is_some_and(|p| p.length() < 1e-3)
}

/// A Point goal that carries the Paladin east, toward the Warrior.
fn walks_toward_warrior(view: &DecisionView) -> bool {
    goal(view).is_some_and(|p| p.x > 0.5)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "directive {:?}; movement {:?}",
        view.directive.map(|d| d.goal),
        view.movement
    )
}

/// Out of heal range: the dying Warrior 45yd off. The band-hold steps aside
/// and the solve walks the Paladin toward it. Hurt but not dying, the same
/// Warrior owes no reach, and the Paladin parks — beside cover the solve would
/// have walked it to.
#[test]
fn a_dying_teammate_beyond_heal_range_lifts_the_park() {
    let dying = scene(AiProfile::TeamPlan, OUT_OF_RANGE, DYING, Vec::new()).decide(PALADIN);
    assert!(
        walks_toward_warrior(&dying),
        "the solve walks into reach: {}",
        describe(&dying)
    );

    let hurt = scene(AiProfile::TeamPlan, OUT_OF_RANGE, HURT, cover()).decide(PALADIN);
    assert!(parked(&hurt), "nobody dying: {}", describe(&hurt));
}

/// Out of sight: the dying Warrior is in heal range with a pillar between.
/// The band-hold steps aside and the solve walks the Paladin round it. With
/// no pillar between them the Warrior is reached, and the Paladin parks —
/// beside cover the solve would have walked it to.
#[test]
fn a_dying_teammate_behind_a_pillar_lifts_the_park() {
    let hidden = scene(AiProfile::TeamPlan, IN_RANGE, DYING, pillar()).decide(PALADIN);
    assert!(
        goal(&hidden).is_some_and(|p| p.length() > 0.5),
        "the solve walks for sight: {}",
        describe(&hidden)
    );

    let reached = scene(AiProfile::TeamPlan, IN_RANGE, DYING, cover()).decide(PALADIN);
    assert!(parked(&reached), "teammate reached: {}", describe(&reached));
}

/// The reach edge is the solve's: `heal_range` less `HEAL_REACH_MARGIN`. A
/// dying Warrior 39.5yd off — in heal range, outside the margin — is not
/// reached, so the Paladin walks. One 38.5yd off, inside the margin, is
/// reached, and the Paladin parks beside cover the solve would have walked
/// it to. The pair fails if the park measures reach with no margin, or with
/// a margin out of step with the solve's.
#[test]
fn the_park_measures_reach_with_the_solves_margin() {
    let edge = scene(
        AiProfile::TeamPlan,
        INSIDE_RANGE_OUTSIDE_MARGIN,
        DYING,
        cover(),
    )
    .decide(PALADIN);
    assert!(
        walks_toward_warrior(&edge),
        "outside the margin, the solve walks: {}",
        describe(&edge)
    );

    let inside = scene(AiProfile::TeamPlan, INSIDE_MARGIN, DYING, cover()).decide(PALADIN);
    assert!(parked(&inside), "inside the margin: {}", describe(&inside));
}

/// The rule is `TeamPlan`'s: `Legacy` has no solve and keeps its park beside
/// the same dying Warrior.
#[test]
fn legacy_keeps_the_park() {
    let view = scene(AiProfile::Legacy, OUT_OF_RANGE, DYING, Vec::new()).decide(PALADIN);
    assert!(parked(&view), "Legacy: {}", describe(&view));
}
