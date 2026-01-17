//! Combat AI Systems
//!
//! Handles all AI decision-making for combatants:
//! - Target acquisition (choosing which enemy to fight)
//! - Ability decisions (class-specific AI for using abilities)
//! - Interrupt decisions (when to interrupt enemy casts)

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::match_config;
use super::components::*;
use super::abilities::AbilityType;
use super::{MELEE_RANGE, is_spell_school_locked, get_next_fct_offset};
use super::combat_core::combatant_id;

/// Helper function to spawn a speech bubble when a combatant uses an ability
pub fn spawn_speech_bubble(
    commands: &mut Commands,
    owner: Entity,
    ability_name: &str,
) {
    commands.spawn((
        SpeechBubble {
            owner,
            text: format!("{}!", ability_name),
            lifetime: 2.0, // 2 seconds
        },
        PlayMatchEntity,
    ));
}

pub fn acquire_targets(
    countdown: Res<MatchCountdown>,
    config: Res<match_config::MatchConfig>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform, Option<&ActiveAuras>)>,
) {
    // Don't acquire targets until gates open
    if !countdown.gates_opened {
        return;
    }

    // First pass: identify which entities have Shadow Sight
    let shadow_sight_holders: std::collections::HashSet<Entity> = combatants
        .iter()
        .filter_map(|(entity, _, _, auras)| {
            if let Some(active) = auras {
                if active.auras.iter().any(|a| a.effect_type == AuraType::ShadowSight) {
                    return Some(entity);
                }
            }
            None
        })
        .collect();

    // Build list of all alive combatants with their info
    // Tuple: (entity, position, stealthed, has_shadow_sight)
    let mut team1_combatants: Vec<(Entity, Vec3, bool, bool)> = Vec::new();
    let mut team2_combatants: Vec<(Entity, Vec3, bool, bool)> = Vec::new();

    for (entity, c, transform, _) in combatants.iter() {
        if !c.is_alive() {
            continue;
        }

        let has_shadow_sight = shadow_sight_holders.contains(&entity);

        if c.team == 1 {
            team1_combatants.push((entity, transform.translation, c.stealthed, has_shadow_sight));
        } else {
            team2_combatants.push((entity, transform.translation, c.stealthed, has_shadow_sight));
        }
    }

    // Sort by entity ID to ensure deterministic ordering matching spawn order
    // Entity IDs are assigned sequentially at spawn time
    team1_combatants.sort_by_key(|(entity, _, _, _)| entity.index());
    team2_combatants.sort_by_key(|(entity, _, _, _)| entity.index());

    // For each combatant, ensure they have a valid target
    for (entity, mut combatant, transform, _) in combatants.iter_mut() {
        if !combatant.is_alive() {
            combatant.target = None;
            continue;
        }

        // Check if this combatant has Shadow Sight
        let i_have_shadow_sight = shadow_sight_holders.contains(&entity);

        // Get enemy team combatants and kill target priority
        let (enemy_combatants, kill_target_index) = if combatant.team == 1 {
            (&team2_combatants, config.team1_kill_target)
        } else {
            (&team1_combatants, config.team2_kill_target)
        };

        // Visibility check: can see enemy if:
        // 1. Enemy is not stealthed, OR
        // 2. I have Shadow Sight buff, OR
        // 3. Enemy has Shadow Sight buff (they're revealed by picking it up)
        let can_see = |stealthed: bool, enemy_has_shadow_sight: bool| -> bool {
            !stealthed || i_have_shadow_sight || enemy_has_shadow_sight
        };

        // Check if current target is still valid (alive, on enemy team, and visible)
        let target_valid = combatant.target.and_then(|target_entity| {
            enemy_combatants
                .iter()
                .find(|(e, _, _, _)| *e == target_entity)
                .filter(|(_, _, stealthed, enemy_ss)| can_see(*stealthed, *enemy_ss))
        }).is_some();

        // If no valid target, acquire a new one
        if !target_valid {
            // Priority 1: Check if kill target is set and visible
            let kill_target = if let Some(index) = kill_target_index {
                enemy_combatants
                    .get(index)
                    .filter(|(_, _, stealthed, enemy_ss)| can_see(*stealthed, *enemy_ss))
                    .map(|(entity, _, _, _)| *entity)
            } else {
                None
            };

            if let Some(priority_target) = kill_target {
                // Use the kill target
                combatant.target = Some(priority_target);
            } else {
                // Priority 2: Fall back to nearest visible enemy
                let my_pos = transform.translation;
                let nearest_enemy = enemy_combatants
                    .iter()
                    .filter(|(_, _, stealthed, enemy_ss)| can_see(*stealthed, *enemy_ss))
                    .min_by(|(_, pos_a, _, _), (_, pos_b, _, _)| {
                        let dist_a = my_pos.distance(*pos_a);
                        let dist_b = my_pos.distance(*pos_b);
                        dist_a.partial_cmp(&dist_b).unwrap()
                    });

                combatant.target = nearest_enemy.map(|(entity, _, _, _)| *entity);
            }
        }
    }
}
pub fn decide_abilities(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform, Option<&mut ActiveAuras>), Without<CastingState>>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't cast abilities during victory celebration
    if celebration.is_some() {
        return;
    }
    // Build position and info maps from all combatants
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, _, transform, _)| (entity, transform.translation))
        .collect();
    
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass, f32, f32)> = combatants
        .iter()
        .map(|(entity, combatant, _, _)| {
            (entity, (combatant.team, combatant.class, combatant.current_health, combatant.max_health))
        })
        .collect();
    
    // Map of entities to their active auras (for checking buffs/debuffs)
    let active_auras_map: std::collections::HashMap<Entity, Vec<Aura>> = combatants
        .iter()
        .filter_map(|(entity, _, _, auras_opt)| {
            auras_opt.map(|auras| (entity, auras.auras.clone()))
        })
        .collect();
    
    // Queue for Ambush attacks (attacker, target, damage, team, class)
    // Queue for instant ability attacks (Ambush, Sinister Strike, Mortal Strike)
    // Format: (attacker_entity, target_entity, damage, attacker_team, attacker_class, ability_type)
    let mut instant_attacks: Vec<(Entity, Entity, f32, u8, match_config::CharacterClass, AbilityType)> = Vec::new();

    // Track targets that have been shielded THIS FRAME to prevent same-frame double-shielding
    // This handles the case where multiple Priests try to shield the same target before AuraPending is processed
    let mut shielded_this_frame: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    
    // Queue for Frost Nova damage (caster, target, damage, caster_team, caster_class, target_pos)
    let mut frost_nova_damage: Vec<(Entity, Entity, f32, u8, match_config::CharacterClass, Vec3)> = Vec::new();
    
    for (entity, mut combatant, transform, auras) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // WoW Mechanic: Cannot use abilities while stunned or feared
        let is_incapacitated = if let Some(ref auras) = auras {
            auras.auras.iter().any(|a| matches!(a.effect_type, AuraType::Stun | AuraType::Fear))
        } else {
            false
        };
        if is_incapacitated {
            continue;
        }
        
        let my_pos = transform.translation;
        
        // Mages cast spells on enemies
        if combatant.class == match_config::CharacterClass::Mage {
            // Check if global cooldown is active
            if combatant.global_cooldown > 0.0 {
                continue; // Can't use abilities during GCD
            }

            // Priority -1: Cast Ice Barrier on self if not shielded
            // Pre-combat (full HP): Always cast
            // In-combat: Only recast when HP < 80%
            let has_absorb_shield = if let Some(auras) = active_auras_map.get(&entity) {
                auras.iter().any(|a| a.effect_type == AuraType::Absorb)
            } else {
                false
            };

            let is_full_hp = combatant.current_health >= combatant.max_health;
            let is_below_threshold = combatant.current_health < combatant.max_health * 0.8;
            let should_shield = !has_absorb_shield && (is_full_hp || is_below_threshold);

            if should_shield {
                let ice_barrier = AbilityType::IceBarrier;
                let barrier_def = ice_barrier.definition();
                let barrier_on_cooldown = combatant.ability_cooldowns.contains_key(&ice_barrier);

                if !barrier_on_cooldown && combatant.current_mana >= barrier_def.mana_cost {
                    // Spawn speech bubble
                    spawn_speech_bubble(&mut commands, entity, "Ice Barrier");

                    // Consume mana
                    combatant.current_mana -= barrier_def.mana_cost;

                    // Put ability on cooldown
                    combatant.ability_cooldowns.insert(ice_barrier, barrier_def.cooldown);

                    // Trigger global cooldown
                    combatant.global_cooldown = 1.5;

                    // Log ability cast
                    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                    combat_log.log_ability_cast(
                        caster_id,
                        "Ice Barrier".to_string(),
                        None,
                        format!("Team {} {} casts Ice Barrier", combatant.team, combatant.class.name()),
                    );

                    // Apply the absorb shield aura
                    let (aura_type, duration, magnitude, _) = barrier_def.applies_aura.unwrap();
                    commands.spawn(AuraPending {
                        target: entity,
                        aura: Aura {
                            effect_type: aura_type,
                            duration,
                            magnitude,
                            break_on_damage_threshold: 0.0,
                            accumulated_damage: 0.0,
                            tick_interval: 0.0,
                            time_until_next_tick: 0.0,
                            caster: Some(entity),
                            ability_name: "Ice Barrier".to_string(),
                            fear_direction: (0.0, 0.0),
                            fear_direction_timer: 0.0,
                        },
                    });

                    info!(
                        "Team {} {} casts Ice Barrier",
                        combatant.team,
                        combatant.class.name()
                    );

                    continue; // Done this frame
                }
            }

            // Priority 0: Cast Arcane Intellect on allies with mana who don't have it
            // (Pre-combat buffing phase)
            let mut unbuffed_mana_ally: Option<(Entity, Vec3)> = None;

            for (ally_entity, &(ally_team, ally_class, ally_hp, _ally_max_hp)) in combatant_info.iter() {
                // Must be same team, alive, and use mana
                if ally_team != combatant.team || ally_hp <= 0.0 {
                    continue;
                }

                // Only buff mana users (Mage, Priest, Warlock)
                let uses_mana = matches!(
                    ally_class,
                    match_config::CharacterClass::Mage
                        | match_config::CharacterClass::Priest
                        | match_config::CharacterClass::Warlock
                );
                if !uses_mana {
                    continue;
                }

                // Check if ally already has MaxManaIncrease buff
                let has_arcane_intellect = if let Some(auras) = active_auras_map.get(ally_entity) {
                    auras.iter().any(|a| a.effect_type == AuraType::MaxManaIncrease)
                } else {
                    false
                };

                if has_arcane_intellect {
                    continue; // Already buffed
                }

                // Get position
                let Some(&ally_pos) = positions.get(ally_entity) else {
                    continue;
                };

                // Found an unbuffed mana ally
                unbuffed_mana_ally = Some((*ally_entity, ally_pos));
                break; // Buff one ally at a time
            }

            // Cast Arcane Intellect on unbuffed mana ally
            if let Some((buff_target, target_pos)) = unbuffed_mana_ally {
                let ability = AbilityType::ArcaneIntellect;
                let def = ability.definition();

                // Check if spell school is locked out
                if !is_spell_school_locked(def.spell_school, auras.as_deref()) && ability.can_cast(&combatant, target_pos, my_pos) {
                    // Consume mana
                    combatant.current_mana -= def.mana_cost;

                    // Trigger global cooldown
                    combatant.global_cooldown = 1.5;

                    // Log ability cast for timeline
                    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                    let target_id = combatant_info.get(&buff_target).map(|(team, class, _, _)| {
                        format!("Team {} {}", team, class.name())
                    });
                    combat_log.log_ability_cast(
                        caster_id,
                        "Arcane Intellect".to_string(),
                        target_id,
                        format!("Team {} {} casts Arcane Intellect", combatant.team, combatant.class.name()),
                    );

                    // Apply the buff aura immediately (instant cast)
                    if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                        commands.spawn(AuraPending {
                            target: buff_target,
                            aura: Aura {
                                effect_type: aura_type,
                                duration,
                                magnitude,
                                break_on_damage_threshold: break_threshold,
                                accumulated_damage: 0.0,
                                tick_interval: 0.0,
                                time_until_next_tick: 0.0,
                                caster: Some(entity),
                                ability_name: def.name.to_string(),
                                fear_direction: (0.0, 0.0),
                                fear_direction_timer: 0.0,
                            },
                        });
                    }

                    info!(
                        "Team {} {} casts Arcane Intellect on ally",
                        combatant.team,
                        combatant.class.name()
                    );

                    continue; // Done this frame
                }
            }

            // First priority: Use Frost Nova if enemies are in melee range (defensive ability)
            let frost_nova = AbilityType::FrostNova;
            let nova_def = frost_nova.definition();
            let nova_on_cooldown = combatant.ability_cooldowns.contains_key(&frost_nova);
            
            // Check if Frost school is locked out
            let frost_locked_out = is_spell_school_locked(nova_def.spell_school, auras.as_deref());
            
            if !nova_on_cooldown && !frost_locked_out && combatant.current_mana >= nova_def.mana_cost {
                // Check if any enemies are within Frost Nova range (melee range for threat detection)
                let enemies_in_melee_range = positions.iter().any(|(enemy_entity, &enemy_pos)| {
                    if let Some(&(enemy_team, _, _, _)) = combatant_info.get(enemy_entity) {
                        if enemy_team != combatant.team {
                            let distance = my_pos.distance(enemy_pos);
                            return distance <= MELEE_RANGE;
                        }
                    }
                    false
                });
                
                if enemies_in_melee_range {
                    // Spawn speech bubble for Frost Nova
                    spawn_speech_bubble(&mut commands, entity, "Frost Nova");

                    // Consume mana
                    combatant.current_mana -= nova_def.mana_cost;

                    // Put ability on cooldown
                    combatant.ability_cooldowns.insert(frost_nova, nova_def.cooldown);

                    // Trigger global cooldown (1.5s standard WoW GCD)
                    combatant.global_cooldown = 1.5;

                    // Log ability cast for timeline (AOE ability, no specific target)
                    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                    combat_log.log_ability_cast(
                        caster_id,
                        "Frost Nova".to_string(),
                        None,
                        format!("Team {} {} casts Frost Nova", combatant.team, combatant.class.name()),
                    );

                    // Collect enemies in range for damage and root
                    let mut frost_nova_targets: Vec<(Entity, Vec3, u8, match_config::CharacterClass)> = Vec::new();
                    for (enemy_entity, &enemy_pos) in positions.iter() {
                        if let Some(&(enemy_team, enemy_class, _, _)) = combatant_info.get(enemy_entity) {
                            if enemy_team != combatant.team {
                                let distance = my_pos.distance(enemy_pos);
                                if distance <= nova_def.range {
                                    frost_nova_targets.push((*enemy_entity, enemy_pos, enemy_team, enemy_class));
                                }
                            }
                        }
                    }
                    
                    // Queue damage and apply root to all targets
                    for (target_entity, target_pos, target_team, target_class) in &frost_nova_targets {
                        // Calculate damage (with stat scaling)
                        let damage = combatant.calculate_ability_damage(&nova_def, &mut game_rng);
                        
                        // Queue damage for later application
                        frost_nova_damage.push((entity, *target_entity, damage, combatant.team, combatant.class, *target_pos));
                        
                        // Apply aura (spawn separate AuraPending entity)
                        if let Some((aura_type, duration, magnitude, break_threshold)) = nova_def.applies_aura {
                            commands.spawn(AuraPending {
                                target: *target_entity,
                                aura: Aura {
                                    effect_type: aura_type,
                                    duration,
                                    magnitude,
                                    break_on_damage_threshold: break_threshold,
                                    accumulated_damage: 0.0,
                                    tick_interval: 0.0,
                                    time_until_next_tick: 0.0,
                                    caster: Some(entity),
                                    ability_name: nova_def.name.to_string(),
                                    fear_direction: (0.0, 0.0),
                                    fear_direction_timer: 0.0,
                                },
                            });
                        }
                    }
                    
                    // Set kiting timer - mage should move away from enemies for the root duration
                    combatant.kiting_timer = nova_def.applies_aura.unwrap().1; // Root duration (6.0s)
                    
                    info!(
                        "Team {} {} casts Frost Nova! (AOE root) - {} enemies affected",
                        combatant.team,
                        combatant.class.name(),
                        frost_nova_targets.len()
                    );
                    
                    continue; // Don't cast Frostbolt this frame
                }
            }
            
            // Second priority: Cast Frostbolt on target
            // While kiting, only cast if we're at a safe distance (beyond melee range + buffer)
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            let distance_to_target = my_pos.distance(target_pos);
            
            // While kiting, only cast if we're at a safe distance
            // Safe distance = beyond melee range + buffer (8 units gives good tactical spacing)
            const SAFE_KITING_DISTANCE: f32 = 8.0;
            if combatant.kiting_timer > 0.0 && distance_to_target < SAFE_KITING_DISTANCE {
                continue; // Too close while kiting, focus on movement
            }
            
            // Check if global cooldown is active
            if combatant.global_cooldown > 0.0 {
                continue; // Can't start casting during GCD
            }
            
            // Try to cast Frostbolt
            let ability = AbilityType::Frostbolt;
            let def = ability.definition();
            
            // Check if spell school is locked out
            if is_spell_school_locked(def.spell_school, auras.as_deref()) {
                continue; // Can't cast - spell school is locked
            }
            
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();
                
                // Trigger global cooldown (1.5s standard WoW GCD)
                // GCD starts when cast BEGINS, not when it completes
                combatant.global_cooldown = 1.5;
                
                // Start casting
                commands.entity(entity).insert(CastingState {
                    ability,
                    time_remaining: def.cast_time,
                    target: Some(target_entity),
                    interrupted: false,
                    interrupted_display_time: 0.0,
                });

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                    format!("Team {} {}", team, class.name())
                });
                combat_log.log_ability_cast(
                    caster_id,
                    def.name.to_string(),
                    target_id,
                    format!("Team {} {} begins casting {}", combatant.team, combatant.class.name(), def.name),
                );

                info!(
                    "Team {} {} starts casting {} on enemy",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }
        // Priests cast Flash Heal on injured allies
        else if combatant.class == match_config::CharacterClass::Priest {
            // Check if global cooldown is active (check once for all abilities)
            if combatant.global_cooldown > 0.0 {
                continue; // Can't cast during GCD
            }
            
            // Priority 0: Cast Power Word: Fortitude on allies who don't have it
            // (Pre-combat buffing phase)
            let mut unbuffed_ally: Option<(Entity, Vec3)> = None;
            
            for (ally_entity, &(ally_team, _ally_class, ally_hp, _ally_max_hp)) in combatant_info.iter() {
                // Must be same team and alive
                if ally_team != combatant.team || ally_hp <= 0.0 {
                    continue;
                }
                
                // Check if ally already has MaxHealthIncrease buff
                let has_fortitude = if let Some(auras) = active_auras_map.get(ally_entity) {
                    auras.iter().any(|a| a.effect_type == AuraType::MaxHealthIncrease)
                } else {
                    false
                };
                
                if has_fortitude {
                    continue; // Already buffed
                }
                
                // Get position
                let Some(&ally_pos) = positions.get(ally_entity) else {
                    continue;
                };
                
                // Found an unbuffed ally
                unbuffed_ally = Some((*ally_entity, ally_pos));
                break; // Buff one ally at a time
            }
            
            // Cast Fortitude on unbuffed ally
            if let Some((buff_target, target_pos)) = unbuffed_ally {
                let ability = AbilityType::PowerWordFortitude;
                let def = ability.definition();
                
                // Check if spell school is locked out
                if !is_spell_school_locked(def.spell_school, auras.as_deref()) && ability.can_cast(&combatant, target_pos, my_pos) {
                    let def = ability.definition();

                    // Consume mana
                    combatant.current_mana -= def.mana_cost;

                    // Trigger global cooldown
                    combatant.global_cooldown = 1.5;

                    // Log ability cast for timeline
                    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                    let target_id = combatant_info.get(&buff_target).map(|(team, class, _, _)| {
                        format!("Team {} {}", team, class.name())
                    });
                    combat_log.log_ability_cast(
                        caster_id,
                        "Power Word: Fortitude".to_string(),
                        target_id,
                        format!("Team {} {} casts Power Word: Fortitude", combatant.team, combatant.class.name()),
                    );

                    // Apply the buff aura immediately (instant cast)
                    if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                        commands.spawn(AuraPending {
                            target: buff_target,
                            aura: Aura {
                                effect_type: aura_type,
                                duration,
                                magnitude,
                                break_on_damage_threshold: break_threshold,
                                accumulated_damage: 0.0,
                                tick_interval: 0.0,
                                time_until_next_tick: 0.0,
                                caster: Some(entity),
                                ability_name: def.name.to_string(),
                                fear_direction: (0.0, 0.0),
                                fear_direction_timer: 0.0,
                            },
                        });
                    }

                    info!(
                        "Team {} {} casts Power Word: Fortitude on ally",
                        combatant.team,
                        combatant.class.name()
                    );

                    continue; // Done this frame
                }
            }

            // Priority 0.5: Cast Power Word: Shield on allies without Weakened Soul
            // Pre-combat (full HP allies): Shield everyone
            // In-combat: Shield allies below 70% HP
            let pw_shield = AbilityType::PowerWordShield;
            let pw_shield_def = pw_shield.definition();

            if !is_spell_school_locked(pw_shield_def.spell_school, auras.as_deref())
               && combatant.current_mana >= pw_shield_def.mana_cost
            {
                let mut shield_target: Option<(Entity, Vec3)> = None;

                // Find ally to shield (prioritize lowest HP)
                let mut best_candidate: Option<(Entity, Vec3, f32)> = None; // (entity, pos, hp_percent)

                for (ally_entity, &(ally_team, _ally_class, ally_hp, ally_max_hp)) in combatant_info.iter() {
                    // Must be same team and alive
                    if ally_team != combatant.team || ally_hp <= 0.0 {
                        continue;
                    }

                    // Check if ally has Weakened Soul or already has Power Word: Shield
                    // Weakened Soul from ANY priest prevents PW:Shield (not just the caster's WS)
                    // Note: Other absorb shields (Ice Barrier) do NOT prevent PW:Shield - they can coexist
                    let ally_auras = active_auras_map.get(ally_entity);
                    let has_weakened_soul = ally_auras
                        .map_or(false, |auras| auras.iter().any(|a| a.effect_type == AuraType::WeakenedSoul));
                    let has_pw_shield = ally_auras
                        .map_or(false, |auras| auras.iter().any(|a|
                            a.effect_type == AuraType::Absorb && a.ability_name == "Power Word: Shield"
                        ));

                    // Also check if target was shielded by another Priest THIS FRAME
                    let shielded_this_frame_check = shielded_this_frame.contains(ally_entity);

                    if has_weakened_soul || has_pw_shield || shielded_this_frame_check {
                        continue; // Can't shield this target
                    }

                    // Get position
                    let Some(&ally_pos) = positions.get(ally_entity) else {
                        continue;
                    };

                    let hp_percent = ally_hp / ally_max_hp;

                    // Pre-combat (full HP): Shield anyone
                    // In-combat: Only shield if below 70% HP
                    let is_full_hp = hp_percent >= 1.0;
                    let is_below_threshold = hp_percent < 0.7;

                    if is_full_hp || is_below_threshold {
                        // Track lowest HP candidate
                        match best_candidate {
                            None => best_candidate = Some((*ally_entity, ally_pos, hp_percent)),
                            Some((_, _, best_percent)) if hp_percent < best_percent => {
                                best_candidate = Some((*ally_entity, ally_pos, hp_percent));
                            }
                            _ => {}
                        }
                    }
                }

                if let Some((target, target_pos, _)) = best_candidate {
                    shield_target = Some((target, target_pos));
                }

                // Cast Power Word: Shield on target
                if let Some((shield_entity, target_pos)) = shield_target {
                    if pw_shield.can_cast(&combatant, target_pos, my_pos) {
                        // Consume mana
                        combatant.current_mana -= pw_shield_def.mana_cost;

                        // Trigger global cooldown
                        combatant.global_cooldown = 1.5;

                        // Log ability cast
                        let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                        let target_id = combatant_info.get(&shield_entity).map(|(team, class, _, _)| {
                            format!("Team {} {}", team, class.name())
                        });
                        combat_log.log_ability_cast(
                            caster_id,
                            "Power Word: Shield".to_string(),
                            target_id,
                            format!("Team {} {} casts Power Word: Shield", combatant.team, combatant.class.name()),
                        );

                        // Apply the absorb shield aura
                        let (aura_type, duration, magnitude, _) = pw_shield_def.applies_aura.unwrap();
                        commands.spawn(AuraPending {
                            target: shield_entity,
                            aura: Aura {
                                effect_type: aura_type,
                                duration,
                                magnitude,
                                break_on_damage_threshold: 0.0,
                                accumulated_damage: 0.0,
                                tick_interval: 0.0,
                                time_until_next_tick: 0.0,
                                caster: Some(entity),
                                ability_name: "Power Word: Shield".to_string(),
                                fear_direction: (0.0, 0.0),
                                fear_direction_timer: 0.0,
                            },
                        });

                        // Apply Weakened Soul debuff (15 seconds)
                        commands.spawn(AuraPending {
                            target: shield_entity,
                            aura: Aura {
                                effect_type: AuraType::WeakenedSoul,
                                duration: 15.0,
                                magnitude: 0.0, // Not used
                                break_on_damage_threshold: 0.0,
                                accumulated_damage: 0.0,
                                tick_interval: 0.0,
                                time_until_next_tick: 0.0,
                                caster: Some(entity),
                                ability_name: "Weakened Soul".to_string(),
                                fear_direction: (0.0, 0.0),
                                fear_direction_timer: 0.0,
                            },
                        });

                        // Mark this target as shielded this frame (prevents other Priests from shielding same target)
                        shielded_this_frame.insert(shield_entity);

                        continue; // Done this frame
                    }
                }
            }

            // Find the lowest HP ally (including self)
            let mut lowest_hp_ally: Option<(Entity, f32, Vec3)> = None;
            
            for (ally_entity, &(ally_team, _ally_class, ally_hp, ally_max_hp)) in combatant_info.iter() {
                // Must be same team and alive
                if ally_team != combatant.team || ally_hp <= 0.0 {
                    continue;
                }

                // Only heal if damaged (below 90% health)
                let hp_percent = ally_hp / ally_max_hp;
                if hp_percent >= 0.9 {
                    continue;
                }
                
                // Get position
                let Some(&ally_pos) = positions.get(ally_entity) else {
                    continue;
                };
                
                // Track lowest HP ally
                match lowest_hp_ally {
                    None => lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos)),
                    Some((_, lowest_percent, _)) if hp_percent < lowest_percent => {
                        lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos));
                    }
                    _ => {}
                }
            }
            
            // Priority 1: Cast heal on lowest HP ally if found
            if let Some((heal_target, _, target_pos)) = lowest_hp_ally {
                let ability = AbilityType::FlashHeal;
                let def = ability.definition();
                
                // Check if spell school is locked out
                if !is_spell_school_locked(def.spell_school, auras.as_deref()) && ability.can_cast(&combatant, target_pos, my_pos) {
                    let def = ability.definition();

                    // Trigger global cooldown (1.5s standard WoW GCD)
                    // GCD starts when cast BEGINS, not when it completes
                    combatant.global_cooldown = 1.5;

                    // Start casting
                    commands.entity(entity).insert(CastingState {
                        ability,
                        time_remaining: def.cast_time,
                        target: Some(heal_target),
                        interrupted: false,
                        interrupted_display_time: 0.0,
                    });

                    // Log ability cast for timeline
                    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                    let target_id = combatant_info.get(&heal_target).map(|(team, class, _, _)| {
                        format!("Team {} {}", team, class.name())
                    });
                    combat_log.log_ability_cast(
                        caster_id,
                        def.name.to_string(),
                        target_id,
                        format!("Team {} {} begins casting {}", combatant.team, combatant.class.name(), def.name),
                    );

                    info!(
                        "Team {} {} starts casting {} on ally",
                        combatant.team,
                        combatant.class.name(),
                        def.name
                    );

                    continue; // Done this frame
                }
            }

            // Priority 2: Cast Mind Blast on enemy if no healing needed
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // Check if Mind Blast is off cooldown
            let ability = AbilityType::MindBlast;
            let on_cooldown = combatant.ability_cooldowns.contains_key(&ability);
            let def = ability.definition();
            
            // Check if spell school is locked out
            if !on_cooldown && !is_spell_school_locked(def.spell_school, auras.as_deref()) && ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();

                // Put on cooldown
                combatant.ability_cooldowns.insert(ability, def.cooldown);

                // Trigger global cooldown (1.5s standard WoW GCD)
                // GCD starts when cast BEGINS, not when it completes
                combatant.global_cooldown = 1.5;

                // Start casting
                commands.entity(entity).insert(CastingState {
                    ability,
                    time_remaining: def.cast_time,
                    target: Some(target_entity),
                    interrupted: false,
                    interrupted_display_time: 0.0,
                });

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                    format!("Team {} {}", team, class.name())
                });
                combat_log.log_ability_cast(
                    caster_id,
                    def.name.to_string(),
                    target_id,
                    format!("Team {} {} begins casting {}", combatant.team, combatant.class.name(), def.name),
                );

                info!(
                    "Team {} {} starts casting {} on enemy",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }

        // Warriors use Charge (gap closer), Mortal Strike, Rend, and Heroic Strike
        if combatant.class == match_config::CharacterClass::Warrior {
            // Check if global cooldown is active for other abilities
            if combatant.global_cooldown > 0.0 {
                continue; // Can't use other abilities during GCD
            }

            // Priority 0: Cast Battle Shout on nearby allies who don't have it
            // (Pre-combat buffing phase - self-cast AOE)
            const BATTLE_SHOUT_RANGE: f32 = 30.0; // Affects all allies within 30 units

            // Check if any nearby ally (including self) needs the buff
            let mut needs_battle_shout = false;
            let mut allies_to_buff: Vec<Entity> = Vec::new();

            for (ally_entity, &(ally_team, _ally_class, ally_hp, _ally_max_hp)) in combatant_info.iter() {
                // Must be same team and alive
                if ally_team != combatant.team || ally_hp <= 0.0 {
                    continue;
                }

                // Get ally position to check range
                let Some(&ally_pos) = positions.get(ally_entity) else {
                    continue;
                };

                let distance_to_ally = my_pos.distance(ally_pos);
                if distance_to_ally > BATTLE_SHOUT_RANGE {
                    continue; // Too far away
                }

                // Check if ally already has AttackPowerIncrease buff
                let has_battle_shout = if let Some(auras) = active_auras_map.get(ally_entity) {
                    auras.iter().any(|a| a.effect_type == AuraType::AttackPowerIncrease)
                } else {
                    false
                };

                if !has_battle_shout {
                    needs_battle_shout = true;
                    allies_to_buff.push(*ally_entity);
                }
            }

            // Cast Battle Shout if any nearby ally needs it
            if needs_battle_shout {
                let ability = AbilityType::BattleShout;
                let def = ability.definition();

                // Battle Shout costs rage, check if we have enough (or it's free pre-combat)
                // Note: Warriors start with 0 rage, so we'll make it free (cost 0) or check resource
                if combatant.current_mana >= def.mana_cost || def.mana_cost == 0.0 {
                    // Consume resource (rage)
                    combatant.current_mana -= def.mana_cost;

                    // Trigger global cooldown
                    combatant.global_cooldown = 1.5;

                    // Log ability cast for timeline (self-cast AOE)
                    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                    combat_log.log_ability_cast(
                        caster_id,
                        "Battle Shout".to_string(),
                        None, // AOE, no specific target
                        format!("Team {} {} uses Battle Shout", combatant.team, combatant.class.name()),
                    );

                    // Apply the buff aura to all nearby allies
                    if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                        for ally_entity in allies_to_buff {
                            commands.spawn(AuraPending {
                                target: ally_entity,
                                aura: Aura {
                                    effect_type: aura_type,
                                    duration,
                                    magnitude,
                                    break_on_damage_threshold: break_threshold,
                                    accumulated_damage: 0.0,
                                    tick_interval: 0.0,
                                    time_until_next_tick: 0.0,
                                    caster: Some(entity),
                                    ability_name: def.name.to_string(),
                                    fear_direction: (0.0, 0.0),
                                    fear_direction_timer: 0.0,
                                },
                            });
                        }
                    }

                    info!(
                        "Team {} {} uses Battle Shout",
                        combatant.team,
                        combatant.class.name()
                    );

                    continue; // Done this frame
                }
            }

            // Check if we have an enemy target for combat abilities
            let Some(target_entity) = combatant.target else {
                continue;
            };

            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };

            let distance_to_target = my_pos.distance(target_pos);

            // NOTE: Interrupt checking (Pummel) is now handled in the dedicated check_interrupts system
            // which runs after apply_deferred so it can see CastingState components from this frame
            
            // Priority 1: Use Charge to close distance if target is at medium range
            // Charge requirements:
            // - Minimum 8 units (can't waste at melee range)
            // - Maximum 25 units (ability range)
            // - Not rooted (can't charge while rooted)
            // - Off cooldown
            const CHARGE_MIN_RANGE: f32 = 8.0;
            let charge = AbilityType::Charge;
            let charge_def = charge.definition();
            let charge_on_cooldown = combatant.ability_cooldowns.contains_key(&charge);
            
            // Check if rooted
            let is_rooted = if let Some(ref auras) = auras {
                auras.auras.iter().any(|aura| matches!(aura.effect_type, AuraType::Root))
            } else {
                false
            };
            
            if !charge_on_cooldown
                && !is_rooted
                && distance_to_target >= CHARGE_MIN_RANGE
                && distance_to_target <= charge_def.range {

                // Use Charge!
                combatant.ability_cooldowns.insert(charge, charge_def.cooldown);
                combatant.global_cooldown = 1.5;

                // Add ChargingState component to enable high-speed movement
                commands.entity(entity).insert(ChargingState {
                    target: target_entity,
                });

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                    format!("Team {} {}", team, class.name())
                });
                combat_log.log_ability_cast(
                    caster_id,
                    "Charge".to_string(),
                    target_id,
                    format!("Team {} {} uses Charge", combatant.team, combatant.class.name()),
                );

                info!(
                    "Team {} {} uses {} on enemy (distance: {:.1} units)",
                    combatant.team,
                    combatant.class.name(),
                    charge_def.name,
                    distance_to_target
                );

                continue; // Done this frame
            }
            
            // Priority 2: Apply Rend if target doesn't have it
            let target_has_rend = if let Some(auras) = active_auras_map.get(&target_entity) {
                auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime)
            } else {
                false
            };
            
            if !target_has_rend {
                let rend = AbilityType::Rend;
                let rend_def = rend.definition();
                let can_cast_rend = rend.can_cast(&combatant, target_pos, my_pos);
                
                if can_cast_rend {
                    // Consume rage
                    combatant.current_mana -= rend_def.mana_cost;

                    // Trigger global cooldown
                    combatant.global_cooldown = 1.5;

                    // Log ability cast for timeline
                    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                    let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                        format!("Team {} {}", team, class.name())
                    });
                    combat_log.log_ability_cast(
                        caster_id,
                        "Rend".to_string(),
                        target_id,
                        format!("Team {} {} uses Rend", combatant.team, combatant.class.name()),
                    );

                    // Apply the DoT aura
                    if let Some((aura_type, duration, magnitude, break_threshold)) = rend_def.applies_aura {
                        commands.spawn(AuraPending {
                            target: target_entity,
                            aura: Aura {
                                effect_type: aura_type,
                                duration,
                                magnitude,
                                break_on_damage_threshold: break_threshold,
                                accumulated_damage: 0.0,
                                tick_interval: 3.0, // Tick every 3 seconds
                                time_until_next_tick: 3.0, // First tick after 3 seconds
                                caster: Some(entity),
                                ability_name: rend_def.name.to_string(),
                                fear_direction: (0.0, 0.0),
                                fear_direction_timer: 0.0,
                            },
                        });
                    }

                    // Log Rend application to combat log
                    combat_log.log(
                        CombatLogEventType::Buff,
                        format!(
                            "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
                            combatant.team,
                            combatant.class.name()
                        )
                    );
                    
                    info!(
                        "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
                        combatant.team,
                        combatant.class.name()
                    );
                    
                    continue; // Done this frame
                }
            }
            
            // Priority 3: Use Mortal Strike if off cooldown and enough rage (high priority cooldown)
            let mortal_strike = AbilityType::MortalStrike;
            let ms_def = mortal_strike.definition();
            let ms_on_cooldown = combatant.ability_cooldowns.contains_key(&mortal_strike);
            let can_cast_ms = mortal_strike.can_cast(&combatant, target_pos, my_pos);
            
            if !ms_on_cooldown && can_cast_ms && combatant.current_mana >= ms_def.mana_cost {
                // Get target info
                let (target_team, target_class) = if let Some(&(team, class, _, _)) = combatant_info.get(&target_entity) {
                    (team, class)
                } else {
                    continue;
                };

                // Consume rage
                combatant.current_mana -= ms_def.mana_cost;

                // Put on cooldown
                combatant.ability_cooldowns.insert(mortal_strike, ms_def.cooldown);

                // Trigger global cooldown
                combatant.global_cooldown = 1.5;

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                combat_log.log_ability_cast(
                    caster_id,
                    "Mortal Strike".to_string(),
                    Some(format!("Team {} {}", target_team, target_class.name())),
                    format!("Team {} {} uses Mortal Strike", combatant.team, combatant.class.name()),
                );

                // Calculate damage
                let damage = combatant.calculate_ability_damage(&ms_def, &mut game_rng);

                // Queue damage to apply (collect for later to avoid borrow issues)
                instant_attacks.push((entity, target_entity, damage, combatant.team, combatant.class, mortal_strike));
                
                // Apply healing reduction aura
                if let Some((aura_type, duration, magnitude, break_threshold)) = ms_def.applies_aura {
                    commands.spawn(AuraPending {
                        target: target_entity,
                        aura: Aura {
                            effect_type: aura_type,
                            duration,
                            magnitude,
                            break_on_damage_threshold: break_threshold,
                            accumulated_damage: 0.0,
                            tick_interval: 0.0,
                            time_until_next_tick: 0.0,
                            caster: Some(entity),
                            ability_name: ms_def.name.to_string(),
                            fear_direction: (0.0, 0.0),
                            fear_direction_timer: 0.0,
                        },
                    });
                }

                // Note: Combat log and FCT are handled in the instant_attacks processing loop
                // to avoid duplicate entries
                
                info!(
                    "Team {} {} uses Mortal Strike for {:.0} damage!",
                    combatant.team,
                    combatant.class.name(),
                    damage
                );
                
                continue; // Done this frame
            }
            
            // Priority 4: Use Heroic Strike if target is in melee range
            // Only use Heroic Strike if we have excess rage (save rage for Rend/Pummel/MortalStrike)
            // Don't queue another Heroic Strike if one is already pending
            if combatant.next_attack_bonus_damage > 0.0 {
                continue;
            }
            
            // Try to use Heroic Strike if we have enough rage and target is in melee range
            let ability = AbilityType::HeroicStrike;
            let def = ability.definition();
            
            // Only use if we have enough rage for both Heroic Strike AND Rend+Pummel+MortalStrike reserve
            // Reserve: 10 (Rend) + 10 (Pummel) + 30 (Mortal Strike) = 50 rage minimum
            const RAGE_RESERVE: f32 = 50.0;
            let can_afford_heroic_strike = combatant.current_mana >= (def.mana_cost + RAGE_RESERVE);
            
            if can_afford_heroic_strike && ability.can_cast(&combatant, target_pos, my_pos) {
                // Since it's instant, apply the effect immediately
                // Consume rage
                combatant.current_mana -= def.mana_cost;
                
                // Set bonus damage for next auto-attack (50% of base attack damage)
                let bonus_damage = combatant.attack_damage * 0.5;
                combatant.next_attack_bonus_damage = bonus_damage;
                
                // Trigger global cooldown (1.5s standard WoW GCD)
                combatant.global_cooldown = 1.5;
                
                info!(
                    "Team {} {} uses {} (next attack +{:.0} damage)",
                    combatant.team,
                    combatant.class.name(),
                    def.name,
                    bonus_damage
                );
            }
        }
        
        // Rogues use Ambush from stealth (instant ability, high damage)
        if combatant.class == match_config::CharacterClass::Rogue && combatant.stealthed {
            // Check if we have an enemy target
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // Try to use Ambush if we have enough energy and target is in melee range
            let ability = AbilityType::Ambush;
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();

                // Consume energy
                combatant.current_mana -= def.mana_cost;

                // Break stealth immediately
                combatant.stealthed = false;

                // Calculate damage (with stat scaling)
                let damage = combatant.calculate_ability_damage(&def, &mut game_rng);

                // Queue the Ambush attack to be applied after the loop
                instant_attacks.push((entity, target_entity, damage, combatant.team, combatant.class, ability));

                // Trigger global cooldown (1.5s standard WoW GCD)
                combatant.global_cooldown = 1.5;

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                    format!("Team {} {}", team, class.name())
                });
                combat_log.log_ability_cast(
                    caster_id,
                    "Ambush".to_string(),
                    target_id,
                    format!("Team {} {} uses Ambush from stealth", combatant.team, combatant.class.name()),
                );

                info!(
                    "Team {} {} uses {} from stealth!",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }
        
        // Rogues use Kick, Kidney Shot and Sinister Strike when out of stealth
        if combatant.class == match_config::CharacterClass::Rogue && !combatant.stealthed {
            // Check if we have an enemy target
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // NOTE: Interrupt checking (Kick) is now handled in the dedicated check_interrupts system
            // which runs after apply_deferred so it can see CastingState components from this frame
            
            // Check if global cooldown is active for other abilities
            if combatant.global_cooldown > 0.0 {
                continue; // Can't use other abilities during GCD
            }
            
            // Priority 1: Use Kidney Shot (stun) if available
            let kidney_shot = AbilityType::KidneyShot;
            let ks_on_cooldown = combatant.ability_cooldowns.contains_key(&kidney_shot);
            
            if !ks_on_cooldown && kidney_shot.can_cast(&combatant, target_pos, my_pos) {
                let def = kidney_shot.definition();

                // Spawn speech bubble
                spawn_speech_bubble(&mut commands, entity, "Kidney Shot");

                // Consume energy
                combatant.current_mana -= def.mana_cost;

                // Put on cooldown
                combatant.ability_cooldowns.insert(kidney_shot, def.cooldown);

                // Trigger global cooldown
                combatant.global_cooldown = 1.5;

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                    format!("Team {} {}", team, class.name())
                });
                combat_log.log_ability_cast(
                    caster_id,
                    "Kidney Shot".to_string(),
                    target_id,
                    format!("Team {} {} uses Kidney Shot", combatant.team, combatant.class.name()),
                );

                // Spawn pending aura (stun effect)
                if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                    commands.spawn(AuraPending {
                        target: target_entity,
                        aura: Aura {
                            effect_type: aura_type,
                            duration,
                            magnitude,
                            break_on_damage_threshold: break_threshold,
                            accumulated_damage: 0.0,
                            tick_interval: 0.0,
                            time_until_next_tick: 0.0,
                            caster: Some(entity),
                            ability_name: def.name.to_string(),
                            fear_direction: (0.0, 0.0),
                            fear_direction_timer: 0.0,
                        },
                    });
                }
                
                info!(
                    "Team {} {} uses {} on enemy!",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );

                // Log to combat log with structured CC data
                if let Some((target_team, target_class, _, _)) = combatant_info.get(&target_entity) {
                    if let Some((aura_type, duration, _, _)) = def.applies_aura {
                        let cc_type = format!("{:?}", aura_type);
                        let message = format!(
                            "Team {} {} uses {} on Team {} {}",
                            combatant.team,
                            combatant.class.name(),
                            def.name,
                            target_team,
                            target_class.name()
                        );
                        combat_log.log_crowd_control(
                            combatant_id(combatant.team, combatant.class),
                            combatant_id(*target_team, *target_class),
                            cc_type,
                            duration,
                            message,
                        );
                    }
                }

                continue; // Done this frame
            }

            // Priority 2: Use Sinister Strike if we have enough energy and target is in melee range
            let ability = AbilityType::SinisterStrike;
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();

                // Consume energy
                combatant.current_mana -= def.mana_cost;

                // Calculate damage (with stat scaling)
                let damage = combatant.calculate_ability_damage(&def, &mut game_rng);

                // Queue the Sinister Strike attack to be applied after the loop
                instant_attacks.push((entity, target_entity, damage, combatant.team, combatant.class, ability));

                // Trigger global cooldown (1.5s standard WoW GCD)
                combatant.global_cooldown = 1.5;

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                    format!("Team {} {}", team, class.name())
                });
                combat_log.log_ability_cast(
                    caster_id,
                    "Sinister Strike".to_string(),
                    target_id,
                    format!("Team {} {} uses Sinister Strike", combatant.team, combatant.class.name()),
                );

                info!(
                    "Team {} {} uses {}!",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }

        // Warlocks use Corruption (instant DoT) and Shadowbolt (cast time projectile)
        if combatant.class == match_config::CharacterClass::Warlock {
            // Check if we have an enemy target
            let Some(target_entity) = combatant.target else {
                continue;
            };

            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };

            // Check if global cooldown is active
            if combatant.global_cooldown > 0.0 {
                continue; // Can't use abilities during GCD
            }

            // Priority 1: Apply Corruption if target doesn't have it (instant DoT)
            let target_has_corruption = if let Some(auras) = active_auras_map.get(&target_entity) {
                // Check for any DoT - in the future we could track specific DoT sources
                auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime)
            } else {
                false
            };

            if !target_has_corruption {
                let corruption = AbilityType::Corruption;
                let corruption_def = corruption.definition();

                // Check if Shadow school is locked out
                if !is_spell_school_locked(corruption_def.spell_school, auras.as_deref()) {
                    if corruption.can_cast(&combatant, target_pos, my_pos) {
                        // Consume mana
                        combatant.current_mana -= corruption_def.mana_cost;

                        // Trigger global cooldown
                        combatant.global_cooldown = 1.5;

                        // Log ability cast for timeline
                        let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                        let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                            format!("Team {} {}", team, class.name())
                        });
                        combat_log.log_ability_cast(
                            caster_id,
                            "Corruption".to_string(),
                            target_id,
                            format!("Team {} {} casts Corruption", combatant.team, combatant.class.name()),
                        );

                        // Apply the DoT aura
                        if let Some((aura_type, duration, magnitude, break_threshold)) = corruption_def.applies_aura {
                            commands.spawn(AuraPending {
                                target: target_entity,
                                aura: Aura {
                                    effect_type: aura_type,
                                    duration,
                                    magnitude,
                                    break_on_damage_threshold: break_threshold,
                                    accumulated_damage: 0.0,
                                    tick_interval: 3.0, // Tick every 3 seconds
                                    time_until_next_tick: 3.0, // First tick after 3 seconds
                                    caster: Some(entity),
                                    ability_name: corruption_def.name.to_string(),
                                    fear_direction: (0.0, 0.0),
                                    fear_direction_timer: 0.0,
                                },
                            });
                        }

                        // Log Corruption application
                        combat_log.log(
                            CombatLogEventType::Buff,
                            format!(
                                "Team {} {} applies Corruption to enemy (10 damage per 3s for 18s)",
                                combatant.team,
                                combatant.class.name()
                            )
                        );

                        info!(
                            "Team {} {} applies Corruption to enemy (10 damage per 3s for 18s)",
                            combatant.team,
                            combatant.class.name()
                        );

                        continue; // Done this frame
                    }
                }
            }

            // Priority 2: Cast Fear if target is not CC'd and Fear is off cooldown
            let fear = AbilityType::Fear;
            let fear_def = fear.definition();
            let fear_cooldown = combatant.ability_cooldowns.get(&fear).copied().unwrap_or(0.0);

            // Check if target is already CC'd (don't waste Fear on CC'd targets)
            let target_is_ccd = if let Some(auras) = active_auras_map.get(&target_entity) {
                auras.iter().any(|a| matches!(a.effect_type, AuraType::Stun | AuraType::Fear | AuraType::Root))
            } else {
                false
            };

            if fear_cooldown <= 0.0 && !target_is_ccd {
                // Check if Shadow school is locked out
                if !is_spell_school_locked(fear_def.spell_school, auras.as_deref()) {
                    if fear.can_cast(&combatant, target_pos, my_pos) {
                        // Trigger global cooldown (starts when cast begins)
                        combatant.global_cooldown = 1.5;

                        // Start casting Fear
                        commands.entity(entity).insert(CastingState {
                            ability: fear,
                            time_remaining: fear_def.cast_time,
                            target: Some(target_entity),
                            interrupted: false,
                            interrupted_display_time: 0.0,
                        });

                        // Log ability cast for timeline
                        let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                        let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                            format!("Team {} {}", team, class.name())
                        });
                        combat_log.log_ability_cast(
                            caster_id,
                            "Fear".to_string(),
                            target_id,
                            format!("Team {} {} begins casting Fear", combatant.team, combatant.class.name()),
                        );

                        info!(
                            "Team {} {} starts casting Fear on enemy",
                            combatant.team,
                            combatant.class.name()
                        );

                        continue; // Done this frame
                    }
                }
            }

            // Priority 3: Cast Shadowbolt (main damage spell with cast time)
            let shadowbolt = AbilityType::Shadowbolt;
            let shadowbolt_def = shadowbolt.definition();

            // Check if Shadow school is locked out
            if is_spell_school_locked(shadowbolt_def.spell_school, auras.as_deref()) {
                continue; // Can't cast - spell school is locked
            }

            if shadowbolt.can_cast(&combatant, target_pos, my_pos) {
                // Trigger global cooldown (starts when cast begins)
                combatant.global_cooldown = 1.5;

                // Start casting
                commands.entity(entity).insert(CastingState {
                    ability: shadowbolt,
                    time_remaining: shadowbolt_def.cast_time,
                    target: Some(target_entity),
                    interrupted: false,
                    interrupted_display_time: 0.0,
                });

                // Log ability cast for timeline
                let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
                let target_id = combatant_info.get(&target_entity).map(|(team, class, _, _)| {
                    format!("Team {} {}", team, class.name())
                });
                combat_log.log_ability_cast(
                    caster_id,
                    "Shadowbolt".to_string(),
                    target_id,
                    format!("Team {} {} begins casting Shadowbolt", combatant.team, combatant.class.name()),
                );

                info!(
                    "Team {} {} starts casting {} on enemy",
                    combatant.team,
                    combatant.class.name(),
                    shadowbolt_def.name
                );
            }
        }
    }

    // Process queued instant attacks (Ambush, Sinister Strike)
    for (attacker_entity, target_entity, damage, attacker_team, attacker_class, ability) in instant_attacks {
        let ability_name = ability.definition().name;
        let mut actual_damage = 0.0;
        let mut absorbed = 0.0;
        let mut target_team = 0;
        let mut target_class = match_config::CharacterClass::Warrior; // Default, will be overwritten

        if let Ok((_, mut target, target_transform, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let (dmg, abs) = super::combat_core::apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );
                actual_damage = dmg;
                absorbed = abs;
                target_team = target.team;
                target_class = target.class;

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });

                info!(
                    "Team {} {}'s {} hits Team {} {} for {:.0} damage!",
                    attacker_team,
                    attacker_class.name(),
                    ability_name,
                    target_team,
                    target_class.name(),
                    actual_damage
                );

                // Spawn floating combat text (yellow for abilities)
                let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
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
                        },
                        PlayMatchEntity,
                    ));
                }

                // Log the instant attack with structured data
                let is_killing_blow = !target.is_alive();
                let message = if absorbed > 0.0 {
                    format!(
                        "Team {} {}'s {} hits Team {} {} for {:.0} damage ({:.0} absorbed)",
                        attacker_team,
                        attacker_class.name(),
                        ability_name,
                        target_team,
                        target_class.name(),
                        actual_damage,
                        absorbed
                    )
                } else {
                    format!(
                        "Team {} {}'s {} hits Team {} {} for {:.0} damage",
                        attacker_team,
                        attacker_class.name(),
                        ability_name,
                        target_team,
                        target_class.name(),
                        actual_damage
                    )
                };
                combat_log.log_damage(
                    combatant_id(attacker_team, attacker_class),
                    combatant_id(target_team, target_class),
                    ability_name.to_string(),
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
        
        // Update attacker's damage dealt
        if let Ok((_, mut attacker, _, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += actual_damage;
        }
    }
    
    // Process queued Frost Nova damage
    for (caster_entity, target_entity, damage, caster_team, caster_class, _target_pos) in frost_nova_damage {
        let mut actual_damage = 0.0;
        let mut absorbed = 0.0;
        let mut target_team = 0;
        let mut target_class = match_config::CharacterClass::Warrior;

        if let Ok((_, mut target, target_transform, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let (dmg, abs) = super::combat_core::apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );
                actual_damage = dmg;
                absorbed = abs;
                target_team = target.team;
                target_class = target.class;

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });

                // Spawn floating combat text (yellow for abilities)
                let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
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
                        },
                        PlayMatchEntity,
                    ));
                }

                // Log the Frost Nova damage with structured data
                let is_killing_blow = !target.is_alive();
                let message = if absorbed > 0.0 {
                    format!(
                        "Team {} {}'s Frost Nova hits Team {} {} for {:.0} damage ({:.0} absorbed)",
                        caster_team,
                        caster_class.name(),
                        target_team,
                        target_class.name(),
                        actual_damage,
                        absorbed
                    )
                } else {
                    format!(
                        "Team {} {}'s Frost Nova hits Team {} {} for {:.0} damage",
                        caster_team,
                        caster_class.name(),
                        target_team,
                        target_class.name(),
                        actual_damage
                    )
                };
                combat_log.log_damage(
                    combatant_id(caster_team, caster_class),
                    combatant_id(target_team, target_class),
                    "Frost Nova".to_string(),
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
        }
        
        // Update caster's damage dealt
        if let Ok((_, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += actual_damage;
        }
    }
}
pub fn check_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform), Without<CastingState>>,
    casting_targets: Query<&CastingState>,
    positions: Query<&Transform>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't interrupt during victory celebration
    if celebration.is_some() {
        return;
    }
    
    for (entity, mut combatant, transform) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Only Warriors and Rogues have interrupts
        if combatant.class != match_config::CharacterClass::Warrior 
            && combatant.class != match_config::CharacterClass::Rogue {
            continue;
        }
        
        let Some(target_entity) = combatant.target else {
            continue;
        };
        
        let Ok(target_transform) = positions.get(target_entity) else {
            continue;
        };
        
        let my_pos = transform.translation;
        let target_pos = target_transform.translation;
        let distance = my_pos.distance(target_pos);
        
        // Check if target is casting
        let Ok(cast_state) = casting_targets.get(target_entity) else {
            continue; // Target not casting
        };
        
        if cast_state.interrupted {
            continue; // Already interrupted
        }
        
        // Determine which interrupt ability to use based on class
        let interrupt_ability = match combatant.class {
            match_config::CharacterClass::Warrior => AbilityType::Pummel,
            match_config::CharacterClass::Rogue => {
                // Rogues cannot use Kick while stealthed - must break stealth first
                if combatant.stealthed {
                    continue;
                }
                AbilityType::Kick
            },
            _ => continue,
        };
        
        let ability_def = interrupt_ability.definition();
        
        // Check if interrupt is on cooldown
        if combatant.ability_cooldowns.contains_key(&interrupt_ability) {
            continue;
        }
        
        // Check if we can cast the interrupt (range, resources, etc.)
        if !interrupt_ability.can_cast(&combatant, target_pos, my_pos) {
            continue;
        }
        
        // Use the interrupt!
        info!(
            "[INTERRUPT] Team {} {} uses {} to interrupt {}'s cast (distance: {:.1}, time_remaining: {:.2}s)",
            combatant.team,
            combatant.class.name(),
            ability_def.name,
            cast_state.ability.definition().name,
            distance,
            cast_state.time_remaining
        );
        
        // Spawn speech bubble for interrupt
        spawn_speech_bubble(&mut commands, entity, ability_def.name);

        // Consume resources
        combatant.current_mana -= ability_def.mana_cost;

        // Put on cooldown
        combatant.ability_cooldowns.insert(interrupt_ability, ability_def.cooldown);

        // Interrupts do NOT trigger GCD in WoW!

        // Log ability cast for timeline
        let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
        combat_log.log_ability_cast(
            caster_id,
            ability_def.name.to_string(),
            None, // Interrupts don't have a "target" in the same way
            format!("Team {} {} uses {}", combatant.team, combatant.class.name(), ability_def.name),
        );

        // Queue the interrupt for processing
        commands.spawn(InterruptPending {
            caster: entity,
            target: target_entity,
            ability: interrupt_ability,
            lockout_duration: ability_def.lockout_duration,
        });

        // Log to combat log
        combat_log.log(
            CombatLogEventType::AbilityUsed,
            format!(
                "Team {} {} uses {} to interrupt enemy cast",
                combatant.team,
                combatant.class.name(),
                ability_def.name
            )
        );
    }
}
