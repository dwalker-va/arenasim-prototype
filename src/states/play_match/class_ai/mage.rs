//! Mage AI Module
//!
//! Handles AI decision-making for the Mage class.
//!
//! ## Priority Order
//! 1. Ice Barrier (self-shield when no shield or HP < 80%)
//! 2. Arcane Intellect (buff mana-using allies pre-combat)
//! 3. Frost Nova (defensive AoE when enemies in melee)
//! 4. Polymorph (CC non-kill target to create outnumbering situation)
//! 5. Frostbolt (main damage spell with kiting behavior)

use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRIT_DAMAGE_MULTIPLIER, DEFENSIVE_HP_THRESHOLD, GCD, MELEE_RANGE, SAFE_KITING_DISTANCE,
};
use crate::states::play_match::combat_core::{calculate_cast_time, roll_crit};
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::utils::{combatant_id, spawn_speech_bubble};

use super::{ClassAI, CombatContext, AbilityDecision};

/// Mage AI implementation.
///
/// Note: Currently uses direct execution via `decide_mage_action()`.
/// The trait implementation is a stub for future refactoring.
pub struct MageAI;

impl ClassAI for MageAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate to trait-based decision making
        // For now, use decide_mage_action() directly from combat_ai.rs
        AbilityDecision::None
    }
}

