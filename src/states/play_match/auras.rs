//! Aura & Status Effect Systems
//!
//! Handles all status effects (buffs, debuffs, DoTs) applied to combatants.
//! Includes:
//! - Aura duration tracking and expiration
//! - Applying pending auras from abilities
//! - Damage-based aura breaking (e.g., Root breaks on damage)
//! - Damage-over-time (DoT) tick processing

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::match_config;
use super::components::*;
use super::get_next_fct_offset;
use super::combat_core::combatant_id;

/// Update all active auras - tick down durations and remove expired ones.
///
/// IMPORTANT: This system must run AFTER process_dot_ticks so that DoTs can
/// apply their final tick before being removed (WoW-style behavior).
///
/// This system runs every frame to decrement aura durations. When an aura expires,
/// it is removed from the combatant's active aura list.
pub fn update_auras(
    time: Res<Time>,
    mut commands: Commands,
    mut combatants: Query<(Entity, &mut ActiveAuras)>,
) {
    let dt = time.delta_secs();

    for (entity, mut auras) in combatants.iter_mut() {
        // Tick down all aura durations and update fear timers
        for aura in auras.auras.iter_mut() {
            aura.duration -= dt;

            // For Fear auras, tick down direction timer and pick new random direction
            if aura.effect_type == AuraType::Fear {
                aura.fear_direction_timer -= dt;

                // Time to pick a new random direction
                if aura.fear_direction_timer <= 0.0 {
                    // Generate random angle (0 to 2*PI)
                    let angle = rand::random::<f32>() * std::f32::consts::TAU;
                    aura.fear_direction = (angle.cos(), angle.sin());

                    // Reset timer: change direction every 1-2 seconds (WoW-style)
                    aura.fear_direction_timer = 1.0 + rand::random::<f32>();
                }
            }
        }

        // Remove expired auras
        auras.auras.retain(|aura| aura.duration > 0.0);

        // Remove component if no auras remain
        if auras.auras.is_empty() {
            commands.entity(entity).remove::<ActiveAuras>();
        }
    }
}

/// Apply pending auras to targets.
/// 
/// This system runs after casting completes and applies any queued auras
/// to their targets. It handles both new auras and stacking existing auras.
pub fn apply_pending_auras(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_auras: Query<(Entity, &AuraPending)>,
    mut combatants: Query<(&mut Combatant, Option<&mut ActiveAuras>)>,
) {
    for (pending_entity, pending) in pending_auras.iter() {
        // Get target combatant
        let Ok((mut target_combatant, active_auras)) = combatants.get_mut(pending.target) else {
            commands.entity(pending_entity).despawn();
            continue;
        };
        
        // Handle MaxHealthIncrease aura - apply HP buff immediately
        if pending.aura.effect_type == AuraType::MaxHealthIncrease {
            let hp_bonus = pending.aura.magnitude;
            target_combatant.max_health += hp_bonus;
            target_combatant.current_health += hp_bonus; // Give them the extra HP
            
            info!(
                "Team {} {} receives Power Word: Fortitude (+{:.0} max HP, now {:.0}/{:.0})",
                target_combatant.team,
                target_combatant.class.name(),
                hp_bonus,
                target_combatant.current_health,
                target_combatant.max_health
            );
            
            // Log to combat log
            combat_log.log(
                CombatLogEventType::Buff,
                format!(
                    "Team {} {} gains Power Word: Fortitude (+{:.0} max HP)",
                    target_combatant.team,
                    target_combatant.class.name(),
                    hp_bonus
                )
            );
        }
        // Try to get existing auras on target
        if let Some(mut active_auras) = active_auras {
            // Add to existing auras
            active_auras.auras.push(pending.aura.clone());
        } else {
            // No existing auras, insert new component
            commands.entity(pending.target).insert(ActiveAuras {
                auras: vec![pending.aura.clone()],
            });
        }
        
        // Remove the pending aura entity
        commands.entity(pending_entity).despawn();
    }
}

