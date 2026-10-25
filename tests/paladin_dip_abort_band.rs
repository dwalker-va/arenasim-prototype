//! The Paladin's Hammer of Justice dip keeps its budget and its dying-teammate
//! abort while an enemy caster sits in the PRESSURED hysteresis band (AS-380,
//! the Paladin twin of AS-371).
//!
//! Under `Legacy` a closing caster focuses the Paladin on the plain trigger
//! (`compound_pressure_trigger`) a band before it ONSETS pressure
//! (`compound_pressure_onset`, AS-249): a Warlock targeting the Paladin from
//! between its 28yd preferred range and the 30yd intent radius holds the
//! trigger without the onset. A dip is preempted only by the onset, so in that
//! band it runs on — and its abort, once gated on the trigger being off, never
//! ran: no budget, no dying-teammate exit.
//!
//! STAGED matches (tier 3, `headless::stage`): the Paladin is carried into the
//! stage mid-dip, walking at the enemy healer, with the Warlock in the band.
//! Each abort is paired with the same stage minus its one condition, in which
//! the dip holds. The seeds govern only the rolls; the situation holds by
//! construction.
//!
//! The same aborts with no caster in the band close the file, with an abort
//! that has already arrived in Hammer of Justice's reach and casts nothing.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::components::{HealerPosture, Posture};
use bevy::prelude::Time;
use common::staged::{entity_id, of_class, play, sim_time, Played};

/// Below `urgency_hp_threshold` (0.5).
const DYING: f32 = 0.3;

const PALADIN: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(1, 1);
const WARLOCK: Unit = Unit::new(2, 0);
const ENEMY_PRIEST: Unit = Unit::new(2, 1);

/// The Warlock's distance from the Paladin: beyond its 28yd preferred range
/// (closing) and inside the 30yd intent radius, so the trigger holds; within
/// `danger_edge_margin` (1yd) of both edges, so the `Legacy` onset does not.
const IN_BAND: f32 = 29.0;
/// The band's edges: the Warlock's preferred range, the intent radius.
const BAND: (f32, f32) = (28.0, 30.0);

fn config(seed: u64, profile: &str) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Paladin".into(), "Warrior".into()],
        team2: vec!["Warlock".into(), "Priest".into()],
        map: "BasicArena".into(),
        ai_profile: Some(profile.into()),
        // The Warlock is after our Paladin.
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

/// The Paladin at the origin, mid-dip at the enemy Priest 20yd east of it
/// (beyond Hammer of Justice's 10yd reach, so the dip does not complete), the
/// Warlock [`IN_BAND`] west of it targeting it, the Warrior beside the Paladin.
fn stage(dip: Dip) -> Stage {
    Stage::new()
        .place(PALADIN, 0.0, 0.0)
        .place(WARRIOR, -2.0, 3.0)
        .health(WARRIOR, dip.warrior)
        .place(ENEMY_PRIEST, 20.0, 0.0)
        .place(WARLOCK, -IN_BAND, 0.0)
        .target(WARLOCK, PALADIN)
        .world(move |w| {
            // The posture machine's clock is the app's (`Time`), countdown
            // included — not the trace's combat clock.
            let now = w.resource::<Time>().elapsed_secs();
            let target = ENEMY_PRIEST.entity(w);
            let paladin = PALADIN.entity(w);
            w.entity_mut(paladin).insert(HealerPosture {
                posture: Posture::Dip,
                dip_target: Some(target),
                dip_until: now + dip.budget_left,
                ..HealerPosture::new(now - 1.0)
            });
        })
}

/// The Paladin's posture transitions out of DIP, as (combat time, trigger).
fn transitions(played: &Played) -> Vec<(f32, String)> {
    of_class(played.staged_trace(), "Paladin")
        .filter(|e| e["kind"] == "movement_decision")
        .filter(|e| e["previous_posture"] == "dip")
        .map(|e| (sim_time(e), e["trigger"].as_str().unwrap_or("").to_string()))
        .collect()
}

/// The stage held: on the frame it landed the Warlock stood in the band,
/// targeting the Paladin. Otherwise the case proves nothing about the band.
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
    let (paladin, p) = unit(1, 0);
    let (_, warlock) = unit(2, 0);
    let d = warlock.position.distance(p.position);
    assert!(
        d > BAND.0 && d <= BAND.1,
        "{case}: the Warlock stood {d:.2}yd from the Paladin, outside the band {BAND:?}"
    );
    assert_eq!(
        warlock.target,
        Some(*paladin),
        "{case}: the Warlock was not targeting the Paladin"
    );
}

