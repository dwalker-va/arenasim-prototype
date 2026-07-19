//! Warrior AI Module
//!
//! Handles AI decision-making for the Warrior class.
//!
//! ## Priority Order
//! 1. Shout (buff allies or debuff enemies based on warrior_shout preference)
//! 2. Charge (gap closer when out of melee range)
//! 3. Rend (bleed DoT on target)
//! 4. Mortal Strike (main damage, healing reduction)
//! 5. Heroic Strike (rage dump)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::match_config::WarriorShout;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::{roll_crit, get_attack_power_bonus_from_slice, get_crit_chance_bonus_from_slice};
use crate::states::play_match::constants::{CHARGE_MIN_RANGE, CRIT_DAMAGE_MULTIPLIER, GCD, MELEE_RANGE};
use crate::states::play_match::decision_trace::{
    ActorView, DecisionEventBuilder, DecisionTrace, MovementGoalKind, MovementTrigger,
    NoActionReason, Posture as TracePosture, RejectionReason, ResourceKind,
};
use crate::states::play_match::map_geometry::has_line_of_sight;
use crate::states::play_match::movement_config::MeleeMovementConfig;

use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::{pressing_when_ahead, CombatContext};
use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

/// Shout range constant (applies to all shout variants)
const SHOUT_RANGE: f32 = 30.0;

/// Rage reserve for essential abilities
const RAGE_RESERVE: f32 = 50.0;

