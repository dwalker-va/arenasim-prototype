//! Warlock AI Module
//!
//! Handles AI decision-making for the Warlock class.
//!
//! ## Priority Order
//! 1. Corruption (instant Shadow DoT)
//! 2. Spread curses to enemies (per-target preferences)
//! 3. Immolate (2s cast Fire DoT) - skipped when being kited
//! 4. Fear (CC on non-CC'd target)
//! 5. Drain Life (when HP < 80% and target has DoTs)
//! 6. Shadow Bolt (main damage spell) - skipped when being kited
//!
//! ## Kiting Detection
//! When being kited (slowed and out of range), the Warlock prioritizes instant-cast
//! abilities over cast-time spells that would be interrupted by movement.
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::match_config::WarlockCurse;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, CastingState, ChannelingState, Combatant,
};
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::is_spell_school_locked;

use super::{AbilityDecision, ClassAI, CombatContext};

/// Check if the Warlock is being kited (slowed and out of preferred range).
/// Returns true if the Warlock should prioritize instant-cast abilities.
fn is_being_kited(
    combatant: &Combatant,
    my_pos: Vec3,
    target_pos: Vec3,
    auras: Option<&ActiveAuras>,
) -> bool {
    // Check if we have a movement speed slow
    let is_slowed = auras
        .map(|a| a.auras.iter().any(|aura| aura.effect_type == AuraType::MovementSpeedSlow))
        .unwrap_or(false);

    // Check if target is beyond our preferred range (we'd need to move)
    let distance_to_target = my_pos.distance(target_pos);
    let preferred_range = combatant.class.preferred_range();
    let out_of_range = distance_to_target > preferred_range;

    // We're being kited if we're slowed AND out of range
    // This means we'll need to move to catch up, which would interrupt casts
    is_slowed && out_of_range
}

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
pub fn decide_warlock_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    // Get target
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        return false;
    };
    let target_pos = target_info.position;

    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Check if kill target is immune (Divine Shield) - skip damage abilities
    let target_immune = ctx.entity_is_immune(target_entity);

    // Detect if we're being kited (slowed and out of range)
    // When kited, prioritize instant-cast abilities over cast-time spells
    let being_kited = is_being_kited(combatant, my_pos, target_pos, auras);

    // Priority 1: Corruption (instant Shadow DoT) - skip if target immune
    if !target_immune {
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
            ctx,
        ) {
            return true;
        }
    }

    // Priority 2: Spread curses to all enemies (instant) - per-enemy immunity filtering inside
    if try_spread_curses(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
    ) {
        return true;
    }

    // Priority 3: Immolate (2s cast Fire DoT) - skip when being kited or target immune
    if !being_kited && !target_immune {
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
            ctx,
        ) {
            return true;
        }
    }

    // Priority 4: Fear (uses CC target if available, otherwise kill target)
    // CC target is separate from kill target to enable strategic CC on healers
    // while focusing damage on a different target
    // Fear is high value even with cast time - landing it can turn the fight
    let fear_target = combatant.cc_target.or(combatant.target);
    if let Some(fear_target_entity) = fear_target {
        if !ctx.entity_is_immune(fear_target_entity) {
            if let Some(fear_target_info) = ctx.combatants.get(&fear_target_entity) {
                let fear_target_pos = fear_target_info.position;
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
                    ctx,
                ) {
                    return true;
                }
            }
        }
    }

    // Priority 5: Drain Life (when HP < 80% and target has DoTs)
    // Skip when being kited or target immune - channeling would be interrupted by movement
    if !being_kited && !target_immune {
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
            ctx,
        ) {
            return true;
        }
    }

    // Priority 6: Shadow Bolt - skip when being kited or target immune
    // When kited or target immune, we'll rely on DoTs and wait for a better opportunity
    if being_kited || target_immune {
        return false;
    }

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
        ctx,
    )
}

