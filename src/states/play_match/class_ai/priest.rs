//! Priest AI Module
//!
//! Handles AI decision-making for the Priest class.
//!
//! ## Priority Order
//! 1. Power Word: Fortitude (buff all allies pre-combat)
//! 2. Dispel Magic - Urgent (Polymorph, Fear - complete loss of control)
//! 3. Power Word: Shield (shield low-health allies)
//! 4. Flash Heal (heal injured allies)
//! 5. Dispel Magic - Maintenance (Roots, DoTs when team HP is stable)
//! 6. Mind Blast (damage when allies are healthy)

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::utils::combatant_id;

use super::{AbilityDecision, ClassAI, CombatContext, is_team_healthy};

/// Priest AI implementation.
///
/// Note: Currently uses direct execution via `decide_priest_action()`.
/// The trait implementation is a stub for future refactoring.
pub struct PriestAI;

impl ClassAI for PriestAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate to trait-based decision making
        // For now, use decide_priest_action() directly from combat_ai.rs
        AbilityDecision::None
    }
}

/// Priest AI: Decides and executes abilities for a Priest combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
#[allow(clippy::too_many_arguments)]
pub fn decide_priest_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    shielded_this_frame: &mut HashSet<Entity>,
    fortified_this_frame: &mut HashSet<Entity>,
) -> bool {
    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Power Word: Fortitude (buff allies)
    if try_fortitude(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
        active_auras_map,
        fortified_this_frame,
    ) {
        return true;
    }

    // Priority 2: Dispel Magic - Urgent (Polymorph, Fear - complete loss of control)
    // These debuffs completely incapacitate, so dispel immediately - before anything else
    if try_dispel_magic(
        commands,
        combat_log,
        abilities,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
        active_auras_map,
        90, // Only Polymorph (100) and Fear (90)
    ) {
        return true;
    }

    // Priority 3: Power Word: Shield (shield allies)
    if try_power_word_shield(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
        active_auras_map,
        shielded_this_frame,
    ) {
        return true;
    }

    // Priority 4: Flash Heal (heal injured allies)
    if try_flash_heal(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
    ) {
        return true;
    }

    // Priority 5: Dispel Magic - Maintenance (Roots, DoTs when team is healthy)
    // Only clean up lesser debuffs when there's no urgent healing needed
    if is_team_healthy(combatant.team, combatant_info) {
        if try_dispel_magic(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            positions,
            combatant_info,
            active_auras_map,
            50, // Roots (80) and DoTs (50)
        ) {
            return true;
        }
    }

    // Priority 6: Mind Blast (damage)
    if try_mind_blast(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
    ) {
        return true;
    }

    false
}

