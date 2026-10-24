//! AS-373 — the healer pillar modes' key, one tick at a time (tier 1).
//!
//! A `TeamPlan` healer HUGS a pillar when its TEAM takes the pillar (AS-385,
//! `class_ai::pillar_stance`: its teammate fights at the pillar, an enemy
//! crowd control that needs sight is ready, and the pillar's ring has ground
//! that sees the teammate) and the ring's arc holds (`class_ai::pillar_mode`);
//! it plays the open otherwise. The stance's own keys and the no-lone-hug rule
//! are `tests/pillar_stance_scenes.rs`. Each scene is a pair — the situation, and its control
//! with one condition removed — so none can pass vacuously:
//!
//! - a Warlock with Fear ready gives hug (against the Warlock silenced, the
//!   modes turned off, `Legacy`, and the Paladin);
//! - in the open, AS-206's repair still keeps the composed FREE goal in a
//!   NON-anchor teammate's sight (against `Legacy`, whose goal stands in that
//!   teammate's shadow);
//! - the Shaman hugs (against the Warlock silenced); the Paladin, the one
//!   healer `HUG_EXCEPTIONS` names, never does;
//! - a Rogue alone gives open: its stuns are a melee's, and a pillar is no
//!   defence against a unit that walks round it;
//! - a wall between the teammate and every point of the ring gives open;
//! - a Fear whose reach covers the whole sighted ring gives open (no ground
//!   there is defended), against the same Fear ready but off the ring;
//! - a hug changing side turns through the sighted arc, never the shadow;
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
    HealerPosture, MovementGoal, PillarMemory, PillarMode, Posture, RingAngle,
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
    scene_with(healer, caster, mate_skin, (18.0, 2.0))
}

/// [`scene`] with the enemy caster at `caster_at`.
fn scene_with(
    healer: CharacterClass,
    caster: CharacterClass,
    mate_skin: f32,
    caster_at: (f32, f32),
) -> Scene {
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
        .at(caster_at.0, caster_at.1)
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

/// The defended arc: a hug is only worth ground the pillar hides from the
/// crowd control the hug is for. A Warlock north of the fight, its Fear ready,
/// sees the whole ring arc that sees the Warrior from within Fear's reach: no
/// ground there is defended, so the healer plays the open. The control moves
/// the same Warlock north beyond Fear's reach of any of that ring — its Fear
/// is still ready, so the crowd-control key still holds — and the healer hugs.
#[test]
fn a_ring_the_fear_covers_gives_open() {
    for healer in HEALERS {
        let covered = scene_with(healer, Warlock, 6.0, (0.0, 26.0)).decide(HEALER);
        assert!(!entered(&covered), "{healer:?}: {:?}", covered.movement);
        let off_ring = scene_with(healer, Warlock, 6.0, (0.0, 46.0)).decide(HEALER);
        assert!(entered(&off_ring), "{healer:?}: {:?}", off_ring.movement);
    }
}

/// The hug's turn goes only through the sighted arc. The Warrior stands 25yd
/// north of the pillar, so the ring's sighted arc runs from about -27 to 207
/// degrees and the shadow is the south side. A Warlock far to the north-east,
/// beyond Fear's reach of the ring, sees all of that arc but its far west end,
/// so the hug's target is there (about 175 degrees). The healer's ring angle
/// stands at -20 degrees: the shorter way to the target (south, 165 degrees)
/// crosses the shadow, the way through the arc (north, 195 degrees) does not.
/// After 0.6s of turning the goal must still see the Warrior, and must have
/// turned north. The team took the pillar while the Warrior fought nearer it
/// (the stance enters at `HUG_ENTER`), and still holds it at 19yd.
#[test]
fn the_hug_turns_through_the_arc_not_the_shadow() {
    let now = 30.0;
    for healer in HEALERS {
        let posture = HealerPosture {
            pillar: PillarMemory {
                mode: PillarMode::Hug { pillar: 0 },
                since: Some(now - 10.0),
                angle: Some(RingAngle {
                    theta: (-20.0f32).to_radians(),
                    at: now - 0.6,
                }),
                ..Default::default()
            },
            ..HealerPosture::new(0.0)
        };
        let mut s = scene_with(healer, Warlock, HUG_ENTER - 1.0, (31.8, 31.8))
            .now(now - 1.0)
            .healer_posture(HEALER, posture);
        assert!(
            s.reclassify().teams[0].pillar_stance.is_some(),
            "{healer:?}: the team never took the pillar"
        );
        s.set_now(now);
        s.set_position(MATE, 0.0, 25.0);
        s.set_position(BRUISER, 1.5, 25.0);
        let view = s.decide(HEALER);
        assert!(
            !left(&view),
            "{healer:?}: the hug ended: {:?}",
            view.movement
        );
        let at = goal(&view).expect("a point walk");
        let mate = Vec3::new(0.0, 1.0, 25.0);
        let deg = Vec2::new(at.x, at.z).to_angle().to_degrees();
        assert!(
            has_line_of_sight(&pillar(), eye(at), eye(mate)),
            "{healer:?}: the goal {at} ({deg:.0} degrees) is in the Warrior's shadow"
        );
        assert!(
            deg > -20.0 && deg < 90.0,
            "{healer:?}: the goal turned the wrong way, to {deg:.0} degrees"
        );
    }
}

/// The one healer `HUG_EXCEPTIONS` names, the Paladin (its FREE is melee
/// pursuit, design D5), never hugs, in FREE or PRESSURED; every other healer
/// does in the same scene. The list is pinned: an entry added or removed
/// fails here.
#[test]
fn the_excepted_healers_do_not_hug() {
    let excepted: Vec<CharacterClass> = HUG_EXCEPTIONS.iter().map(|(c, _)| *c).collect();
    assert_eq!(excepted, vec![Paladin]);
    for (class, posture) in [Paladin]
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
        // ...and for a Shaman: the caster-healer machine's other healer hugs
        // too (the Druid is in `HEALERS`).
        let shaman = held(Shaman).decide(HEALER);
        assert!(
            entered(&shaman),
            "Shaman {posture:?}: {:?}",
            shaman.movement
        );
    }
}

