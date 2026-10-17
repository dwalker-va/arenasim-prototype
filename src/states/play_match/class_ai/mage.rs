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

use crate::combat::log::CombatLog;
use crate::states::match_config::MageArmor;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::{
    calculate_cast_time, get_attack_power_bonus_from_slice, get_crit_chance_bonus_from_slice,
    get_spell_power_bonus_from_slice, roll_crit,
};
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRIT_DAMAGE_MULTIPLIER, DEFENSIVE_HP_THRESHOLD, GCD, MELEE_RANGE, SAFE_KITING_DISTANCE,
};
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, PlayStep, RejectionReason,
};
use crate::states::play_match::situation::Role;
use crate::states::play_match::utils::{combatant_id, log_ability_use};
use bevy::prelude::*;

use super::cast_guard::{cast_gate, PreCastOpts};

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

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos)
    else {
        return false;
    };

    // The Peel play (TeamPlan): a teammate is in danger, so crowd control on
    // its attacker heads the chain — Frost Nova when one is in reach of the
    // Nova, else Polymorph.
    if let Some(call) = super::peel::peel_call(ctx, Role::RangedDps) {
        let peeled = try_frost_nova(
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
            NovaTrigger::Peel(&call.attackers),
            &mut builder,
        ) || call.attackers.iter().any(|&attacker| {
            try_polymorph_on(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                attacker,
                &mut builder,
            )
        });
        if peeled {
            builder.play_step(PlayStep::Peel);
            builder.finish();
            return true;
        }
    }

    // The Chain play (TeamPlan): a lock the team holds on an enemy is about to
    // end, so Polymorph hands off onto it, landing in its last moments — or
    // the global cooldown is held for that moment.
    if let Some(call) = super::chain::chain_call(ctx) {
        let plan = super::chain::plan(
            ctx,
            abilities,
            combatant,
            my_pos,
            auras,
            &call,
            &mut builder,
        );
        let handed = plan.due.contains(&AbilityType::Polymorph)
            && try_polymorph_on(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                call.subject,
                &mut builder,
            );
        if handed || plan.hold {
            builder.play_step(PlayStep::Chain);
            builder.finish();
            return handed;
        }
    }

    // The Burst play (TeamPlan): a kill window is open, so Frostbolt on its
    // enemy heads the chain — only if it lands before the window's `until`
    // (the cast gate's fit).
    if let Some(call) = super::burst::burst_call(ctx) {
        if try_frostbolt_on(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            call.subject,
            &mut builder,
        ) {
            builder.play_step(PlayStep::Burst);
            builder.finish();
            return true;
        }
    }

    // Priority 1: Ice Barrier (self-shield)
    if try_ice_barrier(
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

    // Priority 2: Mage Armor (self-buff based on preference)
    if try_mage_armor(
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
        NovaTrigger::MeleeOnMe,
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
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ice_barrier = AbilityType::IceBarrier;
    let barrier_def = abilities.get_unchecked(&ice_barrier);

    // Check if already shielded
    let has_absorb_shield = ctx
        .active_auras
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

    let cleared = match cast_gate(
        ice_barrier,
        barrier_def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ice_barrier, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(entity), true);

    combatant.current_mana -= barrier_def.mana_cost;
    combatant.start_cooldown(ice_barrier, abilities);
    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Ice Barrier",
        None,
        "casts",
    );

    if let Some(aura_pending) = AuraPending::from_ability(entity, entity, barrier_def) {
        commands.spawn(aura_pending);
    }

    // The Frost launch on the Mage's hands (client kit 207). Ice Barrier never
    // has a `CastingState`, so the marker is its only route to a caster
    // gesture. Spawned on the committed-use branch in BOTH modes (headless
    // never reads it; the match-exit `PlayMatchEntity` sweep reclaims it), per
    // `cosmetic-marker-cross-mode-spawn-parity.md`.
    commands.spawn((
        InstantAbilityFired {
            caster: entity,
            target: Some(entity),
            ability: ice_barrier,
            is_crit: false,
        },
        PlayMatchEntity,
    ));

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
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let (ability, aura_check) = match combatant.mage_armor {
        MageArmor::FrostArmor => (AbilityType::FrostArmor, AuraType::FrostArmorBuff),
        MageArmor::MageArmor => (AbilityType::MageArmorSpell, AuraType::ManaRegenIncrease),
        MageArmor::MoltenArmor => (AbilityType::MoltenArmor, AuraType::CritChanceIncrease),
    };

    let already_buffed = ctx
        .active_auras
        .get(&entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == aura_check))
        .unwrap_or(false);

    if already_buffed {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let def = abilities.get_unchecked(&ability);

    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        None,
        "casts",
    );

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
        let has_arcane_intellect = ctx
            .active_auras
            .get(ally_entity)
            .map(|auras| {
                auras
                    .iter()
                    .any(|a| a.holds_type_slot(AuraType::MaxManaIncrease))
            })
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
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((buff_target, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(buff_target), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&buff_target).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Arcane Intellect",
        target_tuple,
        "casts",
    );

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

