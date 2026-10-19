//! The Priest's Psychic Scream dip keeps its budget and its dying-teammate
//! abort while an enemy caster sits in the PRESSURED hysteresis band (AS-371).
//!
//! Under `Legacy` a closing caster pressures the Priest on the plain trigger
//! (`compound_pressure_trigger`) a band before it ONSETS pressure
//! (`compound_pressure_onset`, AS-249): a Warlock targeting the Priest from
//! between its 28yd preferred range and the 30yd intent radius holds the
//! trigger without the onset. A dip is preempted only by the onset, so in that
//! band it runs on — and its abort, once gated on the trigger being off, never
//! ran: no budget, no dying-teammate exit.
//!
//! STAGED matches (tier 3, `headless::stage`): the Priest is carried into the
//! stage mid-dip, walking at the enemy healer, with the Warlock in the band.
//! Each abort is paired with the same stage minus its one condition, in which
//! the dip holds. The seeds govern only the rolls; the situation holds by
//! construction.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::components::{HealerPosture, Posture};
use bevy::prelude::Time;
use common::staged::{entity_id, of_class, play, sim_time, Played};

/// Below `urgency_hp_threshold` (0.5).
const DYING: f32 = 0.3;

const PRIEST: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(1, 1);
const WARLOCK: Unit = Unit::new(2, 0);
const ENEMY_PRIEST: Unit = Unit::new(2, 1);

/// The Warlock's distance from the Priest: beyond its 28yd preferred range
/// (closing) and inside the 30yd intent radius, so the trigger holds; within
/// `danger_edge_margin` (1yd) of both edges, so the `Legacy` onset does not.
const IN_BAND: f32 = 29.0;
/// The band's edges: the Warlock's preferred range, the intent radius.
const BAND: (f32, f32) = (28.0, 30.0);

fn config(seed: u64, profile: &str) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Warlock".into(), "Priest".into()],
        map: "BasicArena".into(),
        ai_profile: Some(profile.into()),
        // The Warlock is after our Priest.
        team2_kill_target: Some(0),
        random_seed: Some(seed),
        max_duration_secs: 10.0,
        ..Default::default()
    }
}

/// What the dip carries into the stage.
#[derive(Clone, Copy, Debug)]
struct Dip {
    /// The Warrior's health fraction.
    warrior: f32,
    /// Seconds of dip budget left (`dip_until - now`).
    budget_left: f32,
}

/// The Priest at the origin, mid-dip at the enemy Priest 20yd east of it
/// (beyond Psychic Scream's reach, so the dip does not complete), the Warlock
/// [`IN_BAND`] west of it targeting it, the Warrior beside the Priest.
fn stage(dip: Dip) -> Stage {
    Stage::new()
        .place(PRIEST, 0.0, 0.0)
        .place(WARRIOR, -2.0, 3.0)
        .health(WARRIOR, dip.warrior)
        .place(ENEMY_PRIEST, 20.0, 0.0)
        .place(WARLOCK, -IN_BAND, 0.0)
        .target(WARLOCK, PRIEST)
        .world(move |w| {
            // The posture machine's clock is the app's (`Time`), countdown
            // included — not the trace's combat clock.
            let now = w.resource::<Time>().elapsed_secs();
            let target = ENEMY_PRIEST.entity(w);
            let priest = PRIEST.entity(w);
            w.entity_mut(priest).insert(HealerPosture {
                posture: Posture::Dip,
                dip_target: Some(target),
                dip_until: now + dip.budget_left,
                ..HealerPosture::new(now - 1.0)
            });
        })
}

/// The team-1 Priest's posture transitions, as (combat time, trigger).
fn transitions(played: &Played) -> Vec<(f32, String)> {
    of_class(played.staged_trace(), "Priest")
        .filter(|e| e["actor"]["team"] == 1 && e["kind"] == "movement_decision")
        .filter(|e| e["previous_posture"] == "dip")
        .map(|e| (sim_time(e), e["trigger"].as_str().unwrap_or("").to_string()))
        .collect()
}

/// The stage held: on the frame it landed the Warlock stood in the band,
/// targeting the Priest. Otherwise the case proves nothing about the band.
fn assert_in_band(played: &Played, case: &str) {
    let landed = played.landed().app;
    let frame = played
        .frames
        .iter()
        .find(|f| f.sim_time >= landed)
        .expect("the landing frame");
    let unit = |team: u8, slot: u8| {
        frame
            .combatants
            .iter()
            .find(|(_, c)| c.team == team && c.slot == slot && !c.is_pet)
            .expect("the unit")
    };
    let (priest, p) = unit(1, 0);
    let (_, warlock) = unit(2, 0);
    let d = warlock.position.distance(p.position);
    assert!(
        d > BAND.0 && d <= BAND.1,
        "{case}: the Warlock stood {d:.2}yd from the Priest, outside the band {BAND:?}"
    );
    assert_eq!(
        warlock.target,
        Some(*priest),
        "{case}: the Warlock was not targeting the Priest"
    );
}

