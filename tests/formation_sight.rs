//! AS-206 — a FREE healer regains sight of its teammates.
//!
//! The FREE formation point (the engaged-ally centroid offset behind the line)
//! used to take no account of sight, so on a pillar map it could stand a
//! healer in a pillar's shadow from a teammate it had no reason to hide from.
//! Nothing then drew it out: the medic chase walks only for a teammate below
//! `urgency_hp_threshold`. Under `TeamPlan` the point now keeps sight of every
//! teammate when it can (of the most injured when it cannot;
//! `class_ai::formation`), and a healer parked at a blind point is re-sent.
//!
//! Two probes:
//! - `a_free_druid_regains_sight_of_its_teammate` (default run) stages the
//!   pillar edge the defect was found on (tier 3) and pins the walk out of the
//!   shadow to within a second, paired with `Legacy` keeping the Druid blind;
//! - `a_free_healer_regains_sight_of_its_teammates` (`#[ignore]`d, release) is
//!   situation-conditional over a seed bank: it finds its windows — spells in
//!   which a FREE healer, unhurried by any dying teammate, cannot see a
//!   teammate — off the per-frame observer and the trace's posture
//!   transitions, and bounds how long each lasts. No seed is pinned to a
//!   situation.
//!
//! ```bash
//! cargo test --release --test formation_sight -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
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

/// One healer's blind spells in one match, in seconds.
#[derive(Default)]
struct Spells {
    /// Spells that ended — the healer regained sight, or left FREE / the
    /// situation ended — each with whether sight came back.
    closed: Vec<(f32, bool)>,
    /// Where each closed spell began, on the combat clock.
    starts: Vec<f32>,
    /// The match and healer, for the printout.
    label: String,
    /// Frames the healer was FREE with every teammate healthy and alive.
    free_frames: usize,
    /// Of those, frames blind to at least one of them.
    blind_frames: usize,
}

