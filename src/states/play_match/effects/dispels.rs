//! Dispel Effect Processing
//!
//! Processes dispel effects from Priest's Dispel Magic and Paladin's Cleanse.

use bevy::prelude::*;
use smallvec::SmallVec;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::class_ai::priest::DispelPending;
use crate::states::play_match::components::*;

/// Process pending dispels from Dispel Magic or Cleanse.
///
/// When a Priest casts Dispel Magic or a Paladin casts Cleanse, a DispelPending
/// component is spawned. This system finds the target's auras and removes a
/// random dispellable one.
pub fn process_dispels(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_dispels: Query<(Entity, &DispelPending)>,
    mut combatants: Query<(&Combatant, &mut ActiveAuras)>,
    mut game_rng: ResMut<GameRng>,
) {
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
            }
        }

        // Remove the pending dispel entity
        commands.entity(pending_entity).despawn();
    }
}
