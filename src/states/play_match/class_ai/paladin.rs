//! Paladin AI Module
//!
//! Holy warrior and healer - combines healing with melee utility.
//!
//! ## Priority Order
//! 1. Devotion Aura (buff all allies pre-combat)
//! 2. Cleanse - Urgent (Polymorph, Fear on allies)
//! 3. Emergency healing (ally < 40% HP) - Holy Shock (heal ally)
//! 4. Hammer of Justice (stun enemy in melee range)
//! 5. Standard healing (ally < 90% HP) - Flash of Light
//! 6. Holy Light (ally 50-85% HP, safe to cast long heal)
//! 7. Cleanse - Maintenance (roots, DoTs when team stable)
//! 8. Holy Shock (damage) - when team healthy

use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::{GCD, HOLY_SHOCK_DAMAGE_RANGE};
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::utils::combatant_id;

use super::{AbilityDecision, ClassAI, CombatContext, is_team_healthy};

/// Check if any ally is in an emergency situation (< 40% HP)
fn has_emergency_target(team: u8, combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>) -> bool {
    for &(ally_team, _, _, ally_hp, ally_max_hp) in combatant_info.values() {
        if ally_team != team || ally_hp <= 0.0 {
            continue;
        }
        let hp_percent = ally_hp / ally_max_hp;
        if hp_percent < 0.40 {
            return true;
        }
    }
    false
}

/// Paladin AI implementation
pub struct PaladinAI;

impl ClassAI for PaladinAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // Uses decide_paladin_action() directly from combat_ai.rs
        AbilityDecision::None
    }
}

/// Pending dispel to be processed by the aura system.
#[derive(Component)]
pub struct PaladinDispelPending {
    pub target: Entity,
}

/// Paladin AI: Decides and executes abilities for a Paladin combatant.
///
/// Returns `true` if an action was taken this frame.
#[allow(clippy::too_many_arguments)]
pub fn decide_paladin_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Devotion Aura (buff all allies pre-combat)
    if try_devotion_aura(
        commands,
        combat_log,
        abilities,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
        active_auras_map,
    ) {
        return true;
    }

    // Priority 2: Cleanse - Urgent (Polymorph, Fear on allies)
    if try_cleanse(
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

    // Priority 2: Emergency healing - Holy Shock (heal) when ally < 40% HP
    if has_emergency_target(combatant.team, combatant_info) {
        if try_holy_shock_heal(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            positions,
            combatant_info,
        ) {
            return true;
        }
    }

    // Priority 3: Hammer of Justice (stun enemy in melee range)
    if try_hammer_of_justice(
        commands,
        combat_log,
        abilities,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
    ) {
        return true;
    }

    // Priority 4: Standard healing - Flash of Light (ally < 90% HP)
    if try_flash_of_light(
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

    // Priority 5: Holy Light (ally damaged, safe to cast)
    // Use Holy Light when target is above 50% HP (safe to cast slow heal)
    if try_holy_light(
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

    // Priority 6: Cleanse - Maintenance (roots, DoTs when team stable)
    if is_team_healthy(combatant.team, combatant_info) {
        if try_cleanse(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            positions,
            combatant_info,
            active_auras_map,
            50, // Include roots and DoTs
        ) {
            return true;
        }
    }

    // Priority 7: Holy Shock (damage) - when team healthy
    if is_team_healthy(combatant.team, combatant_info) {
        if try_holy_shock_damage(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            positions,
            combatant_info,
        ) {
            return true;
        }
    }

    false
}

/// Try to cast Flash of Light on an injured ally.
#[allow(clippy::too_many_arguments)]
fn try_flash_of_light(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
) -> bool {
    let ability = AbilityType::FlashOfLight;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find the lowest HP ally (below 90%)
    let mut lowest_hp_ally: Option<(Entity, f32, Vec3)> = None;

    for (ally_entity, &(ally_team, _, _, ally_hp, ally_max_hp)) in combatant_info.iter() {
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        let hp_percent = ally_hp / ally_max_hp;
        if hp_percent >= 0.9 {
            continue;
        }

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

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(heal_target),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&heal_target)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
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

    true
}

/// Try to cast Holy Light on an injured ally (prioritize if above 50% HP for safe slow heal)
#[allow(clippy::too_many_arguments)]
fn try_holy_light(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
) -> bool {
    let ability = AbilityType::HolyLight;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find an ally between 50-85% HP (safe to use slow heal)
    let mut best_target: Option<(Entity, f32, Vec3)> = None;

    for (ally_entity, &(ally_team, _, _, ally_hp, ally_max_hp)) in combatant_info.iter() {
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        let hp_percent = ally_hp / ally_max_hp;
        // Only use Holy Light if ally is between 50-85% HP
        // Too low = need fast heal, too high = waste mana
        if hp_percent >= 0.85 || hp_percent < 0.50 {
            continue;
        }

        let Some(&ally_pos) = positions.get(ally_entity) else {
            continue;
        };

        match best_target {
            None => best_target = Some((*ally_entity, hp_percent, ally_pos)),
            Some((_, lowest_percent, _)) if hp_percent < lowest_percent => {
                best_target = Some((*ally_entity, hp_percent, ally_pos));
            }
            _ => {}
        }
    }

    let Some((heal_target, _, target_pos)) = best_target else {
        return false;
    };

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(heal_target),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&heal_target)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
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

    true
}

/// Try to cast Holy Shock as a heal on an emergency target (< 50% HP)
#[allow(clippy::too_many_arguments)]
fn try_holy_shock_heal(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check cooldown
    if combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0) > 0.0 {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find lowest HP ally below 50%
    let mut lowest_hp_ally: Option<(Entity, f32, Vec3)> = None;

    for (ally_entity, &(ally_team, _, _, ally_hp, ally_max_hp)) in combatant_info.iter() {
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        let hp_percent = ally_hp / ally_max_hp;
        if hp_percent >= 0.50 {
            continue;
        }

        let Some(&ally_pos) = positions.get(ally_entity) else {
            continue;
        };

        // Use ability's configured range
        if my_pos.distance(ally_pos) > def.range {
            continue;
        }

        match lowest_hp_ally {
            None => lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos)),
            Some((_, lowest_percent, _)) if hp_percent < lowest_percent => {
                lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos));
            }
            _ => {}
        }
    }

    let Some((heal_target, _, _)) = lowest_hp_ally else {
        return false;
    };

    // Execute instant heal
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&heal_target)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Holy Shock (Heal)".to_string(),
        target_id.clone(),
        format!(
            "Team {} {} casts Holy Shock on {}",
            combatant.team,
            combatant.class.name(),
            target_id.as_deref().unwrap_or("ally")
        ),
    );

    // Spawn pending heal
    commands.spawn(HolyShockHealPending {
        caster_spell_power: combatant.spell_power,
        caster_team: combatant.team,
        caster_class: combatant.class,
        target: heal_target,
    });

    true
}

