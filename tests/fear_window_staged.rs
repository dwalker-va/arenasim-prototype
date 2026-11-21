//! AS-495 — the Warlock's Fear window played through in a real 1v1 against a
//! healer: the Warlock's own Fear lands on the Druid as the gates open, its
//! Corruption and Curse of Agony already ticking, and its mana short of full.
//!
//! What the window must show, frame by frame:
//!
//! - the Fear holds its full eight seconds: nothing the Warlock does breaks
//!   it, and no direct hit of the Warlock's lands, before its last
//!   `FEAR_WINDOW_LEAD` — less the flight a feared Druid's unseen run can cut
//!   (the bolt is aimed with the Druid led one leg along its run, and the
//!   Fear turns it at random after that);
//! - a Shadow Bolt lands as the Fear ends;
//! - the Warlock spends the window's globals (a Life Tap topping its mana up)
//!   instead of idling through it: its idle time in the window is bounded by
//!   the window less the tap's global cooldown and the bolt's cast.
//!
//! The decisions themselves are pinned one tick at a time in
//! `tests/fear_window_scenes.rs`. A STAGED match (tier 3, `headless::stage`):
//! the situation holds by construction, the seed governs only the rolls.

mod common;

use std::sync::{Arc, Mutex};

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::FEAR_WINDOW_LEAD;
use arenasim::states::play_match::components::AuraType;
use arenasim::states::play_match::components::Combatant;
use arenasim::states::play_match::AbilityType;
use common::staged::{entity_id, play, sim_time, Played};

const WARLOCK: Unit = Unit::new(1, 0);
const DRUID: Unit = Unit::new(2, 0);
const FRAME: f32 = 1.0 / 60.0;
/// Fear's full duration (`abilities.ron`).
const FEAR_SECS: f32 = 8.0;
/// How late after the Fear's end its bolt may land. The bolt is timed to its
/// last `FEAR_WINDOW_LEAD` with the feared Druid led along its run for one
/// leg (`cast_guard::lands_on_feared`); a Druid that turns away mid-cast
/// lengthens the flight past the prediction.
const LATE: f32 = 0.25;

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Druid".into()],
        random_seed: Some(seed),
        max_duration_secs: 60.0,
        ..Default::default()
    }
}

/// The stage, and a slot it fills with the bolt's flight slack: the most an
/// unseen run at the Warlock can shorten a Shadow Bolt's cast and flight onto
/// the feared Druid — through the whole cast and the longest flight (the
/// bolt's range), at the Druid's speed over the bolt's. The lead
/// (`cast_guard::lands_on_feared`) sees one leg of the run; the rest is the
/// Fear's random turns.
fn stage(slack: Arc<Mutex<f32>>) -> Stage {
    Stage::new()
        .world(move |world| {
            let bolt = world
                .resource::<AbilityDefinitions>()
                .get_unchecked(&AbilityType::Shadowbolt)
                .clone();
            let druid = DRUID.entity(world);
            let speed = world
                .get::<Combatant>(druid)
                .expect("the Druid")
                .base_movement_speed;
            let flies = bolt.projectile_speed.expect("a bolt flies");
            *slack.lock().unwrap() = (bolt.cast_time + bolt.range / flies) * speed / flies;
        })
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
/// not hits), read off the combat log.
fn bolt_hits(played: &Played) -> Vec<f32> {
    played
        .log
        .lines()
        .filter(|l| {
            l.contains("Team 1 Warlock #1's ")
                && l.contains(" hits Team 2 Druid #1 ")
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
    for seed in [1, 2, 3] {
        let slack = Arc::new(Mutex::new(f32::NAN));
        let played = play(config(seed), stage(slack.clone()));
        let slack = *slack.lock().unwrap();
        let early = FEAR_WINDOW_LEAD + slack;
        let landed = played.landed().app;
        let druid = played.frames[0]
            .combatants
            .iter()
            .find(|(_, c)| c.team == 2 && !c.is_pet)
            .map(|(e, _)| *e)
            .expect("the Druid");

        // The Fear: from the stage to the first frame without it.
        let ended = played
            .staged_frames()
            .skip(1)
            .find(|f| !f.combatants[&druid].aura_types.contains(&AuraType::Fear))
            .map(|f| f.sim_time)
            .unwrap_or_else(|| panic!("seed {seed}: the Fear ends"));
        let feared = played
            .staged_frames()
            .find(|f| f.combatants[&druid].aura_types.contains(&AuraType::Fear))
            .map(|f| f.sim_time)
            .unwrap_or_else(|| panic!("seed {seed}: the Fear lands"));
        let held = ended - feared;
        assert!(
            held >= FEAR_SECS - early - 2.0 * FRAME && held <= FEAR_SECS + 2.0 * FRAME,
            "seed {seed}: the Fear held {held:.3}s, broken more than {early:.2}s early; \
             on the Druid: {:#?}",
            played
                .log
                .lines()
                .filter(|l| l.contains("Druid #1"))
                .collect::<Vec<_>>()
        );

        // No direct hit before the last moment; a bolt in it.
        let hits = bolt_hits(&played);
        let until = feared + FEAR_SECS;
        assert!(
            hits.iter()
                .all(|&t| t < landed || t >= until - early - FRAME),
            "seed {seed}: a hit landed early in the window {landed:.2}..{until:.2}: {hits:?}"
        );
        assert!(
            hits.iter()
                .any(|&t| t >= until - early - FRAME && t <= until + LATE),
            "seed {seed}: a bolt lands as the Fear ends (ends {until:.2}): {hits:?}"
        );
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
}
