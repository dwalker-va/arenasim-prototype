//! Scene tests for the one "already held" rule every hard-CC chooser asks
//! (`CombatContext::held_in_hard_cc`): no stun, fear, polymorph, incapacitate
//! or cyclone is cast on an enemy its team already holds in one — except
//! inside the last `CC_CHAIN_WINDOW` of that hold, where a new one CHAINS onto
//! it. Each test runs the real class dispatch for one tick and is paired with
//! its control, the same scene with the hold removed or shortened.
//!
//! `tests/decision_properties.rs`'s `never_stacks_hard_cc_on_a_unit_its_team_holds`
//! guards the rule over the whole grid; these pin the edges the grid does not
//! walk: the chain window, a root not counting as a hold, and a Fear the team
//! is beating on not counting either (its damage is about to break it).

use arenasim::states::match_config::CharacterClass::{Priest, Rogue, Warlock, Warrior};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::class_ai::CC_CHAIN_WINDOW;
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::{AbilityType, Aura, AuraType};

const ROGUE: &str = "rogue";
const WARLOCK: &str = "warlock";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy warrior";
const HEALER: &str = "enemy priest";

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// `effect` on `who`, `secs` left, cast by `caster`, breaking on damage as
/// `abilities.ron` gives it: a Polymorph on any damage, a Fear at 100, a stun
/// or a root never.
fn held(
    scene: Scene,
    who: &'static str,
    caster: &'static str,
    effect: AuraType,
    secs: f32,
) -> Scene {
    let caster = scene.entity(caster);
    scene.aura_full(
        who,
        Aura {
            effect_type: effect,
            duration: secs,
            break_on_damage_threshold: match effect {
                AuraType::Polymorph => 0.0,
                AuraType::Fear => 100.0,
                _ => -1.0,
            },
            caster: Some(caster),
            ..Default::default()
        },
    )
}

// ---------------------------------------------------------------------------
// Rogue: Kidney Shot chains onto its own Cheap Shot only inside the window.
// ---------------------------------------------------------------------------

fn rogue_scene() -> Scene {
    let mut scene = Scene::new()
        .unit(ROGUE, 1, Rogue)
        .targeting(ENEMY)
        .unit(ALLY, 1, Warrior)
        .at(-6.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(1.0, 0.0)
        .targeting(ROGUE);
    scene.combatant_mut(ROGUE).stealthed = false;
    scene
}

#[test]
fn kidney_shot_chains_onto_its_own_cheap_shot_only_inside_the_chain_window() {
    let chain = held(
        rogue_scene(),
        ENEMY,
        ROGUE,
        AuraType::Stun,
        CC_CHAIN_WINDOW - 0.1,
    )
    .decide(ROGUE);
    assert_eq!(
        chain.chose(),
        Some((AbilityType::KidneyShot, Some(ENEMY))),
        "a Cheap Shot inside the chain window is chained onto: {}",
        describe(&chain)
    );

    let stacked = held(
        rogue_scene(),
        ENEMY,
        ROGUE,
        AuraType::Stun,
        CC_CHAIN_WINDOW + 1.5,
    )
    .decide(ROGUE);
    assert_ne!(
        stacked.chose().map(|(a, _)| a),
        Some(AbilityType::KidneyShot),
        "a Cheap Shot with time left is not stacked on: {}",
        describe(&stacked)
    );
}

#[test]
fn kidney_shot_waits_for_a_teammates_polymorph_but_not_for_a_root() {
    // The control: a rooted Warrior still swings, so it is stunned.
    let rooted = held(rogue_scene(), ENEMY, ALLY, AuraType::Root, 6.0).decide(ROGUE);
    assert_eq!(
        rooted.chose(),
        Some((AbilityType::KidneyShot, Some(ENEMY))),
        "a root is not a hold: {}",
        describe(&rooted)
    );

    let sheeped = held(rogue_scene(), ENEMY, ALLY, AuraType::Polymorph, 6.0).decide(ROGUE);
    assert_ne!(
        sheeped.chose().map(|(a, _)| a),
        Some(AbilityType::KidneyShot),
        "a teammate's Polymorph is a hold: {}",
        describe(&sheeped)
    );
}

// ---------------------------------------------------------------------------
// Warlock: the Priority-4 Fear on its CC target (the card's repro).
// ---------------------------------------------------------------------------

fn dot(name: &str, caster: bevy::prelude::Entity) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 15.0,
        magnitude: 10.0,
        tick_interval: 3.0,
        caster: Some(caster),
        ability_name: name.into(),
        ..Default::default()
    }
}

fn warlock_scene() -> Scene {
    let mut scene = Scene::new()
        .unit(WARLOCK, 1, Warlock)
        .health(0.3)
        .targeting(ENEMY)
        .unit(ALLY, 1, Warrior)
        .at(17.0, 4.0)
        .unit(ENEMY, 2, Warrior)
        .at(19.95, 0.0)
        .targeting(ALLY)
        .unit(HEALER, 2, Priest)
        .at(20.0, 10.0);
    let me = scene.entity(WARLOCK);
    let healer = scene.entity(HEALER);
    scene.combatant_mut(WARLOCK).cc_target = Some(healer);
    scene
        .aura_full(ENEMY, dot("Corruption", me))
        .aura_full(ENEMY, dot("Unstable Affliction", me))
        .aura_full(ENEMY, dot("Immolate", me))
}

#[test]
fn the_warlock_does_not_fear_a_healer_its_team_holds() {
    let free = warlock_scene().decide(WARLOCK);
    assert_eq!(
        free.chose(),
        Some((AbilityType::Fear, Some(HEALER))),
        "the control fears the free healer: {}",
        describe(&free)
    );

    let sheeped = held(warlock_scene(), HEALER, ALLY, AuraType::Polymorph, 6.0).decide(WARLOCK);
    assert_ne!(
        sheeped.chose(),
        Some((AbilityType::Fear, Some(HEALER))),
        "a polymorphed healer is not feared: {}",
        describe(&sheeped)
    );
    assert!(
        matches!(
            sheeped.rejected(AbilityType::Fear),
            Some(RejectionReason::TargetAlreadyCCd {
                cc_type: AuraType::Polymorph
            })
        ),
        "the refusal is traced as the hold: {}",
        describe(&sheeped)
    );
}

// ---------------------------------------------------------------------------
// A Fear the team is attacking through is no hold.
// ---------------------------------------------------------------------------

#[test]
fn a_fear_the_team_is_breaking_does_not_hold_but_one_left_alone_does() {
    // The Rogue's own target, feared by its Warlock: the Rogue's blows are
    // about to break the Fear, so the stun is the lock that lasts.
    let scene = rogue_scene().unit(WARLOCK, 1, Warlock).at(-20.0, 0.0);
    let beaten = held(scene, ENEMY, WARLOCK, AuraType::Fear, 6.0).decide(ROGUE);
    assert_eq!(
        beaten.chose(),
        Some((AbilityType::KidneyShot, Some(ENEMY))),
        "a Fear the Rogue is attacking through is not a hold: {}",
        describe(&beaten)
    );

    // The Warlock's CC target, feared by a teammate and attacked by nobody:
    // that Fear holds, and is not feared again.
    let left = held(warlock_scene(), HEALER, ALLY, AuraType::Fear, 6.0).decide(WARLOCK);
    assert!(
        matches!(
            left.rejected(AbilityType::Fear),
            Some(RejectionReason::TargetAlreadyCCd {
                cc_type: AuraType::Fear
            })
        ),
        "a Fear nobody is breaking holds: {}",
        describe(&left)
    );
}
