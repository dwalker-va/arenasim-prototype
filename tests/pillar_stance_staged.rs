//! AS-387 (AS-385 card B) — the team pillar stance over time (tier 3, staged
//! matches). Each is a pair: the Pillar play run, and withheld from the team
//! (`team1_withheld_plays: ["Pillar"]`), on the same stage and seeds.
//!
//! - `the_healer_hugs_only_when_the_team_takes_the_pillar`: AS-373's follow —
//!   a Warrior walks round a Nagrand pillar to its stunned target while a
//!   Warlock stands off with Fear coming ready. The team takes the pillar,
//!   and its healer hugs and goes round with the Warrior, never losing it for
//!   more than a global cooldown. Withheld, the team takes no stance and the
//!   healer never hugs at all, over the whole match (design D3: no healer
//!   pillars alone).
//!
//! The ranged DPS's half (the kite's pull toward the stance's side) is a
//! one-tick bend, pinned in `tests/pillar_stance_scenes.rs`; its effect over
//! a match is the design's section 7 measurement (`tests/team_sight_cohesion.rs`).

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

fn sees(obstacles: &[ObstacleVolume], a: Vec3, b: Vec3) -> bool {
    has_line_of_sight(obstacles, eye(a), eye(b))
}

/// The pillar-mode triggers `team`/`slot` emitted over the whole match.
fn modes(played: &Played, team: u8, slot: u8) -> Vec<String> {
    let id = common::staged::entity_id(played, team, slot);
    played
        .trace
        .iter()
        .filter(|e| e["actor"]["entity_id"].as_u64() == Some(id))
        .filter_map(|e| e["trigger"].as_str())
        .filter(|t| t.starts_with("PillarMode"))
        .map(str::to_string)
        .collect()
}

/// `team`'s `pillar_stance` changes over the whole match.
fn stances(played: &Played, team: u8) -> Vec<String> {
    played
        .trace
        .iter()
        .filter(|e| e["name"] == "pillar_stance" && e["team"].as_u64() == Some(team as u64))
        .filter_map(|e| e["change"].as_str().map(str::to_string))
        .collect()
}

fn withheld(on: bool) -> Vec<String> {
    if on {
        Vec::new()
    } else {
        vec!["Pillar".to_string()]
    }
}

// ---------------------------------------------------------------------------
// The healer's half
// ---------------------------------------------------------------------------

mod follow {
    use super::*;

    pub const WARRIOR: Unit = Unit::new(1, 0);
    pub const HEALER: Unit = Unit::new(1, 1);
    pub const TARGET: Unit = Unit::new(2, 0);
    pub const WARLOCK: Unit = Unit::new(2, 1);

    pub fn config(seed: u64, on: bool) -> HeadlessMatchConfig {
        HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Priest".into()],
            team2: vec!["Paladin".into(), "Warlock".into()],
            map: "PillaredArena".to_string(),
            ai_profile: Some("TeamPlan".into()),
            team1_kill_target: Some(0),
            team2_kill_target: Some(0),
            team1_withheld_plays: withheld(on),
            random_seed: Some(seed),
            max_duration_secs: 8.0,
            ..Default::default()
        }
    }

    /// AS-373's follow stage (`tests/pillar_mode_staged.rs`): the pillar at
    /// (-40, -20); the enemy Paladin stunned on its west side; our Warrior
    /// north-east of it with its Charge held, so it walks round the pillar's
    /// north side; the Warlock planted far to the south-west, out of Fear
    /// range of the ring, its Fear coming ready; our Priest east, held
    /// PRESSURED, with no mana.
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

/// The longest unbroken spell, in seconds, over the first `window` seconds of
/// the stage, in which `a` could not see `b`.
fn longest_blind(played: &Played, a: Unit, b: Unit, window: f32) -> f32 {
    let obstacles = nagrand();
    let (mut run, mut longest) = (0.0f32, 0.0f32);
    for f in played.staged_frames().take((window / FRAME) as usize) {
        let (pa, pb) = (unit_in(f, a), unit_in(f, b));
        if !pa.alive || !pb.alive {
            break;
        }
        if sees(&obstacles, pa.position, pb.position) {
            run = 0.0;
        } else {
            run += FRAME;
            longest = longest.max(run);
        }
    }
    longest
}

#[test]
fn the_healer_hugs_only_when_the_team_takes_the_pillar() {
    use follow::*;
    for seed in [1, 2, 3] {
        let on = play(config(seed, true), stage());
        let off = play(config(seed, false), stage());
        let longest = longest_blind(&on, HEALER, WARRIOR, 4.5);
        println!(
            "seed {seed}: stance {:?} modes {:?} longest blind {longest:.2}s; \
             withheld stance {:?} modes {:?}",
            stances(&on, 1),
            modes(&on, 1, 1),
            stances(&off, 1),
            modes(&off, 1, 1)
        );
        assert_eq!(
            stances(&on, 1).first().map(String::as_str),
            Some("onset"),
            "seed {seed}: the team never took the pillar"
        );
        assert_eq!(
            modes(&on, 1, 1).first().map(String::as_str),
            Some("PillarModeEnter"),
            "seed {seed}: the healer never hugged"
        );
        assert!(
            longest <= GCD,
            "seed {seed}: hugging, the healer lost its Warrior for {longest:.2}s"
        );
        assert!(
            stances(&off, 1).is_empty(),
            "seed {seed}: withheld, a stance"
        );
        assert!(
            modes(&off, 1, 1).is_empty(),
            "seed {seed}: withheld, the healer hugged alone"
        );
    }
}
