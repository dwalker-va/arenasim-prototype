//! Dispel Effect Processing
//!
//! Processes dispel effects from Priest's Dispel Magic, Paladin's Cleanse,
//! and Felhunter's Devour Magic.

use bevy::prelude::*;
use smallvec::SmallVec;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::class_ai::priest::DispelPending;
use crate::states::play_match::components::*;

/// Process pending dispels from Dispel Magic, Cleanse, or Devour Magic.
///
/// When a dispel is queued, a DispelPending component is spawned. This system
/// finds the target's auras and removes a random dispellable one.
pub fn process_dispels(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_dispels: Query<(Entity, &DispelPending)>,
    mut combatants: Query<(&mut Combatant, &mut ActiveAuras)>,
    mut game_rng: ResMut<GameRng>,
) {
    // Deferred heals to apply after aura processing (avoids borrow conflicts)
    let mut deferred_heals: Vec<(Entity, f32)> = Vec::new();

    for (pending_entity, pending) in pending_dispels.iter() {
        // Get target's auras
        if let Ok((combatant, mut active_auras)) = combatants.get_mut(pending.target) {
            // Find all dispellable aura indices (SmallVec avoids heap allocation for typical aura counts)
            let dispellable_indices: SmallVec<[usize; 8]> = active_auras
                .auras
                .iter()
                .enumerate()
                .filter(|(_, a)| a.can_be_dispelled())
                .map(|(i, _)| i)
                .collect();

            if !dispellable_indices.is_empty() {
                // Randomly select one to remove (WoW Classic behavior)
                let random_idx = (game_rng.random_f32() * dispellable_indices.len() as f32) as usize;
                let idx_to_remove = dispellable_indices[random_idx.min(dispellable_indices.len() - 1)];

                let removed_aura = active_auras.auras.remove(idx_to_remove);

                // Log the dispel using the provided log prefix
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "{} {} removed from Team {} {}",
                        pending.log_prefix,
                        removed_aura.ability_name,
                        combatant.team,
                        combatant.class.name()
                    ),
                );

                info!(
                    "{} {} removed from Team {} {}",
                    pending.log_prefix,
                    removed_aura.ability_name,
                    combatant.team,
                    combatant.class.name()
                );

                // Spawn dispel visual effect
                commands.spawn((
                    DispelBurst {
                        target: pending.target,
                        caster_class: pending.caster_class,
                        lifetime: 0.5,
                        initial_lifetime: 0.5,
                    },
                    PlayMatchEntity,
                ));

                // Queue heal on successful dispel (Felhunter's Devour Magic)
                if let Some((heal_entity, heal_amount)) = pending.heal_on_success {
                    deferred_heals.push((heal_entity, heal_amount));
                }
            }
        }

        // Remove the pending dispel entity
        commands.entity(pending_entity).despawn();
    }

    // Apply deferred heals (Devour Magic self-heal)
    for (heal_entity, heal_amount) in deferred_heals {
        if let Ok((mut heal_combatant, _)) = combatants.get_mut(heal_entity) {
            if !heal_combatant.is_alive() {
                continue;
            }
            let old_hp = heal_combatant.current_health;
            heal_combatant.current_health = (old_hp + heal_amount).min(heal_combatant.max_health);
            let actual_heal = heal_combatant.current_health - old_hp;
            if actual_heal > 0.0 {
                combat_log.log(
                    CombatLogEventType::Healing,
                    format!(
                        "[DEVOUR] Team {} {} heals for {:.0} from Devour Magic",
                        heal_combatant.team,
                        heal_combatant.class.name(),
                        actual_heal
                    ),
                );
            }
        }
    }
}
