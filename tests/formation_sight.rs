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
//! In the default run:
//! - `a_free_healer_regains_sight_of_its_teammate` stages the pillar edge the
//!   defect was found on (tier 3) for each formation machine — the Druid's (the
//!   caster-healer machine it shares with the Shaman) and the Priest's — and
//!   pins the walk out of the shadow to within a second, paired with `Legacy`
//!   keeping the healer blind;
//! - `the_repaired_point_never_flips_across_a_shadow` reads every formation
//!   healer over a seed bank — a 2v2 whose Priest's Warrior walks round the camp
//!   pillar, and a 3v3 whose teammates stand close behind distant pillars — and
//!   bounds its formation goal's frame-to-frame jumps and returns;
//! - `kept_exit_reset::leaving_free_forgets_the_kept_exit` holds the kept exit,
//!   and the formation heading (AS-370), to FREE.
//!
//! `measure_formation_goal_jumps` (`#[ignore]`d) counts formation-goal jumps,
//! attributes each to its cause, and counts frames out of sight, over five
//! comps, for comparing two binaries.

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, ObservedCombatant};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::ArenaMap;
use bevy::prelude::{Entity, Vec3};

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

// ---------------------------------------------------------------------------
// The repaired point stays near the formation and keeps its side (seed bank)
// ---------------------------------------------------------------------------

/// The movement triggers whose `Point` goal is the FREE formation point: a
/// formation re-commit, or the re-anchor on entering FREE. Every other FREE
/// point walk (the medic chase's `SeekLos`, the dispel walk's) aims elsewhere.
const FORMATION_TRIGGERS: [&str; 3] = ["FormationShift", "PressuredExit", "EscapeWindowClosed"];

const FORMATION_HEALERS: [&str; 3] = ["Priest", "Druid", "Shaman"];

/// What one healer's formation goal did over a match: every frame-to-frame jump
/// (combat seconds, yards) of its goal on frames the trace shows it FREE on a
/// formation decision, how many such frame pairs there were, and what changed
/// in the world as each jump over 5yd was made ([`jump_cause`]).
#[derive(Default)]
struct GoalTrack {
    jumps: Vec<(f32, f32)>,
    pairs: usize,
    causes: Vec<&'static str>,
    /// Of the `pairs` frames, those on which the goal, and those on which the
    /// healer itself, had no line of sight to the anchor (the most injured
    /// living teammate).
    blind_goal: usize,
    blind_healer: usize,
}

/// What changed in the frames leading up to frame `i` (the three before it,
/// the decision's snapshot lagging the observation by up to a frame or two)
/// that a formation goal over 5yd away from the last one answers to, checked
/// in this order:
/// - `engaged`: a teammate died, or which teammates have a target changed, so
///   the centroid's membership changed;
/// - `target`: the healer's own target changed (the wand-range clamp's anchor);
/// - `crossing`: the enemy nearest the centroid (as the formation machine takes
///   it: any visible enemy, pets included) turned more than 90 degrees about
///   it — the "behind the line" direction flipping (AS-370);
/// - `other`: none of those (a change of repair exit, a clamp, ...).
fn jump_cause(frames: &[FrameObservation], healer: Entity, i: usize) -> &'static str {
    let window = &frames[i.saturating_sub(3)..=i];
    let Some(me) = frames[i].combatants.get(&healer) else {
        return "other";
    };
    let team = me.team;
    let allies = |f: &FrameObservation| -> Vec<(Entity, bool, bool)> {
        f.combatants
            .iter()
            .filter(|(e, c)| **e != healer && c.team == team && !c.is_pet)
            .map(|(e, c)| (*e, c.alive, c.target.is_some()))
            .collect()
    };
    if window.windows(2).any(|w| allies(&w[0]) != allies(&w[1])) {
        return "engaged";
    }
    let target = |f: &FrameObservation| f.combatants.get(&healer).and_then(|c| c.target);
    if window.windows(2).any(|w| target(&w[0]) != target(&w[1])) {
        return "target";
    }
    // The bearing of the enemy nearest the centroid, from the centroid.
    let bearing = |f: &FrameObservation| -> Option<Vec3> {
        let members: Vec<&ObservedCombatant> = f
            .combatants
            .iter()
            .filter(|(e, c)| **e != healer && c.team == team && !c.is_pet && c.alive)
            .map(|(_, c)| c)
            .collect();
        let engaged: Vec<_> = members.iter().filter(|c| c.target.is_some()).collect();
        let group: Vec<Vec3> = if engaged.is_empty() {
            members.iter().map(|c| c.position).collect()
        } else {
            engaged.iter().map(|c| c.position).collect()
        };
        if group.is_empty() {
            return None;
        }
        let centroid = group.iter().copied().sum::<Vec3>() / group.len() as f32;
        f.combatants
            .values()
            .filter(|c| c.team != team && c.alive && !c.stealthed)
            .min_by(|a, b| flat(a.position, centroid).total_cmp(&flat(b.position, centroid)))
            .map(|e| Vec3::new(centroid.x - e.position.x, 0.0, centroid.z - e.position.z))
    };
    let (first, last) = (bearing(&window[0]), bearing(&frames[i]));
    if let (Some(a), Some(b)) = (first, last) {
        if a.length() > 0.0 && b.length() > 0.0 && a.normalize().dot(b.normalize()) < 0.0 {
            return "crossing";
        }
    }
    "other"
}

