//! The kiter's occlusion-timeout chase against a lone healer hugging a
//! pillar (`seek_chase_timeout`; tier 3, staged).
//!
//! The defect it guards: a ranged attacker could not catch a lone healer
//! tight against a thin pillar. The ENGAGE `range_band` pins the kiter to an
//! orbit, and `los_seek` gives no gradient when every candidate step is
//! occluded, so the kiter never closed. The chase arms once the occluded
//! stall has lasted `seek_chase_timeout` and walks straight at the target,
//! tangent-steered round the pillar, until sight returns.
//!
//! Migrated from the seed-bank probe `movement_probes::chase_los`, whose
//! hugger was the TakeCover play's lone healer: TakeCover was deleted at the
//! 0.8 milestone sweep and no AI hides a lone healer at a pillar any more, so
//! the bank no longer held a hug. Here the hug holds by construction. On
//! TwinPillars the lone Shaman is rooted (a root nothing breaks) against the
//! east pillar's east face, and the Mage stands west of it, in range and out
//! of sight. Every viewpoint the Mage's orbit steps reach is in the pillar's
//! shadow. The seed governs only the rolls.
//!
//! Asserted, per seed: the Mage starts occluded, and stays so long enough that
//! the chase had to arm; its longest occluded window is bounded; the Shaman
//! dies, so the endgame resolves; and the Mage never enters a pillar's
//! footprint while it rounds it.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::states::play_match::{Aura, AuraType};
use arenasim::ArenaMap;
use bevy::prelude::*;
use common::staged::{of_class, play, Played};

const MAGE: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const SHAMAN: Unit = Unit::new(2, 0);

/// The Mage's `seek_chase_timeout` (`movement.ron`): an occluded window
/// shorter than this never needed the chase.
const CHASE_TIMEOUT: f32 = 3.5;

/// The bound on the Mage's longest occluded window: the timeout, then a walk
/// of ~20yd round the pillar at base speed. A stall the chase does not end
/// runs to the cap.
const MAX_OCCLUDED: f32 = 15.0;

/// How long the staged endgame may run before the Shaman must be dead.
const CAP: f32 = 120.0;

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Shaman".into()],
        map: "TwinPillars".to_string(),
        random_seed: Some(seed),
        max_duration_secs: CAP,
        ..Default::default()
    }
}

/// TwinPillars' east pillar: centre (9, 0), radius 2.5 (`maps.ron`).
fn stage() -> Stage {
    Stage::new()
        .place(SHAMAN, 12.2, 0.0)
        .aura(
            SHAMAN,
            Aura {
                effect_type: AuraType::Root,
                duration: 300.0,
                break_on_damage_threshold: -1.0,
                ability_name: "Entangling Roots".into(),
                ..Default::default()
            },
        )
        .target(SHAMAN, MAGE)
        .place(MAGE, -3.0, 0.0)
        .target(MAGE, SHAMAN)
        .place(PRIEST, -4.0, 3.0)
        .target(PRIEST, SHAMAN)
}

fn sees(obstacles: &[ObstacleVolume], a: Vec3, b: Vec3) -> bool {
    has_line_of_sight(
        obstacles,
        Vec3::new(a.x, EYE_HEIGHT, a.z),
        Vec3::new(b.x, EYE_HEIGHT, b.z),
    )
}

struct Chase {
    /// Whether the Mage was out of the Shaman's sight on the staged frame.
    occluded_at_stage: bool,
    /// The Mage's longest continuous occluded window while the Shaman lived.
    longest: f32,
    /// When the Shaman died (seconds after the stage), if it did.
    shaman_death: Option<f32>,
    /// The Mage's closest approach to a pillar's centre.
    nearest_pillar: f32,
    /// The Mage's direct chase (a `SeekLos` walk with a `point` goal) was
    /// traced.
    chased: bool,
}

fn read(played: &Played, obstacles: &[ObstacleVolume]) -> Chase {
    let start = played.landed().app;
    let find = |f: &arenasim::headless::FrameObservation, who: Unit| {
        f.combatants
            .values()
            .find(|c| c.team == who.team && c.slot == who.slot && !c.is_pet)
            .cloned()
            .expect("the unit")
    };
    let mut occluded_at_stage = None;
    let (mut longest, mut run_start) = (0.0f32, None::<f32>);
    let mut shaman_death = None;
    let mut nearest_pillar = f32::INFINITY;
    for f in played.staged_frames() {
        let t = f.sim_time - start;
        let (mage, shaman) = (find(f, MAGE), find(f, SHAMAN));
        for v in obstacles {
            if let ObstacleVolume::Cylinder { center_xz, .. } = v {
                let d = Vec2::new(mage.position.x, mage.position.z).distance(*center_xz);
                nearest_pillar = nearest_pillar.min(d);
            }
        }
        if !shaman.alive {
            shaman_death = Some(t);
            break;
        }
        let occluded = !sees(obstacles, mage.position, shaman.position);
        occluded_at_stage.get_or_insert(occluded);
        if occluded {
            let s = *run_start.get_or_insert(t);
            longest = longest.max(t - s);
        } else {
            run_start = None;
        }
    }
    let chased = of_class(played.staged_trace(), "Mage").any(|e| {
        e["kind"] == "movement_decision" && e["trigger"] == "SeekLos" && e["goal_kind"] == "point"
    });
    Chase {
        occluded_at_stage: occluded_at_stage.unwrap_or(false),
        longest,
        shaman_death,
        nearest_pillar,
        chased,
    }
}

#[test]
fn the_chase_bounds_a_lone_healer_hugging_a_pillar() {
    let obstacles = load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(ArenaMap::TwinPillars)
        .volumes;
    for seed in [1, 2, 3] {
        let c = read(&play(config(seed), stage()), &obstacles);
        println!(
            "seed {seed}: occluded at stage {}, longest {:.2}s, chased {}, Shaman dead at {:?}, \
             nearest pillar {:.2}",
            c.occluded_at_stage, c.longest, c.chased, c.shaman_death, c.nearest_pillar
        );
        // Non-vacuity: the hug held long enough that only the chase ends it.
        assert!(
            c.occluded_at_stage,
            "seed {seed}: the Mage saw the Shaman at once"
        );
        assert!(
            c.longest >= CHASE_TIMEOUT,
            "seed {seed}: the Mage found sight in {:.2}s, before the chase could arm",
            c.longest
        );
        assert!(c.chased, "seed {seed}: no direct chase was traced");
        // The bound.
        assert!(
            c.longest <= MAX_OCCLUDED,
            "seed {seed}: the Mage stayed out of sight {:.2}s (> {MAX_OCCLUDED}s)",
            c.longest
        );
        assert!(
            c.shaman_death.is_some(),
            "seed {seed}: the lone Shaman was never caught"
        );
        // No-clip: the tangent arc keeps the Mage off the pillar's surface.
        assert!(
            c.nearest_pillar >= 2.5 - 0.01,
            "seed {seed}: the Mage came {:.3}yd from a pillar centre (radius 2.5)",
            c.nearest_pillar
        );
    }
}
