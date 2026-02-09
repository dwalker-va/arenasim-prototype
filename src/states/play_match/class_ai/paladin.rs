//! Paladin AI Module
//!
//! Holy warrior and healer - combines healing with melee utility.
//!
//! ## Priority Order
//! 1. Devotion Aura (buff all allies pre-combat)
//! 2. Cleanse - Urgent (Polymorph, Fear on allies)
//! 3. Emergency healing (ally < 40% HP) - Holy Shock (heal)
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
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRITICAL_HP_THRESHOLD, GCD, HEALTHY_HP_THRESHOLD, HOLY_SHOCK_DAMAGE_RANGE,
    LOW_HP_THRESHOLD, SAFE_HEAL_MAX_THRESHOLD,
};
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::utils::combatant_id;

use super::priest::DispelPending;
use super::{dispel_priority, AbilityDecision, ClassAI, CombatContext};

/// Paladin AI implementation
pub struct PaladinAI;

impl ClassAI for PaladinAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // Uses decide_paladin_action() directly from combat_ai.rs
        AbilityDecision::None
    }
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
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
        entity,
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

    // Priority 3: Emergency healing - Holy Shock (heal) when ally < 40% HP
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

    // Priority 4: Hammer of Justice (stun enemy in melee range)
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

    // Priority 5: Standard healing - Flash of Light (ally < 90% HP)
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

    // Priority 6: Holy Light (ally damaged, safe to cast)
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

    // Priority 7: Cleanse - Maintenance (roots, DoTs when team stable)
    if allies_are_healthy(combatant.team, combatant_info) {
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

    // Priority 8: Holy Shock (damage) - when team healthy
    if allies_are_healthy(combatant.team, combatant_info) {
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

/// Check if any ally is in an emergency situation (below critical HP threshold)
fn has_emergency_target(
    team: u8,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
) -> bool {
    combatant_info.values().any(|(ally_team, _, _, hp, max_hp, _)| {
        *ally_team == team && *hp > 0.0 && *max_hp > 0.0 && (*hp / *max_hp) < CRITICAL_HP_THRESHOLD
    })
}

/// Check if all allies are healthy (above healthy HP threshold)
fn allies_are_healthy(
    team: u8,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
) -> bool {
    combatant_info
        .values()
        .filter(|(ally_team, _, _, hp, _, _)| *ally_team == team && *hp > 0.0)
        .all(|(_, _, _, hp, max_hp, _)| *max_hp > 0.0 && (*hp / *max_hp) >= HEALTHY_HP_THRESHOLD)
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
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
    let heal_target = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, max_hp, _))| {
            *team == combatant.team && *hp > 0.0 && *max_hp > 0.0 && (*hp / *max_hp) < 0.9
        })
        .filter_map(|(e, (_, _, class, hp, max_hp, _))| {
            positions.get(e).map(|pos| (e, class, *hp / *max_hp, pos))
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let Some((target_entity, target_class, _, target_pos)) = heal_target else {
        return false;
    };

    if !ability.can_cast_config(combatant, *target_pos, my_pos, def) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(*target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target_class.name());
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        Some(target_id),
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
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
    let heal_target = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, max_hp, _))| {
            if *team != combatant.team || *hp <= 0.0 || *max_hp <= 0.0 {
                return false;
            }
            let pct = *hp / *max_hp;
            pct >= LOW_HP_THRESHOLD && pct < SAFE_HEAL_MAX_THRESHOLD
        })
        .filter_map(|(e, (_, _, class, hp, max_hp, _))| {
            positions.get(e).map(|pos| (e, class, *hp / *max_hp, pos))
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let Some((target_entity, target_class, _, target_pos)) = heal_target else {
        return false;
    };

    if !ability.can_cast_config(combatant, *target_pos, my_pos, def) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(*target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target_class.name());
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        Some(target_id),
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check cooldown
    if combatant
        .ability_cooldowns
        .get(&ability)
        .copied()
        .unwrap_or(0.0)
        > 0.0
    {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find lowest HP ally below 50% and in range
    let heal_target = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, max_hp, _))| {
            *team == combatant.team && *hp > 0.0 && *max_hp > 0.0 && (*hp / *max_hp) < LOW_HP_THRESHOLD
        })
        .filter_map(|(e, (_, _, class, hp, max_hp, _))| {
            positions.get(e).and_then(|pos| {
                if my_pos.distance(*pos) <= def.range {
                    Some((e, class, *hp / *max_hp))
                } else {
                    None
                }
            })
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let Some((target_entity, target_class, _)) = heal_target else {
        return false;
    };

    // Execute instant heal
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target_class.name());
    combat_log.log_ability_cast(
        caster_id,
        "Holy Shock (Heal)".to_string(),
        Some(target_id.clone()),
        format!(
            "Team {} {} casts Holy Shock on {}",
            combatant.team,
            combatant.class.name(),
            target_id
        ),
    );

    // Spawn pending heal
    commands.spawn(HolyShockHealPending {
        caster_spell_power: combatant.spell_power,
        caster_crit_chance: combatant.crit_chance,
        caster_team: combatant.team,
        caster_class: combatant.class,
        target: *target_entity,
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check cooldown
    if combatant
        .ability_cooldowns
        .get(&ability)
        .copied()
        .unwrap_or(0.0)
        > 0.0
    {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find an enemy in range (20 yards for damage), filter out stealthed
    let damage_target = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, _, stealthed))| {
            *team != combatant.team && *hp > 0.0 && !*stealthed
        })
        .find_map(|(e, (_, _, class, _, _, _))| {
            positions.get(e).and_then(|pos| {
                if my_pos.distance(*pos) <= HOLY_SHOCK_DAMAGE_RANGE {
                    Some((e, class))
                } else {
                    None
                }
            })
        });

    let Some((target_entity, target_class)) = damage_target else {
        return false;
    };

    // Execute instant damage
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let enemy_team = if combatant.team == 1 { 2 } else { 1 };
    let target_id = format!("Team {} {}", enemy_team, target_class.name());
    combat_log.log_ability_cast(
        caster_id,
        "Holy Shock (Damage)".to_string(),
        Some(target_id.clone()),
        format!(
            "Team {} {} casts Holy Shock on {}",
            combatant.team,
            combatant.class.name(),
            target_id
        ),
    );

    // Spawn pending damage
    commands.spawn(HolyShockDamagePending {
        caster_spell_power: combatant.spell_power,
        caster_crit_chance: combatant.crit_chance,
        caster_team: combatant.team,
        caster_class: combatant.class,
        target: *target_entity,
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
) -> bool {
    let ability = AbilityType::HammerOfJustice;
    let def = abilities.get_unchecked(&ability);

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check cooldown
    if combatant
        .ability_cooldowns
        .get(&ability)
        .copied()
        .unwrap_or(0.0)
        > 0.0
    {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find enemies in range, filter out stealthed
    let enemies_in_range: Vec<(&Entity, &CharacterClass)> = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, _, stealthed))| {
            *team != combatant.team && *hp > 0.0 && !*stealthed
        })
        .filter_map(|(e, (_, _, class, _, _, _))| {
            positions.get(e).and_then(|pos| {
                if my_pos.distance(*pos) <= def.range {
                    Some((e, class))
                } else {
                    None
                }
            })
        })
        .collect();

    // Prefer healers over DPS
    let stun_target = enemies_in_range
        .iter()
        .find(|(_, class)| matches!(class, CharacterClass::Priest | CharacterClass::Paladin))
        .or_else(|| enemies_in_range.first())
        .copied();

    let Some((target_entity, target_class)) = stun_target else {
        return false;
    };

    // Execute the stun
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let enemy_team = if combatant.team == 1 { 2 } else { 1 };
    let target_id = format!("Team {} {}", enemy_team, target_class.name());
    combat_log.log_ability_cast(
        caster_id.clone(),
        def.name.to_string(),
        Some(target_id.clone()),
        format!(
            "Team {} {} casts Hammer of Justice on {}",
            combatant.team,
            combatant.class.name(),
            target_id
        ),
    );

    // Apply stun aura and log CC
    if let Some(aura_def) = def.applies_aura.as_ref() {
        // Log the CC application
        combat_log.log_crowd_control(
            caster_id,
            target_id.clone(),
            "Stun".to_string(),
            aura_def.duration,
            format!(
                "Team {} {}'s Hammer of Justice stuns {} ({:.1}s)",
                combatant.team,
                combatant.class.name(),
                target_id,
                aura_def.duration
            ),
        );
        commands.spawn(AuraPending {
            target: *target_entity,
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
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
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
    let mut best_candidate: Option<(&Entity, &CharacterClass, i32)> = None;

    for (e, (team, _, class, hp, _, _)) in combatant_info.iter() {
        // Must be alive ally
        if *team != combatant.team || *hp <= 0.0 {
            continue;
        }

        // Check range
        let Some(ally_pos) = positions.get(e) else {
            continue;
        };
        if my_pos.distance(*ally_pos) > def.range {
            continue;
        }

        // Check if ally has any dispellable debuffs
        let Some(ally_auras) = active_auras_map.get(e) else {
            continue;
        };

        // Find highest priority dispellable debuff on this ally
        let mut highest_priority = 0;
        for aura in ally_auras {
            if !aura.can_be_dispelled() {
                continue;
            }

            let priority = dispel_priority(aura.effect_type);

            if priority > highest_priority {
                highest_priority = priority;
            }
        }

        if highest_priority < min_priority {
            continue;
        }

        match best_candidate {
            None => best_candidate = Some((e, class, highest_priority)),
            Some((_, _, best_prio)) if highest_priority > best_prio => {
                best_candidate = Some((e, class, highest_priority));
            }
            _ => {}
        }
    }

    let Some((target_entity, target_class, _)) = best_candidate else {
        return false;
    };

    // Execute Cleanse
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target_class.name());
    combat_log.log_ability_cast(
        caster_id,
        "Cleanse".to_string(),
        Some(target_id.clone()),
        format!(
            "Team {} {} casts Cleanse on {}",
            combatant.team,
            combatant.class.name(),
            target_id
        ),
    );

    // Spawn pending dispel (uses same system as Priest's DispelMagic)
    commands.spawn(DispelPending {
        target: *target_entity,
        log_prefix: "[CLEANSE]",
        caster_class: CharacterClass::Paladin,
    });

    true
}

/// Try to cast Devotion Aura to buff all allies with damage reduction.
/// Buffs all allies in range at once (unlike per-GCD pre-combat buffs).
#[allow(clippy::too_many_arguments)]
fn try_devotion_aura(
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
) -> bool {
    let ability = AbilityType::DevotionAura;
    let def = abilities.get_unchecked(&ability);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check mana (for consistency, even though Devotion Aura costs 0)
    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Helper to check if an entity has Devotion Aura
    let has_devotion_aura = |e: &Entity| -> bool {
        active_auras_map
            .get(e)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageTakenReduction
                        && a.ability_name == "Devotion Aura"
                })
            })
            .unwrap_or(false)
    };

    // Gather allies
    let allies: Vec<(&Entity, &CharacterClass)> = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, _, _))| *team == combatant.team && *hp > 0.0)
        .map(|(e, (_, _, class, _, _, _))| (e, class))
        .collect();

    // If ANY ally already has Devotion Aura, we've already buffed the team
    if allies.iter().any(|(e, _)| has_devotion_aura(e)) {
        return false;
    }

    // Find all allies in range who need the buff
    let allies_to_buff: Vec<&Entity> = allies
        .iter()
        .filter_map(|(e, _)| {
            positions.get(*e).and_then(|pos| {
                if my_pos.distance(*pos) <= def.range {
                    Some(*e)
                } else {
                    None
                }
            })
        })
        .collect();

    if allies_to_buff.is_empty() {
        return false;
    }

    // Apply Devotion Aura to ALL allies at once (matches WoW behavior)
    combatant.global_cooldown = GCD;

    // Log the cast once
    let caster_id = combatant_id(combatant.team, combatant.class);
    combat_log.log_ability_cast(
        caster_id,
        "Devotion Aura".to_string(),
        None, // No single target - affects all allies
        format!(
            "Team {} {} casts Devotion Aura",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply the aura to each ally
    for ally_entity in allies_to_buff {
        if let Some(pending) = AuraPending::from_ability(*ally_entity, entity, def) {
            commands.spawn(pending);
        }
    }

    true
}