impl GoalTrack {
    fn over(&self, yards: f32) -> usize {
        self.jumps.iter().filter(|j| j.1 > yards).count()
    }

    /// Returns: a frame-to-frame jump over `yards` undone, to within 2yd,
    /// inside one second of consecutive formation frames — a goal that went
    /// somewhere and came straight back.
    fn returns(&self, yards: f32, goals: &[(f32, Vec3)]) -> usize {
        let consecutive = |a: f32, b: f32| (b - a - FRAME).abs() < FRAME / 2.0;
        let mut n = 0;
        for k in 1..goals.len() {
            let ((s, prev), (t, p)) = (goals[k - 1], goals[k]);
            if !consecutive(s, t) || flat(prev, p) <= yards {
                continue;
            }
            let mut last = t;
            for &(u, q) in &goals[k + 1..] {
                if !consecutive(last, u) || u - t > 1.0 {
                    break;
                }
                if flat(q, prev) <= 2.0 {
                    n += 1;
                    if std::env::var("FORMATION_JUMPS_LIST").is_ok() {
                        println!("    return: {prev} -> {p} at {t:.2}s, back at {u:.2}s");
                    }
                    break;
                }
                last = u;
            }
        }
        n
    }
}

/// How many times a goal moves more than `yards` within half a second of
/// consecutive formation frames; each counted span is skipped past.
fn half_second_spans_over(yards: f32, goals: &[(f32, Vec3)]) -> usize {
    let consecutive = |a: f32, b: f32| (b - a - FRAME).abs() < FRAME / 2.0;
    let (mut n, mut k) = (0, 0);
    while k < goals.len() {
        let (t, p) = goals[k];
        let mut last = t;
        let mut hit = None;
        for (j, &(u, q)) in goals.iter().enumerate().skip(k + 1) {
            if !consecutive(last, u) || u - t > 0.5 {
                break;
            }
            if flat(p, q) > yards {
                hit = Some(j);
                break;
            }
            last = u;
        }
        match hit {
            Some(j) => {
                n += 1;
                k = goals[j..]
                    .iter()
                    .position(|&(u, _)| u - t > 0.5)
                    .map_or(goals.len(), |i| j + i);
            }
            None => k += 1,
        }
    }
    n
}

