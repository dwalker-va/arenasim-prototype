//! AS-373 — the healer pillar modes over time (tier 3, staged matches).
//!
//! - `a_hugging_healer_follows_its_teammate_round_the_pillar`: a Warrior walks
//!   round a Nagrand pillar to its stunned kill target while a Warlock stands
//!   off with Fear coming ready, and the healer is PRESSURED. Hugging, the
//!   healer goes round the pillar with its Warrior and never loses sight of
//!   it for more than a global cooldown. The pair, with the modes off, hides
//!   behind the pillar from the Warlock and loses the Warrior for seconds —
//!   today's hide.
//! - `the_open_heading_goes_round_the_sighted_side`: no crowd control a pillar
//!   defends against (a Rogue), so the healer plays the open; its teammate
//!   walks past a pillar so the formation heading's target crosses the
//!   teammate's shadow. The goal goes round the sighted side and never
//!   stands in the shadow; the pair, with the modes off (AS-206's exits),
//!   jumps across it.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::components::{HealerPosture, Posture};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::states::play_match::AbilityType;
use arenasim::ArenaMap;
use bevy::prelude::*;
use common::staged::{play, Played};

const FRAME: f32 = 1.0 / 60.0;
/// The global cooldown: a blind spell no longer than this costs at most one
/// heal.
const GCD: f32 = 1.5;

fn nagrand() -> Vec<ObstacleVolume> {
    load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(ArenaMap::PillaredArena)
        .volumes
}

fn eye(p: Vec3) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.z)
}

fn unit_in(frame: &FrameObservation, who: Unit) -> &arenasim::headless::ObservedCombatant {
    frame
        .combatants
        .values()
        .find(|c| c.team == who.team && c.slot == who.slot && !c.is_pet)
        .expect("the unit")
}

/// The longest unbroken spell, in seconds, over the first `window` seconds of
/// the stage, in which `a` could not see `b`, and the total.
fn blind_spells(played: &Played, a: Unit, b: Unit, window: f32) -> (f32, f32) {
    let obstacles = nagrand();
    let (mut run, mut longest, mut total) = (0.0f32, 0.0f32, 0.0f32);
    for f in played.staged_frames().take((window / FRAME) as usize) {
        let (pa, pb) = (unit_in(f, a), unit_in(f, b));
        if !pa.alive || !pb.alive {
            break;
        }
        if has_line_of_sight(&obstacles, eye(pa.position), eye(pb.position)) {
            run = 0.0;
        } else {
            run += FRAME;
            total += FRAME;
            longest = longest.max(run);
        }
    }
    (longest, total)
}

fn modes(played: &Played, team: u8, slot: u8) -> Vec<String> {
    let id = common::staged::entity_id(played, team, slot);
    played
        .staged_trace()
        .filter(|e| e["actor"]["entity_id"].as_u64() == Some(id))
        .filter_map(|e| e["trigger"].as_str())
        .filter(|t| t.starts_with("PillarMode"))
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// The follow
// ---------------------------------------------------------------------------

mod follow {
    use super::*;

    pub const WARRIOR: Unit = Unit::new(1, 0);
    pub const HEALER: Unit = Unit::new(1, 1);
    pub const TARGET: Unit = Unit::new(2, 0);
    pub const WARLOCK: Unit = Unit::new(2, 1);

    pub fn config(seed: u64, modes: bool) -> HeadlessMatchConfig {
        HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Priest".into()],
            team2: vec!["Paladin".into(), "Warlock".into()],
            map: "PillaredArena".to_string(),
            ai_profile: Some("TeamPlan".into()),
            team1_kill_target: Some(0),
            team2_kill_target: Some(0),
            team1_pillar_modes: Some(modes),
            random_seed: Some(seed),
            max_duration_secs: 8.0,
            ..Default::default()
        }
    }

    /// The pillar at (-40, -20). The enemy Paladin, the Warrior's target,
    /// stunned on its west side (a Paladin, so both teams would camp and
    /// neither does); our Warrior north-east of it with its Charge held, so it
    /// walks to the Paladin round the pillar's north side; the Warlock planted
    /// far to the south-west, out of Fear range of the ring,
    /// its Fear coming off cooldown inside the mode's horizon; our Priest
    /// east, held PRESSURED, with no mana (nothing it casts holds it still).
    pub fn stage() -> Stage {
        Stage::new()
            .place(TARGET, -53.0, -23.0)
            .land(TARGET, AbilityType::HammerOfJustice, WARLOCK)
            .place(WARRIOR, -27.0, -15.0)
            .target(WARRIOR, TARGET)
            .cooldown(WARRIOR, AbilityType::Charge, 60.0)
            .place(WARLOCK, -70.0, -42.0)
            .target(WARLOCK, WARRIOR)
            .cooldown(WARLOCK, AbilityType::Fear, 4.0)
            .cooldown(WARLOCK, AbilityType::DeathCoil, 60.0)
            .combatant(WARLOCK, |c| c.base_movement_speed = 0.0)
            .place(HEALER, -32.0, -21.0)
            .mana(HEALER, 0.0)
            .healer_posture(
                HEALER,
                HealerPosture {
                    posture: Posture::Pressured,
                    hold_until: 60.0,
                    ..HealerPosture::new(0.0)
                },
            )
    }
}

/// The window the follow is read over: the Warrior's walk round the pillar,
/// inside the enemy Paladin's stun.
const FOLLOW: f32 = 4.5;

