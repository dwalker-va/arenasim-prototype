//! AS-305 — when a held wearer breaks free with the Medallion
//! (`class_ai::on_use`, the crowd-control break's one AI rule). Each scene
//! runs the real held-unit dispatch (`combat_ai::dispatch_while_held`, the
//! entry point `decide_abilities` runs for a stunned, feared, polymorphed,
//! incapacitated or cycloned unit) for one tick, paired with its control: the
//! same scene with the rule's one condition removed.
//!
//! The rule: break on the first LONG hold (2s or more left) — unless a
//! teammate is going to dispel every long hold, in which case wait for it, at
//! most `DISPEL_WAIT_GRACE_SECS`.

use arenasim::states::match_config::CharacterClass::{Mage, Paladin, Priest, Rogue, Warrior};
use arenasim::states::play_match::class_ai::on_use::DISPEL_WAIT_GRACE_SECS;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::equipment::ItemId::MedallionOfTheAlliance as MEDALLION;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::{AbilityType, AuraType};

const WEARER: &str = "wearer";
const PRIEST: &str = "priest";
const ALLY: &str = "ally";
const ROGUE: &str = "rogue";
const MAGE: &str = "mage";

/// The scene's clock (`Scene::new` stands at 30s).
const NOW: f32 = 30.0;

/// A Warrior wearing the Medallion beside a Mage (no dispel), against a Rogue
/// and a Mage, held by `hold` for `secs`.
fn held(hold: AuraType, secs: f32) -> Scene {
    Scene::new()
        .unit(WEARER, 1, Warrior)
        .on_use_item(WEARER, MEDALLION)
        .aura(WEARER, hold, secs)
        .unit(ALLY, 1, Mage)
        .at(0.0, 15.0)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
        .unit(MAGE, 2, Mage)
        .at(0.0, -25.0)
}

/// As [`held`], with a Priest beside the wearer — in Dispel Magic's reach.
fn held_beside_a_priest(hold: AuraType, secs: f32) -> Scene {
    Scene::new()
        .unit(WEARER, 1, Warrior)
        .on_use_item(WEARER, MEDALLION)
        .aura(WEARER, hold, secs)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 10.0)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
        .unit(MAGE, 2, Mage)
        .at(0.0, -25.0)
}

fn used(view: &DecisionView) -> bool {
    view.items_used == vec![(WEARER, MEDALLION)]
        && matches!(view.item_decisions.as_slice(), [(MEDALLION, None)])
        && view.acted
}

/// Held with a note containing `why`.
fn held_because(view: &DecisionView, why: &str) -> bool {
    view.items_used.is_empty()
        && !view.acted
        && matches!(
            view.item_decisions.as_slice(),
            [(MEDALLION, Some(RejectionReason::PreconditionUnmet { note }))] if note.contains(why)
        )
}

fn waits(view: &DecisionView) -> bool {
    held_because(view, "waiting for")
}

// ---------------------------------------------------------------------------
// The first long hold
// ---------------------------------------------------------------------------

#[test]
fn the_first_long_hold_breaks_and_a_short_one_runs_out() {
    // 2.0s is the floor, inclusive.
    assert!(used(&held(AuraType::Stun, 2.0).decide_held(WEARER)));
    let view = held(AuraType::Stun, 1.9).decide_held(WEARER);
    assert!(held_because(&view, "short hold"), "{view:?}");
}

#[test]
fn every_hold_breaks_without_anyone_low() {
    // No teammate or kill-window condition: a healthy team, a healthy wearer.
    for hold in [
        AuraType::Stun,
        AuraType::Fear,
        AuraType::Polymorph,
        AuraType::Incapacitate,
    ] {
        let view = held(hold, 6.0).decide_held(WEARER);
        assert!(used(&view), "{hold:?}: {view:?}");
    }
}

#[test]
fn a_root_does_not_count_toward_the_floor() {
    // A 1s stun under a 6s polymorph: the polymorph is the hold left.
    let view = held(AuraType::Stun, 1.0)
        .aura(WEARER, AuraType::Polymorph, 6.0)
        .decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    // A 1s stun beside a 6s root: the root holds nothing, so 1s is left.
    let view = held(AuraType::Stun, 1.0)
        .aura(WEARER, AuraType::Root, 6.0)
        .decide_held(WEARER);
    assert!(held_because(&view, "short hold"), "{view:?}");
}

#[test]
fn nothing_is_used_from_inside_a_cyclone() {
    let view = held(AuraType::Cyclone, 6.0).decide_held(WEARER);
    assert!(
        view.item_decisions.is_empty() && view.items_used.is_empty(),
        "{view:?}"
    );
    assert!(!view.acted);
}

