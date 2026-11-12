//! AS-430 — when a FREE wearer uses a buff on-use item (a stat burst or haste:
//! the Talisman of Ephemeral Power, Earthstrike, Kiss of the Spider, the
//! Diamond Flask). Each scene runs the real class dispatch
//! (`combat_ai::dispatch_class_decision`, the entry point `decide_abilities`
//! runs, which asks `class_ai::on_use::try_buffs` ahead of the class chain)
//! for one tick, paired with its control: the same scene with the rule's one
//! condition removed.
//!
//! The rule, one per effect kind: a damage dealer uses it in its team's kill
//! window; a healer when a teammate (itself included) drops low; and a buff
//! carrying a heal over time also when its own wearer is low.

use arenasim::states::match_config::CharacterClass::{Mage, Priest, Warrior};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::equipment::ItemId;

const WEARER: &str = "wearer";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy";

/// Whether the decision saw its team's kill window open.
fn kill_window(view: &DecisionView) -> bool {
    view.situation
        .as_ref()
        .expect("an ability decision carries its situation")
        .kill_window
        .is_some()
}

/// Whether team 1 has a teammate low in `scene`, as its decision classifies it.
fn teammate_low(mut scene: Scene) -> bool {
    scene
        .reclassify()
        .for_team(1)
        .dying_allies()
        .next()
        .is_some()
}

/// The wearer fired `item` this tick, its decision traced as a use.
fn used(view: &DecisionView, item: ItemId) -> bool {
    view.items_used == vec![(WEARER, item)]
        && matches!(view.item_decisions.as_slice(), [(i, None)] if *i == item)
        && !view
            .on_use_after
            .iter()
            .any(|s| s.item == item && s.ready())
}

/// The wearer held `item` this tick, its decision traced with `why`.
fn held(view: &DecisionView, item: ItemId, why: &str) -> bool {
    view.items_used.is_empty()
        && matches!(
            view.item_decisions.as_slice(),
            [(i, Some(RejectionReason::PreconditionUnmet { note }))] if *i == item && note.contains(why)
        )
        && view
            .on_use_after
            .iter()
            .any(|s| s.item == item && s.ready())
}

/// A damage dealer of `class` wearing `item`, beside a healthy Mage, against
/// a lone Mage (no healer to save it) at `enemy_health`, in melee range.
fn dps(
    class: arenasim::states::match_config::CharacterClass,
    item: ItemId,
    enemy_health: f32,
) -> Scene {
    Scene::new()
        .unit(WEARER, 1, class)
        .on_use_item(WEARER, item)
        .targeting(ENEMY)
        .unit(ALLY, 1, Mage)
        .at(0.0, 20.0)
        .unit(ENEMY, 2, Mage)
        .at(2.0, 0.0)
        .health(enemy_health)
}

/// A Priest wearing `item` beside a Warrior at `ally_health`, against a
/// healthy Warrior and a Mage.
fn healer(item: ItemId, ally_health: f32) -> Scene {
    Scene::new()
        .unit(WEARER, 1, Priest)
        .on_use_item(WEARER, item)
        .unit(ALLY, 1, Warrior)
        .at(0.0, 10.0)
        .health(ally_health)
        .unit(ENEMY, 2, Warrior)
        .at(0.0, 12.0)
        .targeting(ALLY)
        .unit("enemy mage", 2, Mage)
        .at(0.0, -30.0)
}

// ---------------------------------------------------------------------------
// A damage dealer: the kill window
// ---------------------------------------------------------------------------

#[test]
fn a_damage_dealer_uses_a_stat_burst_in_the_kill_window_and_not_before() {
    for item in [ItemId::Earthstrike, ItemId::KissOfTheSpider] {
        let view = dps(Warrior, item, 0.2).decide(WEARER);
        assert!(kill_window(&view), "the window is open");
        assert!(used(&view, item), "{item:?}: {view:?}");

        let view = dps(Warrior, item, 0.9).decide(WEARER);
        assert!(!kill_window(&view), "no window");
        assert!(held(&view, item, "no kill window"), "{item:?}: {view:?}");
    }
}

#[test]
fn a_ranged_damage_dealer_uses_it_in_the_window_too() {
    let item = ItemId::TalismanOfEphemeralPower;
    let view = dps(Mage, item, 0.2).decide(WEARER);
    assert!(used(&view, item), "{view:?}");
    let view = dps(Mage, item, 0.9).decide(WEARER);
    assert!(held(&view, item, "no kill window"), "{view:?}");
}

#[test]
fn a_damage_dealer_does_not_spend_it_on_a_teammate_low() {
    // A teammate low is the healer's moment, not the damage dealer's.
    let view = dps(Warrior, ItemId::Earthstrike, 0.9)
        .health_of(ALLY, 0.2)
        .decide(WEARER);
    assert!(!kill_window(&view));
    assert!(
        held(&view, ItemId::Earthstrike, "no kill window"),
        "{view:?}"
    );
}

#[test]
fn a_use_costs_no_global_cooldown() {
    // In the window the Warrior uses Earthstrike AND still takes its
    // ordinary turn — the same choice it makes without the trinket.
    let with = dps(Warrior, ItemId::Earthstrike, 0.2)
        .rage(100.0)
        .decide(WEARER);
    let bare = Scene::new()
        .unit(WEARER, 1, Warrior)
        .targeting(ENEMY)
        .rage(100.0)
        .unit(ALLY, 1, Mage)
        .at(0.0, 20.0)
        .unit(ENEMY, 2, Mage)
        .at(2.0, 0.0)
        .health(0.2)
        .decide(WEARER);
    assert!(used(&with, ItemId::Earthstrike), "{with:?}");
    assert!(bare.chose().is_some(), "the control takes a turn: {bare:?}");
    assert_eq!(with.chose(), bare.chose());
    assert_eq!(with.acted, bare.acted);
}