fn flat(a: Vec3, b: Vec3) -> f32 {
    ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// One match, both teams on `profile`, on Nagrand for `secs`: per formation
/// healer (by entity index) its formation-goal track, and the consecutive
/// formation goals it read.
fn formation_tracks(
    team1: &[&str],
    team2: &[&str],
    seed: u64,
    secs: f32,
    profile: &str,
) -> Vec<(String, GoalTrack, Vec<(f32, Vec3)>)> {
    let played = common::staged::play_seeded(HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        map: "PillaredArena".to_string(),
        ai_profile: Some(profile.to_string()),
        random_seed: Some(seed),
        max_duration_secs: secs,
        ..Default::default()
    });
    let gates = played
        .frames
        .iter()
        .position(|f| f.gates_open)
        .expect("the gates open");
    let healers: Vec<_> = played.frames[0]
        .combatants
        .iter()
        .filter(|(_, c)| {
            !c.is_pet && FORMATION_HEALERS.contains(&format!("{:?}", c.class).as_str())
        })
        .map(|(e, c)| (*e, format!("{:?}", c.class)))
        .collect();
    healers
        .into_iter()
        .map(|(healer, class)| {
            // The healer's movement decisions as (trace frame, posture,
            // trigger). The trace's frame counter is the observer's index.
            let decisions: Vec<(usize, bool)> = played
                .trace
                .iter()
                .filter(|e| {
                    e["kind"] == "movement_decision"
                        && e["actor"]["entity_id"].as_u64() == Some(u64::from(healer.index()))
                })
                .map(|e| {
                    let formation = e["posture"] == "free"
                        && FORMATION_TRIGGERS.contains(&e["trigger"].as_str().unwrap_or_default());
                    (e["frame"].as_u64().unwrap() as usize, formation)
                })
                .collect();
            // Every decision in force from a frame before to a frame after is
            // a formation one (the two clocks may sit a frame apart); a
            // re-commit ON the frame counts, since a jump past the shift
            // threshold is exactly what emits one.
            let formation_at = |frame: usize| {
                let first = decisions.partition_point(|d| d.0 < frame);
                let last = decisions.partition_point(|d| d.0 <= frame + 1);
                first > 0 && decisions[first - 1..last].iter().all(|d| d.1)
            };
            let goal = |i: usize| {
                played.frames[i]
                    .combatants
                    .get(&healer)
                    .filter(|c| c.alive)
                    .and_then(|c| c.point_goal)
                    .filter(|_| formation_at(i))
            };
            let mut track = GoalTrack::default();
            let mut goals = Vec::new();
            let obstacles = nagrand();
            let team = played.frames[0].combatants[&healer].team;
            // Whether `from` sees the most injured living teammate on frame `i`.
            let sees_anchor = |i: usize, from: Vec3| {
                played.frames[i]
                    .combatants
                    .iter()
                    .filter(|(e, c)| **e != healer && c.team == team && !c.is_pet && c.alive)
                    .map(|(_, c)| c)
                    .min_by(|a, b| {
                        (a.current_health / a.max_health)
                            .total_cmp(&(b.current_health / b.max_health))
                    })
                    .is_none_or(|a| has_line_of_sight(&obstacles, eye(from), eye(a.position)))
            };
            for i in gates + 1..played.frames.len() {
                let t = (i - gates) as f32 * FRAME;
                let (Some(a), Some(b)) = (goal(i - 1), goal(i)) else {
                    continue;
                };
                // Runs of consecutive formation frames; a gap (the goal was
                // something else meanwhile) starts a new run.
                if goals
                    .last()
                    .is_none_or(|&(u, _)| t - FRAME - u > FRAME / 2.0)
                {
                    goals.push((t - FRAME, a));
                }
                goals.push((t, b));
                track.pairs += 1;
                if !sees_anchor(i, b) {
                    track.blind_goal += 1;
                    if std::env::var("FORMATION_JUMPS_BLIND").is_ok() {
                        println!("    blind goal {class} at {t:.2}s: {b}");
                    }
                }
                if !sees_anchor(i, played.frames[i].combatants[&healer].position) {
                    track.blind_healer += 1;
                }
                let d = flat(a, b);
                if d > 0.0 {
                    track.jumps.push((t, d));
                    if d > 5.0 {
                        track.causes.push(jump_cause(&played.frames, healer, i));
                    }
                }
            }
            (class, track, goals)
        })
        .collect()
}

/// The comps whose healers repair their formation point most on Nagrand: a
/// Priest's Warrior walking round the camp pillar (2v2); a 3v3 whose Priest's
/// teammates stand close behind pillars far from it, the case that once sent
/// the point ~70yd; and a 2v2 whose Priest stands beside a pillar with its raw
/// point inside the footprint, where a shadow's near end and changes of exit
/// once stepped the goal 9-12yd.
const FLIP_BANK: [(&[&str], &[&str]); 3] = [
    (&["Warrior", "Priest"], &["Mage", "Rogue"]),
    (
        &["Warrior", "Mage", "Priest"],
        &["Rogue", "Warlock", "Druid"],
    ),
    (&["Warlock", "Priest"], &["Rogue", "Druid"]),
];

