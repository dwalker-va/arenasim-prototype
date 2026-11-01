//! AS-245 — where the Druid's mana and global cooldowns go late in a match.
//!
//! - **Overheal in Attrition.** From the Attrition onset (dampening at half),
//!   the focus heals (Rejuvenation and Lifebloom kept on the attacked ally)
//!   skip an ally at full health (`DRUID_FULL_HP`). Before it, that pre-HoT is
//!   the design.
//! - **Full dampening.** Once dampening has run its ramp a heal heals nothing,
//!   so the Druid casts none — and does not leave Travel Form to cast one.
//!
//! SCENE tests (tier 1): one tick of the real class dispatch, each paired with
//! its control — the same scene with the one condition removed.

use arenasim::states::match_config::CharacterClass::{Druid, Priest, Warrior};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::druid::DRUID_FULL_HP;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{AuraPending, Shift};
use arenasim::states::play_match::constants::{DAMPENING_RAMP_SECS, DAMPENING_START_SECS};
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};
use arenasim::states::play_match::situation::{MatchPhase, ATTRITION_ONSET_SECS};
use arenasim::states::play_match::AbilityType;

const DRUID: &str = "druid";
const ALLY: &str = "ally warrior";
const ENEMY: &str = "enemy warrior";
const HEALER: &str = "enemy priest";

/// Twenty seconds after the gates: the Engaged phase.
const ENGAGED: f32 = 20.0;
/// Just past the Attrition onset, with dampening still short of full.
const ATTRITION: f32 = ATTRITION_ONSET_SECS + 1.0;
/// Seconds after the gates at which dampening is full.
const FULL_DAMPENING: f32 = DAMPENING_START_SECS + DAMPENING_RAMP_SECS;

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; shifts {:?}; candidates {:?}",
        view.chosen,
        view.shifts_queued,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// Whether `ability` was refused with a note containing `needle`.
fn refused(view: &DecisionView, ability: AbilityType, needle: &str) -> bool {
    view.candidates.iter().any(|c| {
        c.ability == ability
            && matches!(&c.reason, Some(RejectionReason::PreconditionUnmet { note }) if note.contains(needle))
    })
}

/// `caster`'s `ability` landed on `target`.
fn landed(scene: Scene, target: &'static str, caster: &'static str, ability: AbilityType) -> Scene {
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let aura = AuraPending::from_ability(
        scene.entity(target),
        scene.entity(caster),
        abilities.get_unchecked(&ability),
    )
    .expect("the ability applies an aura")
    .aura;
    scene.aura_full(target, aura)
}

/// Everyone on the Druid's team carries Mark of the Wild.
fn marked(scene: Scene) -> Scene {
    let scene = landed(scene, DRUID, DRUID, AbilityType::MarkOfTheWild);
    landed(scene, ALLY, DRUID, AbilityType::MarkOfTheWild)
}

/// A Druid with a full pool at `now`, its Warrior teammate at `ally_hp` with
/// an enemy Warrior in melee on it — the Druid's heal focus — and the enemy
/// Warrior its kill target. Innervate is down.
fn focus_scene(now: f32, ally_hp: f32) -> Scene {
    let scene = Scene::new()
        .now(now)
        .unit(DRUID, 1, Druid)
        .targeting(ENEMY)
        .cooldown(DRUID, AbilityType::Innervate, 60.0)
        .unit(ALLY, 1, Warrior)
        .at(10.0, 0.0)
        .health(ally_hp)
        .unit(ENEMY, 2, Warrior)
        .at(11.0, 0.0)
        .targeting(ALLY);
    marked(scene)
}

// ---------------------------------------------------------------------------
// Overheal in Attrition
// ---------------------------------------------------------------------------

/// In Attrition, a focus at full health gets no pre-HoT: the global cooldown
/// goes to Moonfire on the kill target.
#[test]
fn in_attrition_a_full_health_focus_is_not_pre_healed() {
    let view = focus_scene(ATTRITION, 1.0).decide(DRUID);
    assert_eq!(view.situation.map(|s| s.phase), Some(MatchPhase::Attrition));
    assert!(
        refused(&view, AbilityType::Rejuvenation, "attrition")
            && refused(&view, AbilityType::Lifebloom, "attrition"),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::Moonfire, Some(ENEMY))),
        "{}",
        describe(&view)
    );
}

/// Control: the same focus in the Engaged phase gets its pre-HoT.
#[test]
fn before_attrition_a_full_health_focus_is_pre_healed() {
    let view = focus_scene(ENGAGED, 1.0).decide(DRUID);
    assert_eq!(view.situation.map(|s| s.phase), Some(MatchPhase::Engaged));
    assert_eq!(
        view.chose(),
        Some((AbilityType::Rejuvenation, Some(ALLY))),
        "{}",
        describe(&view)
    );
}

/// Control: in Attrition, a focus below full health still gets its heals.
#[test]
fn in_attrition_a_hurt_focus_is_still_healed() {
    let view = focus_scene(ATTRITION, DRUID_FULL_HP - 0.05).decide(DRUID);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Rejuvenation, Some(ALLY))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Full dampening: no heal heals
// ---------------------------------------------------------------------------