/// Warrior AI: Decides and executes abilities for a Warrior combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_warrior_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    battle_shouted_this_frame: &mut std::collections::HashSet<Entity>,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // GCD short-circuit — no event (emission gate: tick produced no decision).
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos) else {
        return false;
    };

    // Priority 1: Shout (buff allies or debuff enemies based on preference)
    if try_shout(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        ctx,
        battle_shouted_this_frame,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Get target for combat abilities
    let Some(target_entity) = combatant.target else {
        builder.finish_no_action(NoActionReason::NoValidTarget);
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        builder.finish_no_action(NoActionReason::NoValidTarget);
        return false;
    };
    let target_pos = target_info.position;

    // Don't waste abilities on immune targets (Divine Shield)
    if ctx.entity_is_immune(target_entity) {
        builder.finish_no_action(NoActionReason::TargetImmune);
        return false;
    }

    // Priority 2: Charge (gap closer)
    if try_charge(
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
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Burst-during-CC (bucket A): while the enemy healer is hard-CC'd and
    // can't react, land Mortal Strike (the hard hit + Mortal Wounds healing
    // debuff) BEFORE spending the GCD refreshing the Rend DoT. Outside the
    // window the normal order holds (Rend, then Mortal Strike). Each ability
    // is still attempted at most once per tick (the `!burst_window` guard on
    // the trailing Mortal Strike), keeping the decision trace clean.
    let burst_window = ctx.enemy_healer_is_cced();

    if burst_window
        && try_mortal_strike(
            commands, combat_log, game_rng, abilities, entity, combatant, my_pos, auras,
            target_entity, target_pos, ctx, instant_attacks, &mut builder,
        )
    {
        builder.finish();
        return true;
    }

    // Priority 3: Rend (DoT)
    if try_rend(
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
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 4: Mortal Strike (normal-order attempt — skipped when the burst
    // window already attempted it above).
    if !burst_window
        && try_mortal_strike(
            commands,
            combat_log,
            game_rng,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            target_entity,
            target_pos,
            ctx,
            instant_attacks,
            &mut builder,
        )
    {
        builder.finish();
        return true;
    }

    // Priority 5: Heroic Strike (rage dump)
    try_heroic_strike(abilities, combatant, target_pos, my_pos, &mut builder);

    // No GCD-consuming ability used this tick. Heroic Strike may have queued
    // an attack-bonus without taking a GCD; either way, finish records the
    // candidate set with NoAction (AllCandidatesRejected) when none chose.
    builder.finish();
    false
}

/// Try to cast the warrior's chosen shout (Battle Shout, Demoralizing Shout, or Commanding Shout).
/// Dispatches based on `combatant.warrior_shout` preference.
/// Returns true if the ability was used.
fn try_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    match combatant.warrior_shout {
        WarriorShout::BattleShout => try_battle_shout(
            commands, combat_log, abilities, entity, combatant, my_pos, ctx, shouted_this_frame, builder,
        ),
        WarriorShout::DemoralizingShout => try_demoralizing_shout(
            commands, combat_log, abilities, entity, combatant, my_pos, ctx, shouted_this_frame, builder,
        ),
        WarriorShout::CommandingShout => try_commanding_shout(
            commands, combat_log, abilities, entity, combatant, my_pos, ctx, shouted_this_frame, builder,
        ),
    }
}

/// Try to cast Battle Shout to buff nearby allies with AttackPowerIncrease.
/// Returns true if the ability was used.
fn try_battle_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::BattleShout;
    let def = abilities.get_unchecked(&ability);

    let mut targets: Vec<Entity> = Vec::new();

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 {
            continue;
        }
        if my_pos.distance(info.position) > SHOUT_RANGE {
            continue;
        }

        let already_has = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::AttackPowerIncrease))
            .unwrap_or(false);

        if !already_has && !shouted_this_frame.contains(ally_entity) {
            targets.push(*ally_entity);
        }
    }

    if targets.is_empty() {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    }

    if combatant.current_mana < def.mana_cost && def.mana_cost > 0.0 {
        builder.reject(
            ability,
            RejectionReason::InsufficientResource {
                resource: ResourceKind::Rage,
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    builder.choose(ability, None, true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Battle Shout", None, "uses");

    for target in targets {
        shouted_this_frame.insert(target);
        if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
            commands.spawn(aura_pending);
        }
    }

    info!(
        "Team {} {} uses Battle Shout",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Demoralizing Shout to debuff nearby enemies with AttackPowerReduction.
/// Returns true if the ability was used.
fn try_demoralizing_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::DemoralizingShout;
    let def = abilities.get_unchecked(&ability);

    let mut targets: Vec<Entity> = Vec::new();

    for (enemy_entity, info) in ctx.combatants.iter() {
        if info.team == combatant.team || info.current_health <= 0.0 || info.stealthed {
            continue;
        }
        if my_pos.distance(info.position) > SHOUT_RANGE {
            continue;
        }

        let already_has = ctx.active_auras
            .get(enemy_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::AttackPowerReduction))
            .unwrap_or(false);

        if !already_has && !shouted_this_frame.contains(enemy_entity) {
            targets.push(*enemy_entity);
        }
    }

    if targets.is_empty() {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    }

    if combatant.current_mana < def.mana_cost && def.mana_cost > 0.0 {
        builder.reject(
            ability,
            RejectionReason::InsufficientResource {
                resource: ResourceKind::Rage,
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    builder.choose(ability, None, true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Demoralizing Shout", None, "uses");

    for target in targets {
        shouted_this_frame.insert(target);
        if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
            commands.spawn(aura_pending);
        }
    }

    info!(
        "Team {} {} uses Demoralizing Shout",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Commanding Shout to buff nearby allies with MaxHealthIncrease.
/// Returns true if the ability was used.
fn try_commanding_shout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    shouted_this_frame: &mut std::collections::HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::CommandingShout;
    let def = abilities.get_unchecked(&ability);

    let mut targets: Vec<Entity> = Vec::new();

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 {
            continue;
        }
        if my_pos.distance(info.position) > SHOUT_RANGE {
            continue;
        }

        let already_has = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxHealthIncrease))
            .unwrap_or(false);

        if !already_has && !shouted_this_frame.contains(ally_entity) {
            targets.push(*ally_entity);
        }
    }

    if targets.is_empty() {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    }

    if combatant.current_mana < def.mana_cost && def.mana_cost > 0.0 {
        builder.reject(
            ability,
            RejectionReason::InsufficientResource {
                resource: ResourceKind::Rage,
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    builder.choose(ability, None, true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Commanding Shout", None, "uses");

    for target in targets {
        shouted_this_frame.insert(target);
        if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
            commands.spawn(aura_pending);
        }
    }

    info!(
        "Team {} {} uses Commanding Shout",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to use Charge to close distance.
/// Returns true if Charge was used.
fn try_charge(
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let charge = AbilityType::Charge;
    let charge_def = abilities.get_unchecked(&charge);

    if ctx.has_friendly_breakable_cc(target_entity) {
        builder.reject(charge, RejectionReason::FriendlyBreakableCC);
        return false;
    }

    if let Some(remaining) = combatant.ability_cooldowns.get(&charge) {
        builder.reject(charge, RejectionReason::OnCooldown { remaining: *remaining });
        return false;
    }

    // Check if rooted
    let is_rooted = auras
        .map(|a| a.auras.iter().any(|aura| matches!(aura.effect_type, AuraType::Root)))
        .unwrap_or(false);

    if is_rooted {
        builder.reject(charge, RejectionReason::Rooted);
        return false;
    }

    let distance_to_target = my_pos.distance(target_pos);

    // Must be within charge range
    if distance_to_target < CHARGE_MIN_RANGE {
        builder.reject(
            charge,
            RejectionReason::WithinDeadZone {
                distance: distance_to_target,
                min: CHARGE_MIN_RANGE,
            },
        );
        return false;
    }
    if distance_to_target > charge_def.range {
        builder.reject(
            charge,
            RejectionReason::OutOfRange {
                distance: distance_to_target,
                max: charge_def.range,
            },
        );
        return false;
    }

    // The dash is a straight sprint to the target — if an obstacle crosses that
    // segment, Charge would clip into (or slide to a stop against) the pillar
    // instead of reaching melee. Reject up front so the AI picks another action;
    // `move_to_target`'s per-frame slide is only the mid-dash safety net.
    if !has_line_of_sight(ctx.obstacles, my_pos, target_pos) {
        builder.reject(charge, RejectionReason::LosBlocked);
        return false;
    }

    builder.choose(charge, Some(target_entity), true);

    // Execute Charge
    combatant.ability_cooldowns.insert(charge, charge_def.cooldown);
    combatant.global_cooldown = GCD;

    commands.entity(entity).insert(ChargingState {
        target: target_entity,
    });

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Charge", target_tuple, "uses");

    info!(
        "Team {} {} uses Charge on enemy (distance: {:.1} units)",
        combatant.team,
        combatant.class.name(),
        distance_to_target
    );

    true
}

/// Try to apply Rend DoT to target.
/// Returns true if Rend was used.
fn try_rend(
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let rend = AbilityType::Rend;
    let rend_def = abilities.get_unchecked(&rend);

    // Check if target already has Rend (any DoT for now)
    let target_has_rend = ctx.active_auras
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime))
        .unwrap_or(false);

    if target_has_rend {
        builder.reject(rend, RejectionReason::AlreadyApplied);
        return false;
    }

    let rend_opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        rend,
        rend_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        rend_opts,
    ) {
        builder.reject(
            rend,
            classify_pre_cast_failure(
                rend,
                rend_def,
                combatant,
                my_pos,
                auras,
                Some((target_entity, target_pos)),
                ctx,
                rend_opts,
            ),
        );
        return false;
    }

    builder.choose(rend, Some(target_entity), true);

    // Execute Rend
    combatant.current_mana -= rend_def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Rend", target_tuple, "uses");

    // Apply DoT aura
    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, rend_def) {
        commands.spawn(aura_pending);
    }

    combat_log.log(
        CombatLogEventType::Buff,
        format!(
            "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to use Mortal Strike.
/// Returns true if Mortal Strike was used.
fn try_mortal_strike(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let mortal_strike = AbilityType::MortalStrike;
    let ms_def = abilities.get_unchecked(&mortal_strike);

    let ms_opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        mortal_strike,
        ms_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        ms_opts,
    ) {
        builder.reject(
            mortal_strike,
            classify_pre_cast_failure(
                mortal_strike,
                ms_def,
                combatant,
                my_pos,
                auras,
                Some((target_entity, target_pos)),
                ctx,
                ms_opts,
            ),
        );
        return false;
    }

    // Get target info
    let target_info = match ctx.combatants.get(&target_entity) {
        Some(info) => info,
        None => {
            builder.reject(mortal_strike, RejectionReason::NoValidTarget);
            return false;
        }
    };

    builder.choose(mortal_strike, Some(target_entity), true);

    // Execute Mortal Strike
    combatant.current_mana -= ms_def.mana_cost;
    combatant.ability_cooldowns.insert(mortal_strike, ms_def.cooldown);
    combatant.global_cooldown = GCD;

    // Log
    log_ability_use(combat_log, combatant.team, combatant.class, "Mortal Strike", Some((target_info.team, target_info.class)), "uses");

    // Calculate and queue damage (with dynamic aura bonuses)
    let self_auras = ctx.active_auras.get(&entity).map(|v| v.as_slice()).unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(ms_def, game_rng, ap_bonus, 0.0);
    let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
    if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_class: combatant.class,
        ability: mortal_strike,
        is_crit,
    });

    // Apply healing reduction aura
    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, ms_def) {
        commands.spawn(aura_pending);
    }

    info!(
        "Team {} {} uses Mortal Strike for {:.0} damage!",
        combatant.team,
        combatant.class.name(),
        damage
    );

    true
}

/// Try to queue Heroic Strike for next auto-attack.
/// This doesn't consume a GCD, just queues bonus damage.
fn try_heroic_strike(
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    target_pos: Vec3,
    my_pos: Vec3,
    builder: &mut DecisionEventBuilder<'_>,
) {
    let ability = AbilityType::HeroicStrike;
    let def = abilities.get_unchecked(&ability);

    // Don't queue if one is already pending
    if combatant.next_attack_bonus_damage > 0.0 {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return;
    }

    // Only use if we have enough rage for Heroic Strike AND reserve
    if combatant.current_mana < (def.mana_cost + RAGE_RESERVE) {
        builder.reject(
            ability,
            RejectionReason::InsufficientResource {
                resource: ResourceKind::Rage,
                have: combatant.current_mana,
                need: def.mana_cost + RAGE_RESERVE,
            },
        );
        return;
    }

    if !ability.can_cast_config(combatant, target_pos, my_pos, def) {
        // can_cast_config failed — most likely range. We don't have a direct
        // distance here, but the predicate already short-circuited on the
        // standard range/mana checks. Emit PreconditionUnmet with a hint.
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "can_cast_config failed (range/mana/stealth)".into(),
            },
        );
        return;
    }

    builder.choose(ability, None, true);

    // Consume rage and queue bonus damage
    combatant.current_mana -= def.mana_cost;
    let bonus_damage = combatant.attack_damage * 0.5;
    combatant.next_attack_bonus_damage = bonus_damage;
    combatant.global_cooldown = GCD;

    info!(
        "Team {} {} uses Heroic Strike (next attack +{:.0} damage)",
        combatant.team,
        combatant.class.name(),
        bonus_damage
    );
}

/// Try to use Berserker Rage while incapacitated (fear break path).
///
/// Called from `combat_ai.rs` before the incapacitation gate, mirroring the
/// Paladin Divine Shield while-CC arm. The caller owns the builder lifecycle.
///
/// TBC-faithful semantics: usable while FEARED (breaks the fear + grants 10s
/// fear immunity via the deferred `BerserkerRagePending`), but NOT while
/// stunned/polymorphed/incapacitated, and Death Coil's horror (a Fear-type
/// aura on the dedicated Horror DR bucket) can be neither broken nor blocked.
/// Reads CC state from the snapshot (`ctx`) so instant fears landed earlier
/// this frame (e.g. Psychic Scream) are answered on the same tick.
pub fn try_berserker_rage_while_cc(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::BerserkerRage;
    let def = match abilities.get(&ability) {
        Some(d) => d,
        None => return false,
    };

    // Hard-CC'd in a non-fear way: cannot act at all. Horror also fully locks
    // the Warrior out — only a real Fear leaves the "break out" window open.
    let hard_locked = ctx.self_auras().map_or(false, |auras| {
        auras.iter().any(|a| {
            matches!(
                a.effect_type,
                AuraType::Stun | AuraType::Polymorph | AuraType::Incapacitate
            ) || (a.effect_type == AuraType::Fear
                && a.dr_category() == Some(DRCategory::Horror))
        })
    });
    if hard_locked {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "hard-CC'd (stun/poly/incap/horror) — Berserker Rage only answers Fear".into(),
            },
        );
        return false;
    }

    // Only worth pressing if there's a breakable Fear on us.
    let has_breakable_fear = ctx.self_auras().map_or(false, |auras| {
        auras.iter().any(|a| {
            a.effect_type == AuraType::Fear && a.dr_category() != Some(DRCategory::Horror)
        })
    });
    if !has_breakable_fear {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "no breakable Fear on self".into(),
            },
        );
        return false;
    }

    if combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0) > 0.0 {
        let remaining = combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0);
        builder.reject(ability, RejectionReason::OnCooldown { remaining });
        return false;
    }

    if ctx.has_aura(AuraType::FearImmunity) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    builder.choose(ability, Some(entity), true);

    let caster_id = combatant_id(combatant.team, combatant.class);
    info!("{} breaks fear with Berserker Rage!", caster_id);

    commands.spawn(BerserkerRagePending {
        caster: entity,
        caster_team: combatant.team,
        caster_class: combatant.class,
    });

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Berserker Rage", None, "uses");

    true
}

