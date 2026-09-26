//! A proc trinket's buff against the two engine paths that decide whether a
//! buff STAYS: removal (`process_dispels`) and the one-per-type buff dedup
//! (`apply_pending_auras`). Both are driven as the real systems in a Bevy app.
//!
//! **Removal.** A trinket proc is not magic — an item's effect is not a spell,
//! however it looks — so no purge or dispel may take it. The hazard is that a
//! proc's buff shares its `AuraType` with a magic buff: a Dragonspine proc is
//! the same `AttackPowerIncrease` as Battle Shout, and the Shaman's Purge is
//! pinned to a TYPE. Eligibility therefore has to be asked per aura.
//!
//! **Dedup.** A type-keyed buff (Battle Shout) and a source-keyed proc buff of
//! the same type must coexist whichever arrives first. Only the shout-first
//! order used to hold: a live proc blocked the shout, because the "already has
//! this buff" check counted it as the type's buff.

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, DispelPending,
    DispelScope, GameRng,
};
use arenasim::states::play_match::effects::process_dispels;
use arenasim::states::play_match::equipment::ItemId;
use arenasim::states::play_match::proc_trinkets::{ProcConfig, ProcTrigger};
use arenasim::CharacterClass;

/// Dragonspine Trophy's buff, from the constructor the proc hook uses.
fn dragonspine_buff() -> Aura {
    ProcConfig {
        trigger: ProcTrigger::MeleeHit,
        chance: 0.15,
        effect: AuraType::AttackPowerIncrease,
        magnitude: 55.0,
        duration: 10.0,
        internal_cooldown: 45.0,
    }
    .aura(ItemId::DragonspineTrophy, "Dragonspine Trophy")
}

/// A Battle Shout buff as the ability path builds it: type-keyed, magic.
fn battle_shout_buff() -> Aura {
    Aura {
        effect_type: AuraType::AttackPowerIncrease,
        duration: 120.0,
        magnitude: 20.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Battle Shout".to_string(),
        ..Default::default()
    }
}

fn names(app: &App, entity: Entity) -> Vec<String> {
    let mut names: Vec<String> = app
        .world()
        .entity(entity)
        .get::<ActiveAuras>()
        .map(|a| a.auras.iter().map(|x| x.ability_name.clone()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

// ============================================================================
// Removal
// ============================================================================

fn dispel_app(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(CombatLog::default());
    app.insert_resource(GameRng::from_seed(seed));
    app.add_systems(Update, process_dispels);
    app
}

fn wearer(app: &mut App, auras: Vec<Aura>) -> Entity {
    app.world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            ActiveAuras { auras },
        ))
        .id()
}

fn queue(app: &mut App, target: Entity, scope: DispelScope) {
    let dispeller = app
        .world_mut()
        .spawn(Combatant::new(2, 0, CharacterClass::Shaman))
        .id();
    app.world_mut().spawn(DispelPending {
        target,
        dispeller,
        log_prefix: "[PURGE]",
        caster_class: CharacterClass::Shaman,
        heal_on_success: None,
        scope,
    });
}

/// The headline: a purge pinned to the proc buff's own type, facing nothing
/// else, takes nothing.
#[test]
fn a_purge_cannot_remove_a_proc_buff() {
    let mut app = dispel_app(1);
    let target = wearer(&mut app, vec![dragonspine_buff()]);
    queue(
        &mut app,
        target,
        DispelScope::Purge(AuraType::AttackPowerIncrease),
    );
    app.update();
    assert_eq!(names(&app, target), vec!["Dragonspine Trophy"]);
}

/// The control, and the reason eligibility is asked per aura: beside a magic
/// buff of the same type, the purge takes the magic one EVERY time. The pick
/// among eligible auras is random by design, so this runs it under many RNG
/// seeds — if the proc were eligible, about half of them would take it.
#[test]
fn a_purge_beside_a_same_type_magic_buff_always_takes_the_magic_one() {
    for seed in 0..32 {
        let mut app = dispel_app(seed);
        let target = wearer(&mut app, vec![dragonspine_buff(), battle_shout_buff()]);
        queue(
            &mut app,
            target,
            DispelScope::Purge(AuraType::AttackPowerIncrease),
        );
        app.update();
        assert_eq!(
            names(&app, target),
            vec!["Dragonspine Trophy"],
            "seed {seed}: the purge did not take exactly the Battle Shout"
        );
    }
}

/// No other removal takes it either — every kind of removal the game has.
#[test]
fn no_removal_scope_takes_a_proc_buff() {
    let proc = dragonspine_buff();
    let scopes = [
        DispelScope::Magic,
        DispelScope::MagicOrPoison,
        DispelScope::Purge(proc.effect_type),
        DispelScope::Impairments(vec![AuraType::Root, AuraType::MovementSpeedSlow]),
    ];
    for scope in scopes {
        assert!(!scope.takes(&proc), "{scope:?} takes a proc buff");
    }
    // ...while the same scope set does take the shout it is purged beside.
    assert!(DispelScope::Purge(AuraType::AttackPowerIncrease).takes(&battle_shout_buff()));
}

// ============================================================================
// Dedup
// ============================================================================

fn apply_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(CombatLog::default());
    app.insert_resource(ArenaDampening::default());
    app.add_systems(Update, apply_pending_auras);
    app
}

fn target_with(app: &mut App, auras: Vec<Aura>) -> Entity {
    app.world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            Transform::default(),
            ActiveAuras { auras },
        ))
        .id()
}

fn land(app: &mut App, target: Entity, aura: Aura) {
    app.world_mut().spawn(AuraPending { target, aura });
    app.update();
}

/// A live proc does not stop Battle Shout from landing — the order that used
/// to fail.
#[test]
fn a_shout_lands_on_a_target_already_carrying_a_proc() {
    let mut app = apply_app();
    let target = target_with(&mut app, vec![dragonspine_buff()]);
    land(&mut app, target, battle_shout_buff());
    assert_eq!(
        names(&app, target),
        vec!["Battle Shout", "Dragonspine Trophy"]
    );
}

/// The other order, which always held.
#[test]
fn a_proc_lands_on_a_target_already_carrying_a_shout() {
    let mut app = apply_app();
    let target = target_with(&mut app, vec![battle_shout_buff()]);
    land(&mut app, target, dragonspine_buff());
    assert_eq!(
        names(&app, target),
        vec!["Battle Shout", "Dragonspine Trophy"]
    );
}

/// ...and the one-per-type rule still holds between two TYPE-keyed buffs, so
/// the change is confined to source-keyed ones.
#[test]
fn a_second_shout_is_still_refused() {
    let mut app = apply_app();
    let target = target_with(&mut app, vec![battle_shout_buff()]);
    land(&mut app, target, battle_shout_buff());
    assert_eq!(names(&app, target), vec!["Battle Shout"]);
}