/// Try to cast Power Word: Fortitude on an unbuffed ally.
/// Returns true if the ability was used.
#[allow(clippy::too_many_arguments)]
fn try_fortitude(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    fortified_this_frame: &mut HashSet<Entity>,
) -> bool {
    // Find an unbuffed ally
    let mut unbuffed_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, &(ally_team, _, _ally_class, ally_hp, _ally_max_hp, _)) in combatant_info.iter() {
        // Must be same team and alive
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        // Check if ally already has MaxHealthIncrease buff
        let has_fortitude = active_auras_map
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxHealthIncrease))
            .unwrap_or(false);

        if has_fortitude {
            continue;
        }

        // Check if target was fortified by another Priest this frame
        if fortified_this_frame.contains(ally_entity) {
            continue;
        }

        // Get position
        let Some(&ally_pos) = positions.get(ally_entity) else {
            continue;
        };

        unbuffed_ally = Some((*ally_entity, ally_pos));
        break;
    }

    let Some((buff_target, target_pos)) = unbuffed_ally else {
        return false;
    };

    let ability = AbilityType::PowerWordFortitude;
    let def = abilities.get_unchecked(&ability);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check range and mana
    let distance = my_pos.distance(target_pos);
    if distance > def.range || combatant.current_mana < def.mana_cost {
        return false;
    }

    // Execute the ability
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info.get(&buff_target).map(|(team, _, class, _, _, _)| {
        format!("Team {} {}", team, class.name())
    });
    combat_log.log_ability_cast(
        caster_id,
        "Power Word: Fortitude".to_string(),
        target_id,
        format!(
            "Team {} {} casts Power Word: Fortitude",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply buff aura
    if let Some(aura) = def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: buff_target,
            aura: Aura {
                effect_type: aura.aura_type,
                duration: aura.duration,
                magnitude: aura.magnitude,
                break_on_damage_threshold: aura.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: Some(entity),
                ability_name: def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(def.spell_school),
            },
        });
    }

    // Mark target as fortified this frame
    fortified_this_frame.insert(buff_target);

    info!(
        "Team {} {} casts Power Word: Fortitude on ally",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Power Word: Shield on an ally.
/// Returns true if the ability was used.
#[allow(clippy::too_many_arguments)]
fn try_power_word_shield(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    shielded_this_frame: &mut HashSet<Entity>,
) -> bool {
    let pw_shield = AbilityType::PowerWordShield;
    let pw_shield_def = abilities.get_unchecked(&pw_shield);

    if is_spell_school_locked(pw_shield_def.spell_school, auras) {
        return false;
    }

    if combatant.current_mana < pw_shield_def.mana_cost {
        return false;
    }

    // Find ally to shield (prioritize lowest HP)
    let mut best_candidate: Option<(Entity, Vec3, f32)> = None;

    for (ally_entity, &(ally_team, _, _ally_class, ally_hp, ally_max_hp, _)) in combatant_info.iter() {
        // Must be same team and alive
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        // Check if ally has Weakened Soul or already has Power Word: Shield
        let ally_auras = active_auras_map.get(ally_entity);
        let has_weakened_soul = ally_auras
            .map_or(false, |auras| auras.iter().any(|a| a.effect_type == AuraType::WeakenedSoul));
        let has_pw_shield = ally_auras.map_or(false, |auras| {
            auras
                .iter()
                .any(|a| a.effect_type == AuraType::Absorb && a.ability_name == "Power Word: Shield")
        });

        // Check if target was shielded by another Priest this frame
        let shielded_this_frame_check = shielded_this_frame.contains(ally_entity);

        if has_weakened_soul || has_pw_shield || shielded_this_frame_check {
            continue;
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
            match best_candidate {
                None => best_candidate = Some((*ally_entity, ally_pos, hp_percent)),
                Some((_, _, best_percent)) if hp_percent < best_percent => {
                    best_candidate = Some((*ally_entity, ally_pos, hp_percent));
                }
                _ => {}
            }
        }
    }

    let Some((shield_entity, target_pos, _)) = best_candidate else {
        return false;
    };

    if !pw_shield.can_cast_config(combatant, target_pos, my_pos, pw_shield_def) {
        return false;
    }

    // Execute the ability
    combatant.current_mana -= pw_shield_def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info.get(&shield_entity).map(|(team, _, class, _, _, _)| {
        format!("Team {} {}", team, class.name())
    });
    combat_log.log_ability_cast(
        caster_id,
        "Power Word: Shield".to_string(),
        target_id,
        format!(
            "Team {} {} casts Power Word: Shield",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply absorb shield aura
    if let Some(aura) = pw_shield_def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: shield_entity,
            aura: Aura {
                effect_type: aura.aura_type,
                duration: aura.duration,
                magnitude: aura.magnitude,
                break_on_damage_threshold: aura.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: aura.tick_interval,
                time_until_next_tick: aura.tick_interval,
                caster: Some(entity),
                ability_name: pw_shield_def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(pw_shield_def.spell_school),
            },
        });
    }

    // Apply Weakened Soul debuff (doesn't break on damage)
    commands.spawn(AuraPending {
        target: shield_entity,
        aura: Aura {
            effect_type: AuraType::WeakenedSoul,
            duration: 15.0,
            magnitude: 0.0,
            break_on_damage_threshold: -1.0, // Never breaks on damage
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(entity),
            ability_name: "Weakened Soul".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None, // Weakened Soul is not dispellable
        },
    });

    // Mark target as shielded this frame
    shielded_this_frame.insert(shield_entity);

    true
}