/// The open mode holds the HEADING out of the anchor's shadow only; another
/// teammate's shadow is AS-206's point repair's, which still runs on the
/// composed FREE goal (D7 as amended). A 3v3 round a small pillar at the
/// origin: the anchor (a Warrior at 40%) far to the west, a healthy Mage to
/// the north-east, and the engaged-ally centroid between them, so the
/// formation point behind the line falls behind the pillar from the Mage —
/// out of the anchor's shadow. No enemy has a crowd control a pillar defends
/// against (the Warriors' Charges are down), so the healer plays the open.
/// The goal it is sent to must see the Mage. The control is `Legacy`, which
/// runs neither the heading nor the repair: its goal stands in the Mage's
/// shadow, so the scene does put the point there.
#[test]
fn the_repair_answers_for_a_teammate_the_heading_does_not() {
    use arenasim::states::play_match::AbilityType;
    let small = vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::ZERO,
        radius: 2.0,
        base_y: 0.0,
        height: 10.0,
    }];
    let mage = Vec3::new(10.0, 1.0, 6.0);
    let anchor = Vec3::new(-18.0, 1.0, 5.2);
    let three_v_three = |healer: CharacterClass, profile: AiProfile| {
        Scene::new()
            .obstacles(small.clone())
            .ai_profile(1, profile)
            .ai_profile(2, profile)
            .unit(HEALER, 1, healer)
            .at(-4.0, -14.0)
            .unit(MATE, 1, Warrior)
            .at(anchor.x, anchor.z)
            .health(0.4)
            .targeting(BRUISER)
            .unit("mage", 1, CharacterClass::Mage)
            .at(mage.x, mage.z)
            .targeting(BRUISER)
            .unit(BRUISER, 2, Warrior)
            .at(-2.8, 10.5)
            .targeting(MATE)
            .cooldown(BRUISER, AbilityType::Charge, 60.0)
            .unit("far1", 2, Warrior)
            .at(30.0, -15.0)
            .cooldown("far1", AbilityType::Charge, 60.0)
            .unit("far2", 2, Warrior)
            .at(30.0, 15.0)
            .cooldown("far2", AbilityType::Charge, 60.0)
    };
    for healer in [Priest, Druid] {
        let sees_mage = |profile| {
            let view = three_v_three(healer, profile).decide(HEALER);
            assert!(!entered(&view), "{healer:?}: the scene hugged");
            let at = goal(&view).expect("a point walk");
            (has_line_of_sight(&small, eye(at), eye(mage)), at)
        };
        let (on, at) = sees_mage(AiProfile::TeamPlan);
        assert!(on, "{healer:?}: the open goal {at} is in the Mage's shadow");
        let (legacy, at) = sees_mage(AiProfile::Legacy);
        assert!(
            !legacy,
            "{healer:?}: the Legacy goal {at} sees the Mage — the scene no longer \
             puts the formation point in its shadow"
        );
    }
}

/// The Shaman hugs (user ruling, 2026-10-23: only the Paladin is excepted).
/// The control silences the Warlock, so no crowd control is ready.
#[test]
fn the_shaman_hugs() {
    let view = scene(Shaman, Warlock, 6.0).decide(HEALER);
    assert!(entered(&view), "{:?}", view.movement);
    let silenced = scene(Shaman, Warlock, 6.0).aura(CASTER, AuraType::Silence, 10.0);
    assert!(!entered(&silenced.decide(HEALER)));
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

/// The Schmitt pair, now the team stance's (AS-385): the hug enters with a
/// stance taken at `HUG_ENTER` from the skin and no farther; a stance held
/// keeps the hug out to `HUG_LEAVE`, and lets it go beyond it once the
/// stance's dwell has run.
#[test]
fn the_pillar_distance_is_a_schmitt_pair() {
    let between = 6.0 + (HUG_ENTER + HUG_LEAVE) / 2.0;
    for healer in HEALERS {
        assert!(entered(
            &scene(healer, Warlock, HUG_ENTER - 0.5).decide(HEALER)
        ));
        assert!(!entered(
            &scene(healer, Warlock, HUG_ENTER + 0.5).decide(HEALER)
        ));
        // The team took the pillar at 6yd; the fight then moves.
        let taken = |to: f32| {
            let mut s = scene(healer, Warlock, 6.0)
                .now(30.0)
                .healer_posture(HEALER, hugging(0.0));
            assert!(s.reclassify().teams[0].pillar_stance.is_some());
            s.set_now(30.0 + MODE_DWELL + 0.1);
            s.set_position(MATE, 0.0, to);
            s.set_position(BRUISER, 1.5, to);
            s.decide(HEALER)
        };
        assert!(!left(&taken(between)), "{healer:?}: left inside the band");
        assert!(
            left(&taken(6.0 + HUG_LEAVE + 1.0)),
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
