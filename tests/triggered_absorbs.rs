//! Triggered absorbs (`triggered_absorbs`, AS-421): Sacred Shield's arming
//! aura gives its bearer a slot as it lands (`apply_pending_auras`), and
//! `fire_triggered_absorbs` raises the shield from the bearer's own state at
//! the end of the frame. Each claim is a pair: the case and its control.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, DispelType,
};
use arenasim::states::play_match::triggered_absorbs::fire_triggered_absorbs;

const SHIELD: &str = "Sacred Shield";

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(Time::<()>::default());
    world
}

/// A Paladin (the caster) and a Warrior bearer, the bearer with no auras.
fn units(world: &mut World) -> (Entity, Entity) {
    let caster = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Paladin),
            Transform::default(),
        ))
        .id();
    let bearer = world
        .spawn((
            Combatant::new(1, 1, CharacterClass::Warrior),
            Transform::default(),
            ActiveAuras { auras: Vec::new() },
        ))
        .id();
    (caster, bearer)
}

fn cast(world: &mut World, ability: AbilityType, target: Entity, caster: Entity) {
    let def = AbilityDefinitions::shipped()
        .get_unchecked(&ability)
        .clone();
    let pending = AuraPending::from_ability(target, caster, &def).expect("it applies an aura");
    world.spawn(pending);
    world.run_system_once(apply_pending_auras).unwrap();
    world.flush();
}

/// Advance the clock `dt` and run the end-of-frame system.
fn frame(world: &mut World, dt: f32) {
    world
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(dt));
    world.run_system_once(fire_triggered_absorbs).unwrap();
}

fn take_damage(world: &mut World, bearer: Entity, amount: f32) {
    let mut c = world.get_mut::<Combatant>(bearer).unwrap();
    c.damage_taken += amount;
    c.current_health -= amount;
}

fn shields(world: &World, bearer: Entity) -> Vec<Aura> {
    world
        .get::<ActiveAuras>(bearer)
        .unwrap()
        .auras
        .iter()
        .filter(|a| a.effect_type == AuraType::Absorb)
        .cloned()
        .collect()
}

fn slots(world: &World, bearer: Entity) -> usize {
    world
        .get::<Combatant>(bearer)
        .unwrap()
        .triggered_absorbs
        .len()
}

#[test]
fn the_arming_aura_gives_its_bearer_a_slot_and_another_buff_does_not() {
    let mut w = world();
    let (caster, bearer) = units(&mut w);
    cast(&mut w, AbilityType::PowerWordFortitude, bearer, caster);
    assert_eq!(slots(&w, bearer), 0);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    assert_eq!(slots(&w, bearer), 1);
    let arming = w.get::<ActiveAuras>(bearer).unwrap().auras.clone();
    assert!(arming
        .iter()
        .any(|a| a.effect_type == AuraType::TriggeredAbsorb && a.ability_name == SHIELD));
}

#[test]
fn damage_raises_the_shield_and_no_damage_raises_none() {
    let mut w = world();
    let (caster, bearer) = units(&mut w);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    frame(&mut w, 0.1);
    assert!(shields(&w, bearer).is_empty(), "no damage, no shield");

    take_damage(&mut w, bearer, 12.0);
    frame(&mut w, 0.1);
    let raised = shields(&w, bearer);
    assert_eq!(raised.len(), 1);
    let shield = &raised[0];
    assert_eq!(shield.ability_name, SHIELD);
    assert_eq!(shield.magnitude, 40.0);
    assert_eq!(shield.duration, 6.0);
    assert_eq!(shield.caster, Some(caster));
    // Magic, as its buff is: a purge or dispel reaches it.
    assert_eq!(shield.dispel_type, DispelType::Auto);
    assert!(shield.can_be_purged());
}

#[test]
fn damage_taken_before_the_aura_landed_does_not_raise_it() {
    let mut w = world();
    let (caster, bearer) = units(&mut w);
    take_damage(&mut w, bearer, 30.0);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    frame(&mut w, 0.1);
    assert!(shields(&w, bearer).is_empty());
}

