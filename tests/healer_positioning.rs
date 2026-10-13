//! Healer positioning (AS-249), measured on the ground: where the healer
//! stands, never what its posture machine stored.
//!
//! - **No range-edge bobbing.** A caster targeting a healer pressures it only
//!   while "closing" — between its preferred range (28yd) and
//!   `threat_intent_radius` (30yd). PRESSURED used to enter and leave on that
//!   one test, so a healer whose formation point lay past the caster walked
//!   in, entered PRESSURED, backed off for `pressured_hold`, left it, and
//!   walked back in: an in-and-out bob across the caster's 30yd ring every
//!   ~3 seconds. Under `Legacy` PRESSURED now enters only when the closing
//!   branch holds `danger_edge_margin` inside both its edges, and holds on the
//!   plain test. Pinned as a RATE over a seed bank of `Legacy` matches: how
//!   often a healer turns straight back on its own path, per minute alive.
//! - **Heal range on a focused teammate.** A `Legacy` PRESSURED healer already
//!   out of heal range of a teammate the enemy is focusing closes on it
//!   instead of fleeing freely (the scorer ladder's closing rung). Pinned in a
//!   staged match: a melee on the Druid between it and its Warrior.
//! - **Losing alone means take cover.** Under `TeamPlan`, a healer left alone
//!   against two or more enemies breaks their casters' sight behind a pillar
//!   (the TakeCover play). Pinned against the paired control — the same seed
//!   with the play withheld — and shown inert where there is nothing to hide
//!   behind.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::{ArenaMap, CharacterClass};
use bevy::prelude::{Entity, Vec2, Vec3};
use common::staged::{play, play_seeded, Played};

