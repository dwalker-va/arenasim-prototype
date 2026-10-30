//! Scene tests for the Warlock's Life Tap (AS-296): it converts health into
//! mana when it is MANA-STARVED and the health is safe to spend — nobody has
//! it targeted, a teammate healer can heal it now, and the tap does not leave
//! it dying (`class_ai::warlock::life_tap_hold`). Each gate is pinned as a
//! pair: the scene that taps, and the same scene with one condition changed,
//! held with that gate's typed reason.
//!
//! Every scene runs the real class dispatch for one tick, so a pass also
//! proves the dispatch reaches the tap — and, since the starved Warlock still
//! has the mana for Corruption on an enemy in range, that the tap comes
//! before the rotation.

use arenasim::states::match_config::CharacterClass::{Mage, Priest, Warlock, Warrior};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::class_ai::warlock::LIFE_TAP_STARVED_FRACTION;
use arenasim::states::play_match::components::Combatant;
use arenasim::states::play_match::decision_trace::{RejectionReason, TapHold};
use arenasim::states::play_match::movement_config::load_movement_config;
use arenasim::states::play_match::{AbilityType, AuraType};

const WARLOCK: &str = "warlock";
const PRIEST: &str = "priest";
const ENEMY: &str = "enemy warrior";

/// A starved Warlock (a fifth of its mana: still enough for Corruption) at
/// full health, its Priest 10yd behind it, and an enemy Warrior 20yd off —
/// in Corruption's range, and after the Priest. The Priest targets the
/// Warlock: a teammate's target is no focus.
fn scene() -> Scene {
    Scene::new()
        .unit(WARLOCK, 1, Warlock)
        .mana(0.2)
        .targeting(ENEMY)
        .unit(PRIEST, 1, Priest)
        .at(-10.0, 0.0)
        .targeting(WARLOCK)
        .unit(ENEMY, 2, Warrior)
        .at(20.0, 0.0)
        .targeting(PRIEST)
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
/// gear), and the health below which it is dying.
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
fn a_starved_safe_warlock_taps_before_its_rotation() {
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
fn an_enemy_on_the_warlock_holds_the_tap() {
    let focused = scene().target(ENEMY, WARLOCK);
    assert!(matches!(held(&focused), TapHold::Focused { .. }));
    assert_taps(&scene());
}

/// A caster shooting from range is focus too, though it does not PRESSURE
/// the Warlock (it is not closing): the gate is "an enemy has it targeted".
#[test]
fn a_distant_caster_on_the_warlock_holds_the_tap() {
    let caster = scene()
        .unit("enemy mage", 2, Mage)
        .at(30.0, 0.0)
        .targeting(WARLOCK);
    assert!(matches!(held(&caster), TapHold::Focused { .. }));
}

/// A dead enemy's stale target is nobody's focus.
#[test]
fn a_dead_enemy_targeting_the_warlock_is_no_focus() {
    let dead = scene()
        .unit("enemy mage", 2, Mage)
        .at(30.0, 0.0)
        .targeting(WARLOCK)
        .health(0.0);
    assert_taps(&dead);
}

#[test]
fn no_healer_on_the_team_holds_the_tap() {
    // A Mage teammate in the Priest's place: alive, in reach, full of mana —
    // everything a healer would need but the class.
    let no_healer = Scene::new()
        .unit(WARLOCK, 1, Warlock)
        .mana(0.2)
        .targeting(ENEMY)
        .unit("mage", 1, Mage)
        .at(-10.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(20.0, 0.0)
        .targeting("mage");
    assert_eq!(held(&no_healer), TapHold::NoHealer);
}

/// An ENEMY healer in reach does not give the health back.
#[test]
fn an_enemy_healer_is_no_healer() {
    let enemy_priest = Scene::new()
        .unit(WARLOCK, 1, Warlock)
        .mana(0.2)
        .targeting(ENEMY)
        .unit("enemy priest", 2, Priest)
        .at(-10.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(20.0, 0.0);
    assert_eq!(held(&enemy_priest), TapHold::NoHealer);
}

#[test]
fn a_dead_healer_holds_the_tap() {
    assert_eq!(held(&scene().health_of(PRIEST, 0.0)), TapHold::NoHealer);
}

#[test]
fn a_locked_healer_holds_the_tap() {
    assert_eq!(
        held(&scene().aura(PRIEST, AuraType::Stun, 4.0)),
        TapHold::NoHealer
    );
    // A root does not stop it healing.
    assert_taps(&scene().aura(PRIEST, AuraType::Root, 4.0));
}

#[test]
fn a_healer_out_of_mana_holds_the_tap() {
    assert_eq!(held(&scene().mana_of(PRIEST, 0.0)), TapHold::NoHealer);
}

#[test]
fn a_healer_out_of_reach_holds_the_tap() {
    let heal_range = load_movement_config()
        .expect("movement.ron loads")
        .shared
        .heal_range;
    assert_eq!(
        held(&scene().place(PRIEST, -(heal_range + 1.0), 0.0)),
        TapHold::NoHealer
    );
    assert_taps(&scene().place(PRIEST, -(heal_range - 1.0), 0.0));
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
/// a full-mana Warlock under attack is "not starved", not "focused".
#[test]
fn the_first_gate_that_holds_is_the_one_traced() {
    let both = scene().mana_of(WARLOCK, 1.0).target(ENEMY, WARLOCK);
    assert!(matches!(held(&both), TapHold::NotStarved { .. }));
}