/// At most once every 6s: a hit inside the cooldown raises nothing and is
/// spent; one after it raises a fresh shield in place of the old one.
#[test]
fn the_cooldown_holds_it_and_a_later_hit_raises_it_again() {
    let mut w = world();
    let (caster, bearer) = units(&mut w);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    take_damage(&mut w, bearer, 10.0);
    frame(&mut w, 0.1);
    // The shield soaks some of the next hit.
    w.get_mut::<ActiveAuras>(bearer).unwrap().auras[1].magnitude = 15.0;
    take_damage(&mut w, bearer, 10.0);
    frame(&mut w, 3.0);
    assert_eq!(
        shields(&w, bearer)
            .iter()
            .map(|a| a.magnitude)
            .collect::<Vec<_>>(),
        [15.0],
        "inside the cooldown: no new shield"
    );
    frame(&mut w, 3.1);
    assert_eq!(shields(&w, bearer)[0].magnitude, 15.0, "ready, no new hit");
    take_damage(&mut w, bearer, 10.0);
    frame(&mut w, 0.1);
    assert_eq!(
        shields(&w, bearer)
            .iter()
            .map(|a| a.magnitude)
            .collect::<Vec<_>>(),
        [40.0],
        "the fresh shield replaces the old"
    );
}

#[test]
fn the_slot_ends_with_its_arming_aura() {
    let mut w = world();
    let (caster, bearer) = units(&mut w);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    w.get_mut::<ActiveAuras>(bearer)
        .unwrap()
        .auras
        .retain(|a| a.effect_type != AuraType::TriggeredAbsorb);
    take_damage(&mut w, bearer, 10.0);
    frame(&mut w, 0.1);
    assert!(shields(&w, bearer).is_empty());
    assert_eq!(slots(&w, bearer), 0);
}

