//! Priest AI Module
//!
//! Handles AI decision-making for the Priest class.
//!
//! ## Priority Order
//! 1. Power Word: Fortitude (buff all allies pre-combat)
//! 2. Dispel Magic - Urgent (Polymorph, Fear - complete loss of control)
//! 3. Power Word: Shield (shield low-health allies)
//! 4. Flash Heal (heal injured allies)
//! 5. Dispel Magic - Maintenance (Roots, DoTs when team HP is stable)
//! 6. Mind Blast (damage when allies are healthy)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use std::collections::HashSet;

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, RejectionReason,
};
use crate::states::play_match::utils::log_ability_use;

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

use super::CombatContext;

/// Priest AI: Decides and executes abilities for a Priest combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_priest_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    shielded_this_frame: &mut HashSet<Entity>,
    fortified_this_frame: &mut HashSet<Entity>,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // GCD short-circuit — no event (emission gate).
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos) else {
        return false;
    };

    // Priority 1: Power Word: Fortitude (buff allies)
    if try_fortitude(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        fortified_this_frame, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 2: Dispel Magic - Urgent (Polymorph, Fear)
    if try_dispel_magic(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        90, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 3: Power Word: Shield
    if try_power_word_shield(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        shielded_this_frame, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 4: Flash Heal
    if try_flash_heal(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 5: Dispel Magic - Maintenance (only when team healthy)
    if ctx.is_team_healthy(0.70, my_pos) {
        if try_dispel_magic(
            commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
            50, &mut builder,
        ) {
            builder.finish();
            return true;
        }
    }

    // Priority 6: Mind Blast
    if try_mind_blast(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    builder.finish();
    false
}

/// Try to cast Power Word: Fortitude on an unbuffed ally.
fn try_fortitude(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    fortified_this_frame: &mut HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::PowerWordFortitude;
    let def = abilities.get_unchecked(&ability);

    let mut unbuffed_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }
        let has_fortitude = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxHealthIncrease))
            .unwrap_or(false);
        if has_fortitude {
            continue;
        }
        if fortified_this_frame.contains(ally_entity) {
            continue;
        }
        unbuffed_ally = Some((*ally_entity, info.position));
        break;
    }

    let Some((buff_target, target_pos)) = unbuffed_ally else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((buff_target, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((buff_target, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(buff_target), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&buff_target).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Power Word: Fortitude", target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(buff_target, entity, def) {
        commands.spawn(aura_pending);
    }

    fortified_this_frame.insert(buff_target);

    info!(
        "Team {} {} casts Power Word: Fortitude on ally",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Power Word: Shield on an ally.
fn try_power_word_shield(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    shielded_this_frame: &mut HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let pw_shield = AbilityType::PowerWordShield;
    let pw_shield_def = abilities.get_unchecked(&pw_shield);

    if combatant.current_mana < pw_shield_def.mana_cost {
        builder.reject(
            pw_shield,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: pw_shield_def.mana_cost,
            },
        );
        return false;
    }

    let mut best_candidate: Option<(Entity, Vec3, f32)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }
        let ally_auras = ctx.active_auras.get(ally_entity);
        let has_weakened_soul = ally_auras
            .map_or(false, |auras| auras.iter().any(|a| a.effect_type == AuraType::WeakenedSoul));
        let has_pw_shield = ally_auras.map_or(false, |auras| {
            auras
                .iter()
                .any(|a| a.effect_type == AuraType::Absorb && a.ability_name == "Power Word: Shield")
        });
        let shielded_this_frame_check = shielded_this_frame.contains(ally_entity);

        if has_weakened_soul || has_pw_shield || shielded_this_frame_check {
            continue;
        }

        let hp_percent = info.current_health / info.max_health;
        let is_full_hp = hp_percent >= 1.0;
        let is_below_threshold = hp_percent < 0.7;

        if is_full_hp || is_below_threshold {
            match best_candidate {
                None => best_candidate = Some((*ally_entity, info.position, hp_percent)),
                Some((_, _, best_percent)) if hp_percent < best_percent => {
                    best_candidate = Some((*ally_entity, info.position, hp_percent));
                }
                _ => {}
            }
        }
    }

    let Some((shield_entity, target_pos, _)) = best_candidate else {
        builder.reject(pw_shield, RejectionReason::NoValidTarget);
        return false;
    };

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        pw_shield, pw_shield_def, combatant, my_pos, auras,
        Some((shield_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            pw_shield,
            classify_pre_cast_failure(
                pw_shield, pw_shield_def, combatant, my_pos, auras,
                Some((shield_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(pw_shield, Some(shield_entity), true);

    combatant.current_mana -= pw_shield_def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&shield_entity).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Power Word: Shield", target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(shield_entity, entity, pw_shield_def) {
        commands.spawn(aura_pending);
    }

    commands.spawn(AuraPending {
        target: shield_entity,
        aura: Aura {
            effect_type: AuraType::WeakenedSoul,
            duration: 15.0,
            magnitude: 0.0,
            break_on_damage_threshold: -1.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(entity),
            ability_name: "Weakened Soul".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
            applied_this_frame: false,
            backlash_damage: None,
        },
    });

    shielded_this_frame.insert(shield_entity);

    true
}

/// Try to cast Dispel Magic on an ally with a dispellable debuff.
/// Delegates to the shared `try_dispel_ally()` in `class_ai/mod.rs`, which
/// emits its own reject/choose events to the builder.
fn try_dispel_magic(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
    builder: &mut DecisionEventBuilder<'_>,
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
        AbilityType::DispelMagic,
        "[DISPEL]",
        "Dispel Magic",
        CharacterClass::Priest,
        builder,
    )
}

/// Try to cast Flash Heal on the lowest HP ally.
fn try_flash_heal(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::FlashHeal;
    let def = abilities.get_unchecked(&ability);

    let Some(target_info) = ctx.lowest_health_ally_below(0.9, def.range, my_pos) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let heal_target = target_info.entity;
    let target_pos = target_info.position;

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((heal_target, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((heal_target, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(heal_target), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, heal_target, cast_time));

    let target_tuple = ctx.combatants
        .get(&heal_target)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on ally",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Mind Blast on the current target.
fn try_mind_blast(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::MindBlast;
    let def = abilities.get_unchecked(&ability);

    let Some(target_entity) = combatant.target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let target_pos = target_info.position;

    let opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}
