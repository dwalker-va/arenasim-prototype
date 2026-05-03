//! Paladin AI Module
//!
//! Holy warrior and healer - combines healing with melee utility.
//!
//! ## Priority Order
//! 1. Paladin Aura (buff all allies pre-combat — Devotion/Shadow Resistance/Concentration)
//! 1.5. Divine Shield (emergency: self < 30% HP, or CC break for teammate)
//! 2. Cleanse - Urgent (Polymorph, Fear on allies)
//! 3. Emergency healing (ally < 40% HP) - Holy Shock (heal)
//! 4. Hammer of Justice (stun enemy in melee range)
//! 5. Standard healing (ally < 90% HP) - Flash of Light
//! 6. Holy Light (ally 50-85% HP, safe to cast long heal)
//! 7. Cleanse - Maintenance (roots, DoTs when team stable)
//! 8. Holy Shock (damage) - when team healthy
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::log::CombatLog;
use crate::states::match_config::{CharacterClass, PaladinAura};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRITICAL_HP_THRESHOLD, DIVINE_SHIELD_HP_THRESHOLD, GCD, HEALTHY_HP_THRESHOLD,
    HOLY_SHOCK_DAMAGE_RANGE, LOW_HP_THRESHOLD, SAFE_HEAL_MAX_THRESHOLD,
};
use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::cast_guard::{pre_cast_ok, PreCastOpts};

use super::{CombatContext, CombatantInfo};

/// Paladin AI: Decides and executes abilities for a Paladin combatant.
///
/// Returns `true` if an action was taken this frame.
pub fn decide_paladin_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    paladin_aura_this_frame: &mut std::collections::HashSet<Entity>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
) -> bool {
    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Paladin Aura (buff all allies — chosen via combatant.paladin_aura preference)
    if try_paladin_aura(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        paladin_aura_this_frame,
    ) {
        return true;
    }

    // Priority 1.5: Divine Shield (emergency defensive — self HP critical or CC break for teammate)
    if try_divine_shield(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        auras,
        ctx,
    ) {
        return true;
    }

    // Priority 2: Cleanse - Urgent (Polymorph, Fear on allies)
    if try_cleanse(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        90, // Only Polymorph (100) and Fear (90)
    ) {
        return true;
    }

    // Priority 3: Emergency healing - Holy Shock (heal) when ally < 40% HP
    if has_emergency_target(combatant.team, ctx.combatants) {
        if try_holy_shock_heal(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            ctx,
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
        ctx,
        same_frame_cc_queue,
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
        ctx,
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
        ctx,
    ) {
        return true;
    }

    // Priority 7: Cleanse - Maintenance (roots, DoTs when team stable)
    if ctx.is_team_healthy(HEALTHY_HP_THRESHOLD, my_pos) {
        if try_cleanse(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            50, // Include roots and DoTs
        ) {
            return true;
        }
    }

    // Priority 8: Holy Shock (damage) - when team healthy
    if ctx.is_team_healthy(HEALTHY_HP_THRESHOLD, my_pos) {
        if try_holy_shock_damage(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            ctx,
        ) {
            return true;
        }
    }

    false
}

/// Try to activate Divine Shield.
///
/// Trigger conditions (any of these):
/// 1. Survival: Self HP < 30%
/// 2. CC break for teammate: Self is incapacitated AND any teammate < 30% HP
/// 3. Heal under pressure: Self HP < 50% AND self is being focused
///
/// Guards: not already active, not on cooldown.
/// Note: This is also called from the incapacitation bypass path in combat_ai.rs.
pub fn try_divine_shield(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    auras: Option<&ActiveAuras>,
    _ctx: &CombatContext,
) -> bool {
    let def = abilities.get(&AbilityType::DivineShield);
    let def = match def {
        Some(d) => d,
        None => return false,
    };

    // Guard: on cooldown
    if combatant.ability_cooldowns.get(&AbilityType::DivineShield).copied().unwrap_or(0.0) > 0.0 {
        return false;
    }

    // Guard: already has DamageImmunity active
    if auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity)) {
        return false;
    }

    let self_hp_pct = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        1.0
    };

    // Condition 1: Survival — self HP below critical threshold
    let survival_trigger = self_hp_pct < DIVINE_SHIELD_HP_THRESHOLD;

    // Condition 2: Heal under pressure — self HP < 50% (being focused)
    let pressure_trigger = self_hp_pct < LOW_HP_THRESHOLD;

    if !survival_trigger && !pressure_trigger {
        return false;
    }

    // Activate Divine Shield
    let caster_id = combatant_id(combatant.team, combatant.class);
    info!("{} activates Divine Shield!", caster_id);

    // Spawn DivineShieldPending for deferred processing
    commands.spawn(DivineShieldPending {
        caster: entity,
        caster_team: combatant.team,
        caster_class: combatant.class,
    });

    // Trigger cooldown and GCD
    combatant.ability_cooldowns.insert(AbilityType::DivineShield, def.cooldown);
    combatant.global_cooldown = GCD;

    // Log the cast
    log_ability_use(combat_log, combatant.team, combatant.class, "Divine Shield", None, "casts");

    true
}

