//! AS-483 — the Mage's self-buffs are held by what would actually refuse
//! them, never by a same-type aura from somewhere else.
//!
//! - **Ice Barrier** is an absorb, and absorbs are keyed by their SOURCE:
//!   `apply_pending_auras` lands it beside a Priest's Power Word: Shield or an
//!   item's shield, and refuses it only while an Ice Barrier is up. So only
//!   the Mage's own barrier holds it.
//! - **The armor** (Frost, Mage or Molten) is a one-per-type buff, held by
//!   whatever holds its type's slot (`Aura::holds_type_slot`). A proc buff
//!   of the same type is keyed by its source and holds no slot, so
//!   Doomfinger's crit does not stand for Molten Armor, nor Darkmoon Card:
//!   Blue Dragon's regeneration for Mage Armor; neither does a stacking
//!   aura of the type (an Innervate). No content reaches this today: the
//!   armor is cast before any proc can fire and nothing removes it. The
//!   scenes guard the one state it covers — the armor absent while such a
//!   buff is up.
//!
//! Scenes (tier 1), each a pair: the Mage carrying the other source's aura
//! (the situation the old gate got wrong), and the control carrying its own.

use arenasim::states::match_config::CharacterClass::{Mage, Priest, Warrior};
use arenasim::states::match_config::MageArmor;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::equipment::ItemId;
use arenasim::states::play_match::{AbilityType, Aura, AuraStacks, AuraType, StackScope};

const ME: &str = "me";
const PRIEST: &str = "priest";
const ENEMY: &str = "enemy";

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

