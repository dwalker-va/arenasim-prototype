//! The Frost Armor chill is ONE debuff, and a dispel takes all of it.
//!
//! The unit tests beside `ActiveAuras` prove the removal helpers take a whole
//! compound. This drives the real `process_dispels` system in a Bevy app
//! instead, because the helper is only half the claim: the other half is that
//! the dispel PATH calls it, on the aura it actually rolled, and that the
//! rider — which is not dispellable in its own right and never appears in the
//! candidate set — leaves with it.
//!
//! That asymmetry is the bug the card names. Before the chill's two effects
//! were bound into one debuff, the movement slow was a dispel candidate and
//! the attack-speed slow was not, so Devour Magic or Master's Call lifted half
//! a debuff and left the other half standing under the same name.

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::combat_core::{
    effective_attack_interval, frost_armor_chill_auras,
};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, CompoundDebuff, DispelPending, GameRng,
};
use arenasim::states::play_match::effects::process_dispels;
use arenasim::CharacterClass;

/// A minimal app holding just what `process_dispels` reads.
fn harness() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(CombatLog::default());
    app.insert_resource(GameRng::from_seed(7));
    app.add_systems(Update, process_dispels);
    app
}

fn chilled_victim(app: &mut App, extra: Vec<Aura>) -> Entity {
    let mut auras: Vec<Aura> = frost_armor_chill_auras().into();
    auras.extend(extra);
    app.world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            ActiveAuras { auras },
        ))
        .id()
}

fn queue_dispel(app: &mut App, target: Entity, filter: Option<Vec<AuraType>>) {
    let dispeller = app
        .world_mut()
        .spawn(Combatant::new(1, 1, CharacterClass::Warlock))
        .id();
    app.world_mut().spawn(DispelPending {
        target,
        dispeller,
        log_prefix: "[DEVOUR]",
        caster_class: CharacterClass::Warlock,
        heal_on_success: None,
        aura_type_filter: filter,
        removes_poison: false,
    });
}

fn remaining(app: &App, entity: Entity) -> Vec<AuraType> {
    app.world()
        .entity(entity)
        .get::<ActiveAuras>()
        .map(|auras| auras.auras.iter().map(|a| a.effect_type).collect())
        .unwrap_or_default()
}

/// The headline. An unfiltered dispel (Dispel Magic, Cleanse, Devour Magic)
/// can only ever roll the chill's movement slow — the attack-speed rider is
/// not a candidate — and both effects come off together.
#[test]
fn devour_magic_takes_the_whole_chill() {
    let mut app = harness();
    let victim = chilled_victim(&mut app, vec![]);
    queue_dispel(&mut app, victim, None);
    app.update();

    assert!(
        remaining(&app, victim).is_empty(),
        "a dispel left {:?} of the chill behind",
        remaining(&app, victim)
    );
}

/// Master's Call pins its filter to movement impairments, so it reaches the
/// chill by its FACE and nothing else. It still takes the whole debuff: the
/// rider is part of the thing being removed, not a separate debuff that
/// happens to share a name.
#[test]
fn masters_call_takes_the_whole_chill() {
    let mut app = harness();
    let victim = chilled_victim(&mut app, vec![]);
    queue_dispel(
        &mut app,
        victim,
        Some(vec![AuraType::Root, AuraType::MovementSpeedSlow]),
    );
    app.update();

    assert!(
        remaining(&app, victim).is_empty(),
        "Master's Call left {:?} of the chill behind",
        remaining(&app, victim)
    );
}

/// The effect the rider carries is real, and removing it is what the dispel
/// now buys: a chilled attacker swings slower, and a dispelled one does not.
/// Asserted on the swing timer rather than on the aura list, so the test would
/// fail if the rider were removed from the debuff outright instead of bound
/// into it.
#[test]
fn the_rider_is_what_slows_the_swing() {
    let mut app = harness();
    let victim = chilled_victim(&mut app, vec![]);

    let chilled = {
        let entity = app.world().entity(victim);
        effective_attack_interval(
            entity.get::<Combatant>().expect("combatant"),
            entity.get::<ActiveAuras>(),
        )
    };

    queue_dispel(&mut app, victim, None);
    app.update();

    let entity = app.world().entity(victim);
    let combatant = entity.get::<Combatant>().expect("combatant");
    let freed = effective_attack_interval(combatant, entity.get::<ActiveAuras>());

    assert!(
        chilled > freed,
        "the chill must stretch the swing interval ({chilled} vs {freed})"
    );
    assert_eq!(
        freed,
        1.0 / combatant.attack_speed,
        "after the dispel the attacker swings at its base cadence"
    );
}

/// A dispel takes ONE debuff, not every debuff. The compound must not become a
/// vacuum that clears unrelated auras sharing the target.
#[test]
fn only_the_rolled_debuff_leaves() {
    let mut app = harness();
    let unrelated = Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 12.0,
        magnitude: 10.0,
        ability_name: "Rend".to_string(),
        ..Default::default()
    };
    let victim = chilled_victim(&mut app, vec![unrelated]);
    queue_dispel(&mut app, victim, Some(vec![AuraType::MovementSpeedSlow]));
    app.update();

    assert_eq!(remaining(&app, victim), vec![AuraType::DamageOverTime]);
}

/// The apply site hands out both halves bound to the same debuff. Pinned here
/// as well as in the components' own tests because this file is where a reader
/// looking for "what makes the dispel take both" lands.
#[test]
fn the_proc_applies_one_debuff_with_two_effects() {
    let [face, rider] = frost_armor_chill_auras();
    assert_eq!(face.compound, Some(CompoundDebuff::FrostArmorChill));
    assert_eq!(rider.compound, face.compound);
    assert_eq!(face.ability_name, rider.ability_name);
    assert!(!face.is_compound_rider());
    assert!(rider.is_compound_rider());
    assert!(face.can_be_dispelled(), "the chill comes off by its face");
    assert!(
        !rider.can_be_dispelled(),
        "the rider is not independently dispellable — widening \
         is_magic_dispellable for AttackSpeedSlow was the alternative, and it \
         would have made Demoralizing Shout dispellable as magic too"
    );
}
