//! AS-496 — an enemy held in crowd control is no pressure on a healer.
//!
//! User ruling (2026-11-14): "if they are cc'd they are not an immediate
//! threat." An enemy under a crowd control that stops it acting
//! (`class_ai::holds`: a stun, fear, polymorph, incapacitate or cyclone)
//! does not make its target PRESSURED, so it neither enters nor holds the
//! posture, puts no teammate in danger, and chases no Druid out of a snare.
//! A root is no hold: a rooted melee still pressures in range.
//!
//! The hold is still a WINDOW. A PRESSURED healer whose attacker is held
//! keeps its ESCAPE window on it and uses the hold to open distance, in every
//! healer posture machine (the Priest's, the Paladin's, and the caster-healer
//! machine the Shaman and Druid share).
//!
//! Every test is a pair: the held attacker, and the same attacker unheld (or
//! the one condition the rule reads removed).

use std::collections::BTreeSet;

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Paladin, Priest, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::holds;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::{MovementTrigger, Posture as TracePosture};
use arenasim::states::play_match::{AbilityType, AuraType, HealerPosture, Posture};

const HEALER: &str = "healer";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy";
const CASTER: &str = "caster";
const NOW: f32 = 30.0;

const PROFILES: [AiProfile; 2] = [AiProfile::Legacy, AiProfile::TeamPlan];

/// Every healer posture machine: the Priest's, the Paladin's, and the
/// caster-healer machine (Shaman, Druid).
const HEALERS: [CharacterClass; 4] = [Priest, Paladin, Shaman, Druid];

/// The crowd control that HOLDS, by name: the set the rule reads.
const HOLDS: [AuraType; 5] = [
    AuraType::Stun,
    AuraType::Fear,
    AuraType::Polymorph,
    AuraType::Incapacitate,
    AuraType::Cyclone,
];

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; movement {:?}; candidates {:?}",
        view.chosen,
        view.movement,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// The holds the tests below iterate are exactly the shared set: a hold
/// added to `class_ai::holds` must be added here, so it is tested too.
#[test]
fn the_holds_tested_are_the_shared_set() {
    let shared: BTreeSet<String> = AuraType::ALL
        .iter()
        .filter(|a| holds(**a))
        .map(|a| format!("{a:?}"))
        .collect();
    let tested: BTreeSet<String> = HOLDS.iter().map(|a| format!("{a:?}")).collect();
    assert_eq!(shared, tested);
}

/// A `healer` of `class` under `profile`, a Warrior teammate 10yd off, an
/// enemy Warrior 6yd off targeting `whom`, carrying `aura` (when given).
fn scene(
    profile: AiProfile,
    class: CharacterClass,
    whom: &'static str,
    aura: Option<AuraType>,
) -> Scene {
    let s = Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(HEALER, 1, class)
        .aura(HEALER, AuraType::MaxHealthIncrease, 600.0)
        .unit(ALLY, 1, Warrior)
        .at(0.0, -10.0)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .unit(ENEMY, 2, Warrior)
        .at(-6.0, 0.0)
        .targeting(whom);
    match aura {
        Some(effect) => s.aura(ENEMY, effect, 4.0),
        None => s,
    }
}

fn pressured_enter(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::PressuredEnter)
}

fn escape_open(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::EscapeWindowOpen)
}

/// (pressured, held attacker) for `who`, classified from the scene.
fn standing(scene: &mut Scene, who: &'static str) -> (bool, bool) {
    let entity = scene.entity(who);
    let team = scene.reclassify().for_team(1);
    (team.is_pressured(entity), team.has_held_attacker(entity))
}

/// A held enemy targeting the healer from 6yd does not pressure it: no
/// PRESSURED onset, and the situation names it a held attacker instead. The
/// control is the same enemy, unheld; a root is no hold, and pressures.
#[test]
fn a_held_attacker_does_not_pressure_a_healer() {
    for profile in PROFILES {
        for class in HEALERS {
            for hold in HOLDS {
                let mut held = scene(profile, class, HEALER, Some(hold));
                let view = held.decide(HEALER);
                assert!(
                    !pressured_enter(&view),
                    "{profile:?} {class:?} under {hold:?}: {}",
                    describe(&view)
                );
                assert_eq!(
                    standing(&mut held, HEALER),
                    (false, true),
                    "{profile:?} {class:?} under {hold:?}"
                );
            }
            for unheld in [None, Some(AuraType::Root)] {
                let mut free = scene(profile, class, HEALER, unheld);
                let view = free.decide(HEALER);
                assert!(
                    pressured_enter(&view),
                    "{profile:?} {class:?} with {unheld:?}: {}",
                    describe(&view)
                );
                assert_eq!(
                    standing(&mut free, HEALER),
                    (true, false),
                    "{profile:?} {class:?} with {unheld:?}"
                );
            }
        }
    }
}

