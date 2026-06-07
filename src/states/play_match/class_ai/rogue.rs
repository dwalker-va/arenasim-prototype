//! Rogue AI Module
//!
//! Handles AI decision-making for the Rogue class.
//!
//! ## Priority Order (Stealthed)
//! 1. Ambush (opener from stealth)
//!
//! ## Priority Order (In Combat)
//! 1. Kidney Shot (stun)
//! 2. Sinister Strike (combo point builder)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::match_config::RogueOpener;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::{roll_crit, get_attack_power_bonus_from_slice, get_crit_chance_bonus_from_slice};
use crate::states::play_match::constants::{CRIT_DAMAGE_MULTIPLIER, GCD, MELEE_RANGE};
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, NoActionReason, RejectionReason,
};
use crate::states::play_match::utils::{combatant_id, log_ability_use, spawn_speech_bubble};

use super::CombatContext;
use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

/// Rogue AI: Decides and executes abilities for a Rogue combatant.
pub fn decide_rogue_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // No target — no decision is produced. (Note: unlike most classes, Rogue
    // does NOT short-circuit on GCD up front because the stealthed opener path
    // is independent of GCD. The non-stealthed branch checks GCD itself.)
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_pos) = ctx.combatants.get(&target_entity).map(|info| info.position) else {
        return false;
    };

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, Some(target_entity), my_pos) else {
        return false;
    };

    // Don't waste abilities on immune targets (Divine Shield).
    if ctx.entity_is_immune(target_entity) {
        builder.finish_no_action(NoActionReason::TargetImmune);
        return false;
    }

    if combatant.stealthed {
        let acted = match combatant.rogue_opener {
            RogueOpener::Ambush => try_ambush(
                combat_log, game_rng, abilities, entity, combatant, my_pos,
                target_entity, target_pos, ctx, instant_attacks, &mut builder,
            ),
            RogueOpener::CheapShot => try_cheap_shot(
                commands, combat_log, abilities, entity, combatant, my_pos,
                target_entity, target_pos, ctx, same_frame_cc_queue, &mut builder,
            ),
        };
        builder.finish();
        return acted;
    }

    // Not stealthed: defer to GCD check before considering abilities.
    if combatant.global_cooldown > 0.0 {
        // Don't emit — no decision produced. Drop the builder (no candidates).
        return false;
    }

    // Priority 1: Kidney Shot (melee-range CC)
    let kidney_shot = AbilityType::KidneyShot;
    let kidney_shot_target = select_melee_cc_target(
        combatant.cc_target,
        combatant.target,
        my_pos,
        ctx,
    );
    if let Some((ks_target_entity, ks_target_pos)) = kidney_shot_target {
        let target_already_stunned = ctx.active_auras
            .get(&ks_target_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::Stun))
            .unwrap_or(false);

        if target_already_stunned {
            builder.reject(
                kidney_shot,
                RejectionReason::TargetAlreadyCCd { cc_type: AuraType::Stun },
            );
        } else if ctx.is_dr_immune(ks_target_entity, DRCategory::Stuns) {
            builder.reject(
                kidney_shot,
                RejectionReason::DRImmune { category: DRCategory::Stuns },
            );
        } else if try_kidney_shot(
            commands, combat_log, abilities, entity, combatant, my_pos,
            ks_target_entity, ks_target_pos, ctx, same_frame_cc_queue, &mut builder,
        ) {
            builder.finish();
            return true;
        } else {
            // ENERGY POOLING: when the ONLY thing stopping Kidney Shot is
            // energy, do not burn energy on Sinister Strike this tick — hold
            // until Kidney Shot (60) is affordable. Without this gate, SS
            // (40) re-drains the pool every tick and energy oscillates in
            // the 40-59 band, so the stun NEVER fires. Pre-U4.1 this worked
            // by accident: a target invisible mid-cast made the Rogue skip
            // whole decision ticks, pooling energy unintentionally; the
            // snapshot casting-visibility fix removed those idle ticks and
            // Kidney Shot usage collapsed (86/100 -> 0/100 vs Priest).
            //
            // Classifier-order guarantees make this safe: cooldown is
            // classified before resource, so InsufficientResource implies
            // the CD is ready; resource precedes range, but Kidney Shot and
            // SS share MELEE_RANGE, so suppressing SS while out of range
            // costs nothing. Energy regen ticks passively, so pooling
            // always terminates.
            let ks_def = abilities.get_unchecked(&kidney_shot);
            let reason = classify_pre_cast_failure(
                kidney_shot, ks_def, combatant, my_pos, None,
                Some((ks_target_entity, ks_target_pos)), ctx,
                PreCastOpts::default(),
            );
            if matches!(reason, RejectionReason::InsufficientResource { .. }) {
                builder.reject(
                    AbilityType::SinisterStrike,
                    RejectionReason::PreconditionUnmet {
                        note: "pooling energy for Kidney Shot".into(),
                    },
                );
                builder.finish();
                return false;
            }
        }
    } else {
        builder.reject(kidney_shot, RejectionReason::NoValidTarget);
    }

    // Priority 2: Sinister Strike
    let acted = try_sinister_strike(
        combat_log, game_rng, abilities, entity, combatant, my_pos,
        target_entity, target_pos, ctx, instant_attacks, &mut builder,
    );
    builder.finish();
    acted
}