/// Try to cast Holy Shock as damage on an enemy
#[allow(clippy::too_many_arguments)]
fn try_holy_shock_damage(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check cooldown
    if combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0) > 0.0 {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find an enemy in range (20 yards for damage)
    let mut damage_target: Option<(Entity, Vec3)> = None;

    for (enemy_entity, &(enemy_team, _, _, enemy_hp, _)) in combatant_info.iter() {
        if enemy_team == combatant.team || enemy_hp <= 0.0 {
            continue;
        }

        let Some(&enemy_pos) = positions.get(enemy_entity) else {
            continue;
        };

        // Use constant for damage range (shorter than heal range)
        if my_pos.distance(enemy_pos) > HOLY_SHOCK_DAMAGE_RANGE {
            continue;
        }

        damage_target = Some((*enemy_entity, enemy_pos));
        break;
    }

    let Some((target_entity, _)) = damage_target else {
        return false;
    };

    // Execute instant damage
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Holy Shock (Damage)".to_string(),
        target_id.clone(),
        format!(
            "Team {} {} casts Holy Shock on {}",
            combatant.team,
            combatant.class.name(),
            target_id.as_deref().unwrap_or("enemy")
        ),
    );

    // Spawn pending damage
    commands.spawn(HolyShockDamagePending {
        caster_spell_power: combatant.spell_power,
        caster_team: combatant.team,
        caster_class: combatant.class,
        target: target_entity,
    });

    true
}

/// Try to cast Hammer of Justice on an enemy in melee range
/// Prioritizes healers over DPS
#[allow(clippy::too_many_arguments)]
fn try_hammer_of_justice(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
) -> bool {
    let ability = AbilityType::HammerOfJustice;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check cooldown
    if combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0) > 0.0 {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find an enemy in range (prioritize healers)
    let mut best_target: Option<(Entity, Vec3, bool)> = None; // (entity, pos, is_healer)

    for (enemy_entity, &(enemy_team, _, enemy_class, enemy_hp, _)) in combatant_info.iter() {
        if enemy_team == combatant.team || enemy_hp <= 0.0 {
            continue;
        }

        let Some(&enemy_pos) = positions.get(enemy_entity) else {
            continue;
        };

        // Check range (10 yards)
        if my_pos.distance(enemy_pos) > def.range {
            continue;
        }

        let is_healer = matches!(enemy_class, CharacterClass::Priest | CharacterClass::Paladin);

        match best_target {
            None => best_target = Some((*enemy_entity, enemy_pos, is_healer)),
            Some((_, _, false)) if is_healer => {
                // Prefer healers over DPS
                best_target = Some((*enemy_entity, enemy_pos, true));
            }
            _ => {}
        }
    }

    // Only use if we have a target in range
    let Some((stun_target, _, _)) = best_target else {
        return false;
    };

    // Execute the stun
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&stun_target)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        target_id.clone(),
        format!(
            "Team {} {} casts Hammer of Justice on {}",
            combatant.team,
            combatant.class.name(),
            target_id.as_deref().unwrap_or("enemy")
        ),
    );

    // Apply stun aura
    if let Some(aura_def) = def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: stun_target,
            aura: Aura {
                effect_type: aura_def.aura_type,
                duration: aura_def.duration,
                magnitude: aura_def.magnitude,
                break_on_damage_threshold: aura_def.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(def.spell_school),
            },
        });
    }

    true
}

