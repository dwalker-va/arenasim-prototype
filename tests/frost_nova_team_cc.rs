//! Frost Nova never breaks its own team's crowd control (AS-486) — unless it
//! is the Mage's escape and the Mage is dying.
//!
//! The Nova damages every enemy inside it, so an enemy there that the team
//! holds in a hold any damage breaks (a Polymorph, a Freezing Trap) is freed by
//! it. `class_ai::mage` holds the Nova then (`nova_breaks_team_cc`), with ONE
//! exception (`nova_escape_outweighs_team_cc`): a non-pet enemy in melee range
//! of a dying Mage. Every scene below is a pair with its one condition removed.

use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Priest, Warlock, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::mage::NOVA_HOLDS_FOR_TEAM_CC;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{Aura, AuraType, PetType};
use arenasim::states::play_match::decision_trace::{PlayStep, RejectionReason};
use arenasim::states::play_match::movement_config::load_movement_config;
use arenasim::states::play_match::AbilityType::{FrostNova, IceBarrier, Polymorph};

const ME: &str = "mage";
const MELEE: &str = "enemy warrior";
const HELD: &str = "held enemy";
const ALLY: &str = "ally";
const HUNTER: &str = "hunter";
const ENEMY_HEALER: &str = "enemy priest";
const OWNER: &str = "enemy warlock";
const PET: &str = "enemy felhunter";

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

/// A hold any damage breaks, on whoever carries it, cast by `caster`.
fn hold(effect: AuraType, name: &str, caster: bevy::prelude::Entity) -> Aura {
    Aura {
        effect_type: effect,
        duration: 8.0,
        break_on_damage_threshold: 0.0,
        caster: Some(caster),
        ability_name: name.into(),
        ..Default::default()
    }
}

fn held_for_team_cc(view: &DecisionView) -> bool {
    matches!(
        view.rejected(FrostNova),
        Some(RejectionReason::PreconditionUnmet { note }) if note == NOVA_HOLDS_FOR_TEAM_CC
    )
}

// ---------------------------------------------------------------------------
// The rotation's Nova: a melee on the Mage
// ---------------------------------------------------------------------------

/// What holds the enemy Warlock in a rotation scene.
#[derive(Clone, Copy, Debug)]
enum Held {
    Nothing,
    /// The Mage's own Polymorph.
    MagesPolymorph,
    /// Its teammate Hunter's Freezing Trap.
    HuntersTrap,
}

/// The Mage at `health`, an enemy Warrior in melee range of it, its teammate
/// Hunter 20 yards back, and an enemy Warlock `held_at` yards off, held as
/// `held` says. Ice Barrier is cooling down and Frost Armor and Arcane
/// Intellect are up, so the rotation reaches its Nova step.
fn melee_scene(health: f32, held_at: f32, held: Held) -> Scene {
    let scene = Scene::new()
        .unit(ME, 1, Mage)
        .health(health)
        .aura(ME, AuraType::FrostArmorBuff, 600.0)
        .aura(ME, AuraType::MaxManaIncrease, 600.0)
        .cooldown(ME, IceBarrier, 20.0)
        .targeting(MELEE)
        .unit(HUNTER, 1, Hunter)
        .at(-20.0, 0.0)
        .aura(HUNTER, AuraType::MaxManaIncrease, 600.0)
        .targeting(MELEE)
        .unit(MELEE, 2, Warrior)
        .at(1.5, 0.0)
        .targeting(ME)
        .unit(HELD, 2, Warlock)
        .at(0.0, held_at);
    let (mage, hunter) = (scene.entity(ME), scene.entity(HUNTER));
    match held {
        Held::Nothing => scene,
        Held::MagesPolymorph => scene.aura_full(HELD, hold(AuraType::Polymorph, "Polymorph", mage)),
        Held::HuntersTrap => {
            scene.aura_full(HELD, hold(AuraType::Incapacitate, "Freezing Trap", hunter))
        }
    }
}

/// A healthy Mage holds its Nova while an enemy inside it is in the team's
/// breakable crowd control — its own Polymorph or its teammate's Freezing
/// Trap; with the enemy free the same Nova goes out.
#[test]
fn a_healthy_mage_holds_its_nova_for_the_teams_crowd_control_inside_it() {
    for held in [Held::MagesPolymorph, Held::HuntersTrap] {
        let view = melee_scene(0.9, 6.0, held).decide(ME);
        assert!(held_for_team_cc(&view), "{held:?}: {}", describe(&view));
        assert_ne!(
            view.chose(),
            Some((FrostNova, None)),
            "{held:?}: {}",
            describe(&view)
        );
    }
    let free = melee_scene(0.9, 6.0, Held::Nothing).decide(ME);
    assert_eq!(free.chose(), Some((FrostNova, None)), "{}", describe(&free));
}