/// What makes Frost Nova worth casting.
#[derive(Clone, Copy)]
enum NovaTrigger<'a> {
    /// The rotation's: a (non-pet) enemy is in melee range of the Mage.
    MeleeOnMe,
    /// The Peel play's: one of these attackers of a teammate in danger is a
    /// melee or a pet ([`super::peel::movement_peel_target`]) inside the
    /// Nova's reach.
    Peel(&'a [Entity]),
}

/// Try to cast Frost Nova when `trigger` holds.
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
    trigger: NovaTrigger,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let frost_nova = AbilityType::FrostNova;
    let nova_def = abilities.get_unchecked(&frost_nova);

    let opts = PreCastOpts::default();
    let cleared = match cast_gate(
        frost_nova, nova_def, combatant, my_pos, auras, None, ctx, opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(frost_nova, reason);
            return false;
        }
    };

    match trigger {
        NovaTrigger::MeleeOnMe => {
            let enemies_in_melee_range = ctx.combatants.iter().any(|(_, info)| {
                info.team != combatant.team
                    && info.is_alive
                    && !info.is_pet
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
        }
        NovaTrigger::Peel(attackers) => {
            let in_reach = attackers.iter().any(|&a| {
                super::peel::movement_peel_target(ctx, a)
                    && !ctx.entity_is_immune(a)
                    && ctx
                        .combatants
                        .get(&a)
                        .is_some_and(|info| my_pos.distance(info.position) <= nova_def.range)
            });
            if !in_reach {
                builder.reject(
                    frost_nova,
                    RejectionReason::PreconditionUnmet {
                        note: "peel: no melee or pet attacker of the teammate in danger in \
                               the Nova's reach"
                            .into(),
                    },
                );
                return false;
            }
        }
    }

    builder.choose(cleared, None, true);

    // Caster-side gesture marker (A2) — see the note in `cast_hammer_of_justice`
    // for the spawn contract.
    //
    // Spawned EXACTLY ONCE, here, outside the per-target loop below, and with
    // `target: None`. Frost Nova is a point-blank AoE whose geometry is anchored
    // on the caster's own feet: one marker per victim would stack N copies of a
    // single caster-centred effect at the same origin, N-times bright, with a
    // footprint that changed depending on how many enemies happened to be in
    // range. There is also no honest value for `target` — picking one victim out
    // of the list would be arbitrary, and the geometry would then be anchored on
    // it.
    commands.spawn((
        InstantAbilityFired {
            caster: entity,
            target: None,
            ability: frost_nova,
            is_crit: false,
        },
        PlayMatchEntity,
    ));

    combatant.current_mana -= nova_def.mana_cost;
    combatant.start_cooldown(frost_nova, abilities);
    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Frost Nova",
        None,
        "casts",
    );

    // Carry each target's pet-aware combat-log id so the root-CC log below
    // attributes correctly when Frost Nova catches an enemy pet.
    // Victims come from `area_victims`, not `ctx.combatants`: the Nova was
    // DECIDED on a seen enemy above, but an area does not aim, so a stealthed
    // Rogue standing in it is caught — and revealed by the damage.
    let frost_nova_targets: Vec<(Entity, Vec3, crate::combat::log::CombatantId)> = ctx
        .area_victims(combatant.team, my_pos, nova_def.range)
        .into_iter()
        .map(|info| (info.entity, info.position, info.log_id()))
        .collect();

    let self_auras = ctx
        .active_auras
        .get(&entity)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    // Frost Nova scales with SpellPower, so include any spell-power auras (e.g. an
    // ally Shaman's Flametongue totem) — matching the generic hardcast path.
    let sp_bonus = get_spell_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    for (target_entity, target_pos, target_id) in &frost_nova_targets {
        let mut damage =
            combatant.calculate_ability_damage_config(nova_def, game_rng, ap_bonus, sp_bonus);
        let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
        if is_crit {
            damage *= CRIT_DAMAGE_MULTIPLIER;
        }
        frost_nova_damage.push(super::QueuedAoeDamage {
            caster: entity,
            target: *target_entity,
            damage,
            caster_team: combatant.team,
            caster_slot: combatant.slot,
            caster_class: combatant.class,
            target_pos: *target_pos,
            is_crit,
        });

        if let Some(aura) = nova_def.applies_aura.as_ref() {
            if !ctx.entity_is_immune(*target_entity) {
                if let Some(aura_pending) =
                    AuraPending::from_ability(*target_entity, entity, nova_def)
                {
                    same_frame_cc_queue.push((*target_entity, aura_pending.aura.clone()));
                    commands.spawn(aura_pending);
                }

                let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
                let message = format!(
                    "{}'s {} roots {} ({:.1}s)",
                    caster_id, nova_def.name, target_id, aura.duration
                );
                combat_log.log_crowd_control(
                    caster_id,
                    target_id.clone(),
                    "Root".to_string(),
                    aura.duration,
                    message,
                );
            }
        }
    }

    // Movement after Frost Nova is owned by the ENGAGE/KITE posture machine
    // (dps_postures.rs): a melee-range threat now carrying the Mage's root
    // triggers KITE on the next posture evaluation.

    info!(
        "Team {} {} casts Frost Nova! (AOE root) - {} enemies affected",
        combatant.team,
        combatant.class.name(),
        frost_nova_targets.len()
    );

    true
}

