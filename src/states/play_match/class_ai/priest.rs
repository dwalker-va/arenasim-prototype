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
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use std::collections::HashSet;

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::utils::log_ability_use;

use super::CombatContext;

/// Priest AI: Decides and executes abilities for a Priest combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_priest_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
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
        ctx,
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
        ctx,
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
        ctx,
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
        ctx,
    ) {
        return true;
    }

    // Priority 5: Dispel Magic - Maintenance (Roots, DoTs when team is healthy)
    // Only clean up lesser debuffs when there's no urgent healing needed
    if ctx.is_team_healthy(0.70, my_pos) {
        if try_dispel_magic(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            ctx,
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
        ctx,
    ) {
        return true;
    }

    false
}

/// Try to cast Power Word: Fortitude on an unbuffed ally.
/// Returns true if the ability was used.
fn try_fortitude(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    fortified_this_frame: &mut HashSet<Entity>,
) -> bool {
    // Find an unbuffed ally
    let mut unbuffed_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        // Must be same team, alive, and not a pet
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }

        // Check if ally already has MaxHealthIncrease buff
        let has_fortitude = ctx.active_auras
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

        unbuffed_ally = Some((*ally_entity, info.position));
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
    let target_tuple = ctx.combatants.get(&buff_target).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Power Word: Fortitude", target_tuple, "casts");

    // Apply buff aura
    if let Some(aura_pending) = AuraPending::from_ability(buff_target, entity, def) {
        commands.spawn(aura_pending);
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
fn try_power_word_shield(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
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

    for (ally_entity, info) in ctx.combatants.iter() {
        // Must be same team, alive, and not a pet
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }

        // Check if ally has Weakened Soul or already has Power Word: Shield
        let ally_auras = ctx.active_auras.get(ally_entity);
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

        let hp_percent = info.current_health / info.max_health;

        // Pre-combat (full HP): Shield anyone
        // In-combat: Only shield if below 70% HP
        let is_full_hp = hp_percent >= 1.0;
        let is_below_threshold = hp_percent < 0.7;

        if is_full_hp || is_below_threshold {
            match best_candidate {
                None => best_candidate = Some((*ally_entity, info.position, hp_percent)),
                Some((_, _, best_percent)) if hp_percent < best_percent => {
                    best_candidate = Some((*ally_entity, info.position, hp_percent));
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
    let target_tuple = ctx.combatants.get(&shield_entity).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Power Word: Shield", target_tuple, "casts");

    // Apply absorb shield aura
    if let Some(aura_pending) = AuraPending::from_ability(shield_entity, entity, pw_shield_def) {
        commands.spawn(aura_pending);
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
            applied_this_frame: false,
        },
    });

    // Mark target as shielded this frame
    shielded_this_frame.insert(shield_entity);

    true
}

/// Try to cast Dispel Magic on an ally with a dispellable debuff.
/// Returns true if the ability was used.
///
/// Delegates to the shared `try_dispel_ally()` in `class_ai/mod.rs`.
fn try_dispel_magic(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
) -> bool {
    super::try_dispel_ally(
        commands,
        combat_log,
        abilities,
        combatant,
        my_pos,
        auras,
        ctx,
        min_priority,
        AbilityType::DispelMagic,
        "[DISPEL]",
        "Dispel Magic",
        CharacterClass::Priest,
    )
}

/// Try to cast Flash Heal on the lowest HP ally.
/// Returns true if the ability was used (started casting).
fn try_flash_heal(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::FlashHeal;
    let def = abilities.get_unchecked(&ability);

    // Find the lowest HP ally below 90% health, within range, excluding pets
    let Some(target_info) = ctx.lowest_health_ally_below(0.9, def.range, my_pos) else {
        return false;
    };
    let heal_target = target_info.entity;
    let target_pos = target_info.position;

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

    commands.entity(entity).insert(CastingState::new(ability, heal_target, cast_time));

    // Log
    let target_tuple = ctx.combatants
        .get(&heal_target)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

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
fn try_mind_blast(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        return false;
    };

    if ctx.has_friendly_breakable_cc(target_entity) {
        return false;
    }

    // Don't waste Mind Blast on immune targets (Divine Shield)
    if ctx.entity_is_immune(target_entity) {
        return false;
    }

    let target_pos = target_info.position;

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

    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}
