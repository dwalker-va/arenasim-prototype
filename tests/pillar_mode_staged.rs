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
    pub const MAGE: Unit = Unit::new(2, 0);
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

    /// The pillar at (-40, -20). The Mage stunned on its west side; our
    /// Warrior east of it, so its walk to the Mage rounds the pillar; the
    /// Warlock planted far to the north-west, out of Fear range of the ring,
    /// its Fear coming off cooldown inside the mode's horizon; our Priest
    /// east, held PRESSURED, with no mana (nothing it casts holds it still).
    pub fn stage() -> Stage {
        Stage::new()
            .place(MAGE, -53.0, -23.0)
            .land(MAGE, AbilityType::HammerOfJustice, WARLOCK)
            .place(WARRIOR, -27.0, -15.0)
            .target(WARRIOR, MAGE)
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
/// inside the Mage's stun.
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

#[test]
fn a_hugging_healer_follows_its_teammate_round_the_pillar() {
    for seed in [1, 2, 3] {
        let (on, longest, total, warrior) = follow_arm(seed, true);
        let (_, off_longest, off_total, _) = follow_arm(seed, false);
        if std::env::var("PMS_DEBUG").is_ok() {
            let c = Vec2::new(-40.0, -20.0);
            let ang = |p: Vec3| (Vec2::new(p.x, p.z) - c).to_angle().to_degrees();
            let obstacles = nagrand();
            for (k, f) in on.staged_frames().enumerate().step_by(10).take(30) {
                let (w, h) = (unit_in(f, follow::WARRIOR), unit_in(f, follow::HEALER));
                println!(
                    "t {:.2} w {:.0}deg r{:.1} h {:.0}deg r{:.1} goal {:?} sees {}",
                    k as f32 * FRAME,
                    ang(w.position),
                    Vec2::new(w.position.x, w.position.z).distance(c),
                    ang(h.position),
                    Vec2::new(h.position.x, h.position.z).distance(c),
                    h.point_goal.map(|g| (ang(g).round(), Vec2::new(g.x, g.z).distance(c))),
                    has_line_of_sight(&obstacles, eye(h.position), eye(w.position))
                );
            }
        }
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