/// One arm of the follow: the longest and total blind spell, and where the
/// Warrior ended the window.
fn follow_arm(seed: u64, on: bool) -> (Played, f32, f32, Vec3) {
    use follow::*;
    let played = play(config(seed, on), stage());
    let (longest, total) = blind_spells(&played, HEALER, WARRIOR, FOLLOW);
    let end = played
        .staged_frames()
        .nth((FOLLOW / FRAME) as usize)
        .map(|f| unit_in(f, WARRIOR).position)
        .expect("the window");
    (played, longest, total, end)
}

// ---------------------------------------------------------------------------
// The open traverse
// ---------------------------------------------------------------------------

mod traverse {
    use super::*;

    pub const WARRIOR: Unit = Unit::new(1, 0);
    pub const HEALER: Unit = Unit::new(1, 1);
    pub const ROGUE: Unit = Unit::new(2, 0);
    pub const FAR: Unit = Unit::new(2, 1);

    pub fn config(seed: u64, modes: bool) -> HeadlessMatchConfig {
        HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Druid".into()],
            team2: vec!["Rogue".into(), "Warrior".into()],
            map: "TwinPillars".to_string(),
            ai_profile: Some("TeamPlan".into()),
            team1_kill_target: Some(1),
            team2_kill_target: Some(0),
            team1_pillar_modes: Some(modes),
            random_seed: Some(seed),
            max_duration_secs: 8.0,
            ..Default::default()
        }
    }

    /// TwinPillars. Our Warrior walks west along z = -4, 1.5yd off the skin of
    /// the pillar at (9, 0), to the stunned enemy Warrior at (-14, -4), past a
    /// stunned Rogue at (9, -11). As it passes the Rogue the formation
    /// heading's "behind the line" target turns from east to west, and the
    /// shorter way round turns the point north — behind the pillar, in its
    /// shadow from the Warrior. No enemy has a crowd control a pillar
    /// defends against, so the Druid plays the open. It has no mana, so
    /// nothing it casts holds it still.
    pub fn stage() -> Stage {
        Stage::new()
            .place(WARRIOR, 22.0, -4.0)
            .target(WARRIOR, FAR)
            .cooldown(WARRIOR, AbilityType::Charge, 60.0)
            .place(ROGUE, 9.0, -11.0)
            .land(ROGUE, AbilityType::HammerOfJustice, FAR)
            .target(ROGUE, WARRIOR)
            .combatant(ROGUE, |c| c.stealthed = false)
            .place(FAR, -14.0, -4.0)
            .land(FAR, AbilityType::HammerOfJustice, ROGUE)
            .target(FAR, WARRIOR)
            .place(HEALER, 29.0, -2.0)
            .mana(HEALER, 0.0)
    }
}

/// The window the traverse is read over: the Warrior walks past the Rogue
/// and on, the enemies still stunned.
const TRAVERSE: f32 = 4.5;

/// The healer's formation goal over the traverse: the largest step between
/// consecutive goals, and how many frames the goal stood out of the
/// Warrior's sight.
fn traverse_arm(seed: u64, on: bool) -> (Played, f32, usize) {
    use traverse::*;
    let played = play(config(seed, on), stage());
    let obstacles = load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(ArenaMap::TwinPillars)
        .volumes;
    let (mut last, mut step, mut blind) = (None::<Vec3>, 0.0f32, 0usize);
    for f in played.staged_frames().take((TRAVERSE / FRAME) as usize) {
        let (w, h) = (unit_in(f, WARRIOR), unit_in(f, HEALER));
        let Some(goal) = h.point_goal else { continue };
        if !has_line_of_sight(&obstacles, eye(goal), eye(w.position)) {
            blind += 1;
        }
        if let Some(l) = last {
            step = step.max(Vec2::new(l.x - goal.x, l.z - goal.z).length());
        }
        last = Some(goal);
    }
    (played, step, blind)
}

#[test]
fn the_open_heading_goes_round_the_sighted_side() {
    for seed in [1, 2, 3] {
        let (on, step, blind) = traverse_arm(seed, true);
        let (_, off_step, off_blind) = traverse_arm(seed, false);
        println!(
            "seed {seed}: open step {step:.2}yd blind goal frames {blind}; \
             off step {off_step:.2}yd blind {off_blind}"
        );
        assert!(
            modes(&on, 1, 1).is_empty(),
            "seed {seed}: the healer hugged — the stage should keep it open"
        );
        assert_eq!(
            blind, 0,
            "seed {seed}: a goal stood in the Warrior's shadow"
        );
        assert!(step <= 4.0, "seed {seed}: the goal stepped {step:.2}yd");
        assert!(
            off_step > 5.0 || off_blind > 0,
            "seed {seed}: with the modes off the goal neither stepped nor went blind \
             (step {off_step:.2}yd) — the stage no longer crosses the shadow"
        );
    }
}

#[test]
fn a_hugging_healer_follows_its_teammate_round_the_pillar() {
    for seed in [1, 2, 3] {
        let (on, longest, total, warrior) = follow_arm(seed, true);
        let (_, off_longest, off_total, _) = follow_arm(seed, false);
        println!(
            "seed {seed}: hug longest {longest:.2}s total {total:.2}s; \
             off longest {off_longest:.2}s total {off_total:.2}s"
        );
        assert_eq!(
            modes(&on, 1, 1).first().map(String::as_str),
            Some("PillarModeEnter"),
            "seed {seed}: the healer never hugged"
        );
        assert!(
            warrior.x < -45.0,
            "seed {seed}: the Warrior did not round the pillar ({warrior})"
        );
        assert!(
            longest <= GCD,
            "seed {seed}: hugging, the healer lost its Warrior for {longest:.2}s"
        );
        assert!(
            off_longest > GCD + 0.5,
            "seed {seed}: with the modes off the healer was blind only {off_longest:.2}s — \
             the stage no longer reproduces the hide"
        );
    }
}