/// The gate counts only enemies the Nova reaches (10 yards): a Polymorph 12
/// yards off is not broken, so the Nova goes out.
#[test]
fn a_polymorph_beyond_the_novas_reach_does_not_hold_it() {
    let beyond = melee_scene(0.9, 12.0, Held::MagesPolymorph).decide(ME);
    assert_eq!(
        beyond.chose(),
        Some((FrostNova, None)),
        "{}",
        describe(&beyond)
    );
}

/// Nor does a dead one: a Polymorph still on an enemy that has died holds
/// nothing.
#[test]
fn a_dead_polymorphed_enemy_does_not_hold_it() {
    let dead = melee_scene(0.9, 6.0, Held::MagesPolymorph)
        .health_of(HELD, 0.0)
        .decide(ME);
    assert_eq!(dead.chose(), Some((FrostNova, None)), "{}", describe(&dead));
}

/// The exception: a DYING Mage with a melee on it Novas anyway, through the
/// team's hold — the escape outweighs it. The same Mage healthy holds it (the
/// first test).
#[test]
fn a_dying_mage_novas_its_melee_off_through_the_teams_crowd_control() {
    for held in [Held::MagesPolymorph, Held::HuntersTrap] {
        let view = melee_scene(0.3, 6.0, held).decide(ME);
        assert_eq!(
            view.chose(),
            Some((FrostNova, None)),
            "{held:?}: {}",
            describe(&view)
        );
    }
}

/// "Dying" is the situation's `dying`: below `urgency_hp_threshold`. Just
/// under it the Mage Novas through the team's hold; just over it, it holds.
#[test]
fn the_exception_starts_at_the_urgency_threshold() {
    let threshold = load_movement_config()
        .expect("movement.ron loads")
        .shared
        .urgency_hp_threshold;
    for held in [Held::MagesPolymorph, Held::HuntersTrap] {
        let under = melee_scene(threshold - 0.05, 6.0, held).decide(ME);
        assert_eq!(
            under.chose(),
            Some((FrostNova, None)),
            "{held:?} at {}: {}",
            threshold - 0.05,
            describe(&under)
        );
        let over = melee_scene(threshold + 0.05, 6.0, held).decide(ME);
        assert!(
            held_for_team_cc(&over),
            "{held:?} at {}: {}",
            threshold + 0.05,
            describe(&over)
        );
    }
}

/// The escape is from a non-pet melee, the rotation's own trigger: a dying
/// Mage whose only free attacker at its feet is an enemy pet keeps the
/// team's Polymorph on the Warrior beside it. The control puts a second
/// Warrior where the pet stood, and the Mage Novas.
#[test]
fn an_enemy_pet_at_its_feet_is_no_escape() {
    let scene = |pet: bool| {
        let mut scene = Scene::new()
            .unit(ME, 1, Mage)
            .health(0.3)
            .aura(ME, AuraType::FrostArmorBuff, 600.0)
            .aura(ME, AuraType::MaxManaIncrease, 600.0)
            .cooldown(ME, IceBarrier, 20.0)
            .targeting(ENEMY_HEALER)
            .unit(MELEE, 2, Warrior)
            .at(1.5, 0.0)
            .unit(ENEMY_HEALER, 2, Priest)
            .at(0.0, 25.0)
            .unit(OWNER, 2, Warlock)
            .at(0.0, -25.0);
        scene = if pet {
            scene
                .pet(PET, OWNER, PetType::Felhunter)
                .at(0.0, 1.5)
                .targeting(ME)
        } else {
            scene.unit(PET, 2, Warrior).at(0.0, 1.5).targeting(ME)
        };
        let mage = scene.entity(ME);
        scene.aura_full(MELEE, hold(AuraType::Polymorph, "Polymorph", mage))
    };
    // Held only if the Felhunter's row reads `is_pet`: counted as a melee, it
    // would be the escape, as the control's Warrior is.
    let held = scene(true).decide(ME);
    assert!(held_for_team_cc(&held), "{}", describe(&held));
    assert_ne!(held.chose(), Some((FrostNova, None)), "{}", describe(&held));

    let free = scene(false).decide(ME);
    assert_eq!(free.chose(), Some((FrostNova, None)), "{}", describe(&free));
}

