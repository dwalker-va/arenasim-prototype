//! Divine Shield Effect Processing
//!
//! Processes Divine Shield activation: purges all debuffs and applies DamageImmunity aura.
//! Uses the DivineShieldPending deferred pattern because Paladin AI has immutable aura access.

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::abilities::AbilityType;
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
    mut combatants: Query<(&Combatant, &Transform, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    for (pending_entity, pending) in pending_shields.iter() {
        if let Ok((combatant, transform, active_auras_opt)) = combatants.get_mut(pending.caster) {
            if !combatant.is_alive() {
                commands.entity(pending_entity).despawn();
                continue;
            }
            // The immunity aura is data-driven from `abilities.ron`
            // (DivineShield's `applies_aura`), read from the Paladin's own
            // book. Fallbacks preserve the historical values if the config is
            // missing.
            let ds_def = combatant.book().get_unchecked(&AbilityType::DivineShield);
            // The aura, the log lines and the floating text all carry the
            // ability's RON `name` — the name the renderer keys the bubble on.
            let ds_name = ds_def.name.as_str();
            let ds_aura = ds_def.applies_aura.as_ref();
            let immunity_duration = ds_aura.map(|a| a.duration).unwrap_or(12.0);
            let immunity_magnitude = ds_aura.map(|a| a.magnitude).unwrap_or(1.0);
            let immunity_break = ds_aura.map(|a| a.break_on_damage).unwrap_or(-1.0);

            let immunity_aura = Aura {
                effect_type: AuraType::DamageImmunity,
                duration: immunity_duration,
                magnitude: immunity_magnitude,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                break_on_damage_threshold: immunity_break, // -1.0 default = never break on damage
                accumulated_damage: 0.0,
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                caster: Some(pending.caster),
                ability_name: ds_name.to_string(),
                spell_school: None,
                applied_this_frame: false,
                backlash_damage: None,
                dr_category_override: None,
                dispel_type: DispelType::Auto,
                compound: None,
                source_item: None,
                source_talent: None,
                next_cast: None,
                leech: None,
                stacks: None,
                bloom: None,
            };

            let debuffs_removed = if let Some(mut active_auras) = active_auras_opt {
                // Purge all debuffs and count how many were removed
                // Note: No stat reversal needed here — AP/crit/mana-regen debuffs are dynamic
                // (removing the aura is sufficient), and MaxHealth/MaxMana are always friendly buffs
                // that won't be purged.
                let before = active_auras.auras.len();
                // Classification lives on `Aura::is_hostile_effect`, which is
                // an EXHAUSTIVE match. This was an inline allowlist here, and it
                // silently missed `Incapacitate` when that variant was added —
                // Freezing Trap survived the bubble while the log still claimed
                // debuffs were removed. Keep the decision at the type, where the
                // compiler enforces that new aura types get classified.
                active_auras.auras.retain(|a| !a.is_hostile_effect());
                let removed = before - active_auras.auras.len();
                active_auras.auras.push(immunity_aura);
                removed
            } else {
                // No auras yet — insert new ActiveAuras with DamageImmunity
                // Note: .chain() auto-inserts ApplyDeferred, so this is visible to apply_pending_auras
                commands.entity(pending.caster).insert(ActiveAuras {
                    auras: vec![immunity_aura],
                });
                0
            };

            let caster_id = combatant_id(
                pending.caster_team,
                pending.caster_slot,
                pending.caster_class,
            );

            // Log activation
            combat_log.log(
                CombatLogEventType::Buff,
                format!("{} uses {}", caster_id, ds_name),
            );

            // Log debuff removal if any
            if debuffs_removed > 0 {
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "{}'s {} removes {} debuff{}",
                        caster_id,
                        ds_name,
                        debuffs_removed,
                        if debuffs_removed > 1 { "s" } else { "" }
                    ),
                );
            }

            info!(
                "Team {} {} activates {} (removed {} debuffs)",
                pending.caster_team,
                pending.caster_class.name(),
                ds_name,
                debuffs_removed
            );

            // Spawn golden "Divine Shield" FCT on the Paladin
            let text_position =
                transform.translation + Vec3::new(0.0, super::super::FCT_HEIGHT, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.caster)
            {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: ds_name.to_string(),
                    color: crate::ui::theme::fct::INVULNERABLE,
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