/// Try to use Ambush from stealth.
fn try_ambush(
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Ambush;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, None,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, None,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.stealthed = false;
    combatant.global_cooldown = GCD;

    let self_auras = ctx.active_auras.get(&entity).map(|v| v.as_slice()).unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, ap_bonus);
    let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
    if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Ambush", target_tuple, "uses");

    info!(
        "Team {} {} uses {} from stealth!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Cheap Shot from stealth.
fn try_cheap_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::CheapShot;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability, def, combatant, my_pos, None,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, None,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    spawn_speech_bubble(commands, entity, "Cheap Shot");
    combatant.current_mana -= def.mana_cost;
    combatant.stealthed = false;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Cheap Shot", target_tuple, "uses");

    if let Some(aura) = def.applies_aura.as_ref() {
        if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, def) {
            same_frame_cc_queue.push((target_entity, aura_pending.aura.clone()));
            commands.spawn(aura_pending);
        }

        if let Some(info) = ctx.combatants.get(&target_entity) {
            let cc_type = format!("{:?}", aura.aura_type);
            let message = format!(
                "Team {} {} uses {} on Team {} {}",
                combatant.team,
                combatant.class.name(),
                def.name,
                info.team,
                info.class.name()
            );
            combat_log.log_crowd_control(
                combatant_id(combatant.team, combatant.class),
                combatant_id(info.team, info.class),
                cc_type,
                aura.duration,
                message,
            );
        }
    }

    info!(
        "Team {} {} uses {} from stealth!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Kidney Shot.
fn try_kidney_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let kidney_shot = AbilityType::KidneyShot;
    let def = abilities.get_unchecked(&kidney_shot);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        kidney_shot, def, combatant, my_pos, None,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            kidney_shot,
            classify_pre_cast_failure(
                kidney_shot, def, combatant, my_pos, None,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(kidney_shot, Some(target_entity), true);

    spawn_speech_bubble(commands, entity, "Kidney Shot");
    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(kidney_shot, def.cooldown);
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Kidney Shot", target_tuple, "uses");

    if let Some(aura) = def.applies_aura.as_ref() {
        if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, def) {
            same_frame_cc_queue.push((target_entity, aura_pending.aura.clone()));
            commands.spawn(aura_pending);
        }

        if let Some(info) = ctx.combatants.get(&target_entity) {
            let cc_type = format!("{:?}", aura.aura_type);
            let message = format!(
                "Team {} {} uses {} on Team {} {}",
                combatant.team,
                combatant.class.name(),
                def.name,
                info.team,
                info.class.name()
            );
            combat_log.log_crowd_control(
                combatant_id(combatant.team, combatant.class),
                combatant_id(info.team, info.class),
                cc_type,
                aura.duration,
                message,
            );
        }
    }

    info!(
        "Team {} {} uses {} on enemy!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Sinister Strike.
fn try_sinister_strike(
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::SinisterStrike;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, None,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, None,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let self_auras = ctx.active_auras.get(&entity).map(|v| v.as_slice()).unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, ap_bonus);
    let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
    if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Sinister Strike", target_tuple, "uses");

    info!(
        "Team {} {} uses {}!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Select the best target for melee-range CC abilities.
///
/// For melee CC like Kidney Shot, we need range-aware targeting:
/// 1. If CC target is in melee range, use it (strategic CC on healer)
/// 2. If CC target is out of range but kill target is in range, fall back to kill target
/// 3. If neither is in range, return None
///
/// A stun on the kill target is still valuable even if not the ideal CC target.
fn select_melee_cc_target(
    cc_target: Option<Entity>,
    kill_target: Option<Entity>,
    my_pos: Vec3,
    ctx: &CombatContext,
) -> Option<(Entity, Vec3)> {
    if let Some(cc_entity) = cc_target {
        if !ctx.entity_is_immune(cc_entity) {
            if let Some(info) = ctx.combatants.get(&cc_entity) {
                if my_pos.distance(info.position) <= MELEE_RANGE {
                    return Some((cc_entity, info.position));
                }
            }
        }
    }

    if let Some(kill_entity) = kill_target {
        if !ctx.entity_is_immune(kill_entity) {
            if let Some(info) = ctx.combatants.get(&kill_entity) {
                if my_pos.distance(info.position) <= MELEE_RANGE {
                    return Some((kill_entity, info.position));
                }
            }
        }
    }

    None
}
