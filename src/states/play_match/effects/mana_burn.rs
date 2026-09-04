//! Mana Burn Effect Processing
//!
//! Processes the Priest's Mana Burn: destroys mana on an enemy mana user.
//! Uses the ManaBurnPending deferred pattern — the pending is spawned at cast
//! completion in `combat_core/casting.rs` (where the target is borrowed for
//! damage/heal application) and consumed here with clean mutable access.
//!
//! Deliberately NOT scaled by `ArenaDampening`: dampening throttles healing
//! throughput to force match resolution; mana burn is pressure toward the same
//! resolution, so damping it would work against its own purpose.

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::utils::{combatant_id, combat_log_id_for};

/// Process pending Mana Burns.
///
/// Destroys up to `amount` mana on the target, clamped so `current_mana`
/// never goes negative (a debug invariant asserts this). Only
/// `ResourceType::Mana` targets are burned — Warriors reuse `current_mana`
/// as their rage pool and Rogues as energy, and neither is a legal target.
pub fn process_mana_burn(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_burns: Query<(Entity, &ManaBurnPending)>,
    mut combatants: Query<&mut Combatant>,
    pet_query: Query<&Pet>,
    abilities: Res<AbilityDefinitions>,
) {
    // The landing's school comes from the definition, as at every other
    // `SchoolImpact` spawn site, so a RON retune moves the colour with it.
    let school = abilities.get_unchecked(&AbilityType::ManaBurn).spell_school;
    for (pending_entity, pending) in pending_burns.iter() {
        if let Ok(mut target) = combatants.get_mut(pending.target) {
            if target.is_alive() && target.resource_type == ResourceType::Mana {
                let burned = target.current_mana.min(pending.amount);
                target.current_mana -= burned;

                let target_id = combat_log_id_for(&target, pet_query.get(pending.target).ok());
                let msg = format!(
                    "[MANA BURN] {}'s Mana Burn destroys {:.0} mana on {} ({:.0}/{:.0} remaining)",
                    combatant_id(pending.caster_team, pending.caster_slot, pending.caster_class),
                    burned,
                    target_id,
                    target.current_mana,
                    target.max_mana,
                );
                combat_log.log(CombatLogEventType::Buff, msg.clone());
                info!("{}", msg);

                // The shared, school-coloured landing (`rendering/effects/
                // school_impact.rs`), spawned only when mana actually burned —
                // a Mana Burn on an empty pool is not a landing. Magnitude is
                // the fraction of the POOL destroyed, since there is no damage.
                // Deterministic, no `game_rng`; byte-neutral in headless.
                if burned > 0.0 {
                    if let Some(anchor) = SchoolImpact::anchor_for(AbilityType::ManaBurn) {
                        commands.spawn((
                            SchoolImpact {
                                target: pending.target,
                                ability: AbilityType::ManaBurn,
                                school,
                                anchor,
                                from: pending.impact_from,
                                magnitude: burned / target.max_mana.max(1.0),
                                is_crit: false,
                                age: 0.0,
                            },
                            PlayMatchEntity,
                        ));
                    }
                }
            }
        }
        commands.entity(pending_entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;
    use bevy::ecs::system::RunSystemOnce;

    fn new_world() -> World {
        let mut world = World::new();
        world.insert_resource(CombatLog::default());
        world.insert_resource(AbilityDefinitions::default());
        world
    }

    fn spawn_burn(world: &mut World, target: Entity, amount: f32) -> Entity {
        world
            .spawn(ManaBurnPending {
                target,
                amount,
                caster_team: 1,
                caster_slot: 0,
                caster_class: CharacterClass::Priest,
                impact_from: Vec3::X,
            })
            .id()
    }

    #[test]
    fn burns_mana_on_mana_target() {
        let mut world = new_world();
        // Enemy Priest: 150 max mana, starts full.
        let target = world.spawn(Combatant::new(2, 0, CharacterClass::Priest)).id();
        spawn_burn(&mut world, target, 50.0);

        world.run_system_once(process_mana_burn).expect("system ran");

        let combatant = world.get::<Combatant>(target).unwrap();
        assert!((combatant.current_mana - 100.0).abs() < f32::EPSILON);
        // Pending consumed.
        let mut q = world.query::<&ManaBurnPending>();
        assert_eq!(q.iter(&world).count(), 0);
    }

    #[test]
    fn clamps_at_zero_mana() {
        let mut world = new_world();
        let target_entity = world.spawn(Combatant::new(2, 0, CharacterClass::Priest)).id();
        world.get_mut::<Combatant>(target_entity).unwrap().current_mana = 30.0;
        spawn_burn(&mut world, target_entity, 50.0);

        world.run_system_once(process_mana_burn).expect("system ran");

        let combatant = world.get::<Combatant>(target_entity).unwrap();
        assert!(
            combatant.current_mana.abs() < f32::EPSILON,
            "burn larger than remaining mana must clamp to exactly 0, got {}",
            combatant.current_mana
        );
    }

    #[test]
    fn never_burns_rage_or_energy_pools() {
        let mut world = new_world();
        // Warrior reuses current_mana as RAGE; Rogue as ENERGY. Neither may be burned.
        let warrior = world.spawn(Combatant::new(2, 0, CharacterClass::Warrior)).id();
        world.get_mut::<Combatant>(warrior).unwrap().current_mana = 80.0;
        let rogue = world.spawn(Combatant::new(2, 1, CharacterClass::Rogue)).id();

        spawn_burn(&mut world, warrior, 50.0);
        spawn_burn(&mut world, rogue, 50.0);

        world.run_system_once(process_mana_burn).expect("system ran");

        assert!((world.get::<Combatant>(warrior).unwrap().current_mana - 80.0).abs() < f32::EPSILON);
        assert!((world.get::<Combatant>(rogue).unwrap().current_mana - 100.0).abs() < f32::EPSILON);
        // Pendings still consumed even when the burn was refused.
        let mut q = world.query::<&ManaBurnPending>();
        assert_eq!(q.iter(&world).count(), 0);
    }

    #[test]
    fn dead_target_is_untouched() {
        let mut world = new_world();
        let target_entity = world.spawn(Combatant::new(2, 0, CharacterClass::Priest)).id();
        world.get_mut::<Combatant>(target_entity).unwrap().current_health = 0.0;
        spawn_burn(&mut world, target_entity, 50.0);

        world.run_system_once(process_mana_burn).expect("system ran");

        let combatant = world.get::<Combatant>(target_entity).unwrap();
        assert!((combatant.current_mana - 150.0).abs() < f32::EPSILON);
    }
}
