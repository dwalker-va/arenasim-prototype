//! A Priest's dip that aborts straight to FREE is traced once, as `DipAbort`.
//!
//! The posture evaluation traces every DIP -> FREE transition as a
//! `DipAbort`; the FREE tick that follows on the same frame used to trace the
//! same transition again, as a `PressuredExit` with `previous_posture: dip`.
//!
//! STAGED matches (tier 3, `headless::stage`): the Priest is carried into the
//! stage mid-dip, nothing pressuring it, so an abort lands it in FREE. The
//! abort (a spent budget) is paired with the same stage with budget left, in
//! which the dip holds and nothing is traced out of it.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::components::{HealerPosture, Posture};
use bevy::prelude::Time;
use common::staged::{of_class, play, sim_time, Played};

const PRIEST: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(1, 1);
const WARLOCK: Unit = Unit::new(2, 0);
const ENEMY_PRIEST: Unit = Unit::new(2, 1);

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Warlock".into(), "Priest".into()],
        map: "BasicArena".into(),
        ai_profile: Some("Legacy".into()),
        // Nobody is after our Priest.
        team2_kill_target: Some(1),
        random_seed: Some(seed),
        max_duration_secs: 10.0,
        ..Default::default()
    }
}

/// The Priest at the origin, mid-dip at the enemy Priest 20yd east of it
/// (beyond Psychic Scream's reach, so the dip does not complete), with
/// `budget_left` seconds of dip budget. The Warlock stands 40yd west and both
/// enemies are after the Warrior, so nothing pressures the Priest: an abort
/// lands it in FREE.
fn stage(budget_left: f32) -> Stage {
    Stage::new()
        .place(PRIEST, 0.0, 0.0)
        .place(WARRIOR, -2.0, 3.0)
        .place(ENEMY_PRIEST, 20.0, 0.0)
        .place(WARLOCK, -40.0, 0.0)
        .target(WARLOCK, WARRIOR)
        .target(ENEMY_PRIEST, WARRIOR)
        .world(move |w| {
            // The posture machine's clock is the app's (`Time`).
            let now = w.resource::<Time>().elapsed_secs();
            let target = ENEMY_PRIEST.entity(w);
            let priest = PRIEST.entity(w);
            w.entity_mut(priest).insert(HealerPosture {
                posture: Posture::Dip,
                dip_target: Some(target),
                dip_until: now + budget_left,
                ..HealerPosture::new(now - 1.0)
            });
        })
}

/// The team-1 Priest's traced transitions out of DIP over the first half
/// second of the stage, as (combat time, trigger, posture).
fn exits(played: &Played) -> Vec<(f32, String, String)> {
    let by = played.landed().combat + 0.5;
    of_class(played.staged_trace(), "Priest")
        .filter(|e| e["actor"]["team"] == 1 && e["kind"] == "movement_decision")
        .filter(|e| e["previous_posture"] == "dip")
        .filter(|e| sim_time(e) <= by)
        .map(|e| {
            (
                sim_time(e),
                e["trigger"].as_str().unwrap_or("").to_string(),
                e["posture"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

const SEEDS: [u64; 2] = [1, 2];

/// A spent budget aborts the dip to FREE, and the trace records that
/// transition exactly once: a `DipAbort` into `free`, and no `PressuredExit`.
#[test]
fn a_dip_aborted_to_free_is_traced_once() {
    for seed in SEEDS {
        let played = play(config(seed), stage(0.0));
        let exits = exits(&played);
        assert_eq!(
            exits.len(),
            1,
            "seed {seed}: the abort was not traced exactly once: {exits:?}"
        );
        let (_, trigger, posture) = &exits[0];
        assert_eq!(
            (trigger.as_str(), posture.as_str()),
            ("DipAbort", "free"),
            "seed {seed}: the abort was not traced as a DipAbort into FREE: {exits:?}"
        );
    }
}

/// The control: budget left, so the dip holds and nothing is traced out of it.
#[test]
fn a_budgeted_dip_traces_no_exit() {
    for seed in SEEDS {
        let played = play(config(seed), stage(4.0));
        let exits = exits(&played);
        assert!(
            exits.is_empty(),
            "seed {seed}: the budgeted dip was traced out of: {exits:?}"
        );
    }
}
