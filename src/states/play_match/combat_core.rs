//! Combat Core Systems
//!
//! Handles core combat mechanics:
//! - Movement (move_to_target, kiting logic)
//! - Auto-attacks (melee and ranged wand attacks)
//! - Resource regeneration (Energy, Rage)
//! - Casting (cast time processing, completion)
//! - Interrupt processing (applying lockouts)
//! - Stealth visuals

use std::collections::HashMap;
use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::match_config;
use super::components::*;
use super::abilities::{AbilityType, SpellSchool};
use super::ability_config::AbilityDefinitions;
use super::constants::{CRIT_DAMAGE_MULTIPLIER, CRIT_HEALING_MULTIPLIER};
use super::utils::{spawn_speech_bubble, get_next_fct_offset};
use super::components::ChannelingState;
use super::{MELEE_RANGE, ARENA_HALF_X, ARENA_HALF_Z};

// Re-export combatant_id for backward compatibility (used by other modules)
pub use super::utils::combatant_id;

/// Roll a critical strike check. Returns true if the roll is a crit.
pub fn roll_crit(crit_chance: f32, rng: &mut GameRng) -> bool {
    rng.random_f32() < crit_chance
}

/// Apply damage to a combatant, accounting for absorb shields.
/// Returns (actual_damage_to_health, damage_absorbed).
///
/// If the target has an Absorb aura, damage is first subtracted from the shield.
/// Any remaining damage is applied to health. Depleted shields are removed.
///
/// # Panics (debug only)
/// Panics if damage is negative (damage should always be >= 0).
pub fn apply_damage_with_absorb(
    damage: f32,
    target: &mut Combatant,
    active_auras: Option<&mut ActiveAuras>,
) -> (f32, f32) {
    // Invariant: damage should never be negative
    debug_assert!(
        damage >= 0.0,
        "apply_damage_with_absorb: damage cannot be negative, got {}",
        damage
    );

    // Invariant: target health should be valid before we modify it
    debug_assert!(
        target.current_health >= 0.0,
        "apply_damage_with_absorb: target health already negative ({})",
        target.current_health
    );

    // Check for damage immunity (Divine Shield) — blocks all incoming damage
    if let Some(ref auras) = active_auras {
        if auras.auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity) {
            return (0.0, 0.0);
        }
    }

    let mut remaining_damage = damage;
    let mut total_absorbed = 0.0;

    // First, apply damage taken reduction (e.g., Devotion Aura)
    // Multiple reductions stack multiplicatively (two 10% reductions = 19% total)
    if let Some(ref auras) = active_auras {
        for aura in auras.auras.iter() {
            if aura.effect_type == AuraType::DamageTakenReduction && remaining_damage > 0.0 {
                remaining_damage *= 1.0 - aura.magnitude;
            }
        }
    }

    // Check for absorb shields and consume them
    if let Some(auras) = active_auras {
        for aura in auras.auras.iter_mut() {
            if aura.effect_type == AuraType::Absorb && remaining_damage > 0.0 {
                // Invariant: absorb shield magnitude should be positive
                debug_assert!(
                    aura.magnitude >= 0.0,
                    "apply_damage_with_absorb: absorb shield has negative magnitude ({})",
                    aura.magnitude
                );

                let absorb_amount = aura.magnitude.min(remaining_damage);
                aura.magnitude -= absorb_amount;
                remaining_damage -= absorb_amount;
                total_absorbed += absorb_amount;
            }
        }
        // Remove depleted absorb shields
        auras.auras.retain(|a| !(a.effect_type == AuraType::Absorb && a.magnitude <= 0.0));
    }

    // Apply remaining damage to health
    let actual_damage = remaining_damage.min(target.current_health);
    target.current_health = (target.current_health - remaining_damage).max(0.0);
    target.damage_taken += actual_damage;

    // Post-condition: health should still be valid
    debug_assert!(
        target.current_health >= 0.0,
        "apply_damage_with_absorb: health went negative after damage"
    );

    (actual_damage, total_absorbed)
}

/// Check if a combatant has an absorb shield active
pub fn has_absorb_shield(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::Absorb))
}

/// Check if a combatant has Weakened Soul (cannot receive Power Word: Shield)
pub fn has_weakened_soul(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::WeakenedSoul))
}

/// Get the physical damage reduction multiplier from DamageReduction auras on the attacker.
/// Used by Curse of Weakness to reduce outgoing physical damage by a percentage.
/// Returns the percentage reduction (0.2 = 20% less damage).
/// Multiple reductions stack additively (two 20% reductions = 40% total).
pub fn get_physical_damage_reduction(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::DamageReduction)
            .map(|aura| aura.magnitude)
            .sum()
    })
}

/// Get the total cast time increase from CastTimeIncrease auras on a combatant.
/// Used by Curse of Tongues to slow casting.
/// Returns the percentage increase (0.5 = 50% slower, so multiply cast time by 1.5).
pub fn get_cast_time_increase(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::CastTimeIncrease)
            .map(|aura| aura.magnitude)
            .sum()
    })
}

/// Check if a combatant has damage immunity (Divine Shield active)
pub fn has_damage_immunity(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity))
}

/// Returns the outgoing damage multiplier for the caster.
/// If caster has DamageImmunity (Divine Shield), returns DIVINE_SHIELD_DAMAGE_PENALTY (0.5).
/// Otherwise returns 1.0 (no penalty).
pub fn get_divine_shield_damage_penalty(auras: Option<&ActiveAuras>) -> f32 {
    if has_damage_immunity(auras) {
        super::constants::DIVINE_SHIELD_DAMAGE_PENALTY
    } else {
        1.0
    }
}

/// Calculate the modified cast time accounting for CastTimeIncrease auras.
/// This should be called when starting a cast to get the actual cast duration.
pub fn calculate_cast_time(base_cast_time: f32, auras: Option<&ActiveAuras>) -> f32 {
    if base_cast_time <= 0.0 {
        return 0.0; // Instant casts aren't affected
    }
    let cast_time_increase = get_cast_time_increase(auras);
    base_cast_time * (1.0 + cast_time_increase)
}

fn find_best_kiting_direction(
    current_pos: Vec3,
    enemy_pos: Vec3,
    move_distance: f32,
) -> Vec3 {
    // Calculate ideal direction (directly away from enemy)
    let ideal_direction = Vec3::new(
        current_pos.x - enemy_pos.x,
        0.0,
        current_pos.z - enemy_pos.z,
    ).normalize_or_zero();
    
    if ideal_direction == Vec3::ZERO {
        return Vec3::ZERO; // Already on top of enemy, can't kite
    }
    
    // Check if ideal direction keeps us in bounds
    let ideal_next_pos = current_pos + ideal_direction * move_distance;
    let ideal_in_bounds =
        ideal_next_pos.x >= -ARENA_HALF_X && ideal_next_pos.x <= ARENA_HALF_X &&
        ideal_next_pos.z >= -ARENA_HALF_Z && ideal_next_pos.z <= ARENA_HALF_Z;
    
    if ideal_in_bounds {
        return ideal_direction; // Ideal direction works, use it!
    }
    
    // Ideal direction would hit boundary - find best alternative
    // Test 16 directions around a circle and pick the one that:
    // 1. Stays in bounds
    // 2. Maximizes distance from enemy
    let mut best_direction = Vec3::ZERO;
    let mut best_score = f32::MIN;
    
    for i in 0..16 {
        let angle = (i as f32) * std::f32::consts::TAU / 16.0;
        let candidate_direction = Vec3::new(
            angle.cos(),
            0.0,
            angle.sin(),
        );
        
        // Calculate where we'd end up with this direction
        let candidate_next_pos = current_pos + candidate_direction * move_distance;
        
        // Check if this keeps us in bounds
        let in_bounds =
            candidate_next_pos.x >= -ARENA_HALF_X && candidate_next_pos.x <= ARENA_HALF_X &&
            candidate_next_pos.z >= -ARENA_HALF_Z && candidate_next_pos.z <= ARENA_HALF_Z;
        
        if !in_bounds {
            continue; // Skip directions that go out of bounds
        }
        
        // Score this direction based on:
        // 1. Distance from enemy (higher = better)
        // 2. Alignment with ideal direction (bonus for moving away, not sideways)
        let distance_from_enemy = candidate_next_pos.distance(enemy_pos);
        let alignment_with_ideal = candidate_direction.dot(ideal_direction).max(0.0);
        let score = distance_from_enemy * 2.0 + alignment_with_ideal * 5.0;
        
        if score > best_score {
            best_score = score;
            best_direction = candidate_direction;
        }
    }
    
    best_direction
}