/// Try to cast Cleanse on an ally with a dispellable debuff.
#[allow(clippy::too_many_arguments)]
fn try_cleanse(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    min_priority: i32,
) -> bool {
    let ability = AbilityType::PaladinCleanse;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find ally with highest priority dispellable debuff
    let mut best_candidate: Option<(Entity, Vec3, i32)> = None;

    for (ally_entity, &(ally_team, _, _, ally_hp, _)) in combatant_info.iter() {
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        let Some(&ally_pos) = positions.get(ally_entity) else {
            continue;
        };

        // Check range
        if my_pos.distance(ally_pos) > def.range {
            continue;
        }

        // Check if ally has any dispellable debuffs
        let Some(ally_auras) = active_auras_map.get(ally_entity) else {
            continue;
        };

        // Find highest priority dispellable debuff on this ally
        let mut highest_priority = 0;
        for aura in ally_auras {
            if !aura.can_be_dispelled() {
                continue;
            }

            let priority = match aura.effect_type {
                AuraType::Polymorph => 100,
                AuraType::Fear => 90,
                AuraType::Root => 80,
                AuraType::DamageOverTime => 50,
                AuraType::MovementSpeedSlow => 20,
                _ => 0,
            };

            if priority > highest_priority {
                highest_priority = priority;
            }
        }

        if highest_priority < min_priority {
            continue;
        }

        match best_candidate {
            None => best_candidate = Some((*ally_entity, ally_pos, highest_priority)),
            Some((_, _, best_prio)) if highest_priority > best_prio => {
                best_candidate = Some((*ally_entity, ally_pos, highest_priority));
            }
            _ => {}
        }
    }

    let Some((dispel_target, _, _)) = best_candidate else {
        return false;
    };

    // Execute Cleanse
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&dispel_target)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Cleanse".to_string(),
        target_id.clone(),
        format!(
            "Team {} {} casts Cleanse on {}",
            combatant.team,
            combatant.class.name(),
            target_id.as_deref().unwrap_or("ally")
        ),
    );

    // Spawn pending dispel (uses same system as Priest's DispelMagic)
    commands.spawn(PaladinDispelPending {
        target: dispel_target,
    });

    true
}

/// Try to cast Devotion Aura on an unbuffed ally.
/// Returns true if the ability was used.
/// Similar to Priest's Power Word: Fortitude - buffs team pre-combat.
#[allow(clippy::too_many_arguments)]
fn try_devotion_aura(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    let ability = AbilityType::DevotionAura;
    let def = abilities.get_unchecked(&ability);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Find an ally without Devotion Aura (DamageTakenReduction from us)
    let mut buff_target: Option<Entity> = None;

    for (ally_entity, &(ally_team, _, _, ally_hp, _)) in combatant_info.iter() {
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        let Some(&ally_pos) = positions.get(ally_entity) else {
            continue;
        };

        // Check range (should always be in range with 100.0 range)
        if my_pos.distance(ally_pos) > def.range {
            continue;
        }

        // Check if ally already has Devotion Aura (DamageTakenReduction)
        let has_devotion_aura = active_auras_map
            .get(ally_entity)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageTakenReduction
                        && a.ability_name == "Devotion Aura"
                })
            })
            .unwrap_or(false);

        if has_devotion_aura {
            continue;
        }

        // Found an ally that needs buffing
        buff_target = Some(*ally_entity);
        break;
    }

    let Some(target) = buff_target else {
        return false;
    };

    // Apply Devotion Aura
    combatant.global_cooldown = GCD;

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&target)
        .map(|(team, _, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Devotion Aura".to_string(),
        target_id.clone(),
        format!(
            "Team {} {} casts Devotion Aura",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply the aura
    if let Some(aura_def) = def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target,
            aura: Aura {
                effect_type: aura_def.aura_type,
                duration: aura_def.duration,
                magnitude: aura_def.magnitude,
                break_on_damage_threshold: -1.0, // Never breaks on damage
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(def.spell_school),
            },
        });
    }

    info!(
        "Team {} {} casts Devotion Aura on ally",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Pending Holy Shock heal to be processed
#[derive(Component)]
pub struct HolyShockHealPending {
    pub caster_spell_power: f32,
    pub caster_team: u8,
    pub caster_class: CharacterClass,
    pub target: Entity,
}

/// Pending Holy Shock damage to be processed
#[derive(Component)]
pub struct HolyShockDamagePending {
    pub caster_spell_power: f32,
    pub caster_team: u8,
    pub caster_class: CharacterClass,
    pub target: Entity,
}