/// Process damage-based aura breaking.
/// 
/// When a combatant takes damage, accumulate it on their breakable auras.
/// If accumulated damage exceeds the break threshold, remove the aura.
pub fn process_aura_breaks(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &Combatant, &mut ActiveAuras, Option<&DamageTakenThisFrame>)>,
) {
    for (entity, combatant, mut active_auras, damage_taken) in combatants.iter_mut() {
        let Some(damage_taken) = damage_taken else {
            continue; // No damage this frame
        };
        
        if damage_taken.amount <= 0.0 {
            continue;
        }
        
        // Track which auras to remove
        let mut auras_to_remove = Vec::new();
        
        // Accumulate damage on breakable auras
        for (index, aura) in active_auras.auras.iter_mut().enumerate() {
            if aura.break_on_damage_threshold > 0.0 {
                aura.accumulated_damage += damage_taken.amount;
                
                // Check if aura should break
                if aura.accumulated_damage >= aura.break_on_damage_threshold {
                    auras_to_remove.push(index);
                    
                    // Log the break
                    let aura_name = match aura.effect_type {
                        AuraType::Root => "Root",
                        AuraType::MovementSpeedSlow => "Movement Speed Slow",
                        AuraType::Stun => "Stun",
                        AuraType::Fear => "Fear",
                        AuraType::MaxHealthIncrease => "Power Word: Fortitude", // Should never break on damage
                        AuraType::DamageOverTime => "Rend", // Should never break on damage (has 0.0 threshold)
                        AuraType::SpellSchoolLockout => "Lockout", // Should never break on damage (has 0.0 threshold)
                        AuraType::HealingReduction => "Mortal Strike", // Should never break on damage (has 0.0 threshold)
                    };
                    
                    let message = format!(
                        "Team {} {}'s {} broke from damage ({:.0}/{:.0})",
                        combatant.team,
                        combatant.class.name(),
                        aura_name,
                        aura.accumulated_damage,
                        aura.break_on_damage_threshold
                    );
                    combat_log.log(CombatLogEventType::MatchEvent, message);
                }
            }
        }
        
        // Remove broken auras (in reverse order to preserve indices)
        for &index in auras_to_remove.iter().rev() {
            active_auras.auras.remove(index);
        }
        
        // Clear damage taken component
        commands.entity(entity).remove::<DamageTakenThisFrame>();
    }
}