pub fn move_to_target(
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    mut commands: Commands,
    mut combatants: Query<(Entity, &mut Transform, &Combatant, Option<&ActiveAuras>, Option<&CastingState>, Option<&ChargingState>, Option<&ChannelingState>)>,
    orbs: Query<&Transform, (With<ShadowSightOrb>, Without<Combatant>)>,
    pet_query: Query<&Pet>,
) {
    // Don't allow movement until gates open
    if !countdown.gates_opened {
        return;
    }
    
    let dt = time.delta_secs();
    
    // Build a snapshot of all combatant positions and team info for lookups
    let positions: std::collections::HashMap<Entity, (Vec3, u8)> = combatants
        .iter()
        .map(|(entity, transform, combatant, _, _, _, _)| (entity, (transform.translation, combatant.team)))
        .collect();

    // Move each combatant towards their target if needed
    for (entity, mut transform, combatant, auras, casting_state, charging_state, channeling_state) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Cannot move while casting (WoW mechanic)
        if casting_state.is_some() {
            continue;
        }

        // Cannot move while channeling (WoW mechanic)
        if channeling_state.is_some() {
            continue;
        }

        // Check for movement-preventing CC and wandering CC
        let (is_rooted_or_stunned, fear_direction, polymorph_direction) = if let Some(auras) = auras {
            let rooted_or_stunned = auras.auras.iter().any(|a| matches!(a.effect_type, AuraType::Root | AuraType::Stun));
            let fear_dir = auras.auras.iter()
                .find(|a| a.effect_type == AuraType::Fear)
                .map(|a| a.fear_direction);
            let poly_dir = auras.auras.iter()
                .find(|a| a.effect_type == AuraType::Polymorph)
                .map(|a| a.fear_direction); // Polymorph reuses fear_direction for wandering
            (rooted_or_stunned, fear_dir, poly_dir)
        } else {
            (false, None, None)
        };

        // Cannot move at all if rooted or stunned
        if is_rooted_or_stunned {
            continue;
        }

        let my_pos = transform.translation;

        // FEARED BEHAVIOR: If feared, run in random direction (ignoring normal movement)
        if let Some((dir_x, dir_z)) = fear_direction {
            let direction = Vec3::new(dir_x, 0.0, dir_z).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Feared targets run at normal movement speed (no slows applied during fear)
                let move_distance = combatant.base_movement_speed * dt;

                // Move in fear direction
                transform.translation += direction * move_distance;

                // Clamp to arena bounds
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);

                // Rotate to face direction of travel
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while feared
        }

        // POLYMORPH BEHAVIOR: If polymorphed, wander slowly (like Fear but at 50% speed)
        if let Some((dir_x, dir_z)) = polymorph_direction {
            let direction = Vec3::new(dir_x, 0.0, dir_z).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Polymorphed targets wander at 20% of normal movement speed (sheep waddle slowly!)
                let move_distance = combatant.base_movement_speed * 0.2 * dt;

                // Move in polymorph direction
                transform.translation += direction * move_distance;

                // Clamp to arena bounds
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);

                // Rotate to face direction of travel
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while polymorphed
        }

        // CHARGING BEHAVIOR: If charging, move at high speed toward target ignoring slows
        if let Some(charge_state) = charging_state {
            let Some(&(target_pos, _)) = positions.get(&charge_state.target) else {
                // Target doesn't exist, cancel charge
                commands.entity(entity).remove::<ChargingState>();
                continue;
            };

            let distance = my_pos.distance(target_pos);

            // If we've reached melee range, end the charge
            if distance <= MELEE_RANGE {
                commands.entity(entity).remove::<ChargingState>();

                info!(
                    "Team {} {} completes charge!",
                    combatant.team,
                    combatant.class.name()
                );

                continue; // Will use normal movement/combat next frame
            }

            // Calculate direction to target
            let direction = Vec3::new(
                target_pos.x - my_pos.x,
                0.0,
                target_pos.z - my_pos.z,
            ).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Charge speed: 4x normal movement speed, ignores slows
                const CHARGE_SPEED_MULTIPLIER: f32 = 4.0;
                let charge_speed = combatant.base_movement_speed * CHARGE_SPEED_MULTIPLIER;
                let move_distance = charge_speed * dt;

                // Move towards target
                transform.translation += direction * move_distance;

                // Clamp position to arena bounds
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);
                
                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }
            
            continue; // Skip normal movement logic while charging
        }
        
        // KITING BEHAVIOR: If kiting_timer > 0, move away from nearest enemy
        // Uses intelligent pathfinding that considers arena boundaries
        if combatant.kiting_timer > 0.0 {
            // Find nearest enemy
            let mut nearest_enemy_pos: Option<Vec3> = None;
            let mut nearest_distance = f32::MAX;
            
            for (other_entity, &(other_pos, other_team)) in positions.iter() {
                if *other_entity != entity && other_team != combatant.team {
                    let distance = my_pos.distance(other_pos);
                    if distance < nearest_distance {
                        nearest_distance = distance;
                        nearest_enemy_pos = Some(other_pos);
                    }
                }
            }
            
            // Intelligent kiting: maximize distance from nearest enemy
            if let Some(enemy_pos) = nearest_enemy_pos {
                // Calculate effective movement speed (base * aura modifiers)
                let mut movement_speed = combatant.base_movement_speed;
                if let Some(auras) = auras {
                    for aura in &auras.auras {
                        if aura.effect_type == AuraType::MovementSpeedSlow {
                            movement_speed *= aura.magnitude;
                        }
                    }
                }
                
                let move_distance = movement_speed * dt;
                
                // Find the best direction to move that maximizes distance from enemy
                // while staying within arena bounds
                let best_direction = find_best_kiting_direction(
                    my_pos,
                    enemy_pos,
                    move_distance,
                );
                
                if best_direction != Vec3::ZERO {
                    // Move in the best direction
                    transform.translation += best_direction * move_distance;

                    // Ensure we stay in bounds (in case of floating point errors)
                    transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
                    transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);

                    // Rotate to face direction of travel
                    let target_rotation = Quat::from_rotation_y(best_direction.x.atan2(best_direction.z));
                    transform.rotation = target_rotation;
                }
            }

            continue; // Skip normal movement logic
        }

        // NORMAL MOVEMENT: Get target position
        let Some(target_entity) = combatant.target else {
            // Pets with no target follow their owner
            if let Ok(pet) = pet_query.get(entity) {
                if let Some(&(owner_pos, _)) = positions.get(&pet.owner) {
                    let dist_to_owner = my_pos.distance(owner_pos);
                    if dist_to_owner > 3.0 {
                        let direction = Vec3::new(
                            owner_pos.x - my_pos.x, 0.0, owner_pos.z - my_pos.z,
                        ).normalize_or_zero();
                        if direction != Vec3::ZERO {
                            let mut movement_speed = combatant.base_movement_speed;
                            if let Some(auras) = auras {
                                for aura in &auras.auras {
                                    if aura.effect_type == AuraType::MovementSpeedSlow {
                                        movement_speed *= aura.magnitude;
                                    }
                                }
                            }
                            let move_distance = movement_speed * dt;
                            transform.translation += direction * move_distance;
                            transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
                            transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);
                            let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                            transform.rotation = target_rotation;
                        }
                    }
                }
                continue;
            }

            // No target available (likely facing all-stealth team)
            // When orbs are spawned, ALL combatants (even stealthed) should seek them
            // to break the stalemate. This represents accepting the reveal to gain vision.

            // Find nearest Shadow Sight orb (if any exist)
            let nearest_orb_pos = orbs.iter()
                .map(|orb_transform| orb_transform.translation)
                .min_by(|a, b| {
                    let dist_a = my_pos.distance(*a);
                    let dist_b = my_pos.distance(*b);
                    dist_a.partial_cmp(&dist_b).unwrap()
                });

            // Determine destination: nearest orb if available, otherwise center
            let destination = nearest_orb_pos.unwrap_or(Vec3::ZERO);
            let distance_to_destination = my_pos.distance(destination);

            // Only move if we're far from destination (> 2.5 units for orbs, > 5 units for center)
            let stop_distance = if nearest_orb_pos.is_some() { 2.5 } else { 5.0 };
            if distance_to_destination > stop_distance {
                let direction = Vec3::new(
                    destination.x - my_pos.x,
                    0.0,
                    destination.z - my_pos.z,
                ).normalize_or_zero();

                if direction != Vec3::ZERO {
                    // Calculate effective movement speed
                    let mut movement_speed = combatant.base_movement_speed;
                    if let Some(auras) = auras {
                        for aura in &auras.auras {
                            if aura.effect_type == AuraType::MovementSpeedSlow {
                                movement_speed *= aura.magnitude;
                            }
                        }
                    }

                    // Move towards destination
                    let move_distance = movement_speed * dt;
                    transform.translation += direction * move_distance;

                    // Clamp position to arena bounds
                    transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
                    transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);

                    // Rotate to face destination
                    let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                    transform.rotation = target_rotation;
                }
            }

            continue;
        };
        
        let Some(&(target_pos, _)) = positions.get(&target_entity) else {
            continue;
        };
        
        let distance = my_pos.distance(target_pos);

        // Use class-specific preferred range, or pet-type preferred range for pets
        let stop_distance = if let Ok(pet) = pet_query.get(entity) {
            pet.pet_type.preferred_range()
        } else {
            combatant.class.preferred_range()
        };

        // If out of range, move towards target
        if distance > stop_distance {
            // Calculate direction to target (only in XZ plane, keep Y constant)
            let direction = Vec3::new(
                target_pos.x - my_pos.x,
                0.0, // Don't move vertically
                target_pos.z - my_pos.z,
            ).normalize_or_zero();
            
            if direction != Vec3::ZERO {
                // Calculate effective movement speed (base * aura modifiers)
                let mut movement_speed = combatant.base_movement_speed;
                if let Some(auras) = auras {
                    for aura in &auras.auras {
                        if aura.effect_type == AuraType::MovementSpeedSlow {
                            movement_speed *= aura.magnitude;
                        }
                    }
                }
                
                // Move towards target
                let move_distance = movement_speed * dt;
                transform.translation += direction * move_distance;

                // Clamp position to arena bounds
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);

                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }
        }
    }
}

