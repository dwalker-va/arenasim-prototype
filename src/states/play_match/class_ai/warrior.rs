//! Warrior AI Module
//!
//! Handles AI decision-making for the Warrior class.
//!
//! ## Priority Order
//! 1. Shout (buff allies or debuff enemies based on warrior_shout preference)
//! 2. Charge (gap closer when out of melee range)
//! 3. Rend (bleed DoT on target)
//! 4. Mortal Strike (main damage, healing reduction)
//! 5. Heroic Strike (rage dump)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::match_config::WarriorShout;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::roll_crit;
use crate::states::play_match::constants::{CHARGE_MIN_RANGE, CRIT_DAMAGE_MULTIPLIER, GCD};

use crate::states::play_match::utils::log_ability_use;

use super::CombatContext;

/// Shout range constant (applies to all shout variants)
const SHOUT_RANGE: f32 = 30.0;

/// Rage reserve for essential abilities
const RAGE_RESERVE: f32 = 50.0;

/// Warrior AI: Decides and executes abilities for a Warrior combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_warrior_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    battle_shouted_this_frame: &mut std::collections::HashSet<Entity>,
) -> bool {
    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Shout (buff allies or debuff enemies based on preference)
    if try_shout(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        ctx,
        battle_shouted_this_frame,
    ) {
        return true;
    }

    // Get target for combat abilities
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        return false;
    };
    let target_pos = target_info.position;

    // Don't waste abilities on immune targets (Divine Shield)
    if ctx.entity_is_immune(target_entity) {
        return false;
    }

    // Priority 2: Charge (gap closer)
    if try_charge(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        target_entity,
        target_pos,
        ctx,
    ) {
        return true;
    }

    // Priority 3: Rend (DoT)
    if try_rend(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        target_entity,
        target_pos,
        ctx,
    ) {
        return true;
    }

    // Priority 4: Mortal Strike
    if try_mortal_strike(
        commands,
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
    ) {
        return true;
    }

    // Priority 5: Heroic Strike (rage dump)
    try_heroic_strike(abilities, combatant, target_pos, my_pos);

    false
}

/// Try to cast the warrior's chosen shout (Battle Shout, Demoralizing Shout, or Commanding Shout).
/// Dispatches based on `combatant.warrior_shout` preference.
/// Returns true if the ability was used.
fn try_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
) -> bool {
    match combatant.warrior_shout {
        WarriorShout::BattleShout => try_battle_shout(
            commands, combat_log, abilities, entity, combatant, my_pos, ctx, shouted_this_frame,
        ),
        WarriorShout::DemoralizingShout => try_demoralizing_shout(
            commands, combat_log, abilities, entity, combatant, my_pos, ctx, shouted_this_frame,
        ),
        WarriorShout::CommandingShout => try_commanding_shout(
            commands, combat_log, abilities, entity, combatant, my_pos, ctx, shouted_this_frame,
        ),
    }
}