/// The dip's first evaluation: within a frame of the stage landing.
fn first_evaluation(played: &Played) -> f32 {
    played.landed().combat + 1.5 / 60.0
}

/// The first transition out of DIP, if it came on the dip's first evaluation.
fn first_exit(played: &Played) -> Option<String> {
    let by = first_evaluation(played);
    transitions(played)
        .into_iter()
        .find(|(t, _)| *t <= by)
        .map(|(_, trigger)| trigger)
}

const SEEDS: [u64; 2] = [1, 2];

/// A dying Warrior aborts the dip on its first evaluation, Warlock in the
/// band or not. Its pair: the Warrior healthy, the same stage — the dip holds
/// ([`assert_dip_holds`]).
#[test]
fn a_dying_teammate_aborts_the_dip_with_a_caster_in_the_band() {
    for seed in SEEDS {
        let case = format!("dying, seed {seed}");
        let played = play(
            config(seed, "Legacy"),
            stage(Dip {
                warrior: DYING,
                budget_left: 4.0,
            }),
        );
        assert_in_band(&played, &case);
        assert_eq!(
            first_exit(&played).as_deref(),
            Some("DipAbort"),
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
        let case = format!("spent, seed {seed}");
        let played = play(
            config(seed, "Legacy"),
            stage(Dip {
                warrior: 1.0,
                budget_left: 0.0,
            }),
        );
        assert_in_band(&played, &case);
        assert_eq!(
            first_exit(&played).as_deref(),
            Some("DipAbort"),
            "{case}: the dip did not abort on its first evaluation; transitions {:?}",
            transitions(&played)
        );

        assert_dip_holds(seed);
    }
}

/// The control: Warrior healthy, budget left, Warlock in the band — nothing
/// asks the dip to end, so it holds for at least half a second, and the
/// Paladin walks it toward the enemy Priest.
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
    // Not vacuous: the Paladin is walking its dip, toward the enemy Priest.
    let paladin = entity_id(&played, 1, 0);
    let x_at = |t: f32| {
        played
            .frames
            .iter()
            .find(|f| f.sim_time >= played.landed().app + t)
            .and_then(|f| {
                f.combatants
                    .iter()
                    .find(|(e, _)| e.index() as u64 == paladin)
                    .map(|(_, c)| c.position.x)
            })
            .expect("the Paladin")
    };
    assert!(
        x_at(0.5) > x_at(0.0) + 1.0,
        "{case}: the Paladin is not walking its dip east ({} -> {})",
        x_at(0.0),
        x_at(0.5)
    );
}

/// Under `TeamPlan` the onset is the trigger, so the same stage preempts the
/// dip into PRESSURED rather than aborting it — the band does not exist there,
/// and the abort's old `!focused` gate never decided anything.
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
        assert_eq!(
            first_exit(&played).as_deref(),
            Some("PressuredEnter"),
            "{case}: expected a preemption into PRESSURED on the first evaluation; transitions {:?}",
            transitions(&played)
        );
    }
}

// --- With no caster in the band (AS-116, from AS-380) -----------------------
//
// The plain aborts, staged: the Warlock is on our Warrior, so nothing focuses
// the Paladin and neither the trigger nor the onset holds. These are the
// aborts every dip outside the band takes; until now only a seed-pinned probe
// (`movement_probes::paladin_postures::dip_aborts_without_casting`, migrated
// here) covered them.

/// The Warlock is after our Warrior.
fn plain_config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team2_kill_target: Some(1),
        ..config(seed, "Legacy")
    }
}

/// [`stage`]'s dip with the enemy Priest `priest_at` yards east of the
/// Paladin, and the Warlock 35yd off, beyond the intent radius, targeting our
/// Warrior.
fn plain_stage(dip: Dip, priest_at: f32) -> Stage {
    stage(dip)
        .place(ENEMY_PRIEST, priest_at, 0.0)
        .place(WARLOCK, -30.0, 18.0)
        .target(WARLOCK, WARRIOR)
}

/// The stage held: on the frame it landed the Warlock was targeting our
/// Warrior, more than the intent radius from the Paladin.
fn assert_no_caster_on_the_paladin(played: &Played, case: &str) {
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
    let (_, p) = unit(1, 0);
    let (warrior, _) = unit(1, 1);
    let (_, warlock) = unit(2, 0);
    let d = warlock.position.distance(p.position);
    assert!(
        d > BAND.1,
        "{case}: the Warlock stood {d:.2}yd from the Paladin, inside the intent radius"
    );
    assert_eq!(
        warlock.target,
        Some(*warrior),
        "{case}: the Warlock was not targeting the Warrior"
    );
}

