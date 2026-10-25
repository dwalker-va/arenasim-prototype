//! The play pipeline's controls, stage by stage (`class_ai::plays`).
//!
//! For each play, a scene in which that play — and no other — is the deciding
//! unit's, under `TeamPlan`. Then every measurement control, keyed by play
//! id, is shown reaching THAT play and only it, on its own team only:
//!
//! - withholding a GATED play from the unit's team takes it away; withholding
//!   it from the other team, or withholding any other gated play, does not. A
//!   graduated play cannot be withheld: excepting it everywhere for the unit's
//!   team (its control) takes it away, and doing so for the other team, or
//!   for any other play, does not;
//! - an exceptions table naming the unit (its class, and for Peel the class
//!   of the teammate it peels for) takes it away; one naming another class,
//!   or set on another play, does not;
//! - lifting the play's exceptions for the unit's team gives it back; lifting
//!   them for the other team, or lifting another play's, does not.
//!
//! So a stage whose exceptions the walk ignores, a withhold or a lift keyed to
//! the wrong play or the wrong team, fails here.

use arenasim::states::match_config::CharacterClass::{self, Mage, Priest, Rogue, Warlock, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::PlayTag;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::situation::{
    ClassSet, ExceptionKey, ExceptionSet, Play, PlayId, PlaySet, CHAIN, PEEL,
};
use arenasim::states::play_match::AuraType;
use bevy::prelude::Vec2;

const ME: &str = "me";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy";
const ENEMY_HEALER: &str = "enemy priest";

/// A stage's scene, and what makes it the deciding unit's.
struct Case {
    id: PlayId,
    /// `Play` of that kind, for the scene builder.
    play: Play,
    /// The deciding unit's class.
    class: CharacterClass,
    /// The exceptions table entry naming the deciding unit.
    me: ExceptionKey,
    /// An entry naming someone else.
    other: ExceptionKey,
    build: fn() -> Scene,
}

fn cases() -> Vec<Case> {
    vec![
        // A Mage, its Warrior teammate dying under an enemy Warrior.
        Case {
            id: PlayId::Peel,
            play: PEEL,
            class: Mage,
            me: ExceptionKey::in_context(Mage, Warrior),
            other: ExceptionKey::in_context(Mage, Rogue),
            build: || {
                let mut s = Scene::new()
                    .ai_profile(1, AiProfile::TeamPlan)
                    .unit(ME, 1, Mage)
                    .at(0.0, 0.0)
                    .targeting(ENEMY_HEALER)
                    .unit(ALLY, 1, Warrior)
                    .at(3.0, 0.0)
                    .health(0.3)
                    .targeting(ENEMY_HEALER)
                    .unit(ENEMY, 2, Warrior)
                    .at(4.5, 0.0)
                    .targeting(ALLY)
                    .unit(ENEMY_HEALER, 2, Priest)
                    .at(0.0, 30.0);
                s.combatant_mut(ME).stealthed = false;
                s
            },
        },
        // A Mage and its Priest, the enemy Priest alone and hidden behind a
        // pillar: the team hunts it, and the Mage takes a station.
        Case {
            id: PlayId::Hunt,
            play: Play::Hunt,
            class: Mage,
            me: ExceptionKey::class(Mage),
            other: ExceptionKey::class(Warlock),
            build: || {
                Scene::new()
                    .obstacles(vec![ObstacleVolume::Cylinder {
                        center_xz: Vec2::ZERO,
                        radius: 6.0,
                        base_y: 0.0,
                        height: 10.0,
                    }])
                    .ai_profile(1, AiProfile::TeamPlan)
                    .unit(ME, 1, Mage)
                    .at(-20.0, 0.0)
                    .targeting(ENEMY_HEALER)
                    .unit(ALLY, 1, Priest)
                    .at(-22.0, 4.0)
                    .unit(ENEMY_HEALER, 2, Priest)
                    .at(7.0, 0.0)
                    .targeting(ME)
            },
        },
        // A Priest, its Warrior fighting at a pillar with an enemy Warlock's
        // Fear ready: the team takes the pillar.
        Case {
            id: PlayId::Pillar,
            play: Play::Pillar,
            class: Priest,
            me: ExceptionKey::class(Priest),
            other: ExceptionKey::class(Mage),
            build: || {
                Scene::new()
                    .obstacles(vec![ObstacleVolume::Cylinder {
                        center_xz: Vec2::ZERO,
                        radius: 6.0,
                        base_y: 0.0,
                        height: 10.0,
                    }])
                    .ai_profile(1, AiProfile::TeamPlan)
                    .unit(ME, 1, Priest)
                    .at(-14.0, 0.0)
                    .unit(ALLY, 1, Warrior)
                    .at(0.0, 12.0)
                    .targeting(ENEMY)
                    .unit(ENEMY, 2, Warrior)
                    .at(1.5, 12.0)
                    .targeting(ALLY)
                    .unit(ENEMY_HEALER, 2, Warlock)
                    .at(18.0, 2.0)
                    .targeting(ALLY)
            },
        },
        // A Mage, the enemy Priest's stun ending within the horizon.
        Case {
            id: PlayId::Chain,
            play: CHAIN,
            class: Mage,
            me: ExceptionKey::class(Mage),
            other: ExceptionKey::class(Rogue),
            build: || {
                let mut s = Scene::new()
                    .ai_profile(1, AiProfile::TeamPlan)
                    .unit(ME, 1, Mage)
                    .at(0.0, 0.0)
                    .targeting(ENEMY)
                    .unit(ALLY, 1, Rogue)
                    .at(-5.0, 0.0)
                    .targeting(ENEMY)
                    .unit(ENEMY, 2, Warrior)
                    .at(-3.0, -20.0)
                    .targeting(ALLY)
                    .unit(ENEMY_HEALER, 2, Priest)
                    .at(0.0, 20.0)
                    .aura(ENEMY_HEALER, AuraType::Stun, 1.7);
                s.combatant_mut(ME).stealthed = false;
                s.combatant_mut(ALLY).stealthed = false;
                s
            },
        },
        // A Warrior, the enemy Mage low and its Priest stunned past the
        // Chain play's horizon: a kill window, and no lock ending.
        Case {
            id: PlayId::Burst,
            play: Play::Burst,
            class: Warrior,
            me: ExceptionKey::class(Warrior),
            other: ExceptionKey::class(Rogue),
            build: || {
                let mut s = Scene::new()
                    .ai_profile(1, AiProfile::TeamPlan)
                    .unit(ME, 1, Warrior)
                    .at(0.0, 0.0)
                    .targeting(ENEMY)
                    .unit(ALLY, 1, Priest)
                    .at(-10.0, 0.0)
                    .targeting(ENEMY)
                    .unit(ENEMY, 2, Mage)
                    .at(1.5, 0.0)
                    .health(0.2)
                    .targeting(ALLY)
                    .unit(ENEMY_HEALER, 2, Priest)
                    .at(0.0, 20.0)
                    .aura(ENEMY_HEALER, AuraType::Stun, 6.0);
                s.combatant_mut(ME).stealthed = false;
                s
            },
        },
    ]
}

/// The play the decision was taken under, as the trace tags it, by kind.
fn played(view: &DecisionView) -> Option<PlayId> {
    let situation = view.situation.as_ref().expect("a situation");
    situation.play.map(|tag| match tag {
        PlayTag::Peel { .. } => PlayId::Peel,
        PlayTag::Hunt => PlayId::Hunt,
        PlayTag::Pillar => PlayId::Pillar,
        PlayTag::Chain { .. } => PlayId::Chain,
        PlayTag::Burst => PlayId::Burst,
    })
}

fn kind(id: PlayId) -> Play {
    cases()
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| c.play)
        .unwrap()
}