#[test]
fn a_spent_medallion_is_not_asked_again() {
    let mut scene = held(AuraType::Stun, 4.0);
    scene.combatant_mut(WEARER).on_use_items[0].spent = true;
    let view = scene.decide_held(WEARER);
    assert!(
        view.items_used.is_empty() && view.item_decisions.is_empty(),
        "{view:?}"
    );
    assert!(!view.acted);
}

#[test]
fn a_unit_without_one_decides_nothing_about_items() {
    let view = Scene::new()
        .unit(WEARER, 1, Warrior)
        .aura(WEARER, AuraType::Stun, 4.0)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .decide_held(WEARER);
    assert!(view.item_decisions.is_empty() && view.items_used.is_empty());
}

/// A Paladin wearing the Medallion or not, stunned 4s beside a Priest at 30%:
/// Divine Shield's teammate trigger holds too.
fn paladin_scene(medallion: bool) -> Scene {
    let scene = Scene::new().unit(WEARER, 1, Paladin);
    let scene = if medallion {
        scene.on_use_item(WEARER, MEDALLION)
    } else {
        scene
    };
    scene
        .aura(WEARER, AuraType::Stun, 4.0)
        .unit(PRIEST, 1, Priest)
        .at(0.0, 10.0)
        .health(0.3)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
}

#[test]
fn the_medallion_goes_before_a_class_break() {
    // Wearing it, the Paladin breaks free with the Medallion and keeps its
    // bubble...
    let view = paladin_scene(true).decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    assert_eq!(view.chose(), None, "{view:?}");
    // ...and without it, the same hold is broken by Divine Shield.
    let view = paladin_scene(false).decide_held(WEARER);
    assert_eq!(view.chose().map(|c| c.0), Some(AbilityType::DivineShield));
    assert!(view.items_used.is_empty());
}

// ---------------------------------------------------------------------------
// Waiting for a teammate's dispel
// ---------------------------------------------------------------------------

#[test]
fn a_dispellable_hold_waits_for_the_priest_and_a_stun_does_not() {
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0).decide_held(WEARER);
    assert!(waits(&view), "{view:?}");
    // Freezing Trap and Fear are urgent dispels too.
    assert!(waits(
        &held_beside_a_priest(AuraType::Incapacitate, 8.0).decide_held(WEARER)
    ));
    // A feared Warrior keeps the Medallion for the Priest and answers the
    // fear with its own Berserker Rage, the class break below it.
    let view = held_beside_a_priest(AuraType::Fear, 8.0).decide_held(WEARER);
    assert!(view.items_used.is_empty(), "{view:?}");
    assert!(
        matches!(view.item_decisions.as_slice(),
            [(MEDALLION, Some(RejectionReason::PreconditionUnmet { note }))] if note.contains("waiting for")),
        "{view:?}"
    );
    assert_eq!(view.chose().map(|c| c.0), Some(AbilityType::BerserkerRage));
    // Control: no dispel lifts a stun.
    assert!(used(
        &held_beside_a_priest(AuraType::Stun, 4.0).decide_held(WEARER)
    ));
    // Control: a stun beside the polymorph is still a long hold the dispel
    // leaves, so there is nothing to wait for.
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0)
        .aura(WEARER, AuraType::Stun, 4.0)
        .decide_held(WEARER);
    assert!(used(&view), "{view:?}");
}

#[test]
fn a_teammate_with_no_dispel_is_not_waited_for() {
    // `held` stands a Mage beside the wearer: it frees nobody.
    assert!(used(&held(AuraType::Polymorph, 8.0).decide_held(WEARER)));
}

#[test]
fn a_priest_that_cannot_cast_is_not_waited_for() {
    // Silenced.
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0)
        .aura(PRIEST, AuraType::Silence, 4.0)
        .decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    // Held itself.
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0)
        .aura(PRIEST, AuraType::Stun, 4.0)
        .decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    // Out of mana for Dispel Magic.
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0)
        .mana_of(PRIEST, 0.0)
        .decide_held(WEARER);
    assert!(used(&view), "{view:?}");
    // Dispel Magic on cooldown.
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0)
        .cooldown(PRIEST, AbilityType::DispelMagic, 3.0)
        .decide_held(WEARER);
    assert!(used(&view), "{view:?}");
}