/// Try to cast Dispel Magic on an ally with a dispellable debuff.
/// Returns true if the ability was used.
///
/// AI prioritizes dispelling based on severity:
/// - Polymorph (100) - complete incapacitate
/// - Fear (90) - loss of control
/// - Root (80) - movement impaired
/// - Magic DoTs (50) - Corruption, Immolate
/// - Movement slows (20) - Frostbolt slow (typically not worth dispelling)
///
/// The `min_priority` parameter controls which debuffs are considered:
/// - 90: Only urgent CC (Polymorph, Fear)
/// - 50: Include roots and DoTs
/// - 20: Include slows (not recommended)
///
/// Note: The actual debuff removed is random per WoW Classic behavior.
/// The AI just identifies which ally needs dispelling most.
#[allow(clippy::too_many_arguments)]
fn try_dispel_magic(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    min_priority: i32,
) -> bool {
    let ability = AbilityType::DispelMagic;
    let def = abilities.get_unchecked(&ability);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find ally with dispellable debuff, prioritized by severity
    // Priority: Polymorph > Fear > Root > Magic DoT > Slow
    let mut best_candidate: Option<(Entity, Vec3, i32)> = None; // (entity, pos, priority)

    for (ally_entity, &(ally_team, _, _ally_class, ally_hp, _ally_max_hp, _)) in combatant_info.iter() {
        // Must be same team and alive
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        // Check if ally has any dispellable debuffs
        let ally_auras = match active_auras_map.get(ally_entity) {
            Some(auras) => auras,
            None => continue,
        };

        // Find highest priority dispellable debuff on this ally
        let mut highest_priority = -1;
        for aura in ally_auras {
            if !aura.can_be_dispelled() {
                continue;
            }

            // Calculate priority based on aura type
            // Only dispel debuffs with priority >= 50 (meaningful CC or DoTs)
            // Don't waste mana/GCDs on minor slows
            let priority = match aura.effect_type {
                AuraType::Polymorph => 100,  // Highest - complete incapacitate
                AuraType::Fear => 90,         // Very high - loss of control
                AuraType::Root => 80,         // High - can't move
                AuraType::DamageOverTime => 50,  // Medium - taking damage
                AuraType::MovementSpeedSlow => 20, // Too low to dispel (threshold is 50)
                _ => 0, // Other types
            };

            if priority > highest_priority {
                highest_priority = priority;
            }
        }

        // Check against minimum priority threshold
        if highest_priority < min_priority {
            continue; // No debuffs worth dispelling at this priority level
        }

        // Get position
        let Some(&ally_pos) = positions.get(ally_entity) else {
            continue;
        };

        // Check if in range
        let distance = my_pos.distance(ally_pos);
        if distance > def.range {
            continue;
        }

        // Track best candidate
        match best_candidate {
            None => best_candidate = Some((*ally_entity, ally_pos, highest_priority)),
            Some((_, _, best_priority)) if highest_priority > best_priority => {
                best_candidate = Some((*ally_entity, ally_pos, highest_priority));
            }
            _ => {}
        }
    }

    let Some((dispel_target, _target_pos, _)) = best_candidate else {
        return false;
    };

    // Execute the ability
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Get the target's auras and remove a random dispellable one
    if let Some(target_auras) = active_auras_map.get(&dispel_target) {
        // Collect indices of dispellable auras
        let dispellable_indices: Vec<usize> = target_auras
            .iter()
            .enumerate()
            .filter(|(_, a)| a.can_be_dispelled())
            .map(|(i, _)| i)
            .collect();

        if !dispellable_indices.is_empty() {
            // We can't mutably access active_auras_map here, so we'll spawn a DispelPending
            // component that will be processed in a separate system.
            // Note: The actual aura removed is randomly selected in process_dispels (WoW Classic behavior),
            // so we don't log a specific aura name here - that's logged when the dispel actually happens.
            commands.spawn(DispelPending {
                target: dispel_target,
                log_prefix: "[DISPEL]",
            });

            // Log the dispel cast (the actual removal is logged in process_dispels)
            let caster_id = combatant_id(combatant.team, combatant.class);
            let target_id = combatant_info.get(&dispel_target).map(|(team, _, class, _, _, _)| {
                format!("Team {} {}", team, class.name())
            });
            combat_log.log_ability_cast(
                caster_id,
                "Dispel Magic".to_string(),
                target_id.clone(),
                format!(
                    "Team {} {} casts Dispel Magic on {}",
                    combatant.team,
                    combatant.class.name(),
                    target_id.unwrap_or_else(|| "ally".to_string())
                ),
            );

            info!(
                "Team {} {} casts Dispel Magic on ally",
                combatant.team,
                combatant.class.name()
            );

            return true;
        }
    }

    false
}

/// Pending dispel to be processed by the aura system.
/// This allows dispels to be applied without holding mutable references
/// to the aura map during AI decision making.
/// Note: The actual aura removed is randomly selected in process_dispels (WoW Classic behavior).
///
/// Used by both Priest (Dispel Magic) and Paladin (Cleanse) - only the log_prefix differs.
#[derive(bevy::prelude::Component)]
pub struct DispelPending {
    /// Target entity to dispel
    pub target: Entity,
    /// Log prefix for combat log (e.g., "[DISPEL]" for Priest, "[CLEANSE]" for Paladin)
    pub log_prefix: &'static str,
}

/// Try to cast Flash Heal on the lowest HP ally.
/// Returns true if the ability was used (started casting).
#[allow(clippy::too_many_arguments)]
fn try_flash_heal(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
) -> bool {
    // Find the lowest HP ally
    let mut lowest_hp_ally: Option<(Entity, f32, Vec3)> = None;

    for (ally_entity, &(ally_team, _, _ally_class, ally_hp, ally_max_hp, _)) in combatant_info.iter() {
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

        match lowest_hp_ally {
            None => lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos)),
            Some((_, lowest_percent, _)) if hp_percent < lowest_percent => {
                lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos));
            }
            _ => {}
        }
    }

    let Some((heal_target, _, target_pos)) = lowest_hp_ally else {
        return false;
    };

    let ability = AbilityType::FlashHeal;
    let def = abilities.get_unchecked(&ability);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Start casting (affected by Curse of Tongues)
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(heal_target),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&heal_target)
        .map(|(team, _, class, _, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        target_id,
        format!(
            "Team {} {} begins casting {}",
            combatant.team,
            combatant.class.name(),
            def.name
        ),
    );

    info!(
        "Team {} {} starts casting {} on ally",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Mind Blast on the current target.
/// Returns true if casting was started.
#[allow(clippy::too_many_arguments)]
fn try_mind_blast(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
) -> bool {
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(&target_pos) = positions.get(&target_entity) else {
        return false;
    };

    let ability = AbilityType::MindBlast;
    let on_cooldown = combatant.ability_cooldowns.contains_key(&ability);
    let def = abilities.get_unchecked(&ability);

    if on_cooldown {
        return false;
    }

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Execute the ability (affected by Curse of Tongues)
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, _, class, _, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        target_id,
        format!(
            "Team {} {} begins casting {}",
            combatant.team,
            combatant.class.name(),
            def.name
        ),
    );

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}
