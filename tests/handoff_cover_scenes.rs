//! AS-440 — the hand-off cover rule, one tick at a time (tier 1).
//!
//! A PRESSURED `TeamPlan` healer used to take cover from the enemy caster
//! shooting it. When that caster is a ranged DPS that can take the healer's
//! partner instead (`target_focus::could_turn_on_teammate`: in `FOCUS_REACH`
//! and in its sight), the team focus moves the fire onto the partner. So
//! cover from casters is waived then (`team_solve::SolveWorld::cover_waived`)
//! and the healer stays where it is. Crowd-control reach and sight of the
//! partner are unchanged.
//!
//! Each scene is a pair: the situation, and its control with one condition
//! removed. In the control the partner is beyond the Mage's focus reach or
//! out of its sight, the Mage follows no focus (a Druid), the Mage's team runs
//! no target focus (turned off, or `Legacy`), or the rule is turned off for
//! the healer's team; and the healer takes the pillar's shadow.

use arenasim::states::match_config::CharacterClass;
use arenasim::states::match_config::CharacterClass::{Druid, Mage, Priest, Shaman, Warrior};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::class_ai::target_focus::FOCUS_REACH;
use arenasim::states::play_match::components::MovementGoal;
use arenasim::states::play_match::decision_trace::Posture;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use bevy::prelude::*;

const HEALER: &str = "healer";
const MATE: &str = "warrior";
const CASTER: &str = "mage";

/// A pillar at the origin.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::ZERO,
        radius: 2.5,
        base_y: 0.0,
        height: 10.0,
    }]
}

/// The enemy Mage stands east of the pillar and shoots our healer, 10yd off
/// north-west of it (so the healer is PRESSURED). Its Polymorph is on
/// cooldown, so no crowd control reaches the healer and cover is the only
/// thing the solve can buy against it. Our Warrior stands south-west at
/// `mate`.
fn scene(healer: CharacterClass, mate: (f32, f32)) -> Scene {
    scene_against(healer, Mage, mate)
}

/// [`scene`] with `caster` in the Mage's place.
fn scene_against(healer: CharacterClass, caster: CharacterClass, mate: (f32, f32)) -> Scene {
    Scene::new()
        .obstacles(pillar())
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(HEALER, 1, healer)
        .at(-1.0, 6.0)
        .unit(MATE, 1, Warrior)
        .at(mate.0, mate.1)
        .unit(CASTER, 2, caster)
        .at(9.0, 3.0)
        .targeting(HEALER)
        .cooldown(CASTER, AbilityType::Polymorph, 60.0)
}

/// The partner in the Mage's sight and focus reach.
const EXPOSED: (f32, f32) = (-6.0, -14.0);
/// The partner in its sight but beyond its focus reach.
const BEYOND: (f32, f32) = (-14.0, -26.0);
/// The partner in its focus reach but in the pillar's shadow from it.
const SHADOWED: (f32, f32) = (-12.0, -5.0);

fn eye(p: Vec3) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.z)
}

/// Where the decision sends the healer: its point goal, or where it stands.
fn spot(scene: &Scene, view: &DecisionView) -> Vec3 {
    view.directive
        .and_then(|d| match d.goal {
            MovementGoal::Point(p) => Some(p),
            _ => None,
        })
        .unwrap_or_else(|| scene.position(HEALER))
}

fn hidden_from_caster(scene: &Scene, at: Vec3) -> bool {
    !has_line_of_sight(&pillar(), eye(scene.position(CASTER)), eye(at))
}

fn sees_partner(scene: &Scene, at: Vec3) -> bool {
    has_line_of_sight(&pillar(), eye(at), eye(scene.position(MATE)))
}

const HEALERS: [CharacterClass; 2] = [Priest, Shaman];

/// The geometry the scenes rest on, checked so no pair passes on a typo:
/// the healer starts in the Mage's sight and danger radius; the exposed
/// partner stands in the Mage's sight inside its focus reach, the far one in
/// its sight beyond it, the shadowed one inside its reach out of its sight;
/// all three in the healer's heal range.
#[test]
fn the_scene_geometry_is_what_the_pairs_assume() {
    let s = scene(Priest, EXPOSED);
    let (h, c) = (s.position(HEALER), s.position(CASTER));
    assert!(!hidden_from_caster(&s, h));
    assert!(h.distance(c) < 12.0);
    for (mate, seen, inside) in [
        (EXPOSED, true, true),
        (BEYOND, true, false),
        (SHADOWED, false, true),
    ] {
        let m = Vec3::new(mate.0, h.y, mate.1);
        assert_eq!(
            has_line_of_sight(&pillar(), eye(c), eye(m)),
            seen,
            "{mate:?}"
        );
        assert_eq!(m.distance(c) <= FOCUS_REACH, inside, "{mate:?}");
        assert!(m.distance(h) < 40.0, "{mate:?}");
    }
}

