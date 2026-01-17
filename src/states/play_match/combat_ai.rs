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
use super::ability_config::AbilityDefinitions;
use super::utils::{combatant_id, get_next_fct_offset};
use super::constants::{GCD, CHARGE_MIN_RANGE};
use super::is_spell_school_locked;
use super::class_ai;

// Re-export spawn_speech_bubble for backward compatibility (used by other modules)
pub use super::utils::spawn_speech_bubble;

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
    abilities: Res<AbilityDefinitions>,
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
            if class_ai::mage::decide_mage_action(
                &mut commands,
                &mut combat_log,
                &mut game_rng,
                &abilities,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &positions,
                &combatant_info,
                &active_auras_map,
                &mut frost_nova_damage,
            ) {
                continue;
            }
        }
        // Priests cast Flash Heal on injured allies
        else if combatant.class == match_config::CharacterClass::Priest {
            if class_ai::priest::decide_priest_action(
                &mut commands,
                &mut combat_log,
                &abilities,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &positions,
                &combatant_info,
                &active_auras_map,
                &mut shielded_this_frame,
            ) {
                continue;
            }
        }

        // Warriors use Charge (gap closer), Mortal Strike, Rend, and Heroic Strike
        if combatant.class == match_config::CharacterClass::Warrior {
            if class_ai::warrior::decide_warrior_action(
                &mut commands,
                &mut combat_log,
                &mut game_rng,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &positions,
                &combatant_info,
                &active_auras_map,
                &mut instant_attacks,
            ) {
                continue;
            }
        }

        // Rogues use Ambush from stealth, Kick, Kidney Shot and Sinister Strike
        if combatant.class == match_config::CharacterClass::Rogue {
            if class_ai::rogue::decide_rogue_action(
                &mut commands,
                &mut combat_log,
                &mut game_rng,
                entity,
                &mut combatant,
                my_pos,
                &positions,
                &combatant_info,
                &mut instant_attacks,
            ) {
                continue;
            }
        }

        // Warlocks use Corruption (instant DoT), Fear, and Shadowbolt
        if combatant.class == match_config::CharacterClass::Warlock {
            if class_ai::warlock::decide_warlock_action(
                &mut commands,
                &mut combat_log,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &positions,
                &combatant_info,
                &active_auras_map,
            ) {
                continue;
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
