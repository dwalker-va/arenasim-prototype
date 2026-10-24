//! AS-388 — the Peel walk in time: a `TeamPlan` Priest walks at the Rogue on
//! its dying Warrior and Screams it, and the enemy Druid standing beside the
//! Rogue with it. Paired with the same stage and the Peel play withheld from
//! the Priest's team, where the Priest neither walks nor Screams.
//!
//! STAGED matches (tier 3, `headless::stage`) on BasicArena, Warrior+Priest
//! (`TeamPlan`) against Rogue+Druid (`Legacy`). On the frame the gates open:
//!
//! - the Priest at (-20, 0), its Holy school locked out (a kicked heal), so
//!   it cannot answer the danger with a heal and the walk is its answer. The
//!   Scream is Shadow, and is not locked. (A Priest that can heal its dying
//!   teammate in sight heals first: every heal on a dying teammate is
//!   critical, and fires through the walk's deferral);
//! - the Warrior at (10, 0) below `urgency_hp_threshold`, with the Rogue on
//!   it at (11.5, 0), out of stealth — 31.5yd from the Priest, beyond the
//!   Scream and inside the walk's reach;
//! - the Druid at (5, 3), held in place by a stun so the walk's arrival keeps
//!   it inside the Scream (and a stunned Druid's own crowd control is not
//!   ready, so it does not hold the walk either).
//!
//! The seeds govern only the rolls; the situation holds by construction.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::components::{Aura, AuraType};
use arenasim::states::play_match::SpellSchool;
use common::staged::{entity_id, of_class, play, sim_time, Played};

const PRIEST: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(1, 1);
const ROGUE: Unit = Unit::new(2, 0);
const DRUID: Unit = Unit::new(2, 1);

/// Below `urgency_hp_threshold` (0.5), so a melee on it puts it in danger,
/// and high enough that the Rogue does not kill it before the walk arrives.
const DYING: f32 = 0.45;

/// Seconds after the stage lands within which the walk must reach and Scream:
/// 23.5yd of walking at the Priest's 5.1yd/s is about 4.6s.
const WITHIN: f32 = 6.0;

/// The Holy lockout on the Priest: longer than [`WITHIN`].
const LOCKOUT: f32 = 8.0;

const SEEDS: [u64; 3] = [1, 2, 3];

fn config(seed: u64, withheld: bool) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Rogue".into(), "Druid".into()],
        map: "BasicArena".into(),
        team1_ai_profile: Some("TeamPlan".into()),
        team2_ai_profile: Some("Legacy".into()),
        team1_withheld_plays: if withheld {
            vec!["Peel".into()]
        } else {
            Vec::new()
        },
        random_seed: Some(seed),
        max_duration_secs: 12.0,
        ..Default::default()
    }
}

fn stage() -> Stage {
    Stage::new()
        .place(PRIEST, -20.0, 0.0)
        .place(WARRIOR, 10.0, 0.0)
        .health(WARRIOR, DYING)
        .target(WARRIOR, ROGUE)
        .place(ROGUE, 11.5, 0.0)
        .combatant(ROGUE, |c| c.stealthed = false)
        .target(ROGUE, WARRIOR)
        .aura(
            PRIEST,
            Aura {
                effect_type: AuraType::SpellSchoolLockout,
                magnitude: SpellSchool::Holy.to_lockout_magnitude(),
                duration: LOCKOUT,
                ..Default::default()
            },
        )
        .place(DRUID, 5.0, 3.0)
        .aura(
            DRUID,
            Aura {
                effect_type: AuraType::Stun,
                duration: 8.0,
                // The Priest's wand on its way in must not free it.
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        )
}

/// What the team-1 Priest did within [`WITHIN`] of the stage landing.
#[derive(Debug, Default)]
struct Response {
    /// The `PeelWalk` events, by the entity id they walked at.
    walks: Vec<u64>,
    /// Psychic Screams chosen by the Peel step.
    peel_screams: usize,
    /// Whom the Priest's Screams feared, from the combat log: (Rogue, Druid).
    feared: (bool, bool),
    /// DIP entries (the retired dip): none under `TeamPlan`.
    dips: usize,
}

fn respond(played: &Played) -> Response {
    let by = played.landed().combat + WITHIN;
    let priest: Vec<_> = of_class(played.staged_trace(), "Priest")
        .filter(|e| e["actor"]["team"] == 1 && sim_time(e) <= by)
        .collect();
    let walks = priest
        .iter()
        .filter(|e| e["kind"] == "movement_decision" && e["trigger"] == "PeelWalk")
        .filter_map(|e| e["target"]["entity_id"].as_u64())
        .collect();
    let dips = priest
        .iter()
        .filter(|e| e["kind"] == "movement_decision" && e["trigger"] == "DipEnter")
        .count();
    let peel_screams = priest
        .iter()
        .filter(|e| e["kind"] == "ability_decision")
        .filter(|e| e["outcome"]["ability"] == "PsychicScream")
        .filter(|e| e["situation"]["play_step"] == "peel")
        .count();
    // The combat log is stamped on the app's clock, countdown included.
    let log_by = played.landed().app + WITHIN;
    let scream_lines: Vec<&str> = played
        .log
        .lines()
        .filter(|l| l.contains("Team 1 Priest #1's Psychic Scream fears"))
        .filter(|l| log_time(l).is_some_and(|t| t <= log_by))
        .collect();
    let feared = (
        scream_lines.iter().any(|l| l.contains("Rogue")),
        scream_lines.iter().any(|l| l.contains("Druid")),
    );
    Response {
        walks,
        peel_screams,
        feared,
        dips,
    }
}

/// A combat-log line's stamp, `[ 14.62s]`, in seconds.
fn log_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line[open..].find("s]")? + open;
    line[open + 1..close].trim().parse().ok()
}

#[test]
fn the_priest_walks_and_screams_the_rogue_and_the_druid_beside_it() {
    for seed in SEEDS {
        let played = play(config(seed, false), stage());
        let rogue = entity_id(&played, 2, 0);
        let r = respond(&played);
        assert_eq!(r.dips, 0, "seed {seed}: {r:?}");
        assert!(
            !r.walks.is_empty() && r.walks.iter().all(|&w| w == rogue),
            "seed {seed}: the walk is not at the Rogue ({rogue}): {r:?}"
        );
        assert!(r.peel_screams >= 1, "seed {seed}: no peel Scream: {r:?}");
        assert_eq!(
            r.feared,
            (true, true),
            "seed {seed}: the Scream did not take both: {r:?}"
        );

        // The pair: the same stage, the Peel play withheld from team 1.
        let played = play(config(seed, true), stage());
        let r = respond(&played);
        assert!(r.walks.is_empty(), "seed {seed}, withheld: {r:?}");
        assert_eq!(r.peel_screams, 0, "seed {seed}, withheld: {r:?}");
        assert_eq!(r.dips, 0, "seed {seed}, withheld: {r:?}");
        assert_eq!(r.feared, (false, false), "seed {seed}, withheld: {r:?}");
    }
}
