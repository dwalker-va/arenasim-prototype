//! AS-429 — Aegis of the Last Stand, the triggered absorb: when its wearer
//! drops below 35% health it gains an absorb shield of 20% of its maximum
//! health, once a match, scaled by arena dampening where it lands.
//!
//! Staged matches (tier 3, `headless::stage`): the wearer is put on the field
//! already hurt, with two enemies on it, so the crossing happens by
//! construction on every seed. Each claim has its control with the one
//! condition removed.

mod common;

use std::collections::HashMap;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::DAMPENING_RAMP_SECS;
use arenasim::states::play_match::DAMPENING_START_SECS;
use common::staged::{play, Played};

const WARRIOR: Unit = Unit::new(1, 0);
const AEGIS_LINE: &str = "gains Aegis of the Last Stand (absorbs ";
const THRESHOLD: f32 = 0.35;
const SIZE: f32 = 0.20;

fn config(wears_aegis: bool) -> HeadlessMatchConfig {
    let mut trinket = HashMap::new();
    if wears_aegis {
        trinket.insert("Trinket1".to_string(), "AegisOfTheLastStand".to_string());
    }
    HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Rogue".into(), "Warlock".into()],
        map: "BasicArena".into(),
        team1_equipment: vec![trinket],
        // Both enemies on the Warrior.
        team2_kill_target: Some(0),
        random_seed: Some(7),
        max_duration_secs: 40.0,
        ..Default::default()
    }
}

/// The Warrior hurt but above the threshold, its enemies in reach.
fn hurt(fraction: f32) -> Stage {
    Stage::new()
        .place(WARRIOR, 0.0, 0.0)
        .place(Unit::new(2, 0), 2.0, 0.0)
        .place(Unit::new(2, 1), 20.0, 0.0)
        .health(WARRIOR, fraction)
}

/// Every Aegis line in the log: (app-clock stamp, the absorb it states).
fn aegis_lines(played: &Played) -> Vec<(f32, f32)> {
    played
        .log
        .lines()
        .filter(|l| l.contains(AEGIS_LINE))
        .map(|l| {
            let stamp = l
                .trim_start_matches('[')
                .split('s')
                .next()
                .unwrap()
                .trim()
                .parse::<f32>()
                .expect("a stamp");
            let n = l
                .split(AEGIS_LINE)
                .nth(1)
                .unwrap()
                .trim_end_matches(')')
                .parse::<f32>()
                .expect("an amount");
            (stamp, n)
        })
        .collect()
}

/// The Warrior's (app time, health, max health) on each staged frame, alive.
fn warrior_track(played: &Played) -> Vec<(f32, f32, f32)> {
    played
        .staged_frames()
        .filter_map(|f| {
            f.combatants
                .values()
                .find(|c| c.team == 1 && c.slot == 0 && !c.is_pet && c.alive)
                .map(|c| (f.sim_time, c.current_health, c.max_health))
        })
        .collect()
}

/// The first frame the Warrior is alive below the threshold.
fn first_below(track: &[(f32, f32, f32)]) -> (f32, f32) {
    track
        .iter()
        .find(|(_, h, max)| *h < THRESHOLD * max)
        .map(|(t, _, max)| (*t, *max))
        .expect("the Warrior never dropped below 35% — the stage did not reach the situation")
}

/// The wearer's damage crosses 35%: the shield lands on that frame, at 20% of
/// its maximum health, and never again, although the wearer stays below the
/// line long after it.
#[test]
fn crossing_the_threshold_raises_the_shield_once() {
    let played = play(config(true), hurt(0.45));
    let track = warrior_track(&played);
    let (crossed, max) = first_below(&track);
    let lines = aegis_lines(&played);
    assert_eq!(
        lines.len(),
        1,
        "Aegis fired {} times: {lines:?}",
        lines.len()
    );
    let (at, absorbs) = lines[0];
    // The frame of the crossing (stamps are to the hundredth).
    assert!(
        (at - crossed).abs() < 0.02,
        "the shield landed at {at}s, the crossing was at {crossed}s"
    );
    assert!(
        (absorbs - (SIZE * max).round()).abs() <= 1.0,
        "the shield absorbs {absorbs}, 20% of {max} is {}",
        SIZE * max
    );
    // Non-vacuity for "once": the wearer spent many frames below the line
    // after the shield, any of which would have raised a second one.
    let below_after = track
        .iter()
        .filter(|(t, h, m)| *t > at + 0.5 && *h < THRESHOLD * m)
        .count();
    assert!(
        below_after > 30,
        "only {below_after} frames below 35% after the shield: the once-a-match \
         claim was not exercised"
    );
    // The shield is used: hits on the Warrior are absorbed after it lands.
    assert!(
        played
            .log
            .lines()
            .any(|l| l.contains("Team 1 Warrior") && l.contains("absorbed")),
        "nothing the Warrior took was absorbed"
    );
}

/// The control: the same crossing without the trinket raises nothing.
#[test]
fn without_the_trinket_the_crossing_raises_nothing() {
    let played = play(config(false), hurt(0.45));
    first_below(&warrior_track(&played));
    assert!(aegis_lines(&played).is_empty());
}

/// The control on the trigger: a wearer above the line all match long gains
/// nothing. Its enemies are across the arena and the match is short, so it
/// takes no damage that could carry it below.
#[test]
fn a_wearer_above_the_line_gains_nothing() {
    let config = HeadlessMatchConfig {
        max_duration_secs: 1.0,
        ..config(true)
    };
    let stage = Stage::new()
        .place(WARRIOR, -40.0, 0.0)
        .place(Unit::new(2, 0), 40.0, 0.0)
        .place(Unit::new(2, 1), 40.0, 5.0)
        .health(WARRIOR, 0.36);
    let played = play(config, stage);
    let track = warrior_track(&played);
    assert!(!track.is_empty());
    assert!(track.iter().all(|(_, h, m)| *h >= THRESHOLD * m));
    assert!(aegis_lines(&played).is_empty());
}

/// Arena dampening scales the shield where it lands: staged deep into the
/// ramp, already below the line, the shield is 20% of maximum health less the
/// reduction on that frame; its control, the same stage at the gates, is the
/// whole 20%.
#[test]
fn dampening_scales_the_shield_where_it_lands() {
    let at = DAMPENING_START_SECS + 0.5 * DAMPENING_RAMP_SECS;
    let damped = play(config(true), hurt(0.30).clock(at));
    let fresh = play(config(true), hurt(0.30));
    let max = warrior_track(&fresh)[0].2;

    let fresh_lines = aegis_lines(&fresh);
    assert_eq!(fresh_lines.len(), 1);
    assert!((fresh_lines[0].1 - (SIZE * max).round()).abs() <= 1.0);

    let damped_lines = aegis_lines(&damped);
    assert_eq!(damped_lines.len(), 1);
    // The reduction on the landing frame, one tick past the staged clock.
    let reduction = (at + 1.0 / 60.0 - DAMPENING_START_SECS) / DAMPENING_RAMP_SECS;
    let want = SIZE * max * (1.0 - reduction);
    assert!(
        (damped_lines[0].1 - want).abs() <= 1.0,
        "the dampened shield absorbs {}, want {want:.1} (reduction {reduction:.3})",
        damped_lines[0].1
    );
}