/// A Mage at full health beside a teammate Priest, with an enemy Warrior
/// standing off at 30yd: nothing calls a play, so the chain opens on its
/// self-buffs.
fn mage() -> Scene {
    Scene::new()
        .unit(ME, 1, Mage)
        .targeting(ENEMY)
        .unit(PRIEST, 1, Priest)
        .at(-10.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(30.0, 0.0)
}

fn assert_chose_self(view: &DecisionView, ability: AbilityType) {
    assert_eq!(
        view.chose(),
        Some((ability, Some(ME))),
        "{}",
        describe(view)
    );
}

fn assert_held(view: &DecisionView, ability: AbilityType) {
    assert!(
        matches!(
            view.rejected(ability),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{ability:?} should be held by its own copy: {}",
        describe(view)
    );
    assert_ne!(
        view.chose(),
        Some((ability, Some(ME))),
        "{}",
        describe(view)
    );
}

// ---------------------------------------------------------------------------
// Ice Barrier
// ---------------------------------------------------------------------------

/// An absorb named `name`, cast on the Mage by `caster`.
fn absorb(scene: &Scene, caster: &'static str, name: &str) -> Aura {
    Aura {
        effect_type: AuraType::Absorb,
        duration: 25.0,
        magnitude: 70.0,
        break_on_damage_threshold: -1.0,
        caster: Some(scene.entity(caster)),
        ability_name: name.to_string(),
        ..Default::default()
    }
}

fn shielded_by(caster: &'static str, name: &str) -> Scene {
    let scene = mage();
    let shield = absorb(&scene, caster, name);
    scene.aura_full(ME, shield)
}

#[test]
fn a_priests_shield_does_not_hold_the_mages_ice_barrier() {
    let view = shielded_by(PRIEST, "Power Word: Shield").decide(ME);
    assert_chose_self(&view, AbilityType::IceBarrier);
}

/// An item's triggered shield (Aegis of the Last Stand, no caster) is an
/// absorb of its own source too.
#[test]
fn an_items_shield_does_not_hold_the_mages_ice_barrier() {
    let shield = Aura {
        effect_type: AuraType::Absorb,
        duration: 25.0,
        magnitude: 90.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Aegis of the Last Stand".to_string(),
        source_item: Some(ItemId::AegisOfTheLastStand),
        ..Default::default()
    };
    let view = mage().aura_full(ME, shield).decide(ME);
    assert_chose_self(&view, AbilityType::IceBarrier);
}

/// Control: the Mage's own Ice Barrier holds it.
#[test]
fn the_mages_own_ice_barrier_holds_its_ice_barrier() {
    let view = shielded_by(ME, "Ice Barrier").decide(ME);
    assert_held(&view, AbilityType::IceBarrier);
}

/// Control: with nothing on it, the Mage at full health raises its barrier —
/// so the shield scene above is not passing on a barrier the Mage would
/// never cast.
#[test]
fn an_unshielded_mage_raises_its_ice_barrier() {
    let view = mage().decide(ME);
    assert_chose_self(&view, AbilityType::IceBarrier);
}

// ---------------------------------------------------------------------------
// The armor
// ---------------------------------------------------------------------------

/// The Mage wearing `armor`, its Ice Barrier on cooldown so the chain
/// reaches the armor.
fn armored(armor: MageArmor) -> Scene {
    let mut scene = mage().cooldown(ME, AbilityType::IceBarrier, 20.0);
    scene.combatant_mut(ME).mage_armor = armor;
    scene
}

/// A proc trinket's buff of `effect` on the Mage, as `proc_trinkets` lands
/// it: no caster, named after the item, carrying the item.
fn with_proc(scene: Scene, item: ItemId, name: &str, effect: AuraType) -> Scene {
    scene.aura_full(
        ME,
        Aura {
            effect_type: effect,
            duration: 8.0,
            magnitude: 0.1,
            break_on_damage_threshold: -1.0,
            ability_name: name.to_string(),
            source_item: Some(item),
            ..Default::default()
        },
    )
}

/// The Mage's own armor buff of `effect`, named `name`.
fn with_own_armor(scene: Scene, name: &str, effect: AuraType) -> Scene {
    let me = scene.entity(ME);
    scene.aura_full(
        ME,
        Aura {
            effect_type: effect,
            duration: 500.0,
            magnitude: 0.05,
            break_on_damage_threshold: -1.0,
            caster: Some(me),
            ability_name: name.to_string(),
            ..Default::default()
        },
    )
}

#[test]
fn a_crit_proc_does_not_hold_molten_armor() {
    let scene = with_proc(
        armored(MageArmor::MoltenArmor),
        ItemId::Doomfinger,
        "Doomfinger",
        AuraType::CritChanceIncrease,
    );
    assert_chose_self(&scene.decide(ME), AbilityType::MoltenArmor);
}

/// Control: Molten Armor itself holds Molten Armor.
#[test]
fn molten_armor_holds_molten_armor() {
    let scene = with_own_armor(
        armored(MageArmor::MoltenArmor),
        "Molten Armor",
        AuraType::CritChanceIncrease,
    );
    assert_held(&scene.decide(ME), AbilityType::MoltenArmor);
}

#[test]
fn a_regeneration_proc_does_not_hold_mage_armor() {
    let scene = with_proc(
        armored(MageArmor::MageArmor),
        ItemId::DarkmoonCardBlueDragon,
        "Darkmoon Card: Blue Dragon",
        AuraType::ManaRegenIncrease,
    );
    assert_chose_self(&scene.decide(ME), AbilityType::MageArmorSpell);
}

/// A stacking regeneration aura (Innervate's shape: refresh-only, keyed by
/// its source, no item) holds no slot either. Tells `holds_type_slot` apart
/// from a check on `source_item` alone.
#[test]
fn a_stacking_regeneration_aura_does_not_hold_mage_armor() {
    let scene = armored(MageArmor::MageArmor);
    let caster = scene.entity(PRIEST);
    let scene = scene.aura_full(
        ME,
        Aura {
            effect_type: AuraType::ManaRegenIncrease,
            duration: 20.0,
            magnitude: 8.0,
            break_on_damage_threshold: -1.0,
            caster: Some(caster),
            ability_name: "Innervate".to_string(),
            stacks: Some(AuraStacks::new(1, StackScope::PerCaster)),
            ..Default::default()
        },
    );
    assert_chose_self(&scene.decide(ME), AbilityType::MageArmorSpell);
}

/// Control: Mage Armor itself holds Mage Armor.
#[test]
fn mage_armor_holds_mage_armor() {
    let scene = with_own_armor(
        armored(MageArmor::MageArmor),
        "Mage Armor",
        AuraType::ManaRegenIncrease,
    );
    assert_held(&scene.decide(ME), AbilityType::MageArmorSpell);
}
