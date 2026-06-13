//! Mage AI Module
//!
//! Handles AI decision-making for the Mage class.
//!
//! ## Priority Order
//! 1. Ice Barrier (self-shield when no shield or HP < 80%)
//! 2. Mage Armor (self-buff based on preference: Frost Armor / Mage Armor / Molten Armor)
//! 3. Arcane Intellect (buff mana-using allies pre-combat)
//! 4. Frost Nova (defensive AoE when enemies in melee)
//! 5. Polymorph (CC non-kill target to create outnumbering situation)
//! 6. Frostbolt (main damage spell with kiting behavior)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use crate::combat::log::CombatLog;
use crate::states::match_config::{CharacterClass, MageArmor};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRIT_DAMAGE_MULTIPLIER, DEFENSIVE_HP_THRESHOLD, GCD, MELEE_RANGE, SAFE_KITING_DISTANCE,
};
use crate::states::play_match::combat_core::{calculate_cast_time, roll_crit, get_attack_power_bonus_from_slice, get_crit_chance_bonus_from_slice};
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, RejectionReason,
};
use crate::states::play_match::utils::{combatant_id, log_ability_use, spawn_speech_bubble};

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

use super::CombatContext;

/// Mage AI: Decides and executes abilities for a Mage combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_mage_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    frost_nova_damage: &mut Vec<super::QueuedAoeDamage>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // GCD short-circuit — no event (emission gate).
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos) else {
        return false;
    };

    // Priority 1: Ice Barrier (self-shield)
    if try_ice_barrier(commands, combat_log, abilities, entity, combatant, ctx, &mut builder) {
        builder.finish();
        return true;
    }

    // Priority 2: Mage Armor (self-buff based on preference)
    if try_mage_armor(commands, combat_log, abilities, entity, combatant, ctx, &mut builder) {
        builder.finish();
        return true;
    }

    // Priority 3: Arcane Intellect (buff mana-using allies)
    if try_arcane_intellect(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 4: Frost Nova (defensive AoE)
    if try_frost_nova(
        commands,
        combat_log,
        game_rng,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        frost_nova_damage,
        same_frame_cc_queue,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 5: Polymorph (CC non-kill target)
    if try_polymorph(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 6: Frostbolt (main damage spell)
    if try_frostbolt(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    builder.finish();
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
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ice_barrier = AbilityType::IceBarrier;
    let barrier_def = abilities.get_unchecked(&ice_barrier);

    // Check if already shielded
    let has_absorb_shield = ctx.active_auras
        .get(&entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::Absorb))
        .unwrap_or(false);

    if has_absorb_shield {
        builder.reject(ice_barrier, RejectionReason::AlreadyApplied);
        return false;
    }

    let is_full_hp = combatant.current_health >= combatant.max_health;
    let is_below_threshold =
        combatant.current_health < combatant.max_health * DEFENSIVE_HP_THRESHOLD;
    if !(is_full_hp || is_below_threshold) {
        builder.reject(
            ice_barrier,
            RejectionReason::PreconditionUnmet {
                note: "HP above defensive threshold and not full".into(),
            },
        );
        return false;
    }

    if let Some(remaining) = combatant.ability_cooldowns.get(&ice_barrier) {
        builder.reject(ice_barrier, RejectionReason::OnCooldown { remaining: *remaining });
        return false;
    }

    if combatant.current_mana < barrier_def.mana_cost {
        builder.reject(
            ice_barrier,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: barrier_def.mana_cost,
            },
        );
        return false;
    }

    builder.choose(ice_barrier, Some(entity), true);

    spawn_speech_bubble(commands, entity, "Ice Barrier");
    combatant.current_mana -= barrier_def.mana_cost;
    combatant.ability_cooldowns.insert(ice_barrier, barrier_def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Ice Barrier", None, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(entity, entity, barrier_def) {
        commands.spawn(aura_pending);
    }

    info!(
        "Team {} {} casts Ice Barrier",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast the chosen Mage Armor on self (Frost Armor, Mage Armor, or Molten Armor).
/// Returns true if the ability was used.
fn try_mage_armor(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let (ability, aura_check) = match combatant.mage_armor {
        MageArmor::FrostArmor => (AbilityType::FrostArmor, AuraType::FrostArmorBuff),
        MageArmor::MageArmor => (AbilityType::MageArmorSpell, AuraType::ManaRegenIncrease),
        MageArmor::MoltenArmor => (AbilityType::MoltenArmor, AuraType::CritChanceIncrease),
    };

    let already_buffed = ctx.active_auras
        .get(&entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == aura_check))
        .unwrap_or(false);

    if already_buffed {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost {
        builder.reject(
            ability,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    builder.choose(ability, Some(entity), true);

    spawn_speech_bubble(commands, entity, &def.name);
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, None, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(entity, entity, def) {
        commands.spawn(aura_pending);
    }

    info!(
        "Team {} {} casts {}",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Arcane Intellect on an unbuffed mana-using ally.
/// Returns true if the ability was used.
fn try_arcane_intellect(
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
    let ability = AbilityType::ArcaneIntellect;
    let def = abilities.get_unchecked(&ability);

    let mut unbuffed_mana_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 {
            continue;
        }
        if !info.class.uses_mana() {
            continue;
        }
        let has_arcane_intellect = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxManaIncrease))
            .unwrap_or(false);
        if has_arcane_intellect {
            continue;
        }
        unbuffed_mana_ally = Some((*ally_entity, info.position));
        break;
    }

    let Some((buff_target, target_pos)) = unbuffed_mana_ally else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((buff_target, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((buff_target, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(buff_target), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&buff_target).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Arcane Intellect", target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(buff_target, entity, def) {
        commands.spawn(aura_pending);
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
fn try_frost_nova(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    frost_nova_damage: &mut Vec<super::QueuedAoeDamage>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let frost_nova = AbilityType::FrostNova;
    let nova_def = abilities.get_unchecked(&frost_nova);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(frost_nova, nova_def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            frost_nova,
            classify_pre_cast_failure(frost_nova, nova_def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    let enemies_in_melee_range = ctx.combatants.iter().any(|(_, info)| {
        info.team != combatant.team && info.is_alive && !info.is_pet
            && my_pos.distance(info.position) <= MELEE_RANGE
    });

    if !enemies_in_melee_range {
        builder.reject(
            frost_nova,
            RejectionReason::PreconditionUnmet {
                note: "no enemies in melee range".into(),
            },
        );
        return false;
    }

    builder.choose(frost_nova, None, true);

    spawn_speech_bubble(commands, entity, "Frost Nova");
    combatant.current_mana -= nova_def.mana_cost;
    combatant.ability_cooldowns.insert(frost_nova, nova_def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Frost Nova", None, "casts");

    let mut frost_nova_targets: Vec<(Entity, Vec3, u8, CharacterClass)> = Vec::new();
    for (enemy_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team && info.is_alive {
            let distance = my_pos.distance(info.position);
            if distance <= nova_def.range {
                frost_nova_targets.push((*enemy_entity, info.position, info.team, info.class));
            }
        }
    }

    let self_auras = ctx.active_auras.get(&entity).map(|v| v.as_slice()).unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    for (target_entity, target_pos, target_team, target_class) in &frost_nova_targets {
        let mut damage = combatant.calculate_ability_damage_config(nova_def, game_rng, ap_bonus);
        let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
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

        if let Some(aura) = nova_def.applies_aura.as_ref() {
            if !ctx.entity_is_immune(*target_entity) {
                if let Some(aura_pending) = AuraPending::from_ability(*target_entity, entity, nova_def) {
                    same_frame_cc_queue.push((*target_entity, aura_pending.aura.clone()));
                    commands.spawn(aura_pending);
                }

                let message = format!(
                    "Team {} {}'s {} roots Team {} {} ({:.1}s)",
                    combatant.team,
                    combatant.class.name(),
                    nova_def.name,
                    target_team,
                    target_class.name(),
                    aura.duration
                );
                combat_log.log_crowd_control(
                    combatant_id(combatant.team, combatant.class),
                    combatant_id(*target_team, *target_class),
                    "Root".to_string(),
                    aura.duration,
                    message,
                );
            }
        }
    }

    // Movement after Frost Nova is owned by the ENGAGE/KITE posture machine
    // (mage_postures.rs): a melee-range threat now carrying the Mage's root
    // triggers KITE on the next posture evaluation. The Mage no longer writes
    // `kiting_timer` (the legacy kiting branch is Hunter-only now).
    debug_assert_eq!(
        combatant.kiting_timer, 0.0,
        "Mage must not set kiting_timer — movement is posture-driven (U5)"
    );

    info!(
        "Team {} {} casts Frost Nova! (AOE root) - {} enemies affected",
        combatant.team,
        combatant.class.name(),
        frost_nova_targets.len()
    );

    true
}

/// Try to cast Polymorph on the CC target (non-kill target).
fn try_polymorph(
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
    let ability = AbilityType::Polymorph;
    let def = abilities.get_unchecked(&ability);

    let Some(cc_target) = combatant.cc_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    // Don't polymorph the kill target — any damage will break it immediately.
    if combatant.target == Some(cc_target) {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "cc_target equals kill target — would break on damage".into(),
            },
        );
        return false;
    }

    let Some(target_info) = ctx.combatants.get(&cc_target) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_pos = target_info.position;

    if ctx.is_dr_immune(cc_target, DRCategory::Incapacitates) {
        builder.reject(
            ability,
            RejectionReason::DRImmune {
                category: DRCategory::Incapacitates,
            },
        );
        return false;
    }

    // Check if target is already CC'd
    let already_ccd_type = ctx.active_auras
        .get(&cc_target)
        .and_then(|auras| {
            auras.iter().find_map(|a| {
                if matches!(
                    a.effect_type,
                    AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph
                ) {
                    Some(a.effect_type)
                } else {
                    None
                }
            })
        });

    if let Some(cc_type) = already_ccd_type {
        builder.reject(ability, RejectionReason::TargetAlreadyCCd { cc_type });
        return false;
    }

    // GCD check (defensive — outer function already checked).
    if combatant.global_cooldown > 0.0 {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "global cooldown active".into(),
            },
        );
        return false;
    }

    let opts = PreCastOpts {
        check_target_immune: true,
        check_friendly_dots: true,
        ..Default::default()
    };
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((cc_target, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((cc_target, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(cc_target), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, cc_target, cast_time));

    let target_tuple = ctx.combatants
        .get(&cc_target)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on cc_target",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Frostbolt on the current target.
fn try_frostbolt(
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
    let ability = AbilityType::Frostbolt;
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

    let distance_to_target = my_pos.distance(target_pos);

    // While kiting, only cast if at safe distance. Kiting is now posture-state
    // (mage_postures.rs) rather than the legacy `kiting_timer`; the equivalent
    // world-state condition is "a Mage-owned root/slow is on an enemy within
    // safe-kiting distance" — proximity-gated so Frostbolt's own never-breaking
    // slow on a kited-away enemy doesn't permanently suppress hard-casts.
    let kiting =
        super::mage_postures::mage_impaired_enemy(ctx, entity, my_pos, Some(SAFE_KITING_DISTANCE));
    if kiting && distance_to_target < SAFE_KITING_DISTANCE {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "kiting and below safe distance".into(),
            },
        );
        return false;
    }

    if combatant.global_cooldown > 0.0 {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "global cooldown active".into(),
            },
        );
        return false;
    }

    let opts = PreCastOpts {
        check_target_immune: true,
        check_friendly_cc: true,
        ..Default::default()
    };
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((target_entity, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

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