/// The repaired formation point never flips across a shadow: over the bank's
/// comps, seeds 0-9, both team orders and the first 30s (the walk out of the
/// room, the camp release and first contact), no frame-to-frame jump of any
/// formation healer's goal over 25yd — the repair stays within `REPAIR_LIMIT`
/// (16yd) of the formation — and no jump over 10yd that is undone inside a
/// second.
#[test]
fn the_repaired_point_never_flips_across_a_shadow() {
    let mut big = Vec::new();
    let mut returns = Vec::new();
    let mut pairs = 0;
    for (a, b) in FLIP_BANK {
        for seed in 0..10 {
            for (t1, t2) in [(a, b), (b, a)] {
                for (class, track, goals) in formation_tracks(t1, t2, seed, 30.0, "TeamPlan") {
                    pairs += track.pairs;
                    let label = format!("{t1:?} v {t2:?} seed {seed} {class}");
                    for &(t, d) in track.jumps.iter().filter(|j| j.1 > 25.0) {
                        big.push(format!("{label}: {d:.1}yd at {t:.2}s"));
                    }
                    let r = track.returns(10.0, &goals);
                    if r > 0 {
                        returns.push(format!("{label}: {r} returns"));
                    }
                }
            }
        }
    }
    println!("{pairs} formation-goal frame pairs");
    assert!(
        pairs > 5_000,
        "the bank no longer walks its healers to their formation points ({pairs} frames)"
    );
    assert!(big.is_empty(), "formation goal jumps over 25yd: {big:#?}");
    assert!(returns.is_empty(), "formation goal returns: {returns:#?}");
}