/// Update visual appearance of stealthed combatants.
/// 
/// Makes stealthed Rogues semi-transparent (40% alpha) with a darker tint
/// to clearly indicate their stealth status. When they break stealth (e.g., by using Ambush),
/// they return to full opacity and original color.
pub fn update_stealth_visuals(
    combatants: Query<(&Combatant, &MeshMaterial3d<StandardMaterial>), Changed<Combatant>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (combatant, material_handle) in combatants.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let current_color = material.base_color.to_srgba();
            let current_alpha = current_color.alpha;
            
            if combatant.stealthed {
                // Only apply stealth effect if not already stealthed (alpha is 1.0)
                if current_alpha >= 0.9 {
                    // Semi-transparent with darker tint for stealth
                    let color = Color::srgba(
                        current_color.red * 0.6,
                        current_color.green * 0.6,
                        current_color.blue * 0.6,
                        0.4, // 40% opacity
                    );
                    material.base_color = color;
                }
            } else {
                // Only restore if currently stealthed (alpha is low)
                if current_alpha < 0.9 {
                    // Restore original color by reversing the darkening (divide by 0.6)
                    let color = Color::srgba(
                        (current_color.red / 0.6).min(1.0),
                        (current_color.green / 0.6).min(1.0),
                        (current_color.blue / 0.6).min(1.0),
                        1.0, // Full opacity
                    );
                    material.base_color = color;
                }
            }
        }
    }
}

/// Auto-attack system: Process attacks based on attack speed timers.
/// 
/// Each combatant has an attack timer that counts up. When it reaches
/// the attack interval (1.0 / attack_speed), they check if they're in
/// range and attack their target.
/// 
/// **Range Check**: Only melee attacks for now, must be within MELEE_RANGE.
/// **WoW Mechanic**: Cannot auto-attack while casting (checked via `CastingState`).
/// 
/// Damage is applied immediately and stats are updated for both attacker and target.
/// All attacks are logged to the combat log for display.
pub fn combat_auto_attack(
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&CastingState>, Option<&ChannelingState>, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
    auto_attack_pet_query: Query<&Pet>,
) {
    // Don't deal damage during victory celebration
    if celebration.is_some() {
        return;
    }
    let dt = time.delta_secs();
    
    // Update match time in combat log (starts from beginning, including prep phase)
    combat_log.match_time += dt;
    
    // Don't allow auto-attacks until gates open
    if !countdown.gates_opened {
        return;
    }
    
    // Build a snapshot of positions for range checks
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _, _, _)| (entity, transform.translation))
        .collect();

    // Build a snapshot of combatant info for logging
    // Tuple: (team, class, display_name) — display_name is "Felhunter" for pets, class name otherwise
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass, String)> = combatants
        .iter()
        .map(|(entity, _, combatant, _, _, _)| {
            let display_name = if let Ok(pet) = auto_attack_pet_query.get(entity) {
                pet.pet_type.name().to_string()
            } else {
                combatant.class.name().to_string()
            };
            (entity, (combatant.team, combatant.class, display_name))
        })
        .collect();
    
    // Collect attacks that will happen this frame (attacker, target, damage)
    let mut attacks = Vec::new();
    
    // Track damage per target for batching floating combat text
    let mut damage_per_target: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    // Track damage per target for aura breaking
    let mut damage_per_aura_break: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    
    for (attacker_entity, transform, mut combatant, casting_state, channeling_state, auras) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        // WoW Mechanic: Cannot auto-attack while stunned, feared, or polymorphed
        let is_incapacitated = super::utils::is_incapacitated(auras.as_deref());
        if is_incapacitated {
            continue;
        }

        // WoW Mechanic: Cannot auto-attack while casting
        if casting_state.is_some() {
            continue;
        }

        // WoW Mechanic: Cannot auto-attack while channeling
        if channeling_state.is_some() {
            continue;
        }
        
        // WoW Mechanic: Cannot auto-attack while stealthed (Rogues must use abilities)
        if combatant.stealthed {
            continue;
        }

        // Update attack timer
        combatant.attack_timer += dt;

        // Check if ready to attack and has a target
        let attack_interval = 1.0 / combatant.attack_speed;
        if combatant.attack_timer >= attack_interval {
            if let Some(target_entity) = combatant.target {
                // Check if target is in range before attacking
                if let Some(&target_pos) = positions.get(&target_entity) {
                    let my_pos = transform.translation;
                    
                    if combatant.in_attack_range(my_pos, target_pos) {
                        // Calculate total damage (base + bonus from Heroic Strike, etc.)
                        let base_damage = combatant.attack_damage + combatant.next_attack_bonus_damage;
                        // Roll crit before damage reduction
                        let is_crit = roll_crit(combatant.crit_chance, &mut game_rng);
                        let crit_damage = if is_crit { base_damage * CRIT_DAMAGE_MULTIPLIER } else { base_damage };
                        // Apply physical damage reduction from curses (Curse of Weakness: -20%)
                        let damage_reduction = get_physical_damage_reduction(auras.as_deref());
                        // Apply Divine Shield outgoing damage penalty (50%)
                        let ds_penalty = get_divine_shield_damage_penalty(auras.as_deref());
                        let total_damage = (crit_damage * (1.0 - damage_reduction) * ds_penalty).max(0.0);
                        let has_bonus = combatant.next_attack_bonus_damage > 0.0;

                        attacks.push((attacker_entity, target_entity, total_damage, has_bonus, is_crit));
                        combatant.attack_timer = 0.0;
                        
                        // Consume the bonus damage after queueing the attack
                        combatant.next_attack_bonus_damage = 0.0;
                        
                        // Break stealth on auto-attack
                        if combatant.stealthed {
                            combatant.stealthed = false;
                            info!(
                                "Team {} {} breaks stealth with auto-attack!",
                                combatant.team,
                                combatant.class.name()
                            );
                        }
                        
                        // Warriors generate Rage from auto-attacks
                        if combatant.resource_type == ResourceType::Rage {
                            let rage_gain = 10.0; // Gain 10 rage per auto-attack
                            combatant.current_mana = (combatant.current_mana + rage_gain).min(combatant.max_mana);
                        }
                    }
                    // If not in range, timer keeps building up so they attack immediately when in range
                }
            }
        }
    }

    // Apply damage to targets and track damage dealt
    let mut damage_dealt_updates: Vec<(Entity, f32)> = Vec::new();
    let mut absorbed_per_target: HashMap<Entity, f32> = HashMap::new();

    // Track which combatants have died during this frame's attack processing
    // This prevents dead combatants from dealing damage after being killed
    let mut died_this_frame: std::collections::HashSet<Entity> = std::collections::HashSet::new();

    // Track crit status per target for FCT display (auto-attacks batch into damage_per_target)
    let mut crit_per_target: HashMap<Entity, bool> = HashMap::new();

    // Build a map of targets with breakable CC from friendly casters.
    // Key: target entity, Value: team of the CC caster.
    // This prevents friendly auto-attacks from breaking their own team's Polymorph/Fear.
    let mut friendly_cc_team: HashMap<Entity, u8> = HashMap::new();
    for (entity, _, combatant, _, _, auras) in combatants.iter() {
        if let Some(auras) = auras {
            for aura in &auras.auras {
                // Only care about CC auras that break on damage
                if aura.break_on_damage_threshold >= 0.0
                    && matches!(aura.effect_type, AuraType::Polymorph | AuraType::Fear)
                {
                    // Look up the caster's team
                    if let Some(caster_entity) = aura.caster {
                        if let Some(&(caster_team, _, _)) = combatant_info.get(&caster_entity) {
                            // Only track if the CC is from the opposing team of the target
                            // (i.e., the CC caster is an enemy of the CC'd target)
                            if caster_team != combatant.team {
                                friendly_cc_team.insert(entity, caster_team);
                            }
                        }
                    }
                }
            }
        }
    }

    for (attacker_entity, target_entity, damage, has_bonus, is_crit) in attacks {
        // If any attack to this target crits, mark the FCT as crit
        crit_per_target.entry(target_entity).and_modify(|c| *c = *c || is_crit).or_insert(is_crit);
        // Bug fix: Don't allow attacks from combatants who died earlier this frame
        // This can happen when two combatants attack each other in the same frame
        if died_this_frame.contains(&attacker_entity) {
            continue;
        }

        // Bug fix: Don't auto-attack targets with breakable CC from a friendly caster.
        // This prevents, e.g., a Warlock pet from breaking its team's Polymorph.
        if let Some(&cc_caster_team) = friendly_cc_team.get(&target_entity) {
            if let Some(&(attacker_team, _, _)) = combatant_info.get(&attacker_entity) {
                if attacker_team == cc_caster_team {
                    continue;
                }
            }
        }

        if let Ok((_, _, mut target, _, _, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let (actual_damage, absorbed) = apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Track damage for aura breaking (only actual damage, not absorbed)
                *damage_per_aura_break.entry(target_entity).or_insert(0.0) += actual_damage;

                // Batch damage for floating combat text (sum all damage to same target)
                *damage_per_target.entry(target_entity).or_insert(0.0) += actual_damage;
                *absorbed_per_target.entry(target_entity).or_insert(0.0) += absorbed;

                // Collect attacker damage for later update (include absorbed damage - attacker dealt it)
                damage_dealt_updates.push((attacker_entity, actual_damage + absorbed));

                // Log the attack with structured data
                if let (Some((attacker_team, attacker_class, attacker_name)), Some((target_team, _target_class, target_name))) =
                    (combatant_info.get(&attacker_entity), combatant_info.get(&target_entity)) {
                    let attack_name = if has_bonus {
                        "Heroic Strike" // Enhanced auto-attack
                    } else if attacker_class.is_melee() {
                        "Auto Attack"
                    } else {
                        "Wand Shot"
                    };
                    let verb = if is_crit { "CRITS" } else { "hits" };
                    let message = if absorbed > 0.0 {
                        format!(
                            "Team {} {}'s {} {} Team {} {} for {:.0} damage ({:.0} absorbed)",
                            attacker_team,
                            attacker_name,
                            attack_name,
                            verb,
                            target_team,
                            target_name,
                            actual_damage,
                            absorbed
                        )
                    } else {
                        format!(
                            "Team {} {}'s {} {} Team {} {} for {:.0} damage",
                            attacker_team,
                            attacker_name,
                            attack_name,
                            verb,
                            target_team,
                            target_name,
                            actual_damage
                        )
                    };

                    let attacker_id = format!("Team {} {}", attacker_team, attacker_name);
                    let target_id = format!("Team {} {}", target_team, target_name);

                    let is_killing_blow = !target.is_alive();
                    combat_log.log_damage(
                        attacker_id.clone(),
                        target_id.clone(),
                        attack_name.to_string(),
                        actual_damage + absorbed, // Total damage dealt (including absorbed)
                        is_killing_blow,
                        is_crit,
                        message,
                    );

                    // Log death with killer tracking (only on first death to prevent duplicates)
                    if is_killing_blow {
                        // Track that this target died - prevents them from dealing damage
                        // if they had a queued attack later in this frame
                        died_this_frame.insert(target_entity);

                        // Mark target as dead to prevent duplicate death processing across systems
                        let was_already_dead = if let Ok((_, _, mut dead_target, _, _, _)) = combatants.get_mut(target_entity) {
                            let already = dead_target.is_dead;
                            dead_target.is_dead = true;
                            already
                        } else {
                            true // entity gone, treat as already dead
                        };

                        if was_already_dead {
                            continue;
                        }

                        // Cancel any in-progress cast or channel so dead combatants can't finish spells
                        commands.entity(target_entity).remove::<CastingState>();
                        commands.entity(target_entity).remove::<ChannelingState>();

                        let death_message = format!(
                            "Team {} {} has been eliminated",
                            target_team,
                            target_name
                        );
                        combat_log.log_death(
                            target_id,
                            Some(attacker_id),
                            death_message,
                        );
                    }
                }
            }
        }
    }
    
    // Spawn floating combat text for each target that took damage (batched)
    for (target_entity, total_damage) in damage_per_target {
        let target_was_crit = crit_per_target.get(&target_entity).copied().unwrap_or(false);
        if let Some(&target_pos) = positions.get(&target_entity) {
            // Spawn floating text slightly above the combatant
            let text_position = target_pos + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);

            // Get deterministic offset based on pattern state
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                // Fallback to center if state not found
                (0.0, 0.0)
            };

            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("{:.0}", total_damage),
                    color: egui::Color32::WHITE, // White for auto-attacks
                    lifetime: 1.5, // Display for 1.5 seconds
                    vertical_offset: offset_y,
                    is_crit: target_was_crit,
                },
                PlayMatchEntity,
            ));

            // Spawn light blue floating combat text for absorbed damage
            if let Some(&total_absorbed) = absorbed_per_target.get(&target_entity) {
                if total_absorbed > 0.0 {
                    let (absorb_offset_x, absorb_offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                        get_next_fct_offset(&mut fct_state)
                    } else {
                        (0.0, 0.0)
                    };
                    commands.spawn((
                        FloatingCombatText {
                            world_position: text_position + Vec3::new(absorb_offset_x, absorb_offset_y, 0.0),
                            text: format!("{:.0} absorbed", total_absorbed),
                            color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                            lifetime: 1.5,
                            vertical_offset: absorb_offset_y,
                            is_crit: false,
                        },
                        PlayMatchEntity,
                    ));
                }
            }
        }
    }
    
    // Update attacker damage dealt stats
    for (attacker_entity, damage) in damage_dealt_updates {
        if let Ok((_, _, mut attacker, _, _, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += damage;
        }
    }
    
    // Track damage for aura breaking
    for (target_entity, total_damage) in damage_per_aura_break {
        commands.entity(target_entity).insert(DamageTakenThisFrame {
            amount: total_damage,
        });
    }
}

