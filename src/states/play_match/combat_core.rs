//! Combat Core Systems
//!
//! Handles core combat mechanics:
//! - Movement (move_to_target, kiting logic)
//! - Auto-attacks (melee and ranged wand attacks)
//! - Resource regeneration (Energy, Rage)
//! - Casting (cast time processing, completion)
//! - Interrupt processing (applying lockouts)
//! - Stealth visuals

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::{CombatLog, CombatLogEventType, CombatantId};
use super::match_config;
use super::components::*;
use super::abilities::{AbilityType, SpellSchool};
use super::{MELEE_RANGE, ARENA_HALF_SIZE, get_next_fct_offset};

/// Helper to generate a consistent combatant ID for the combat log
/// Format: "Team {team} {class}" e.g., "Team 1 Warrior"
pub fn combatant_id(team: u8, class: match_config::CharacterClass) -> CombatantId {
    format!("Team {} {}", team, class.name())
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
        ideal_next_pos.x >= -ARENA_HALF_SIZE && ideal_next_pos.x <= ARENA_HALF_SIZE &&
        ideal_next_pos.z >= -ARENA_HALF_SIZE && ideal_next_pos.z <= ARENA_HALF_SIZE;
    
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
            candidate_next_pos.x >= -ARENA_HALF_SIZE && candidate_next_pos.x <= ARENA_HALF_SIZE &&
            candidate_next_pos.z >= -ARENA_HALF_SIZE && candidate_next_pos.z <= ARENA_HALF_SIZE;
        
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
    mut combatants: Query<(Entity, &mut Transform, &Combatant, Option<&ActiveAuras>, Option<&CastingState>, Option<&ChargingState>)>,
) {
    // Don't allow movement until gates open
    if !countdown.gates_opened {
        return;
    }
    
    let dt = time.delta_secs();
    
    // Build a snapshot of all combatant positions and team info for lookups
    let positions: std::collections::HashMap<Entity, (Vec3, u8)> = combatants
        .iter()
        .map(|(entity, transform, combatant, _, _, _)| (entity, (transform.translation, combatant.team)))
        .collect();
    
    // Move each combatant towards their target if needed
    for (entity, mut transform, combatant, auras, casting_state, charging_state) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Cannot move while casting (WoW mechanic)
        if casting_state.is_some() {
            continue;
        }
        
        // Check if rooted, stunned, or feared - if so, cannot move intentionally
        let is_cc_locked = if let Some(auras) = auras {
            auras.auras.iter().any(|a| matches!(a.effect_type, AuraType::Root | AuraType::Stun | AuraType::Fear))
        } else {
            false
        };
        
        if is_cc_locked {
            continue;
        }
        
        let my_pos = transform.translation;
        
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
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                
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
                    transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    
                    // Rotate to face direction of travel
                    let target_rotation = Quat::from_rotation_y(best_direction.x.atan2(best_direction.z));
                    transform.rotation = target_rotation;
                }
            }
            
            continue; // Skip normal movement logic
        }
        
        // NORMAL MOVEMENT: Get target position
        let Some(target_entity) = combatant.target else {
            // No target available (likely facing all-stealth team)
            // Move to defensive position in center of arena to anticipate stealth openers
            let defensive_pos = Vec3::ZERO; // Center of arena
            let distance_to_defensive = my_pos.distance(defensive_pos);
            
            // Only move if we're far from the defensive position (> 5 units)
            if distance_to_defensive > 5.0 {
                let direction = Vec3::new(
                    defensive_pos.x - my_pos.x,
                    0.0,
                    defensive_pos.z - my_pos.z,
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
                    
                    // Move towards defensive position
                    let move_distance = movement_speed * dt;
                    transform.translation += direction * move_distance;
                    
                    // Clamp position to arena bounds
                    transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    
                    // Rotate to face center
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

        // Use class-specific preferred range - the optimal distance where they
        // can use all their important abilities without unnecessary repositioning
        let stop_distance = combatant.class.preferred_range();

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
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                
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
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&CastingState>, Option<&ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't deal damage during victory celebration
    if celebration.is_some() {
        return;
    }
    let dt = time.delta_secs();
    
    // Update match time in combat log (countdown doesn't count against match time)
    if countdown.gates_opened {
        combat_log.match_time += dt;
    }
    
    // Don't allow auto-attacks until gates open
    if !countdown.gates_opened {
        return;
    }
    
    // Build a snapshot of positions for range checks
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _, _)| (entity, transform.translation))
        .collect();
    
    // Build a snapshot of combatant info for logging
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass)> = combatants
        .iter()
        .map(|(entity, _, combatant, _, _)| (entity, (combatant.team, combatant.class)))
        .collect();
    
    // Collect attacks that will happen this frame (attacker, target, damage)
    let mut attacks = Vec::new();
    
    // Track damage per target for batching floating combat text
    let mut damage_per_target: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    // Track damage per target for aura breaking
    let mut damage_per_aura_break: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    
    for (attacker_entity, transform, mut combatant, casting_state, auras) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // WoW Mechanic: Cannot auto-attack while stunned or feared
        let is_incapacitated = if let Some(auras) = auras {
            auras.auras.iter().any(|a| matches!(a.effect_type, AuraType::Stun | AuraType::Fear))
        } else {
            false
        };
        if is_incapacitated {
            continue;
        }
        
        // WoW Mechanic: Cannot auto-attack while casting
        if casting_state.is_some() {
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
                        let total_damage = combatant.attack_damage + combatant.next_attack_bonus_damage;
                        let has_bonus = combatant.next_attack_bonus_damage > 0.0;
                        
                        attacks.push((attacker_entity, target_entity, total_damage, has_bonus));
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
    
    for (attacker_entity, target_entity, damage, has_bonus) in attacks {
        if let Ok((_, _, mut target, _, _)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                let actual_damage = damage.min(target.current_health);
                target.current_health = (target.current_health - damage).max(0.0);
                target.damage_taken += actual_damage;
                
                // Warriors generate Rage from taking damage
                if target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }
                
                // Track damage for aura breaking
                *damage_per_aura_break.entry(target_entity).or_insert(0.0) += actual_damage;
                
                // Batch damage for floating combat text (sum all damage to same target)
                *damage_per_target.entry(target_entity).or_insert(0.0) += actual_damage;
                
                // Collect attacker damage for later update
                damage_dealt_updates.push((attacker_entity, actual_damage));
                
                // Log the attack with structured data
                if let (Some(&(attacker_team, attacker_class)), Some(&(target_team, target_class))) =
                    (combatant_info.get(&attacker_entity), combatant_info.get(&target_entity)) {
                    let attack_name = if has_bonus {
                        "Heroic Strike" // Enhanced auto-attack
                    } else {
                        // Distinguish between melee and wand attacks based on class
                        match attacker_class {
                            match_config::CharacterClass::Mage | match_config::CharacterClass::Priest => "Wand Shot",
                            _ => "Auto Attack",
                        }
                    };
                    let message = format!(
                        "Team {} {}'s {} hits Team {} {} for {:.0} damage",
                        attacker_team,
                        attacker_class.name(),
                        attack_name,
                        target_team,
                        target_class.name(),
                        actual_damage
                    );

                    let is_killing_blow = !target.is_alive();
                    combat_log.log_damage(
                        combatant_id(attacker_team, attacker_class),
                        combatant_id(target_team, target_class),
                        attack_name.to_string(),
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
                            Some(combatant_id(attacker_team, attacker_class)),
                            death_message,
                        );
                    }
                }
            }
        }
    }
    
    // Spawn floating combat text for each target that took damage (batched)
    for (target_entity, total_damage) in damage_per_target {
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
                },
                PlayMatchEntity,
            ));
        }
    }
    
    // Update attacker damage dealt stats
    for (attacker_entity, damage) in damage_dealt_updates {
        if let Ok((_, _, mut attacker, _, _)) = combatants.get_mut(attacker_entity) {
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

/// Process interrupt attempts: interrupt target's cast and apply spell school lockout.
pub fn process_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    interrupts: Query<(Entity, &InterruptPending)>,
    mut targets: Query<(&mut CastingState, &Combatant)>,
    combatants: Query<&Combatant>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't process interrupts during victory celebration
    if celebration.is_some() {
        return;
    }
    
    for (interrupt_entity, interrupt) in interrupts.iter() {
        // Check if target is still casting
        if let Ok((mut cast_state, target_combatant)) = targets.get_mut(interrupt.target) {
            // Don't interrupt if already interrupted
            if cast_state.interrupted {
                commands.entity(interrupt_entity).despawn();
                continue;
            }
            
            // Get the spell school of the interrupted spell
            let interrupted_ability_def = cast_state.ability.definition();
            let interrupted_school = interrupted_ability_def.spell_school;
            let interrupted_spell_name = interrupted_ability_def.name;
            
            // Mark cast as interrupted
            cast_state.interrupted = true;
            cast_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds
            
            // Get caster info for logging
            let caster_info = if let Ok(caster) = combatants.get(interrupt.caster) {
                (caster.team, caster.class)
            } else {
                (0, match_config::CharacterClass::Warrior) // Fallback
            };
            
            // Apply spell school lockout aura
            // Store the locked school as the magnitude (cast to f32)
            let locked_school_value = match interrupted_school {
                SpellSchool::Physical => 0.0,
                SpellSchool::Frost => 1.0,
                SpellSchool::Holy => 2.0,
                SpellSchool::Shadow => 3.0,
                SpellSchool::None => 4.0,
            };
            
            commands.spawn(AuraPending {
                target: interrupt.target,
                aura: Aura {
                    effect_type: AuraType::SpellSchoolLockout,
                    duration: interrupt.lockout_duration,
                    magnitude: locked_school_value,
                    break_on_damage_threshold: 0.0,
                    accumulated_damage: 0.0,
                    tick_interval: 0.0,
                    time_until_next_tick: 0.0,
                    caster: Some(interrupt.caster),
                    ability_name: interrupt.ability.definition().name.to_string(),
                },
            });
            
            // Log the interrupt
            let school_name = match interrupted_school {
                SpellSchool::Physical => "Physical",
                SpellSchool::Frost => "Frost",
                SpellSchool::Holy => "Holy",
                SpellSchool::Shadow => "Shadow",
                SpellSchool::None => "None",
            };
            
            let message = format!(
                "Team {} {} interrupts Team {} {}'s {} - {} school locked for {:.1}s",
                caster_info.0,
                caster_info.1.name(),
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
        
        // Despawn the interrupt entity
        commands.entity(interrupt_entity).despawn();
    }
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
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&mut CastingState>, Option<&ActiveAuras>)>,
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
    for (caster_entity, caster_transform, mut caster, casting_state, _auras) in combatants.iter_mut() {
        let Some(mut casting) = casting_state else {
            continue;
        };
        
        if !caster.is_alive() {
            // Cancel cast if caster dies
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
            let def = ability.definition();
            let target_entity = casting.target;
            
            // Consume mana
            caster.current_mana -= def.mana_cost;
            
            // Pre-calculate damage/healing (using caster's stats)
            let ability_damage = caster.calculate_ability_damage(&def);
            let ability_healing = caster.calculate_ability_healing(&def);
            
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
    for (caster_entity, caster_team, caster_class, caster_pos, ability_damage, ability_healing, ability, target_entity) in completed_casts {
        let def = ability.definition();
        
        // Get target
        let Some(target_entity) = target_entity else {
            continue;
        };
        
        // If this ability uses a projectile, spawn it and skip immediate effect application
        if let Some(projectile_speed) = def.projectile_speed {
            // Spawn projectile visual and logic entity
            commands.spawn((
                Projectile {
                    caster: caster_entity,
                    target: target_entity,
                    ability,
                    speed: projectile_speed,
                    caster_team,
                    caster_class,
                },
                PlayMatchEntity,
            ));
            continue; // Skip immediate damage/healing - projectile will handle it on impact
        }
        
        // Check if this is self-targeting (e.g., priest healing themselves)
        let is_self_target = target_entity == caster_entity;
        
        // Get target combatant
        let Ok((_, target_transform, mut target, _, target_auras)) = combatants.get_mut(target_entity) else {
            continue;
        };
        
        if !target.is_alive() {
            continue;
        }
        
        let target_pos = target_transform.translation;
        let distance = caster_pos.distance(target_pos);
        let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
        
        // Handle damage spells
        if def.is_damage() {
            // Use pre-calculated damage (already includes stat scaling)
            let damage = ability_damage;
            
            let actual_damage = damage.min(target.current_health);
            target.current_health = (target.current_health - damage).max(0.0);
            target.damage_taken += actual_damage;
            
            // Warriors generate Rage from taking damage
            if target.resource_type == ResourceType::Rage {
                let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
            }
            
            // Track damage for aura breaking
            commands.entity(target_entity).insert(DamageTakenThisFrame {
                amount: actual_damage,
            });
            
            // Break stealth on offensive ability use
            break_stealth.push(caster_entity);
            
            // Track damage dealt for caster (update later to avoid double borrow)
            if is_self_target {
                // Self-damage: target IS caster, so update now
                target.damage_dealt += actual_damage;
            } else {
                // Different target: collect for later update
                caster_damage_updates.push((caster_entity, actual_damage));
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
                },
                PlayMatchEntity,
            ));
            
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
            
            // Log the damage with structured data
            let is_killing_blow = !target.is_alive();
            let message = format!(
                "Team {} {}'s {} hits Team {} {} for {:.0} damage",
                caster_team,
                caster_class.name(),
                def.name,
                target.team,
                target.class.name(),
                actual_damage
            );
            combat_log.log_damage(
                combatant_id(caster_team, caster_class),
                combatant_id(target.team, target.class),
                def.name.to_string(),
                actual_damage,
                is_killing_blow,
                message,
            );
        }
        // Handle healing spells
        else if def.is_heal() {
            // Use pre-calculated healing (already includes stat scaling)
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
                },
                PlayMatchEntity,
            ));
            
            // Log the healing with structured data
            let message = format!(
                "Team {} {}'s {} heals Team {} {} for {:.0}",
                caster_team,
                caster_class.name(),
                def.name,
                target.team,
                target.class.name(),
                actual_healing
            );
            combat_log.log_healing(
                combatant_id(caster_team, caster_class),
                combatant_id(target.team, target.class),
                def.name.to_string(),
                actual_healing,
                message,
            );
        }
        
        // Apply aura if applicable (store for later application)
        if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
            // We'll apply auras in a separate pass to avoid borrow issues
            commands.spawn((
                AuraPending {
                    target: target_entity,
                    aura: Aura {
                        effect_type: aura_type,
                        duration,
                        magnitude,
                        break_on_damage_threshold: break_threshold,
                        accumulated_damage: 0.0,
                        tick_interval: 0.0,
                        time_until_next_tick: 0.0,
                        caster: Some(caster_entity),
                        ability_name: def.name.to_string(),
                    },
                },
                PlayMatchEntity,
            ));
            
            info!(
                "Queued {:?} aura for Team {} {} (magnitude: {}, duration: {}s)",
                aura_type,
                target.team,
                target.class.name(),
                magnitude,
                duration
            );
        }

        // Track cooldown if ability has one
        if def.cooldown > 0.0 {
            cooldown_updates.push((caster_entity, ability, def.cooldown));
        }

        // Check for death (log if killed by non-damage abilities/auras)
        // Note: damage abilities already log death via is_killing_blow above
        if !target.is_alive() && !def.is_damage() {
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