/// Measurement, not a gate: per comp, the formation goals' frame-to-frame jumps
/// over 5yd, 10yd and 25yd, their returns, the half-second spans over which a
/// goal moves more than 10yd, the jumps over 5yd by cause ([`jump_cause`]), and
/// the frames on which the goal, and the healer, had no sight of the most
/// injured teammate. Run on two binaries to compare; set
/// `FORMATION_JUMPS_SEEDS` for the seed count (default 10),
/// `FORMATION_JUMPS_PROFILE` for the AI profile both teams run (default
/// `TeamPlan`), `FORMATION_JUMPS_LIST` to print every jump over 10yd and
/// `FORMATION_JUMPS_BLIND` to print every frame whose goal is out of sight.
#[test]
#[ignore = "measurement: run with --release -- --ignored --nocapture"]
fn measure_formation_goal_jumps() {
    let comps: [(&[&str], &[&str]); 5] = [
        (&["Warrior", "Priest"], &["Mage", "Rogue"]),
        (
            &["Warrior", "Mage", "Priest"],
            &["Rogue", "Warlock", "Druid"],
        ),
        (&["Mage", "Druid"], &["Hunter", "Paladin"]),
        (&["Warlock", "Priest"], &["Rogue", "Druid"]),
        (&["Hunter", "Shaman"], &["Warrior", "Paladin"]),
    ];
    let seeds: u64 = std::env::var("FORMATION_JUMPS_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let profile =
        std::env::var("FORMATION_JUMPS_PROFILE").unwrap_or_else(|_| "TeamPlan".to_string());
    for (a, b) in comps {
        let (mut over5, mut over10, mut over25, mut ret, mut pairs) = (0, 0, 0, 0, 0);
        let mut spans = 0;
        let mut causes: std::collections::BTreeMap<&str, usize> = Default::default();
        let (mut blind_goal, mut blind_healer) = (0, 0);
        for seed in 0..seeds {
            for (t1, t2) in [(a, b), (b, a)] {
                for (class, track, goals) in formation_tracks(t1, t2, seed, 120.0, &profile) {
                    if std::env::var("FORMATION_JUMPS_LIST").is_ok() {
                        for &(t, d) in track.jumps.iter().filter(|j| j.1 > 10.0) {
                            println!("  {t1:?} v {t2:?} s{seed} {class}: {d:.1}yd at {t:.2}s");
                        }
                    }
                    pairs += track.pairs;
                    over5 += track.over(5.0);
                    over10 += track.over(10.0);
                    over25 += track.over(25.0);
                    ret += track.returns(5.0, &goals);
                    spans += half_second_spans_over(10.0, &goals);
                    blind_goal += track.blind_goal;
                    blind_healer += track.blind_healer;
                    for c in &track.causes {
                        *causes.entry(c).or_default() += 1;
                    }
                }
            }
        }
        println!(
            "{a:?} v {b:?}: pairs {pairs}, over5 {over5}, over10 {over10}, over25 {over25}, \
             returns {ret}, 0.5s spans over 10yd {spans}; over5 by cause {causes:?}; \
             anchor out of sight: goal {blind_goal}, healer {blind_healer}"
        );
    }
}

// ---------------------------------------------------------------------------
// The kept exit does not outlive FREE
// ---------------------------------------------------------------------------

mod kept_exit_reset {
    use std::collections::BTreeMap;

    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::ai_profile::AiProfile;
    use arenasim::states::play_match::class_ai::caster_healer_posture::evaluate_caster_healer_posture;
    use arenasim::states::play_match::class_ai::priest::evaluate_priest_posture;
    use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
    use arenasim::states::play_match::decision_trace::DecisionTrace;
    use arenasim::states::play_match::situation::{MatchClock, SituationConfig};
    use arenasim::states::play_match::{
        AbilityDefinitions, AbilityType, Combatant, DRTracker, FormationHeading, FormationMemory,
        HealerPosture, MovementConfig, Posture,
    };
    use bevy::ecs::world::CommandQueue;
    use bevy::prelude::*;

    static NO_DR: BTreeMap<Entity, DRTracker> = BTreeMap::new();
    static NO_CD: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();

    fn info(entity: Entity, team: u8, class: CharacterClass, x: f32, z: f32) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position: Vec3::new(x, 1.0, z),
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
        }
    }

    /// One posture tick for a FREE `class` healer carrying a kept exit and a
    /// formation heading,
    /// with an enemy Warrior on it (`pressed`) or nowhere near. Returns the
    /// posture state after the tick.
    fn tick(class: CharacterClass, pressed: bool) -> HealerPosture {
        let mut world = World::new();
        let healer = world.spawn_empty().id();
        let mage = world.spawn_empty().id();
        let warrior = world.spawn_empty().id();
        let enemy_at = if pressed { (2.0, 0.0) } else { (0.0, 60.0) };
        let mut enemy = info(warrior, 2, CharacterClass::Warrior, enemy_at.0, enemy_at.1);
        enemy.target = Some(healer);
        let mut ally = info(mage, 1, CharacterClass::Mage, 10.0, 0.0);
        ally.target = Some(warrior);
        let roster: BTreeMap<Entity, CombatantInfo> =
            [info(healer, 1, class, 0.0, 0.0), ally, enemy]
                .into_iter()
                .map(|u| (u.entity, u))
                .collect();
        let auras = BTreeMap::new();
        let ctx = CombatContext::new(
            healer,
            1,
            &roster,
            &auras,
            &NO_DR,
            &NO_CD,
            &[],
            Default::default(),
            AiProfile::TeamPlan,
        )
        .classified(&SituationConfig::default(), MatchClock::after_gates(5.0));
        let mut state = HealerPosture::new(0.0);
        state.formation.exit = Some(Vec2::new(10.0, 5.0));
        state.formation.heading = Some(FormationHeading {
            dir: Vec2::Y,
            at: 4.9,
        });
        let combatant = Combatant::new(1, 0, class);
        let movement = MovementConfig::default();
        let abilities = AbilityDefinitions::default();
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut trace = DecisionTrace::default();
        let pos = Vec3::new(0.0, 1.0, 0.0);
        match class {
            CharacterClass::Priest => {
                evaluate_priest_posture(
                    &mut commands,
                    healer,
                    &combatant,
                    pos,
                    &ctx,
                    &abilities,
                    None,
                    Some(&mut state),
                    None,
                    &movement,
                    5.0,
                    &mut trace,
                );
            }
            _ => {
                evaluate_caster_healer_posture(
                    &mut commands,
                    healer,
                    &combatant,
                    pos,
                    &ctx,
                    Some(&mut state),
                    None,
                    &movement,
                    &movement.druid,
                    false,
                    5.0,
                    &mut trace,
                );
            }
        }
        state
    }

    /// A healer pressed out of FREE drops its kept exit and its heading, as it
    /// drops its last formation point; the control, left FREE, keeps the exit
    /// (nothing in an
    /// obstacle-free tick touches it).
    #[test]
    fn leaving_free_forgets_the_kept_exit() {
        for class in [CharacterClass::Priest, CharacterClass::Druid] {
            let pressed = tick(class, true);
            assert_ne!(
                pressed.posture,
                Posture::Free,
                "{class:?}: the scene presses it"
            );
            assert_eq!(
                pressed.formation,
                FormationMemory::default(),
                "{class:?}: side or heading carried out of FREE"
            );
            let free = tick(class, false);
            assert_eq!(
                free.posture,
                Posture::Free,
                "{class:?}: the control stays FREE"
            );
            assert!(
                free.formation.exit.is_some(),
                "{class:?}: the control keeps it"
            );
            assert!(
                free.formation.heading.is_some(),
                "{class:?}: the control turns its heading"
            );
        }
    }
}