/// Resource regeneration system: Regenerate mana for all combatants.
/// 
/// Each combatant with mana regeneration gains mana per second up to their max.
/// Also ticks down ability cooldowns over time.
pub fn regenerate_resources(
    time: Res<Time>,
    mut combatants: Query<&mut Combatant>,
) {
    let dt = time.delta_secs();
    
    for mut combatant in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Regenerate mana/resources
        if combatant.mana_regen > 0.0 {
            combatant.current_mana = (combatant.current_mana + combatant.mana_regen * dt).min(combatant.max_mana);
        }
        
        // Tick down ability cooldowns
        let abilities_on_cooldown: Vec<AbilityType> = combatant.ability_cooldowns.keys().copied().collect();
        for ability in abilities_on_cooldown {
            if let Some(cooldown) = combatant.ability_cooldowns.get_mut(&ability) {
                *cooldown -= dt;
                if *cooldown <= 0.0 {
                    combatant.ability_cooldowns.remove(&ability);
                }
            }
        }
        
        // Tick down global cooldown
        if combatant.global_cooldown > 0.0 {
            combatant.global_cooldown -= dt;
            if combatant.global_cooldown < 0.0 {
                combatant.global_cooldown = 0.0;
            }
        }
        
        // Tick down kiting timer
        if combatant.kiting_timer > 0.0 {
            combatant.kiting_timer -= dt;
            if combatant.kiting_timer < 0.0 {
                combatant.kiting_timer = 0.0;
            }
        }
    }
}

/// Process interrupt attempts: interrupt target's cast or channel and apply spell school lockout.
pub fn process_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    interrupts: Query<(Entity, &InterruptPending)>,
    mut casting_targets: Query<(&mut CastingState, &Combatant), Without<ChannelingState>>,
    mut channeling_targets: Query<(&mut ChannelingState, &Combatant), Without<CastingState>>,
    combatants: Query<&Combatant>,
    pet_query: Query<&Pet>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't process interrupts during victory celebration
    if celebration.is_some() {
        return;
    }

    for (interrupt_entity, interrupt) in interrupts.iter() {
        let mut interrupted = false;

        // Check if target is casting
        if let Ok((mut cast_state, target_combatant)) = casting_targets.get_mut(interrupt.target) {
            // Don't interrupt if already interrupted
            if !cast_state.interrupted {
                // Get the spell school of the interrupted spell
                let interrupted_ability_def = abilities.get_unchecked(&cast_state.ability);
                let interrupted_school = interrupted_ability_def.spell_school;
                let interrupted_spell_name = &interrupted_ability_def.name;

                // Mark cast as interrupted
                cast_state.interrupted = true;
                cast_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds

                // Mark the ability cast as interrupted in the combat log (for timeline visualization)
                let interrupted_caster_id = format!("Team {} {}", target_combatant.team, target_combatant.class.name());
                combat_log.mark_cast_interrupted(&interrupted_caster_id, interrupted_spell_name);

                // Apply lockout and log
                apply_interrupt_lockout(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    interrupt,
                    &combatants,
                    &pet_query,
                    target_combatant,
                    interrupted_school,
                    interrupted_spell_name,
                );

                interrupted = true;
            }
        }

        // Check if target is channeling (if not already interrupted a cast)
        if !interrupted {
            if let Ok((mut channel_state, target_combatant)) = channeling_targets.get_mut(interrupt.target) {
                // Don't interrupt if already interrupted
                if !channel_state.interrupted {
                    // Get the spell school of the interrupted channel
                    let interrupted_ability_def = abilities.get_unchecked(&channel_state.ability);
                    let interrupted_school = interrupted_ability_def.spell_school;
                    let interrupted_spell_name = &interrupted_ability_def.name;

                    // Mark channel as interrupted
                    channel_state.interrupted = true;
                    channel_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds

                    // Mark the ability as interrupted in the combat log (for timeline visualization)
                    let interrupted_caster_id = format!("Team {} {}", target_combatant.team, target_combatant.class.name());
                    combat_log.mark_cast_interrupted(&interrupted_caster_id, interrupted_spell_name);

                    // Apply lockout and log
                    apply_interrupt_lockout(
                        &mut commands,
                        &mut combat_log,
                        &abilities,
                        interrupt,
                        &combatants,
                        &pet_query,
                        target_combatant,
                        interrupted_school,
                        interrupted_spell_name,
                    );
                }
            }
        }

        // Despawn the interrupt entity
        commands.entity(interrupt_entity).despawn();
    }
}