/// Play one match and collect, for every formation healer on a team running
/// `profile_of(team)`, its blind spells: maximal runs of frames in which it is
/// FREE (per the trace's posture transitions), alive, every living non-pet
/// teammate is at or above `URGENCY`, and at least one of them is out of its
/// sight.
fn blind_spells(
    team1: &[&str],
    team2: &[&str],
    profiles: (&str, &str),
    seed: u64,
    obstacles: &[ObstacleVolume],
) -> Vec<Spells> {
    let dir = tempfile::tempdir().expect("scratch");
    // `PROBE_TRACE_DIR` keeps every trace, named for its match, to read a
    // spell the printout lists.
    let trace_path = match std::env::var("PROBE_TRACE_DIR") {
        Ok(keep) => std::path::Path::new(&keep).join(format!(
            "{}_v_{}_{}_{}_{seed}.jsonl",
            team1.join(""),
            team2.join(""),
            profiles.0,
            profiles.1
        )),
        Err(_) => dir.path().join("trace.jsonl"),
    };
    type Frame = (bool, BTreeMap<u32, (u8, bool, bool, Vec3, f32)>);
    let mut frames: Vec<Frame> = Vec::new();
    run_headless_match_observed(
        HeadlessMatchConfig {
            team1: team1.iter().map(|s| s.to_string()).collect(),
            team2: team2.iter().map(|s| s.to_string()).collect(),
            map: "PillaredArena".to_string(),
            team1_ai_profile: Some(profiles.0.to_string()),
            team2_ai_profile: Some(profiles.1.to_string()),
            max_duration_secs: 300.0,
            random_seed: Some(seed),
            ..Default::default()
        },
        true,
        Some(TraceConfig {
            output_path: trace_path.clone(),
        }),
        |f| {
            frames.push((
                f.gates_open,
                f.combatants
                    .iter()
                    .map(|(e, c)| {
                        let hp = c.current_health / c.max_health.max(1.0);
                        (e.index(), (c.team, c.is_pet, c.alive, c.position, hp))
                    })
                    .collect(),
            ))
        },
    )
    .expect("the match plays");

    // Posture transitions per healer entity, as (frame, posture).
    let mut postures: BTreeMap<u32, Vec<(usize, String)>> = BTreeMap::new();
    let raw = std::fs::read_to_string(&trace_path).expect("trace");
    let events: Vec<serde_json::Value> = raw
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    // The trace's frame counter and the observer's frame index are one clock:
    // the first decision after the gates sits on the first gates-open frame.
    let gates_frame = frames.iter().position(|(open, _)| *open);
    let gates_trace = events
        .iter()
        .find(|e| e["kind"] == "ability_decision" && e["situation"]["phase"] != "prematch")
        .and_then(|e| e["frame"].as_u64());
    if let (Some(a), Some(b)) = (gates_frame, gates_trace) {
        assert!(
            (a as i64 - b as i64).abs() <= 1,
            "observer frame {a} vs trace frame {b} at the gates"
        );
    }
    for e in &events {
        if e["kind"] != "movement_decision" {
            continue;
        }
        let class = e["actor"]["class"].as_str().unwrap_or_default();
        // Team 1's healer only: it runs the profile under measure, and the
        // opponents' healer the other.
        if !matches!(class, "Priest" | "Druid" | "Shaman") || e["actor"]["team"] != 1 {
            continue;
        }
        let id = e["actor"]["entity_id"].as_u64().unwrap() as u32;
        let frame = e["frame"].as_u64().unwrap() as usize;
        let posture = e["posture"].as_str().unwrap_or_default().to_string();
        postures.entry(id).or_default().push((frame, posture));
    }

    let mut out = Vec::new();
    for (&healer, timeline) in &postures {
        let mut spells = Spells::default();
        let mut open: Option<usize> = None;
        let mut cursor = 0;
        let mut posture = "free";
        for (i, (gates_open, frame)) in frames.iter().enumerate() {
            while cursor < timeline.len() && timeline[cursor].0 <= i {
                posture = timeline[cursor].1.as_str();
                cursor += 1;
            }
            let Some(&(team, _, alive, pos, _)) = frame.get(&healer) else {
                continue;
            };
            let mates: Vec<(Vec3, f32)> = frame
                .iter()
                .filter(|(&e, &(t, pet, a, _, _))| e != healer && t == team && !pet && a)
                .map(|(_, &(_, _, _, p, hp))| (p, hp))
                .collect();
            let in_situation = *gates_open
                && alive
                && posture == "free"
                && !mates.is_empty()
                && mates.iter().all(|&(_, hp)| hp >= URGENCY);
            let blind = in_situation
                && mates
                    .iter()
                    .any(|&(p, _)| !has_line_of_sight(obstacles, eye(pos), eye(p)));
            if in_situation {
                spells.free_frames += 1;
            }
            if blind {
                spells.blind_frames += 1;
                open.get_or_insert(i);
            } else if let Some(start) = open.take() {
                spells
                    .starts
                    .push((start - gates_frame.unwrap_or(0)) as f32 * FRAME);
                // Sight came back inside the situation, or the situation ended.
                spells
                    .closed
                    .push(((i - start) as f32 * FRAME, in_situation));
            }
        }
        if let Some(start) = open {
            spells
                .starts
                .push((start - gates_frame.unwrap_or(0)) as f32 * FRAME);
            spells
                .closed
                .push(((frames.len() - start) as f32 * FRAME, false));
        }
        spells.label = format!("{team1:?} v {team2:?} seed {seed} healer #{healer}");
        out.push(spells);
    }
    out
}

/// The bank: every formation healer with ranged and melee partners, against
/// four opponent pairs, on Nagrand. The healer is on team 1, the side measured.
fn bank() -> Vec<(Vec<&'static str>, Vec<&'static str>)> {
    let mut comps = Vec::new();
    for healer in ["Priest", "Druid", "Shaman"] {
        for partner in ["Mage", "Warrior", "Hunter"] {
            for enemy in [
                ["Warlock", "Paladin"],
                ["Rogue", "Priest"],
                ["Hunter", "Paladin"],
                ["Warrior", "Paladin"],
            ] {
                comps.push((vec![partner, healer], enemy.to_vec()));
            }
        }
    }
    comps
}