/// Try to use Divine Shield while incapacitated (CC break path).
///
/// Called from combat_ai.rs before the incapacitation gate.
/// Only triggers when self is CC'd AND a teammate is in critical danger.
pub fn try_divine_shield_while_cc(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let def = abilities.get(&AbilityType::DivineShield);
    let def = match def {
        Some(d) => d,
        None => return false,
    };

    // Guard: on cooldown
    if combatant.ability_cooldowns.get(&AbilityType::DivineShield).copied().unwrap_or(0.0) > 0.0 {
        return false;
    }

    // Guard: already has DamageImmunity active
    if auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity)) {
        return false;
    }

    // CC break trigger: any teammate (non-pet) below critical HP (they need healing NOW)
    let teammate_in_danger = ctx.combatants.values().any(|info| {
        info.team == combatant.team
            && info.current_health > 0.0
            && info.max_health > 0.0
            && !info.is_pet
            && (info.current_health / info.max_health) < DIVINE_SHIELD_HP_THRESHOLD
    });

    // Also trigger if self is in survival danger
    let self_hp_pct = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        1.0
    };
    let self_in_danger = self_hp_pct < DIVINE_SHIELD_HP_THRESHOLD;

    if !teammate_in_danger && !self_in_danger {
        return false;
    }

    // Activate Divine Shield (breaks CC via process_divine_shield debuff purge)
    let caster_id = combatant_id(combatant.team, combatant.class);
    info!("{} breaks CC with Divine Shield!", caster_id);

    commands.spawn(DivineShieldPending {
        caster: entity,
        caster_team: combatant.team,
        caster_class: combatant.class,
    });

    combatant.ability_cooldowns.insert(AbilityType::DivineShield, def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Divine Shield", None, "casts");

    true
}

/// Check if any ally is in an emergency situation (below critical HP threshold)
fn has_emergency_target(
    team: u8,
    combatant_info: &HashMap<Entity, CombatantInfo>,
) -> bool {
    combatant_info.values().any(|info| {
        info.team == team
            && !info.is_pet
            && info.current_health > 0.0
            && info.max_health > 0.0
            && (info.current_health / info.max_health) < CRITICAL_HP_THRESHOLD
    })
}

/// Try to cast Flash of Light on an injured ally.
fn try_flash_of_light(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::FlashOfLight;
    let def = abilities.get_unchecked(&ability);

    // Cheap fail-fast before scanning allies. Full preamble runs in `pre_cast_ok`.
    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find the lowest HP ally (below 90%), excluding pets, within range
    let Some(target_info) = ctx.lowest_health_ally_below(0.9, def.range, my_pos) else {
        return false;
    };
    let target_entity = &target_info.entity;
    let target_class = target_info.class;
    let target_pos = target_info.position;

    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((*target_entity, target_pos)),
        ctx,
        PreCastOpts::default(),
    ) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, *target_entity, cast_time));

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((combatant.team, target_class)), "begins casting");

    true
}

