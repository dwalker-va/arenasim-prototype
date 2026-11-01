//! AS-245 — a Druid left in a duel it cannot pay for walks in and swings its
//! staff.
//!
//! Two healers dueling ran each other out of mana inside the first minute and
//! then stood at spell range until the 300s cap: 1v1 Druid v Druid, v Shaman
//! and v Paladin drew every time. The Druid's staff is the one damage it has
//! that costs no mana, so a Druid that is its team's last unit, against one
//! enemy, closes to melee while it cannot afford a Moonfire
//! (`class_ai::druid::duel_melee`).
//!
//! The latch is pinned on its own in `class_ai/druid.rs`; this is the played
//! guard for its wiring — the latch set each turn, the pursuit closing to melee
//! on it, and the posture machine standing aside — on a STAGED match, so the
//! duel is on the field by construction and no seed can stop reaching it.
//! Each case is paired with its controls: the same stage with mana in hand,
//! with a teammate alive, and against two enemies.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::constants::MELEE_RANGE;
use common::staged::{play, Played};

const DRUID: Unit = Unit::new(1, 0);
const SHAMAN: Unit = Unit::new(2, 0);
const PRIEST: Unit = Unit::new(2, 1);
const MAGE: Unit = Unit::new(1, 1);

/// Where the stage lands: well into the Attrition phase, both healers long
/// out of mana in a real duel.
const CLOCK: f32 = 150.0;
/// How long the staged duel plays.
const PLAY_SECS: f32 = 20.0;

fn config(team1: &[&str], seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|c| c.to_string()).collect(),
        team2: vec!["Shaman".into()],
        random_seed: Some(seed),
        // The stage moves the timeout's clock to `CLOCK`.
        max_duration_secs: CLOCK + PLAY_SECS,
        ..Default::default()
    }
}

/// The two healers 28yd apart, each at its own spell range, the Shaman out of
/// mana and the Druid with `druid_mana` of its pool.
fn stage(druid_mana: f32) -> Stage {
    Stage::new()
        .clock(CLOCK)
        .place(DRUID, -14.0, 0.0)
        .place(SHAMAN, 14.0, 0.0)
        .mana(SHAMAN, 0.0)
        .mana(DRUID, druid_mana)
}

/// The closest the Druid came to the Shaman while both lived, and whether its
/// staff landed a swing on the Shaman.
fn duel(played: &Played) -> (f32, bool) {
    let closest = played
        .staged_frames()
        .filter_map(|f| {
            let at = |team: u8, slot: u8| {
                f.combatants
                    .values()
                    .find(|c| c.team == team && c.slot == slot && !c.is_pet && c.alive)
                    .map(|c| c.position)
            };
            Some(at(1, 0)?.distance(at(2, 0)?))
        })
        .fold(f32::MAX, f32::min);
    let swung = played
        .log
        .lines()
        .any(|l| l.contains("Team 1 Druid #1's Auto Attack") && l.contains("Team 2 Shaman #1"));
    (closest, swung)
}

/// Out of mana in a duel, the Druid walks the 28yd in and its staff lands.
/// Over three seeds: the stage holds the duel, the seed moves the rolls.
#[test]
fn an_oom_druid_in_a_duel_closes_and_swings_its_staff() {
    for seed in [1, 2, 3] {
        let played = play(config(&["Druid"], seed), stage(0.0));
        let (closest, swung) = duel(&played);
        assert!(
            closest <= MELEE_RANGE,
            "seed {seed}: the Druid never closed to melee (closest {closest:.1}yd)"
        );
        assert!(swung, "seed {seed}: the Druid's staff never landed a swing");
    }
}

/// Control: the same duel with the Druid's pool full. It can pay for its
/// Moonfire, so it keeps to spell range and never swings.
#[test]
fn a_druid_with_mana_in_a_duel_keeps_to_spell_range() {
    for seed in [1, 2, 3] {
        let played = play(config(&["Druid"], seed), stage(1.0));
        let (closest, swung) = duel(&played);
        assert!(
            closest > 10.0,
            "seed {seed}: the Druid closed to {closest:.1}yd with mana to cast"
        );
        assert!(!swung, "seed {seed}: the Druid swung its staff");
    }
}

/// Control: the same out-of-mana Druid with a teammate alive. It is no duel,
/// so the Druid stays back with its Mage instead of walking into melee.
#[test]
fn an_oom_druid_with_a_teammate_does_not_walk_into_melee() {
    for seed in [1, 2, 3] {
        let played = play(
            config(&["Druid", "Mage"], seed),
            stage(0.0).place(MAGE, -30.0, 10.0),
        );
        let (closest, swung) = duel(&played);
        assert!(
            closest > 10.0,
            "seed {seed}: the Druid closed to {closest:.1}yd with a teammate alive"
        );
        assert!(!swung, "seed {seed}: the Druid swung its staff");
    }
}

/// Control: the same lone, out-of-mana Druid against TWO enemies. It is no
/// duel, so it does not walk into melee with the Shaman.
#[test]
fn an_oom_druid_alone_against_two_does_not_walk_into_melee() {
    for seed in [1, 2, 3] {
        let played = play(
            HeadlessMatchConfig {
                team2: vec!["Shaman".into(), "Priest".into()],
                ..config(&["Druid"], seed)
            },
            stage(0.0).place(PRIEST, 30.0, 20.0).mana(PRIEST, 0.0),
        );
        let (closest, swung) = duel(&played);
        assert!(
            closest > 10.0,
            "seed {seed}: the Druid closed to {closest:.1}yd against two"
        );
        assert!(!swung, "seed {seed}: the Druid swung its staff");
    }
}
