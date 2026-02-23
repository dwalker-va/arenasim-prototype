//! Combat AI Systems
//!
//! Handles all AI decision-making for combatants:
//! - Target acquisition (choosing which enemy to fight)
//! - Ability decisions (class-specific AI for using abilities)
//! - Interrupt decisions (when to interrupt enemy casts)

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::CombatLog;
use super::match_config;
use super::components::*;
use super::abilities::AbilityType;
use super::ability_config::AbilityDefinitions;
use super::utils::{combatant_id, get_next_fct_offset};
use super::class_ai;

// Re-export spawn_speech_bubble for backward compatibility (used by other modules)
pub use super::utils::spawn_speech_bubble;

pub fn acquire_targets(
    countdown: Res<MatchCountdown>,
    config: Res<match_config::MatchConfig>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform, Option<&ActiveAuras>)>,
    pet_query: Query<&Pet>,
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
    // Tuple: (entity, position, stealthed, has_shadow_sight, class, current_health, is_immune, is_pet)
    let mut team1_combatants: Vec<(Entity, Vec3, bool, bool, match_config::CharacterClass, f32, bool, bool)> = Vec::new();
    let mut team2_combatants: Vec<(Entity, Vec3, bool, bool, match_config::CharacterClass, f32, bool, bool)> = Vec::new();

    // Collect active auras for CC checking
    let active_auras_map: std::collections::HashMap<Entity, Vec<Aura>> = combatants
        .iter()
        .filter_map(|(entity, _, _, auras_opt)| {
            auras_opt.map(|auras| (entity, auras.auras.clone()))
        })
        .collect();

    for (entity, c, transform, _) in combatants.iter() {
        if !c.is_alive() {
            continue;
        }

        let has_shadow_sight = shadow_sight_holders.contains(&entity);
        let is_immune = active_auras_map
            .get(&entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity))
            .unwrap_or(false);
        let is_pet = pet_query.get(entity).is_ok();

        if c.team == 1 {
            team1_combatants.push((entity, transform.translation, c.stealthed, has_shadow_sight, c.class, c.current_health, is_immune, is_pet));
        } else {
            team2_combatants.push((entity, transform.translation, c.stealthed, has_shadow_sight, c.class, c.current_health, is_immune, is_pet));
        }
    }

    // Sort by entity ID to ensure deterministic ordering matching spawn order
    // Entity IDs are assigned sequentially at spawn time
    team1_combatants.sort_by_key(|(entity, _, _, _, _, _, _, _)| entity.index());
    team2_combatants.sort_by_key(|(entity, _, _, _, _, _, _, _)| entity.index());

    // For each combatant, ensure they have a valid target
    for (entity, mut combatant, transform, _) in combatants.iter_mut() {
        if !combatant.is_alive() {
            combatant.target = None;
            combatant.cc_target = None;
            continue;
        }

        // Check if this combatant has Shadow Sight
        let i_have_shadow_sight = shadow_sight_holders.contains(&entity);

        // Get enemy team combatants and target priorities
        let (enemy_combatants, kill_target_index, cc_target_index) = if combatant.team == 1 {
            (&team2_combatants, config.team1_kill_target, config.team1_cc_target)
        } else {
            (&team1_combatants, config.team2_kill_target, config.team2_cc_target)
        };

        // Visibility check: can see enemy if:
        // 1. Enemy is not stealthed, OR
        // 2. I have Shadow Sight buff, OR
        // 3. Enemy has Shadow Sight buff (they're revealed by picking it up)
        let can_see = |stealthed: bool, enemy_has_shadow_sight: bool| -> bool {
            !stealthed || i_have_shadow_sight || enemy_has_shadow_sight
        };

        // Check if current target is still valid (alive, on enemy team, visible, and not immune)
        let target_valid = combatant.target.and_then(|target_entity| {
            enemy_combatants
                .iter()
                .find(|(e, _, _, _, _, _, _, _)| *e == target_entity)
                .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| can_see(*stealthed, *enemy_ss) && !immune)
        }).is_some();

        // If no valid target, acquire a new one
        if !target_valid {
            // Priority 1: Check if kill target is set, visible, and not immune
            let kill_target = if let Some(index) = kill_target_index {
                enemy_combatants
                    .get(index)
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| can_see(*stealthed, *enemy_ss) && !immune)
                    .map(|(entity, _, _, _, _, _, _, _)| *entity)
            } else {
                None
            };

            if let Some(priority_target) = kill_target {
                // Use the kill target
                combatant.target = Some(priority_target);
            } else {
                // Priority 2: Fall back to nearest visible, non-immune enemy
                // Prefer non-pet targets; only target pets if no primary enemies are available
                let my_pos = transform.translation;
                let visible_enemies: Vec<_> = enemy_combatants
                    .iter()
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| can_see(*stealthed, *enemy_ss) && !immune)
                    .collect();

                // Try non-pet enemies first, fall back to all visible if none
                let nearest_primary = visible_enemies.iter()
                    .filter(|(_, _, _, _, _, _, _, is_pet)| !is_pet)
                    .min_by(|(_, pos_a, _, _, _, _, _, _), (_, pos_b, _, _, _, _, _, _)| {
                        my_pos.distance(*pos_a).partial_cmp(&my_pos.distance(*pos_b)).unwrap()
                    });

                let nearest_enemy = nearest_primary.or_else(|| {
                    visible_enemies.iter()
                        .min_by(|(_, pos_a, _, _, _, _, _, _), (_, pos_b, _, _, _, _, _, _)| {
                            my_pos.distance(*pos_a).partial_cmp(&my_pos.distance(*pos_b)).unwrap()
                        })
                });

                combatant.target = nearest_enemy.map(|(entity, _, _, _, _, _, _, _)| *entity);
            }
        } else if let Some(index) = kill_target_index {
            // Current target is valid, but check if configured kill target has become
            // available (e.g., Rogue broke stealth) and should take priority
            if let Some((kt_entity, _, stealthed, enemy_ss, _, _, immune, _)) = enemy_combatants.get(index) {
                if can_see(*stealthed, *enemy_ss) && !immune && combatant.target != Some(*kt_entity) {
                    combatant.target = Some(*kt_entity);
                }
            }
        }

        // ===== CC Target Acquisition =====
        // Separate from kill target - use for CC abilities to create outnumbering situations

        // Check if current CC target is still valid (not immune)
        let cc_target_valid = combatant.cc_target.and_then(|cc_target_entity| {
            enemy_combatants
                .iter()
                .find(|(e, _, _, _, _, _, _, _)| *e == cc_target_entity)
                .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| can_see(*stealthed, *enemy_ss) && !immune)
        }).is_some();

        if !cc_target_valid {
            // Priority 1: Use explicitly configured CC target (if visible and not immune)
            let explicit_cc_target = if let Some(index) = cc_target_index {
                enemy_combatants
                    .get(index)
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| can_see(*stealthed, *enemy_ss) && !immune)
                    .map(|(entity, _, _, _, _, _, _, _)| *entity)
            } else {
                None
            };

            if let Some(cc_target) = explicit_cc_target {
                combatant.cc_target = Some(cc_target);
            } else {
                // Priority 2: Use heuristic selection
                // Score: Healer +100, Non-kill-target +50, Higher HP +20
                // Skip already-CC'd targets
                combatant.cc_target = select_cc_target_heuristic(
                    enemy_combatants,
                    combatant.target,
                    &active_auras_map,
                    &can_see,
                );
            }
        } else if let Some(index) = cc_target_index {
            // Current CC target is valid, but check if configured CC target has become
            // available (e.g., broke stealth) and should take priority
            if let Some((cc_entity, _, stealthed, enemy_ss, _, _, immune, _)) = enemy_combatants.get(index) {
                if can_see(*stealthed, *enemy_ss) && !immune && combatant.cc_target != Some(*cc_entity) {
                    combatant.cc_target = Some(*cc_entity);
                }
            }
        }
    }
}

