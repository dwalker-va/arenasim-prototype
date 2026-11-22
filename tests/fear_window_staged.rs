//! AS-495 — the Warlock's Fear window played through in a real 1v1 against a
//! healer: the Warlock's own Fear lands on the Druid as the gates open, its
//! Corruption and Curse of Agony already ticking, and its mana short of full.
//!
//! What the window must show, over a bank of seeds, in literal seconds (the
//! ruling's quarter second, never derived from the constants under test):
//!
//! - the timed Shadow Bolt lands around the Fear's end: the median bolt lands
//!   inside its last quarter second, short of its first instant, and none
//!   lands more than [`EARLY_MAX`] early or [`LATE_MAX`] late. The bolt is
//!   aimed at the middle of that quarter second, its landing predicted with
//!   the feared Druid led along the heading it runs on; the Fear turns the
//!   Druid at random, which spreads it;
//! - no direct hit of the Warlock's lands earlier in the Fear, and the Fear
//!   holds its full eight seconds to within that spread;
//! - the Warlock spends the window's globals (a Life Tap topping its mana up)
//!   instead of idling through it: its idle time in the window is bounded by
//!   the window less the tap's global cooldown and the bolt's cast.
//!
//! The decisions themselves are pinned one tick at a time in
//! `tests/fear_window_scenes.rs`. A STAGED match (tier 3, `headless::stage`):
//! the situation holds by construction, the seed governs only the rolls.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::components::AuraType;
use arenasim::states::play_match::AbilityType;
use common::staged::{entity_id, play, sim_time, Played};

const WARLOCK: Unit = Unit::new(1, 0);
const DRUID: Unit = Unit::new(2, 0);
const FRAME: f32 = 1.0 / 60.0;
/// Fear's full duration (`abilities.ron`).
const FEAR_SECS: f32 = 8.0;
const SEEDS: std::ops::RangeInclusive<u64> = 1..=12;
/// The ruling's window: damage lands in the Fear's last quarter second.
const WINDOW: f32 = 0.25;
/// The median bolt lands no closer to the end than this: inside the window,
/// clear of its last instant.
const MEDIAN_LATEST: f32 = -0.05;
/// ...and no further before the end than this: short of the window's first
/// instant, so a window aimed half a second wider fails here.
const MEDIAN_EARLIEST: f32 = 0.17;
/// The most any one bolt lands before the Fear ends. Measured 0.45s over this
/// bank: a feared Druid that turns to run at the Warlock mid-cast shortens
/// the flight past the prediction.
const EARLY_MAX: f32 = 0.6;
/// The most any one bolt lands after it: one that turns away lengthens it.
const LATE_MAX: f32 = 0.25;

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Druid".into()],
        random_seed: Some(seed),
        max_duration_secs: 60.0,
        ..Default::default()
    }
}

fn stage() -> Stage {
    Stage::new()
        .place(WARLOCK, 0.0, 0.0)
        .place(DRUID, 20.0, 0.0)
        .target(WARLOCK, DRUID)
        .target(DRUID, WARLOCK)
        .mana(WARLOCK, 0.35)
        .land(DRUID, AbilityType::Corruption, WARLOCK)
        .land(DRUID, AbilityType::CurseOfAgony, WARLOCK)
        .land(DRUID, AbilityType::Fear, WARLOCK)
}

/// App-clock times of the Warlock's direct hits on the Druid (DoT ticks are
/// not hits, and neither is the wand, which fires the frame a hold ends),
/// read off the combat log.
fn bolt_hits(played: &Played) -> Vec<f32> {
    played
        .log
        .lines()
        .filter(|l| {
            l.contains("Team 1 Warlock #1's ")
                && (l.contains(" hits Team 2 Druid #1 ") || l.contains(" CRITS Team 2 Druid #1 "))
                && !l.contains("Wand")
        })
        .filter_map(|l| {
            let open = l.find('[')?;
            let close = l[open..].find("s]")? + open;
            l[open + 1..close].trim().parse().ok()
        })
        .collect()
}

