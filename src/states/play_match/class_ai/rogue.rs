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

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::match_config::RogueOpener;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::roll_crit;
use crate::states::play_match::constants::{CRIT_DAMAGE_MULTIPLIER, GCD, MELEE_RANGE};
use crate::states::play_match::utils::{combatant_id, spawn_speech_bubble};

use super::{AbilityDecision, ClassAI, CombatContext};

/// Rogue AI implementation.
///
/// Note: Currently uses direct execution via `decide_rogue_action()`.
/// The trait implementation is a stub for future refactoring.
pub struct RogueAI;

impl ClassAI for RogueAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate to trait-based decision making
        // For now, use decide_rogue_action() directly from combat_ai.rs
        AbilityDecision::None
    }
}

/// Rogue AI: Decides and executes abilities for a Rogue combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
#[allow(clippy::too_many_arguments)]
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
) -> bool {
    // Get target
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_pos) = ctx.combatants.get(&target_entity).map(|info| info.position) else {
        return false;
    };

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
        // Check if target is already stunned - don't waste Kidney Shot
        let target_already_stunned = ctx.active_auras
            .get(&ks_target_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::Stun))
            .unwrap_or(false);

        if !target_already_stunned {
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
#[allow(clippy::too_many_arguments)]
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

    // Calculate and queue damage
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng);
    let is_crit = roll_crit(combatant.crit_chance, game_rng);
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
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Ambush".to_string(),
        target_id,
        format!(
            "Team {} {} uses Ambush from stealth",
            combatant.team,
            combatant.class.name()
        ),
    );

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
#[allow(clippy::too_many_arguments)]
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
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Cheap Shot".to_string(),
        target_id.clone(),
        format!(
            "Team {} {} uses Cheap Shot from stealth",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply stun aura
    if let Some(aura) = def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: target_entity,
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
                spell_school: None, // Physical stun, not dispellable
            },
        });

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
#[allow(clippy::too_many_arguments)]
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
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Kidney Shot".to_string(),
        target_id.clone(),
        format!(
            "Team {} {} uses Kidney Shot",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply stun aura
    if let Some(aura) = def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: target_entity,
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
                spell_school: None, // Physical stun, not dispellable
            },
        });

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
#[allow(clippy::too_many_arguments)]
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

    // Calculate and queue damage
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng);
    let is_crit = roll_crit(combatant.crit_chance, game_rng);
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
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Sinister Strike".to_string(),
        target_id,
        format!(
            "Team {} {} uses Sinister Strike",
            combatant.team,
            combatant.class.name()
        ),
    );

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
    // First, check if CC target is in melee range
    if let Some(cc_entity) = cc_target {
        if let Some(info) = ctx.combatants.get(&cc_entity) {
            if my_pos.distance(info.position) <= MELEE_RANGE {
                return Some((cc_entity, info.position));
            }
        }
    }

    // CC target not in range - fall back to kill target if in melee range
    if let Some(kill_entity) = kill_target {
        if let Some(info) = ctx.combatants.get(&kill_entity) {
            if my_pos.distance(info.position) <= MELEE_RANGE {
                return Some((kill_entity, info.position));
            }
        }
    }

    // Neither target in melee range
    None
}
