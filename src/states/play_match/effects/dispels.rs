//! Dispel Effect Processing
//!
//! Processes dispel effects from Priest's Dispel Magic, Paladin's Cleanse,
//! and Felhunter's Devour Magic.

use bevy::prelude::*;
use smallvec::SmallVec;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::components::*;
use crate::states::play_match::effects::backlash::BacklashPending;

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
    // Deferred UA backlash spawns. We collect (dispeller, caster, damage) from
    // each removed Unstable Affliction aura inside the dispel-target borrow scope,
    // then resolve the dispeller's team and spawn `BacklashPending` after the
    // borrow is released — avoids `&mut Combatant` aliasing on `combatants`.
    let mut deferred_backlashes: Vec<(Entity, Entity, f32)> = Vec::new();

    for (pending_entity, pending) in pending_dispels.iter() {
        // Get target's auras
        if let Ok((combatant, mut active_auras)) = combatants.get_mut(pending.target) {
            // Find all dispellable aura indices (SmallVec avoids heap allocation for typical aura counts)
            let dispellable_indices: SmallVec<[usize; 8]> = active_auras
                .auras
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    // If aura_type_filter is set, only match those specific types
                    if let Some(ref filter) = pending.aura_type_filter {
                        filter.contains(&a.effect_type)
                    } else {
                        a.can_be_dispelled()
                    }
                })
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

                // Detect Unstable Affliction backlash. Match by ability name string
                // to mirror the pattern used elsewhere (e.g., Corruption / try_corruption).
                // The ability_name field is the canonical source of truth for which
                // ability spawned the aura, even if the same AuraType is reused.
                if removed_aura.ability_name == "Unstable Affliction"
                    && removed_aura.caster.is_some()
                {
                    // Snapshot data needed after the borrow is released.
                    deferred_backlashes.push((
                        pending.dispeller,
                        removed_aura.caster.unwrap(),
                        removed_aura.backlash_damage.unwrap_or(0.0),
                    ));
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

    // Apply deferred Unstable Affliction backlash spawns. Resolved here (after
    // the dispel-target borrow scope) so we can read the dispeller and caster
    // teams via the same `combatants` query without aliasing conflicts.
    //
    // Team-comparison guard: only fire backlash when the dispeller is on a
    // DIFFERENT team than the original UA caster. If a Warlock's own team
    // dispels their UA (e.g., a friendly Priest cleanses to remove a misclick),
    // backlash should NOT fire — UA's penalty exists to deter ENEMY dispels.
    for (dispeller, caster, damage) in deferred_backlashes {
        let Ok((dispeller_combatant, _)) = combatants.get(dispeller) else {
            continue;
        };
        let Ok((caster_combatant, _)) = combatants.get(caster) else {
            // Caster despawned; treat as no-op rather than firing backlash with
            // ambiguous attribution.
            continue;
        };
        if dispeller_combatant.team == caster_combatant.team {
            continue;
        }

        commands.spawn(BacklashPending {
            dispeller,
            damage,
            // Hardcoded MVP value. A future iteration can source this from the
            // ability's DispelBacklashConfig if per-ability tuning is needed.
            silence_duration: 5.0,
            caster,
        });
    }
}