/// The dip's first evaluation: within a frame of the stage landing.
fn first_evaluation(played: &Played) -> f32 {
    played.landed().combat + 1.5 / 60.0
}

fn aborted_at_once(played: &Played) -> bool {
    let by = first_evaluation(played);
    transitions(played)
        .iter()
        .any(|(t, trigger)| *t <= by && trigger == "DipAbort")
}

const SEEDS: [u64; 2] = [1, 2];

/// A dying Warrior aborts the dip on its first evaluation, Warlock in the
/// band or not. Its pair: the Warrior healthy, the same stage — the dip
/// holds (no transition out of DIP for half a second).
#[test]
fn a_dying_teammate_aborts_the_dip_with_a_caster_in_the_band() {
    for seed in SEEDS {
        let dying = Dip {
            warrior: DYING,
            budget_left: 4.0,
        };
        let case = format!("dying, seed {seed}");
        let played = play(config(seed, "Legacy"), stage(dying));
        assert_in_band(&played, &case);
        assert!(
            aborted_at_once(&played),
            "{case}: the dip did not abort on its first evaluation; transitions {:?}",
            transitions(&played)
        );

        assert_dip_holds(seed);
    }
}

/// A spent budget aborts the dip on its first evaluation, Warlock in the band
/// or not. Its pair is the same healthy, budgeted stage
/// ([`assert_dip_holds`]).
#[test]
fn a_spent_budget_aborts_the_dip_with_a_caster_in_the_band() {
    for seed in SEEDS {
        let spent = Dip {
            warrior: 1.0,
            budget_left: 0.0,
        };
        let case = format!("spent, seed {seed}");
        let played = play(config(seed, "Legacy"), stage(spent));
        assert_in_band(&played, &case);
        assert!(
            aborted_at_once(&played),
            "{case}: the dip did not abort on its first evaluation; transitions {:?}",
            transitions(&played)
        );
    }
}

/// The control: Warrior healthy, budget left, Warlock in the band — nothing
/// asks the dip to end, so it holds for at least half a second.
fn assert_dip_holds(seed: u64) {
    let case = format!("control, seed {seed}");
    let played = play(
        config(seed, "Legacy"),
        stage(Dip {
            warrior: 1.0,
            budget_left: 4.0,
        }),
    );
    assert_in_band(&played, &case);
    let landed = played.landed().combat;
    let early: Vec<_> = transitions(&played)
        .into_iter()
        .filter(|(t, _)| *t < landed + 0.5)
        .collect();
    assert!(
        early.is_empty(),
        "{case}: the dip left DIP with nothing asking it to: {early:?}"
    );
    // Not vacuous: the Priest is walking its dip, toward the enemy Priest.
    let priest = entity_id(&played, 1, 0);
    let x_at = |t: f32| {
        played
            .frames
            .iter()
            .find(|f| f.sim_time >= played.landed().app + t)
            .and_then(|f| {
                f.combatants
                    .iter()
                    .find(|(e, _)| e.index() as u64 == priest)
                    .map(|(_, c)| c.position.x)
            })
            .expect("the Priest")
    };
    assert!(
        x_at(0.5) > x_at(0.0) + 1.0,
        "{case}: the Priest is not walking its dip east ({} -> {})",
        x_at(0.0),
        x_at(0.5)
    );
}

/// Under `TeamPlan` the onset is the trigger, so the same stage preempts the
/// dip into PRESSURED rather than aborting it — the band does not exist there.
#[test]
fn under_teamplan_the_band_preempts_the_dip() {
    for seed in SEEDS {
        let case = format!("TeamPlan, seed {seed}");
        let played = play(
            config(seed, "TeamPlan"),
            stage(Dip {
                warrior: DYING,
                budget_left: 4.0,
            }),
        );
        assert_in_band(&played, &case);
        let by = first_evaluation(&played);
        let first = transitions(&played).into_iter().find(|(t, _)| *t <= by);
        assert!(
            matches!(&first, Some((_, trigger)) if trigger != "DipAbort"),
            "{case}: expected a preemption out of DIP on the first evaluation, got {first:?}"
        );
    }
}