/// Dampening scales the shield where it lands; the log states that amount.
#[test]
fn dampening_scales_the_shield_at_its_application_site() {
    let mut w = world();
    let (caster, bearer) = units(&mut w);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    w.resource_mut::<ArenaDampening>().reduction = 0.5;
    take_damage(&mut w, bearer, 10.0);
    frame(&mut w, 0.1);
    assert_eq!(shields(&w, bearer)[0].magnitude, 20.0);
    let log = w.resource::<CombatLog>();
    assert!(
        log.entries
            .iter()
            .any(|e| e.message.contains("gains Sacred Shield (absorbs 20)")),
        "{:?}",
        log.entries.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// A recast refreshes the 30s buff and keeps the slot as it was: it never
/// rearms the shield early.
#[test]
fn a_recast_keeps_the_slot_and_its_cooldown() {
    let mut w = world();
    let (caster, bearer) = units(&mut w);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    take_damage(&mut w, bearer, 10.0);
    frame(&mut w, 0.1);
    w.get_mut::<ActiveAuras>(bearer)
        .unwrap()
        .auras
        .retain(|a| a.effect_type != AuraType::Absorb);
    cast(&mut w, AbilityType::SacredShield, bearer, caster);
    assert_eq!(slots(&w, bearer), 1);
    take_damage(&mut w, bearer, 10.0);
    frame(&mut w, 0.1);
    assert!(shields(&w, bearer).is_empty(), "still on its cooldown");
}

/// The loader holds a triggered absorb to the aura that arms it: a
/// `TriggeredAbsorb` aura with no config, a config on another aura, and a
/// config that cannot fire are each refused; the shipped table passes.
#[test]
fn the_loader_refuses_a_triggered_absorb_on_the_wrong_aura_or_none() {
    use arenasim::states::play_match::ability_config::AbilitiesConfig;
    use arenasim::states::play_match::triggered_absorbs::AbsorbSize;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let shipped = || -> AbilitiesConfig {
        let text = std::fs::read_to_string(arenasim::paths::asset_path("config/abilities.ron"))
            .expect("abilities.ron reads");
        ron::from_str(&text).expect("abilities.ron parses")
    };
    let validates = |config: AbilitiesConfig| {
        catch_unwind(AssertUnwindSafe(|| {
            AbilityDefinitions::new(config).validate().is_ok()
        }))
        .unwrap_or(false)
    };
    assert!(validates(shipped()), "the shipped table");

    let mut bare = shipped();
    let aura = bare.abilities.get_mut(&AbilityType::SacredShield).unwrap();
    aura.applies_aura.as_mut().unwrap().triggered_absorb = None;
    assert!(!validates(bare), "a TriggeredAbsorb aura with no config");

    let mut misplaced = shipped();
    let rider = misplaced.abilities[&AbilityType::SacredShield]
        .applies_aura
        .as_ref()
        .unwrap()
        .triggered_absorb;
    misplaced
        .abilities
        .get_mut(&AbilityType::PowerWordFortitude)
        .unwrap()
        .applies_aura
        .as_mut()
        .unwrap()
        .triggered_absorb = rider;
    assert!(!validates(misplaced), "a config on another aura");

    let mut empty = shipped();
    empty
        .abilities
        .get_mut(&AbilityType::SacredShield)
        .unwrap()
        .applies_aura
        .as_mut()
        .unwrap()
        .triggered_absorb
        .as_mut()
        .unwrap()
        .shield = AbsorbSize::Flat(0.0);
    assert!(!validates(empty), "a shield of nothing");
}

fn holds_shield(world: &World, who: Entity) -> bool {
    world.get::<ActiveAuras>(who).is_some_and(|a| {
        a.auras
            .iter()
            .any(|a| a.effect_type == AuraType::TriggeredAbsorb && a.ability_name == SHIELD)
    })
}

/// One target per Paladin: casting it on a second ally takes it off the
/// first, whose slot then ends; a second Paladin's shield on that ally is its
/// own and stays.
#[test]
fn sacred_shield_is_one_target_per_paladin() {
    let mut w = world();
    let (caster, first) = units(&mut w);
    let second = w
        .spawn((
            Combatant::new(1, 2, CharacterClass::Mage),
            Transform::default(),
            ActiveAuras { auras: Vec::new() },
        ))
        .id();
    cast(&mut w, AbilityType::SacredShield, first, caster);
    assert!(holds_shield(&w, first));
    cast(&mut w, AbilityType::SacredShield, second, caster);
    assert!(holds_shield(&w, second));
    assert!(!holds_shield(&w, first), "moved off the first ally");
    frame(&mut w, 0.1);
    assert_eq!(slots(&w, first), 0, "its slot ends with it");
    assert!(w.resource::<CombatLog>().entries.iter().any(|e| e
        .message
        .contains("Sacred Shield fades (it moved to another target)")));

    // Control: another Paladin's shield on the second ally leaves the first
    // Paladin's on the first.
    let mut w = world();
    let (caster, first) = units(&mut w);
    let other = w
        .spawn((
            Combatant::new(1, 2, CharacterClass::Paladin),
            Transform::default(),
        ))
        .id();
    let second = w
        .spawn((
            Combatant::new(1, 3, CharacterClass::Mage),
            Transform::default(),
            ActiveAuras { auras: Vec::new() },
        ))
        .id();
    cast(&mut w, AbilityType::SacredShield, first, caster);
    cast(&mut w, AbilityType::SacredShield, second, other);
    assert!(holds_shield(&w, first) && holds_shield(&w, second));
}

/// A killing blow raises nothing: the bearer is dead when the system runs, and
/// a corpse gains no shield. Control: the same blow short of lethal raises it.
#[test]
fn a_killing_blow_raises_no_shield() {
    for lethal in [true, false] {
        let mut w = world();
        let (caster, bearer) = units(&mut w);
        cast(&mut w, AbilityType::SacredShield, bearer, caster);
        let health = w.get::<Combatant>(bearer).unwrap().current_health;
        let blow = if lethal { health } else { health - 1.0 };
        take_damage(&mut w, bearer, blow);
        frame(&mut w, 0.1);
        assert_eq!(shields(&w, bearer).is_empty(), lethal, "lethal: {lethal}");
    }
}
