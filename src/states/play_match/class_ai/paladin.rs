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
use crate::states::play_match::constants::{GCD, HOLY_SHOCK_DAMAGE_RANGE};
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::utils::combatant_id;

use super::priest::DispelPending;
use super::{AbilityDecision, ClassAI, CombatContext};

/// Pre-computed ally information to avoid repeated iteration over combatant_info
struct AllyInfo {
    entity: Entity,
    class: CharacterClass,
    hp_percent: f32,
    pos: Vec3,
}

/// Pre-computed enemy information to avoid repeated iteration over combatant_info
struct EnemyInfo {
    entity: Entity,
    class: CharacterClass,
    pos: Vec3,
}

/// Check if any ally is in an emergency situation (< 40% HP)
fn has_emergency_target(allies: &[AllyInfo]) -> bool {
    allies.iter().any(|ally| ally.hp_percent < 0.40)
}

/// Check if all allies are healthy (> 70% HP)
fn allies_are_healthy(allies: &[AllyInfo]) -> bool {
    allies.iter().all(|ally| ally.hp_percent >= 0.70)
}

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

    // Pre-compute allies and enemies once to avoid repeated iteration
    let allies: Vec<AllyInfo> = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, max_hp, _))| *team == combatant.team && *hp > 0.0 && *max_hp > 0.0)
        .filter_map(|(e, (_, _, class, hp, max_hp, _))| {
            positions.get(e).map(|pos| AllyInfo {
                entity: *e,
                class: *class,
                hp_percent: *hp / *max_hp,
                pos: *pos,
            })
        })
        .collect();

    // Filter out stealthed enemies - can't target what we can't see
    let enemies: Vec<EnemyInfo> = combatant_info
        .iter()
        .filter(|(_, (team, _, _, hp, _, stealthed))| {
            *team != combatant.team && *hp > 0.0 && !*stealthed
        })
        .filter_map(|(e, (_, _, class, _, _, _))| {
            positions.get(e).map(|pos| EnemyInfo {
                entity: *e,
                class: *class,
                pos: *pos,
            })
        })
        .collect();

    // Priority 1: Devotion Aura (buff all allies pre-combat)
    if try_devotion_aura(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        &allies,
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
        &allies,
        active_auras_map,
        90, // Only Polymorph (100) and Fear (90)
    ) {
        return true;
    }

    // Priority 3: Emergency healing - Holy Shock (heal) when ally < 40% HP
    if has_emergency_target(&allies) {
        if try_holy_shock_heal(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            &allies,
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
        &enemies,
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
        &allies,
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
        &allies,
    ) {
        return true;
    }

    // Priority 7: Cleanse - Maintenance (roots, DoTs when team stable)
    if allies_are_healthy(&allies) {
        if try_cleanse(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            &allies,
            active_auras_map,
            50, // Include roots and DoTs
        ) {
            return true;
        }
    }

    // Priority 8: Holy Shock (damage) - when team healthy
    if allies_are_healthy(&allies) {
        if try_holy_shock_damage(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            &enemies,
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
    allies: &[AllyInfo],
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
    let heal_target = allies
        .iter()
        .filter(|ally| ally.hp_percent < 0.9)
        .min_by(|a, b| a.hp_percent.partial_cmp(&b.hp_percent).unwrap_or(std::cmp::Ordering::Equal));

    let Some(target) = heal_target else {
        return false;
    };

    if !ability.can_cast_config(combatant, target.pos, my_pos, def) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(target.entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target.class.name());
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
    allies: &[AllyInfo],
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
    // Too low = need fast heal, too high = waste mana
    let heal_target = allies
        .iter()
        .filter(|ally| ally.hp_percent >= 0.50 && ally.hp_percent < 0.85)
        .min_by(|a, b| a.hp_percent.partial_cmp(&b.hp_percent).unwrap_or(std::cmp::Ordering::Equal));

    let Some(target) = heal_target else {
        return false;
    };

    if !ability.can_cast_config(combatant, target.pos, my_pos, def) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(target.entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target.class.name());
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
    allies: &[AllyInfo],
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
    let heal_target = allies
        .iter()
        .filter(|ally| ally.hp_percent < 0.50 && my_pos.distance(ally.pos) <= def.range)
        .min_by(|a, b| a.hp_percent.partial_cmp(&b.hp_percent).unwrap_or(std::cmp::Ordering::Equal));

    let Some(target) = heal_target else {
        return false;
    };

    // Execute instant heal
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target.class.name());
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
        caster_team: combatant.team,
        caster_class: combatant.class,
        target: target.entity,
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
    enemies: &[EnemyInfo],
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

    // Find an enemy in range (20 yards for damage)
    let damage_target = enemies
        .iter()
        .find(|enemy| my_pos.distance(enemy.pos) <= HOLY_SHOCK_DAMAGE_RANGE);

    let Some(target) = damage_target else {
        return false;
    };

    // Execute instant damage
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let enemy_team = if combatant.team == 1 { 2 } else { 1 };
    let target_id = format!("Team {} {}", enemy_team, target.class.name());
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
        caster_team: combatant.team,
        caster_class: combatant.class,
        target: target.entity,
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
    enemies: &[EnemyInfo],
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

    // Find an enemy in range, prioritizing healers
    let enemies_in_range: Vec<&EnemyInfo> = enemies
        .iter()
        .filter(|enemy| my_pos.distance(enemy.pos) <= def.range)
        .collect();

    // Prefer healers over DPS
    let stun_target = enemies_in_range
        .iter()
        .find(|e| matches!(e.class, CharacterClass::Priest | CharacterClass::Paladin))
        .or_else(|| enemies_in_range.first())
        .copied();

    let Some(target) = stun_target else {
        return false;
    };

    // Execute the stun
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let caster_id = combatant_id(combatant.team, combatant.class);
    let enemy_team = if combatant.team == 1 { 2 } else { 1 };
    let target_id = format!("Team {} {}", enemy_team, target.class.name());
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
            target: target.entity,
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
    allies: &[AllyInfo],
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
    let mut best_candidate: Option<(&AllyInfo, i32)> = None;

    for ally in allies {
        // Check range
        if my_pos.distance(ally.pos) > def.range {
            continue;
        }

        // Check if ally has any dispellable debuffs
        let Some(ally_auras) = active_auras_map.get(&ally.entity) else {
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
            None => best_candidate = Some((ally, highest_priority)),
            Some((_, best_prio)) if highest_priority > best_prio => {
                best_candidate = Some((ally, highest_priority));
            }
            _ => {}
        }
    }

    let Some((target, _)) = best_candidate else {
        return false;
    };

    // Execute Cleanse
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = format!("Team {} {}", combatant.team, target.class.name());
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
        target: target.entity,
        log_prefix: "[CLEANSE]",
    });

    true
}

