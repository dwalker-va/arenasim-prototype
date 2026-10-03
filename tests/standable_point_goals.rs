//! Every `MovementGoal::Point` a `TeamPlan` healer is sent to on Nagrand is a
//! spot it can stand on (AS-190).
//!
//! The executor stops a body radius short of a goal it cannot reach (AS-181),
//! so the position track never shows a bad goal — this reads the goal itself,
//! off the observer's `point_goal`, every frame.
//!
//! Two producers issued goals inside a pillar's footprint:
//!
//! - the team solve (`team_solve::solve_unit`, `TeamPlan` only), whose local
//!   ring and ally steps are offsets from a healer pressed against a pillar,
//!   and an in-pillar point is occluded from everyone, so `OccupyCover`
//!   preferred it;
//! - the FREE formation point (`compute_formation_point` in `priest.rs` and
//!   `caster_healer_posture.rs`), the ally centroid offset behind the line,
//!   which takes no account of cover. Its projection is `TeamPlan` only;
//!   `Legacy`'s in-pillar formation point is AS-203.
//!
//! Each case fails with the named projection removed (grouped below by which):
//! removing the solve's fails 8 cases, with Priest, Shaman, Paladin and Druid
//! goals; removing the formation point's fails 4, with Druid and Shaman goals.
//! A rebase that moves a seed off the defect leaves the guard looking at
//! nothing there, so re-run that mutation check when either producer's inputs
//! change.

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{position_blocked, ObstacleVolume, MOVER_RADIUS};
use arenasim::ArenaMap;
use bevy::prelude::Vec2;

/// `(profile, team 1, team 2, seed)` — each issued an in-pillar goal before
/// the fix.
const CASES: [(&str, &[&str], &[&str], u64); 10] = [
    // Fail without the solve's projection only. First offender: a Priest,
    // Priest, Shaman, Paladin, Priest, Druid.
    (
        "TeamPlan",
        &["Warrior", "Priest"],
        &["Warlock", "Priest"],
        1,
    ),
    ("TeamPlan", &["Rogue", "Priest"], &["Hunter", "Priest"], 1),
    ("TeamPlan", &["Rogue", "Shaman"], &["Warlock", "Paladin"], 1),
    (
        "TeamPlan",
        &["Hunter", "Paladin"],
        &["Warrior", "Shaman"],
        3,
    ),
    ("TeamPlan", &["Hunter", "Druid"], &["Rogue", "Priest"], 1),
    ("TeamPlan", &["Mage", "Druid"], &["Warlock", "Priest"], 1),
    // Fail without either projection: a Priest via the solve, a Druid via
    // the formation point.
    ("TeamPlan", &["Warrior", "Druid"], &["Hunter", "Priest"], 6),
    ("TeamPlan", &["Warrior", "Druid"], &["Hunter", "Priest"], 1),
    // Fail without the formation point's projection only (Druid, Shaman).
    ("TeamPlan", &["Hunter", "Paladin"], &["Mage", "Druid"], 2),
    ("TeamPlan", &["Rogue", "Shaman"], &["Mage", "Druid"], 1),
    // Legacy's in-pillar formation point is AS-203.
];

/// How far outside a pillar's skin a goal still counts as one placed AT the
/// pillar — the neighbourhood the defect lived in, counted so a run that never
/// sent anyone near cover cannot pass for a clean one.
const NEAR_SKIN: f32 = 1.0;

fn nagrand() -> Vec<ObstacleVolume> {
    load_map_geometry_config()
        .expect("assets/config/maps.ron must load")
        .active_for(ArenaMap::PillaredArena)
        .volumes
}

#[test]
fn healer_point_goals_are_standable_on_nagrand() {
    let volumes = nagrand();
    assert!(
        !volumes.is_empty(),
        "PillaredArena loaded with no obstacles"
    );

    // One line per offending case (its frame count and first offence), so a
    // failure names every case that regressed, not just the first few frames.
    let mut offenders: Vec<String> = Vec::new();
    let mut goal_frames = 0usize;
    let mut near_skin_frames = 0usize;

    for (profile, team1, team2, seed) in CASES {
        let label = format!("{profile} {team1:?} vs {team2:?} seed {seed}");
        let mut bad_frames = 0usize;
        let mut first: Option<String> = None;
        run_headless_match_observed(
            HeadlessMatchConfig {
                team1: team1.iter().map(|s| s.to_string()).collect(),
                team2: team2.iter().map(|s| s.to_string()).collect(),
                map: "PillaredArena".to_string(),
                ai_profile: Some(profile.to_string()),
                max_duration_secs: 300.0,
                random_seed: Some(seed),
                ..Default::default()
            },
            true,
            None,
            |f| {
                for obs in f.combatants.values() {
                    let Some(goal) = obs.point_goal.filter(|_| obs.alive) else {
                        continue;
                    };
                    goal_frames += 1;
                    let g = Vec2::new(goal.x, goal.z);
                    if volumes.iter().any(|v| {
                        let (c, r) = v.footprint_disc();
                        g.distance(c) < r + MOVER_RADIUS + NEAR_SKIN
                    }) {
                        near_skin_frames += 1;
                    }
                    if position_blocked(&volumes, goal) {
                        bad_frames += 1;
                        first.get_or_insert_with(|| {
                            format!(
                                "t={:.2}: {:?} (team {} slot {}) sent to {goal:?}, \
                                 standing at {:?}",
                                f.sim_time, obs.class, obs.team, obs.slot, obs.position
                            )
                        });
                    }
                }
            },
        )
        .expect("match runs");
        if let Some(first) = first {
            offenders.push(format!("{label}: {bad_frames} frames, first {first}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "Point goals inside an obstacle footprint:\n{}",
        offenders.join("\n")
    );
    // Non-vacuity: goals were issued, and some of them hug a pillar — the only
    // place an in-footprint goal can arise.
    assert!(goal_frames > 0, "no Point goals observed at all");
    assert!(
        near_skin_frames > 0,
        "no Point goal within {NEAR_SKIN}yd of a pillar's skin across {goal_frames} \
         goal-frames — the guard never looked where the defect lives"
    );
    println!(
        "{goal_frames} Point goal-frames, {near_skin_frames} within {NEAR_SKIN}yd of a pillar"
    );
}
