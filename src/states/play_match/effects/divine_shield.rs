//! Divine Shield Effect Processing
//!
//! Processes Divine Shield activation: purges all debuffs and applies DamageImmunity aura.
//! Uses the DivineShieldPending deferred pattern because Paladin AI has immutable aura access.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::components::*;
use crate::states::play_match::utils::{combatant_id, get_next_fct_offset};

/// Process pending Divine Shield activations.
///
/// When a Paladin activates Divine Shield, a DivineShieldPending component is spawned.
/// This system purges all debuffs, applies the DamageImmunity aura, and logs the activation.
pub fn process_divine_shield(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_shields: Query<(Entity, &DivineShieldPending)>,
    mut combatants: Query<(&Combatant, &Transform, &mut ActiveAuras)>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    for (pending_entity, pending) in pending_shields.iter() {
        if let Ok((combatant, transform, mut active_auras)) = combatants.get_mut(pending.caster) {
            if !combatant.is_alive() {
                commands.entity(pending_entity).despawn();
                continue;
            }

            // Count and purge all debuffs
            let debuffs_removed = active_auras.auras.iter()
                .filter(|a| matches!(a.effect_type,
                    AuraType::MovementSpeedSlow | AuraType::Root | AuraType::Stun |
                    AuraType::DamageOverTime | AuraType::SpellSchoolLockout |
                    AuraType::HealingReduction | AuraType::Fear | AuraType::Polymorph |
                    AuraType::DamageReduction | AuraType::CastTimeIncrease
                ))
                .count();

            active_auras.auras.retain(|a| !matches!(a.effect_type,
                AuraType::MovementSpeedSlow | AuraType::Root | AuraType::Stun |
                AuraType::DamageOverTime | AuraType::SpellSchoolLockout |
                AuraType::HealingReduction | AuraType::Fear | AuraType::Polymorph |
                AuraType::DamageReduction | AuraType::CastTimeIncrease
            ));

            // Apply DamageImmunity aura (12s duration)
            active_auras.auras.push(Aura {
                effect_type: AuraType::DamageImmunity,
                duration: 12.0,
                magnitude: 1.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                break_on_damage_threshold: 0.0,
                accumulated_damage: 0.0,
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                caster: Some(pending.caster),
                ability_name: "Divine Shield".to_string(),
                spell_school: None,
            });

            let caster_id = combatant_id(pending.caster_team, pending.caster_class);

            // Log activation
            combat_log.log(
                CombatLogEventType::Buff,
                format!("{} uses Divine Shield", caster_id),
            );

            // Log debuff removal if any
            if debuffs_removed > 0 {
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "{}'s Divine Shield removes {} debuff{}",
                        caster_id,
                        debuffs_removed,
                        if debuffs_removed > 1 { "s" } else { "" }
                    ),
                );
            }

            info!(
                "Team {} {} activates Divine Shield (removed {} debuffs)",
                pending.caster_team,
                pending.caster_class.name(),
                debuffs_removed
            );

            // Spawn golden "Divine Shield" FCT on the Paladin
            let text_position = transform.translation + Vec3::new(0.0, super::super::FCT_HEIGHT, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.caster) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: "Divine Shield".to_string(),
                    color: egui::Color32::from_rgb(255, 215, 0), // Gold
                    lifetime: 2.0,
                    vertical_offset: offset_y,
                    is_crit: false,
                },
                PlayMatchEntity,
            ));
        }

        // Remove the pending entity
        commands.entity(pending_entity).despawn();
    }
}
