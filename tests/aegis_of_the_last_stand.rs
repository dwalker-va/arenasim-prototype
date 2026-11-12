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
use arenasim::states::play_match::components::{ActiveAuras, Aura, AuraType, DispelType};
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

/// Whether the Warrior's first absorb appears on the frame at app time `at`:
/// it wears one on that frame and wore none on the frame before.
fn shield_appears_at(played: &Played, at: f32) -> bool {
    let shielded: Vec<(f32, bool)> = played
        .staged_frames()
        .filter_map(|f| {
            f.combatants
                .values()
                .find(|c| c.team == 1 && c.slot == 0 && !c.is_pet)
                .map(|c| (f.sim_time, c.aura_types.contains(&AuraType::Absorb)))
        })
        .collect();
    shielded
        .windows(2)
        .any(|w| w[1].0 == at && w[1].1 && !w[0].1)
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
    // The frame of the crossing: the shield is on it (and its log line,
    // whose stamps are to the hundredth).
    assert!(
        shield_appears_at(&played, crossed),
        "the shield did not appear on the crossing frame"
    );
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

const PRIEST: Unit = Unit::new(1, 1);

/// The crossing comes by a TRANSFER: the Warrior stands out of the fight, no
/// hit of its own landing before the crossing, and half of what its Priest
/// takes is sent to it (a `DamageTransfer` whose caster it is, Blessing of
/// Sacrifice's shape). A transfer lands in `process_damage_follow_ups`, after
/// the frame's hits, so this pins that the trigger runs after it: the shield
/// is up on the frame the transfer crossed the line, not the frame after. And
/// a Blessing of Sacrifice share is damage its receiver takes (AS-459), so the
/// shield it raised soaks the shares that follow it.
#[test]
fn a_transfer_that_crosses_the_line_raises_the_shield_that_frame() {
    let mut trinket = HashMap::new();
    trinket.insert("Trinket1".to_string(), "AegisOfTheLastStand".to_string());
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Priest".into()],
        team2: vec!["Rogue".into(), "Warlock".into()],
        map: "BasicArena".into(),
        team1_equipment: vec![trinket, HashMap::new()],
        // Both enemies on the Priest.
        team2_kill_target: Some(1),
        random_seed: Some(7),
        max_duration_secs: 30.0,
        ..Default::default()
    };
    let stage = Stage::new()
        .place(WARRIOR, -30.0, 0.0)
        .place(PRIEST, 10.0, 0.0)
        .place(Unit::new(2, 0), 12.0, 0.0)
        .place(Unit::new(2, 1), 25.0, 0.0)
        .health(WARRIOR, 0.36)
        // Nothing the countdown put on it: its Priest's Power Word: Shield
        // would be an absorb before the crossing.
        .clear_auras(WARRIOR)
        // No mana: the Priest cannot heal the Warrior back over the line.
        .mana(PRIEST, 0.0)
        .world(|world| {
            let warrior = WARRIOR.entity(world);
            let priest = PRIEST.entity(world);
            let transfer = Aura {
                effect_type: AuraType::DamageTransfer,
                duration: 100.0,
                magnitude: 0.5,
                break_on_damage_threshold: -1.0,
                ability_name: "Staged sacrifice".into(),
                caster: Some(warrior),
                dispel_type: DispelType::Untyped,
                ..Default::default()
            };
            match world.get_mut::<ActiveAuras>(priest) {
                Some(mut auras) => auras.auras.push(transfer),
                None => {
                    world.entity_mut(priest).insert(ActiveAuras {
                        auras: vec![transfer],
                    });
                }
            }
        });
    let played = play(config, stage);
    let (crossed, _) = first_below(&warrior_track(&played));
    assert!(
        played
            .log
            .lines()
            .any(|l| l.contains("[TRANSFER] Team 1 Warrior #1 takes")),
        "no transfer reached the Warrior"
    );
    // Only shares reached the Warrior before it crossed: every drop in its
    // health up to the crossing is, to the log's rounding, the shares logged
    // on that frame.
    let shares: Vec<(f32, f32)> = played
        .log
        .lines()
        .filter_map(|l| {
            let rest = l.split("[TRANSFER] Team 1 Warrior #1 takes ").nth(1)?;
            Some((log_stamp(l), rest.split(' ').next()?.parse().ok()?))
        })
        .collect();
    let track = warrior_track(&played);
    let drops: Vec<(f32, f32)> = track
        .windows(2)
        .filter(|w| w[1].0 <= crossed && w[1].1 < w[0].1)
        .map(|w| (w[1].0, w[0].1 - w[1].1))
        .collect();
    assert!(!drops.is_empty(), "the Warrior's health never dropped");
    for (at, drop) in &drops {
        let logged: Vec<f32> = shares
            .iter()
            .filter(|(stamp, _)| (stamp - at).abs() <= 0.006)
            .map(|(_, n)| *n)
            .collect();
        let sum: f32 = logged.iter().sum();
        assert!(
            !logged.is_empty() && (sum - drop).abs() <= 0.5 * logged.len() as f32,
            "a drop of {drop} at {at} is not the shares logged there ({logged:?})"
        );
    }
    let aegis = aegis_lines(&played);
    assert_eq!(aegis.len(), 1);
    assert!(
        shield_appears_at(&played, crossed),
        "the shield did not appear on the frame the transfer crossed the line"
    );
    let soaked_shares = played
        .log
        .lines()
        .filter(|l| {
            log_stamp(l) > aegis[0].0
                && l.contains("[TRANSFER] Team 1 Warrior #1 takes")
                && l.contains("absorbed)")
        })
        .count();
    assert!(
        soaked_shares > 0,
        "the shield soaked no share:\n{}",
        played.log
    );
}

/// A log line's stamp ("[ 13.80s] ...").
fn log_stamp(line: &str) -> f32 {
    line.trim_start_matches('[')
        .split('s')
        .next()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(f32::MAX)
}