/// Helper function to apply spell school lockout and log the interrupt.
fn apply_interrupt_lockout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    interrupt: &InterruptPending,
    combatants: &Query<&Combatant>,
    pet_query: &Query<&Pet>,
    target_combatant: &Combatant,
    interrupted_school: SpellSchool,
    interrupted_spell_name: &str,
) {
    // Get caster info for logging
    let caster_info = if let Ok(caster) = combatants.get(interrupt.caster) {
        let display_name = if let Ok(pet) = pet_query.get(interrupt.caster) {
            pet.pet_type.name().to_string()
        } else {
            caster.class.name().to_string()
        };
        (caster.team, display_name)
    } else {
        (0, "Unknown".to_string()) // Fallback
    };

    // Apply spell school lockout aura
    // Store the locked school as the magnitude (cast to f32)
    let locked_school_value = match interrupted_school {
        SpellSchool::Physical => 0.0,
        SpellSchool::Frost => 1.0,
        SpellSchool::Holy => 2.0,
        SpellSchool::Shadow => 3.0,
        SpellSchool::Arcane => 4.0,
        SpellSchool::Fire => 5.0,
        SpellSchool::None => 6.0,
    };

    commands.spawn(AuraPending {
        target: interrupt.target,
        aura: Aura {
            effect_type: AuraType::SpellSchoolLockout,
            duration: interrupt.lockout_duration,
            magnitude: locked_school_value,
            break_on_damage_threshold: -1.0, // Never breaks on damage
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(interrupt.caster),
            ability_name: abilities.get_unchecked(&interrupt.ability).name.clone(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None, // Lockouts are not dispellable
        },
    });

    // Log the interrupt
    let school_name = match interrupted_school {
        SpellSchool::Physical => "Physical",
        SpellSchool::Frost => "Frost",
        SpellSchool::Holy => "Holy",
        SpellSchool::Shadow => "Shadow",
        SpellSchool::Arcane => "Arcane",
        SpellSchool::Fire => "Fire",
        SpellSchool::None => "None",
    };

    let message = format!(
        "Team {} {} interrupts Team {} {}'s {} - {} school locked for {:.1}s",
        caster_info.0,
        caster_info.1,
        target_combatant.team,
        target_combatant.class.name(),
        interrupted_spell_name,
        school_name,
        interrupt.lockout_duration
    );
    combat_log.log(CombatLogEventType::AbilityUsed, message);

    info!(
        "Team {} {} interrupted! {} school locked for {:.1}s",
        target_combatant.team,
        target_combatant.class.name(),
        school_name,
        interrupt.lockout_duration
    );
}

/// Process casting: update cast timers and apply effects when casts complete.
///
/// When a cast completes:
/// 1. Consume mana
/// 2. Deal damage (for damage spells) or heal (for healing spells)
/// 3. Apply auras (if applicable)
/// 4. Spawn floating combat text (yellow for damage, green for healing)
/// 5. Log to combat log with position data
pub fn process_casting(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    mut game_rng: ResMut<GameRng>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&mut CastingState>, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't complete casts during victory celebration
    if celebration.is_some() {
        return;
    }
    
    let dt = time.delta_secs();
    
    // Track completed casts
    let mut completed_casts = Vec::new();
    
    // First pass: update cast timers and collect completed casts
    for (caster_entity, caster_transform, mut caster, casting_state, caster_auras) in combatants.iter_mut() {
        let Some(mut casting) = casting_state else {
            continue;
        };
        
        if !caster.is_alive() {
            // Cancel cast if caster dies
            commands.entity(caster_entity).remove::<CastingState>();
            continue;
        }

        // WoW Mechanic: Stun, Fear, and Polymorph cancel casts in progress
        // (Root does NOT interrupt casting — only movement)
        let is_incapacitated = super::utils::is_incapacitated(caster_auras.as_deref());
        if is_incapacitated {
            let ability_def = abilities.get_unchecked(&casting.ability);
            let caster_id = format!("Team {} {}", caster.team, caster.class.name());
            combat_log.mark_cast_interrupted(&caster_id, &ability_def.name);
            combat_log.log(
                CombatLogEventType::CrowdControl,
                format!("{}'s {} interrupted by crowd control", caster_id, ability_def.name),
            );
            commands.entity(caster_entity).remove::<CastingState>();
            continue;
        }

        // Handle interrupted casts
        if casting.interrupted {
            // Tick down the interrupted display timer
            casting.interrupted_display_time -= dt;
            
            // Remove CastingState once display time expires
            if casting.interrupted_display_time <= 0.0 {
                commands.entity(caster_entity).remove::<CastingState>();
            }
            
            // Don't process interrupted casts
            continue;
        }
        
        // Tick down cast time
        casting.time_remaining -= dt;
        
        // Check if cast completed
        if casting.time_remaining <= 0.0 {
            let ability = casting.ability;
            let def = abilities.get_unchecked(&ability);
            let target_entity = casting.target;

            // Consume mana
            caster.current_mana -= def.mana_cost;

            // Pre-calculate damage/healing (using caster's stats)
            let mut ability_damage = caster.calculate_ability_damage_config(def, &mut game_rng);

            // Roll crit for damage (before physical damage reduction)
            let is_crit_damage = if def.is_damage() {
                let crit = roll_crit(caster.crit_chance, &mut game_rng);
                if crit { ability_damage *= CRIT_DAMAGE_MULTIPLIER; }
                crit
            } else {
                false
            };

            // Apply physical damage reduction for Physical abilities (Curse of Weakness: -20%)
            if def.spell_school == SpellSchool::Physical {
                let damage_reduction = get_physical_damage_reduction(caster_auras.as_deref());
                ability_damage = (ability_damage * (1.0 - damage_reduction)).max(0.0);
            }

            // Apply Divine Shield outgoing damage penalty (50%)
            let ds_penalty = get_divine_shield_damage_penalty(caster_auras.as_deref());
            ability_damage = (ability_damage * ds_penalty).max(0.0);

            let mut ability_healing = caster.calculate_ability_healing_config(def, &mut game_rng);

            // Roll crit for healing (before healing reduction)
            let is_crit_heal = if def.is_heal() {
                let crit = roll_crit(caster.crit_chance, &mut game_rng);
                if crit { ability_healing *= CRIT_HEALING_MULTIPLIER; }
                crit
            } else {
                false
            };

            // Store cast info for processing
            completed_casts.push((
                caster_entity,
                caster.team,
                caster.class,
                caster_transform.translation,
                ability_damage,
                ability_healing,
                ability,
                target_entity,
                is_crit_damage,
                is_crit_heal,
            ));
            
            // Remove casting state
            // Note: GCD was already triggered when the cast began, not here
            commands.entity(caster_entity).remove::<CastingState>();
        }
    }
    
    // Track damage_dealt updates for casters (to apply after processing all casts)
    let mut caster_damage_updates: Vec<(Entity, f32)> = Vec::new();
    // Track healing_done updates for healers (to apply after processing all casts)
    let mut caster_healing_updates: Vec<(Entity, f32)> = Vec::new();
    // Track ability cooldowns to apply (caster_entity, ability, cooldown_duration)
    let mut cooldown_updates: Vec<(Entity, AbilityType, f32)> = Vec::new();
    // Track casters who should have stealth broken (offensive abilities)
    let mut break_stealth: Vec<Entity> = Vec::new();
    
    // Process completed casts
    for (caster_entity, caster_team, caster_class, caster_pos, ability_damage, ability_healing, ability, target_entity, is_crit_damage, is_crit_heal) in completed_casts {
        let def = abilities.get_unchecked(&ability);
        
        // Get target
        let Some(target_entity) = target_entity else {
            continue;
        };
        
        // If this ability uses a projectile, spawn it and skip immediate effect application
        if let Some(projectile_speed) = def.projectile_speed {
            // Spawn projectile with Transform (required for move_projectiles to work in headless mode)
            // Visual mesh/material is added by spawn_projectile_visuals in graphical mode
            commands.spawn((
                Projectile {
                    caster: caster_entity,
                    target: target_entity,
                    ability,
                    speed: projectile_speed,
                    caster_team,
                    caster_class,
                },
                Transform::from_translation(caster_pos + Vec3::new(0.0, 1.5, 0.0)), // Spawn at chest height
                PlayMatchEntity,
            ));
            continue; // Skip immediate damage/healing - projectile will handle it on impact
        }
        
        // Check if this is self-targeting (e.g., priest healing themselves)
        let is_self_target = target_entity == caster_entity;
        
        // Get target combatant
        let Ok((_, target_transform, mut target, _, mut target_auras)) = combatants.get_mut(target_entity) else {
            continue;
        };
        
        if !target.is_alive() {
            continue;
        }
        
        let target_pos = target_transform.translation;
        let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
        
        // Handle damage spells
        if def.is_damage() {
            // Use pre-calculated damage (already includes stat scaling and DamageReduction)
            let damage = ability_damage;

            // Apply damage with absorb shield consideration
            let (actual_damage, absorbed) = apply_damage_with_absorb(
                damage,
                &mut target,
                target_auras.as_deref_mut(),
            );

            // Warriors generate Rage from taking damage (only on actual health damage)
            if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
            }

            // Track damage for aura breaking
            commands.entity(target_entity).insert(DamageTakenThisFrame {
                amount: actual_damage,
            });

            // Break stealth on offensive ability use
            break_stealth.push(caster_entity);

            // Track damage dealt for caster (include absorbed - caster dealt it)
            let total_damage_dealt = actual_damage + absorbed;
            if is_self_target {
                // Self-damage: target IS caster, so update now
                target.damage_dealt += total_damage_dealt;
            } else {
                // Different target: collect for later update
                caster_damage_updates.push((caster_entity, total_damage_dealt));
            }

            // Spawn floating combat text (yellow for damage abilities)
            // Get deterministic offset based on pattern state
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("{:.0}", actual_damage),
                    color: egui::Color32::from_rgb(255, 255, 0), // Yellow for abilities
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit: is_crit_damage,
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

            // Spawn visual effect for Mind Blast (shadow impact)
            if ability == AbilityType::MindBlast {
                commands.spawn((
                    SpellImpactEffect {
                        position: target_pos,
                        lifetime: 0.5,
                        initial_lifetime: 0.5,
                        initial_scale: 0.5,
                        final_scale: 2.0,
                    },
                    PlayMatchEntity,
                ));
            }

            // Spawn flame particles for Immolate (fire rising effect)
            if ability == AbilityType::Immolate {
                // Spawn 8-12 flame particles around target
                let particle_count = 8 + (game_rng.random_f32() * 5.0) as i32;
                for _ in 0..particle_count {
                    // Randomize position slightly around target
                    let offset = Vec3::new(
                        (game_rng.random_f32() - 0.5) * 1.0,  // -0.5 to 0.5
                        game_rng.random_f32() * 0.5,          // 0 to 0.5 (start near ground)
                        (game_rng.random_f32() - 0.5) * 1.0,
                    );
                    let velocity = Vec3::new(
                        (game_rng.random_f32() - 0.5) * 0.5,  // Slight horizontal drift
                        2.0 + game_rng.random_f32() * 1.5,    // Upward: 2.0-3.5 units/sec
                        (game_rng.random_f32() - 0.5) * 0.5,
                    );
                    let lifetime = 0.6 + game_rng.random_f32() * 0.4;  // 0.6-1.0 sec
                    commands.spawn((
                        FlameParticle {
                            velocity,
                            lifetime,
                            initial_lifetime: lifetime,
                        },
                        Transform::from_translation(target_pos + offset),
                        PlayMatchEntity,
                    ));
                }
            }

            // Log the damage with structured data
            let is_killing_blow = !target.is_alive();
            let is_first_death = is_killing_blow && !target.is_dead;
            if is_first_death {
                target.is_dead = true;
            }
            let verb = if is_crit_damage { "CRITS" } else { "hits" };
            let message = if absorbed > 0.0 {
                format!(
                    "Team {} {}'s {} {} Team {} {} for {:.0} damage ({:.0} absorbed)",
                    caster_team,
                    caster_class.name(),
                    def.name,
                    verb,
                    target.team,
                    target.class.name(),
                    actual_damage,
                    absorbed
                )
            } else {
                format!(
                    "Team {} {}'s {} {} Team {} {} for {:.0} damage",
                    caster_team,
                    caster_class.name(),
                    def.name,
                    verb,
                    target.team,
                    target.class.name(),
                    actual_damage
                )
            };
            combat_log.log_damage(
                combatant_id(caster_team, caster_class),
                combatant_id(target.team, target.class),
                def.name.to_string(),
                total_damage_dealt, // Total damage including absorbed
                is_killing_blow,
                is_crit_damage,
                message,
            );
        }
        // Handle healing spells
        else if def.is_heal() {
            // Use pre-calculated healing (already includes stat scaling + crit)
            let mut healing = ability_healing;
            
            // Check for healing reduction auras
            if let Some(auras) = target_auras {
                for aura in &auras.auras {
                    if aura.effect_type == AuraType::HealingReduction {
                        // Magnitude is a multiplier (e.g., 0.65 = 35% reduction)
                        healing *= aura.magnitude;
                    }
                }
            }
            
            // Apply healing (don't overheal)
            let actual_healing = healing.min(target.max_health - target.current_health);
            target.current_health = (target.current_health + healing).min(target.max_health);
            
            // Track healing done for healer (update later to avoid double borrow)
            if is_self_target {
                // Self-healing: target IS caster, so update now
                target.healing_done += actual_healing;
            } else {
                // Different target: collect for later update
                caster_healing_updates.push((caster_entity, actual_healing));
            }
            
            // Spawn floating combat text (green for healing)
            // Get deterministic offset based on pattern state
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("+{:.0}", actual_healing),
                    color: egui::Color32::from_rgb(100, 255, 100), // Green for healing
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit: is_crit_heal,
                },
                PlayMatchEntity,
            ));

            // Log the healing with structured data
            let verb = if is_crit_heal { "CRITICALLY heals" } else { "heals" };
            let message = format!(
                "Team {} {}'s {} {} Team {} {} for {:.0}",
                caster_team,
                caster_class.name(),
                def.name,
                verb,
                target.team,
                target.class.name(),
                actual_healing
            );
            combat_log.log_healing(
                combatant_id(caster_team, caster_class),
                combatant_id(target.team, target.class),
                def.name.to_string(),
                actual_healing,
                is_crit_heal,
                message,
            );

            // Spawn healing light column visual effect
            commands.spawn((
                HealingLightColumn {
                    target: target_entity,
                    healer_class: caster_class,
                    lifetime: 0.8,
                    initial_lifetime: 0.8,
                },
                PlayMatchEntity,
            ));
        }

        // Apply aura if applicable (store for later application)
        if let Some(aura) = def.applies_aura.as_ref() {
            // We'll apply auras in a separate pass to avoid borrow issues
            // Convert spell school to Option (None for Physical, since physical = not magic-dispellable)
            let aura_spell_school = match def.spell_school {
                SpellSchool::Physical | SpellSchool::None => None,
                school => Some(school),
            };
            commands.spawn((
                AuraPending {
                    target: target_entity,
                    aura: Aura {
                        effect_type: aura.aura_type,
                        duration: aura.duration,
                        magnitude: aura.magnitude,
                        break_on_damage_threshold: aura.break_on_damage,
                        accumulated_damage: 0.0,
                        tick_interval: aura.tick_interval,
                        time_until_next_tick: aura.tick_interval,
                        caster: Some(caster_entity),
                        ability_name: def.name.clone(),
                        fear_direction: (0.0, 0.0),
                        fear_direction_timer: 0.0,
                        spell_school: aura_spell_school,
                    },
                },
                PlayMatchEntity,
            ));
            
            info!(
                "Queued {:?} aura for Team {} {} (magnitude: {}, duration: {}s)",
                aura.aura_type,
                target.team,
                target.class.name(),
                aura.magnitude,
                aura.duration
            );

            // Log CC application for all crowd control types
            match aura.aura_type {
                AuraType::Fear => {
                    spawn_speech_bubble(&mut commands, caster_entity, "Fear");
                    let message = format!(
                        "Team {} {}'s {} lands on Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Fear".to_string(),
                        aura.duration,
                        message,
                    );
                }
                AuraType::Root => {
                    let message = format!(
                        "Team {} {}'s {} roots Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Root".to_string(),
                        aura.duration,
                        message,
                    );
                }
                AuraType::Stun => {
                    let message = format!(
                        "Team {} {}'s {} stuns Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Stun".to_string(),
                        aura.duration,
                        message,
                    );
                }
                AuraType::Polymorph => {
                    let message = format!(
                        "Team {} {}'s {} polymorphs Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Polymorph".to_string(),
                        aura.duration,
                        message,
                    );
                }
                _ => {} // Non-CC auras don't need logging here
            }
        }

        // Track cooldown if ability has one
        if def.cooldown > 0.0 {
            cooldown_updates.push((caster_entity, ability, def.cooldown));
        }

        // Check for death (log if killed by non-damage abilities/auras)
        // Note: damage abilities already log death via is_first_death above
        if !target.is_alive() && !def.is_damage() && !target.is_dead {
            target.is_dead = true;

            // Cancel any in-progress cast or channel so dead combatants can't finish spells
            commands.entity(target_entity).remove::<CastingState>();
            commands.entity(target_entity).remove::<ChannelingState>();

            let message = format!(
                "Team {} {} has been eliminated",
                target.team,
                target.class.name()
            );
            combat_log.log_death(
                combatant_id(target.team, target.class),
                Some(combatant_id(caster_team, caster_class)),
                message,
            );
        }
    }
    
    // Apply collected caster damage updates
    for (caster_entity, damage) in caster_damage_updates {
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += damage;
        }
    }
    
    // Apply collected healer healing updates
    for (healer_entity, healing) in caster_healing_updates {
        if let Ok((_, _, mut healer, _, _)) = combatants.get_mut(healer_entity) {
            healer.healing_done += healing;
        }
    }

    // Apply collected ability cooldowns
    for (caster_entity, ability, cooldown) in cooldown_updates {
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.ability_cooldowns.insert(ability, cooldown);
        }
    }

    // Break stealth for casters who used offensive abilities
    for caster_entity in break_stealth {
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            if caster.stealthed {
                caster.stealthed = false;
                info!(
                    "Team {} {} breaks stealth!",
                    caster.team,
                    caster.class.name()
                );
            }
        }
    }
}

