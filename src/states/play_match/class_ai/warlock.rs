//! Warlock AI Module
//!
//! Handles AI decision-making for the Warlock class.
//!
//! ## Priority Order
//! 1. Corruption (instant Shadow DoT)
//! 2. Immolate (2s cast Fire DoT)
//! 3. Fear (CC on non-CC'd target)
//! 4. Drain Life (when HP < 80% and target has DoTs)
//! 5. Shadow Bolt (main damage spell)

use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, CastingState, ChannelingState, Combatant,
};
use crate::states::play_match::constants::GCD;
use crate::states::play_match::is_spell_school_locked;

use super::{AbilityDecision, ClassAI, CombatContext};

/// Warlock AI implementation.
///
/// Note: Currently uses direct execution via `decide_warlock_action()`.
/// The trait implementation is a stub for future refactoring.
pub struct WarlockAI;

impl ClassAI for WarlockAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate to trait-based decision making
        // For now, use decide_warlock_action() directly from combat_ai.rs
        AbilityDecision::None
    }
}

/// Warlock AI: Decides and executes abilities for a Warlock combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
#[allow(clippy::too_many_arguments)]
pub fn decide_warlock_action(
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
    // Get target
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(&target_pos) = positions.get(&target_entity) else {
        return false;
    };

    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Corruption (instant Shadow DoT)
    if try_corruption(
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
        active_auras_map,
    ) {
        return true;
    }

    // Priority 2: Immolate (2s cast Fire DoT)
    if try_immolate(
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
        active_auras_map,
    ) {
        return true;
    }

    // Priority 3: Fear (uses CC target if available, otherwise kill target)
    // CC target is separate from kill target to enable strategic CC on healers
    // while focusing damage on a different target
    let fear_target = combatant.cc_target.or(combatant.target);
    if let Some(fear_target_entity) = fear_target {
        if let Some(&fear_target_pos) = positions.get(&fear_target_entity) {
            if try_fear(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                fear_target_entity,
                fear_target_pos,
                combatant_info,
                active_auras_map,
            ) {
                return true;
            }
        }
    }

    // Priority 4: Drain Life (when HP < 80% and target has DoTs)
    if try_drain_life(
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
        active_auras_map,
    ) {
        return true;
    }

    // Priority 5: Shadow Bolt
    try_shadowbolt(
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
    )
}

