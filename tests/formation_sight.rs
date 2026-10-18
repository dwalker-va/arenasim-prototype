//! AS-206 — a FREE healer regains sight of its teammates.
//!
//! The FREE formation point (the engaged-ally centroid offset behind the line)
//! used to take no account of sight, so on a pillar map it could stand a
//! healer in a pillar's shadow from a teammate it had no reason to hide from.
//! Nothing then drew it out: the medic chase walks only for a teammate below
//! `urgency_hp_threshold`. Under `TeamPlan` the point now keeps sight of every
//! teammate when it can (of the most injured when it cannot;
//! `class_ai::formation`).
//!
//! The probe stages the pillar edge the defect was found on (tier 3) for each
//! formation machine — the Druid's (the caster-healer machine it shares with
//! the Shaman) and the Priest's — and pins the walk out of the shadow to within
//! a second, paired with `Legacy` keeping the healer blind.

use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::ArenaMap;
use bevy::prelude::Vec3;

/// The shipped `urgency_hp_threshold`: below it the medic chase owns the walk.
const URGENCY: f32 = 0.5;
const FRAME: f32 = 1.0 / 60.0;

fn nagrand() -> Vec<ObstacleVolume> {
    load_map_geometry_config()
        .expect("assets/config/maps.ron must load")
        .active_for(ArenaMap::PillaredArena)
        .volumes
}

fn eye(p: Vec3) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.z)
}

// ---------------------------------------------------------------------------
// The staged pillar edge (tier 3)
// ---------------------------------------------------------------------------

/// The pillar edge the defect was found on (AS-205's stage, with the Mage
/// healthy): Warrior + Paladin vs Mage + a healer on Nagrand, round the pillar
/// at (-40, -20). The Mage is held against the pillar's north face by the
/// Paladin's Hammer of Justice, at 90% — above `urgency_hp_threshold`, so no
/// medic chase. The Paladin is the enemy nearest the Mage, so the healer's
/// formation point (behind the Mage, away from the Paladin) falls inside the
/// pillar and is projected onto its east face: out of the Mage's sight. The
/// healer has no mana, so nothing it casts holds it still — the scene is its
/// walk alone.
mod staged {
    use super::*;
    use arenasim::headless::{Stage, Unit};
    use arenasim::states::play_match::AbilityType;

    pub const WARRIOR: Unit = Unit::new(1, 0);
    pub const PALADIN: Unit = Unit::new(1, 1);
    pub const MAGE: Unit = Unit::new(2, 0);
    pub const HEALER: Unit = Unit::new(2, 1);

    pub fn config(seed: u64, profile: &str, healer: &str) -> HeadlessMatchConfig {
        HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Paladin".into()],
            team2: vec!["Mage".into(), healer.into()],
            map: "PillaredArena".to_string(),
            ai_profile: Some(profile.to_string()),
            team1_kill_target: Some(0),
            team2_kill_target: Some(0),
            random_seed: Some(seed),
            max_duration_secs: 6.0,
            ..Default::default()
        }
    }

    pub fn stage() -> Stage {
        Stage::new()
            .place(WARRIOR, -44.5, -15.8)
            .place(PALADIN, -43.5, -12.9)
            .place(MAGE, -42.6, -14.1)
            .health(MAGE, 0.9)
            .land(MAGE, AbilityType::HammerOfJustice, PALADIN)
            .cooldown(PALADIN, AbilityType::HammerOfJustice, 60.0)
            .place(HEALER, -35.8, -15.5)
            .mana(HEALER, 0.0)
    }
}

mod common;
use common::staged::{play, Played};

/// The window the scene is read over: inside the Mage's stun, and before the
/// pair on it can take it below the urgency line on any seed.
const STUNNED: f32 = 3.5;
/// The staged probe's N: a FREE healer regains sight of its Mage within this.
const STAGED_REGAIN: f32 = 1.0;

/// Per staged frame up to `STUNNED`: (seconds since the stage, the healer sees
/// the Mage, the healer's point goal sees the Mage, the Mage's health).
fn sight_track(
    played: &Played,
    obstacles: &[ObstacleVolume],
) -> Vec<(f32, bool, Option<bool>, f32)> {
    played
        .staged_frames()
        .enumerate()
        .map(|(k, f)| {
            let get = |team: u8, slot: u8| {
                f.combatants
                    .values()
                    .find(|c| c.team == team && c.slot == slot && !c.is_pet)
                    .expect("the unit")
            };
            let (mage, healer) = (get(2, 0), get(2, 1));
            let sees = |p: Vec3| has_line_of_sight(obstacles, eye(p), eye(mage.position));
            (
                k as f32 * FRAME,
                sees(healer.position),
                healer.point_goal.map(sees),
                mage.current_health / mage.max_health,
            )
        })
        .take_while(|&(t, ..)| t < STUNNED)
        .collect()
}

/// The formation machines: the caster-healer machine (the Druid's, shared
/// with the Shaman) and the Priest's. Each applies the constraint at the end
/// of its own `compute_formation_point`.
const HEALERS: [&str; 2] = ["Druid", "Priest"];

/// The probe. Under `TeamPlan` the healer walks out of the Mage's shadow
/// within `STAGED_REGAIN` and keeps sight for the rest of the stun, and every
/// point it is sent to sees the Mage. Its pair is `Legacy`, the one condition
/// removed (the unconstrained point): the same stage keeps the healer blind for
/// most of the window — so the stage still contains the defect.
#[test]
fn a_free_healer_regains_sight_of_its_teammate() {
    let obstacles = nagrand();
    for healer in HEALERS {
        for seed in [1, 2, 3] {
            let track = sight_track(
                &play(staged::config(seed, "TeamPlan", healer), staged::stage()),
                &obstacles,
            );
            assert!(
                track.iter().all(|&(.., hp)| hp >= URGENCY),
                "{healer} seed {seed}: the Mage fell below the urgency line — the medic \
                 chase, not the formation point, owns the walk"
            );
            assert!(
                !track[0].1,
                "{healer} seed {seed}: the stage no longer starts the healer blind"
            );
            let regained = track
                .iter()
                .position(|&(_, sees, ..)| sees)
                .map(|k| track[k].0);
            assert!(
                regained.is_some_and(|t| t <= STAGED_REGAIN),
                "{healer} seed {seed}: regained sight of the Mage at {regained:?}, \
                 not within {STAGED_REGAIN}s"
            );
            let blind_after = track
                .iter()
                .filter(|&&(t, sees, ..)| t > STAGED_REGAIN && !sees)
                .count();
            assert_eq!(blind_after, 0, "{healer} seed {seed}: lost sight again");
            let blind_goals = track
                .iter()
                .filter(|&&(_, _, goal, _)| goal == Some(false))
                .count();
            assert_eq!(
                blind_goals, 0,
                "{healer} seed {seed}: sent to a point out of sight"
            );

            // The pair: the unconstrained point keeps the healer blind.
            let control = sight_track(
                &play(staged::config(seed, "Legacy", healer), staged::stage()),
                &obstacles,
            );
            let blind = control.iter().filter(|&&(_, sees, ..)| !sees).count() as f32 * FRAME;
            assert!(
                blind >= 3.0,
                "{healer} seed {seed}: under Legacy the healer was blind only {blind:.2}s \
                 — the stage no longer puts its formation point in the Mage's shadow"
            );
        }
    }
}
