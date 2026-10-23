//! AS-373 — the healer pillar modes' key, one tick at a time (tier 1).
//!
//! A `TeamPlan` healer HUGS a pillar when its teammate fights at the pillar,
//! an enemy crowd control that needs sight is ready for it, and the pillar's
//! ring has ground that sees the teammate (`class_ai::pillar_mode`); it plays
//! the open otherwise. Each scene is a pair — the situation, and its control
//! with one condition removed — so none can pass vacuously:
//!
//! - a Warlock with Fear ready gives hug (against the Warlock silenced, the
//!   modes turned off, `Legacy`, and the Paladin);
//! - a Rogue alone gives open: its stuns are a melee's, and a pillar is no
//!   defence against a unit that walks round it;
//! - a wall between the teammate and every point of the ring gives open;
//! - the hug holds across the Schmitt band (entered at 12yd from the skin,
//!   left beyond 20yd) and through its dwell (left only after 2s).

use arenasim::states::match_config::CharacterClass;
use arenasim::states::match_config::CharacterClass::{
    Druid, Paladin, Priest, Rogue, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::pillar_mode::{
    HUG_ENTER, HUG_EXCEPTIONS, HUG_LEAVE, MODE_DWELL,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{
    HealerPosture, MovementGoal, PillarMemory, PillarMode, Posture,
};
use arenasim::states::play_match::decision_trace::MovementTrigger;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::states::play_match::AuraType;
use bevy::prelude::*;

const HEALER: &str = "healer";
const MATE: &str = "warrior";
const CASTER: &str = "caster";
const BRUISER: &str = "bruiser";

/// A Nagrand-sized pillar at the origin: footprint disc radius 6, ring at 8.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::ZERO,
        radius: 6.0,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// Our healer west of the pillar, our Warrior fighting an enemy Warrior north
/// of it (`mate_skin` yards from its skin), and an enemy `caster` standing off
/// east of it. Nobody targets the healer, so it is FREE.
fn scene(healer: CharacterClass, caster: CharacterClass, mate_skin: f32) -> Scene {
    let z = 6.0 + mate_skin;
    Scene::new()
        .obstacles(pillar())
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(HEALER, 1, healer)
        .at(-14.0, 0.0)
        .unit(MATE, 1, Warrior)
        .at(0.0, z)
        .targeting(BRUISER)
        .unit(BRUISER, 2, Warrior)
        .at(1.5, z)
        .targeting(MATE)
        .unit(CASTER, 2, caster)
        .at(18.0, 2.0)
        .targeting(MATE)
}

fn triggers(view: &DecisionView) -> Vec<MovementTrigger> {
    view.movement.iter().map(|m| m.trigger).collect()
}

fn entered(view: &DecisionView) -> bool {
    triggers(view).contains(&MovementTrigger::PillarModeEnter)
}

fn left(view: &DecisionView) -> bool {
    triggers(view).contains(&MovementTrigger::PillarModeLeave)
}

fn goal(view: &DecisionView) -> Option<Vec3> {
    view.directive.and_then(|d| match d.goal {
        MovementGoal::Point(p) => Some(p),
        _ => None,
    })
}

fn eye(p: Vec3) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.z)
}

/// The FREE healers that run the modes: the Priest's machine and the
/// caster-healer machine (the Druid's, shared with the Shaman).
const HEALERS: [CharacterClass; 2] = [Priest, Druid];

#[test]
fn a_ready_fear_at_a_pillar_fight_gives_hug() {
    for healer in HEALERS {
        let s = scene(healer, Warlock, 6.0);
        let view = s.decide(HEALER);
        assert!(entered(&view), "{healer:?}: {:?}", view.movement);
        // The goal stands on the ring and sees the Warrior.
        let at = goal(&view).expect("a point walk");
        let r = Vec2::new(at.x, at.z).length();
        assert!((r - 8.0).abs() < 0.6, "{healer:?}: goal {at} is {r}yd out");
        let mate = Vec3::new(0.0, 1.0, 12.0);
        assert!(
            has_line_of_sight(&pillar(), eye(at), eye(mate)),
            "{healer:?}: the ring goal {at} is out of the Warrior's sight"
        );

        // The controls, each with one condition removed.
        let silenced = scene(healer, Warlock, 6.0).aura(CASTER, AuraType::Silence, 10.0);
        assert!(
            !entered(&silenced.decide(HEALER)),
            "{healer:?}: silenced Warlock"
        );
        let off = scene(healer, Warlock, 6.0).without_pillar_modes(1);
        assert!(!entered(&off.decide(HEALER)), "{healer:?}: modes off");
        let legacy = scene(healer, Warlock, 6.0).ai_profile(1, AiProfile::Legacy);
        assert!(!entered(&legacy.decide(HEALER)), "{healer:?}: Legacy");
    }
}

