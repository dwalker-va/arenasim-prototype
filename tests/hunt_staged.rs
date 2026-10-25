//! AS-394 — the Hunt play in a played endgame (tier 3, staged).
//!
//! Nagrand (PillaredArena). The enemy is a lone Priest, staged on the east
//! face of the south-east pillar at (40, -20); our Mage and Priest stand
//! west of that pillar, out of its sight. The match then plays on: the lone
//! Priest takes cover (`TakeCover`), and our team hunts it.
//!
//! - With the play, the team puts its Mage and Priest on opposite sides of
//!   the pillar: their bearings round its centre come 150° or more apart, and
//!   from then on the lone Priest is in a hunter's sight far more of the time
//!   than with the play withheld.
//! - Withheld, the Priest keeps its formation behind the Mage, so the two
//!   never split round the pillar.
//!
//! The situation holds by construction (a 2v1 from the gates); the seed
//! governs only the rolls.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::map_geometry::has_line_of_sight;
use bevy::prelude::*;
use common::staged::{play, Played};

const MAGE: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const HIDER: Unit = Unit::new(2, 0);

/// The south-east pillar's centre (`maps.ron`).
const PILLAR: Vec2 = Vec2::new(40.0, -20.0);

/// How long after the stage the endgame is read.
const WINDOW: f32 = 20.0;

fn config(seed: u64, hunt: bool) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Priest".into()],
        map: "PillaredArena".to_string(),
        ai_profile: Some("TeamPlan".into()),
        team1_withheld_plays: if hunt { vec![] } else { vec!["Hunt".into()] },
        random_seed: Some(seed),
        max_duration_secs: 40.0,
        ..Default::default()
    }
}

fn stage() -> Stage {
    Stage::new()
        .place(HIDER, 47.0, -20.0)
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
    /// The share of frames, from 6s after the stage, in which a hunter saw
    /// the lone Priest.
    seen: f32,
    hunt_onsets: usize,
}

fn read(played: &Played) -> Read {
    let obstacles = arenasim::states::play_match::map_config::load_map_geometry_config()
        .expect("maps.ron")
        .active_for(arenasim::ArenaMap::PillaredArena)
        .volumes;
    let start = played.landed().app;
    let mut split_at = None;
    let (mut frames, mut seen) = (0u32, 0u32);
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
        if t >= 6.0 {
            frames += 1;
            if [mage, priest]
                .iter()
                .any(|u| u.alive && has_line_of_sight(&obstacles, u.position, hider.position))
            {
                seen += 1;
            }
        }
    }
    Read {
        split_at,
        seen: seen as f32 / frames.max(1) as f32,
        hunt_onsets: played
            .staged_trace()
            .filter(|e| {
                e["kind"] == "situation_change"
                    && e["name"] == "hunt"
                    && e["change"] == "onset"
                    && e["team"] == 1
            })
            .count(),
    }
}

#[test]
fn the_hunters_split_round_the_hiders_pillar() {
    for seed in [1, 2, 3] {
        let on = read(&play(config(seed, true), stage()));
        let off = read(&play(config(seed, false), stage()));
        println!(
            "seed {seed}: hunt split at {:?}, seen {:.2}, onsets {}; withheld split at {:?}, seen {:.2}, onsets {}",
            on.split_at, on.seen, on.hunt_onsets, off.split_at, off.seen, off.hunt_onsets
        );
        assert!(on.hunt_onsets >= 1, "seed {seed}: the team never hunted");
        let at = on
            .split_at
            .unwrap_or_else(|| panic!("seed {seed}: the hunters never split"));
        assert!(at < 8.0, "seed {seed}: split only at {at:.1}s");
        assert_eq!(off.hunt_onsets, 0, "seed {seed}: withheld, yet hunted");
        assert_eq!(off.split_at, None, "seed {seed}: withheld, yet split");
        assert!(
            on.seen > off.seen + 0.1,
            "seed {seed}: seen {:.2} hunting against {:.2} withheld",
            on.seen,
            off.seen
        );
    }
}