/// Each of the Druid's heals, in the scene that chooses it: the emergency
/// Rejuvenation for a dying focus; Swiftmend once that focus carries the
/// Druid's Rejuvenation; Lifebloom for a hurt focus already rejuvenated.
fn heal_scenes(now: f32) -> [(AbilityType, Scene); 3] {
    let dying = focus_scene(now, 0.3);
    let rejuvenated = landed(
        focus_scene(now, 0.3),
        ALLY,
        DRUID,
        AbilityType::Rejuvenation,
    );
    let rolling = landed(
        focus_scene(now, 0.8),
        ALLY,
        DRUID,
        AbilityType::Rejuvenation,
    );
    [
        (AbilityType::Rejuvenation, dying),
        (AbilityType::Swiftmend, rejuvenated),
        (AbilityType::Lifebloom, rolling),
    ]
}

/// Under full dampening none of the heals is cast.
#[test]
fn under_full_dampening_the_druid_casts_no_heal() {
    for (heal, scene) in heal_scenes(FULL_DAMPENING + 1.0) {
        let view = scene.decide(DRUID);
        assert!(
            refused(&view, heal, "full dampening"),
            "{heal:?}: {}",
            describe(&view)
        );
        assert!(
            !matches!(
                view.chose(),
                Some((
                    AbilityType::Rejuvenation | AbilityType::Swiftmend | AbilityType::Lifebloom,
                    _
                ))
            ),
            "{heal:?}: {}",
            describe(&view)
        );
    }
}

/// Control: the same scenes a moment before dampening is full each cast
/// their heal on the focus.
#[test]
fn just_before_full_dampening_the_druid_still_heals() {
    for (heal, scene) in heal_scenes(FULL_DAMPENING - 1.0) {
        let view = scene.decide(DRUID);
        assert_eq!(
            view.chose(),
            Some((heal, Some(ALLY))),
            "{}",
            describe(&view)
        );
    }
}

/// A shifted Druid, `in_form` seconds into the form, at `hp`, its teammate
/// at `ally_hp` beside it, and an enemy Priest 50yd off — beyond Moonfire's
/// reach, and no chaser.
fn shifted_scene(now: f32, in_form: f32, hp: f32, ally_hp: f32) -> Scene {
    let scene = Scene::new()
        .now(now)
        .unit(DRUID, 1, Druid)
        .health(hp)
        .targeting(HEALER)
        .unit(ALLY, 1, Warrior)
        .at(5.0, 0.0)
        .health(ally_hp)
        .unit(HEALER, 2, Priest)
        .at(50.0, 0.0);
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let druid = scene.entity(DRUID);
    let mut form = AuraPending::from_ability(
        druid,
        druid,
        abilities.get_unchecked(&AbilityType::TravelForm),
    )
    .expect("Travel Form applies an aura")
    .aura;
    form.duration -= in_form;
    scene.aura_full(DRUID, form)
}

fn leaves(scene: &Scene) -> bool {
    let view = scene.decide(DRUID);
    view.shifts_queued == [Shift::Out]
}

/// A dying Druid leaves the form for its emergency heal — but not once
/// dampening is full, when the heal would heal nothing. Half a second into the
/// form, so only the emergency leave can fire.
#[test]
fn a_dying_druid_leaves_the_form_to_heal_only_while_heals_heal() {
    assert!(leaves(&shifted_scene(FULL_DAMPENING - 1.0, 0.5, 0.3, 1.0)));
    assert!(!leaves(&shifted_scene(FULL_DAMPENING + 1.0, 0.5, 0.3, 1.0)));
}

/// A safe Druid leaves the form to heal a hurt teammate — but not once
/// dampening is full. Five seconds into the form, healthy itself, with no
/// enemy in reach: the hurt teammate is its only work.
#[test]
fn a_safe_druid_leaves_the_form_for_a_hurt_teammate_only_while_heals_heal() {
    assert!(leaves(&shifted_scene(FULL_DAMPENING - 1.0, 5.0, 1.0, 0.6)));
    assert!(!leaves(&shifted_scene(FULL_DAMPENING + 1.0, 5.0, 1.0, 0.6)));
}

// ---------------------------------------------------------------------------
// The duel: the posture machine stands aside, but not for the run
// ---------------------------------------------------------------------------

/// A lone Druid in a duel, its duel-melee latch holding (30 mana, short of
/// two Moonfires), an enemy Warrior 6yd off attacking it; `shifted` puts it
/// half a second into Travel Form.
fn duel_scene(shifted: bool) -> Scene {
    let mut scene = Scene::new()
        .unit(DRUID, 1, Druid)
        .rage(30.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Warrior)
        .at(6.0, 0.0)
        .targeting(DRUID);
    scene.combatant_mut(DRUID).druid_duel_melee = true;
    if !shifted {
        return scene;
    }
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let druid = scene.entity(DRUID);
    let mut form = AuraPending::from_ability(
        druid,
        druid,
        abilities.get_unchecked(&AbilityType::TravelForm),
    )
    .expect("Travel Form applies an aura")
    .aura;
    form.duration -= 0.5;
    scene.aura_full(DRUID, form)
}

/// Closing to swing its staff, the Druid leaves its legs to the pursuit: the
/// posture machine decides no movement and leaves it no directive.
#[test]
fn a_druid_closing_in_a_duel_takes_no_posture_movement() {
    let view = duel_scene(false).decide(DRUID);
    assert!(view.movement.is_empty(), "{:?}", view.movement);
    assert!(view.directive.is_none(), "{:?}", view.directive);
}

/// Control: shifted, the same Druid runs — the posture machine's escape.
#[test]
fn a_shifted_druid_in_a_duel_still_runs() {
    let view = duel_scene(true).decide(DRUID);
    assert!(
        view.movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::ShiftEscape),
        "{:?}",
        view.movement
    );
}