/// Whether the engine would refuse a Polymorph on `target` — it is shifted into
/// Travel Form or cycloned — asked through the engine's own immunity rule
/// (`auras::aura_type_immunity_source`), so the Mage never casts one that
/// cannot land.
pub fn polymorph_refused(ctx: &CombatContext, target: Entity) -> bool {
    ctx.active_auras.get(&target).is_some_and(|auras| {
        crate::states::play_match::auras::aura_type_immunity_source(auras, AuraType::Polymorph)
            .is_some()
    })
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
    let Some(cc_target) = combatant.cc_target else {
        builder.reject(AbilityType::Polymorph, RejectionReason::NoValidTarget);
        return false;
    };
    try_polymorph_on(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx, cc_target, builder,
    )
}

/// Try to cast Polymorph on `cc_target`: the rotation's CC target, or the
/// Peel play's attacker.
fn try_polymorph_on(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    cc_target: Entity,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Polymorph;
    let def = abilities.get_unchecked(&ability);

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

    // A Druid in Travel Form (or a cycloned target) cannot be polymorphed: the
    // engine refuses the aura (`aura_immunity_source`), so the cast would
    // spend the GCD and land nothing.
    if polymorph_refused(ctx, cc_target) {
        builder.reject(ability, RejectionReason::TargetImmune);
        return false;
    }

    if ctx.is_dr_immune(cc_target, DRCategory::Incapacitates) {
        builder.reject(
            ability,
            RejectionReason::DRImmune {
                category: DRCategory::Incapacitates,
            },
        );
        return false;
    }

    // Already held (its own Polymorph, a teammate's stun)? No value in
    // re-CCing it. A rooted target still acts, so a root does not count.
    // A Chain play hand-off is judged where it lands (`hold_lands_after`).
    let lands_after =
        super::cast_guard::hold_lands_after(def, my_pos, auras, Some((cc_target, target_pos)), ctx);
    if let Some(cc_type) = ctx.held_in_hard_cc_at(cc_target, super::Incoming::of(def), lands_after)
    {
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
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((cc_target, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(cc_target), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(ability, cc_target, cast_time));

    let target_tuple = ctx.combatants.get(&cc_target).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_tuple,
        "begins casting",
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
    let Some(target_entity) = combatant.target else {
        builder.reject(AbilityType::Frostbolt, RejectionReason::NoValidTarget);
        return false;
    };
    try_frostbolt_on(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        target_entity,
        builder,
    )
}

/// Try to cast Frostbolt on `target_entity`.
fn try_frostbolt_on(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    target_entity: Entity,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Frostbolt;
    let def = abilities.get_unchecked(&ability);

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_pos = target_info.position;

    let distance_to_target = my_pos.distance(target_pos);

    // While kiting, only cast if at safe distance. Kiting is now posture-state
    // (dps_postures.rs) rather than the legacy `kiting_timer`; the equivalent
    // world-state condition is "a Mage-owned root/slow is on an enemy within
    // safe-kiting distance" — proximity-gated so Frostbolt's own never-breaking
    // slow on a kited-away enemy doesn't permanently suppress hard-casts.
    let kiting =
        super::dps_postures::mage_impaired_enemy(ctx, entity, my_pos, Some(SAFE_KITING_DISTANCE));
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
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(ability, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_tuple,
        "begins casting",
    );

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}
