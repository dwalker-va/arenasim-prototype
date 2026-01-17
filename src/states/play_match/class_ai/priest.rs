//! Priest AI Module
//!
//! Handles AI decision-making for the Priest class.
//!
//! ## Priority Order
//! 1. Power Word: Fortitude (buff all allies pre-combat)
//! 2. Power Word: Shield (shield low-health allies)
//! 3. Flash Heal (heal injured allies)
//! 4. Mind Blast (damage when allies are healthy)

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::utils::combatant_id;

use super::{AbilityDecision, ClassAI, CombatContext};

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
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    shielded_this_frame: &mut HashSet<Entity>,
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
    ) {
        return true;
    }

    // Priority 2: Power Word: Shield (shield allies)
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

    // Priority 3: Flash Heal (heal injured allies)
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

    // Priority 4: Mind Blast (damage)
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
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Find an unbuffed ally
    let mut unbuffed_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, &(ally_team, _ally_class, ally_hp, _ally_max_hp)) in combatant_info.iter() {
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
    let target_id = combatant_info.get(&buff_target).map(|(team, class, _, _)| {
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
            },
        });
    }

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
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
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

    for (ally_entity, &(ally_team, _ally_class, ally_hp, ally_max_hp)) in combatant_info.iter() {
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

    if !pw_shield.can_cast(combatant, target_pos, my_pos) {
        return false;
    }

    // Execute the ability
    combatant.current_mana -= pw_shield_def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info.get(&shield_entity).map(|(team, class, _, _)| {
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
            },
        });
    }

    // Apply Weakened Soul debuff
    commands.spawn(AuraPending {
        target: shield_entity,
        aura: Aura {
            effect_type: AuraType::WeakenedSoul,
            duration: 15.0,
            magnitude: 0.0,
            break_on_damage_threshold: 0.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(entity),
            ability_name: "Weakened Soul".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
        },
    });

    // Mark target as shielded this frame
    shielded_this_frame.insert(shield_entity);

    true
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
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
) -> bool {
    // Find the lowest HP ally
    let mut lowest_hp_ally: Option<(Entity, f32, Vec3)> = None;

    for (ally_entity, &(ally_team, _ally_class, ally_hp, ally_max_hp)) in combatant_info.iter() {
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

    if !ability.can_cast(combatant, target_pos, my_pos) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: def.cast_time,
        target: Some(heal_target),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&heal_target)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
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
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
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

    if !ability.can_cast(combatant, target_pos, my_pos) {
        return false;
    }

    // Execute the ability
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: def.cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
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