/// The healers `HUG_EXCEPTIONS` names never hug — the Paladin, whose FREE is
/// melee pursuit (design D5), and the Shaman, on measurement — in FREE or
/// PRESSURED. The list is pinned: an entry added or removed fails here.
#[test]
fn the_excepted_healers_do_not_hug() {
    let excepted: Vec<CharacterClass> = HUG_EXCEPTIONS.iter().map(|(c, _)| *c).collect();
    assert_eq!(excepted, vec![Paladin, Shaman]);
    for (class, posture) in [Paladin, Shaman]
        .into_iter()
        .flat_map(|c| [(c, Posture::Free), (c, Posture::Pressured)])
    {
        let held = |class| {
            scene(class, Warlock, 6.0).healer_posture(
                HEALER,
                HealerPosture {
                    posture,
                    hold_until: 60.0,
                    ..HealerPosture::new(0.0)
                },
            )
        };
        let view = held(class).decide(HEALER);
        assert!(
            !entered(&view),
            "{class:?} {posture:?}: {:?}",
            view.movement
        );
        // The pair: the same scene hugs for a Priest.
        let priest = held(Priest).decide(HEALER);
        assert!(entered(&priest), "{posture:?}: {:?}", priest.movement);
    }
}

/// A Rogue's stuns need sight, but a Rogue walks round the pillar to land
/// them: no defence, so the healer plays the open.
#[test]
fn a_rogue_alone_gives_open() {
    for healer in HEALERS {
        let rogue = scene(healer, Rogue, 6.0);
        let view = rogue.decide(HEALER);
        assert!(!entered(&view), "{healer:?}: {:?}", view.movement);
        assert!(entered(&scene(healer, Warlock, 6.0).decide(HEALER)));
    }
}

/// A wall between the Warrior and the whole ring: no ground at the pillar
/// sees it, so hugging there could not keep it — the healer stays open.
#[test]
fn an_empty_ring_arc_gives_open() {
    let mut walled = pillar();
    walled.push(ObstacleVolume::Aabb {
        min: Vec3::new(-30.0, 0.0, 9.5),
        max: Vec3::new(30.0, 6.0, 10.5),
    });
    for healer in HEALERS {
        let view = scene(healer, Warlock, 6.0)
            .obstacles(walled.clone())
            .decide(HEALER);
        assert!(!entered(&view), "{healer:?}: {:?}", view.movement);
    }
}

fn hugging(since: f32) -> HealerPosture {
    HealerPosture {
        pillar: PillarMemory {
            mode: PillarMode::Hug { pillar: 0 },
            since: Some(since),
            ..Default::default()
        },
        ..HealerPosture::new(0.0)
    }
}

/// The Schmitt pair: open enters at `HUG_ENTER` from the skin and no
/// farther; a hug holds out to `HUG_LEAVE` and lets go beyond it.
#[test]
fn the_pillar_distance_is_a_schmitt_pair() {
    let between = (HUG_ENTER + HUG_LEAVE) / 2.0;
    for healer in HEALERS {
        assert!(entered(
            &scene(healer, Warlock, HUG_ENTER - 0.5).decide(HEALER)
        ));
        assert!(!entered(
            &scene(healer, Warlock, HUG_ENTER + 0.5).decide(HEALER)
        ));
        let held = scene(healer, Warlock, between).healer_posture(HEALER, hugging(0.0));
        assert!(
            !left(&held.decide(HEALER)),
            "{healer:?}: left inside the band"
        );
        let gone = scene(healer, Warlock, HUG_LEAVE + 1.0).healer_posture(HEALER, hugging(0.0));
        assert!(
            left(&gone.decide(HEALER)),
            "{healer:?}: held beyond the band"
        );
    }
}

/// The dwell: a hug whose keys have lapsed is held until `MODE_DWELL` has
/// run since it began, and let go once it has.
#[test]
fn a_hug_is_held_through_its_dwell() {
    for healer in HEALERS {
        let fresh = scene(healer, Warlock, HUG_LEAVE + 1.0)
            .now(30.0)
            .healer_posture(HEALER, hugging(30.0 - MODE_DWELL / 2.0));
        assert!(
            !left(&fresh.decide(HEALER)),
            "{healer:?}: left inside its dwell"
        );
        let dwelt = scene(healer, Warlock, HUG_LEAVE + 1.0)
            .now(30.0)
            .healer_posture(HEALER, hugging(30.0 - MODE_DWELL - 0.1));
        assert!(
            left(&dwelt.decide(HEALER)),
            "{healer:?}: held past its dwell"
        );
    }
}
