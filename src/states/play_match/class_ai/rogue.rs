//! Rogue AI Module
//!
//! Handles AI decision-making for the Rogue class.
//!
//! ## Priority Order (Stealthed)
//! 1. Ambush (opener from stealth)
//!
//! ## Priority Order (In Combat)
//! 1. Kidney Shot (stun)
//! 2. Sinister Strike (combo point builder)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::match_config::RogueOpener;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::{roll_crit, get_attack_power_bonus_from_slice, get_crit_chance_bonus_from_slice};
use crate::states::play_match::constants::{CRIT_DAMAGE_MULTIPLIER, GCD, MELEE_RANGE};
use crate::states::play_match::utils::{combatant_id, log_ability_use, spawn_speech_bubble};

use super::CombatContext;

/// Rogue AI: Decides and executes abilities for a Rogue combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_rogue_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
) -> bool {
    // Get target
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_pos) = ctx.combatants.get(&target_entity).map(|info| info.position) else {
        return false;
    };

    // Don't waste abilities on immune targets (Divine Shield)
    if ctx.entity_is_immune(target_entity) {
        return false;
    }

    if combatant.stealthed {
        // Stealthed: Use opener based on preference
        return match combatant.rogue_opener {
            RogueOpener::Ambush => try_ambush(
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                target_entity,
                target_pos,
                ctx,
                instant_attacks,
            ),
            RogueOpener::CheapShot => try_cheap_shot(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                target_entity,
                target_pos,
                ctx,
                same_frame_cc_queue,
            ),
        };
    }

    // Not stealthed: Check GCD first
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Kidney Shot (melee-range CC)
    // For melee CC, we need range-aware targeting:
    // - If CC target is in melee range, use it (strategic CC on healer)
    // - If CC target is out of range but kill target is in range, use kill target
    // - A stun on kill target is still valuable (helps secure kill)
    // - Don't use if target is already stunned (waste of CC)
    let kidney_shot_target = select_melee_cc_target(
        combatant.cc_target,
        combatant.target,
        my_pos,
        ctx,
    );
    if let Some((ks_target_entity, ks_target_pos)) = kidney_shot_target {
        // Check if target is already stunned or DR-immune to stuns
        let target_already_stunned = ctx.active_auras
            .get(&ks_target_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::Stun))
            .unwrap_or(false);

        if !target_already_stunned && !ctx.is_dr_immune(ks_target_entity, DRCategory::Stuns) {
            if try_kidney_shot(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                ks_target_entity,
                ks_target_pos,
                ctx,
                same_frame_cc_queue,
            ) {
                return true;
            }
        }
    }

    // Priority 2: Sinister Strike
    try_sinister_strike(
        combat_log,
        game_rng,
        abilities,
        entity,
        combatant,
        my_pos,
        target_entity,
        target_pos,
        ctx,
        instant_attacks,
    )
}