/// With the partner in the Mage's focus reach and sight the PRESSURED healer
/// does not walk into the pillar's shadow. With the partner beyond that reach,
/// or in the pillar's shadow from the Mage, the same healer takes the shadow
/// and keeps its partner in sight.
#[test]
fn an_exposed_partner_waives_the_healers_cover() {
    for healer in HEALERS {
        let s = scene(healer, EXPOSED);
        let view = s.decide(HEALER);
        assert!(
            view.movement
                .iter()
                .any(|m| m.posture == Posture::Pressured),
            "{healer:?}: not PRESSURED: {:?}",
            view.movement
        );
        let at = spot(&s, &view);
        assert!(
            !hidden_from_caster(&s, at),
            "{healer:?}: it hid at {at} although the Mage can take its partner"
        );

        for control in [BEYOND, SHADOWED] {
            let far = scene(healer, control);
            let view = far.decide(HEALER);
            let at = spot(&far, &view);
            assert!(
                hidden_from_caster(&far, at),
                "{healer:?}, partner at {control:?}: the control should take cover, went to {at}"
            );
            assert!(sees_partner(&far, at), "{healer:?}: cover at {at} is blind");
        }
    }
}

/// The rule asks for a unit that FOLLOWS a focus (`target_focus::follows_focus`,
/// a ranged DPS). An enemy Druid shooting the healer can see the exposed
/// partner too, but no team focus moves it, so the healer takes cover from it
/// as before.
#[test]
fn a_caster_that_follows_no_focus_leaves_the_cover_in_place() {
    for healer in HEALERS {
        let s = scene_against(healer, Druid, EXPOSED).cooldown(CASTER, AbilityType::Cyclone, 60.0);
        let view = s.decide(HEALER);
        assert!(
            view.movement
                .iter()
                .any(|m| m.posture == Posture::Pressured),
            "{healer:?}: not PRESSURED: {:?}",
            view.movement
        );
        let at = spot(&s, &view);
        assert!(
            hidden_from_caster(&s, at),
            "{healer:?}: an enemy Druid cannot move a focus, yet the healer stayed out at {at}"
        );
    }
}

/// The rule answers the ENEMY's target focus. Against a Mage whose team runs
/// one (the default `TeamPlan`), the healer stays out of the shadow
/// ([`an_exposed_partner_waives_the_healers_cover`]); against the same Mage
/// with its team's focus turned off, or on `Legacy` (which runs none), no
/// focus will move onto the partner, so the healer takes cover as before.
#[test]
fn an_enemy_without_a_focus_leaves_the_cover_in_place() {
    for healer in HEALERS {
        for (name, s) in [
            ("focus off", scene(healer, EXPOSED).without_target_focus(2)),
            (
                "Legacy",
                scene(healer, EXPOSED).ai_profile(2, AiProfile::Legacy),
            ),
        ] {
            let view = s.decide(HEALER);
            assert!(
                view.movement
                    .iter()
                    .any(|m| m.posture == Posture::Pressured),
                "{healer:?} vs {name}: not PRESSURED: {:?}",
                view.movement
            );
            let at = spot(&s, &view);
            assert!(
                hidden_from_caster(&s, at),
                "{healer:?} vs {name}: no focus can move, yet the healer stayed out at {at}"
            );
        }
    }
}

/// The rule's measurement switch: with the rule off for the team, the healer
/// with the exposed partner takes cover as before.
#[test]
fn with_the_rule_off_the_healer_takes_cover() {
    for healer in HEALERS {
        let off = scene(healer, EXPOSED).without_handoff_rule(1);
        let view = off.decide(HEALER);
        let at = spot(&off, &view);
        assert!(
            hidden_from_caster(&off, at),
            "{healer:?}: with the rule off it should take cover, went to {at}"
        );
    }
}