/// True while a movement-impairing CC (Root / Stun / Incapacitate — the same
/// set `move_to_target` treats as movement-preventing) sits on the warrior.
/// Fear is excluded: a feared warrior already runs, so there is no stalled "go"
/// to reset. Pure over the aura list for unit testing.
pub fn under_movement_cc(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| {
        a.auras.iter().any(|aura| {
            matches!(
                aura.effect_type,
                AuraType::Root | AuraType::Stun | AuraType::Incapacitate
            )
        })
    })
}

/// The melee tempo-reset decision seam (Warrior), pure for unit testing.
/// The reset runs — falling back toward the healer instead of face-chasing —
/// only while ALL of: the team is NOT pressing an advantage (a clearly
/// winning team keeps chasing rather than resetting tempo), the CC-armed window
/// is still open (`now < armed_until`), the gap closer (Charge) is on cooldown,
/// the warrior is out of melee range of its target, and a living healer ally
/// exists to fall back toward. Any one failing resumes normal pursuit; the gap
/// closer coming up is the intended exit (re-engage with a fresh Charge).
pub fn melee_reset_active(
    now: f32,
    armed_until: f32,
    charge_on_cooldown: bool,
    out_of_melee: bool,
    has_healer: bool,
    pressing: bool,
) -> bool {
    !pressing && now < armed_until && charge_on_cooldown && out_of_melee && has_healer
}