/// Process channeling: update channel timers and apply tick effects.
///
/// When a channel tick occurs:
/// 1. Deal damage to target (with absorb shield handling)
/// 2. Heal caster (reduced by healing reduction auras)
/// 3. Spawn floating combat text (yellow for damage, green for heal)
/// 4. Log to combat log
///
/// Channel ends when duration expires or either caster/target dies.
pub fn process_channeling(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&mut ChannelingState>, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't process channels during victory celebration
    if celebration.is_some() {
        return;
    }

    let dt = time.delta_secs();

    // Track updates to apply after the loop
    let mut remove_channel: Vec<Entity> = Vec::new();
    let mut caster_healing_updates: Vec<(Entity, f32)> = Vec::new();
    // (caster_entity, target_entity, damage, caster_team, caster_class)
    let mut damage_to_apply: Vec<(Entity, Entity, f32, u8, match_config::CharacterClass)> = Vec::new();

    // Build a snapshot of positions and health for lookups
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _, _)| (entity, transform.translation))
        .collect();
    let health_info: std::collections::HashMap<Entity, (bool, u8, match_config::CharacterClass)> = combatants
        .iter()
        .map(|(entity, _, combatant, _, _)| (entity, (combatant.is_alive(), combatant.team, combatant.class)))
        .collect();
    // Snapshot target immunity status for Drain Life healing suppression
    let immunity_info: std::collections::HashSet<Entity> = combatants
        .iter()
        .filter(|(_, _, _, _, auras)| {
            auras.as_ref().map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity))
        })
        .map(|(entity, _, _, _, _)| entity)
        .collect();

    for (caster_entity, _caster_transform, caster, channeling_state, caster_auras) in combatants.iter_mut() {
        let Some(mut channeling) = channeling_state else {
            continue;
        };

        // Handle interrupted state display
        if channeling.interrupted {
            channeling.interrupted_display_time -= dt;
            if channeling.interrupted_display_time <= 0.0 {
                remove_channel.push(caster_entity);
            }
            continue;
        }

        // Check if caster died
        if !caster.is_alive() {
            remove_channel.push(caster_entity);
            continue;
        }

        // WoW Mechanic: Stun, Fear, and Polymorph cancel channels in progress
        // (Root does NOT interrupt channeling — only movement)
        let is_incapacitated = super::utils::is_incapacitated(caster_auras.as_deref());
        if is_incapacitated {
            let ability_def = abilities.get_unchecked(&channeling.ability);
            let caster_id = format!("Team {} {}", caster.team, caster.class.name());
            combat_log.mark_cast_interrupted(&caster_id, &ability_def.name);
            combat_log.log(
                CombatLogEventType::CrowdControl,
                format!("{}'s {} interrupted by crowd control", caster_id, ability_def.name),
            );
            remove_channel.push(caster_entity);
            continue;
        }

        // Check if target died or no longer exists
        let target_alive = health_info
            .get(&channeling.target)
            .map(|(alive, _, _)| *alive)
            .unwrap_or(false);
        if !target_alive {
            remove_channel.push(caster_entity);
            continue;
        }

        // Tick down timers
        channeling.duration_remaining -= dt;
        channeling.time_until_next_tick -= dt;

        // Process tick if ready
        if channeling.time_until_next_tick <= 0.0 {
            let ability_def = abilities.get_unchecked(&channeling.ability);

            // Calculate damage (flat damage per tick for Drain Life)
            let damage = ability_def.damage_base_min;

            // Track damage to apply later (includes target entity and caster info for death logging)
            damage_to_apply.push((caster_entity, channeling.target, damage, caster.team, caster.class));

            // Track healing for caster (Drain Life heals 0 if target has DamageImmunity)
            let healing = ability_def.channel_healing_per_tick;
            let target_immune = immunity_info.contains(&channeling.target);
            if healing > 0.0 && !target_immune {
                caster_healing_updates.push((caster_entity, healing));
            }

            // Log the tick
            if let Some(&(_, target_team, target_class)) = health_info.get(&channeling.target) {
                let damage_message = format!(
                    "Team {} {}'s {} ticks on Team {} {} for {:.0} damage",
                    caster.team,
                    caster.class.name(),
                    ability_def.name,
                    target_team,
                    target_class.name(),
                    damage
                );
                combat_log.log_damage(
                    combatant_id(caster.team, caster.class),
                    combatant_id(target_team, target_class),
                    format!("{} (tick)", ability_def.name),
                    damage,
                    false, // Not a killing blow check here - will be handled when applying damage
                    false, // is_crit - channel ticks never crit
                    damage_message,
                );

                if healing > 0.0 {
                    let heal_message = format!(
                        "Team {} {}'s {} heals for {:.0}",
                        caster.team,
                        caster.class.name(),
                        ability_def.name,
                        healing
                    );
                    combat_log.log_healing(
                        combatant_id(caster.team, caster.class),
                        combatant_id(caster.team, caster.class),
                        format!("{} (tick)", ability_def.name),
                        healing,
                        false, // is_crit - channel ticks never crit
                        heal_message,
                    );
                }
            }

            // Spawn floating combat text for damage on target
            if let Some(&target_pos) = positions.get(&channeling.target) {
                let text_position = target_pos + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
                let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(channeling.target) {
                    get_next_fct_offset(&mut fct_state)
                } else {
                    (0.0, 0.0)
                };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                        text: format!("{:.0}", damage),
                        color: egui::Color32::from_rgb(255, 255, 0), // Yellow for ability damage
                        lifetime: 1.5,
                        vertical_offset: offset_y,
                        is_crit: false,
                    },
                    PlayMatchEntity,
                ));
            }

            // Reset tick timer
            channeling.time_until_next_tick = channeling.tick_interval;
            channeling.ticks_applied += 1;
        }

        // Check if channel duration expired
        if channeling.duration_remaining <= 0.0 {
            remove_channel.push(caster_entity);

            info!(
                "Team {} {} completed {} channel ({} ticks)",
                caster.team,
                caster.class.name(),
                abilities.get_unchecked(&channeling.ability).name,
                channeling.ticks_applied
            );
        }
    }

    // Apply damage to targets and update caster stats
    for (caster_entity, target_entity, damage, caster_team, caster_class) in damage_to_apply {
        // Apply damage to target
        if let Ok((_, target_transform, mut target, _, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                let (actual_damage, absorbed) = apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );

                // Track damage for aura breaking
                if actual_damage > 0.0 {
                    commands.entity(target_entity).insert(DamageTakenThisFrame {
                        amount: actual_damage,
                    });
                }

                // Spawn absorbed text if any
                if absorbed > 0.0 {
                    let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
                    let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                        get_next_fct_offset(&mut fct_state)
                    } else {
                        (0.0, 0.0)
                    };
                    commands.spawn((
                        FloatingCombatText {
                            world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                            text: format!("{:.0} absorbed", absorbed),
                            color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                            lifetime: 1.5,
                            vertical_offset: offset_y,
                            is_crit: false,
                        },
                        PlayMatchEntity,
                    ));
                }

                // Warriors generate Rage from taking damage
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Check for killing blow
                if !target.is_alive() && !target.is_dead {
                    target.is_dead = true;
                    let death_message = format!(
                        "Team {} {} has been eliminated",
                        target.team,
                        target.class.name()
                    );
                    combat_log.log_death(
                        combatant_id(target.team, target.class),
                        Some(combatant_id(caster_team, caster_class)),
                        death_message,
                    );
                }
            }
        }

        // Update caster damage dealt stats
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += damage;
        }
    }

    // Apply healing to casters and spawn healing FCT
    for (caster_entity, healing) in caster_healing_updates {
        if let Ok((_, caster_transform, mut caster, _, caster_auras)) = combatants.get_mut(caster_entity) {
            let mut actual_healing = healing;

            // Check for healing reduction auras
            if let Some(auras) = caster_auras {
                for aura in &auras.auras {
                    if aura.effect_type == AuraType::HealingReduction {
                        actual_healing *= aura.magnitude;
                    }
                }
            }

            // Apply healing
            let effective_healing = actual_healing.min(caster.max_health - caster.current_health);
            caster.current_health = (caster.current_health + actual_healing).min(caster.max_health);
            caster.healing_done += effective_healing;

            // Spawn floating combat text for healing
            let text_position = caster_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(caster_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("+{:.0}", effective_healing),
                    color: egui::Color32::from_rgb(100, 255, 100), // Green for healing
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit: false,
                },
                PlayMatchEntity,
            ));
        }
    }

    // Remove completed/interrupted channels
    for entity in remove_channel {
        commands.entity(entity).remove::<ChannelingState>();
    }
}

