//! Rogue AI Module
//!
//! Handles AI decision-making for the Rogue class.
//!
//! ## Priority Order (Stealthed)
//! 1. Ambush (opener from stealth)
//!
//! ## Priority Order (In Combat)
//! 1. Kidney Shot (stun) — gated by `plan_kidney_shot`:
//!    - **Opener-extend**: chain Kidney onto an expiring Cheap Shot on the kill
//!      target for a ~10s lockdown (Kidney Shot has its own DR category, so the
//!      stuns don't diminish).
//!    - **Caster chain** (Mage/Priest/Warlock/Paladin): Kick → hold → Kidney.
//!      Hold the stun while a Kick or school lockout is denying casts (no
//!      double-spend); fire it to extend the denial as the lockout lapses, or
//!      immediately if the target casts an un-locked second school.
//!    - **Aggressive** (Warrior/Rogue/Hunter): opportunistic stun whenever up.
//! 2. Sinister Strike (combo point builder)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::match_config::{CharacterClass, RogueOpener};
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
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

    // Priority 1: Kidney Shot (melee-range CC), gated by the chain planner.
    // The planner decides whether to fire the stun now, hold it (so a Kick or an
    // active school lockout does the denial instead of double-spending), or pool
    // energy so the stun is ready the instant a control window lapses. See
    // `plan_kidney_shot` for the full state machine.
    let kidney_shot = AbilityType::KidneyShot;
    match plan_kidney_shot(entity, combatant, ctx, abilities, my_pos) {
        KidneyPlan::NoTarget => {
            builder.reject(kidney_shot, RejectionReason::NoValidTarget);
        }
        KidneyPlan::Hold(reason) => {
            // Hold the stun but keep building damage with Sinister Strike.
            builder.reject(kidney_shot, reason);
        }
        KidneyPlan::Pool(reason) => {
            // Suppress BOTH the stun (not yet) and Sinister Strike (pool energy),
            // so Kidney Shot is affordable the instant its control window reaches
            // the chain buffer — a seamless stun chain with no energy-starved gap.
            builder.reject(kidney_shot, reason);
            builder.reject(
                AbilityType::SinisterStrike,
                RejectionReason::PreconditionUnmet {
                    note: "pooling energy for Kidney Shot chain".into(),
                },
            );
            builder.finish();
            return false;
        }
        KidneyPlan::Fire { target: ks_target_entity, pos: ks_target_pos, stacking } => {
            // `stacking` is set only by the opener-extend branch, which
            // intentionally stacks Kidney onto an about-to-expire Cheap Shot — so
            // the usual "already stunned" guard must not block it there.
            let target_already_stunned = ctx.active_auras
                .get(&ks_target_entity)
                .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::Stun))
                .unwrap_or(false);

            if target_already_stunned && !stacking {
                builder.reject(
                    kidney_shot,
                    RejectionReason::TargetAlreadyCCd { cc_type: AuraType::Stun },
                );
            } else if ctx.is_dr_immune(ks_target_entity, DRCategory::KidneyShotStun) {
                builder.reject(
                    kidney_shot,
                    RejectionReason::DRImmune { category: DRCategory::KidneyShotStun },
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
        }
    }

    // Priority 2: Sinister Strike
    let acted = try_sinister_strike(
        combat_log, game_rng, abilities, entity, combatant, my_pos,
        target_entity, target_pos, ctx, instant_attacks, &mut builder,
    );
    builder.finish();
    acted
}

/// Buffer (seconds) before a control window expires at which the Rogue chains
/// the next stun. Firing this early trades ~0.5s of the lockdown for safety
/// against the target dying / going immune / leaving melee in the final moment.
const KIDNEY_CHAIN_BUFFER: f32 = 0.5;

