//! Auto-attack system: melee swings, wand shots, auto shots, Heroic Strike, rage generation.

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::CombatLog;
use super::super::match_config;
use super::super::components::*;
use super::super::abilities::SpellSchool;
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

    // Build a snapshot of combatant info for logging and alive checks
    // Tuple: (team, class, display_name, is_melee, is_alive)
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass, String, bool, bool)> = combatants
        .iter()
        .map(|(entity, _, combatant, _, _, _)| {
            let (display_name, is_melee) = if let Ok(pet) = auto_attack_pet_query.get(entity) {
                (pet.pet_type.name().to_string(), pet.pet_type.is_melee())
            } else {
                (combatant.class.name().to_string(), combatant.class.is_melee())
            };
            (entity, (combatant.team, combatant.class, display_name, is_melee, combatant.is_alive()))
        })
        .collect();

    // Auto-attacks must not shatter friendly crowd control. The AI ability path
    // already guards casts via `pre_cast_ok(check_friendly_cc)`, but
    // auto-attacks bypass that guard. Two tiers, each mapping a target entity to
    // the team of the caster who placed the CC (only one caster team recorded
    // per target — a target carrying the same CC class from two teams at once
    // does not occur):
    //  - `incap_cc_team`: break-on-ANY-damage incapacitates (Freezing Trap /
    //    Polymorph, threshold 0.0). NO attacker may break these — most visibly a
    //    Hunter's melee pet sitting on a trapped target.
    //  - `root_cc_team`: damage-breakable Roots (Spider Web, Frost Nova). A PET
    //    must not break these: it webs a target to peel it off the owner, and
    //    meleeing through the Web both defeats the peel and shatters it. A ranged
    //    player legitimately nukes a rooted target (root + nuke), so this tier is
    //    pet-only. Stuns/Fears are excluded — those are offensive setups the pet
    //    should keep attacking through.
    let caster_team = |a: &Aura| a.caster.and_then(|c| combatant_info.get(&c)).map(|info| info.0);
    let incap_cc_team: std::collections::HashMap<Entity, u8> = combatants
        .iter()
        .filter_map(|(entity, _, _, _, _, auras)| {
            let auras = auras?;
            auras
                .auras
                .iter()
                .find_map(|a| (a.break_on_damage_threshold == 0.0).then(|| caster_team(a)).flatten())
                .map(|team| (entity, team))
        })
        .collect();
    let root_cc_team: std::collections::HashMap<Entity, u8> = combatants
        .iter()
        .filter_map(|(entity, _, _, _, _, auras)| {
            let auras = auras?;
            auras
                .auras
                .iter()
                .find_map(|a| {
                    // `> 0.0` (not `>= 0.0`): a Root at threshold 0.0 is an
                    // any-damage break and belongs to the incap tier above, which
                    // blocks ALL attackers — keep the two tiers a clean partition.
                    (a.effect_type == AuraType::Root && a.break_on_damage_threshold > 0.0)
                        .then(|| caster_team(a))
                        .flatten()
                })
                .map(|team| (entity, team))
        })
        .collect();

    // Collect attacks that will happen this frame (attacker, target, damage)
    let mut attacks = Vec::new();

    // Track damage per target for batching floating combat text.
    // BTreeMap (not HashMap) so iteration order is deterministic by Entity —
    // FCT entity spawn order would otherwise vary across runs due to Rust's
    // randomized HashMap hasher, breaking byte-identical determinism for
    // self-mirror matchups (same class on both teams).
    let mut damage_per_target: std::collections::BTreeMap<Entity, f32> = std::collections::BTreeMap::new();
    // Track damage per target for aura breaking. Same BTreeMap rationale as
    // above — the iteration at the bottom of this function spawns commands
    // whose order can ripple into downstream entity allocation.
    let mut damage_per_aura_break: std::collections::BTreeMap<Entity, f32> = std::collections::BTreeMap::new();

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
        // Apply AttackSpeedSlow auras to increase the interval
        let mut attack_interval = 1.0 / combatant.attack_speed;
        if let Some(ref auras) = auras {
            for aura in auras.auras.iter() {
                if aura.effect_type == AuraType::AttackSpeedSlow {
                    // magnitude = slow amount (e.g., 0.25 = 25% slower → 1.33x interval)
                    // Clamp to 0.75 max to prevent division by near-zero or negative
                    let clamped = aura.magnitude.min(0.75);
                    attack_interval *= 1.0 / (1.0 - clamped);
                }
            }
        }
        if combatant.attack_timer >= attack_interval {
            if let Some(target_entity) = combatant.target {
                // Skip if target is dead (will be retargeted next frame)
                if !combatant_info.get(&target_entity).map_or(false, |info| info.4) {
                    continue;
                }
                // Don't shatter our own team's CC. The timer keeps building so
                // the attack resumes the instant the CC ends.
                //  - incapacitates (Freezing Trap / Polymorph): no attacker.
                //  - Roots (Spider Web): pets only — a ranged owner still nukes
                //    a rooted target.
                let attacker_is_pet = auto_attack_pet_query.get(attacker_entity).is_ok();
                if incap_cc_team.get(&target_entity) == Some(&combatant.team)
                    || (attacker_is_pet && root_cc_team.get(&target_entity) == Some(&combatant.team))
                {
                    continue;
                }
                // Check if target is in range before attacking
                if let Some(&target_pos) = positions.get(&target_entity) {
                    let my_pos = transform.translation;

                    // Use pet-aware is_melee from snapshot (pets inherit owner's class
                    // but may have different melee/ranged behavior)
                    let &(_, attacker_class, _, attacker_is_melee, _) = &combatant_info[&attacker_entity];
                    let attack_range = if attacker_is_melee {
                        MELEE_RANGE
                    } else if attacker_class == match_config::CharacterClass::Hunter {
                        AUTO_SHOT_RANGE
                    } else {
                        WAND_RANGE
                    };
                    let distance = my_pos.distance(target_pos);
                    // Hunter dead zone: the ranged Auto Shot can't fire within 8
                    // yards. This applies ONLY to the ranged Hunter — a melee pet
                    // (Spider/Boar) inherits the Hunter class but attacks in melee,
                    // so without the `!attacker_is_melee` guard it would skip every
                    // swing (it is always inside the dead zone while meleeing).
                    if attacker_class == match_config::CharacterClass::Hunter
                        && !attacker_is_melee
                        && distance < HUNTER_DEAD_ZONE
                    {
                        continue;
                    }
                    if distance <= attack_range {
                        // Calculate total damage (base + bonus from Heroic Strike, etc.)
                        let base_damage = combatant.attack_damage + combatant.next_attack_bonus_damage;
                        // Roll crit before damage reduction (include dynamic crit bonus from auras)
                        let crit_bonus = super::get_crit_chance_bonus(auras.as_deref());
                        let is_crit = roll_crit(combatant.crit_chance + crit_bonus, &mut game_rng);
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

    // Apply damage to targets and track damage dealt.
    // The maps/sets below all use BTreeMap/BTreeSet rather than HashMap/HashSet
    // so iteration order is deterministic by Entity. `frost_armor_procs` in
    // particular drives `commands.spawn(AuraPending)` calls below, where the
    // call order determines entity ID allocation and ripples into downstream
    // query iteration — a pre-existing source of self-mirror non-determinism
    // before this fix.
    let mut damage_dealt_updates: Vec<(Entity, f32)> = Vec::new();
    let mut absorbed_per_target: std::collections::BTreeMap<Entity, f32> = std::collections::BTreeMap::new();

    // Track which combatants have died during this frame's attack processing
    let mut died_this_frame: std::collections::BTreeSet<Entity> = std::collections::BTreeSet::new();

    // Track crit status per target for FCT display
    let mut crit_per_target: std::collections::BTreeMap<Entity, bool> = std::collections::BTreeMap::new();

    // Track Frost Armor procs: attacker entities to apply slows to after the loop.
    let mut frost_armor_procs: std::collections::BTreeSet<Entity> = std::collections::BTreeSet::new();

    // Build a map of targets with breakable CC from friendly casters.
    let mut friendly_cc_team: std::collections::BTreeMap<Entity, u8> = std::collections::BTreeMap::new();
    for (entity, _, combatant, _, _, auras) in combatants.iter() {
        if let Some(auras) = auras {
            for aura in &auras.auras {
                // Only care about CC auras that break on damage
                if aura.break_on_damage_threshold >= 0.0
                    && matches!(aura.effect_type, AuraType::Polymorph | AuraType::Fear)
                {
                    // Look up the caster's team
                    if let Some(caster_entity) = aura.caster {
                        if let Some(&(caster_team, _, _, _, _)) = combatant_info.get(&caster_entity) {
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
            if let Some(&(attacker_team, _, _, _, _)) = combatant_info.get(&attacker_entity) {
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
                    SpellSchool::Physical,
                );

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Check for Frost Armor proc: if target has FrostArmorBuff and attacker is melee
                if let Some(&(_, _, _, attacker_is_melee, _)) = combatant_info.get(&attacker_entity) {
                    if attacker_is_melee {
                        if let Some(ref target_auras_ref) = target_auras {
                            if target_auras_ref.auras.iter().any(|a| a.effect_type == AuraType::FrostArmorBuff) {
                                frost_armor_procs.insert(attacker_entity);
                            }
                        }
                    }
                }

                // Track damage for aura breaking (only actual damage, not absorbed)
                *damage_per_aura_break.entry(target_entity).or_insert(0.0) += actual_damage;

                // Batch damage for floating combat text (sum all damage to same target)
                *damage_per_target.entry(target_entity).or_insert(0.0) += actual_damage;
                *absorbed_per_target.entry(target_entity).or_insert(0.0) += absorbed;

                // Collect attacker damage for later update (include absorbed damage - attacker dealt it)
                damage_dealt_updates.push((attacker_entity, actual_damage + absorbed));

                // Log the attack with structured data
                if let (Some((attacker_team, attacker_class, attacker_name, attacker_is_melee, _)), Some((target_team, _target_class, target_name, _, _))) =
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

    // Apply Frost Armor procs: slow melee attackers who hit a target with FrostArmorBuff
    // Only apply if the attacker doesn't already have a Frost Armor slow (prevents DR escalation)
    for attacker_entity in frost_armor_procs {
        // Check if attacker already has the Frost Armor slow active
        let already_has_frost_slow = if let Ok((_, _, _, _, _, Some(ref attacker_auras))) = combatants.get(attacker_entity) {
            attacker_auras.auras.iter().any(|a| {
                a.effect_type == AuraType::MovementSpeedSlow && a.ability_name == "Frost Armor"
            })
        } else {
            false
        };
        if already_has_frost_slow {
            continue;
        }
        // Apply MovementSpeedSlow (30% slow = magnitude 0.7) for 5 seconds
        commands.spawn(AuraPending {
            target: attacker_entity,
            aura: Aura {
                effect_type: AuraType::MovementSpeedSlow,
                duration: 5.0,
                magnitude: 0.7, // 30% slow (0.7 = 70% speed)
                break_on_damage_threshold: -1.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Frost Armor".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(SpellSchool::Frost),
                applied_this_frame: false,
                backlash_damage: None,
                dr_category_override: None,
            },
        });
        // Apply AttackSpeedSlow (25% slower attacks) for 5 seconds
        commands.spawn(AuraPending {
            target: attacker_entity,
            aura: Aura {
                effect_type: AuraType::AttackSpeedSlow,
                duration: 5.0,
                magnitude: 0.25,
                break_on_damage_threshold: -1.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Frost Armor".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(SpellSchool::Frost),
                applied_this_frame: false,
                backlash_damage: None,
                dr_category_override: None,
            },
        });
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