/// Trigger death animation when a combatant dies.
/// Detects dead combatants without a DeathAnimation component and adds one.
pub fn trigger_death_animation(
    mut commands: Commands,
    combatants: Query<(Entity, &Combatant, &Transform), Without<DeathAnimation>>,
    all_combatants: Query<(&Transform, &Combatant)>,
) {
    for (entity, combatant, transform) in combatants.iter() {
        if combatant.is_alive() {
            continue;
        }

        // Combatant just died - calculate fall direction
        // Fall away from the nearest living enemy (dramatic effect)
        let my_pos = transform.translation;
        let mut nearest_enemy_pos: Option<Vec3> = None;
        let mut nearest_distance = f32::MAX;

        for (other_transform, other_combatant) in all_combatants.iter() {
            if other_combatant.team != combatant.team && other_combatant.is_alive() {
                let distance = my_pos.distance(other_transform.translation);
                if distance < nearest_distance {
                    nearest_distance = distance;
                    nearest_enemy_pos = Some(other_transform.translation);
                }
            }
        }

        // Fall direction: away from nearest enemy, or forward if no enemy found
        let fall_direction = if let Some(enemy_pos) = nearest_enemy_pos {
            Vec3::new(
                my_pos.x - enemy_pos.x,
                0.0,
                my_pos.z - enemy_pos.z,
            ).normalize_or_zero()
        } else {
            // No enemy found, fall in the direction they're facing
            let forward = transform.rotation * Vec3::Z;
            Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero()
        };

        // Default to falling along negative Z if no direction could be determined
        let fall_direction = if fall_direction == Vec3::ZERO {
            Vec3::new(0.0, 0.0, -1.0)
        } else {
            fall_direction
        };

        commands.entity(entity).insert(DeathAnimation::new(fall_direction));

        info!(
            "Team {} {} death animation started (falling toward {:?})",
            combatant.team,
            combatant.class.name(),
            fall_direction
        );
    }
}