/// Try to cast Battle Shout to buff nearby allies with AttackPowerIncrease.
/// Returns true if the ability was used.
fn try_battle_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
) -> bool {
    let mut targets: Vec<Entity> = Vec::new();

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 {
            continue;
        }
        if my_pos.distance(info.position) > SHOUT_RANGE {
            continue;
        }

        let already_has = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::AttackPowerIncrease))
            .unwrap_or(false);

        if !already_has && !shouted_this_frame.contains(ally_entity) {
            targets.push(*ally_entity);
        }
    }

    if targets.is_empty() {
        return false;
    }

    let ability = AbilityType::BattleShout;
    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost && def.mana_cost > 0.0 {
        return false;
    }

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Battle Shout", None, "uses");

    for target in targets {
        shouted_this_frame.insert(target);
        if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
            commands.spawn(aura_pending);
        }
    }

    info!(
        "Team {} {} uses Battle Shout",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Demoralizing Shout to debuff nearby enemies with AttackPowerReduction.
/// Returns true if the ability was used.
fn try_demoralizing_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
) -> bool {
    let mut targets: Vec<Entity> = Vec::new();

    for (enemy_entity, info) in ctx.combatants.iter() {
        // Must be opposite team, alive, and visible (not stealthed)
        if info.team == combatant.team || info.current_health <= 0.0 || info.stealthed {
            continue;
        }
        if my_pos.distance(info.position) > SHOUT_RANGE {
            continue;
        }

        let already_has = ctx.active_auras
            .get(enemy_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::AttackPowerReduction))
            .unwrap_or(false);

        if !already_has && !shouted_this_frame.contains(enemy_entity) {
            targets.push(*enemy_entity);
        }
    }

    if targets.is_empty() {
        return false;
    }

    let ability = AbilityType::DemoralizingShout;
    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost && def.mana_cost > 0.0 {
        return false;
    }

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Demoralizing Shout", None, "uses");

    for target in targets {
        shouted_this_frame.insert(target);
        if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
            commands.spawn(aura_pending);
        }
    }

    info!(
        "Team {} {} uses Demoralizing Shout",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Commanding Shout to buff nearby allies with MaxHealthIncrease.
/// Returns true if the ability was used.
fn try_commanding_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
) -> bool {
    let mut targets: Vec<Entity> = Vec::new();

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 {
            continue;
        }
        if my_pos.distance(info.position) > SHOUT_RANGE {
            continue;
        }

        let already_has = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxHealthIncrease))
            .unwrap_or(false);

        if !already_has && !shouted_this_frame.contains(ally_entity) {
            targets.push(*ally_entity);
        }
    }

    if targets.is_empty() {
        return false;
    }

    let ability = AbilityType::CommandingShout;
    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost && def.mana_cost > 0.0 {
        return false;
    }

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Commanding Shout", None, "uses");

    for target in targets {
        shouted_this_frame.insert(target);
        if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
            commands.spawn(aura_pending);
        }
    }

    info!(
        "Team {} {} uses Commanding Shout",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to use Charge to close distance.
/// Returns true if Charge was used.
fn try_charge(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
) -> bool {
    let charge = AbilityType::Charge;
    let charge_def = abilities.get_unchecked(&charge);
    let charge_on_cooldown = combatant.ability_cooldowns.contains_key(&charge);

    if charge_on_cooldown {
        return false;
    }

    // Check if rooted
    let is_rooted = auras
        .map(|a| a.auras.iter().any(|aura| matches!(aura.effect_type, AuraType::Root)))
        .unwrap_or(false);

    if is_rooted {
        return false;
    }

    let distance_to_target = my_pos.distance(target_pos);

    // Must be within charge range
    if distance_to_target < CHARGE_MIN_RANGE || distance_to_target > charge_def.range {
        return false;
    }

    // Execute Charge
    combatant.ability_cooldowns.insert(charge, charge_def.cooldown);
    combatant.global_cooldown = GCD;

    commands.entity(entity).insert(ChargingState {
        target: target_entity,
    });

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Charge", target_tuple, "uses");

    info!(
        "Team {} {} uses Charge on enemy (distance: {:.1} units)",
        combatant.team,
        combatant.class.name(),
        distance_to_target
    );

    true
}

/// Try to apply Rend DoT to target.
/// Returns true if Rend was used.
fn try_rend(
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
    // Don't apply Rend to a target polymorphed by our own team
    if ctx.has_friendly_breakable_cc(target_entity) {
        return false;
    }

    // Check if target already has Rend (any DoT for now)
    let target_has_rend = ctx.active_auras
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime))
        .unwrap_or(false);

    if target_has_rend {
        return false;
    }

    let rend = AbilityType::Rend;
    let rend_def = abilities.get_unchecked(&rend);

    if !rend.can_cast_config(combatant, target_pos, my_pos, rend_def) {
        return false;
    }

    // Execute Rend
    combatant.current_mana -= rend_def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Rend", target_tuple, "uses");

    // Apply DoT aura
    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, rend_def) {
        commands.spawn(aura_pending);
    }

    combat_log.log(
        CombatLogEventType::Buff,
        format!(
            "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to use Mortal Strike.
/// Returns true if Mortal Strike was used.
fn try_mortal_strike(
    commands: &mut Commands,
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
    let mortal_strike = AbilityType::MortalStrike;
    let ms_def = abilities.get_unchecked(&mortal_strike);
    let ms_on_cooldown = combatant.ability_cooldowns.contains_key(&mortal_strike);

    if ms_on_cooldown {
        return false;
    }

    if !mortal_strike.can_cast_config(combatant, target_pos, my_pos, ms_def) {
        return false;
    }

    if combatant.current_mana < ms_def.mana_cost {
        return false;
    }

    // Get target info
    let target_info = match ctx.combatants.get(&target_entity) {
        Some(info) => info,
        None => return false,
    };

    // Execute Mortal Strike
    combatant.current_mana -= ms_def.mana_cost;
    combatant.ability_cooldowns.insert(mortal_strike, ms_def.cooldown);
    combatant.global_cooldown = GCD;

    // Log
    log_ability_use(combat_log, combatant.team, combatant.class, "Mortal Strike", Some((target_info.team, target_info.class)), "uses");

    // Calculate and queue damage
    let mut damage = combatant.calculate_ability_damage_config(ms_def, game_rng);
    let is_crit = roll_crit(combatant.crit_chance, game_rng);
    if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_class: combatant.class,
        ability: mortal_strike,
        is_crit,
    });

    // Apply healing reduction aura
    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, ms_def) {
        commands.spawn(aura_pending);
    }

    info!(
        "Team {} {} uses Mortal Strike for {:.0} damage!",
        combatant.team,
        combatant.class.name(),
        damage
    );

    true
}

/// Try to queue Heroic Strike for next auto-attack.
/// This doesn't consume a GCD, just queues bonus damage.
fn try_heroic_strike(abilities: &AbilityDefinitions, combatant: &mut Combatant, target_pos: Vec3, my_pos: Vec3) {
    // Don't queue if one is already pending
    if combatant.next_attack_bonus_damage > 0.0 {
        return;
    }

    let ability = AbilityType::HeroicStrike;
    let def = abilities.get_unchecked(&ability);

    // Only use if we have enough rage for Heroic Strike AND reserve
    let can_afford = combatant.current_mana >= (def.mana_cost + RAGE_RESERVE);

    if !can_afford {
        return;
    }

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return;
    }

    // Consume rage and queue bonus damage
    combatant.current_mana -= def.mana_cost;
    let bonus_damage = combatant.attack_damage * 0.5;
    combatant.next_attack_bonus_damage = bonus_damage;
    combatant.global_cooldown = GCD;

    info!(
        "Team {} {} uses Heroic Strike (next attack +{:.0} damage)",
        combatant.team,
        combatant.class.name(),
        bonus_damage
    );
}
