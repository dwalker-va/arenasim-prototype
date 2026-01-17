//! Warlock AI Module
//!
//! Handles AI decision-making for the Warlock class.
//!
//! ## Priority Order
//! 1. Corruption (DoT on enemies without it)
//! 2. Fear (CC on non-CC'd target)
//! 3. Shadow Bolt (main damage spell)

use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;
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

    // Priority 1: Corruption (instant DoT)
    if try_corruption(
        commands,
        combat_log,
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

    // Priority 2: Fear
    if try_fear(
        commands,
        combat_log,
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

    // Priority 3: Shadow Bolt
    try_shadowbolt(
        commands,
        combat_log,
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
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    active_auras_map: &HashMap<Entity, Vec<Aura>>,
) -> bool {
    // Check if target already has Corruption (any DoT for now)
    let target_has_corruption = active_auras_map
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime))
        .unwrap_or(false);

    if target_has_corruption {
        return false;
    }

    let corruption = AbilityType::Corruption;
    let corruption_def = corruption.definition();

    // Check if Shadow school is locked out
    if is_spell_school_locked(corruption_def.spell_school, auras) {
        return false;
    }

    if !corruption.can_cast(combatant, target_pos, my_pos) {
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
    if let Some((aura_type, duration, magnitude, break_threshold)) = corruption_def.applies_aura {
        commands.spawn(AuraPending {
            target: target_entity,
            aura: Aura {
                effect_type: aura_type,
                duration,
                magnitude,
                break_on_damage_threshold: break_threshold,
                accumulated_damage: 0.0,
                tick_interval: 3.0,
                time_until_next_tick: 3.0,
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

/// Try to cast Fear on target.
/// Returns true if Fear was started.
#[allow(clippy::too_many_arguments)]
fn try_fear(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
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
    let fear_def = fear.definition();
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

    if !fear.can_cast(combatant, target_pos, my_pos) {
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
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
) -> bool {
    let shadowbolt = AbilityType::Shadowbolt;
    let shadowbolt_def = shadowbolt.definition();

    // Check if Shadow school is locked out
    if is_spell_school_locked(shadowbolt_def.spell_school, auras) {
        return false;
    }

    if !shadowbolt.can_cast(combatant, target_pos, my_pos) {
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
