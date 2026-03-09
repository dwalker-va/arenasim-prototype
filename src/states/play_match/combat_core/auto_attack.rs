//! Auto-attack system: melee swings, wand shots, auto shots, Heroic Strike, rage generation.

use std::collections::HashMap;
use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::CombatLog;
use super::super::match_config;
use super::super::components::*;
use super::super::constants::CRIT_DAMAGE_MULTIPLIER;
use super::super::utils::get_next_fct_offset;
use super::super::{MELEE_RANGE, WAND_RANGE, HUNTER_DEAD_ZONE, AUTO_SHOT_RANGE, FCT_HEIGHT};
use super::damage::{roll_crit, apply_damage_with_absorb, get_physical_damage_reduction, get_divine_shield_damage_penalty};

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
    // Tuple: (team, class, display_name, is_melee) — pets use PetType for display_name and is_melee
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass, String, bool)> = combatants
        .iter()
        .map(|(entity, _, combatant, _, _, _)| {
            let (display_name, is_melee) = if let Ok(pet) = auto_attack_pet_query.get(entity) {
                (pet.pet_type.name().to_string(), pet.pet_type.is_melee())
            } else {
                (combatant.class.name().to_string(), combatant.class.is_melee())
            };
            (entity, (combatant.team, combatant.class, display_name, is_melee))
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
        let is_incapacitated = super::super::utils::is_incapacitated(auras.as_deref());
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

                    // Use pet-aware is_melee from snapshot (pets inherit owner's class
                    // but may have different melee/ranged behavior)
                    let &(_, attacker_class, _, attacker_is_melee) = &combatant_info[&attacker_entity];
                    let attack_range = if attacker_is_melee {
                        MELEE_RANGE
                    } else if attacker_class == match_config::CharacterClass::Hunter {
                        AUTO_SHOT_RANGE
                    } else {
                        WAND_RANGE
                    };
                    let distance = my_pos.distance(target_pos);
                    // Hunter dead zone: can't auto-attack within 8 yards
                    if attacker_class == match_config::CharacterClass::Hunter && distance < HUNTER_DEAD_ZONE {
                        continue;
                    }
                    if distance <= attack_range {
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
                        if let Some(&(caster_team, _, _, _)) = combatant_info.get(&caster_entity) {
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
            if let Some(&(attacker_team, _, _, _)) = combatant_info.get(&attacker_entity) {
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
                if let (Some((attacker_team, attacker_class, attacker_name, attacker_is_melee)), Some((target_team, _target_class, target_name, _))) =
                    (combatant_info.get(&attacker_entity), combatant_info.get(&target_entity)) {
                    let attack_name = if has_bonus {
                        "Heroic Strike" // Enhanced auto-attack
                    } else if *attacker_is_melee {
                        "Auto Attack"
                    } else if *attacker_class == match_config::CharacterClass::Hunter {
                        "Auto Shot"
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
            let text_position = target_pos + Vec3::new(0.0, FCT_HEIGHT, 0.0);

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