/// Process damage-over-time ticks.
///
/// IMPORTANT: This system must run BEFORE update_auras so that the final tick
/// fires exactly when the aura expires (WoW-style DoT behavior). For example,
/// an 18s DoT with 3s ticks will tick at t=3,6,9,12,15,18 (6 total ticks).
///
/// For each combatant with DoT auras:
/// 1. Tick down time_until_next_tick
/// 2. When it reaches 0, apply damage
/// 3. Reset timer for next tick
/// 4. Spawn floating combat text
/// 5. Log to combat log
pub fn process_dot_ticks(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants_with_auras: Query<(Entity, &mut Combatant, &Transform, &mut ActiveAuras)>,
    combatants_without_auras: Query<(Entity, &Combatant), Without<ActiveAuras>>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't tick DoTs during victory celebration (prevents killing winners)
    if celebration.is_some() {
        return;
    }
    let dt = time.delta_secs();
    
    // Build a map of entity -> (team, class) for quick lookups
    // Include BOTH combatants with auras AND combatants without auras (like the Warrior caster)
    let mut combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass)> = 
        combatants_with_auras
            .iter()
            .map(|(entity, combatant, _, _)| (entity, (combatant.team, combatant.class)))
            .collect();
    
    // Add combatants without auras to the map
    for (entity, combatant) in combatants_without_auras.iter() {
        combatant_info.insert(entity, (combatant.team, combatant.class));
    }
    
    // Build a map of entity -> position
    let positions: std::collections::HashMap<Entity, Vec3> = combatants_with_auras
        .iter()
        .map(|(entity, _, transform, _)| (entity, transform.translation))
        .collect();
    
    // Track DoT damage to apply (to avoid borrow issues)
    // Format: (target_entity, caster_entity, damage, target_pos, caster_team, caster_class, ability_name)
    let mut dot_damage_to_apply: Vec<(Entity, Entity, f32, Vec3, u8, match_config::CharacterClass, String)> = Vec::new();
    
    // First pass: tick down DoT timers and queue damage
    for (entity, combatant, _transform, mut active_auras) in combatants_with_auras.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        let target_pos = positions.get(&entity).copied().unwrap_or(Vec3::ZERO);
        
        for aura in active_auras.auras.iter_mut() {
            if aura.effect_type != AuraType::DamageOverTime {
                continue;
            }
            
            // Tick down time until next damage application
            aura.time_until_next_tick -= dt;

            // Check if we should apply damage:
            // 1. Normal tick: time_until_next_tick <= 0
            // 2. Final tick: aura is expiring this frame (duration - dt <= 0) but tick timer hasn't fired
            //    This ensures WoW-style behavior where the final tick happens exactly at expiration
            let normal_tick = aura.time_until_next_tick <= 0.0;
            let final_tick = !normal_tick && (aura.duration - dt) <= 0.0;

            if normal_tick || final_tick {
                // Time to apply DoT damage!
                let damage = aura.magnitude;

                // Get caster info (if still exists)
                if let Some(caster_entity) = aura.caster {
                    if let Some(&(caster_team, caster_class)) = combatant_info.get(&caster_entity) {
                        dot_damage_to_apply.push((
                            entity,
                            caster_entity,
                            damage,
                            target_pos,
                            caster_team,
                            caster_class,
                            aura.ability_name.clone(),
                        ));
                    }
                }

                // Reset tick timer (only for normal ticks, final tick doesn't need reset)
                if normal_tick {
                    aura.time_until_next_tick = aura.tick_interval;
                }
            }
        }
    }
    
    // Track caster damage dealt updates
    let mut caster_damage_updates: Vec<(Entity, f32)> = Vec::new();
    
    // Second pass: apply queued DoT damage to targets
    for (target_entity, caster_entity, damage, target_pos, caster_team, caster_class, ability_name) in dot_damage_to_apply {
        // Get target combatant
        let Ok((_, mut target, _, _)) = combatants_with_auras.get_mut(target_entity) else {
            continue;
        };
        
        if !target.is_alive() {
            continue;
        }
        
        let target_team = target.team;
        let target_class = target.class;
        
        // Apply damage
        let actual_damage = damage.min(target.current_health);
        target.current_health = (target.current_health - damage).max(0.0);
        target.damage_taken += actual_damage;
        
        // Track damage for aura breaking
        commands.entity(target_entity).insert(DamageTakenThisFrame {
            amount: actual_damage,
        });
        
        // Warriors generate Rage from taking damage
        if target.resource_type == ResourceType::Rage {
            let rage_gain = actual_damage * 0.15;
            target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
        }
        
        // Queue caster damage_dealt update
        caster_damage_updates.push((caster_entity, actual_damage));
        
        // Spawn floating combat text (yellow for DoT ticks, like ability damage)
        // Get deterministic offset based on pattern state
        let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
            get_next_fct_offset(&mut fct_state)
        } else {
            (0.0, 0.0)
        };
        commands.spawn((
            FloatingCombatText {
                world_position: target_pos + Vec3::new(offset_x, super::FCT_HEIGHT + offset_y, 0.0),
                text: format!("{:.0}", actual_damage),
                color: egui::Color32::from_rgb(255, 255, 0), // Yellow for ability damage
                lifetime: 1.5,
                vertical_offset: offset_y,
            },
            PlayMatchEntity,
        ));
        
        // Log to combat log with structured data
        let is_killing_blow = !target.is_alive();
        let message = format!(
            "Team {} {}'s {} ticks for {:.0} damage on Team {} {}",
            caster_team,
            caster_class.name(),
            ability_name,
            actual_damage,
            target_team,
            target_class.name()
        );
        combat_log.log_damage(
            combatant_id(caster_team, caster_class),
            combatant_id(target_team, target_class),
            ability_name.clone(),
            actual_damage,
            is_killing_blow,
            message,
        );

        // Log death with killer tracking
        if is_killing_blow {
            let death_message = format!(
                "Team {} {} has been eliminated",
                target_team,
                target_class.name()
            );
            combat_log.log_death(
                combatant_id(target_team, target_class),
                Some(combatant_id(caster_team, caster_class)),
                death_message,
            );
        }
    }
    
    // Third pass: update caster damage_dealt stats
    for (caster_entity, damage_dealt) in caster_damage_updates {
        if let Ok((_, mut caster, _, _)) = combatants_with_auras.get_mut(caster_entity) {
            caster.damage_dealt += damage_dealt;
        }
    }
}