#[test]
fn every_control_reaches_its_own_play_on_its_own_team() {
    let all = cases();
    assert_eq!(
        all.iter().map(|c| c.id).collect::<Vec<_>>(),
        PlayId::ALL.to_vec(),
        "a case per play"
    );
    for case in &all {
        let id = case.id;
        let decide = |s: Scene| played(&s.decide(ME));
        assert_eq!(decide((case.build)()), Some(id), "{id:?}: the scene's play");

        // Withholding: a GATED play only. A graduated play cannot be
        // withheld; its control is every exception, on its own team.
        if id.is_gated() {
            assert_ne!(
                decide((case.build)().withhold(1, PlaySet::of(&[case.play]))),
                Some(id),
                "{id:?} withheld from its team"
            );
            assert_eq!(
                decide((case.build)().withhold(2, PlaySet::of(&[case.play]))),
                Some(id),
                "{id:?} withheld from the other team"
            );
        } else {
            assert_ne!(
                decide((case.build)().except_everywhere(1, PlaySet::of(&[case.play]))),
                Some(id),
                "{id:?} excepted everywhere on its team"
            );
            assert_eq!(
                decide((case.build)().except_everywhere(2, PlaySet::of(&[case.play]))),
                Some(id),
                "{id:?} excepted everywhere on the other team"
            );
        }
        for other in PlayId::ALL.into_iter().filter(|o| *o != id) {
            let others = PlaySet::of(&[kind(other)]);
            if other.is_gated() {
                assert_eq!(
                    decide((case.build)().withhold(1, others)),
                    Some(id),
                    "{id:?} with {other:?} withheld"
                );
            }
            assert_eq!(
                decide((case.build)().except_everywhere(1, others)),
                Some(id),
                "{id:?} with {other:?} excepted everywhere"
            );
        }

        // Exceptions.
        let me = ExceptionSet::of(&[case.me]);
        assert_ne!(
            decide((case.build)().exceptions(case.play, me)),
            Some(id),
            "{id:?} excepted for {:?}",
            case.class
        );
        assert_eq!(
            decide((case.build)().exceptions(case.play, ExceptionSet::of(&[case.other]))),
            Some(id),
            "{id:?} excepted for another"
        );
        for other in PlayId::ALL.into_iter().filter(|o| *o != id) {
            assert_eq!(
                decide((case.build)().exceptions(kind(other), me)),
                Some(id),
                "{id:?} with {other:?}'s exceptions naming {:?}",
                case.class
            );
        }

        // Lifting.
        let excepted = || (case.build)().exceptions(case.play, me);
        assert_eq!(
            decide(excepted().lift(1, PlaySet::of(&[case.play]))),
            Some(id),
            "{id:?} lifted for its team"
        );
        assert_ne!(
            decide(excepted().lift(2, PlaySet::of(&[case.play]))),
            Some(id),
            "{id:?} lifted for the other team"
        );
        for other in PlayId::ALL.into_iter().filter(|o| *o != id) {
            assert_ne!(
                decide(excepted().lift(1, PlaySet::of(&[kind(other)]))),
                Some(id),
                "{id:?} with {other:?} lifted"
            );
        }
        // A class set is that class in any context: it excepts a play keyed
        // by class alone, never one keyed by context (Peel).
        let by_class = decide((case.build)().exceptions(case.play, ClassSet::of(&[case.class])));
        assert_eq!(
            by_class == Some(id),
            case.me.context.is_some(),
            "{id:?} excepted by a class set"
        );
    }
}

/// A unit with no peel tool is passed by the Peel stage even with a teammate
/// in danger: a Warrior beside its dying Priest, with an enemy Mage low and no
/// enemy healer (a kill window), runs the Burst play, not Peel. The scene
/// opens both windows, so the walk reached Peel and passed it.
#[test]
fn a_warrior_with_no_peel_tool_bursts_past_a_teammate_in_danger() {
    const ENEMY_MAGE: &str = "enemy mage";
    let mut s = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ME, 1, Warrior)
        .at(0.0, 0.0)
        .targeting(ENEMY_MAGE)
        .unit(ALLY, 1, Priest)
        .at(3.0, 0.0)
        .health(0.3)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Warrior)
        .at(4.5, 0.0)
        .targeting(ALLY)
        .unit(ENEMY_MAGE, 2, Mage)
        .at(1.5, 0.0)
        .health(0.2)
        .targeting(ME);
    s.combatant_mut(ME).stealthed = false;
    let view = s.decide(ME);
    let situation = view.situation.as_ref().expect("a situation");
    assert!(situation.ally_in_danger.is_some(), "no teammate in danger");
    assert!(situation.kill_window.is_some(), "no kill window");
    assert_eq!(played(&view), Some(PlayId::Burst));
}