/// A dying teammate whose only attacker is held is not in danger, so no
/// peel is owed; the same attacker unheld puts it in danger.
#[test]
fn a_held_attacker_puts_no_teammate_in_danger() {
    for profile in PROFILES {
        let danger = |aura| {
            let mut s = scene(profile, Priest, ALLY, aura);
            s.set_health(ALLY, 0.3);
            s.decide(HEALER)
                .situation
                .and_then(|s| s.ally_in_danger)
                .map(|w| w.subject)
        };
        let ally = scene(profile, Priest, ALLY, None).entity(ALLY).index();
        for hold in HOLDS {
            assert_eq!(danger(Some(hold)), None, "{profile:?} under {hold:?}");
        }
        assert_eq!(danger(None), Some(ally), "{profile:?} unheld");
    }
}

/// A healer already PRESSURED, its hold past.
fn pressured() -> HealerPosture {
    HealerPosture {
        posture: Posture::Pressured,
        hold_until: NOW - 1.0,
        ..HealerPosture::new(NOW - 3.0)
    }
}

/// The hold is still a window: a PRESSURED healer whose attacker is stunned
/// (or incapacitated, or cycloned — the holds an escape window reads) opens
/// its ESCAPE window, with its PRESSURED hold already run out, in every
/// healer machine.
///
/// Controls: the same stunned enemy beside the healer but on its TEAMMATE is
/// no attacker of the healer and opens nothing; the same enemy unheld keeps
/// it PRESSURED.
#[test]
fn a_held_attacker_opens_the_escape_window() {
    for profile in PROFILES {
        for class in HEALERS {
            let decide = |whom, aura| {
                scene(profile, class, whom, aura)
                    .healer_posture(HEALER, pressured())
                    .decide(HEALER)
            };
            for hold in [AuraType::Stun, AuraType::Incapacitate, AuraType::Cyclone] {
                let view = decide(HEALER, Some(hold));
                assert!(
                    escape_open(&view),
                    "{profile:?} {class:?} under {hold:?}: {}",
                    describe(&view)
                );
            }
            let elsewhere = decide(ALLY, Some(AuraType::Stun));
            assert!(
                !escape_open(&elsewhere),
                "{profile:?} {class:?}, stunned on the teammate: {}",
                describe(&elsewhere)
            );
            let free = decide(HEALER, None);
            assert!(
                !escape_open(&free)
                    && free
                        .movement
                        .iter()
                        .all(|m| m.posture != TracePosture::Free),
                "{profile:?} {class:?}, unheld: {}",
                describe(&free)
            );
        }
    }
}

/// The Paladin opens no escape window with no teammate to keep alive, held
/// attacker or not: its PRESSURED posture is for a team (the degenerate-case
/// gate), and a held attacker passes the same gate. The control is the
/// scene with its teammate.
#[test]
fn a_lone_paladin_opens_no_escape_window_on_a_held_attacker() {
    for profile in PROFILES {
        let lone = Scene::new()
            .now(NOW)
            .ai_profile(1, profile)
            .unit(HEALER, 1, Paladin)
            .aura(HEALER, AuraType::MaxHealthIncrease, 600.0)
            .unit(ENEMY, 2, Warrior)
            .at(-6.0, 0.0)
            .targeting(HEALER)
            .aura(ENEMY, AuraType::Stun, 4.0)
            .healer_posture(HEALER, pressured())
            .decide(HEALER);
        assert!(!escape_open(&lone), "{profile:?}: {}", describe(&lone));
        let teamed = scene(profile, Paladin, HEALER, Some(AuraType::Stun))
            .healer_posture(HEALER, pressured())
            .decide(HEALER);
        assert!(escape_open(&teamed), "{profile:?}: {}", describe(&teamed));
    }
}

/// A snared Druid pressured by a Warlock shifts only for a melee that can
/// chase it: a held Warrior on it is no chaser. The control is the same
/// Warrior unheld.
#[test]
fn a_held_melee_is_no_chaser_to_shift_from() {
    for profile in PROFILES {
        let decide = |aura: Option<AuraType>| {
            let s = Scene::new()
                .now(NOW)
                .ai_profile(1, profile)
                .unit(HEALER, 1, Druid)
                .aura(HEALER, AuraType::MaxHealthIncrease, 600.0)
                .aura(HEALER, AuraType::MovementSpeedSlow, 4.0)
                .unit(ALLY, 1, Warrior)
                .at(0.0, -10.0)
                .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
                .unit(CASTER, 2, Warlock)
                .at(9.0, 0.0)
                .targeting(HEALER)
                .unit(ENEMY, 2, Warrior)
                .at(-20.0, 0.0)
                .targeting(HEALER);
            match aura {
                Some(effect) => s.aura(ENEMY, effect, 4.0),
                None => s,
            }
            .decide(HEALER)
        };
        let shifted = |v: &DecisionView| {
            v.chose()
                .is_some_and(|(ability, _)| ability == AbilityType::TravelForm)
        };
        let held = decide(Some(AuraType::Stun));
        assert!(!shifted(&held), "{profile:?}: {}", describe(&held));
        let free = decide(None);
        assert!(shifted(&free), "{profile:?}: {}", describe(&free));
    }
}
