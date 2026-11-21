//! AS-496 — the healer posture through a short hold on its attacker, over
//! time (a staged match, tier 3).
//!
//! A held enemy is no pressure, and its pressure returns the frame the hold
//! ends. Neither end may strobe the healer's PRESSURED posture:
//! - a stun long enough for an escape window (`escape_min_window`) takes the
//!   PRESSURED healer into ESCAPE for the hold and back to PRESSURED as it
//!   ends — never through FREE, and nothing further for the PRESSURED hold
//!   that follows;
//! - a stun too short for a window, inside the posture's `pressured_hold`,
//!   changes nothing: the hold keeps the healer PRESSURED across it.
//!
//! The control is the same stage with no stun: the healer stays PRESSURED.
//! Both AI profiles, several seeds (the seed only rolls the dice; the
//! situation holds by construction).

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::{
    AbilityType, Aura, AuraType, HealerPosture, MovementConfig, Posture,
};
use bevy::prelude::*;
use common::staged::{of_class, play, sim_time, Played};

const PRIEST: Unit = Unit::new(1, 0);
const ALLY: Unit = Unit::new(1, 1);
const ENEMY: Unit = Unit::new(2, 0);

/// Seconds into the fight the stage lands, PRESSURED well past its hold.
const LANDS: f32 = 3.0;
/// How long after the stage the posture is read: the stun, the PRESSURED
/// hold that follows it, and slack.
const WATCH: f32 = 4.0;

const PROFILES: [&str; 2] = ["Legacy", "TeamPlan"];
const SEEDS: [u64; 3] = [1, 2, 3];

fn config(profile: &str, seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Warrior".into()],
        map: "BasicArena".to_string(),
        ai_profile: Some(profile.into()),
        random_seed: Some(seed),
        max_duration_secs: 20.0,
        ..Default::default()
    }
}

fn shipped() -> MovementConfig {
    arenasim::states::play_match::movement_config::load_movement_config()
        .expect("movement.ron loads")
}

/// The enemy Warrior on the Priest, 4yd off, the Priest PRESSURED with
/// `hold_left` seconds of its PRESSURED hold to run (on the app clock, which
/// the posture machines read), and the Warrior stunned for `stun` seconds
/// (none for the control). The stun never breaks on damage. The Priest's
/// Psychic Scream is on cooldown so it adds no fear of its own.
fn stage(stun: Option<f32>, hold_left: f32) -> Stage {
    let mut s = Stage::new()
        .after_gates(LANDS)
        .place(PRIEST, 0.0, 0.0)
        .place(ALLY, 0.0, -10.0)
        .place(ENEMY, -4.0, 0.0)
        .target(ENEMY, PRIEST)
        .cooldown(PRIEST, AbilityType::PsychicScream, 30.0)
        .world(move |w| {
            let now = w.resource::<Time>().elapsed_secs();
            let e = PRIEST.entity(w);
            w.entity_mut(e).insert(HealerPosture {
                posture: Posture::Pressured,
                hold_until: now + hold_left,
                ..HealerPosture::new(now - 3.0)
            });
        });
    if let Some(duration) = stun {
        s = s.aura(
            ENEMY,
            Aura {
                effect_type: AuraType::Stun,
                duration,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        );
    }
    s
}

/// The Priest's posture changes in the watch window, in order: each
/// (sim_time, from, to).
fn transitions(played: &Played) -> Vec<(f32, String, String)> {
    let from = played.landed().combat;
    of_class(played.staged_trace(), "Priest")
        .filter(|e| e["kind"] == "movement_decision" && e["actor"]["team"] == 1)
        .filter(|e| sim_time(e) <= from + WATCH)
        .filter_map(|e| {
            let prev = e["previous_posture"].as_str()?;
            Some((
                sim_time(e) - from,
                prev.to_string(),
                e["posture"].as_str().unwrap_or_default().to_string(),
            ))
        })
        .collect()
}

/// The stunned Warrior stays on the Priest and the stun lands: what the
/// test is about happened.
fn assert_staged(played: &Played, what: &str) {
    let enemy = played.frames[0]
        .combatants
        .iter()
        .find(|(_, c)| c.team == 2 && !c.is_pet)
        .map(|(e, _)| *e)
        .expect("the enemy Warrior");
    let priest = played.frames[0]
        .combatants
        .iter()
        .find(|(_, c)| c.team == 1 && c.slot == 0 && !c.is_pet)
        .map(|(e, _)| *e)
        .expect("the Priest");
    let first = played.staged_frames().next().expect("a staged frame");
    assert_eq!(
        first.combatants[&enemy].target,
        Some(priest),
        "{what}: the Warrior is not on the Priest"
    );
}

fn stunned(played: &Played) -> bool {
    played.staged_frames().take(2).any(|f| {
        f.combatants
            .values()
            .any(|c| c.team == 2 && c.aura_types.contains(&AuraType::Stun))
    })
}

fn names(t: &[(f32, String, String)]) -> Vec<(&str, &str)> {
    t.iter().map(|(_, a, b)| (a.as_str(), b.as_str())).collect()
}

#[test]
fn a_short_stun_takes_the_healer_through_escape_and_back_without_strobing() {
    let stun = 1.0;
    let hold = shipped().shared.pressured_hold;
    assert!(stun >= shipped().shared.escape_min_window);
    for profile in PROFILES {
        for seed in SEEDS {
            let what = format!("{profile} seed {seed}");
            let played = play(config(profile, seed), stage(Some(stun), -1.0));
            assert_staged(&played, &what);
            assert!(stunned(&played), "{what}: the stun never landed");
            let t = transitions(&played);
            assert_eq!(
                names(&t),
                [("pressured", "escape"), ("escape", "pressured")],
                "{what}: {t:?}"
            );
            // Back to PRESSURED as the stun ends, with its hold after it.
            let back = t[1].0;
            assert!(
                (back - stun).abs() < 0.1,
                "{what}: back to PRESSURED at {back}s, the stun ran {stun}s"
            );
            assert!(back + hold <= WATCH, "the window covers the new hold");

            // Control: no stun, no change.
            let calm = play(config(profile, seed), stage(None, -1.0));
            assert_staged(&calm, &what);
            assert_eq!(transitions(&calm), [], "{what}: with no stun");
        }
    }
}

#[test]
fn a_stun_too_short_for_a_window_is_covered_by_the_pressured_hold() {
    let cfg = shipped().shared;
    let stun = 0.4;
    assert!(stun < cfg.escape_min_window && stun < cfg.pressured_hold);
    for profile in PROFILES {
        for seed in SEEDS {
            let what = format!("{profile} seed {seed}");
            let held = play(config(profile, seed), stage(Some(stun), cfg.pressured_hold));
            assert_staged(&held, &what);
            assert!(stunned(&held), "{what}: the stun never landed");
            assert_eq!(transitions(&held), [], "{what}: inside the hold");

            // Control: the hold already run out, the same stun lets the
            // healer go for the frames it lasts — once — and the threat's
            // return takes it straight back.
            let bare = play(config(profile, seed), stage(Some(stun), -1.0));
            let t = transitions(&bare);
            assert_eq!(
                names(&t),
                [("pressured", "free"), ("free", "pressured")],
                "{what}: {t:?}"
            );
        }
    }
}