/// Nearest living, non-pet healer ally position to `my_pos` — the fallback
/// anchor for the tempo reset. Deterministic: iterates the BTree-ordered
/// snapshot, tie-breaking equal distances by entity order.
fn nearest_healer_ally(ctx: &CombatContext, my_pos: Vec3) -> Option<Vec3> {
    ctx.alive_allies()
        .into_iter()
        .filter(|i| i.class.is_healer())
        .min_by(|a, b| {
            a.position
                .distance(my_pos)
                .partial_cmp(&b.position.distance(my_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|i| i.position)
}

/// Warrior tempo-reset movement pre-pass. Runs before the ability pass and
/// outside the GCD short-circuit (legs aren't on the GCD), mirroring the Mage /
/// healer posture pre-passes. Arms the reset while under movement CC, then —
/// once the CC drops and while Charge is still down — issues a
/// `MovementGoal::Point` directive toward the nearest healer for the armed
/// window, so the warrior regroups instead of walking straight back into the
/// kiter's CC. The directive is re-issued each frame while active and cleared
/// on deactivation; `move_to_target` executes it in the same slot as the kite /
/// healer directives. A no-op when the warrior has no healer ally (nothing to
/// fall back toward) and, being CC-gated, dormant until its go is actually
/// stopped.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_warrior_reset(
    commands: &mut Commands,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    reset_state: Option<&mut MeleeResetState>,
    directive: Option<&MovementDirective>,
    cfg: &MeleeMovementConfig,
    press_margin: f32,
    now: f32,
    decision_trace: &mut DecisionTrace,
) {
    let mut local = MeleeResetState::default();
    let needs_insert = reset_state.is_none();
    let state: &mut MeleeResetState = reset_state.unwrap_or(&mut local);

    // Press-when-ahead: a team clearly ahead keeps chasing rather than
    // resetting tempo, so suppress BOTH arming and activation while pressing.
    let pressing = pressing_when_ahead(ctx.team_hp_advantage(), press_margin);

    // Arm while under movement CC — the window stays open `reset_window` after
    // the CC ends so the reset actually runs (a rooted warrior can't move, so
    // the useful moment is right after the root drops). Not while pressing.
    if !pressing && under_movement_cc(auras) {
        state.armed_until = now + cfg.reset_window;
    }

    let charge_on_cd = combatant
        .ability_cooldowns
        .get(&AbilityType::Charge)
        .copied()
        .unwrap_or(0.0)
        > 0.0;
    let out_of_melee = combatant
        .target
        .and_then(|t| ctx.combatants.get(&t))
        .map_or(false, |i| my_pos.distance(i.position) > MELEE_RANGE);
    let healer_pos = nearest_healer_ally(ctx, my_pos);

    let active = melee_reset_active(
        now,
        state.armed_until,
        charge_on_cd,
        out_of_melee,
        healer_pos.is_some(),
        pressing,
    );

    if active {
        let healer_pos = healer_pos.expect("active implies a healer ally");
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(healer_pos),
            // Bounded by the armed window — the directive can never outlive it.
            expires: state.armed_until,
            committed_until: now, // no commit window; re-issued each frame
        });
        if !state.active {
            // Activation edge — trace once, not every frame.
            if let Some(info) = ctx.combatants.get(&entity) {
                let actor = ActorView::from_info(info);
                let mut builder = decision_trace.start_movement_decision(actor, None);
                builder.direction_change(
                    TracePosture::Free,
                    MovementTrigger::MeleeReset,
                    MovementGoalKind::Point,
                );
                builder.finish();
            }
        }
        state.active = true;
    } else {
        if state.active && directive.is_some() {
            // We issued the fallback directive; clear it so normal pursuit
            // (face-chase / charge) resumes.
            commands.entity(entity).remove::<MovementDirective>();
        }
        state.active = false;
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }
}