/// Try to cast Holy Light on an injured ally (prioritize if above 50% HP for safe slow heal)
fn try_holy_light(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::HolyLight;
    let def = abilities.get_unchecked(&ability);

    // Cheap fail-fast before scanning allies. Full preamble runs in `pre_cast_ok`.
    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find an ally between 50-85% HP (safe to use slow heal), excluding pets, within range
    let Some(target_info) = ctx.lowest_health_ally_below(SAFE_HEAL_MAX_THRESHOLD, def.range, my_pos) else {
        return false;
    };
    // Skip if target is critically low — Flash of Light or Holy Shock should handle that
    if target_info.health_pct() < LOW_HP_THRESHOLD {
        return false;
    }
    let target_entity = &target_info.entity;
    let target_class = target_info.class;
    let target_pos = target_info.position;

    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((*target_entity, target_pos)),
        ctx,
        PreCastOpts::default(),
    ) {
        return false;
    }

    // Start casting
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, *target_entity, cast_time));

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((combatant.team, target_class)), "begins casting");

    true
}

/// Try to cast Holy Shock as a heal on an emergency target (< 50% HP)
fn try_holy_shock_heal(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    // Range is enforced by `lowest_health_ally_below`, so target=None is safe;
    // pre_cast_ok handles school lockout, silence, cooldown, and mana.
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    ) {
        return false;
    }

    // Find lowest HP ally below 50% and in range, excluding pets
    let Some(target_info) = ctx.lowest_health_ally_below(LOW_HP_THRESHOLD, def.range, my_pos) else {
        return false;
    };
    let target_entity = &target_info.entity;
    let target_class = target_info.class;

    // Execute instant heal
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    log_ability_use(combat_log, combatant.team, combatant.class, "Holy Shock (Heal)", Some((combatant.team, target_class)), "casts");

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
fn try_holy_shock_damage(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    // Cheap fail-fast (mana + school + silence + cooldown). Target-side guards
    // (friendly-CC, immunity) run after target selection.
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    ) {
        return false;
    }

    // Find an enemy in range (20 yards for damage), filter out stealthed and immune
    let damage_target = ctx.combatants
        .iter()
        .filter(|(_, info)| {
            info.team != combatant.team && info.current_health > 0.0 && !info.stealthed
        })
        .filter(|(e, _)| !ctx.entity_is_immune(**e))
        .find_map(|(e, info)| {
            if my_pos.distance(info.position) <= HOLY_SHOCK_DAMAGE_RANGE {
                Some((e, info.position, info.class))
            } else {
                None
            }
        });

    let Some((target_entity, target_pos, target_class)) = damage_target else {
        return false;
    };

    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((*target_entity, target_pos)),
        ctx,
        PreCastOpts {
            check_friendly_cc: true,
            check_target_immune: true,
            ..Default::default()
        },
    ) {
        return false;
    }

    // Execute instant damage
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    // Log the cast
    let enemy_team = if combatant.team == 1 { 2 } else { 1 };
    log_ability_use(combat_log, combatant.team, combatant.class, "Holy Shock (Damage)", Some((enemy_team, target_class)), "casts");

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
fn try_hammer_of_justice(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
) -> bool {
    let ability = AbilityType::HammerOfJustice;
    let def = abilities.get_unchecked(&ability);

    // Universal preamble (school + silence + cooldown + mana). Per-target guards
    // (immunity, DR) are applied during target selection below.
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    ) {
        return false;
    }

    // Find enemies in range, filter out stealthed, immune, pets, and DR-immune to stuns
    let enemies_in_range: Vec<(&Entity, CharacterClass)> = ctx.combatants
        .iter()
        .filter(|(_, info)| {
            info.team != combatant.team && info.current_health > 0.0 && !info.stealthed && !info.is_pet
        })
        .filter(|(e, _)| !ctx.entity_is_immune(**e) && !ctx.is_dr_immune(**e, DRCategory::Stuns))
        .filter_map(|(e, info)| {
            if my_pos.distance(info.position) <= def.range {
                Some((e, info.class))
            } else {
                None
            }
        })
        .collect();

    // Prefer healers over DPS
    let stun_target = enemies_in_range
        .iter()
        .find(|(_, class)| class.is_healer())
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
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((enemy_team, target_class)), "casts");

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
        let hoj_aura = Aura {
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
            applied_this_frame: false,
            backlash_damage: None,
        };
        // Reflect same-frame so other class AIs running later this frame see the stun —
        // see `auras::reflect_instant_cc_in_snapshot` for details.
        same_frame_cc_queue.push((*target_entity, hoj_aura.clone()));
        commands.spawn(AuraPending {
            target: *target_entity,
            aura: hoj_aura,
        });
    }

    true
}

