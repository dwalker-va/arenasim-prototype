//! AS-296 — a Life Tap played in a real match: the conversion lands at its
//! one site, one for one, at the full amount with arena dampening at 100%
//! (the mana is not healing), and the tap spends the global cooldown.
//!
//! A STAGED match (tier 3, `headless::stage`): the Warlock starved of mana,
//! at full health, its Priest beside it, and the clock past the end of the
//! dampening ramp — by construction, so no seed can stop reaching it. The
//! gates themselves are pinned scene by scene in `tests/life_tap_scenes.rs`.

mod common;

use std::sync::{Arc, Mutex};

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{ArenaDampening, Combatant};
use arenasim::states::play_match::AbilityType;
use common::staged::{of_class, play, sim_time};

const WARLOCK: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const WARRIOR: Unit = Unit::new(2, 0);

/// Past the dampening ramp's end (75s + 120s): healing fully dampened.
const CLOCK: f32 = 200.0;
const MANA: f32 = 0.1;
/// The fixed 60Hz frame.
const FRAME: f32 = 1.0 / 60.0;

fn config() -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into(), "Priest".into()],
        team2: vec!["Warrior".into()],
        // The Warrior is after the Priest, so nobody has the Warlock targeted.
        team2_kill_target: Some(1),
        random_seed: Some(1),
        max_duration_secs: CLOCK + 10.0,
        ..Default::default()
    }
}

/// What the Warlock's tap should convert, read off the staged world.
#[derive(Clone, Copy, Debug, Default)]
struct Expected {
    amount: f32,
    max_health: f32,
    mana_before: f32,
}

#[test]
fn a_tap_converts_its_full_amount_under_full_dampening_and_spends_the_gcd() {
    let expected = Arc::new(Mutex::new(None));
    let read = expected.clone();
    let stage = Stage::new()
        .clock(CLOCK)
        .place(WARLOCK, 0.0, 0.0)
        .place(PRIEST, -10.0, 0.0)
        .place(WARRIOR, 25.0, 0.0)
        .target(WARRIOR, PRIEST)
        .health(WARLOCK, 1.0)
        .mana(WARLOCK, MANA)
        .world(move |world| {
            world.resource_mut::<ArenaDampening>().reduction = 1.0;
            let conversion = world
                .resource::<AbilityDefinitions>()
                .get_unchecked(&AbilityType::LifeTap)
                .health_to_mana
                .expect("Life Tap converts health into mana");
            let e = WARLOCK.entity(world);
            let c = world.get::<Combatant>(e).expect("the Warlock");
            *read.lock().unwrap() = Some(Expected {
                amount: conversion.amount(c.spell_power),
                max_health: c.max_health,
                mana_before: c.current_mana,
            });
        });
    let played = play(config(), stage);
    let expected = expected.lock().unwrap().expect("the stage landed");

    // Non-vacuity: healing was fully dampened when the Warlock tapped.
    assert!(
        played
            .log
            .contains("Arena dampening reaches 100% — healing and absorbs reduced"),
        "the stage should put dampening at 100%"
    );

    // The tap, traced as a choice with no target.
    let warlock: Vec<&serde_json::Value> = of_class(played.staged_trace(), "Warlock")
        .filter(|e| e["kind"] == "ability_decision")
        .collect();
    let tap = warlock
        .iter()
        .find(|e| e["outcome"]["ability"] == "LifeTap")
        .unwrap_or_else(|| panic!("the starved Warlock taps: {warlock:#?}"));
    assert!(tap["outcome"]["target_id"].is_null(), "{tap}");
    let tapped_at = sim_time(tap);

    // One for one, the full amount: health down by it, mana up by it — plus
    // one frame of the gear's mana regeneration, well inside the tolerance and
    // far below any share dampening could take.
    let line = format!(
        "converts {:.0} health into {:.0} mana",
        expected.amount, expected.amount
    );
    assert!(played.log.contains(&line), "the log should say `{line}`");
    let converted = played.staged_frames().any(|f| {
        f.combatants.values().any(|c| {
            c.team == 1
                && c.slot == 0
                && !c.is_pet
                && (c.current_health - (expected.max_health - expected.amount)).abs() < 1e-3
                && (c.current_mana - (expected.mana_before + expected.amount)).abs() < 0.1
        })
    });
    assert!(
        converted,
        "a frame should show the Warlock {:.1} health down and {:.1} mana up from {expected:?}; \
         its first frames: {:?}",
        expected.amount,
        expected.amount,
        played
            .staged_frames()
            .take(3)
            .flat_map(|f| f.combatants.values().filter(|c| c.team == 1 && c.slot == 0))
            .map(|c| (c.current_health, c.max_health, c.current_mana, c.max_mana))
            .collect::<Vec<_>>()
    );

    // The tap is on the global cooldown: the Warlock's next action, with an
    // enemy in Corruption's range and the mana for it, waits it out.
    let next = warlock
        .iter()
        .filter(|e| sim_time(e) > tapped_at && e["outcome"]["type"] == "action_taken")
        .map(|e| sim_time(e))
        .next()
        .expect("the Warlock acts again after its tap");
    assert!(
        next >= tapped_at + 1.5 - FRAME,
        "the Warlock acted {:.3}s after its tap, inside the global cooldown",
        next - tapped_at
    );
}