/// Mage AI: Decides and executes abilities for a Mage combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
#[allow(clippy::too_many_arguments)]
pub fn decide_mage_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
    frost_nova_damage: &mut Vec<super::QueuedAoeDamage>,
) -> bool {
    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Ice Barrier (self-shield)
    if try_ice_barrier(commands, combat_log, abilities, entity, combatant, active_auras_map) {
        return true;
    }

    // Priority 2: Arcane Intellect (buff mana-using allies)
    if try_arcane_intellect(
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

    // Priority 3: Frost Nova (defensive AoE)
    if try_frost_nova(
        commands,
        combat_log,
        game_rng,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        positions,
        combatant_info,
        frost_nova_damage,
    ) {
        return true;
    }

    // Priority 4: Polymorph (CC non-kill target)
    if try_polymorph(
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

    // Priority 5: Frostbolt (main damage spell)
    if try_frostbolt(
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

/// Try to cast Ice Barrier on self.
/// Returns true if the ability was used.
fn try_ice_barrier(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Check if already shielded
    let has_absorb_shield = active_auras_map
        .get(&entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::Absorb))
        .unwrap_or(false);

    // Pre-combat (full HP): Always cast
    // In-combat: Only recast when HP < threshold
    let is_full_hp = combatant.current_health >= combatant.max_health;
    let is_below_threshold =
        combatant.current_health < combatant.max_health * DEFENSIVE_HP_THRESHOLD;
    let should_shield = !has_absorb_shield && (is_full_hp || is_below_threshold);

    if !should_shield {
        return false;
    }

    let ice_barrier = AbilityType::IceBarrier;
    let barrier_def = abilities.get_unchecked(&ice_barrier);
    let barrier_on_cooldown = combatant.ability_cooldowns.contains_key(&ice_barrier);

    if barrier_on_cooldown || combatant.current_mana < barrier_def.mana_cost {
        return false;
    }

    // Execute the ability
    spawn_speech_bubble(commands, entity, "Ice Barrier");
    combatant.current_mana -= barrier_def.mana_cost;
    combatant.ability_cooldowns.insert(ice_barrier, barrier_def.cooldown);
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    combat_log.log_ability_cast(
        caster_id,
        "Ice Barrier".to_string(),
        None,
        format!(
            "Team {} {} casts Ice Barrier",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply absorb shield aura
    let aura = barrier_def.applies_aura.as_ref().unwrap();
    commands.spawn(AuraPending {
        target: entity,
        aura: Aura {
            effect_type: aura.aura_type,
            duration: aura.duration,
            magnitude: aura.magnitude,
            break_on_damage_threshold: 0.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(entity),
            ability_name: "Ice Barrier".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: Some(barrier_def.spell_school),
        },
    });

    info!(
        "Team {} {} casts Ice Barrier",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Arcane Intellect on an unbuffed mana-using ally.
/// Returns true if the ability was used.
#[allow(clippy::too_many_arguments)]
fn try_arcane_intellect(
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
    // Find an unbuffed mana-using ally
    let mut unbuffed_mana_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, &(ally_team, _, ally_class, ally_hp, _ally_max_hp, _)) in combatant_info.iter() {
        // Must be same team, alive, and use mana
        if ally_team != combatant.team || ally_hp <= 0.0 {
            continue;
        }

        // Only buff mana users (Mage, Priest, Warlock)
        let uses_mana = matches!(
            ally_class,
            CharacterClass::Mage | CharacterClass::Priest | CharacterClass::Warlock
        );
        if !uses_mana {
            continue;
        }

        // Check if ally already has MaxManaIncrease buff
        let has_arcane_intellect = active_auras_map
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxManaIncrease))
            .unwrap_or(false);

        if has_arcane_intellect {
            continue;
        }

        // Get position
        let Some(&ally_pos) = positions.get(ally_entity) else {
            continue;
        };

        unbuffed_mana_ally = Some((*ally_entity, ally_pos));
        break;
    }

    let Some((buff_target, target_pos)) = unbuffed_mana_ally else {
        return false;
    };

    let ability = AbilityType::ArcaneIntellect;
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
        "Arcane Intellect".to_string(),
        target_id,
        format!(
            "Team {} {} casts Arcane Intellect",
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

    info!(
        "Team {} {} casts Arcane Intellect on ally",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Frost Nova when enemies are in melee range.
/// Returns true if the ability was used.
#[allow(clippy::too_many_arguments)]
fn try_frost_nova(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, u8, CharacterClass, f32, f32, bool)>,
    frost_nova_damage: &mut Vec<super::QueuedAoeDamage>,
) -> bool {
    let frost_nova = AbilityType::FrostNova;
    let nova_def = abilities.get_unchecked(&frost_nova);
    let nova_on_cooldown = combatant.ability_cooldowns.contains_key(&frost_nova);

    if nova_on_cooldown {
        return false;
    }

    // Check if Frost school is locked out
    if is_spell_school_locked(nova_def.spell_school, auras) {
        return false;
    }

    if combatant.current_mana < nova_def.mana_cost {
        return false;
    }

    // Check if any enemies are within melee range
    let enemies_in_melee_range = positions.iter().any(|(enemy_entity, &enemy_pos)| {
        if let Some(&(enemy_team, _, _, _, _, _)) = combatant_info.get(enemy_entity) {
            if enemy_team != combatant.team {
                return my_pos.distance(enemy_pos) <= MELEE_RANGE;
            }
        }
        false
    });

    if !enemies_in_melee_range {
        return false;
    }

    // Execute the ability
    spawn_speech_bubble(commands, entity, "Frost Nova");
    combatant.current_mana -= nova_def.mana_cost;
    combatant.ability_cooldowns.insert(frost_nova, nova_def.cooldown);
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    combat_log.log_ability_cast(
        caster_id,
        "Frost Nova".to_string(),
        None,
        format!(
            "Team {} {} casts Frost Nova",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Collect enemies in range for damage and root
    let mut frost_nova_targets: Vec<(Entity, Vec3, u8, CharacterClass)> = Vec::new();
    for (enemy_entity, &enemy_pos) in positions.iter() {
        if let Some(&(enemy_team, _, enemy_class, _, _, _)) = combatant_info.get(enemy_entity) {
            if enemy_team != combatant.team {
                let distance = my_pos.distance(enemy_pos);
                if distance <= nova_def.range {
                    frost_nova_targets.push((*enemy_entity, enemy_pos, enemy_team, enemy_class));
                }
            }
        }
    }

    // Queue damage and apply root to all targets
    for (target_entity, target_pos, _target_team, _target_class) in &frost_nova_targets {
        let mut damage = combatant.calculate_ability_damage_config(nova_def, game_rng);
        let is_crit = roll_crit(combatant.crit_chance, game_rng);
        if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
        frost_nova_damage.push(super::QueuedAoeDamage {
            caster: entity,
            target: *target_entity,
            damage,
            caster_team: combatant.team,
            caster_class: combatant.class,
            target_pos: *target_pos,
            is_crit,
        });

        // Apply root aura
        if let Some(aura) = nova_def.applies_aura.as_ref() {
            commands.spawn(AuraPending {
                target: *target_entity,
                aura: Aura {
                    effect_type: aura.aura_type,
                    duration: aura.duration,
                    magnitude: aura.magnitude,
                    break_on_damage_threshold: aura.break_on_damage,
                    accumulated_damage: 0.0,
                    tick_interval: 0.0,
                    time_until_next_tick: 0.0,
                    caster: Some(entity),
                    ability_name: nova_def.name.to_string(),
                    fear_direction: (0.0, 0.0),
                    fear_direction_timer: 0.0,
                    spell_school: Some(nova_def.spell_school),
                },
            });

            // Log CC application for Frost Nova root
            let message = format!(
                "Team {} {}'s {} roots Team {} {} ({:.1}s)",
                combatant.team,
                combatant.class.name(),
                nova_def.name,
                _target_team,
                _target_class.name(),
                aura.duration
            );
            combat_log.log_crowd_control(
                combatant_id(combatant.team, combatant.class),
                combatant_id(*_target_team, *_target_class),
                "Root".to_string(),
                aura.duration,
                message,
            );
        }
    }

    // Set kiting timer
    combatant.kiting_timer = nova_def.applies_aura.as_ref().unwrap().duration;

    info!(
        "Team {} {} casts Frost Nova! (AOE root) - {} enemies affected",
        combatant.team,
        combatant.class.name(),
        frost_nova_targets.len()
    );

    true
}

/// Try to cast Polymorph on the CC target (non-kill target).
///
/// Polymorph is a long-duration CC that breaks on ANY damage, so it should only
/// be used on targets that won't take damage (the cc_target, not kill_target).
///
/// Returns true if casting was started.
#[allow(clippy::too_many_arguments)]
fn try_polymorph(
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
    // Polymorph targets the cc_target, NOT the kill target
    let Some(cc_target) = combatant.cc_target else {
        return false;
    };

    // Don't polymorph the kill target - any damage will break it immediately
    if combatant.target == Some(cc_target) {
        return false;
    }

    let Some(&target_pos) = positions.get(&cc_target) else {
        return false;
    };

    // Check if target is already CC'd (don't waste Polymorph on already CC'd targets)
    let target_already_ccd = active_auras_map
        .get(&cc_target)
        .map(|auras| {
            auras.iter().any(|a| {
                matches!(
                    a.effect_type,
                    AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph
                )
            })
        })
        .unwrap_or(false);

    if target_already_ccd {
        return false;
    }

    // Check GCD
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let ability = AbilityType::Polymorph;
    let def = abilities.get_unchecked(&ability);

    // Check if Arcane spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check range and mana
    let distance_to_target = my_pos.distance(target_pos);
    if distance_to_target > def.range || combatant.current_mana < def.mana_cost {
        return false;
    }

    // Start casting Polymorph (affected by Curse of Tongues)
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability,
        time_remaining: cast_time,
        target: Some(cc_target),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = combatant_id(combatant.team, combatant.class);
    let target_id = combatant_info
        .get(&cc_target)
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
        "Team {} {} starts casting {} on cc_target",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Frostbolt on the current target.
/// Returns true if casting was started.
#[allow(clippy::too_many_arguments)]
fn try_frostbolt(
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

    let distance_to_target = my_pos.distance(target_pos);

    // While kiting, only cast if at safe distance
    if combatant.kiting_timer > 0.0 && distance_to_target < SAFE_KITING_DISTANCE {
        return false;
    }

    // Check GCD (redundant but safe)
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let ability = AbilityType::Frostbolt;
    let def = abilities.get_unchecked(&ability);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    // Check range and mana
    if distance_to_target > def.range || combatant.current_mana < def.mana_cost {
        return false;
    }

    // Start casting (affected by Curse of Tongues)
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
