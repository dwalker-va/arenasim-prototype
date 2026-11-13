//! The PRESSURED anchor (R6), in a played match: a `Legacy` Priest under
//! attack does not leave heal range (40) of its teammate.
//!
//! The scorer masks every step that ends beyond heal range of the healer's
//! anchor ally, so a PRESSURED Priest retreats from its attacker AROUND that
//! ally, never away from it. This was a probe over a seed bank of the statue
//! comp (`movement_probes.rs`), but there the ally always stood near and the
//! windows were shorter than its 2.5s grace, so it could not fail. Here the
//! situation holds by construction (a staged match, tier 3): the ally is
//! stunned 34yd away, and the attacker stands on the ally's side of the
//! Priest, so every step of a plain retreat carries the Priest out of reach.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::{AbilityType, Aura, AuraType};
use common::staged::{play, sim_time, Played};

const ALLY: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const ATTACKER: Unit = Unit::new(2, 0);

/// The shipped `heal_range`.
const HEAL_RANGE: f32 = 40.0;
/// How long the Priest may stand out of heal range at a stretch: a step's
/// worth, for the scorer's lookahead against the per-frame position. Far
/// shorter than the PRESSURED time the stage holds (`MIN_PRESSURED`).
const GRACE: f32 = 0.25;
/// The PRESSURED time the stage must produce for the grace to be judged.
const MIN_PRESSURED: f32 = 3.0;
/// The retreat must carry the Priest to the edge of reach: the constraint
/// bound, not merely held.
const EDGE: f32 = HEAL_RANGE - 2.0;

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Priest".into()],
        team2: vec!["Warrior".into()],
        map: "BasicArena".into(),
        ai_profile: Some("Legacy".into()),
        team2_kill_target: Some(1),
        random_seed: Some(seed),
        max_duration_secs: 18.0,
        ..Default::default()
    }
}

/// The Priest at the arena's centre, its Warrior stunned 34yd west, and the
/// enemy Warrior at its west shoulder, on it: the retreat runs east, out of
/// reach. Psychic Scream is on cooldown, so the Priest cannot fear the
/// attacker off and leave PRESSURED.
fn stage() -> Stage {
    Stage::new()
        .place(ALLY, -34.0, 0.0)
        .aura(
            ALLY,
            Aura {
                effect_type: AuraType::Stun,
                duration: 60.0,
                magnitude: 1.0,
                ..Default::default()
            },
        )
        .place(PRIEST, 0.0, 0.0)
        .place(ATTACKER, -2.0, 0.0)
        .target(ATTACKER, PRIEST)
        .cooldown(PRIEST, AbilityType::PsychicScream, 60.0)
}

/// The Priest's PRESSURED windows on the app clock, from its posture
/// transitions; one still open ends at the match's last frame.
fn pressured_windows(played: &Played) -> Vec<(f32, f32)> {
    let landed = played.landed();
    let offset = landed.app - landed.combat;
    let end = played.frames.last().expect("frames").sim_time;
    let mut windows = Vec::new();
    let mut open = None;
    for e in played.staged_trace().filter(|e| {
        e["kind"] == "movement_decision" && e["actor"]["team"] == 1 && e["actor"]["slot"] == 1
    }) {
        match e["trigger"].as_str() {
            Some("PressuredEnter") if open.is_none() => open = Some(sim_time(e) + offset),
            Some("PressuredExit") => {
                if let Some(start) = open.take() {
                    windows.push((start, sim_time(e) + offset));
                }
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        windows.push((start, end));
    }
    windows
}

/// `(app time, Priest–ally distance on x/z)` for every staged frame both are
/// alive in.
fn ally_distance(played: &Played) -> Vec<(f32, f32)> {
    played
        .staged_frames()
        .filter_map(|f| {
            let of = |team: u8, slot: u8| {
                f.combatants
                    .values()
                    .find(|c| c.team == team && c.slot == slot && !c.is_pet && c.alive)
            };
            let (priest, ally) = (of(1, 1)?, of(1, 0)?);
            let d = (priest.position - ally.position).with_y(0.0).length();
            Some((f.sim_time, d))
        })
        .collect()
}

/// The longest stretch of consecutive samples out of heal range, in seconds.
fn longest_out_of_range(series: &[(f32, f32)]) -> f32 {
    let mut longest = 0.0f32;
    let mut run: Option<(f32, f32)> = None;
    for &(t, d) in series {
        if d > HEAL_RANGE {
            let start = run.map_or(t, |(s, _)| s);
            run = Some((start, t));
            longest = longest.max(t - start);
        } else {
            run = None;
        }
    }
    longest
}

/// Pushed east by an attacker on its west side, the PRESSURED Priest retreats
/// to the edge of its stunned teammate's heal range and no further: it is
/// never out of range for more than a step. Non-vacuity: the stage holds the
/// Priest PRESSURED for seconds, and the retreat reaches the edge, so it is
/// the constraint that stops it there.
#[test]
fn a_pressured_priest_stays_in_heal_range_of_its_teammate() {
    for seed in [1, 2, 3] {
        let played = play(config(seed), stage());
        let windows = pressured_windows(&played);
        let pressured: f32 = windows.iter().map(|(a, b)| b - a).sum();
        assert!(
            pressured >= MIN_PRESSURED,
            "seed {seed}: PRESSURED for only {pressured:.2}s ({windows:?}), \
             under the {MIN_PRESSURED}s the grace is judged over"
        );

        let distance = ally_distance(&played);
        let mut farthest = 0.0f32;
        for (start, stop) in &windows {
            let inside: Vec<(f32, f32)> = distance
                .iter()
                .copied()
                .filter(|(t, _)| t >= start && t <= stop)
                .collect();
            farthest = inside.iter().fold(farthest, |m, (_, d)| m.max(*d));
            let out = longest_out_of_range(&inside);
            eprintln!(
                "seed {seed}: PRESSURED [{start:.1},{stop:.1}]s, farthest {:.1}yd, \
                 longest out of range {out:.2}s",
                inside.iter().fold(0.0f32, |m, (_, d)| m.max(*d)),
            );
            assert!(
                out <= GRACE,
                "seed {seed}: the PRESSURED Priest stood out of heal range ({HEAL_RANGE}) \
                 of its teammate for {out:.2}s (grace {GRACE}s) in [{start:.1},{stop:.1}]"
            );
        }
        assert!(
            farthest >= EDGE,
            "seed {seed}: the retreat reached only {farthest:.1}yd from the teammate, \
             short of the {EDGE}yd edge: the anchor never bound"
        );
    }
}