#[test]
fn the_fear_window_holds_its_full_length_and_its_damage_lands_as_it_ends() {
    // How long before the Fear's end each seed's timed bolt landed (negative:
    // after it).
    let mut offsets = Vec::new();
    for seed in SEEDS {
        let played = play(config(seed), stage());
        let landed = played.landed().app;
        let druid = played.frames[0]
            .combatants
            .iter()
            .find(|(_, c)| c.team == 2 && !c.is_pet)
            .map(|(e, _)| *e)
            .expect("the Druid");

        // The Fear: from its first frame to the first frame without it.
        let feared = played
            .staged_frames()
            .find(|f| f.combatants[&druid].aura_types.contains(&AuraType::Fear))
            .map(|f| f.sim_time)
            .unwrap_or_else(|| panic!("seed {seed}: the Fear lands"));
        let ended = played
            .staged_frames()
            .skip(1)
            .find(|f| !f.combatants[&druid].aura_types.contains(&AuraType::Fear))
            .map(|f| f.sim_time)
            .unwrap_or_else(|| panic!("seed {seed}: the Fear ends"));
        let held = ended - feared;
        assert!(
            (FEAR_SECS - EARLY_MAX - 2.0 * FRAME..=FEAR_SECS + 2.0 * FRAME).contains(&held),
            "seed {seed}: the Fear held {held:.3}s"
        );

        // No direct hit earlier in the Fear; the timed bolt around its end.
        let hits = bolt_hits(&played);
        let until = feared + FEAR_SECS;
        assert!(
            hits.iter()
                .all(|&t| t < landed || t >= until - EARLY_MAX - FRAME),
            "seed {seed}: a hit landed early in the Fear {feared:.2}..{until:.2}: {hits:?}"
        );
        let bolt = hits
            .iter()
            .copied()
            .find(|&t| t >= until - EARLY_MAX - FRAME)
            .unwrap_or_else(|| panic!("seed {seed}: no bolt around the Fear's end: {hits:?}"));
        let offset = until - bolt;
        assert!(
            (-LATE_MAX - FRAME..=EARLY_MAX + FRAME).contains(&offset),
            "seed {seed}: the bolt landed {offset:.3}s before the Fear's end"
        );
        offsets.push(offset);
        assert!(
            played
                .log
                .contains("Team 1 Warlock #1 begins casting Shadow Bolt on Team 2 Druid #1"),
            "seed {seed}: the hit is the window's bolt"
        );

        // The window's globals: a Life Tap, and idle time bounded by the
        // window less that tap's global cooldown and the bolt's cast.
        let warlock = entity_id(&played, 1, 0);
        let (combat_landed, combat_until) =
            (played.landed().combat, played.landed().combat + FEAR_SECS);
        let in_window = |e: &&serde_json::Value| {
            e["kind"] == "ability_decision"
                && e["actor"]["entity_id"].as_u64() == Some(warlock)
                && (combat_landed..combat_until).contains(&sim_time(e))
        };
        let tapped = played
            .staged_trace()
            .filter(in_window)
            .any(|e| e["outcome"]["ability"] == "LifeTap");
        assert!(
            tapped,
            "seed {seed}: the window's global goes on a Life Tap"
        );
        let idle = played
            .staged_trace()
            .filter(in_window)
            .filter(|e| e["outcome"]["type"] == "no_action")
            .count() as f32
            * FRAME;
        assert!(
            idle <= FEAR_SECS - 1.5 - 2.0 + 0.25,
            "seed {seed}: the Warlock idled {idle:.2}s of the {FEAR_SECS}s window"
        );
    }

    offsets.sort_by(f32::total_cmp);
    let median = (offsets[offsets.len() / 2 - 1] + offsets[offsets.len() / 2]) / 2.0;
    assert!(
        (MEDIAN_LATEST..=MEDIAN_EARLIEST).contains(&median),
        "the median bolt landed {median:.3}s before the Fear's end, outside \
         {MEDIAN_LATEST}..={MEDIAN_EARLIEST} (the window is the last {WINDOW}s): {offsets:?}"
    );
    // As the Fear ends: in its last quarter second, or a tenth of a second
    // after it (the late side of the spread, where the target was free for
    // that tenth).
    let inside = offsets
        .iter()
        .filter(|&&o| (-0.1..=WINDOW + FRAME).contains(&o))
        .count();
    assert!(
        inside * 3 >= offsets.len() * 2,
        "two bolts in three land as the Fear ends: {inside} of {}: {offsets:?}",
        offsets.len()
    );
}
