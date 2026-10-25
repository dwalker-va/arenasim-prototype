//! AS-394 — the Hunt play in a played endgame (tier 3, staged).
//!
//! Nagrand (PillaredArena). The enemy is a lone Priest, staged on the east
//! face of the south-east pillar at (40, -20); our Mage and Priest stand
//! west of that pillar, out of its sight, and a root holds it there. The
//! match then plays on, and our team hunts it.
//!
//! - With the play, the team puts its Mage and Priest on opposite sides of
//!   the pillar: each walks to a station, and their bearings round its centre
//!   come 150° or more apart.
//! - With the play excepted everywhere (its control: it graduated and cannot
//!   be withheld), the hunt is still classified but nobody takes a station,
//!   and the two never split round the pillar.
//!
//! The play's sight gain needs a healer that moves round its pillar away from
//! its hunters. That was the TakeCover play's lone healer, deleted at the 0.8
//! sweep; no AI does it now, and a rooted healer cannot, so this test pins
//! the stations and not the sight. The situation holds by construction (a 2v1
//! from the gates); the seed governs only the rolls.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::class_ai::plays::every_exception_entry;
use arenasim::states::play_match::situation::PlayId;
use arenasim::states::play_match::{Aura, AuraType};
use bevy::prelude::*;
use common::staged::{play, Played};

const MAGE: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const HIDER: Unit = Unit::new(2, 0);

/// The south-east pillar's centre (`maps.ron`).
const PILLAR: Vec2 = Vec2::new(40.0, -20.0);

/// How long after the stage the endgame is read.
const WINDOW: f32 = 20.0;

/// The hunters must split within this many seconds of the stage: the walk to
/// the far station is about 30yd, a few seconds at base speed once the hunt
/// has begun.
const SPLIT_BY: f32 = 8.0;

fn config(seed: u64, hunt: bool) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Priest".into()],
        map: "PillaredArena".to_string(),
        ai_profile: Some("TeamPlan".into()),
        // Hunt graduated: its control excepts it everywhere on team 1.
        team1_exceptions: if hunt {
            Default::default()
        } else {
            [("Hunt".into(), every_exception_entry(PlayId::Hunt))].into()
        },
        random_seed: Some(seed),
        max_duration_secs: 40.0,
        ..Default::default()
    }
}

/// The lone Priest is held on the pillar's east face by a root that nothing
/// breaks: it hides there by construction. (No AI hides a lone healer at a
/// pillar since TakeCover was deleted at the 0.8 sweep; the hunt answers any
/// healer that stands there, and a root holds no healer from healing, so the
/// Hunt play still runs on it.)
/// The lone Priest is held on the pillar's east face by a root that nothing
/// breaks: it stands at the pillar by construction. (No AI hides a lone
/// healer at a pillar since TakeCover was deleted at the 0.8 sweep. A root
/// stops no heal, so the Hunt play still runs on it.)
fn stage() -> Stage {
    Stage::new()
        .place(HIDER, 47.0, -20.0)
        .aura(
            HIDER,
            Aura {
                effect_type: AuraType::Root,
                duration: 60.0,
                break_on_damage_threshold: -1.0,
                ability_name: "Entangling Roots".into(),
                ..Default::default()
            },
        )
        .aura(
            HIDER,
            Aura {
                effect_type: AuraType::Root,
                duration: 60.0,
                break_on_damage_threshold: -1.0,
                ability_name: "Entangling Roots".into(),
                ..Default::default()
            },
        )
        .place(MAGE, 16.0, -18.0)
        .target(MAGE, HIDER)
        .place(PRIEST, 12.0, -23.0)
        .target(PRIEST, HIDER)
        .target(HIDER, MAGE)
}

fn find(frame: &FrameObservation, who: Unit) -> &arenasim::headless::ObservedCombatant {
    frame
        .combatants
        .values()
        .find(|c| c.team == who.team && c.slot == who.slot && !c.is_pet)
        .expect("the unit")
}

fn bearing(p: Vec3) -> f32 {
    (Vec2::new(p.x, p.z) - PILLAR).to_angle()
}

fn gap(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d).to_degrees()
}

struct Read {
    /// When the Mage and the Priest first stood 150° apart round the pillar
    /// (seconds after the stage), if they did.
    split_at: Option<f32>,
    hunt_onsets: usize,
    /// Team 1's walks to a station (`MovementTrigger::Hunt` decisions).
    station_walks: usize,
}

fn read(played: &Played) -> Read {
    let start = played.landed().app;
    let mut split_at = None;
    for f in played.staged_frames() {
        let t = f.sim_time - start;
        if t > WINDOW {
            break;
        }
        let (mage, priest, hider) = (find(f, MAGE), find(f, PRIEST), find(f, HIDER));
        if !hider.alive {
            break;
        }
        if split_at.is_none()
            && mage.alive
            && priest.alive
            && gap(bearing(mage.position), bearing(priest.position)) >= 150.0
        {
            split_at = Some(t);
        }
    }
    let team1 = |e: &serde_json::Value| e["actor"]["team"] == 1 || e["team"] == 1;
    Read {
        split_at,
        hunt_onsets: played
            .staged_trace()
            .filter(|e| {
                e["kind"] == "situation_change"
                    && e["name"] == "hunt"
                    && e["change"] == "onset"
                    && e["team"] == 1
            })
            .count(),
        station_walks: played
            .staged_trace()
            .filter(|e| e["kind"] == "movement_decision" && e["trigger"] == "Hunt" && team1(e))
            .count(),
    }
}

#[test]
fn the_hunters_split_round_the_hiders_pillar() {
    for seed in [1, 2, 3] {
        let on = read(&play(config(seed, true), stage()));
        let off = read(&play(config(seed, false), stage()));
        println!(
            "seed {seed}: hunt split at {:?}, onsets {}, station walks {}; excepted split at {:?}, \
             onsets {}, station walks {}",
            on.split_at,
            on.hunt_onsets,
            on.station_walks,
            off.split_at,
            off.hunt_onsets,
            off.station_walks
        );
        assert!(on.hunt_onsets >= 1, "seed {seed}: the team never hunted");
        assert!(
            on.station_walks >= 2,
            "seed {seed}: the two never walked to stations"
        );
        let at = on
            .split_at
            .unwrap_or_else(|| panic!("seed {seed}: the hunters never split"));
        assert!(at < SPLIT_BY, "seed {seed}: split only at {at:.1}s");
        // The control: the hunt is still classified (the team runs the play),
        // but every class is excepted, so nobody takes a station.
        assert!(
            off.hunt_onsets >= 1,
            "seed {seed}: excepted, the hunt never began"
        );
        assert_eq!(
            off.station_walks, 0,
            "seed {seed}: excepted, yet walked a station"
        );
        assert_eq!(off.split_at, None, "seed {seed}: excepted, yet split");
    }
}