#[test]
fn the_priest_owes_its_dispel_to_the_teammate_it_reaches_first() {
    // A second polymorphed teammate stands in the Priest's reach while the
    // wearer, behind a pillar, does not: the rotation frees the other one, so
    // the wearer does not wait.
    let pillar = ObstacleVolume::Cylinder {
        center_xz: bevy::prelude::Vec2::new(0.0, 5.0),
        radius: 2.0,
        base_y: 0.0,
        height: 10.0,
    };
    let scene = |second_held: bool| {
        let s = Scene::new()
            .obstacles(vec![pillar])
            .unit(WEARER, 1, Warrior)
            .on_use_item(WEARER, MEDALLION)
            .aura(WEARER, AuraType::Polymorph, 8.0)
            .unit(PRIEST, 1, Priest)
            .at(0.0, 10.0)
            .unit(ALLY, 1, Rogue)
            .at(10.0, 10.0)
            .unit(ROGUE, 2, Rogue)
            .at(-20.0, 0.0)
            .unit(MAGE, 2, Mage)
            .at(0.0, -25.0);
        if second_held {
            s.aura(ALLY, AuraType::Polymorph, 8.0)
        } else {
            s
        }
    };
    assert!(used(&scene(true).decide_held(WEARER)));
    // Control: with nobody else to free, the walk round the pillar is owed to
    // the wearer, and it waits.
    let view = scene(false).decide_held(WEARER);
    assert!(waits(&view), "{view:?}");
}

#[test]
fn the_wait_ends_when_its_grace_runs_out() {
    // A wait begun just inside the grace holds...
    let mut scene = held_beside_a_priest(AuraType::Polymorph, 8.0);
    scene.combatant_mut(WEARER).on_use_items[0].waiting =
        Some((NOW - DISPEL_WAIT_GRACE_SECS + 0.1, NOW - 1.0 / 60.0));
    assert!(waits(&scene.decide_held(WEARER)));
    // ...and one begun a grace ago breaks free.
    let mut scene = held_beside_a_priest(AuraType::Polymorph, 8.0);
    scene.combatant_mut(WEARER).on_use_items[0].waiting =
        Some((NOW - DISPEL_WAIT_GRACE_SECS, NOW - 1.0 / 60.0));
    assert!(used(&scene.decide_held(WEARER)));
    // A wait last asked long ago belonged to an earlier hold: this one starts
    // its own.
    let mut scene = held_beside_a_priest(AuraType::Polymorph, 8.0);
    scene.combatant_mut(WEARER).on_use_items[0].waiting = Some((NOW - 20.0, NOW - 10.0));
    assert!(waits(&scene.decide_held(WEARER)));
}

#[test]
fn a_wait_remembers_when_it_began() {
    // The first ask starts the wait now...
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0).decide_held(WEARER);
    assert_eq!(view.on_use_after[0].waiting, Some((NOW, NOW)), "{view:?}");
    // ...a later ask of the same hold keeps its start...
    let mut scene = held_beside_a_priest(AuraType::Polymorph, 8.0);
    scene.combatant_mut(WEARER).on_use_items[0].waiting = Some((NOW - 1.0, NOW - 1.0 / 60.0));
    let view = scene.decide_held(WEARER);
    assert_eq!(view.on_use_after[0].waiting, Some((NOW - 1.0, NOW)), "{view:?}");
    // ...and a break spends the Medallion.
    let view = held(AuraType::Stun, 4.0).decide_held(WEARER);
    assert!(view.on_use_after[0].spent, "{view:?}");
}

#[test]
fn only_the_long_holds_must_be_dispellable() {
    // A 1s stun under an 8s polymorph: the stun runs out on its own, so the
    // polymorph is the one long hold, and the Priest will dispel it.
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0)
        .aura(WEARER, AuraType::Stun, 1.0)
        .decide_held(WEARER);
    assert!(waits(&view), "{view:?}");
    // A root holds nothing, so a long root beside it changes nothing.
    let view = held_beside_a_priest(AuraType::Polymorph, 8.0)
        .aura(WEARER, AuraType::Root, 6.0)
        .decide_held(WEARER);
    assert!(waits(&view), "{view:?}");
}

#[test]
fn a_cycloned_paladin_uses_nothing() {
    // Divine Shield's teammate trigger holds (a Priest at 30%, a long hold),
    // but nothing is used from inside a Cyclone.
    let scene = |hold| {
        Scene::new()
            .unit(WEARER, 1, Paladin)
            .aura(WEARER, hold, 4.0)
            .unit(PRIEST, 1, Priest)
            .at(0.0, 10.0)
            .health(0.3)
            .unit(ROGUE, 2, Rogue)
            .at(2.0, 0.0)
    };
    assert_eq!(scene(AuraType::Cyclone).decide_held(WEARER).chose(), None);
    assert_eq!(
        scene(AuraType::Stun).decide_held(WEARER).chose().map(|c| c.0),
        Some(AbilityType::DivineShield)
    );
}

#[test]
fn a_wait_does_not_outlast_the_floor() {
    // An expected dispel, but the polymorph is nearly over: nothing to break.
    let view = held_beside_a_priest(AuraType::Polymorph, 1.5).decide_held(WEARER);
    assert!(held_because(&view, "short hold"), "{view:?}");
}