/// Animate dead combatants falling over.
/// Updates the DeathAnimation component each frame to rotate and lower the capsule.
pub fn animate_death(
    time: Res<Time>,
    mut combatants: Query<(&mut Transform, &mut DeathAnimation)>,
) {
    let dt = time.delta_secs();

    for (mut transform, mut death_anim) in combatants.iter_mut() {
        if death_anim.is_complete() {
            continue;
        }

        // Advance animation
        death_anim.progress += dt / DeathAnimation::DURATION;
        death_anim.progress = death_anim.progress.min(1.0);

        // Ease-out for natural deceleration (fast start, slow finish)
        let t = ease_out_quad(death_anim.progress);

        // Rotation: 0° -> 90° around axis perpendicular to fall direction
        // The rotation axis is perpendicular to both Y (up) and fall direction
        let rotation_axis = Vec3::Y.cross(death_anim.fall_direction).normalize_or_zero();

        if rotation_axis != Vec3::ZERO {
            let rotation_angle = t * std::f32::consts::FRAC_PI_2; // 90 degrees
            transform.rotation = Quat::from_axis_angle(rotation_axis, rotation_angle);
        }

        // Lower Y as capsule falls (1.0 standing -> 0.5 lying flat)
        transform.translation.y = 1.0 - (t * 0.5);
    }
}

/// Ease-out quadratic function for smooth deceleration.
/// Returns 0.0 at t=0.0 and 1.0 at t=1.0, with decreasing rate of change.
fn ease_out_quad(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(2)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test combatant
    fn create_test_combatant(health: f32) -> Combatant {
        let mut combatant = Combatant::new(1, 0, match_config::CharacterClass::Warrior);
        combatant.max_health = health;
        combatant.current_health = health;
        combatant.damage_taken = 0.0;
        combatant
    }

    /// Helper to create an absorb aura
    fn create_absorb_aura(amount: f32, ability_name: &str) -> Aura {
        Aura {
            effect_type: AuraType::Absorb,
            duration: 30.0,
            magnitude: amount,
            break_on_damage_threshold: 0.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: ability_name.to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
        }
    }

    // =========================================================================
    // apply_damage_with_absorb Tests
    // =========================================================================

    #[test]
    fn test_damage_with_no_shields() {
        let mut target = create_test_combatant(100.0);

        let (actual_damage, absorbed) = apply_damage_with_absorb(30.0, &mut target, None);

        assert_eq!(actual_damage, 30.0, "All damage should hit health");
        assert_eq!(absorbed, 0.0, "No damage should be absorbed");
        assert_eq!(target.current_health, 70.0, "Health should decrease by damage");
        assert_eq!(target.damage_taken, 30.0, "Damage taken should be tracked");
    }

    #[test]
    fn test_damage_fully_absorbed_by_shield() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![create_absorb_aura(50.0, "Power Word: Shield")],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(30.0, &mut target, Some(&mut auras));

        assert_eq!(actual_damage, 0.0, "No damage should hit health");
        assert_eq!(absorbed, 30.0, "All damage should be absorbed");
        assert_eq!(target.current_health, 100.0, "Health should remain full");
        assert_eq!(auras.auras[0].magnitude, 20.0, "Shield should have 20 remaining");
    }

    #[test]
    fn test_damage_partially_absorbed() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![create_absorb_aura(20.0, "Power Word: Shield")],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(50.0, &mut target, Some(&mut auras));

        assert_eq!(absorbed, 20.0, "Shield should absorb its full amount");
        assert_eq!(actual_damage, 30.0, "Remaining damage should hit health");
        assert_eq!(target.current_health, 70.0, "Health should decrease by remaining damage");
        assert!(auras.auras.is_empty(), "Depleted shield should be removed");
    }

    #[test]
    fn test_multiple_shields_stack() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![
                create_absorb_aura(30.0, "Power Word: Shield"),
                create_absorb_aura(40.0, "Ice Barrier"),
            ],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(50.0, &mut target, Some(&mut auras));

        assert_eq!(absorbed, 50.0, "All damage should be absorbed by combined shields");
        assert_eq!(actual_damage, 0.0, "No damage should hit health");
        assert_eq!(target.current_health, 100.0, "Health should remain full");

        // First shield should be consumed, second should have remaining
        assert_eq!(auras.auras.len(), 1, "One shield should remain");
        assert_eq!(auras.auras[0].magnitude, 20.0, "Ice Barrier should have 20 remaining");
    }

    #[test]
    fn test_damage_exceeds_health() {
        let mut target = create_test_combatant(50.0);

        let (actual_damage, absorbed) = apply_damage_with_absorb(100.0, &mut target, None);

        assert_eq!(actual_damage, 50.0, "Actual damage should be limited by remaining health");
        assert_eq!(absorbed, 0.0, "No damage absorbed");
        assert_eq!(target.current_health, 0.0, "Target should be dead");
    }

    #[test]
    fn test_zero_damage() {
        let mut target = create_test_combatant(100.0);

        let (actual_damage, absorbed) = apply_damage_with_absorb(0.0, &mut target, None);

        assert_eq!(actual_damage, 0.0, "No damage dealt");
        assert_eq!(absorbed, 0.0, "No damage absorbed");
        assert_eq!(target.current_health, 100.0, "Health unchanged");
    }

    #[test]
    fn test_depleted_shield_removed() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![create_absorb_aura(25.0, "Power Word: Shield")],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(25.0, &mut target, Some(&mut auras));

        assert_eq!(absorbed, 25.0);
        assert_eq!(actual_damage, 0.0);
        assert!(auras.auras.is_empty(), "Exactly-depleted shield should be removed");
    }

    // =========================================================================
    // has_absorb_shield Tests
    // =========================================================================

    #[test]
    fn test_has_absorb_shield_with_no_auras() {
        assert!(!has_absorb_shield(None));
    }

    #[test]
    fn test_has_absorb_shield_with_empty_auras() {
        let auras = ActiveAuras { auras: vec![] };
        assert!(!has_absorb_shield(Some(&auras)));
    }

    #[test]
    fn test_has_absorb_shield_with_absorb() {
        let auras = ActiveAuras {
            auras: vec![create_absorb_aura(50.0, "Power Word: Shield")],
        };
        assert!(has_absorb_shield(Some(&auras)));
    }

    #[test]
    fn test_has_absorb_shield_with_other_auras() {
        let auras = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::MovementSpeedSlow,
                duration: 5.0,
                magnitude: 0.7,
                break_on_damage_threshold: 0.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Frostbolt".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: None,
            }],
        };
        assert!(!has_absorb_shield(Some(&auras)));
    }

    // =========================================================================
    // has_weakened_soul Tests
    // =========================================================================

    #[test]
    fn test_has_weakened_soul_with_no_auras() {
        assert!(!has_weakened_soul(None));
    }

    #[test]
    fn test_has_weakened_soul_with_weakened_soul() {
        let auras = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::WeakenedSoul,
                duration: 15.0,
                magnitude: 0.0,
                break_on_damage_threshold: 0.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Weakened Soul".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: None,
            }],
        };
        assert!(has_weakened_soul(Some(&auras)));
    }

    // =========================================================================
    // combatant_id Tests
    // =========================================================================

    #[test]
    fn test_combatant_id_format() {
        let id = combatant_id(1, match_config::CharacterClass::Warrior);
        assert_eq!(id, "Team 1 Warrior");
    }

    #[test]
    fn test_combatant_id_team2() {
        let id = combatant_id(2, match_config::CharacterClass::Mage);
        assert_eq!(id, "Team 2 Mage");
    }

    // =========================================================================
    // ease_out_quad Tests
    // =========================================================================

    #[test]
    fn test_ease_out_quad_boundaries() {
        assert_eq!(ease_out_quad(0.0), 0.0, "Should return 0 at t=0");
        assert_eq!(ease_out_quad(1.0), 1.0, "Should return 1 at t=1");
    }

    #[test]
    fn test_ease_out_quad_midpoint() {
        let mid = ease_out_quad(0.5);
        assert!(mid > 0.5, "Ease-out should be > 0.5 at t=0.5, got {}", mid);
        assert!(mid < 1.0, "Ease-out should be < 1.0 at t=0.5, got {}", mid);
    }
}

