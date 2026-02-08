//! Holy Shock Effect Processing
//!
//! Processes instant Holy Shock healing and damage effects.
//! Holy Shock is unique in that it can be used offensively or defensively.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::CRIT_HEALING_MULTIPLIER;
use crate::states::play_match::constants::CRIT_DAMAGE_MULTIPLIER;
use crate::states::play_match::utils::{combatant_id, get_next_fct_offset};

/// Process pending Holy Shock heals.
///
/// When a Paladin casts Holy Shock on an ally, a HolyShockHealPending component is spawned.
/// This system applies the healing to the target.
pub fn process_holy_shock_heals(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    abilities: Res<AbilityDefinitions>,
    pending_heals: Query<(Entity, &HolyShockHealPending)>,
    mut combatants: Query<(&mut Combatant, &Transform, Option<&ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    let ability_def = abilities.get_unchecked(&AbilityType::HolyShock);

    for (pending_entity, pending) in pending_heals.iter() {
        // Get target combatant
        if let Ok((mut target, target_transform, target_auras)) = combatants.get_mut(pending.target) {
            if !target.is_alive() {
                commands.entity(pending_entity).despawn();
                continue;
            }

            // Calculate healing amount using ability config
            let base_heal = ability_def.healing_base_min
                + game_rng.random_f32() * (ability_def.healing_base_max - ability_def.healing_base_min);
            let spell_power_bonus = pending.caster_spell_power * ability_def.healing_coefficient;
            let mut heal_amount = base_heal + spell_power_bonus;

            // Roll crit before reductions
            let is_crit = super::super::combat_core::roll_crit(pending.caster_crit_chance, &mut game_rng);
            if is_crit {
                heal_amount *= CRIT_HEALING_MULTIPLIER;
            }

            // Check for healing reduction debuffs (e.g., Mortal Strike)
            if let Some(auras) = target_auras {
                for aura in &auras.auras {
                    if aura.effect_type == AuraType::HealingReduction {
                        // Magnitude is a multiplier (e.g., 0.65 = 35% reduction)
                        heal_amount *= aura.magnitude;
                    }
                }
            }

            let old_health = target.current_health;
            target.current_health = (target.current_health + heal_amount).min(target.max_health);
            let actual_heal = target.current_health - old_health;

            let target_team = target.team;
            let target_class = target.class;

            // Spawn floating combat text (green for healing)
            let text_position = target_transform.translation + Vec3::new(0.0, super::super::FCT_HEIGHT, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.target) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("+{:.0}", actual_heal),
                    color: egui::Color32::from_rgb(0, 255, 0), // Green for healing
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit,
                },
                PlayMatchEntity,
            ));

            // Log the heal with caster attribution
            let caster_id = combatant_id(pending.caster_team, pending.caster_class);
            let verb = if is_crit { "CRITICALLY heals" } else { "heals" };
            let message = format!(
                "{}'s Holy Shock {} Team {} {} for {:.0}",
                caster_id,
                verb,
                target_team,
                target_class.name(),
                actual_heal
            );
            combat_log.log_healing(
                caster_id.clone(),
                combatant_id(target_team, target_class),
                "Holy Shock".to_string(),
                actual_heal,
                is_crit,
                message,
            );
        }

        // Remove the pending heal entity
        commands.entity(pending_entity).despawn();
    }
}

/// Process pending Holy Shock damage.
///
/// When a Paladin casts Holy Shock on an enemy, a HolyShockDamagePending component is spawned.
/// This system applies the damage to the target.
pub fn process_holy_shock_damage(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    abilities: Res<AbilityDefinitions>,
    pending_damage: Query<(Entity, &HolyShockDamagePending)>,
    mut combatants: Query<(&mut Combatant, &Transform, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    let ability_def = abilities.get_unchecked(&AbilityType::HolyShock);

    for (pending_entity, pending) in pending_damage.iter() {
        // Get target combatant
        if let Ok((mut target, target_transform, mut target_auras)) = combatants.get_mut(pending.target) {
            if !target.is_alive() {
                commands.entity(pending_entity).despawn();
                continue;
            }

            // Calculate damage amount using ability config
            let base_damage = ability_def.damage_base_min
                + game_rng.random_f32() * (ability_def.damage_base_max - ability_def.damage_base_min);
            let spell_power_bonus = pending.caster_spell_power * ability_def.damage_coefficient;
            let mut raw_damage = base_damage + spell_power_bonus;

            // Roll crit before absorbs/reductions
            let is_crit = super::super::combat_core::roll_crit(pending.caster_crit_chance, &mut game_rng);
            if is_crit {
                raw_damage *= CRIT_DAMAGE_MULTIPLIER;
            }

            // Apply damage with absorb shield consideration
            let (actual_damage, absorbed) = super::super::combat_core::apply_damage_with_absorb(
                raw_damage,
                &mut target,
                target_auras.as_deref_mut(),
            );

            let target_team = target.team;
            let target_class = target.class;

            // Track damage for aura breaking
            commands.entity(pending.target).insert(DamageTakenThisFrame {
                amount: actual_damage,
            });

            // Warriors generate Rage from taking damage
            if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                let rage_gain = actual_damage * 0.15;
                target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
            }

            // Spawn floating combat text (yellow for ability damage)
            let text_position = target_transform.translation + Vec3::new(0.0, super::super::FCT_HEIGHT, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.target) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("{:.0}", actual_damage),
                    color: egui::Color32::from_rgb(255, 255, 0), // Yellow for ability damage
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit,
                },
                PlayMatchEntity,
            ));

            // Spawn absorbed text if applicable
            if absorbed > 0.0 {
                let (absorb_offset_x, absorb_offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.target) {
                    get_next_fct_offset(&mut fct_state)
                } else {
                    (0.0, 0.0)
                };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(absorb_offset_x, absorb_offset_y, 0.0),
                        text: format!("{:.0} absorbed", absorbed),
                        color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                        lifetime: 1.5,
                        vertical_offset: absorb_offset_y,
                        is_crit: false,
                    },
                    PlayMatchEntity,
                ));
            }

            // Log damage with caster attribution
            let caster_id = combatant_id(pending.caster_team, pending.caster_class);
            let is_killing_blow = !target.is_alive();
            let verb = if is_crit { "CRITS" } else { "hits" };
            let message = if absorbed > 0.0 {
                format!(
                    "{}'s Holy Shock {} Team {} {} for {:.0} damage ({:.0} absorbed)",
                    caster_id,
                    verb,
                    target_team,
                    target_class.name(),
                    actual_damage,
                    absorbed
                )
            } else {
                format!(
                    "{}'s Holy Shock {} Team {} {} for {:.0} damage",
                    caster_id,
                    verb,
                    target_team,
                    target_class.name(),
                    actual_damage
                )
            };
            combat_log.log_damage(
                caster_id.clone(),
                combatant_id(target_team, target_class),
                "Holy Shock".to_string(),
                actual_damage,
                is_killing_blow,
                is_crit,
                message,
            );

            // Log death if killing blow
            if is_killing_blow {
                let death_message = format!(
                    "Team {} {} has been eliminated by {}'s Holy Shock",
                    target_team,
                    target_class.name(),
                    caster_id
                );
                combat_log.log_death(
                    combatant_id(target_team, target_class),
                    Some(caster_id),
                    death_message,
                );
            }
        }

        // Remove the pending damage entity
        commands.entity(pending_entity).despawn();
    }
}