/// Try to apply Corruption DoT to target.
/// Returns true if Corruption was cast.
#[allow(clippy::too_many_arguments)]
fn try_corruption(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Check if target already has Corruption (check by ability name to allow stacking with Immolate)
    let target_has_corruption = active_auras_map
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a|
            a.effect_type == AuraType::DamageOverTime && a.ability_name == "Corruption"
        ))
        .unwrap_or(false);

    if target_has_corruption {
        return false;
    }

    let corruption = AbilityType::Corruption;
    let corruption_def = abilities.get_unchecked(&corruption);

    // Check if Shadow school is locked out
    if is_spell_school_locked(corruption_def.spell_school, auras) {
        return false;
    }

    if !corruption.can_cast_config(combatant, target_pos, my_pos, corruption_def) {
        return false;
    }

    // Execute Corruption
    combatant.current_mana -= corruption_def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Corruption".to_string(),
        target_id,
        format!(
            "Team {} {} casts Corruption",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply DoT aura
    if let Some(aura) = corruption_def.applies_aura.as_ref() {
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
                ability_name: corruption_def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
            },
        });
    }

    combat_log.log(
        CombatLogEventType::Buff,
        format!(
            "Team {} {} applies Corruption to enemy (10 damage per 3s for 18s)",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} applies Corruption to enemy (10 damage per 3s for 18s)",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Immolate on target.
/// Returns true if Immolate cast was started.
#[allow(clippy::too_many_arguments)]
fn try_immolate(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Check if target already has Immolate (check by ability name to allow stacking with Corruption)
    let target_has_immolate = active_auras_map
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a|
            a.effect_type == AuraType::DamageOverTime && a.ability_name == "Immolate"
        ))
        .unwrap_or(false);

    if target_has_immolate {
        return false;
    }

    let immolate = AbilityType::Immolate;
    let immolate_def = abilities.get_unchecked(&immolate);

    // Check if Fire school is locked out
    if is_spell_school_locked(immolate_def.spell_school, auras) {
        return false;
    }

    if !immolate.can_cast_config(combatant, target_pos, my_pos, immolate_def) {
        return false;
    }

    // Execute Immolate (start casting - 2s cast time)
    combatant.global_cooldown = GCD;

    commands.entity(entity).insert(CastingState {
        ability: immolate,
        time_remaining: immolate_def.cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Immolate".to_string(),
        target_id,
        format!(
            "Team {} {} begins casting Immolate",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} starts casting Immolate on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Fear on target.
/// Returns true if Fear was started.
#[allow(clippy::too_many_arguments)]
fn try_fear(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    let fear = AbilityType::Fear;
    let fear_def = abilities.get_unchecked(&fear);
    let fear_cooldown = combatant.ability_cooldowns.get(&fear).copied().unwrap_or(0.0);

    if fear_cooldown > 0.0 {
        return false;
    }

    // Check if target is already CC'd
    let target_is_ccd = active_auras_map
        .get(&target_entity)
        .map(|auras| {
            auras
                .iter()
                .any(|a| matches!(a.effect_type, AuraType::Stun | AuraType::Fear | AuraType::Root))
        })
        .unwrap_or(false);

    if target_is_ccd {
        return false;
    }

    // Check if Shadow school is locked out
    if is_spell_school_locked(fear_def.spell_school, auras) {
        return false;
    }

    if !fear.can_cast_config(combatant, target_pos, my_pos, fear_def) {
        return false;
    }

    // Execute Fear (start casting)
    combatant.global_cooldown = GCD;

    commands.entity(entity).insert(CastingState {
        ability: fear,
        time_remaining: fear_def.cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Fear".to_string(),
        target_id,
        format!(
            "Team {} {} begins casting Fear",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} starts casting Fear on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Shadow Bolt on target.
/// Returns true if Shadow Bolt was started.
#[allow(clippy::too_many_arguments)]
fn try_shadowbolt(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
) -> bool {
    let shadowbolt = AbilityType::Shadowbolt;
    let shadowbolt_def = abilities.get_unchecked(&shadowbolt);

    // Check if Shadow school is locked out
    if is_spell_school_locked(shadowbolt_def.spell_school, auras) {
        return false;
    }

    if !shadowbolt.can_cast_config(combatant, target_pos, my_pos, shadowbolt_def) {
        return false;
    }

    // Execute Shadow Bolt (start casting)
    combatant.global_cooldown = GCD;

    commands.entity(entity).insert(CastingState {
        ability: shadowbolt,
        time_remaining: shadowbolt_def.cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Shadowbolt".to_string(),
        target_id,
        format!(
            "Team {} {} begins casting Shadowbolt",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        shadowbolt_def.name
    );

    true
}

/// Try to channel Drain Life on target.
/// Only used when HP < 80% and target has at least one DoT ticking.
/// Returns true if Drain Life was started.
#[allow(clippy::too_many_arguments)]
fn try_drain_life(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Only use Drain Life when we need healing (HP < 80%)
    let hp_percent = combatant.current_health / combatant.max_health;
    if hp_percent >= 0.8 {
        return false;
    }

    // Only use when target has at least one DoT ticking (maintain pressure)
    let target_has_dot = active_auras_map
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime))
        .unwrap_or(false);

    if !target_has_dot {
        return false;
    }

    let drain_life = AbilityType::DrainLife;
    let drain_life_def = abilities.get_unchecked(&drain_life);

    // Check if Shadow school is locked out
    if is_spell_school_locked(drain_life_def.spell_school, auras) {
        return false;
    }

    if !drain_life.can_cast_config(combatant, target_pos, my_pos, drain_life_def) {
        return false;
    }

    // Execute Drain Life (start channeling)
    combatant.current_mana -= drain_life_def.mana_cost;
    combatant.global_cooldown = GCD;

    // Get channel parameters
    let channel_duration = drain_life_def.channel_duration.unwrap_or(5.0);
    let tick_interval = drain_life_def.channel_tick_interval;

    commands.entity(entity).insert(ChannelingState {
        ability: drain_life,
        duration_remaining: channel_duration,
        time_until_next_tick: tick_interval,
        tick_interval,
        target: target_entity,
        interrupted: false,
        interrupted_display_time: 0.0,
        ticks_applied: 0,
    });

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Drain Life".to_string(),
        target_id,
        format!(
            "Team {} {} begins channeling Drain Life",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} starts channeling Drain Life on enemy (HP: {:.0}%)",
        combatant.team,
        combatant.class.name(),
        hp_percent * 100.0
    );

    true
}