/// The planner's verdict for the in-combat Kidney Shot decision.
enum KidneyPlan {
    /// No valid melee Kidney Shot target this tick.
    NoTarget,
    /// Hold the stun (reject Kidney) but keep doing Sinister Strike damage. The
    /// carried reason is surfaced in the decision trace.
    Hold(RejectionReason),
    /// Hold the stun AND suppress Sinister Strike to pool energy, so Kidney Shot
    /// is affordable the instant its control window lapses (seamless chain).
    Pool(RejectionReason),
    /// Fire Kidney Shot now on `target`. `stacking` is set only for the
    /// opener-extend (intentionally stacking onto an expiring Cheap Shot), which
    /// must bypass the "already stunned" guard.
    Fire { target: Entity, pos: Vec3, stacking: bool },
}

/// Spellcasting classes — those whose primary threat is interruptible casts, so
/// the Rogue reserves Kidney Shot for the Kick → hold → Kidney denial chain
/// against them rather than spending it opportunistically. Warrior/Rogue/Hunter
/// are excluded: they have no (or only one, low-value) cast, so the Rogue stuns
/// them aggressively whenever it can. (Hunter's sole cast is Aimed Shot, and a
/// Physical-school lockout wouldn't even stop its auto/instant shots.)
fn is_spellcaster(class: CharacterClass) -> bool {
    matches!(
        class,
        CharacterClass::Mage
            | CharacterClass::Priest
            | CharacterClass::Warlock
            | CharacterClass::Paladin
    )
}

/// Build a `PreconditionUnmet` rejection with a static note.
fn hold_reason(note: &'static str) -> RejectionReason {
    RejectionReason::PreconditionUnmet { note: note.into() }
}

/// Remaining duration of the longest-lasting stun this Rogue applied to `target`
/// — i.e. when the Rogue's own stun lockdown on the target ends. `None` if the
/// Rogue has no active stun on the target. Used to time the opener Cheap Shot →
/// Kidney Shot chain.
fn my_stun_lockdown_remaining(ctx: &CombatContext, target: Entity, me: Entity) -> Option<f32> {
    ctx.active_auras
        .get(&target)?
        .iter()
        .filter(|a| a.effect_type == AuraType::Stun && a.caster == Some(me))
        .map(|a| a.duration)
        .max_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
}

/// Active spell-school lockout on `target`: `(remaining_seconds, locked_school)`.
/// `None` if the target has no lockout. The locked school is decoded from the
/// aura's `magnitude` (see `SpellSchool::from_lockout_magnitude`).
fn lockout_on(ctx: &CombatContext, target: Entity) -> Option<(f32, SpellSchool)> {
    ctx.active_auras
        .get(&target)?
        .iter()
        .find(|a| a.effect_type == AuraType::SpellSchoolLockout)
        .map(|a| (a.duration, SpellSchool::from_lockout_magnitude(a.magnitude)))
}