/// The escape is from a melee that can hit the Mage: a dying Mage whose only
/// melee is the enemy it holds in Polymorph, wandering at its feet, keeps the
/// Polymorph. With that enemy free (the control) the dying Mage Novas it.
#[test]
fn a_dying_mage_does_not_nova_a_held_enemy_at_its_feet() {
    let at_feet = |sheeped: bool| {
        let scene = Scene::new()
            .unit(ME, 1, Mage)
            .health(0.3)
            .aura(ME, AuraType::FrostArmorBuff, 600.0)
            .aura(ME, AuraType::MaxManaIncrease, 600.0)
            .cooldown(ME, IceBarrier, 20.0)
            .targeting(ENEMY_HEALER)
            .unit(MELEE, 2, Warrior)
            .at(1.5, 0.0)
            .unit(ENEMY_HEALER, 2, Priest)
            .at(0.0, 25.0);
        let mage = scene.entity(ME);
        if sheeped {
            scene.aura_full(MELEE, hold(AuraType::Polymorph, "Polymorph", mage))
        } else {
            scene.target(MELEE, ME)
        }
    };
    let held = at_feet(true).decide(ME);
    assert!(held_for_team_cc(&held), "{}", describe(&held));
    assert_ne!(held.chose(), Some((FrostNova, None)), "{}", describe(&held));

    let free = at_feet(false).decide(ME);
    assert_eq!(free.chose(), Some((FrostNova, None)), "{}", describe(&free));
}

// ---------------------------------------------------------------------------
// The Peel play's Nova: an attacker of a teammate in danger
// ---------------------------------------------------------------------------

/// The Peel scene (`tests/peel_scenes.rs`): the Mage's teammate at 30% with
/// an enemy Warrior on it, inside the Mage's Nova, under `TeamPlan` (2v2: the
/// Mage does not peel on a team of three). The Mage at `health`, and an enemy
/// Warlock 5 yards behind it — in the Mage's own Polymorph when `sheeped`.
fn peel_scene(health: f32, sheeped: bool) -> Scene {
    let scene = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ME, 1, Mage)
        .health(health)
        .targeting(ENEMY_HEALER)
        .unit(ALLY, 1, Priest)
        .at(3.0, 0.0)
        .health(0.3)
        .targeting(ENEMY_HEALER)
        .unit(MELEE, 2, Warrior)
        .at(4.5, 0.0)
        .targeting(ALLY)
        .unit(HELD, 2, Warlock)
        .at(-5.0, 0.0)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(0.0, 30.0);
    if sheeped {
        let mage = scene.entity(ME);
        scene.aura_full(HELD, hold(AuraType::Polymorph, "Polymorph", mage))
    } else {
        scene
    }
}

fn peel_step(view: &DecisionView) -> Option<PlayStep> {
    view.situation.as_ref().and_then(|s| s.play_step)
}

/// A peel Nova would free the Warlock the Mage holds in Polymorph, so the Nova
/// is held; with the Warlock free the Mage peels with the Nova.
#[test]
fn a_peel_nova_keeps_the_teams_polymorph() {
    let trapped = peel_scene(1.0, true).decide(ME);
    assert!(held_for_team_cc(&trapped), "{}", describe(&trapped));
    assert_eq!(
        trapped.chose(),
        Some((Polymorph, Some(MELEE))),
        "{}",
        describe(&trapped)
    );
    assert_eq!(peel_step(&trapped), Some(PlayStep::Peel));

    let open = peel_scene(1.0, false).decide(ME);
    assert_eq!(open.chose(), Some((FrostNova, None)), "{}", describe(&open));
    assert_eq!(peel_step(&open), Some(PlayStep::Peel));
}

/// The exception is the Mage's own ESCAPE, not its health alone: a dying Mage
/// with no melee on it still keeps the Polymorph when it peels for a teammate.
#[test]
fn a_dying_mage_with_no_melee_on_it_still_keeps_the_polymorph() {
    let view = peel_scene(0.4, true).decide(ME);
    let situation = view.situation.as_ref().expect("a situation");
    assert!(situation.ally_in_danger.is_some(), "{}", describe(&view));
    assert!(held_for_team_cc(&view), "{}", describe(&view));
    assert_ne!(view.chose(), Some((FrostNova, None)), "{}", describe(&view));
}