fn volumes(map: ArenaMap) -> Vec<ObstacleVolume> {
    load_map_geometry_config()
        .expect("assets/config/maps.ron must load")
        .active_for(map)
        .volumes
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

fn sees(volumes: &[ObstacleVolume], a: Vec3, b: Vec3) -> bool {
    has_line_of_sight(
        volumes,
        Vec3::new(a.x, EYE_HEIGHT, a.z),
        Vec3::new(b.x, EYE_HEIGHT, b.z),
    )
}

/// One frame's step on the app clock (the observer is called every frame).
const FRAME: f32 = 1.0 / 60.0;

// ---------------------------------------------------------------------------
// 1. Range-edge bobbing
// ---------------------------------------------------------------------------

/// The healer's path is read every this many seconds — long enough that a
/// step is a heading, not a frame's jitter.
const BOB_SAMPLE: f32 = 0.5;

/// A sampled step shorter than this (yards) is standing, not a heading.
const BOB_MIN_STEP: f32 = 0.5;

/// Two consecutive headings this far apart (cosine) are a REVERSAL: the
/// healer turned back on itself (within ~45 degrees of straight back).
const BOB_REVERSAL_COS: f32 = -0.7;

/// What the healers of one side measured over a match.
#[derive(Default, Debug, Clone, Copy)]
struct Bob {
    /// Reversals of heading: the healer walked one way, then back.
    reversals: u32,
    /// Seconds the healers were alive with the gates open.
    alive_secs: f32,
}

impl std::ops::AddAssign for Bob {
    fn add_assign(&mut self, o: Self) {
        self.reversals += o.reversals;
        self.alive_secs += o.alive_secs;
    }
}

impl Bob {
    fn per_minute(&self) -> f32 {
        self.reversals as f32 * 60.0 / self.alive_secs.max(1e-6)
    }
}

/// The healers of `team`: how often each walked one way and then back.
/// An in-and-out bob is a reversal every half cycle; a healer that holds a
/// spot, follows its teammate or runs from a melee reverses rarely.
fn bob(played: &Played, team: u8) -> Bob {
    let mut out = Bob::default();
    let healers: Vec<Entity> = played.frames[0]
        .combatants
        .iter()
        .filter(|(_, c)| c.team == team && !c.is_pet && c.class.is_healer())
        .map(|(e, _)| *e)
        .collect();
    for h in healers {
        let path: Vec<(f32, Vec2)> = played
            .frames
            .iter()
            .filter(|f| f.gates_open)
            .filter_map(|f| {
                f.combatants
                    .get(&h)
                    .filter(|c| c.alive)
                    .map(|c| (f.sim_time, xz(c.position)))
            })
            .collect();
        out.alive_secs += path.len() as f32 * FRAME;
        let mut sampled: Vec<Vec2> = Vec::new();
        let mut last = f32::NEG_INFINITY;
        for (t, p) in path {
            if t - last >= BOB_SAMPLE - 1e-4 {
                sampled.push(p);
                last = t;
            }
        }
        let mut heading: Option<Vec2> = None;
        for pair in sampled.windows(2) {
            let step = pair[1] - pair[0];
            if step.length() < BOB_MIN_STEP {
                continue;
            }
            let dir = step.normalize();
            if heading.is_some_and(|h| h.dot(dir) < BOB_REVERSAL_COS) {
                out.reversals += 1;
            }
            heading = Some(dir);
        }
    }
    out
}

/// The bank: 2v2s with a ranged enemy that targets the healer, both
/// replays the card came from among them, on both maps, `Legacy`.
fn bob_bank() -> Vec<(HeadlessMatchConfig, u8)> {
    let lineups: [(&[&str], &[&str], u8); 4] = [
        // (team1, team2, the healer's team)
        (&["Warrior", "Druid"], &["Hunter", "Shaman"], 1),
        (&["Mage", "Shaman"], &["Warrior", "Druid"], 2),
        (&["Mage", "Shaman"], &["Warrior", "Druid"], 1),
        (&["Warrior", "Priest"], &["Warlock", "Shaman"], 1),
    ];
    let mut out = Vec::new();
    for map in ["BasicArena", "PillaredArena"] {
        for (t1, t2, team) in lineups {
            for seed in 1..=6u64 {
                out.push((
                    HeadlessMatchConfig {
                        team1: t1.iter().map(|s| s.to_string()).collect(),
                        team2: t2.iter().map(|s| s.to_string()).collect(),
                        map: map.into(),
                        random_seed: Some(seed),
                        max_duration_secs: 120.0,
                        ..Default::default()
                    },
                    team,
                ));
            }
        }
    }
    out
}

/// Pinned ceiling on the bank's reversal rate, per minute of healer life. A
/// healer reverses for real reasons too — a juke round a melee, a turn back
/// to its teammate — so the ceiling is a rate, not zero. Measured over this
/// bank: 14.3/min on `main` @ 51b5c046, 3.3/min with the onset gap; the
/// ceiling sits between them.
const BOB_CEILING_PER_MIN: f32 = 6.0;

/// Seconds of healer life the bank must observe, or the rate means nothing.
const BOB_MIN_ALIVE_SECS: f32 = 600.0;

#[test]
fn healers_do_not_bob_in_and_out() {
    let mut total = Bob::default();
    for (config, team) in bob_bank() {
        let label = format!(
            "{:?} v {:?} @{} #{:?}",
            config.team1, config.team2, config.map, config.random_seed
        );
        let b = bob(&play_seeded(config), team);
        eprintln!(
            "{label}: {} reversals over {:.1}s = {:.2}/min",
            b.reversals,
            b.alive_secs,
            b.per_minute()
        );
        total += b;
    }
    eprintln!(
        "bank: {} reversals over {:.1}s = {:.2}/min",
        total.reversals,
        total.alive_secs,
        total.per_minute()
    );
    assert!(
        total.alive_secs >= BOB_MIN_ALIVE_SECS,
        "only {:.1}s of healer life — the bank no longer measures the bob",
        total.alive_secs
    );
    assert!(
        total.per_minute() <= BOB_CEILING_PER_MIN,
        "healers turn straight back on themselves {:.2} times a minute ({} reversals \
         over {:.1}s) — over the {BOB_CEILING_PER_MIN}/min ceiling",
        total.per_minute(),
        total.reversals,
        total.alive_secs
    );
}

// ---------------------------------------------------------------------------
// 2. Heal range on a focused teammate (Legacy)
// ---------------------------------------------------------------------------

const WARRIOR: Unit = Unit::new(1, 0);
const DRUID: Unit = Unit::new(1, 1);

/// Heal range (`shared.heal_range`; every Druid heal reaches 40yd).
const HEAL_RANGE: f32 = 40.0;

/// The XZ distance between team 1's slot-0 and slot-1 units on each staged
/// frame where both live, with its offset from the stage.
fn separations(played: &Played) -> Vec<(f32, f32)> {
    let landed = played.landed().app;
    played
        .staged_frames()
        .filter_map(|f| {
            let unit = |slot| {
                f.combatants
                    .values()
                    .find(|c| c.team == 1 && c.slot == slot && !c.is_pet && c.alive)
            };
            Some((
                f.sim_time - landed,
                xz(unit(0)?.position).distance(xz(unit(1)?.position)),
            ))
        })
        .collect()
}

/// The Druid 46yd from its Warrior, an enemy Warrior on it standing between the
/// two (its Charge spent), and an enemy Paladin on its Warrior: the Warrior is focused, and the
/// Druid is pressured out of its reach.
fn boxed_out(seed: u64) -> (HeadlessMatchConfig, Stage) {
    const ENEMY_WARRIOR: Unit = Unit::new(2, 0);
    const ENEMY_PALADIN: Unit = Unit::new(2, 1);
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Druid".into()],
        team2: vec!["Warrior".into(), "Paladin".into()],
        map: "BasicArena".into(),
        random_seed: Some(seed),
        max_duration_secs: 30.0,
        ..Default::default()
    };
    let stage = Stage::new()
        .place(DRUID, -20.0, 0.0)
        .place(ENEMY_WARRIOR, -17.0, 0.0)
        .target(ENEMY_WARRIOR, DRUID)
        .cooldown(ENEMY_WARRIOR, AbilityType::Charge, 60.0)
        .place(WARRIOR, 26.0, 0.0)
        .place(ENEMY_PALADIN, 27.5, 0.0)
        .target(ENEMY_PALADIN, WARRIOR)
        .target(WARRIOR, ENEMY_PALADIN);
    (config, stage)
}

