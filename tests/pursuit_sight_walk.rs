//! AS-360 — a pursuer in range of its target but out of its sight walks round
//! the pillar until it sees it.
//!
//! The defect was found in a played match on PillaredArena: after the Mage and
//! the Hunter died, a Warlock faced a lone enemy Shaman with no mana, out of
//! the Shaman's sight across a pillar and inside its own preferred range. The
//! pursuit stopped at that range, every Warlock cast was refused `LosBlocked`,
//! the Shaman had nothing to cast and no reason to move, and the match drew at
//! the 300s cap.
//!
//! The rule (`combat_core::movement::pursuit_closes`, pinned case by case in
//! its unit tests): a unit that is not a healer stops at its preferred range
//! only once it sees its target. This is the played guard: a STAGED match
//! (tier 3, `headless::stage`) puts the parked pair on the field by
//! construction, so no seed can stop reaching it.
//!
//! The pairs, each with one condition removed:
//! - Nagrand, out of sight: the Warlock walks round the pillar and casts.
//! - Nagrand, in sight: the Warlock stands where it is and casts.
//! - BasicArena, the out-of-sight placement's geometry with no pillar: the
//!   Warlock stands where it is and casts.

mod common;

use bevy::prelude::*;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::ArenaMap;
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume};
use common::staged::{play, Played};

const SHAMAN: Unit = Unit::new(1, 0);
const WARLOCK: Unit = Unit::new(2, 0);
const FELHUNTER: Unit = Unit::pet_of(2, 0);

/// The Shaman, 8yd east of the centre of the Nagrand pillar at (40, 20)
/// (apothem 5.5yd) — parked against its east face.
const SHAMAN_AT: Vec2 = Vec2::new(48.0, 20.0);
/// The Warlock 15yd west of the pillar's centre: 23yd from the Shaman, inside
/// its 28yd preferred range, with the pillar square between them.
const OCCLUDED_AT: Vec2 = Vec2::new(25.0, 20.0);
/// The Warlock north of the pillar: 21.5yd from the Shaman, the sightline
/// passing 7.4yd from the pillar's centre (clear of its 6yd circumradius).
const SIGHTED_AT: Vec2 = Vec2::new(40.0, 40.0);
/// BasicArena has no pillar, and its octagon ends at x = 36.5: the out-of-sight
/// placement, shifted onto it whole.
const BASIC_SHIFT: Vec2 = Vec2::new(-36.0, -20.0);

const SEEDS: [u64; 3] = [1, 2, 3];
/// Seconds the match plays from the stage.
const SECS: f32 = 12.0;
/// The Warlock has the Shaman in sight within this many seconds. Rounding
/// the pillar takes about three; the rest is the Shaman's own answer — once
/// the Warlock closes inside its danger radius it turns PRESSURED and steps
/// back into cover, and the Warlock follows it round.
const SIGHT_WITHIN: f32 = 8.0;

fn config(map: &str, profile: &str, seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Shaman".into()],
        team2: vec!["Warlock".into()],
        map: map.into(),
        ai_profile: Some(profile.into()),
        random_seed: Some(seed),
        max_duration_secs: SECS,
        ..Default::default()
    }
}

/// The lone Shaman, out of mana, parked at `shaman`; the Warlock at
/// `warlock`, each targeting the other. The Felhunter is dead (in the found
/// match it died with the Mage), so nothing else walks to the Shaman.
fn stage(shaman: Vec2, warlock: Vec2) -> Stage {
    Stage::new()
        .place(SHAMAN, shaman.x, shaman.y)
        .mana(SHAMAN, 0.0)
        .place(WARLOCK, warlock.x, warlock.y)
        .target(WARLOCK, SHAMAN)
        .target(SHAMAN, WARLOCK)
        .health(FELHUNTER, 0.0)
        .combatant(FELHUNTER, |c| c.is_dead = true)
}

fn nagrand() -> Vec<ObstacleVolume> {
    load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(ArenaMap::PillaredArena)
        .volumes
}

/// `(warlock, shaman)` positions on a frame.
fn pair(frame: &FrameObservation) -> (Vec3, Vec3) {
    let find = |team: u8| {
        frame
            .combatants
            .values()
            .find(|c| c.team == team && c.slot == 0 && !c.is_pet)
            .map(|c| c.position)
            .expect("both combatants are on the field")
    };
    (find(2), find(1))
}

/// How far the Warlock walked from the stage on.
fn warlock_path(played: &Played) -> f32 {
    let at: Vec<Vec3> = played.staged_frames().map(|f| pair(f).0).collect();
    at.windows(2)
        .map(|w| Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length())
        .sum()
}

/// The Warlock's damage to the Shaman over the match.
fn warlock_damage(played: &Played) -> f32 {
    played.result.team2_combatants[0].damage_dealt
}