/// Decide what to do with Kidney Shot this tick. See module docs for the policy:
/// opener-extend on the kill target, the Kick→hold→Kidney chain against caster
/// classes, and aggressive opportunistic stuns against everyone else.
fn plan_kidney_shot(
    entity: Entity,
    combatant: &Combatant,
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    my_pos: Vec3,
) -> KidneyPlan {
    let kidney_cost = abilities.get_unchecked(&AbilityType::KidneyShot).mana_cost;
    let energy = combatant.current_mana;
    let kidney_on_cd = combatant
        .ability_cooldowns
        .contains_key(&AbilityType::KidneyShot);

    // 1. OPENER-EXTEND: keep the kill target locked down by chaining Kidney Shot
    //    onto an expiring Cheap Shot (the default opener). Kidney Shot has its
    //    own DR category, so the two stuns don't diminish — a clean ~10s window.
    if let Some(kill) = combatant.target {
        if let Some(info) = ctx.combatants.get(&kill) {
            let in_melee = my_pos.distance(info.position) <= MELEE_RANGE;
            if in_melee && !ctx.entity_is_immune(kill) && !kidney_on_cd {
                if let Some(rem) = my_stun_lockdown_remaining(ctx, kill, entity) {
                    if rem <= KIDNEY_CHAIN_BUFFER {
                        return KidneyPlan::Fire { target: kill, pos: info.position, stacking: true };
                    }
                    // Pool through the opener stun so Kidney is guaranteed ready
                    // at its expiry — a one-time ~3.5s of held Sinister Strikes is
                    // worth the airtight 10s lockdown.
                    return KidneyPlan::Pool(hold_reason("pooling Kidney to extend opener Cheap Shot"));
                }
            }
        }
    }

    // 2. Standard in-combat melee CC target: the Cheap Shot healer (cc_target)
    //    if it's in melee, else the kill target.
    let (tgt, tgt_pos) = match select_melee_cc_target(
        combatant.cc_target,
        combatant.target,
        my_pos,
        ctx,
    ) {
        Some(x) => x,
        None => return KidneyPlan::NoTarget,
    };
    let tgt_class = match ctx.combatants.get(&tgt) {
        Some(i) => i.class,
        None => return KidneyPlan::NoTarget,
    };

    // 3. Non-caster (Warrior/Rogue/Hunter): aggressive opportunistic stun.
    if !is_spellcaster(tgt_class) {
        return KidneyPlan::Fire { target: tgt, pos: tgt_pos, stacking: false };
    }

    // 4. Caster (Mage/Priest/Warlock/Paladin): the Kick → hold → Kidney chain.
    let lockout = lockout_on(ctx, tgt);
    let casting_school = ctx
        .combatants
        .get(&tgt)
        .and_then(|i| i.casting_ability)
        .map(|ab| abilities.get_unchecked(&ab).spell_school);
    let kick_ready = !combatant.ability_cooldowns.contains_key(&AbilityType::Kick)
        && my_pos.distance(tgt_pos) <= MELEE_RANGE;

    if let Some(cast_school) = casting_school {
        // Target is casting right now.
        let covered = lockout.is_some_and(|(_, locked)| locked == cast_school);
        if covered {
            // The active lockout already covers this school — nothing to add.
            return KidneyPlan::Hold(hold_reason("Kidney held: school lockout already covers this cast"));
        }
        if kick_ready {
            // Kick runs later this same frame and will interrupt this cast — don't
            // double-spend the 30s stun on a cast Kick handles for free.
            return KidneyPlan::Hold(hold_reason("Kidney held: Kick will interrupt this cast"));
        }
        // Unlocked second-school cast and Kick unavailable: the stun is the only
        // denial left, so spend it.
        return KidneyPlan::Fire { target: tgt, pos: tgt_pos, stacking: false };
    }

    // Target not casting.
    if let Some((rem, _)) = lockout {
        if rem <= KIDNEY_CHAIN_BUFFER {
            // Lockout lapsing — extend the denial with the stun.
            return KidneyPlan::Fire { target: tgt, pos: tgt_pos, stacking: false };
        }
        // Lockout still denying. Pool energy if a Kidney isn't yet affordable so
        // it's ready the instant the lockout reaches the chain buffer.
        if !kidney_on_cd && energy < kidney_cost {
            return KidneyPlan::Pool(hold_reason("pooling Kidney to extend school lockout"));
        }
        return KidneyPlan::Hold(hold_reason("Kidney held: school lockout still active"));
    }

    // No active lockout and not casting: do NOT sit on the stun waiting for a
    // chain that may never start. A kiting healer is only briefly in melee and
    // rarely casting at that instant, so an indefinite "wait for Kick first"
    // hold wastes Kidney's 30s cooldown entirely (measured: a Rogue+healer comp
    // cast Kidney once in a 283s match and cratered 91% -> 14% vs Warrior+Priest).
    // Stun proactively instead — Kidney-then-Kick denies just as long as
    // Kick-then-Kidney (6+4 == 4+6) and guarantees the stun lands while the
    // target is reachable. The chain still extends an ACTIVE lockout (above) and
    // still avoids double-spending on a cast Kick will catch (the kick_ready
    // hold above); it just no longer reserves the stun pre-emptively.
    KidneyPlan::Fire { target: tgt, pos: tgt_pos, stacking: false }
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