/// Try to cast Cleanse on an ally with a dispellable debuff.
///
/// Delegates to the shared `try_dispel_ally()` in `class_ai/mod.rs`.
fn try_cleanse(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
) -> bool {
    super::try_dispel_ally(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        min_priority,
        AbilityType::PaladinCleanse,
        "[CLEANSE]",
        "Cleanse",
        CharacterClass::Paladin,
    )
}

/// Try to apply the Paladin's chosen aura to all allies.
///
/// Dispatches based on `combatant.paladin_aura` preference:
/// - DevotionAura: DamageTakenReduction buff
/// - ShadowResistanceAura: SpellResistanceBuff
/// - ConcentrationAura: LockoutDurationReduction (shorter lockouts)
///
/// All three use the same team-wide application pattern (100yd range, passive, no mana cost).
fn try_paladin_aura(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    paladin_aura_this_frame: &mut std::collections::HashSet<Entity>,
) -> bool {
    // Determine which ability and aura type to use based on preference
    let (ability, aura_check_type, aura_name) = match combatant.paladin_aura {
        PaladinAura::DevotionAura => (
            AbilityType::DevotionAura,
            AuraType::DamageTakenReduction,
            "Devotion Aura",
        ),
        PaladinAura::ShadowResistanceAura => (
            AbilityType::ShadowResistanceAura,
            AuraType::SpellResistanceBuff,
            "Shadow Resistance Aura",
        ),
        PaladinAura::ConcentrationAura => (
            AbilityType::ConcentrationAura,
            AuraType::LockoutDurationReduction,
            "Concentration Aura",
        ),
    };

    let def = abilities.get_unchecked(&ability);

    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    ) {
        return false;
    }

    // Helper to check if an entity already has this aura active
    let has_aura = |e: &Entity| -> bool {
        ctx.active_auras
            .get(e)
            .map(|active| {
                active.iter().any(|a| {
                    a.effect_type == aura_check_type
                        && a.ability_name == aura_name
                })
            })
            .unwrap_or(false)
    };

    // Gather allies (exclude pets — auras are for primary combatants)
    let allies: Vec<(&Entity, CharacterClass)> = ctx.combatants
        .iter()
        .filter(|(_, info)| info.team == combatant.team && info.current_health > 0.0 && !info.is_pet)
        .map(|(e, info)| (e, info.class))
        .collect();

    // If ANY ally already has this aura (or was buffed this frame), skip
    if allies.iter().any(|(e, _)| has_aura(e) || paladin_aura_this_frame.contains(*e)) {
        return false;
    }

    // Find all allies in range who need the buff (exclude pets)
    let allies_to_buff: Vec<&Entity> = ctx.combatants
        .iter()
        .filter(|(_, info)| info.team == combatant.team && info.current_health > 0.0 && !info.is_pet)
        .filter_map(|(e, info)| {
            if my_pos.distance(info.position) <= def.range && !paladin_aura_this_frame.contains(e) {
                Some(e)
            } else {
                None
            }
        })
        .collect();

    if allies_to_buff.is_empty() {
        return false;
    }

    // Apply aura to ALL allies at once (matches WoW behavior)
    combatant.global_cooldown = GCD;

    // Log the cast once
    log_ability_use(combat_log, combatant.team, combatant.class, aura_name, None, "casts");

    // Apply the aura to each ally
    for ally_entity in allies_to_buff {
        paladin_aura_this_frame.insert(*ally_entity);
        if let Some(pending) = AuraPending::from_ability(*ally_entity, entity, def) {
            commands.spawn(pending);
        }
    }

    true
}
