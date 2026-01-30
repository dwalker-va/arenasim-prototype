//! Warrior AI Module
//!
//! Handles AI decision-making for the Warrior class.
//!
//! ## Priority Order
//! 1. Battle Shout (buff self and allies pre-combat)
//! 2. Charge (gap closer when out of melee range)
//! 3. Rend (bleed DoT on target)
//! 4. Mortal Strike (main damage, healing reduction)
//! 5. Heroic Strike (rage dump)

use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{CHARGE_MIN_RANGE, GCD};

use super::{AbilityDecision, ClassAI, CombatContext};

/// Battle Shout range constant
const BATTLE_SHOUT_RANGE: f32 = 30.0;

/// Rage reserve for essential abilities
const RAGE_RESERVE: f32 = 50.0;

/// Warrior AI implementation.
///
/// Note: Currently uses direct execution via `decide_warrior_action()`.
/// The trait implementation is a stub for future refactoring.
pub struct WarriorAI;

impl ClassAI for WarriorAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate to trait-based decision making
        // For now, use decide_warrior_action() directly from combat_ai.rs
        AbilityDecision::None
    }
}

/// Warrior AI: Decides and executes abilities for a Warrior combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
#[allow(clippy::too_many_arguments)]
pub fn decide_warrior_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    instant_attacks: &mut Vec<(Entity, Entity, f32, u8, CharacterClass, AbilityType)>,
) -> bool {
    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Battle Shout (buff allies)
    if try_battle_shout(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        positions,
        combatant_info,
        active_auras_map,
    ) {
        return true;
    }

    // Get target for combat abilities
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(&target_pos) = positions.get(&target_entity) else {
        return false;
    };

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
        combatant_info,
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
        combatant_info,
        active_auras_map,
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
        combatant_info,
        instant_attacks,
    ) {
        return true;
    }

    // Priority 5: Heroic Strike (rage dump)
    try_heroic_strike(abilities, combatant, target_pos, my_pos);

    false
}

/// Try to cast Battle Shout to buff nearby allies.
/// Returns true if the ability was used.
#[allow(clippy::too_many_arguments)]
fn try_battle_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Check if any nearby ally needs the buff
    let mut allies_to_buff: Vec<Entity> = Vec::new();

    for (ally_entity, &(ally_team, _, _ally_class, ally_hp, _ally_max_hp)) in combatant_info.iter() {
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
            continue;
        }

        // Check if ally already has AttackPowerIncrease buff
        let has_battle_shout = active_auras_map
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::AttackPowerIncrease))
            .unwrap_or(false);

        if !has_battle_shout {
            allies_to_buff.push(*ally_entity);
        }
    }

    if allies_to_buff.is_empty() {
        return false;
    }

    let ability = AbilityType::BattleShout;
    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost && def.mana_cost > 0.0 {
        return false;
    }

    // Execute the ability
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    combat_log.log_ability_cast(
        caster_id,
        "Battle Shout".to_string(),
        None,
        format!(
            "Team {} {} uses Battle Shout",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply buff to all nearby allies
    if let Some(aura) = def.applies_aura.as_ref() {
        for ally_entity in allies_to_buff {
            commands.spawn(AuraPending {
                target: ally_entity,
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
                    spell_school: None, // Buff, not dispellable by Dispel Magic
                },
            });
        }
    }

    info!(
        "Team {} {} uses Battle Shout",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to use Charge to close distance.
/// Returns true if Charge was used.
#[allow(clippy::too_many_arguments)]
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
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
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Charge".to_string(),
        target_id,
        format!(
            "Team {} {} uses Charge",
            combatant.team,
            combatant.class.name()
        ),
    );

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
#[allow(clippy::too_many_arguments)]
fn try_rend(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Check if target already has Rend (any DoT for now)
    let target_has_rend = active_auras_map
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
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Rend".to_string(),
        target_id,
        format!(
            "Team {} {} uses Rend",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply DoT aura
    if let Some(aura) = rend_def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: target_entity,
            aura: Aura {
                effect_type: aura.aura_type,
                duration: aura.duration,
                magnitude: aura.magnitude,
                break_on_damage_threshold: aura.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: aura.tick_interval,
                time_until_next_tick: aura.tick_interval,
                caster: Some(entity),
                ability_name: rend_def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: None, // Physical DoT, NOT dispellable
            },
        });
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
#[allow(clippy::too_many_arguments)]
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
    instant_attacks: &mut Vec<(Entity, Entity, f32, u8, CharacterClass, AbilityType)>,
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
    let (target_team, target_class) = match combatant_info.get(&target_entity) {
        Some(&(team, _, class, _, _)) => (team, class),
        None => return false,
    };

    // Execute Mortal Strike
    combatant.current_mana -= ms_def.mana_cost;
    combatant.ability_cooldowns.insert(mortal_strike, ms_def.cooldown);
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    combat_log.log_ability_cast(
        caster_id,
        "Mortal Strike".to_string(),
        Some(format!("Team {} {}", target_team, target_class.name())),
        format!(
            "Team {} {} uses Mortal Strike",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Calculate and queue damage
    let damage = combatant.calculate_ability_damage_config(ms_def, game_rng);
    instant_attacks.push((
        entity,
        target_entity,
        damage,
        combatant.team,
        combatant.class,
        mortal_strike,
    ));

    // Apply healing reduction aura
    if let Some(aura) = ms_def.applies_aura.as_ref() {
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
                ability_name: ms_def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: None, // Physical debuff, NOT dispellable
            },
        });
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
