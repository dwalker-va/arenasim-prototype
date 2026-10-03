//! Every `MovementGoal::Point` a `TeamPlan` unit is sent to on Nagrand is a
//! spot it can stand on (AS-190).
//!
//! The team solve once picked spots INSIDE a pillar's footprint: its local ring
//! and its steps toward an ally are offsets from the unit, and an in-pillar
//! point is occluded from everyone, so `OccupyCover` preferred it. The executor
//! stops a body radius short of a goal it cannot reach (AS-181), so the
//! position track never shows the bad goal — this reads the goal itself, off
//! the observer's `point_goal`, every frame.
//!
//! Every match below is a seed on which the unfixed solve issued such a goal,
//! from a Priest (the first, second and fourth matches), a Shaman (the third)
//! or a Druid (the fifth). The fourth also runs a Paladin through the solve.
//! A rebase that moves a seed off the defect leaves the guard looking at
//! nothing there, so re-check them against an unfixed build when the solve's
//! inputs change.

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{position_blocked, ObstacleVolume, MOVER_RADIUS};
use arenasim::ArenaMap;
use bevy::prelude::Vec2;

/// `(team 1, team 2, seed)` — each issued an in-pillar goal before the fix.
const MATCHES: [(&[&str], &[&str], u64); 5] = [
    (&["Warrior", "Priest"], &["Warlock", "Priest"], 1),
    (&["Rogue", "Priest"], &["Hunter", "Priest"], 1),
    (&["Rogue", "Shaman"], &["Warlock", "Paladin"], 1),
    (&["Warrior", "Paladin"], &["Mage", "Priest"], 2),
    (&["Warrior", "Druid"], &["Hunter", "Priest"], 6),
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
fn team_plan_point_goals_are_standable_on_nagrand() {
    let volumes = nagrand();
    assert!(
        !volumes.is_empty(),
        "PillaredArena loaded with no obstacles"
    );

    let mut offenders: Vec<String> = Vec::new();
    let mut goal_frames = 0usize;
    let mut near_skin_frames = 0usize;

    for (team1, team2, seed) in MATCHES {
        let label = format!("{team1:?} vs {team2:?} seed {seed}");
        run_headless_match_observed(
            HeadlessMatchConfig {
                team1: team1.iter().map(|s| s.to_string()).collect(),
                team2: team2.iter().map(|s| s.to_string()).collect(),
                map: "PillaredArena".to_string(),
                ai_profile: Some("TeamPlan".to_string()),
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
                    if position_blocked(&volumes, goal) && offenders.len() < 10 {
                        offenders.push(format!(
                            "{label} t={:.2}: {:?} (team {} slot {}) sent to {goal:?}, \
                             standing at {:?}",
                            f.sim_time, obs.class, obs.team, obs.slot, obs.position
                        ));
                    }
                }
            },
        )
        .expect("match runs");
    }

    assert!(
        offenders.is_empty(),
        "TeamPlan issued Point goals inside an obstacle footprint:\n{}",
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
