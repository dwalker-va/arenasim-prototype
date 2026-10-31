//! Scene tests for the Warlock's Life Tap (AS-296): it converts health into
//! mana when it NEEDS the mana (below `LIFE_TAP_STARVED_FRACTION` of its
//! pool) and is HEALTHY — the tap leaves it at or above
//! `urgency_hp_threshold` (`class_ai::warlock::life_tap_hold`). Those are the
//! only two conditions (user ruling, 2026-10-30): it taps under both AI
//! profiles, with or without a healer, targeted or not. Each gate is pinned
//! as a pair: the scene that taps, and the same scene with one condition
//! changed, held with that gate's typed reason.
//!
//! Every scene runs the real class dispatch for one tick, so a pass also
//! proves the dispatch reaches the tap — and, since the starved Warlock still
//! has the mana for Corruption on an enemy in range, that the tap comes
//! before the rotation.

use arenasim::states::match_config::CharacterClass::{Mage, Priest, Warlock, Warrior};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::class_ai::warlock::{
    convert_health_to_mana, LIFE_TAP_STARVED_FRACTION,
};
use arenasim::states::play_match::components::Combatant;
use arenasim::states::play_match::decision_trace::{RejectionReason, TapHold};
use arenasim::states::play_match::movement_config::load_movement_config;
use arenasim::states::play_match::{AbilityType, AuraType};

const WARLOCK: &str = "warlock";
const PRIEST: &str = "priest";
const ENEMY: &str = "enemy warrior";