/// Try to use Ambush from stealth.
/// Returns true if Ambush was used.
fn try_ambush(
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
) -> bool {
    let ability = AbilityType::Ambush;
    let def = abilities.get_unchecked(&ability);

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Execute Ambush
    combatant.current_mana -= def.mana_cost;
    combatant.stealthed = false;
    combatant.global_cooldown = GCD;

    // Calculate and queue damage (with dynamic aura bonuses)
    let self_auras = ctx.active_auras.get(&entity).map(|v| v.as_slice()).unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, ap_bonus);
    let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
    if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Ambush", target_tuple, "uses");

    info!(
        "Team {} {} uses {} from stealth!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Cheap Shot from stealth.
/// Returns true if Cheap Shot was used.
fn try_cheap_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
) -> bool {
    let ability = AbilityType::CheapShot;
    let def = abilities.get_unchecked(&ability);

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Execute Cheap Shot
    spawn_speech_bubble(commands, entity, "Cheap Shot");
    combatant.current_mana -= def.mana_cost;
    combatant.stealthed = false;
    combatant.global_cooldown = GCD;

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Cheap Shot", target_tuple, "uses");

    // Apply stun aura
    if let Some(aura) = def.applies_aura.as_ref() {
        if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, def) {
            // Reflect the stun in this frame's snapshot so class AIs running later in the
            // same `decide_abilities` loop see the target as stunned and do not waste a
            // cast or interrupt on a target that is about to be CC'd anyway.
            same_frame_cc_queue.push((target_entity, aura_pending.aura.clone()));
            commands.spawn(aura_pending);
        }

        // Log CC
        if let Some(info) = ctx.combatants.get(&target_entity) {
            let cc_type = format!("{:?}", aura.aura_type);
            let message = format!(
                "Team {} {} uses {} on Team {} {}",
                combatant.team,
                combatant.class.name(),
                def.name,
                info.team,
                info.class.name()
            );
            combat_log.log_crowd_control(
                combatant_id(combatant.team, combatant.class),
                combatant_id(info.team, info.class),
                cc_type,
                aura.duration,
                message,
            );
        }
    }

    info!(
        "Team {} {} uses {} from stealth!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Kidney Shot.
/// Returns true if Kidney Shot was used.
fn try_kidney_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
) -> bool {
    let kidney_shot = AbilityType::KidneyShot;
    let ks_on_cooldown = combatant.ability_cooldowns.contains_key(&kidney_shot);

    if ks_on_cooldown {
        return false;
    }

    let def = abilities.get_unchecked(&kidney_shot);

    if !kidney_shot.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Execute Kidney Shot
    spawn_speech_bubble(commands, entity, "Kidney Shot");
    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(kidney_shot, def.cooldown);
    combatant.global_cooldown = GCD;

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Kidney Shot", target_tuple, "uses");

    // Apply stun aura
    if let Some(aura) = def.applies_aura.as_ref() {
        if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, def) {
            // Reflect same-frame — see try_cheap_shot for rationale.
            same_frame_cc_queue.push((target_entity, aura_pending.aura.clone()));
            commands.spawn(aura_pending);
        }

        // Log CC
        if let Some(info) = ctx.combatants.get(&target_entity) {
            let cc_type = format!("{:?}", aura.aura_type);
            let message = format!(
                "Team {} {} uses {} on Team {} {}",
                combatant.team,
                combatant.class.name(),
                def.name,
                info.team,
                info.class.name()
            );
            combat_log.log_crowd_control(
                combatant_id(combatant.team, combatant.class),
                combatant_id(info.team, info.class),
                cc_type,
                aura.duration,
                message,
            );
        }
    }

    info!(
        "Team {} {} uses {} on enemy!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Sinister Strike.
/// Returns true if Sinister Strike was used.
fn try_sinister_strike(
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
) -> bool {
    let ability = AbilityType::SinisterStrike;
    let def = abilities.get_unchecked(&ability);

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Execute Sinister Strike
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Calculate and queue damage (with dynamic aura bonuses)
    let self_auras = ctx.active_auras.get(&entity).map(|v| v.as_slice()).unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, ap_bonus);
    let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
    if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Sinister Strike", target_tuple, "uses");

    info!(
        "Team {} {} uses {}!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Select the best target for melee-range CC abilities.
///
/// For melee CC like Kidney Shot, we need range-aware targeting:
/// 1. If CC target is in melee range, use it (strategic CC on healer)
/// 2. If CC target is out of range but kill target is in range, fall back to kill target
/// 3. If neither is in range, return None
///
/// A stun on the kill target is still valuable even if not the ideal CC target.
fn select_melee_cc_target(
    cc_target: Option<Entity>,
    kill_target: Option<Entity>,
    my_pos: Vec3,
    ctx: &CombatContext,
) -> Option<(Entity, Vec3)> {
    // First, check if CC target is in melee range and not immune
    if let Some(cc_entity) = cc_target {
        if !ctx.entity_is_immune(cc_entity) {
            if let Some(info) = ctx.combatants.get(&cc_entity) {
                if my_pos.distance(info.position) <= MELEE_RANGE {
                    return Some((cc_entity, info.position));
                }
            }
        }
    }

    // CC target not in range - fall back to kill target if in melee range and not immune
    if let Some(kill_entity) = kill_target {
        if !ctx.entity_is_immune(kill_entity) {
            if let Some(info) = ctx.combatants.get(&kill_entity) {
                if my_pos.distance(info.position) <= MELEE_RANGE {
                    return Some((kill_entity, info.position));
                }
            }
        }
    }

    // Neither target in melee range
    None
}