/// Try to apply Corruption DoT to target.
/// Returns true if Corruption was cast.
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
    ctx: &CombatContext,
) -> bool {
    // Check if target already has Corruption (check by ability name to allow stacking with Immolate)
    let target_has_corruption = ctx.active_auras
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
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
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
                spell_school: Some(corruption_def.spell_school),
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
    ctx: &CombatContext,
) -> bool {
    // Check if target already has Immolate (check by ability name to allow stacking with Corruption)
    let target_has_immolate = ctx.active_auras
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

    // Execute Immolate (start casting - affected by Curse of Tongues)
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(immolate_def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability: immolate,
        time_remaining: cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
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
    ctx: &CombatContext,
) -> bool {
    let fear = AbilityType::Fear;
    let fear_def = abilities.get_unchecked(&fear);
    let fear_cooldown = combatant.ability_cooldowns.get(&fear).copied().unwrap_or(0.0);

    if fear_cooldown > 0.0 {
        return false;
    }

    // Check if target is already CC'd
    let target_is_ccd = ctx.active_auras
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

    // Execute Fear (start casting - affected by Curse of Tongues)
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(fear_def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability: fear,
        time_remaining: cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
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
    ctx: &CombatContext,
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

    // Execute Shadow Bolt (start casting - affected by Curse of Tongues)
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(shadowbolt_def.cast_time, auras);

    commands.entity(entity).insert(CastingState {
        ability: shadowbolt,
        time_remaining: cast_time,
        target: Some(target_entity),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
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
    ctx: &CombatContext,
) -> bool {
    // Only use Drain Life when we need healing (HP < 80%)
    let hp_percent = combatant.current_health / combatant.max_health;
    if hp_percent >= 0.8 {
        return false;
    }

    // Only use when target has at least one DoT ticking (maintain pressure)
    let target_has_dot = ctx.active_auras
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
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
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

/// Try to spread curses to all enemies based on per-target preferences.
/// Returns true if a curse was cast.
///
/// Curses are mutually exclusive per target - only one curse can be active.
/// Preferences are indexed by enemy slot (0, 1, 2).
fn try_spread_curses(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    // Skip if no curse preferences configured
    if combatant.warlock_curse_prefs.is_empty() {
        return false;
    }

    // Build list of enemy entities with their slot indices
    let mut enemies: Vec<(Entity, Vec3, u8)> = ctx.combatants
        .iter()
        .filter_map(|(&enemy_entity, info)| {
            // Only target alive enemies on opposite team
            if info.team != combatant.team && info.current_health > 0.0 {
                Some((enemy_entity, info.position, info.slot))
            } else {
                None
            }
        })
        .collect();

    // Sort by slot index to ensure consistent ordering that matches UI
    enemies.sort_by_key(|(_, _, slot)| *slot);

    // Try to curse each enemy based on preferences
    for (enemy_entity, enemy_pos, enemy_slot) in enemies {
        // Skip immune targets (Divine Shield)
        if ctx.entity_is_immune(enemy_entity) {
            continue;
        }

        // Get curse preference for this enemy slot (default to Agony if not specified)
        let curse_pref = combatant
            .warlock_curse_prefs
            .get(enemy_slot as usize)
            .copied()
            .unwrap_or(WarlockCurse::Agony);

        // Check if target already has a curse from us
        let has_our_curse = ctx.active_auras
            .get(&enemy_entity)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.caster == Some(entity)
                        && (a.ability_name == "Curse of Agony"
                            || a.ability_name == "Curse of Weakness"
                            || a.ability_name == "Curse of Tongues")
                })
            })
            .unwrap_or(false);

        if has_our_curse {
            continue;
        }

        // Try to cast the preferred curse
        let (ability, ability_name) = match curse_pref {
            WarlockCurse::Agony => (AbilityType::CurseOfAgony, "Curse of Agony"),
            WarlockCurse::Weakness => (AbilityType::CurseOfWeakness, "Curse of Weakness"),
            WarlockCurse::Tongues => (AbilityType::CurseOfTongues, "Curse of Tongues"),
        };

        if try_cast_curse(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            enemy_entity,
            enemy_pos,
            ctx,
            ability,
            ability_name,
        ) {
            return true;
        }
    }

    false
}

/// Cast a specific curse on a target.
/// Returns true if the curse was cast successfully.
fn try_cast_curse(
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
    ability: AbilityType,
    ability_name: &str,
) -> bool {
    let ability_def = abilities.get_unchecked(&ability);

    // Check if Shadow school is locked out
    if is_spell_school_locked(ability_def.spell_school, auras) {
        return false;
    }

    // Check if we can cast (range, mana, etc.)
    if !ability.can_cast_config(combatant, target_pos, my_pos, ability_def) {
        return false;
    }

    // Execute curse (all curses are instant)
    combatant.current_mana -= ability_def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = ctx.combatants
        .get(&target_entity)
        .map(|info| format!("Team {} {}", info.team, info.class.name()));
    combat_log.log_ability_cast(
        caster_id,
        ability_name.to_string(),
        target_id,
        format!(
            "Team {} {} casts {}",
            combatant.team,
            combatant.class.name(),
            ability_name
        ),
    );

    // Apply aura
    if let Some(aura_config) = ability_def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: target_entity,
            aura: Aura {
                effect_type: aura_config.aura_type,
                duration: aura_config.duration,
                magnitude: aura_config.magnitude,
                break_on_damage_threshold: aura_config.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: aura_config.tick_interval,
                time_until_next_tick: aura_config.tick_interval,
                caster: Some(entity),
                ability_name: ability_def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(ability_def.spell_school),
            },
        });
    }

    let effect_description = match ability {
        AbilityType::CurseOfAgony => "14 damage per 4s for 24s",
        AbilityType::CurseOfWeakness => "-20% physical damage for 2 min",
        AbilityType::CurseOfTongues => "+50% cast time for 30s",
        _ => "",
    };

    combat_log.log(
        CombatLogEventType::Buff,
        format!(
            "Team {} {} applies {} to enemy ({})",
            combatant.team,
            combatant.class.name(),
            ability_name,
            effect_description
        ),
    );

    info!(
        "Team {} {} applies {} to enemy ({})",
        combatant.team,
        combatant.class.name(),
        ability_name,
        effect_description
    );

    true
}
