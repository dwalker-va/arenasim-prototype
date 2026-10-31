//! AS-241 — the Druid's Travel Form run keeps its room to run on Nagrand.
//!
//! A shifted Druid flees its chaser (`flee`, the AS-200 escape), straight away
//! from it. On Nagrand that line often points down one of the two starting
//! rooms — dead-end corridors past the bowl wall, whose back wall ends the
//! run. The AS-164 panel found a shifted Druid entering a room in about one
//! match in five. The run now never steps deeper into a room
//! (`movement_scoring::MASK_DEAD_END`, set by the chaser escape alone).
//!
//! STAGED matches (tier 3): the Druid is rooted with an enemy Warrior on it,
//! so it shifts out of the root on its next global cooldown and runs, and the
//! scene holds whatever the seed rolls. Each asserts the geometry the frames
//! show — how deep in a room the Druid ever stood, and the gap to its chaser —
//! and each is played over several seeds.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::ArenaMap;
use arenasim::states::play_match::arena_bounds::ArenaBounds;
use arenasim::states::play_match::components::{Aura, AuraType};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use bevy::prelude::*;
use common::staged::{play, Played};

const ALLY: Unit = Unit::new(1, 0);
const DRUID: Unit = Unit::new(1, 1);
const CHASER: Unit = Unit::new(2, 0);
const ENEMY_HEALER: Unit = Unit::new(2, 1);

/// The seeds each scene is played on. The scene is the stage's, not the
/// seed's; the seeds vary only the rolls.
const SEEDS: [u64; 3] = [1, 2, 3];

/// How long after the shift the run is followed.
const RUN_SECS: f32 = 3.0;

/// How far past a room's mouth a run that only GRAZES its corner reaches: the
/// mask judges a heading at two points of its run, so a run along the bowl
/// wall past the mouth may clip the corner by a fraction of a yard. Running
/// down the room is several yards.
const GRAZE_YARDS: f32 = 1.0;

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Druid".into()],
        team2: vec!["Warrior".into(), "Priest".into()],
        map: "PillaredArena".into(),
        // The enemy Warrior is after the Druid.
        team2_kill_target: Some(1),
        random_seed: Some(seed),
        max_duration_secs: 12.0,
        ..Default::default()
    }
}

fn bounds() -> ArenaBounds {
    load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(ArenaMap::PillaredArena)
        .bounds
}

/// The Druid rooted at `(x, z)` with the enemy Warrior at `chaser` on it; its
/// teammate and the enemy healer stand well clear.
fn stage(x: f32, z: f32, chaser: (f32, f32)) -> Stage {
    Stage::new()
        .place(DRUID, x, z)
        .clear_auras(DRUID)
        .aura(
            DRUID,
            Aura {
                effect_type: AuraType::Root,
                duration: 4.0,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        )
        .place(CHASER, chaser.0, chaser.1)
        .target(CHASER, DRUID)
        .place(ALLY, -30.0, 20.0)
        .place(ENEMY_HEALER, 30.0, -20.0)
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// The Druid's free run after it shifts: every frame from the shift until
/// [`RUN_SECS`] later, or until it dies, drops the form, or its chaser dies.
struct Run {
    frames: Vec<(f32, Vec3, Vec3)>,
}

fn run(played: &Played) -> Run {
    let druid = |f: &FrameObservation| {
        f.combatants
            .iter()
            .find(|(_, c)| c.team == 1 && c.slot == 1 && !c.is_pet)
            .map(|(_, c)| c.clone())
            .expect("the Druid")
    };
    let chaser = |f: &FrameObservation| {
        f.combatants
            .iter()
            .find(|(_, c)| c.team == 2 && c.slot == 0 && !c.is_pet)
            .map(|(_, c)| c.clone())
            .expect("the chaser")
    };
    let frames: Vec<&FrameObservation> = played.staged_frames().collect();
    let shift = frames
        .iter()
        .position(|f| druid(f).aura_types.contains(&AuraType::TravelForm))
        .expect("the rooted Druid shifts out of the root");
    let start = frames[shift].sim_time;
    Run {
        frames: frames[shift..]
            .iter()
            .take_while(|f| {
                let (d, c) = (druid(f), chaser(f));
                f.sim_time <= start + RUN_SECS
                    && d.alive
                    && c.alive
                    && d.aura_types.contains(&AuraType::TravelForm)
            })
            .map(|f| (f.sim_time - start, druid(f).position, chaser(f).position))
            .collect(),
    }
}

impl Run {
    fn deepest(&self, bounds: &ArenaBounds) -> f32 {
        self.frames
            .iter()
            .map(|(_, d, _)| bounds.dead_end_depth(d.x, d.z))
            .fold(0.0, f32::max)
    }

    fn gap_at(&self, secs: f32) -> f32 {
        let (_, d, c) = self
            .frames
            .iter()
            .rev()
            .find(|(t, _, _)| *t <= secs)
            .expect("a frame");
        xz(*d).distance(xz(*c))
    }
}

/// With its back to its own starting room's mouth — the Warrior on the bowl
/// side, so straight away from it is straight down the room — the shifted
/// Druid never sets foot in the room, and still pulls away from the Warrior.
#[test]
fn a_shifted_druid_does_not_run_down_a_starting_room() {
    let bounds = bounds();
    for seed in SEEDS {
        let played = play(config(seed), stage(-55.0, 0.0, (-50.0, 0.0)));
        let run = run(&played);
        assert!(
            run.frames.last().unwrap().0 >= 1.5,
            "seed {seed}: the run lasted only {:.2}s",
            run.frames.last().unwrap().0
        );
        let deepest = run.deepest(&bounds);
        assert!(
            deepest <= GRAZE_YARDS,
            "seed {seed}: the shifted Druid ran {deepest:.1}yd down the starting room"
        );
        let (start, after) = (run.gap_at(0.0), run.gap_at(1.5));
        assert!(
            after > start,
            "seed {seed}: no ground gained on the chaser ({start:.1} -> {after:.1}yd)"
        );
    }
}

/// Rooted inside the room with the Warrior at its mouth, the shifted Druid
/// never goes deeper than it stood: the back wall is no escape.
#[test]
fn a_shifted_druid_in_a_starting_room_never_runs_deeper() {
    let bounds = bounds();
    for seed in SEEDS {
        let played = play(config(seed), stage(-64.0, 3.0, (-60.0, 0.0)));
        let run = run(&played);
        let (_, first, _) = run.frames[0];
        let from = bounds.dead_end_depth(first.x, first.z);
        assert!(from > 0.0, "seed {seed}: the Druid shifts inside the room");
        let deepest = run.deepest(&bounds);
        assert!(
            deepest <= from,
            "seed {seed}: shifted {from:.1}yd down the room, it ran on to {deepest:.1}yd"
        );
    }
}