/// The defect's scene. The Warlock starts out of sight and in range, walks
/// round the pillar, has the Shaman in sight within [`SIGHT_WITHIN`], and
/// lands damage on it. Under both AI profiles: the walk is the pursuit's,
/// which neither one owns.
#[test]
fn an_occluded_warlock_walks_round_the_pillar_to_its_target() {
    let volumes = nagrand();
    for profile in ["Legacy", "TeamPlan"] {
        for seed in SEEDS {
            let played = play(
                config("PillaredArena", profile, seed),
                stage(SHAMAN_AT, OCCLUDED_AT),
            );
            let frames: Vec<&FrameObservation> = played.staged_frames().collect();

            // The scene holds: in range and out of sight as the stage lands.
            let (w0, s0) = pair(frames[0]);
            assert!(
                w0.distance(s0) < 28.0 && !has_line_of_sight(&volumes, w0, s0),
                "{profile} seed {seed}: the stage did not put the Warlock in range and out of \
                 sight ({w0} -> {s0})"
            );

            let first_sight = frames
                .iter()
                .find(|f| {
                    let (w, s) = pair(f);
                    has_line_of_sight(&volumes, w, s)
                })
                .map(|f| f.sim_time - frames[0].sim_time);
            assert!(
                first_sight.is_some_and(|t| t <= SIGHT_WITHIN),
                "{profile} seed {seed}: the Warlock did not regain sight of the Shaman within \
                 {SIGHT_WITHIN}s (first sight: {first_sight:?}; walked {:.1}yd)",
                warlock_path(&played)
            );
            assert!(
                warlock_path(&played) >= 3.0,
                "{profile} seed {seed}: the Warlock regained sight without walking for it \
                 ({:.1}yd) — the Shaman came to it, and the scene tested nothing",
                warlock_path(&played)
            );
            assert!(
                warlock_damage(&played) > 0.0,
                "{profile} seed {seed}: the Warlock dealt no damage to the parked Shaman"
            );
        }
    }
}

/// The healer half of the rule: a healer does not walk for sight. In the
/// defect's scene the Shaman is in range of the Warlock and out of its sight
/// too, and it holds its ground — its posture machine, not the pursuit, owns
/// where it stands — until the Warlock comes round the pillar inside its
/// danger radius. The Warlock is still 12yd off at 2s.
#[test]
fn the_parked_healer_does_not_walk_for_sight() {
    const HOLDS_FOR: f32 = 2.0;
    for profile in ["Legacy", "TeamPlan"] {
        for seed in SEEDS {
            let played = play(
                config("PillaredArena", profile, seed),
                stage(SHAMAN_AT, OCCLUDED_AT),
            );
            let frames: Vec<&FrameObservation> = played.staged_frames().collect();
            let start = frames[0].sim_time;
            let early: Vec<Vec3> = frames
                .iter()
                .take_while(|f| f.sim_time - start <= HOLDS_FOR)
                .map(|f| pair(f).1)
                .collect();
            let walked: f32 = early
                .windows(2)
                .map(|w| Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length())
                .sum();
            assert!(
                walked < 0.01,
                "{profile} seed {seed}: the Shaman, a healer in range and out of the \
                 Warlock's sight, walked {walked:.2}yd in its first {HOLDS_FOR}s"
            );
        }
    }
}

/// The frames from the stage until the Warlock first loses sight of the
/// Shaman (all of them if it never does), how long they span, and how far the
/// Warlock walked in them.
fn while_sighted(played: &Played, volumes: &[ObstacleVolume]) -> (f32, f32) {
    let frames: Vec<&FrameObservation> = played
        .staged_frames()
        .take_while(|f| {
            let (w, s) = pair(f);
            has_line_of_sight(volumes, w, s)
        })
        .collect();
    let span = match (frames.first(), frames.last()) {
        (Some(a), Some(b)) => b.sim_time - a.sim_time,
        _ => 0.0,
    };
    let walked = frames
        .windows(2)
        .map(|w| {
            let (a, b) = (pair(w[0]).0, pair(w[1]).0);
            Vec2::new(b.x - a.x, b.z - a.z).length()
        })
        .sum();
    (span, walked)
}

/// The controls' claim: from the stage until it first loses sight of the
/// Shaman — at least `MIN_SIGHTED` seconds, so the claim is not vacuous — the
/// Warlock in range does not take a step, and it lands damage.
fn stands_while_sighted(played: &Played, volumes: &[ObstacleVolume], what: &str) {
    const MIN_SIGHTED: f32 = 3.0;
    let (span, walked) = while_sighted(played, volumes);
    assert!(
        span >= MIN_SIGHTED,
        "{what}: the Warlock had the Shaman in sight for only {span:.1}s — the control \
         tested nothing"
    );
    assert!(
        walked < 0.01,
        "{what}: a Warlock in range and in sight walked {walked:.2}yd in {span:.1}s"
    );
    assert!(
        warlock_damage(played) > 0.0,
        "{what}: the Warlock dealt no damage (the control tested nothing)"
    );
}

/// Control on the same map: the Warlock starts in range AND in sight, so it
/// has no walking to do. It stands where the stage put it and casts — until
/// the Shaman steps behind the pillar, when the walk is owed again.
#[test]
fn a_sighted_warlock_in_range_stands() {
    let volumes = nagrand();
    for seed in SEEDS {
        let played = play(
            config("PillaredArena", "Legacy", seed),
            stage(SHAMAN_AT, SIGHTED_AT),
        );
        stands_while_sighted(&played, &volumes, &format!("Nagrand seed {seed}"));
    }
}

/// Control without the pillar: the defect's placement on BasicArena. The
/// Warlock sees the Shaman from where it stands, stays there, and casts.
#[test]
fn on_basic_arena_the_same_placement_stands() {
    for seed in SEEDS {
        let played = play(
            config("BasicArena", "Legacy", seed),
            stage(SHAMAN_AT + BASIC_SHIFT, OCCLUDED_AT + BASIC_SHIFT),
        );
        stands_while_sighted(&played, &[], &format!("BasicArena seed {seed}"));
    }
}
