//! AS-440 — the hand-off cover rule in a played match (tier 3, staged).
//!
//! TwinPillars' west pillar stands at (-9, 0). The enemy Mage stands east of
//! it, shooting our Priest about 10yd off north-west of it, so the Priest is
//! PRESSURED. Our Warrior stands south-west of the pillar in the Mage's sight
//! and focus reach. The Mage and the Warrior are frozen in place, and the
//! Mage's Polymorph and Frost Nova are on cooldown. So no crowd control
//! reaches the Priest, and cover is the only thing its solve can buy against
//! the Mage. The situation holds by construction, whatever the seed.
//!
//! - With the rule on, the Priest stays in the Mage's sight: hiding would only
//!   hand the Mage's fire to the Warrior.
//! - With it off (`team1_handoff_rule: false`), the same Priest walks into the
//!   pillar's shadow.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::states::play_match::AbilityType;
use bevy::prelude::*;
use common::staged::{play, Played};

const FRAME: f32 = 1.0 / 60.0;
/// The window read after the stage lands: the Priest's opening Mind Blast,
/// then the walk.
const WINDOW: f32 = 5.0;

const WARRIOR: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const MAGE: Unit = Unit::new(2, 0);

fn config(seed: u64, rule: bool) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Priest".into()],
        team2: vec!["Mage".into()],
        map: "TwinPillars".to_string(),
        ai_profile: Some("TeamPlan".into()),
        team1_handoff_rule: Some(rule),
        random_seed: Some(seed),
        max_duration_secs: 8.0,
        ..Default::default()
    }
}

fn stage() -> Stage {
    let frozen = |c: &mut arenasim::states::play_match::Combatant| c.base_movement_speed = 0.0;
    Stage::new()
        .place(MAGE, 0.0, 3.0)
        .target(MAGE, PRIEST)
        .combatant(MAGE, frozen)
        .cooldown(MAGE, AbilityType::Polymorph, 60.0)
        .cooldown(MAGE, AbilityType::FrostNova, 60.0)
        .place(PRIEST, -10.0, 6.0)
        .place(WARRIOR, -15.0, -14.0)
        .target(WARRIOR, MAGE)
        .cooldown(WARRIOR, AbilityType::Charge, 60.0)
        .combatant(WARRIOR, frozen)
}

/// TwinPillars' west pillar, as `assets/config/maps.ron` declares it.
fn west_pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(-9.0, 0.0),
        radius: 2.5,
        base_y: 0.0,
        height: 5.0,
    }]
}

fn find(frame: &FrameObservation, who: Unit) -> &arenasim::headless::ObservedCombatant {
    frame
        .combatants
        .values()
        .find(|c| c.team == who.team && c.slot == who.slot && !c.is_pet)
        .expect("the unit")
}

fn eye(p: Vec3) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.z)
}

/// Seconds of the window the Priest stood hidden from the Mage.
fn hidden_secs(played: &Played) -> f32 {
    played
        .staged_frames()
        .take((WINDOW / FRAME) as usize)
        .filter(|f| {
            let (p, m) = (find(f, PRIEST), find(f, MAGE));
            p.alive && !has_line_of_sight(&west_pillar(), eye(m.position), eye(p.position))
        })
        .count() as f32
        * FRAME
}

/// Whether the Priest made a PRESSURED movement decision in the window.
fn pressured(played: &Played) -> bool {
    played.staged_trace().any(|e| {
        e["kind"] == "movement_decision"
            && e["actor"]["class"] == "Priest"
            && e["posture"] == "pressured"
    })
}

#[test]
fn the_geometry_is_twinpillars_west_pillar() {
    let geometry = arenasim::states::play_match::map_config::load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(arenasim::ArenaMap::TwinPillars)
        .volumes;
    assert!(
        geometry.contains(&west_pillar()[0]),
        "the staged pillar is not TwinPillars' west pillar: {geometry:?}"
    );
}

#[test]
fn a_pressured_priest_keeps_out_of_cover_while_its_warrior_is_exposed() {
    for seed in [1, 2] {
        let on = play(config(seed, true), stage());
        let off = play(config(seed, false), stage());
        assert!(
            pressured(&on) && pressured(&off),
            "seed {seed}: not PRESSURED"
        );
        let (h_on, h_off) = (hidden_secs(&on), hidden_secs(&off));
        assert!(
            h_on < 0.25,
            "seed {seed}: with the rule on the Priest hid {h_on:.2}s of {WINDOW}s"
        );
        assert!(
            h_off > 2.0,
            "seed {seed}: with the rule off the Priest hid only {h_off:.2}s of {WINDOW}s"
        );
    }
}