#[test]
fn an_item_on_its_cooldown_is_not_asked() {
    let mut scene = dps(Warrior, ItemId::Earthstrike, 0.2);
    scene.combatant_mut(WEARER).on_use_items[0].remaining = 30.0;
    let view = scene.decide(WEARER);
    assert!(view.items_used.is_empty(), "{view:?}");
    assert!(view.item_decisions.is_empty(), "{view:?}");
    // ...and once it is ready, the same tick fires it.
    let ready = dps(Warrior, ItemId::Earthstrike, 0.2).decide(WEARER);
    assert!(used(&ready, ItemId::Earthstrike), "{ready:?}");
}

#[test]
fn a_once_a_match_item_is_spent_by_its_use() {
    let view = dps(Warrior, ItemId::DiamondFlask, 0.2).decide(WEARER);
    assert!(used(&view, ItemId::DiamondFlask), "{view:?}");
    let slot = &view.on_use_after[0];
    assert!(slot.spent, "the Flask is spent: {slot:?}");
}

#[test]
fn nothing_is_asked_before_the_gates_open() {
    let view = dps(Warrior, ItemId::Earthstrike, 0.2)
        .before_gates()
        .decide(WEARER);
    assert!(view.items_used.is_empty(), "{view:?}");
    assert!(view.item_decisions.is_empty(), "{view:?}");
}

// ---------------------------------------------------------------------------
// A healer: a teammate low
// ---------------------------------------------------------------------------

#[test]
fn a_healer_uses_a_stat_burst_when_a_teammate_drops_low() {
    let item = ItemId::TalismanOfEphemeralPower;
    assert!(teammate_low(healer(item, 0.3)));
    let view = healer(item, 0.3).decide(WEARER);
    assert!(used(&view, item), "{view:?}");

    assert!(!teammate_low(healer(item, 0.9)));
    let view = healer(item, 0.9).decide(WEARER);
    assert!(held(&view, item, "no teammate low"), "{view:?}");
}

#[test]
fn a_healer_counts_itself_as_a_teammate_low() {
    let item = ItemId::TalismanOfEphemeralPower;
    let view = healer(item, 0.9).health_of(WEARER, 0.3).decide(WEARER);
    assert!(used(&view, item), "{view:?}");
}

#[test]
fn a_healer_does_not_spend_it_on_the_kill_window() {
    // Its team has a kill window (the enemy Mage alone and low), but nobody
    // on its own side is low.
    let item = ItemId::TalismanOfEphemeralPower;
    let view = Scene::new()
        .unit(WEARER, 1, Priest)
        .on_use_item(WEARER, item)
        .unit(ALLY, 1, Warrior)
        .at(0.0, 10.0)
        .unit(ENEMY, 2, Mage)
        .at(0.0, 25.0)
        .health(0.2)
        .decide(WEARER);
    assert!(kill_window(&view), "the window is open");
    assert!(held(&view, item, "no teammate low"), "{view:?}");
}

// ---------------------------------------------------------------------------
// A heal over time: its wearer low
// ---------------------------------------------------------------------------

#[test]
fn a_buff_that_heals_is_used_when_its_wearer_drops_low() {
    // A Warrior low, no kill window: the Flask's heal is its moment.
    let view = dps(Warrior, ItemId::DiamondFlask, 0.9)
        .health_of(WEARER, 0.3)
        .decide(WEARER);
    assert!(!kill_window(&view));
    assert!(used(&view, ItemId::DiamondFlask), "{view:?}");

    // Control: the same Warrior with a buff that does not heal holds it.
    let view = dps(Warrior, ItemId::Earthstrike, 0.9)
        .health_of(WEARER, 0.3)
        .decide(WEARER);
    assert!(
        held(&view, ItemId::Earthstrike, "no kill window"),
        "{view:?}"
    );

    // Control: the Flask on a healthy Warrior is held.
    let view = dps(Warrior, ItemId::DiamondFlask, 0.9).decide(WEARER);
    assert!(
        held(&view, ItemId::DiamondFlask, "no kill window"),
        "{view:?}"
    );
}

#[test]
fn a_teammate_low_is_not_the_wearers_own_heal() {
    // A damage dealer's Flask heals only itself: a teammate low does not
    // fire it.
    let scene = || dps(Warrior, ItemId::DiamondFlask, 0.9).health_of(ALLY, 0.3);
    assert!(teammate_low(scene()));
    let view = scene().decide(WEARER);
    assert!(
        held(&view, ItemId::DiamondFlask, "no kill window"),
        "{view:?}"
    );
}

// ---------------------------------------------------------------------------
// The crowd-control break is not a buff
// ---------------------------------------------------------------------------

#[test]
fn the_medallion_is_never_asked_by_a_free_unit() {
    let view = dps(Warrior, ItemId::MedallionOfTheAlliance, 0.2).decide(WEARER);
    assert!(view.items_used.is_empty(), "{view:?}");
    assert!(view.item_decisions.is_empty(), "{view:?}");
}

#[test]
fn a_held_wearer_never_uses_a_buff() {
    // A stunned Warrior in its team's kill window: the held dispatch asks only
    // a crowd-control break, and a buff would break nothing.
    let item = ItemId::Earthstrike;
    let view = dps(Warrior, item, 0.2)
        .aura(WEARER, arenasim::states::play_match::AuraType::Stun, 6.0)
        .decide_held(WEARER);
    assert!(view.items_used.is_empty(), "{view:?}");
    assert!(view.item_decisions.is_empty(), "{view:?}");
    // Control: the same wearer free uses it.
    assert!(used(&dps(Warrior, item, 0.2).decide(WEARER), item));
}