#[derive(Default, Debug)]
struct Tally {
    spells: usize,
    regained: usize,
    over: usize,
    longest: f32,
    free_frames: usize,
    blind_frames: usize,
}

fn measure(profile: &str, seeds: std::ops::Range<u64>, limit: f32) -> Tally {
    let obstacles = nagrand();
    let jobs: Vec<_> = bank()
        .into_iter()
        .flat_map(|c| seeds.clone().map(move |s| (c.clone(), s)))
        .collect();
    let workers = std::env::var("PROBE_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4usize);
    let chunks: Vec<Vec<_>> = (0..workers)
        .map(|w| jobs.iter().skip(w).step_by(workers).cloned().collect())
        .collect();
    let results: Vec<Spells> = std::thread::scope(|s| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                let obstacles = &obstacles;
                s.spawn(move || {
                    chunk
                        .iter()
                        .flat_map(|((t1, t2), seed)| {
                            // The healer's team runs `profile`; the opponents
                            // `Legacy`, whose healer is not measured.
                            blind_spells(t1, t2, (profile, "Legacy"), *seed, obstacles)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    let mut t = Tally::default();
    for s in results {
        t.free_frames += s.free_frames;
        t.blind_frames += s.blind_frames;
        for (k, (len, regained)) in s.closed.into_iter().enumerate() {
            t.spells += 1;
            t.regained += regained as usize;
            t.longest = t.longest.max(len);
            if len > limit {
                t.over += 1;
                println!("  {:.2}s blind from {:.2}s: {}", len, s.starts[k], s.label);
            }
        }
    }
    t
}

// ---------------------------------------------------------------------------
// The staged pillar edge (tier 3)
// ---------------------------------------------------------------------------

/// The pillar edge the defect was found on (AS-205's stage, with the Mage
/// healthy): Warrior + Paladin vs Mage + Druid on Nagrand, round the pillar at
/// (-40, -20). The Mage is held against the pillar's north face by the
/// Paladin's Hammer of Justice, at 90% — above `urgency_hp_threshold`, so no
/// medic chase. The Paladin is the enemy nearest the Mage, so the Druid's
/// formation point (behind the Mage, away from the Paladin) falls inside the
/// pillar and is projected onto its east face: out of the Mage's sight. The
/// Druid has no mana, so nothing it casts holds it still — the scene is its
/// walk alone.
mod staged {
    use super::*;
    use arenasim::headless::{Stage, Unit};
    use arenasim::states::play_match::AbilityType;

    pub const WARRIOR: Unit = Unit::new(1, 0);
    pub const PALADIN: Unit = Unit::new(1, 1);
    pub const MAGE: Unit = Unit::new(2, 0);
    pub const DRUID: Unit = Unit::new(2, 1);

    pub fn config(seed: u64, profile: &str) -> HeadlessMatchConfig {
        HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Paladin".into()],
            team2: vec!["Mage".into(), "Druid".into()],
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
            .place(DRUID, -35.8, -15.5)
            .mana(DRUID, 0.0)
    }
}

mod common;
use common::staged::{play, Played};

/// The window the scene is read over: inside the Mage's stun, and before the
/// pair on it can take it below the urgency line on any seed.
const STUNNED: f32 = 3.5;
/// The staged probe's N: a FREE Druid regains sight of its Mage within this.
const STAGED_REGAIN: f32 = 1.0;

/// Per staged frame up to `STUNNED`: (seconds since the stage, the Druid sees
/// the Mage, the Druid's point goal sees the Mage, the Mage's health).
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
            let (mage, druid) = (get(2, 0), get(2, 1));
            let sees = |p: Vec3| has_line_of_sight(obstacles, eye(p), eye(mage.position));
            (
                k as f32 * FRAME,
                sees(druid.position),
                druid.point_goal.map(sees),
                mage.current_health / mage.max_health,
            )
        })
        .take_while(|&(t, ..)| t < STUNNED)
        .collect()
}

/// The probe. Under `TeamPlan` the Druid walks out of the Mage's shadow within
/// `STAGED_REGAIN` and keeps sight for the rest of the stun, and every point
/// it is sent to sees the Mage. Its pair is `Legacy`, the one condition
/// removed (the unconstrained point): the same stage keeps the Druid blind
/// for most of the window — so the stage still contains the defect.
#[test]
fn a_free_druid_regains_sight_of_its_teammate() {
    let obstacles = nagrand();
    for seed in [1, 2, 3] {
        let track = sight_track(
            &play(staged::config(seed, "TeamPlan"), staged::stage()),
            &obstacles,
        );
        assert!(
            track.iter().all(|&(.., hp)| hp >= URGENCY),
            "seed {seed}: the Mage fell below the urgency line — the medic chase, not the \
             formation point, owns the walk"
        );
        assert!(
            !track[0].1,
            "seed {seed}: the stage no longer starts the Druid blind"
        );
        let regained = track
            .iter()
            .position(|&(_, sees, ..)| sees)
            .map(|k| track[k].0);
        assert!(
            regained.is_some_and(|t| t <= STAGED_REGAIN),
            "seed {seed}: the Druid regained sight of the Mage at {regained:?}, \
             not within {STAGED_REGAIN}s"
        );
        let blind_after = track
            .iter()
            .filter(|&&(t, sees, ..)| t > STAGED_REGAIN && !sees)
            .count();
        assert_eq!(blind_after, 0, "seed {seed}: the Druid lost sight again");
        let blind_goals = track
            .iter()
            .filter(|&&(_, _, goal, _)| goal == Some(false))
            .count();
        assert_eq!(
            blind_goals, 0,
            "seed {seed}: the Druid was sent to a point out of sight"
        );

        // The pair: the unconstrained point keeps the Druid blind.
        let control = sight_track(
            &play(staged::config(seed, "Legacy"), staged::stage()),
            &obstacles,
        );
        let blind = control.iter().filter(|&&(_, sees, ..)| !sees).count() as f32 * FRAME;
        assert!(
            blind >= 3.0,
            "seed {seed}: under Legacy the Druid was blind only {blind:.2}s — the stage no \
             longer puts its formation point in the Mage's shadow"
        );
    }
}

/// Blind spells longer than this many seconds are what the card is about: a
/// healer standing still behind a pillar. A teammate stepping behind cover
/// blinds the healer until the point moves and the healer walks clear, which
/// takes a formation re-commit and a few yards.
const REGAIN_WITHIN: f32 = 3.0;

/// The probe. Under `TeamPlan` a FREE healer regains sight of its teammates
/// within `REGAIN_WITHIN` seconds: no blind spell runs longer, over the whole
/// bank. Also prints the `Legacy` reading (unconstrained) for comparison.
#[test]
#[ignore = "seed-bank probe: ~580 matches; run with --release -- --ignored"]
fn a_free_healer_regains_sight_of_its_teammates() {
    let seeds = 1..9;
    let team_plan = measure("TeamPlan", seeds.clone(), REGAIN_WITHIN);
    let legacy = measure("Legacy", seeds, REGAIN_WITHIN);
    println!("TeamPlan: {team_plan:?}");
    println!("Legacy:   {legacy:?}");
    assert!(
        team_plan.spells >= 10,
        "the bank no longer finds FREE healers blinded by cover: {team_plan:?}"
    );
    assert_eq!(
        team_plan.over, 0,
        "a FREE TeamPlan healer stayed blind to a healthy teammate longer than \
         {REGAIN_WITHIN}s: {team_plan:?}"
    );
}