/// Select the best CC target using heuristics.
/// Priority scoring:
/// 1. Healer: +100 points - highest CC value (UNLESS we're killing the healer)
/// 2. Non-kill-target: +50 points - enables outnumbering
/// 3. Higher HP: +20 points - don't waste CC on dying targets
/// Required: Not already CC'd
///
/// Special case: If kill_target is a healer, we INVERT healer priority.
/// When killing the healer, we want to CC the DPS to prevent them from peeling.
fn select_cc_target_heuristic(
    enemy_combatants: &[(Entity, Vec3, bool, bool, match_config::CharacterClass, f32, bool, bool)],
    kill_target: Option<Entity>,
    active_auras_map: &std::collections::HashMap<Entity, Vec<Aura>>,
    can_see: &impl Fn(bool, bool) -> bool,
) -> Option<Entity> {
    // Check if kill target is a healer - if so, we invert healer CC priority
    let killing_healer = kill_target
        .and_then(|kt| {
            enemy_combatants
                .iter()
                .find(|(e, _, _, _, _, _, _, _)| *e == kt)
                .map(|(_, _, _, _, class, _, _, _)| class.is_healer())
        })
        .unwrap_or(false);

    // Filter to visible, non-CC'd, non-immune enemies and score them
    // Pets are excluded — CC on a pet is wasted
    let mut scored_targets: Vec<(Entity, i32)> = enemy_combatants
        .iter()
        .filter(|(_, _, stealthed, enemy_ss, _, _, immune, is_pet)| can_see(*stealthed, *enemy_ss) && !immune && !is_pet)
        .filter(|(entity, _, _, _, _, _, _, _)| !is_entity_ccd(*entity, active_auras_map))
        .map(|(entity, _, _, _, class, current_health, _, _)| {
            let mut score = 0i32;
            let is_healer = class.is_healer();

            // Healer/DPS priority depends on who we're killing
            if killing_healer {
                // Killing healer -> CC the DPS to prevent peel
                if !is_healer {
                    score += 100;
                }
            } else {
                // Killing DPS -> CC the healer to prevent healing
                if is_healer {
                    score += 100;
                }
            }

            // Non-kill-target bonus (enables 2v1 situations)
            if kill_target != Some(*entity) {
                score += 50;
            }

            // Higher HP bonus (don't waste CC on dying targets)
            // Scale 0-20 based on health percentage (approximating with raw health)
            // Most classes have 150-200 HP, so 200 HP = 20 points
            score += (*current_health as i32 / 10).min(20);

            (*entity, score)
        })
        .collect();

    // Sort by score descending, then by entity index for determinism
    scored_targets.sort_by(|(e1, s1), (e2, s2)| {
        s2.cmp(s1).then_with(|| e1.index().cmp(&e2.index()))
    });

    scored_targets.first().map(|(entity, _)| *entity)
}