/// A starved Warlock (a fifth of its mana: still enough for Corruption) at
/// full health, its Priest 10yd behind it, and an enemy Warrior 20yd off —
/// in Corruption's range, and after the Priest.
fn scene() -> Scene {
    Scene::new()
        .unit(WARLOCK, 1, Warlock)
        .mana(0.2)
        .targeting(ENEMY)
        .unit(PRIEST, 1, Priest)
        .at(-10.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(20.0, 0.0)
        .targeting(PRIEST)
}

/// [`scene`] with a Mage in the Priest's place: no healer on the team.
fn healerless() -> Scene {
    Scene::new()
        .unit(WARLOCK, 1, Warlock)
        .mana(0.2)
        .targeting(ENEMY)
        .unit("mage", 1, Mage)
        .at(-10.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(20.0, 0.0)
        .targeting("mage")
}

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

fn assert_taps(scene: &Scene) {
    let view = scene.decide(WARLOCK);
    assert_eq!(
        view.chosen,
        Some((AbilityType::LifeTap, None)),
        "the Warlock should Life Tap: {}",
        describe(&view)
    );
}

/// The decision held its tap, and why.
fn held(scene: &Scene) -> TapHold {
    let view = scene.decide(WARLOCK);
    assert_ne!(
        view.chosen.map(|(a, _)| a),
        Some(AbilityType::LifeTap),
        "the Warlock should hold its Life Tap: {}",
        describe(&view)
    );
    match view.rejected(AbilityType::LifeTap) {
        Some(RejectionReason::TapHeld { hold }) => *hold,
        other => panic!(
            "Life Tap should be traced TapHeld, got {other:?}: {}",
            describe(&view)
        ),
    }
}

/// What a tap converts for a Warlock at base stats (a scene's units wear no
/// gear), the health below which it is dying, and its maximum health.
fn amount_and_floor() -> (f32, f32, f32) {
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let movement = load_movement_config().expect("movement.ron loads");
    let warlock = Combatant::new(1, 0, Warlock);
    let amount = abilities
        .get_unchecked(&AbilityType::LifeTap)
        .health_to_mana
        .expect("Life Tap converts health into mana")
        .amount(warlock.spell_power);
    let floor = warlock.max_health * movement.shared.urgency_hp_threshold;
    (amount, floor, warlock.max_health)
}

#[test]
fn a_starved_healthy_warlock_taps_before_its_rotation() {
    let scene = scene();
    assert_taps(&scene);
    // The tap took the turn the rotation would have had.
    let view = scene
        .mana_of(WARLOCK, LIFE_TAP_STARVED_FRACTION)
        .decide(WARLOCK);
    assert_eq!(
        view.chosen,
        Some((AbilityType::Corruption, Some(ENEMY))),
        "without the tap, Corruption is what the Warlock casts: {}",
        describe(&view)
    );
}

#[test]
fn not_starved_holds_the_tap() {
    let at = scene().mana_of(WARLOCK, LIFE_TAP_STARVED_FRACTION);
    match held(&at) {
        TapHold::NotStarved {
            mana,
            starved_below,
        } => assert_eq!(mana, starved_below),
        other => panic!("expected NotStarved, got {other:?}"),
    }
    assert_taps(&scene().mana_of(WARLOCK, LIFE_TAP_STARVED_FRACTION - 0.01));
}

#[test]
fn a_tap_into_death_range_is_held() {
    let (amount, floor, max) = amount_and_floor();
    let short = scene().health_of(WARLOCK, (floor + amount - 1.0) / max);
    match held(&short) {
        TapHold::DeathRange {
            health_after,
            floor: f,
        } => {
            assert_eq!(f, floor);
            assert!(health_after < floor);
        }
        other => panic!("expected DeathRange, got {other:?}"),
    }
    assert_taps(&scene().health_of(WARLOCK, (floor + amount + 1.0) / max));
}

/// The gates are asked in order, so the trace names the first that holds:
/// a full-mana Warlock in death range is "not starved".
#[test]
fn the_first_gate_that_holds_is_the_one_traced() {
    let (_, floor, max) = amount_and_floor();
    let both = scene()
        .mana_of(WARLOCK, 1.0)
        .health_of(WARLOCK, floor / max);
    assert!(matches!(held(&both), TapHold::NotStarved { .. }));
}

// ---------------------------------------------------------------------------
// The ruling's "only two conditions": no healer, focus or profile gate.
// ---------------------------------------------------------------------------

/// A Warlock with no healer on its team taps — and is still held by its
/// health, so the healerless pair is the death-range pair again.
#[test]
fn a_healerless_warlock_taps_when_healthy() {
    assert_taps(&healerless());
    let (amount, floor, max) = amount_and_floor();
    let short = healerless().health_of(WARLOCK, (floor + amount - 1.0) / max);
    assert!(matches!(held(&short), TapHold::DeathRange { .. }));
}

/// Whether its healer can heal it changes nothing: dead, stunned, out of
/// mana, out of reach.
#[test]
fn its_healers_state_does_not_hold_the_tap() {
    assert_taps(&scene().health_of(PRIEST, 0.0));
    assert_taps(&scene().aura(PRIEST, AuraType::Stun, 4.0));
    assert_taps(&scene().mana_of(PRIEST, 0.0));
    assert_taps(&scene().place(PRIEST, -60.0, 0.0));
}

/// An enemy on the Warlock does not hold the tap: health is the gate.
#[test]
fn a_targeted_warlock_taps_when_healthy() {
    assert_taps(&scene().target(ENEMY, WARLOCK));
    assert_taps(
        &scene()
            .unit("enemy mage", 2, Mage)
            .at(30.0, 0.0)
            .targeting(WARLOCK),
    );
}

/// Life Tap is not a play: it runs under `TeamPlan` as under `Legacy`.
#[test]
fn a_teamplan_warlock_taps_too() {
    assert_taps(&scene().ai_profile(1, AiProfile::TeamPlan));
    assert_taps(&healerless().ai_profile(1, AiProfile::TeamPlan));
    let held_tp = scene()
        .ai_profile(1, AiProfile::TeamPlan)
        .mana_of(WARLOCK, LIFE_TAP_STARVED_FRACTION);
    assert!(matches!(held(&held_tp), TapHold::NotStarved { .. }));
}

// ---------------------------------------------------------------------------
// The conversion's backstops.
// ---------------------------------------------------------------------------

/// The 1-health floor never kills and never RAISES health; the mana cap
/// never LOWERS mana.
#[test]
fn the_conversion_never_moves_the_wrong_way() {
    let fresh = || Combatant::new(1, 0, Warlock);

    // The ordinary case: one for one.
    let mut c = fresh();
    c.current_mana = 10.0;
    let (h, m) = (c.current_health, c.current_mana);
    convert_health_to_mana(&mut c, 50.0);
    assert_eq!((c.current_health, c.current_mana), (h - 50.0, m + 50.0));

    // A tap larger than the health left stops at 1.
    let mut c = fresh();
    c.current_health = 20.0;
    convert_health_to_mana(&mut c, 50.0);
    assert_eq!(c.current_health, 1.0);

    // Already below 1: the floor does not raise it.
    let mut c = fresh();
    c.current_health = 0.5;
    convert_health_to_mana(&mut c, 50.0);
    assert!(
        c.current_health <= 0.5,
        "health rose to {}",
        c.current_health
    );

    // Mana is capped at the pool ...
    let mut c = fresh();
    c.current_mana = c.max_mana - 10.0;
    convert_health_to_mana(&mut c, 50.0);
    assert_eq!(c.current_mana, c.max_mana);

    // ... and the cap never lowers mana already above it.
    let mut c = fresh();
    c.current_mana = c.max_mana + 20.0;
    let above = c.current_mana;
    convert_health_to_mana(&mut c, 50.0);
    assert!(c.current_mana >= above, "mana fell to {}", c.current_mana);
}