/// Seconds after the stage the closing is measured over.
const BOXED_WINDOW: f32 = 6.0;

#[test]
fn a_pressured_druid_closes_on_its_focused_warrior_instead_of_fleeing_out_of_reach() {
    for seed in 1..=4u64 {
        let (config, stage) = boxed_out(seed);
        let played = play(config, stage);
        let sep = separations(&played);
        let window: Vec<_> = sep.iter().filter(|(t, _)| *t <= BOXED_WINDOW).collect();
        let start = window.first().expect("a staged frame").1;
        let end = window.last().unwrap().1;
        eprintln!("seed {seed}: {start:.1}yd -> {end:.1}yd");
        assert!(
            start > HEAL_RANGE,
            "seed {seed}: the stage must start out of reach"
        );
        assert!(
            end < start - 5.0,
            "seed {seed}: the Druid did not close on its focused Warrior: {start:.1}yd -> {end:.1}yd"
        );
        assert!(
            end <= HEAL_RANGE,
            "seed {seed}: still out of heal range of its focused Warrior after {BOXED_WINDOW}s: \
             {end:.1}yd"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Losing alone means take cover (TeamPlan)
// ---------------------------------------------------------------------------

/// Warrior+Druid against Hunter+Shaman, the Warrior one hit from death under
/// the Hunter, the Druid in the open with the Shaman on it.
fn last_stand(map: &str, seed: u64, play_on: bool) -> (HeadlessMatchConfig, Stage) {
    const HUNTER: Unit = Unit::new(2, 0);
    const SHAMAN: Unit = Unit::new(2, 1);
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Druid".into()],
        team2: vec!["Hunter".into(), "Shaman".into()],
        map: map.into(),
        ai_profile: Some("TeamPlan".into()),
        team1_withheld_plays: if play_on {
            Vec::new()
        } else {
            vec!["TakeCover".into()]
        },
        random_seed: Some(seed),
        max_duration_secs: 60.0,
        ..Default::default()
    };
    let stage = Stage::new()
        .place(DRUID, -25.0, 0.0)
        .place(WARRIOR, -5.0, 0.0)
        .health(WARRIOR, 0.01)
        .place(HUNTER, 18.0, 0.0)
        .target(HUNTER, WARRIOR)
        .place(SHAMAN, 12.0, 10.0)
        .target(SHAMAN, DRUID);
    (config, stage)
}

/// After the Warrior dies: the share of the Druid's living frames on which
/// NEITHER living enemy can see it, over `secs`, and the seconds from the
/// death to the first such frame.
fn hidden_after_death(
    played: &Played,
    volumes: &[ObstacleVolume],
    secs: f32,
) -> (f32, Option<f32>) {
    let frames: Vec<_> = played.staged_frames().collect();
    let death = frames
        .iter()
        .find(|f| {
            f.combatants
                .values()
                .any(|c| c.team == 1 && c.slot == 0 && !c.is_pet && !c.alive)
        })
        .map(|f| f.sim_time)
        .expect("the staged Warrior dies");
    let mut hidden = 0u32;
    let mut total = 0u32;
    let mut first = None;
    for f in frames
        .iter()
        .filter(|f| f.sim_time >= death && f.sim_time < death + secs)
    {
        let Some(druid) = f
            .combatants
            .values()
            .find(|c| c.team == 1 && c.class == CharacterClass::Druid && c.alive)
        else {
            break;
        };
        total += 1;
        let seen = f
            .combatants
            .values()
            .filter(|c| c.team == 2 && !c.is_pet && c.alive)
            .any(|c| sees(volumes, c.position, druid.position));
        if !seen {
            hidden += 1;
            first.get_or_insert(f.sim_time - death);
        }
    }
    (hidden as f32 / total.max(1) as f32, first)
}

/// Seconds after the Warrior's death the cover is measured over.
const COVER_WINDOW: f32 = 15.0;

/// The first hidden frame comes within this many seconds of the death: the
/// walk starts on the death, and the nearest shadow is ~25yd off.
const COVER_ONSET_SECS: f32 = 6.0;

#[test]
fn a_druid_left_alone_against_two_takes_a_pillar() {
    let nagrand = volumes(ArenaMap::PillaredArena);
    for seed in 1..=4u64 {
        let (config, stage) = last_stand("PillaredArena", seed, true);
        let (with, first) = hidden_after_death(&play(config, stage), &nagrand, COVER_WINDOW);
        let (config, stage) = last_stand("PillaredArena", seed, false);
        let (without, _) = hidden_after_death(&play(config, stage), &nagrand, COVER_WINDOW);
        eprintln!(
            "seed {seed}: hidden {:.0}% (first at {first:?}) vs withheld {:.0}%",
            with * 100.0,
            without * 100.0
        );
        let first = first.unwrap_or_else(|| panic!("seed {seed}: the Druid never broke sight"));
        assert!(
            first <= COVER_ONSET_SECS,
            "seed {seed}: first out of sight {first:.2}s after its Warrior died"
        );
        assert!(
            with >= 0.5,
            "seed {seed}: out of both enemies' sight only {:.0}% of the {COVER_WINDOW}s after",
            with * 100.0
        );
        assert!(
            without <= 0.1,
            "seed {seed}: the control (play withheld) hid {:.0}%, so the stage no longer \
             separates the play from what the Druid does anyway",
            without * 100.0
        );
    }
}

/// Where there is nothing to hide behind the play does nothing: the same
/// stage on BasicArena plays frame for frame the same with it withheld.
#[test]
fn the_last_stand_is_inert_with_nothing_to_hide_behind() {
    for seed in 1..=2u64 {
        let (config, stage) = last_stand("BasicArena", seed, true);
        let with = play(config, stage);
        let (config, stage) = last_stand("BasicArena", seed, false);
        let without = play(config, stage);
        assert!(
            with.log.contains("Warrior #1 has been eliminated"),
            "seed {seed}: the staged Warrior must die, or the last stand never opens"
        );
        let combat = |p: &Played| {
            let at = p.log.find("COMBAT LOG").expect("a combat log");
            p.log[at..].to_string()
        };
        assert_eq!(combat(&with), combat(&without), "seed {seed}");
        let positions = |p: &Played| -> Vec<Vec3> {
            p.frames
                .iter()
                .flat_map(|f| f.combatants.values().map(|c| c.position))
                .collect()
        };
        assert_eq!(positions(&with), positions(&without), "seed {seed}");
    }
}
