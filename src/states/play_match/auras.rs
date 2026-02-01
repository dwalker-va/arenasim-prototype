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
use super::class_ai::priest::DispelPending;
use super::class_ai::paladin::{PaladinDispelPending, HolyShockHealPending, HolyShockDamagePending};
use super::utils::{combatant_id, get_next_fct_offset};
use super::ability_config::AbilityDefinitions;
use super::abilities::AbilityType;

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
    mut game_rng: ResMut<GameRng>,
    mut combatants: Query<(Entity, &mut ActiveAuras)>,
) {
    let dt = time.delta_secs();

    for (entity, mut auras) in combatants.iter_mut() {
        // Tick down all aura durations and update fear timers
        for aura in auras.auras.iter_mut() {
            aura.duration -= dt;

            // For Fear and Polymorph auras, tick down direction timer and pick new random direction
            if matches!(aura.effect_type, AuraType::Fear | AuraType::Polymorph) {
                aura.fear_direction_timer -= dt;

                // Time to pick a new random direction
                if aura.fear_direction_timer <= 0.0 {
                    // Generate random angle (0 to 2*PI) using seeded RNG
                    let angle = game_rng.random_f32() * std::f32::consts::TAU;
                    aura.fear_direction = (angle.cos(), angle.sin());

                    // Reset timer: change direction every 1-2 seconds (WoW-style)
                    // Polymorph changes direction slightly less frequently (sheep wander lazily)
                    let base_timer = if aura.effect_type == AuraType::Polymorph { 1.5 } else { 1.0 };
                    aura.fear_direction_timer = base_timer + game_rng.random_f32();
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
///
/// CC immunity: Combatants who are charging are immune to crowd control effects.
/// When a CC would be applied to a charging target, "Immune" floating text is shown.
pub fn apply_pending_auras(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_auras: Query<(Entity, &AuraPending)>,
    mut combatants: Query<(&mut Combatant, Option<&mut ActiveAuras>, &Transform)>,
    charging_query: Query<&ChargingState>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    use std::collections::{HashSet, HashMap};

    // Track which buff auras we've applied this frame to prevent stacking
    // Key: (target_entity, buff_key) where buff_key identifies the specific buff
    // For absorbs: "absorb:{ability_name}", for others: "type:{AuraType}"
    let mut applied_buffs: HashSet<(Entity, String)> = HashSet::new();

    // Track auras to add for entities that don't have ActiveAuras component yet
    // This prevents multiple insert() calls from overwriting each other
    let mut new_auras_map: HashMap<Entity, Vec<Aura>> = HashMap::new();

    for (pending_entity, pending) in pending_auras.iter() {
        // Invariant: aura duration should be positive
        debug_assert!(
            pending.aura.duration > 0.0,
            "apply_pending_auras: aura '{}' has non-positive duration ({})",
            pending.aura.ability_name,
            pending.aura.duration
        );

        // Invariant: tick interval should be non-negative (0 means no ticking)
        debug_assert!(
            pending.aura.tick_interval >= 0.0,
            "apply_pending_auras: aura '{}' has negative tick_interval ({})",
            pending.aura.ability_name,
            pending.aura.tick_interval
        );

        // Get target combatant
        let Ok((mut target_combatant, active_auras, target_transform)) = combatants.get_mut(pending.target) else {
            commands.entity(pending_entity).despawn();
            continue;
        };

        // Check for CC immunity: Charging combatants are immune to crowd control
        let is_cc_aura = matches!(
            pending.aura.effect_type,
            AuraType::Fear | AuraType::Stun | AuraType::Root | AuraType::Polymorph
        );
        let is_charging = charging_query.get(pending.target).is_ok();

        if is_cc_aura && is_charging {
            // Target is immune - show floating text and log
            let text_position = target_transform.translation + Vec3::new(0.0, 2.5, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.target) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };

            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: "Immune".to_string(),
                    color: egui::Color32::YELLOW,
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                },
                PlayMatchEntity,
            ));

            // Log to combat log
            let cc_name = match pending.aura.effect_type {
                AuraType::Fear => "Fear",
                AuraType::Stun => "Stun",
                AuraType::Root => "Root",
                AuraType::Polymorph => "Polymorph",
                _ => "CC",
            };
            combat_log.log(
                CombatLogEventType::MatchEvent,
                format!(
                    "Team {} {}'s {} is immune (charging)",
                    target_combatant.team,
                    target_combatant.class.name(),
                    cc_name
                )
            );
            info!(
                "Team {} {} is immune to {} (charging)",
                target_combatant.team,
                target_combatant.class.name(),
                cc_name
            );

            commands.entity(pending_entity).despawn();
            continue;
        }

        // Check if target already has this buff type (prevent stacking for buff auras)
        // Also includes Absorb shields and WeakenedSoul to prevent same-frame double-application
        // Note: Different Absorb abilities (Ice Barrier vs PW:S) CAN coexist - only same ability is blocked
        let is_buff_aura = matches!(
            pending.aura.effect_type,
            AuraType::MaxHealthIncrease | AuraType::MaxManaIncrease | AuraType::AttackPowerIncrease
            | AuraType::Absorb | AuraType::WeakenedSoul
        );
        if is_buff_aura {
            // For Absorb shields, use ability_name as the key to allow different absorbs to coexist
            // For other buffs, use the aura type
            let buff_key: String = if pending.aura.effect_type == AuraType::Absorb {
                format!("absorb:{}", pending.aura.ability_name)
            } else {
                format!("type:{:?}", pending.aura.effect_type)
            };

            // Check if we already applied this specific buff to this target THIS FRAME
            if applied_buffs.contains(&(pending.target, buff_key.clone())) {
                commands.entity(pending_entity).despawn();
                continue;
            }

            // Check if target already has this specific buff from a PREVIOUS frame
            let already_has_buff_existing = if let Some(ref auras) = active_auras {
                if pending.aura.effect_type == AuraType::Absorb {
                    // For absorbs, check same ability name
                    auras.auras.iter().any(|a|
                        a.effect_type == AuraType::Absorb && a.ability_name == pending.aura.ability_name
                    )
                } else {
                    // For other buffs, check same effect type
                    auras.auras.iter().any(|a| a.effect_type == pending.aura.effect_type)
                }
            } else {
                false
            };

            // Also check auras we're accumulating this frame for entities without ActiveAuras
            let already_has_buff_new = if let Some(new_auras) = new_auras_map.get(&pending.target) {
                if pending.aura.effect_type == AuraType::Absorb {
                    new_auras.iter().any(|a|
                        a.effect_type == AuraType::Absorb && a.ability_name == pending.aura.ability_name
                    )
                } else {
                    new_auras.iter().any(|a| a.effect_type == pending.aura.effect_type)
                }
            } else {
                false
            };

            if already_has_buff_existing || already_has_buff_new {
                // Skip - target already has this buff
                commands.entity(pending_entity).despawn();
                continue;
            }

            // Mark this buff as applied for this frame
            applied_buffs.insert((pending.target, buff_key));
        }

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

        // Handle MaxManaIncrease aura (Arcane Intellect) - apply mana buff immediately
        if pending.aura.effect_type == AuraType::MaxManaIncrease {
            let mana_bonus = pending.aura.magnitude;
            target_combatant.max_mana += mana_bonus;
            target_combatant.current_mana += mana_bonus; // Give them the extra mana

            info!(
                "Team {} {} receives Arcane Intellect (+{:.0} max mana, now {:.0}/{:.0})",
                target_combatant.team,
                target_combatant.class.name(),
                mana_bonus,
                target_combatant.current_mana,
                target_combatant.max_mana
            );

            // Log to combat log
            combat_log.log(
                CombatLogEventType::Buff,
                format!(
                    "Team {} {} gains Arcane Intellect (+{:.0} max mana)",
                    target_combatant.team,
                    target_combatant.class.name(),
                    mana_bonus
                )
            );
        }

        // Handle AttackPowerIncrease aura (Battle Shout) - apply AP buff immediately
        if pending.aura.effect_type == AuraType::AttackPowerIncrease {
            let ap_bonus = pending.aura.magnitude;
            target_combatant.attack_power += ap_bonus;

            info!(
                "Team {} {} receives Battle Shout (+{:.0} attack power, now {:.0})",
                target_combatant.team,
                target_combatant.class.name(),
                ap_bonus,
                target_combatant.attack_power
            );

            // Log to combat log
            combat_log.log(
                CombatLogEventType::Buff,
                format!(
                    "Team {} {} gains Battle Shout (+{:.0} attack power)",
                    target_combatant.team,
                    target_combatant.class.name(),
                    ap_bonus
                )
            );
        }

        // Add aura to target
        if let Some(mut active_auras) = active_auras {
            // Add to existing ActiveAuras component
            active_auras.auras.push(pending.aura.clone());
        } else {
            // Entity doesn't have ActiveAuras yet - accumulate in our map
            // This prevents multiple insert() calls from overwriting each other
            new_auras_map
                .entry(pending.target)
                .or_insert_with(Vec::new)
                .push(pending.aura.clone());
        }

        // Remove the pending aura entity
        commands.entity(pending_entity).despawn();
    }

    // Now insert ActiveAuras components for entities that didn't have them
    for (entity, auras) in new_auras_map {
        commands.entity(entity).insert(ActiveAuras { auras });
    }
}

/// Process pending dispels from Dispel Magic.
///
/// When a Priest casts Dispel Magic, a DispelPending component is spawned.
/// This system finds the target's auras and removes a random dispellable one.
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
            // Find all dispellable aura indices
            let dispellable_indices: Vec<usize> = active_auras
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

                // Log the dispel
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "[DISPEL] {} removed from Team {} {}",
                        removed_aura.ability_name,
                        combatant.team,
                        combatant.class.name()
                    ),
                );

                info!(
                    "[DISPEL] {} removed from Team {} {}",
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
        // Note: threshold of 0.0 means "break on ANY damage" (e.g., Polymorph)
        // threshold of -1.0 or negative means "never break on damage"
        for (index, aura) in active_auras.auras.iter_mut().enumerate() {
            if aura.break_on_damage_threshold >= 0.0 {
                aura.accumulated_damage += damage_taken.amount;

                // Check if aura should break (threshold 0 = break on any damage)
                if aura.accumulated_damage > aura.break_on_damage_threshold {
                    auras_to_remove.push(index);
                    
                    // Log the break - use the ability name stored on the aura
                    let aura_name = if aura.ability_name.is_empty() {
                        // Fallback for auras without ability names
                        match aura.effect_type {
                            AuraType::Root => "Root",
                            AuraType::MovementSpeedSlow => "Slow",
                            AuraType::Stun => "Stun",
                            AuraType::Fear => "Fear",
                            AuraType::Polymorph => "Polymorph",
                            _ => "Effect",
                        }
                    } else {
                        aura.ability_name.as_str()
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
        let Ok((_, mut target, _, mut target_auras)) = combatants_with_auras.get_mut(target_entity) else {
            continue;
        };

        if !target.is_alive() {
            continue;
        }

        let target_team = target.team;
        let target_class = target.class;

        // Apply damage with absorb shield consideration
        let (actual_damage, absorbed) = super::combat_core::apply_damage_with_absorb(
            damage,
            &mut target,
            Some(&mut target_auras),
        );

        // Track damage for aura breaking (only actual damage, not absorbed)
        commands.entity(target_entity).insert(DamageTakenThisFrame {
            amount: actual_damage,
        });

        // Warriors generate Rage from taking damage (only on actual health damage)
        if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
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

        // Spawn light blue floating combat text for absorbed damage
        if absorbed > 0.0 {
            let (absorb_offset_x, absorb_offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: target_pos + Vec3::new(absorb_offset_x, super::FCT_HEIGHT + absorb_offset_y, 0.0),
                    text: format!("{:.0} absorbed", absorbed),
                    color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                    lifetime: 1.5,
                    vertical_offset: absorb_offset_y,
                },
                PlayMatchEntity,
            ));
        }

        // Log to combat log with structured data
        let is_killing_blow = !target.is_alive();
        let message = if absorbed > 0.0 {
            format!(
                "Team {} {}'s {} ticks for {:.0} damage on Team {} {} ({:.0} absorbed)",
                caster_team,
                caster_class.name(),
                ability_name,
                actual_damage,
                target_team,
                target_class.name(),
                absorbed
            )
        } else {
            format!(
                "Team {} {}'s {} ticks for {:.0} damage on Team {} {}",
                caster_team,
                caster_class.name(),
                ability_name,
                actual_damage,
                target_team,
                target_class.name()
            )
        };
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

/// Process pending Paladin dispels (Cleanse).
///
/// When a Paladin casts Cleanse, a PaladinDispelPending component is spawned.
/// This system finds the target's auras and removes a random dispellable one.
/// (Same logic as Priest's Dispel Magic)
pub fn process_paladin_dispels(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_dispels: Query<(Entity, &PaladinDispelPending)>,
    mut combatants: Query<(&Combatant, &mut ActiveAuras)>,
    mut game_rng: ResMut<GameRng>,
) {
    for (pending_entity, pending) in pending_dispels.iter() {
        // Get target's auras
        if let Ok((combatant, mut active_auras)) = combatants.get_mut(pending.target) {
            // Find all dispellable aura indices
            let dispellable_indices: Vec<usize> = active_auras
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

                // Log the dispel
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "[CLEANSE] {} removed from Team {} {}",
                        removed_aura.ability_name,
                        combatant.team,
                        combatant.class.name()
                    ),
                );

                info!(
                    "[CLEANSE] {} removed from Team {} {}",
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
            let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
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
                },
                PlayMatchEntity,
            ));

            // Log the heal with caster attribution
            let caster_id = combatant_id(pending.caster_team, pending.caster_class);
            combat_log.log(
                CombatLogEventType::Healing,
                format!(
                    "{}'s Holy Shock heals Team {} {} for {:.0}",
                    caster_id,
                    target_team,
                    target_class.name(),
                    actual_heal
                ),
            );

            info!(
                "Team {} {}'s Holy Shock heals Team {} {} for {:.0} ({:.0}/{:.0})",
                pending.caster_team,
                pending.caster_class.name(),
                target_team,
                target_class.name(),
                actual_heal,
                target.current_health,
                target.max_health
            );
        } else {
            // Target entity no longer exists - clean up orphaned pending
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
            let raw_damage = base_damage + spell_power_bonus;

            // Apply damage with absorb shield consideration
            let (actual_damage, absorbed) = super::combat_core::apply_damage_with_absorb(
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
            let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
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
                    },
                    PlayMatchEntity,
                ));
            }

            // Log damage with caster attribution
            let caster_id = combatant_id(pending.caster_team, pending.caster_class);
            let is_killing_blow = !target.is_alive();
            let message = if absorbed > 0.0 {
                format!(
                    "{}'s Holy Shock hits Team {} {} for {:.0} damage ({:.0} absorbed)",
                    caster_id,
                    target_team,
                    target_class.name(),
                    actual_damage,
                    absorbed
                )
            } else {
                format!(
                    "{}'s Holy Shock hits Team {} {} for {:.0} damage",
                    caster_id,
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
        } else {
            // Target entity no longer exists - clean up orphaned pending
        }

        // Remove the pending damage entity
        commands.entity(pending_entity).despawn();
    }
}