/// Try to cast Devotion Aura on an unbuffed ally.
///
/// **Design Note**: Unlike WoW Classic's toggle aura that instantly affects all party
/// members, this implementation applies the buff to one ally per GCD. This is an
/// intentional design choice for game balance - it creates a tactical window during
/// the pre-combat phase where the enemy team can engage before the Paladin's team
/// is fully buffed, adding strategic depth to match openings.
///
/// Similar to Priest's Power Word: Fortitude - buffs team pre-combat.
///
/// Returns true if the ability was used.
#[allow(clippy::too_many_arguments)]
fn try_devotion_aura(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    allies: &[AllyInfo],
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
    let has_devotion_aura = |entity: Entity| -> bool {
        active_auras_map
            .get(&entity)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageTakenReduction
                        && a.ability_name == "Devotion Aura"
                })
            })
            .unwrap_or(false)
    };

    // If ANY ally already has Devotion Aura, we've already buffed the team
    if allies.iter().any(|ally| has_devotion_aura(ally.entity)) {
        return false;
    }

    // Find all allies in range who need the buff
    let allies_to_buff: Vec<&AllyInfo> = allies
        .iter()
        .filter(|ally| my_pos.distance(ally.pos) <= def.range)
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
    for ally in allies_to_buff {
        if let Some(pending) = AuraPending::from_ability(ally.entity, entity, def) {
            commands.spawn(pending);
        }
    }

    true
}
