//! AS-394 — the Hunt play in a played endgame (tier 3, staged).
//!
//! Nagrand (PillaredArena). The enemy is a lone Priest, staged on the east
//! face of the south-east pillar at (40, -20); our Mage and Priest stand
//! west of that pillar, out of its sight, and a root holds it there. The
//! match then plays on, and our team hunts it.
//!
//! - With the play, the team puts its Mage and Priest on opposite sides of
//!   the pillar: each walks round its own side toward sight, and their
//!   bearings round its centre come 150° or more apart.
//! - With the play excepted everywhere (its control: it graduated and cannot
//!   be withheld), the hunt is still classified but nobody hunts,
//!   and the two never split round the pillar.
//! - A hunter's walk takes it off its healer posture's formation point
//!   (AS-469).
//!
//! The pincer's own geometry (nobody blind, opposite sides, the walk round
//! the back, the hunt held in the shadow) is pinned in
//! `tests/hunt_pincer_staged.rs`; the side assignment measured round the
//! pillar (AS-403) by `hunt::tests::units_on_one_side_take_the_station_on_their_own_hand`.
//!
//! The play's sight gain needs a healer that moves round its pillar away from
//! its hunters. That was the TakeCover play's lone healer, deleted at the 0.8
//! sweep; no AI does it now, and a rooted healer cannot, so this test pins
//! the split and not the sight. The situation holds by construction (a 2v1
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
/// breaks: it stands at the pillar by construction. (No AI hides a lone
/// healer at a pillar since TakeCover was deleted at the 0.8 sweep. A root
/// stops no heal, so the Hunt play still runs on it.)
fn stage() -> Stage {
    stage_at((16.0, -18.0), (12.0, -23.0))
}

/// [`stage`], with our Mage and our Priest placed at `mage` and `priest`
/// (x, z).
fn stage_at(mage: (f32, f32), priest: (f32, f32)) -> Stage {
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
        .place(MAGE, mage.0, mage.1)
        .target(MAGE, HIDER)
        .place(PRIEST, priest.0, priest.1)
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

/// A station walk takes the healer off its formation point, so the formation
/// memory does not outlive it (AS-469): our Priest is staged carrying a
/// formation heading, a kept exit and a reach repair's stay, and from its
/// first walk to a station its memory is empty. The control: with the play
/// excepted, the same staged Priest plays FREE and is still turning the
/// staged heading on that frame.
#[test]
fn a_station_walk_forgets_the_formation_memory() {
    use arenasim::states::play_match::class_ai::formation::ReachKept;
    use arenasim::states::play_match::{FormationHeading, FormationMemory, HealerPosture};
    let mut posture = HealerPosture::new(0.0);
    posture.formation = FormationMemory {
        heading: Some(FormationHeading {
            dir: Vec2::Y,
            at: 10.0,
        }),
        exit: Some(Vec2::new(10.0, -23.0)),
        reach: Some(ReachKept::Stay { blind_since: None }),
        seed_from_healer: false,
    };
    let staged = || stage().healer_posture(PRIEST, posture);
    let memory = |played: &Played, frame: usize| {
        find(&played.frames[frame], PRIEST)
            .formation
            .expect("the Priest has a healer posture")
    };
    for seed in [1, 2, 3] {
        let on = play(config(seed, true), staged());
        let walk = on
            .staged_trace()
            .find(|e| {
                e["kind"] == "movement_decision"
                    && e["trigger"] == "Hunt"
                    && e["actor"]["team"] == 1
                    && e["actor"]["class"] == "Priest"
            })
            .and_then(|e| e["frame"].as_u64())
            .unwrap_or_else(|| panic!("seed {seed}: the Priest never walked to a station"))
            as usize;
        // The observation a trace frame names is taken as that frame
        // begins; the walk's own frame ends on the next one.
        assert_eq!(
            memory(&on, walk + 1),
            FormationMemory::default(),
            "seed {seed}: the formation memory outlived the station walk"
        );
        // The staged heading points north (+z), across the "behind the line"
        // direction (west, away from the hider): only the staged memory,
        // turned for a frame, still points north.
        let off = play(config(seed, false), staged());
        let heading = memory(&off, walk + 1).heading.expect("a FREE heading");
        assert!(
            heading.dir.dot(Vec2::Y) > 0.9,
            "seed {seed}: excepted, the staged heading never landed ({heading:?})"
        );
    }
}