/// Check if an entity is currently CC'd (Stun, Fear, Root, or Polymorph).
fn is_entity_ccd(entity: Entity, active_auras_map: &std::collections::HashMap<Entity, Vec<Aura>>) -> bool {
    active_auras_map
        .get(&entity)
        .map(|auras| {
            auras.iter().any(|a| {
                matches!(
                    a.effect_type,
                    AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph
                )
            })
        })
        .unwrap_or(false)
}
pub fn decide_abilities(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    abilities: Res<AbilityDefinitions>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform, Option<&mut ActiveAuras>), (Without<CastingState>, Without<ChannelingState>)>,
    casting_auras: Query<(Entity, &ActiveAuras), With<CastingState>>,
    channeling_auras: Query<(Entity, &ActiveAuras), (With<ChannelingState>, Without<CastingState>)>,
    dr_tracker_query: Query<(Entity, &DRTracker)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
    pet_query: Query<&Pet>,
) {
    // Don't cast abilities during victory celebration
    if celebration.is_some() {
        return;
    }

    // CombatantInfo is a per-frame snapshot. Mutations to Combatant components
    // during class AI dispatch are not reflected in other entities' views.
    // Safe because each entity is dispatched at most once per frame.
    let combatant_info: std::collections::HashMap<Entity, class_ai::CombatantInfo> = combatants
        .iter()
        .map(|(entity, combatant, transform, _)| {
            let pet_comp = pet_query.get(entity).ok();
            (entity, class_ai::CombatantInfo {
                entity,
                team: combatant.team,
                slot: combatant.slot,
                class: combatant.class,
                current_health: combatant.current_health,
                max_health: combatant.max_health,
                current_mana: combatant.current_mana,
                max_mana: combatant.max_mana,
                position: transform.translation,
                is_alive: combatant.is_alive(),
                stealthed: combatant.stealthed,
                target: combatant.target,
                is_pet: pet_comp.is_some(),
                pet_type: pet_comp.map(|p| p.pet_type),
            })
        })
        .collect();

    // Map of entities to their active auras (for checking buffs/debuffs)
    // We need auras from:
    // 1. Non-casting/non-channeling entities (from main query)
    // 2. Casting entities (separate query to avoid conflicts)
    // 3. Channeling entities (separate query to avoid conflicts)
    let mut active_auras_map: std::collections::HashMap<Entity, Vec<Aura>> = combatants
        .iter()
        .filter_map(|(entity, _, _, auras_opt)| {
            auras_opt.map(|auras| (entity, auras.auras.clone()))
        })
        .collect();

    // Add auras from casting entities
    for (entity, auras) in casting_auras.iter() {
        active_auras_map.insert(entity, auras.auras.clone());
    }

    // Add auras from channeling entities
    for (entity, auras) in channeling_auras.iter() {
        active_auras_map.insert(entity, auras.auras.clone());
    }

    // Build DR tracker map for AI immunity queries
    let dr_trackers: std::collections::HashMap<Entity, DRTracker> = dr_tracker_query
        .iter()
        .map(|(entity, tracker)| (entity, tracker.clone()))
        .collect();

    // Queue for instant ability attacks (Ambush, Sinister Strike, Mortal Strike)
    let mut instant_attacks: Vec<class_ai::QueuedInstantAttack> = Vec::new();

    // Track targets that have been shielded THIS FRAME to prevent same-frame double-shielding
    // This handles the case where multiple Priests try to shield the same target before AuraPending is processed
    let mut shielded_this_frame: std::collections::HashSet<Entity> = std::collections::HashSet::new();

    // Track targets that have been fortified THIS FRAME to prevent same-frame double-buffing
    // This handles the case where multiple Priests try to buff the same target before AuraPending is processed
    let mut fortified_this_frame: std::collections::HashSet<Entity> = std::collections::HashSet::new();

    // Track targets that have received Battle Shout THIS FRAME to prevent duplicate buffs
    // when multiple Warriors cast Battle Shout before AuraPending is processed
    let mut battle_shouted_this_frame: std::collections::HashSet<Entity> = std::collections::HashSet::new();

    // Track targets that have received Devotion Aura THIS FRAME to prevent duplicate buffs
    // when multiple Paladins cast Devotion Aura before AuraPending is processed
    let mut devotion_aura_this_frame: std::collections::HashSet<Entity> = std::collections::HashSet::new();

    // Queue for Frost Nova damage
    let mut frost_nova_damage: Vec<class_ai::QueuedAoeDamage> = Vec::new();
    
    for (entity, mut combatant, transform, auras) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        // Pets use pet_ai_system for their decisions, skip class AI dispatch
        if pet_query.get(entity).is_ok() {
            continue;
        }

        // WoW Mechanic: Cannot use abilities while stunned, feared, or polymorphed
        let is_incapacitated = super::utils::is_incapacitated(auras.as_deref());

        // Paladin-specific: Divine Shield can be used while incapacitated
        if is_incapacitated && combatant.class == match_config::CharacterClass::Paladin {
            let cc_ctx = class_ai::CombatContext {
                combatants: &combatant_info,
                active_auras: &active_auras_map,
                dr_trackers: &dr_trackers,
                self_entity: entity,
            };
            if class_ai::paladin::try_divine_shield_while_cc(
                &mut commands,
                &mut combat_log,
                &abilities,
                entity,
                &mut combatant,
                auras.as_deref(),
                &cc_ctx,
            ) {
                continue; // DivineShieldPending spawned — CC will be purged next frame
            }
            continue; // Still incapacitated, can't do anything else
        }

        if is_incapacitated {
            continue;
        }
        
        let my_pos = transform.translation;

        let ctx = class_ai::CombatContext {
            combatants: &combatant_info,
            active_auras: &active_auras_map,
            dr_trackers: &dr_trackers,
            self_entity: entity,
        };

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
                &ctx,
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
                &ctx,
                &mut shielded_this_frame,
                &mut fortified_this_frame,
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
                &abilities,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &ctx,
                &mut instant_attacks,
                &mut battle_shouted_this_frame,
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
                &abilities,
                entity,
                &mut combatant,
                my_pos,
                &ctx,
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
                &abilities,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &ctx,
            ) {
                continue;
            }
        }

        // Paladins use Flash of Light, Holy Light, Holy Shock, Hammer of Justice, and Cleanse
        if combatant.class == match_config::CharacterClass::Paladin {
            if class_ai::paladin::decide_paladin_action(
                &mut commands,
                &mut combat_log,
                &abilities,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &ctx,
                &mut devotion_aura_this_frame,
            ) {
                continue;
            }
        }

        // Hunters use Aimed Shot, Arcane Shot, Concussive Shot, Disengage, and Traps
        if combatant.class == match_config::CharacterClass::Hunter {
            if class_ai::hunter::decide_hunter_action(
                &mut commands,
                &mut combat_log,
                &mut game_rng,
                &abilities,
                entity,
                &mut combatant,
                my_pos,
                auras.as_deref(),
                &ctx,
                &mut instant_attacks,
            ) {
                continue;
            }
        }
    }

    // Process queued instant attacks (Ambush, Sinister Strike, Mortal Strike)
    for atk in instant_attacks {
        let class_ai::QueuedInstantAttack {
            attacker: attacker_entity,
            target: target_entity,
            damage,
            attacker_team,
            attacker_class,
            ability,
            is_crit,
        } = atk;
        let ability_name = abilities.get_unchecked(&ability).name.clone();
        let mut actual_damage = 0.0;

        // Apply Divine Shield outgoing damage penalty (50%) if attacker has DamageImmunity
        let ds_penalty = if let Some(attacker_auras) = active_auras_map.get(&attacker_entity) {
            if attacker_auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity) {
                super::constants::DIVINE_SHIELD_DAMAGE_PENALTY
            } else {
                1.0
            }
        } else {
            1.0
        };
        let damage = (damage * ds_penalty).max(0.0);

        if let Ok((_, mut target, target_transform, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let (dmg, absorbed) = super::combat_core::apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );
                actual_damage = dmg;
                let target_team = target.team;
                let target_class = target.class;

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
                        is_crit,
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

                // Log the instant attack with structured data
                let is_killing_blow = !target.is_alive();
                let is_first_death = is_killing_blow && !target.is_dead;
                if is_first_death {
                    target.is_dead = true;
                }
                let verb = if is_crit { "CRITS" } else { "hits" };
                let message = if absorbed > 0.0 {
                    format!(
                        "Team {} {}'s {} {} Team {} {} for {:.0} damage ({:.0} absorbed)",
                        attacker_team,
                        attacker_class.name(),
                        ability_name,
                        verb,
                        target_team,
                        target_class.name(),
                        actual_damage,
                        absorbed
                    )
                } else {
                    format!(
                        "Team {} {}'s {} {} Team {} {} for {:.0} damage",
                        attacker_team,
                        attacker_class.name(),
                        ability_name,
                        verb,
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
                    is_crit,
                    message,
                );

                // Log death with killer tracking (only on first death to prevent duplicates)
                if is_first_death {
                    // Cancel any in-progress cast or channel so dead combatants can't finish spells
                    commands.entity(target_entity).remove::<CastingState>();
                    commands.entity(target_entity).remove::<ChannelingState>();

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
    for aoe in frost_nova_damage {
        let class_ai::QueuedAoeDamage {
            caster: caster_entity,
            target: target_entity,
            damage,
            caster_team,
            caster_class,
            target_pos: _target_pos,
            is_crit,
        } = aoe;
        let mut actual_damage = 0.0;

        if let Ok((_, mut target, target_transform, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let (dmg, absorbed) = super::combat_core::apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );
                actual_damage = dmg;
                let target_team = target.team;
                let target_class = target.class;

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
                        is_crit,
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

                // Log the Frost Nova damage with structured data
                let is_killing_blow = !target.is_alive();
                let is_first_death = is_killing_blow && !target.is_dead;
                if is_first_death {
                    target.is_dead = true;
                }
                let verb = if is_crit { "CRITS" } else { "hits" };
                let message = if absorbed > 0.0 {
                    format!(
                        "Team {} {}'s Frost Nova {} Team {} {} for {:.0} damage ({:.0} absorbed)",
                        caster_team,
                        caster_class.name(),
                        verb,
                        target_team,
                        target_class.name(),
                        actual_damage,
                        absorbed
                    )
                } else {
                    format!(
                        "Team {} {}'s Frost Nova {} Team {} {} for {:.0} damage",
                        caster_team,
                        caster_class.name(),
                        verb,
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
                    is_crit,
                    message,
                );

                // Log death with killer tracking (only on first death to prevent duplicates)
                if is_first_death {
                    // Cancel any in-progress cast or channel so dead combatants can't finish spells
                    commands.entity(target_entity).remove::<CastingState>();
                    commands.entity(target_entity).remove::<ChannelingState>();

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
    abilities: Res<AbilityDefinitions>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform), Without<CastingState>>,
    casting_targets: Query<&CastingState>,
    channeling_targets: Query<&ChannelingState>,
    positions: Query<&Transform>,
    all_auras: Query<&ActiveAuras>,
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

        // Don't waste interrupts on immune targets (Divine Shield)
        if let Ok(target_auras) = all_auras.get(target_entity) {
            if target_auras.auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity) {
                continue;
            }
        }

        let Ok(target_transform) = positions.get(target_entity) else {
            continue;
        };

        let my_pos = transform.translation;
        let target_pos = target_transform.translation;
        let distance = my_pos.distance(target_pos);

        // Check if target is casting or channeling
        let target_ability_name: String;
        let is_interruptable: bool;

        if let Ok(cast_state) = casting_targets.get(target_entity) {
            if cast_state.interrupted {
                continue; // Already interrupted
            }
            target_ability_name = abilities.get_unchecked(&cast_state.ability).name.clone();
            is_interruptable = true;
        } else if let Ok(channel_state) = channeling_targets.get(target_entity) {
            if channel_state.interrupted {
                continue; // Already interrupted
            }
            target_ability_name = abilities.get_unchecked(&channel_state.ability).name.clone();
            is_interruptable = true;
        } else {
            continue; // Target not casting or channeling
        }

        if !is_interruptable {
            continue;
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

        let ability_def = abilities.get_unchecked(&interrupt_ability);

        // Check if interrupt is on cooldown
        if combatant.ability_cooldowns.contains_key(&interrupt_ability) {
            continue;
        }

        // Check if we can cast the interrupt (range, resources, etc.)
        if !interrupt_ability.can_cast_config(&combatant, target_pos, my_pos, ability_def) {
            continue;
        }

        // Use the interrupt!
        info!(
            "[INTERRUPT] Team {} {} uses {} to interrupt {}'s {} (distance: {:.1})",
            combatant.team,
            combatant.class.name(),
            ability_def.name,
            target_ability_name,
            distance,
            distance
        );

        // Spawn speech bubble for interrupt
        spawn_speech_bubble(&mut commands, entity, &ability_def.name);

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
        // Note: The actual interrupt result (with school lockout info) is logged in process_interrupts
        commands.spawn(InterruptPending {
            caster: entity,
            target: target_entity,
            ability: interrupt_ability,
            lockout_duration: ability_def.lockout_duration,
        });
    }
}