/// The Paladin's Hammer of Justice casts, on the combat clock.
fn hammers(played: &Played) -> Vec<f32> {
    of_class(played.staged_trace(), "Paladin")
        .filter(|e| {
            e["kind"] == "ability_decision"
                && e["outcome"]["type"] == "action_taken"
                && e["outcome"]["ability"] == "HammerOfJustice"
        })
        .map(sim_time)
        .collect()
}

/// A dying Warrior and a spent budget each abort the dip on its first
/// evaluation with no caster anywhere near the band; the same stage with
/// neither holds the dip.
#[test]
fn the_plain_aborts_with_no_caster_in_the_band() {
    for seed in SEEDS {
        for (case, dip) in [
            (
                "dying",
                Dip {
                    warrior: DYING,
                    budget_left: 4.0,
                },
            ),
            (
                "spent",
                Dip {
                    warrior: 1.0,
                    budget_left: 0.0,
                },
            ),
        ] {
            let case = format!("{case}, no caster, seed {seed}");
            let played = play(plain_config(seed), plain_stage(dip, 20.0));
            assert_no_caster_on_the_paladin(&played, &case);
            assert_eq!(
                first_exit(&played).as_deref(),
                Some("DipAbort"),
                "{case}: the dip did not abort on its first evaluation; transitions {:?}",
                transitions(&played)
            );
        }

        let case = format!("control, no caster, seed {seed}");
        let played = play(
            plain_config(seed),
            plain_stage(
                Dip {
                    warrior: 1.0,
                    budget_left: 4.0,
                },
                20.0,
            ),
        );
        assert_no_caster_on_the_paladin(&played, &case);
        let landed = played.landed().combat;
        let early: Vec<_> = transitions(&played)
            .into_iter()
            .filter(|(t, _)| *t < landed + 0.5)
            .collect();
        assert!(
            early.is_empty(),
            "{case}: the dip left DIP with nothing asking it to: {early:?}"
        );
    }
}

/// An abort casts nothing (migrated from the seed-pinned
/// `movement_probes::paladin_postures::dip_aborts_without_casting`). The enemy
/// Priest stands 9yd off, inside Hammer of Justice's 10yd reach, so the dip
/// has ARRIVED: with the Warrior dying it aborts on its first evaluation, its
/// first action is on the Warrior, and no Hammer goes out for the next
/// second; with the Warrior healthy, the
/// control, the same arrival completes the dip and casts it.
#[test]
fn an_abort_in_hammer_reach_casts_nothing() {
    for seed in SEEDS {
        let case = format!("dying, in reach, seed {seed}");
        let played = play(
            plain_config(seed),
            plain_stage(
                Dip {
                    warrior: DYING,
                    budget_left: 4.0,
                },
                9.0,
            ),
        );
        assert_no_caster_on_the_paladin(&played, &case);
        assert_eq!(
            first_exit(&played).as_deref(),
            Some("DipAbort"),
            "{case}: the dip did not abort on its first evaluation; transitions {:?}",
            transitions(&played)
        );
        let landed = played.landed().combat;
        let cast: Vec<f32> = hammers(&played)
            .into_iter()
            .filter(|t| *t < landed + 1.0)
            .collect();
        assert!(
            cast.is_empty(),
            "{case}: an aborted dip cast Hammer of Justice at {cast:?}"
        );
        // What the abort frees the global cooldown for: the heal on the dying
        // Warrior, on the abort's own evaluation.
        let warrior = entity_id(&played, 1, 1);
        let first = of_class(played.staged_trace(), "Paladin")
            .find(|e| e["kind"] == "ability_decision" && e["outcome"]["type"] == "action_taken")
            .unwrap_or_else(|| panic!("{case}: the Paladin did nothing"));
        assert!(
            sim_time(first) <= first_evaluation(&played)
                && first["outcome"]["target_id"].as_u64() == Some(warrior),
            "{case}: the abort's first action was not on the dying Warrior: {first}"
        );

        let case = format!("control, in reach, seed {seed}");
        let played = play(
            plain_config(seed),
            plain_stage(
                Dip {
                    warrior: 1.0,
                    budget_left: 4.0,
                },
                9.0,
            ),
        );
        assert_no_caster_on_the_paladin(&played, &case);
        assert_eq!(
            first_exit(&played).as_deref(),
            Some("DipComplete"),
            "{case}: the arrival did not complete the dip; transitions {:?}",
            transitions(&played)
        );
        let by = first_evaluation(&played);
        assert!(
            hammers(&played).iter().any(|t| *t <= by),
            "{case}: the completed dip cast no Hammer of Justice: {:?}",
            hammers(&played)
        );
    }
}
