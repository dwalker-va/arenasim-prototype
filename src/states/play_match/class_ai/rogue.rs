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
//!
//! Expose Armor (the Combat capstone, a talent) goes between the two: below
//! the Kidney Shot planner, so it never takes the stun's moment or the energy
//! pooled for it, and ahead of Sinister Strike. While a burst is called it goes
//! on the kill window's enemy ahead of the burst instead. Either way, only when
//! halving the target's armor makes a Physical hit land at least
//! [`EXPOSE_ARMOR_MIN_GAIN`] harder ([`try_expose_armor`]).
//!
//! Rupture (the Assassination tree's bleed, a talent) goes in the same place:
//! below the Kidney Shot planner and ahead of Sinister Strike, on the kill
//! target, whenever this Rogue's own Rupture is not already on it — so it
//! keeps rolling as its cooldown allows ([`try_rupture`]). The two talents
//! sit in different trees and no legal build holds both.
//!
//! Hemorrhage (the Subtlety tree's strike, a talent) takes Sinister Strike's
//! place wherever it is struck while its debuff on the target is down or low
//! ([`hemorrhage_wanted`]); otherwise Sinister Strike goes as before.
//!
//! Shadow Dance (the Subtlety capstone) is spent only when the stun chain it
//! opens can run ([`try_shadow_dance`]): the kill target in melee and held by
//! nothing, its stun diminishing returns clear, Kidney Shot ready by the time
//! the two Cheap Shots run out, the enemy healer unable to answer, and the
//! energy the chain needs pooled ([`dance_chain`]). While it lasts the Rogue
//! runs the chain ([`dance_turn`]) — Cheap Shot, Cheap Shot at half, then the
//! Kidney Shot planner's opener-extend — and strikes with Ambush only with
//! energy to spare for the next stun. Both sit just below the Peel play: a
//! teammate in danger comes first, and the dance's own chain is the Rogue's
//! lock, ahead of the Chain play's hand-offs onto a teammate's.
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::match_config::{CharacterClass, RogueOpener};
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::ability_config::AbilityConfig;
use crate::states::play_match::combat_core::{
    armor_met, armor_reduction, get_attack_power_bonus_from_slice,
    get_crit_chance_bonus_from_slice, roll_crit,
};
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRIT_DAMAGE_MULTIPLIER, DR_MULTIPLIERS, GCD, MELEE_RANGE,
};
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, NoActionReason, PlayStep, RejectionReason,
};
use crate::states::play_match::situation::Role;
use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::cast_guard::{cast_gate, classify_pre_cast_failure, PreCastOpts};
use super::CombatContext;

/// Rogue AI: Decides and executes abilities for a Rogue combatant.
pub fn decide_rogue_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
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

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, Some(target_entity), my_pos)
    else {
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
            ),
            RogueOpener::CheapShot => try_cheap_shot(
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
                same_frame_cc_queue,
                &mut builder,
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

    // The Peel play (TeamPlan): a teammate is in danger, so Kidney Shot on
    // its attacker heads the chain, ahead of the chain planner's own use of
    // the stun.
    if let Some(call) = super::peel::peel_call(ctx, Role::MeleeDps) {
        let peeled = call.attackers.iter().any(|&attacker| {
            let Some(pos) = ctx.combatants.get(&attacker).map(|i| i.position) else {
                return false;
            };
            if ctx.is_dr_immune(attacker, DRCategory::KidneyShotStun) {
                builder.reject(
                    AbilityType::KidneyShot,
                    RejectionReason::DRImmune {
                        category: DRCategory::KidneyShotStun,
                    },
                );
                return false;
            }
            if ctx.entity_is_immune(attacker) {
                builder.reject(AbilityType::KidneyShot, RejectionReason::TargetImmune);
                return false;
            }
            try_kidney_shot(
                commands,
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                attacker,
                pos,
                ctx,
                same_frame_cc_queue,
                &mut builder,
            )
        });
        if peeled {
            builder.play_step(PlayStep::Peel);
            builder.finish();
            return true;
        }
    }

    // Shadow Dance (a talent): while it lasts, its stun chain on the kill
    // target; before it, the moment to spend it. A Rogue without the talent
    // has no rule here.
    if abilities.get(&AbilityType::ShadowDance).is_some() {
        if let Some(acted) = dance_turn(
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
            same_frame_cc_queue,
            &mut builder,
        ) {
            builder.finish();
            return acted;
        }
        match try_shadow_dance(
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
            DanceCall::Danced => {
                builder.finish();
                return true;
            }
            DanceCall::Pool => {
                reject_strikes(abilities, &mut builder, "pooling energy for Shadow Dance");
                builder.finish();
                return false;
            }
            DanceCall::Not => {}
        }
    }

    // The Chain play (TeamPlan): a lock the team holds on an enemy is about to
    // end, so Kidney Shot hands off onto it, landing in its last moments — or
    // the global cooldown (and the energy) is held for that moment.
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
        let handed = plan.due.contains(&AbilityType::KidneyShot)
            && ctx
                .combatants
                .get(&call.subject)
                .map(|i| i.position)
                .is_some_and(|pos| {
                    try_kidney_shot(
                        commands,
                        combat_log,
                        game_rng,
                        abilities,
                        entity,
                        combatant,
                        my_pos,
                        auras,
                        call.subject,
                        pos,
                        ctx,
                        same_frame_cc_queue,
                        &mut builder,
                    )
                });
        if handed || plan.hold {
            builder.play_step(PlayStep::Chain);
            builder.finish();
            return handed;
        }
    }

    // Expose Armor (a talent). A Rogue without the talent has no rule here.
    let has_expose_armor = abilities.get(&AbilityType::ExposeArmor).is_some();

    // The Burst play (TeamPlan): a kill window is open, so Sinister Strike on
    // its enemy heads the chain — ahead of the Kidney Shot planner, whose holds
    // and pooling would otherwise keep the energy back. Expose Armor on that
    // enemy goes first, ahead of the burst it opens up.
    let burst = super::burst::burst_call(ctx);
    if let Some(call) = burst.as_ref() {
        if let Some(pos) = ctx.combatants.get(&call.subject).map(|i| i.position) {
            if has_expose_armor
                && try_expose_armor(
                    commands,
                    combat_log,
                    game_rng,
                    abilities,
                    entity,
                    combatant,
                    my_pos,
                    auras,
                    call.subject,
                    ctx,
                    &mut builder,
                )
            {
                builder.finish();
                return true;
            }
            if try_strike(
                commands,
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                call.subject,
                pos,
                ctx,
                instant_attacks,
                &mut builder,
            ) {
                builder.play_step(PlayStep::Burst);
                builder.finish();
                return true;
            }
        }
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
            reject_strikes(
                abilities,
                &mut builder,
                "pooling energy for Kidney Shot chain",
            );
            builder.finish();
            return false;
        }
        KidneyPlan::Fire {
            target: ks_target_entity,
            pos: ks_target_pos,
        } => {
            // A target already held is refused by the cast gate, unless its
            // hold is inside the chain window — where the opener-extend
            // chains Kidney onto its own expiring Cheap Shot.
            if ctx.is_dr_immune(ks_target_entity, DRCategory::KidneyShotStun) {
                builder.reject(
                    kidney_shot,
                    RejectionReason::DRImmune {
                        category: DRCategory::KidneyShotStun,
                    },
                );
            } else if try_kidney_shot(
                commands,
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ks_target_entity,
                ks_target_pos,
                ctx,
                same_frame_cc_queue,
                &mut builder,
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
                    kidney_shot,
                    ks_def,
                    combatant,
                    my_pos,
                    auras,
                    Some((ks_target_entity, ks_target_pos)),
                    ctx,
                    PreCastOpts::default(),
                );
                if matches!(reason, RejectionReason::InsufficientResource { .. }) {
                    reject_strikes(abilities, &mut builder, "pooling energy for Kidney Shot");
                    builder.finish();
                    return false;
                }
            }
        }
    }

    // Expose Armor on the kill target, BELOW the Kidney Shot planner: it never
    // takes a moment the planner fires the stun in, nor energy it is pooling
    // (each of those returned above). Under a burst it was decided there.
    if has_expose_armor
        && burst.is_none()
        && try_expose_armor(
            commands,
            combat_log,
            game_rng,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            target_entity,
            ctx,
            &mut builder,
        )
    {
        builder.finish();
        return true;
    }

    // Rupture on the kill target, in the same place and for the same reason:
    // it never takes the stun's moment or the energy pooled for it.
    if abilities.get(&AbilityType::Rupture).is_some()
        && try_rupture(
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
            &mut builder,
        )
    {
        builder.finish();
        return true;
    }

    // Priority 2: Sinister Strike, or Hemorrhage in its place
    let acted = try_strike(
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
    );
    builder.finish();
    acted
}

/// Buffer (seconds) before a control window expires at which the Rogue chains
/// the next stun. Firing this early trades ~0.5s of the lockdown for safety
/// against the target dying / going immune / leaving melee in the final moment.
/// It IS the chain window ([`super::CC_CHAIN_WINDOW`]): the cast gate refuses a
/// stun on a target still held for longer, so the chain fires the moment the
/// gate lets it.
const KIDNEY_CHAIN_BUFFER: f32 = super::CC_CHAIN_WINDOW;

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
    /// Fire Kidney Shot now on `target`.
    Fire { target: Entity, pos: Vec3 },
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
    abilities: &AbilityBook,
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
                        return KidneyPlan::Fire {
                            target: kill,
                            pos: info.position,
                        };
                    }
                    // Pool through the opener stun so Kidney is guaranteed ready
                    // at its expiry — a one-time ~3.5s of held Sinister Strikes is
                    // worth the airtight 10s lockdown.
                    return KidneyPlan::Pool(hold_reason(
                        "pooling Kidney to extend opener Cheap Shot",
                    ));
                }
            }
        }
    }

    // 2. Standard in-combat melee CC target: the Cheap Shot healer (cc_target)
    //    if it's in melee, else the kill target.
    let (tgt, tgt_pos) =
        match select_melee_cc_target(combatant.cc_target, combatant.target, my_pos, ctx) {
            Some(x) => x,
            None => return KidneyPlan::NoTarget,
        };
    let tgt_class = match ctx.combatants.get(&tgt) {
        Some(i) => i.class,
        None => return KidneyPlan::NoTarget,
    };

    // 3. Non-caster (Warrior/Rogue/Hunter): aggressive opportunistic stun.
    if !is_spellcaster(tgt_class) {
        return KidneyPlan::Fire {
            target: tgt,
            pos: tgt_pos,
        };
    }

    // 4. Caster (Mage/Priest/Warlock/Paladin): the Kick → hold → Kidney chain.
    let lockout = lockout_on(ctx, tgt);
    // The target's cast, read from the TARGET's own book.
    let casting = ctx
        .combatants
        .get(&tgt)
        .and_then(|i| i.casting_ability.map(|ab| i.book.get_unchecked(&ab)));
    let casting_school = casting.map(|def| def.spell_school);
    // Kick takes the cast only if it can be interrupted; against one that
    // is, the stun is the only denial there is.
    let kick_ready = !combatant.ability_cooldowns.contains_key(&AbilityType::Kick)
        && my_pos.distance(tgt_pos) <= MELEE_RANGE
        && casting.is_none_or(|def| !def.is_uninterruptible_cast());

    if let Some(cast_school) = casting_school {
        // Target is casting right now.
        let covered = lockout.is_some_and(|(_, locked)| locked == cast_school);
        if covered {
            // The active lockout already covers this school — nothing to add.
            return KidneyPlan::Hold(hold_reason(
                "Kidney held: school lockout already covers this cast",
            ));
        }
        if kick_ready {
            // Kick runs later this same frame and will interrupt this cast — don't
            // double-spend the 30s stun on a cast Kick handles for free.
            return KidneyPlan::Hold(hold_reason("Kidney held: Kick will interrupt this cast"));
        }
        // Unlocked second-school cast and Kick unavailable: the stun is the only
        // denial left, so spend it.
        return KidneyPlan::Fire {
            target: tgt,
            pos: tgt_pos,
        };
    }

    // Target not casting.
    if let Some((rem, _)) = lockout {
        if rem <= KIDNEY_CHAIN_BUFFER {
            // Lockout lapsing — extend the denial with the stun.
            return KidneyPlan::Fire {
                target: tgt,
                pos: tgt_pos,
            };
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
    KidneyPlan::Fire {
        target: tgt,
        pos: tgt_pos,
    }
}

/// Try to use Ambush from stealth.
fn try_ambush(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
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
    let ability = AbilityType::Ambush;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts {
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

    builder.choose(cleared, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.stealthed = false;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Ambush",
        target_tuple,
        "uses",
    );

    // A dodged or parried Ambush deals nothing; its energy is spent and the
    // Rogue is out of stealth.
    if ctx.melee_attack_avoided(commands, game_rng, target_entity, &def.name) {
        return true;
    }

    let self_auras = ctx
        .active_auras
        .get(&entity)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, ap_bonus, 0.0);
    let is_crit = roll_crit(
        combatant.crit_chance + crit_bonus + def.crit_chance,
        game_rng,
    );
    if is_crit {
        damage *= CRIT_DAMAGE_MULTIPLIER;
    }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_slot: combatant.slot,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });

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
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::CheapShot;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts::default();
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

    builder.choose(cleared, Some(target_entity), true);

    // Caster-side gesture marker (A2) — see the note in `cast_hammer_of_justice`
    // for the spawn contract. Cheap Shot is instant AND aura-only, so without
    // this the Rogue plays nothing at all while stunning someone.
    commands.spawn((
        InstantAbilityFired {
            caster: entity,
            target: Some(target_entity),
            ability,
            is_crit: false,
        },
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.stealthed = false;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Cheap Shot",
        target_tuple,
        "uses",
    );

    // A dodged or parried Cheap Shot stuns nobody; what it cost is spent.
    if ctx.melee_attack_avoided(commands, game_rng, target_entity, &def.name) {
        return true;
    }

    if let Some(aura) = def.applies_aura.as_ref() {
        if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, def) {
            same_frame_cc_queue.push((target_entity, aura_pending.aura.clone()));
            commands.spawn(aura_pending);
        }

        if let Some(info) = ctx.combatants.get(&target_entity) {
            let cc_type = format!("{:?}", aura.aura_type);
            let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
            let target_id = info.log_id();
            let message = format!("{} uses {} on {}", caster_id, def.name, target_id);
            combat_log.log_crowd_control(
                caster_id.clone(),
                target_id,
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
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let kidney_shot = AbilityType::KidneyShot;
    let def = abilities.get_unchecked(&kidney_shot);

    let opts = PreCastOpts::default();
    let cleared = match cast_gate(
        kidney_shot,
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
            builder.reject(kidney_shot, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), true);

    // Caster-side gesture marker (A2) — see the note in `cast_hammer_of_justice`
    // for the spawn contract.
    commands.spawn((
        InstantAbilityFired {
            caster: entity,
            target: Some(target_entity),
            ability: kidney_shot,
            is_crit: false,
        },
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(kidney_shot);
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Kidney Shot",
        target_tuple,
        "uses",
    );

    // A dodged or parried Kidney Shot stuns nobody; what it cost is spent.
    if ctx.melee_attack_avoided(commands, game_rng, target_entity, &def.name) {
        return true;
    }

    if let Some(aura) = def.applies_aura.as_ref() {
        if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, def) {
            same_frame_cc_queue.push((target_entity, aura_pending.aura.clone()));
            commands.spawn(aura_pending);
        }

        if let Some(info) = ctx.combatants.get(&target_entity) {
            let cc_type = format!("{:?}", aura.aura_type);
            let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
            let target_id = info.log_id();
            let message = format!("{} uses {} on {}", caster_id, def.name, target_id);
            combat_log.log_crowd_control(
                caster_id.clone(),
                target_id,
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

/// The least Expose Armor must add to a Physical hit on its target for the
/// Rogue to spend it there: 8%. Halving the starting kit's armor adds about
/// 10% on mail and 17-20% on plate, against 6.5% on leather and 3% on cloth,
/// so it goes on mail and plate and not on the rest.
pub const EXPOSE_ARMOR_MIN_GAIN: f32 = 0.08;

/// How much harder a Physical hit lands on a target with `armor` wearing
/// `auras` once Expose Armor (`def`) is on it too, as a fraction: `0.17` is
/// 17% more damage. Read through the damage pipeline's own armor rule
/// (`combat_core::armor_met`), so an Expose (or any stronger reduction)
/// already on the target makes another worth nothing.
pub fn expose_armor_gain(armor: f32, auras: &[Aura], def: &AbilityConfig) -> f32 {
    let Some(effect) = def.applies_aura.as_ref() else {
        return 0.0;
    };
    let before = armor_met(armor, auras);
    let mut exposed = auras.to_vec();
    exposed.push(Aura {
        effect_type: effect.aura_type,
        magnitude: effect.magnitude,
        ..Default::default()
    });
    let after = armor_met(armor, &exposed);
    (1.0 - armor_reduction(after)) / (1.0 - armor_reduction(before)) - 1.0
}

/// Try to use Expose Armor on `target_entity`: only while it is worth
/// [`EXPOSE_ARMOR_MIN_GAIN`] ([`expose_armor_gain`]). A melee attack: dodged
/// or parried, it applies nothing and its cost is spent.
fn try_expose_armor(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::ExposeArmor;
    let def = abilities.get_unchecked(&ability);
    let Some(info) = ctx.combatants.get(&target_entity) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_auras = ctx
        .active_auras
        .get(&target_entity)
        .map_or(&[][..], Vec::as_slice);
    if expose_armor_gain(info.armor, target_auras, def) < EXPOSE_ARMOR_MIN_GAIN {
        let exposed = def.applies_aura.as_ref().is_some_and(|effect| {
            target_auras
                .iter()
                .any(|a| a.effect_type == effect.aura_type && a.magnitude >= effect.magnitude)
        });
        builder.reject(
            ability,
            hold_reason(if exposed {
                "target's armor is already exposed"
            } else {
                "target's armor is too light to expose"
            }),
        );
        return false;
    }

    let opts = PreCastOpts {
        check_target_immune: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, info.position)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(info.log_id()),
        "uses",
    );

    // A dodged or parried Expose Armor exposes nothing; its energy is spent.
    if ctx.melee_attack_avoided(commands, game_rng, target_entity, &def.name) {
        return true;
    }

    if let Some(pending) = AuraPending::from_ability(target_entity, entity, def) {
        commands.spawn(pending);
    }
    true
}

/// Try to use Rupture on `target_entity`, keeping it rolling: only while
/// this Rogue's own Rupture is not on the target. Never on a target the team
/// holds in crowd control a hit breaks (its ticks would break it). A melee
/// attack: dodged or parried, it applies nothing and its cost is spent.
fn try_rupture(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Rupture;
    let def = abilities.get_unchecked(&ability);
    let mine_on_target = ctx.active_auras.get(&target_entity).is_some_and(|list| {
        list.iter()
            .any(|a| a.caster == Some(entity) && a.ability_name == def.name)
    });
    if mine_on_target {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let opts = PreCastOpts {
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

    builder.choose(cleared, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    combatant.global_cooldown = GCD;

    let target_id = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_id,
        "uses",
    );

    // A dodged or parried Rupture opens no bleed; its energy is spent.
    if ctx.melee_attack_avoided(commands, game_rng, target_entity, &def.name) {
        return true;
    }

    if let Some(pending) = AuraPending::from_ability(target_entity, entity, def) {
        commands.spawn(pending);
    }
    true
}

/// Try to use Sinister Strike.
fn try_sinister_strike(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
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
    let ability = AbilityType::SinisterStrike;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts {
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

    builder.choose(cleared, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Sinister Strike",
        target_tuple,
        "uses",
    );

    // A dodged or parried Sinister Strike deals nothing; its energy is spent.
    if ctx.melee_attack_avoided(commands, game_rng, target_entity, &def.name) {
        return true;
    }

    let self_auras = ctx
        .active_auras
        .get(&entity)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, ap_bonus, 0.0);
    let is_crit = roll_crit(
        combatant.crit_chance + crit_bonus + def.crit_chance,
        game_rng,
    );
    if is_crit {
        damage *= CRIT_DAMAGE_MULTIPLIER;
    }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_slot: combatant.slot,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });

    info!(
        "Team {} {} uses {}!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Reject the Rogue's strikes — Sinister Strike, and Hemorrhage when it has
/// it — for `note`: the turn pools its energy for something better.
fn reject_strikes(abilities: &AbilityBook, builder: &mut DecisionEventBuilder<'_>, note: &str) {
    for ability in [AbilityType::SinisterStrike, AbilityType::Hemorrhage] {
        if abilities.get(&ability).is_some() {
            builder.reject(
                ability,
                RejectionReason::PreconditionUnmet { note: note.into() },
            );
        }
    }
}

/// How little of Hemorrhage's debuff may be left before the Rogue strikes
/// with Hemorrhage again rather than Sinister Strike: two global cooldowns,
/// so the refresh lands before it runs out.
pub const HEMORRHAGE_REFRESH_SECS: f32 = 2.0 * GCD;

/// Whether Hemorrhage's debuff (`def`'s aura) on `target` is down or low:
/// absent, short of its full stacks, or within [`HEMORRHAGE_REFRESH_SECS`]
/// of running out. Anyone's counts: one Hemorrhage debuff stacks per target.
pub fn hemorrhage_wanted(ctx: &CombatContext, target: Entity, def: &AbilityConfig) -> bool {
    let Some(effect) = def.applies_aura.as_ref() else {
        return false;
    };
    let on_target = ctx.active_auras.get(&target).and_then(|list| {
        list.iter()
            .find(|a| a.effect_type == effect.aura_type && a.ability_name == def.name)
    });
    match on_target {
        None => true,
        Some(aura) => {
            aura.stacks.is_some_and(|s| s.count < s.max) || aura.duration <= HEMORRHAGE_REFRESH_SECS
        }
    }
}

/// The Rogue's strike on `target_entity`: Hemorrhage, when the Rogue has it
/// and its debuff there is down or low ([`hemorrhage_wanted`]); otherwise
/// Sinister Strike.
fn try_strike(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
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
    if let Some(def) = abilities.get(&AbilityType::Hemorrhage) {
        if !hemorrhage_wanted(ctx, target_entity, def) {
            builder.reject(
                AbilityType::Hemorrhage,
                hold_reason("Hemorrhage is up on the target"),
            );
        } else if try_hemorrhage(
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
            builder,
        ) {
            return true;
        }
    }
    try_sinister_strike(
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
        builder,
    )
}

/// Try to use Hemorrhage: Sinister Strike's blow, and its debuff on the
/// target. A melee attack: dodged or parried, it deals and applies nothing,
/// and its cost is spent.
fn try_hemorrhage(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
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
    let ability = AbilityType::Hemorrhage;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts {
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

    builder.choose(cleared, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_id = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_id,
        "uses",
    );

    // A dodged or parried Hemorrhage deals nothing and opens no wound.
    if ctx.melee_attack_avoided(commands, game_rng, target_entity, &def.name) {
        return true;
    }

    let self_auras = ctx
        .active_auras
        .get(&entity)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, ap_bonus, 0.0);
    let is_crit = roll_crit(
        combatant.crit_chance + crit_bonus + def.crit_chance,
        game_rng,
    );
    if is_crit {
        damage *= CRIT_DAMAGE_MULTIPLIER;
    }
    instant_attacks.push(super::QueuedInstantAttack {
        attacker: entity,
        target: target_entity,
        damage,
        attacker_team: combatant.team,
        attacker_slot: combatant.slot,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });
    if let Some(pending) = AuraPending::from_ability(target_entity, entity, def) {
        commands.spawn(pending);
    }
    true
}

/// What one Shadow Dance's stun chain asks of a fresh target: Cheap Shot, a
/// second Cheap Shot (the stun diminishing returns halve it), then Kidney
/// Shot (its own bucket), each going on as the stun before it reaches
/// [`super::CC_CHAIN_WINDOW`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DanceChain {
    /// Seconds into the chain at which Kidney Shot goes on.
    pub kidney_at: f32,
    /// The energy the Rogue needs at the start for each stun to be
    /// affordable when it is due, at its regeneration: never more than its
    /// bar holds.
    pub energy: f32,
}

/// The chain of a Shadow Dance by a Rogue with these Cheap Shot and Kidney
/// Shot configs, `regen` energy a second and a bar of `max_energy`. With the
/// shipped kit (60 and 60, 20 a second): the second Cheap Shot at 3.5s,
/// Kidney Shot at 5s, and 80 energy to start.
pub fn dance_chain(
    cheap_shot: &AbilityConfig,
    kidney_shot: &AbilityConfig,
    regen: f32,
    max_energy: f32,
) -> DanceChain {
    let window = super::CC_CHAIN_WINDOW;
    let stun = cheap_shot.applies_aura.as_ref().map_or(0.0, |a| a.duration);
    let second_at = (stun - window).max(0.0);
    let kidney_at = second_at + (stun * DR_MULTIPLIERS[1] - window).max(0.0);
    let cheap = cheap_shot.mana_cost;
    let energy = [
        (0.0, cheap),
        (second_at, 2.0 * cheap),
        (kidney_at, 2.0 * cheap + kidney_shot.mana_cost),
    ]
    .into_iter()
    .map(|(at, spent)| spent - regen * at)
    .fold(0.0_f32, f32::max);
    DanceChain {
        kidney_at,
        energy: energy.min(max_energy),
    }
}

/// Whether the Rogue is dancing: its Shadow Dance aura is up.
fn dancing(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|x| x.effect_type == AuraType::ShadowDance)
    })
}

/// Whether an enemy healer could answer a stun chain on `target` now: a
/// living enemy healer other than `target` (a chain on the healer itself
/// locks it) that is not the one the team holds locked, while `target` is
/// not the team's kill window (low, and beyond every enemy healer's help).
fn enemy_healer_answers(ctx: &CombatContext, my_team: u8, target: Entity) -> bool {
    let situation = ctx.situation();
    if situation.kill_window.is_some_and(|w| w.subject == target) {
        return false;
    }
    let locked = situation.enemy_healer_locked.map(|w| w.subject);
    ctx.combatants.values().any(|i| {
        i.team != my_team
            && i.is_alive
            && !i.is_pet
            && i.is_healer()
            && i.entity != target
            && Some(i.entity) != locked
    })
}

/// What [`try_shadow_dance`] decided.
enum DanceCall {
    /// Shadow Dance was used.
    Danced,
    /// Everything else is ready: hold the turn's energy for the chain.
    Pool,
    /// Not this turn.
    Not,
}

/// Try to use Shadow Dance on the kill target `target`: only when the stun
/// chain it opens can run. The target in melee, not immune (the decision's
/// head refuses an immune one) and held by nothing; its stun diminishing
/// returns, both buckets, clear; Kidney Shot ready by the time the two Cheap
/// Shots run out; the enemy healer unable to answer
/// ([`enemy_healer_answers`]: none standing but the target itself, or the
/// one standing locked, or the target the team's kill window); and the
/// energy the chain needs
/// ([`dance_chain`]) — the one condition the Rogue pools for. Never on its
/// cooldown alone. Free and off the global cooldown: the chain's first Cheap
/// Shot follows on the next turn.
fn try_shadow_dance(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> DanceCall {
    let ability = AbilityType::ShadowDance;
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
            return DanceCall::Not;
        }
    };
    let chain = dance_chain(
        abilities.get_unchecked(&AbilityType::CheapShot),
        abilities.get_unchecked(&AbilityType::KidneyShot),
        combatant.mana_regen,
        combatant.max_mana,
    );
    let kidney_wait = combatant
        .ability_cooldowns
        .get(&AbilityType::KidneyShot)
        .copied()
        .unwrap_or(0.0);
    let unmet = if my_pos.distance(target_pos) > MELEE_RANGE {
        Some("the kill target is out of melee")
    } else if ctx.held_in_hard_cc(target).is_some() {
        Some("the kill target is already held")
    } else if ctx.dr_level(target, DRCategory::Stuns) > 0
        || ctx.dr_level(target, DRCategory::KidneyShotStun) > 0
    {
        Some("the kill target's stun diminishing returns are not clear")
    } else if kidney_wait > chain.kidney_at {
        Some("Kidney Shot would not be ready for the chain")
    } else if enemy_healer_answers(ctx, combatant.team, target) {
        Some("the enemy healer is free to answer the chain")
    } else {
        None
    };
    if let Some(note) = unmet {
        builder.reject(ability, hold_reason(note));
        return DanceCall::Not;
    }
    if combatant.current_mana < chain.energy {
        builder.reject(
            ability,
            hold_reason("pooling energy for the Shadow Dance chain"),
        );
        return DanceCall::Pool;
    }

    builder.choose(cleared, Some(entity), true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        None,
        "uses",
    );
    if let Some(pending) = AuraPending::from_ability(entity, entity, def) {
        commands.spawn(pending);
    }
    DanceCall::Danced
}

/// Seconds left on the longest hold ([`super::holds`]) on `target`, `0.0`
/// for none.
fn hold_remaining(ctx: &CombatContext, target: Entity) -> f32 {
    ctx.active_auras.get(&target).map_or(0.0, |list| {
        list.iter()
            .filter(|a| super::holds(a.effect_type))
            .map(|a| a.duration)
            .fold(0.0, f32::max)
    })
}

/// One turn of a Shadow Dance in progress, on the kill target `target`.
/// `None` when the Rogue is not dancing, or the dance has nothing to say
/// this turn (out of melee, or the target in its team's breakable crowd
/// control), so the ordinary rotation runs.
///
/// While a Cheap Shot still lands for half or more (the stun diminishing
/// returns have seen at most one), the chain is Cheap Shot: onto a target
/// held by nothing, or one whose hold is in its last
/// [`super::CC_CHAIN_WINDOW`]. While the hold runs, Ambush goes only with
/// energy to spare for that Cheap Shot when it is due, and the turn pools
/// otherwise. After the second Cheap Shot, Kidney Shot is the planner's
/// opener-extend (`plan_kidney_shot`); once it is spent, Ambush is the
/// dance's strike.
fn dance_turn(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> Option<bool> {
    if !dancing(auras)
        || my_pos.distance(target_pos) > MELEE_RANGE
        || ctx.has_friendly_breakable_cc(target)
    {
        return None;
    }
    let cheap_cost = abilities.get_unchecked(&AbilityType::CheapShot).mana_cost;
    let ambush_cost = abilities.get_unchecked(&AbilityType::Ambush).mana_cost;
    let pool = |builder: &mut DecisionEventBuilder<'_>, note: &'static str| {
        builder.reject(AbilityType::Ambush, hold_reason(note));
        reject_strikes(abilities, builder, note);
        Some(false)
    };

    if ctx.dr_level(target, DRCategory::Stuns) <= 1 {
        let held = hold_remaining(ctx, target);
        if held > super::CC_CHAIN_WINDOW {
            // The energy left when the next Cheap Shot is due, after it.
            let due_in = held - super::CC_CHAIN_WINDOW;
            let spare = combatant.current_mana + combatant.mana_regen * due_in - cheap_cost;
            if spare >= ambush_cost
                && try_ambush(
                    commands,
                    combat_log,
                    game_rng,
                    abilities,
                    entity,
                    combatant,
                    my_pos,
                    auras,
                    target,
                    target_pos,
                    ctx,
                    instant_attacks,
                    builder,
                )
            {
                return Some(true);
            }
            return pool(builder, "pooling energy for the next Cheap Shot");
        }
        if try_cheap_shot(
            commands,
            combat_log,
            game_rng,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            target,
            target_pos,
            ctx,
            same_frame_cc_queue,
            builder,
        ) {
            return Some(true);
        }
        if combatant.current_mana < cheap_cost {
            return pool(builder, "pooling energy for Cheap Shot");
        }
        return None;
    }

    // The Cheap Shots are spent: Kidney Shot, while it is still to come, is
    // the planner's.
    let kidney_to_come = !combatant
        .ability_cooldowns
        .contains_key(&AbilityType::KidneyShot)
        && !ctx.is_dr_immune(target, DRCategory::KidneyShotStun);
    if kidney_to_come {
        return None;
    }
    try_ambush(
        commands,
        combat_log,
        game_rng,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        target,
        target_pos,
        ctx,
        instant_attacks,
        builder,
    )
    .then_some(true)
}

/// Select the best target for melee-range CC abilities.
///
/// For melee CC like Kidney Shot, we need range-aware targeting:
/// 1. If CC target is in melee range, use it (strategic CC on healer)
/// 2. If CC target is out of range but kill target is in range, fall back to kill target
/// 3. If neither is in range, return None
///
/// A candidate already held ([`CombatContext::held_in_hard_cc`]) is passed
/// over for the next, as one immune is. Kidney Shot's stun never breaks on
/// damage, so a Fear the Rogue's team is beating on does not hold it off.
///
/// A stun on the kill target is still valuable even if not the ideal CC target.
fn select_melee_cc_target(
    cc_target: Option<Entity>,
    kill_target: Option<Entity>,
    my_pos: Vec3,
    ctx: &CombatContext,
) -> Option<(Entity, Vec3)> {
    if let Some(cc_entity) = cc_target {
        if !ctx.entity_is_immune(cc_entity) && ctx.held_in_hard_cc(cc_entity).is_none() {
            if let Some(info) = ctx.combatants.get(&cc_entity) {
                if my_pos.distance(info.position) <= MELEE_RANGE {
                    return Some((cc_entity, info.position));
                }
            }
        }
    }

    if let Some(kill_entity) = kill_target {
        if !ctx.entity_is_immune(kill_entity) && ctx.held_in_hard_cc(kill_entity).is_none() {
            if let Some(info) = ctx.combatants.get(&kill_entity) {
                if my_pos.distance(info.position) <= MELEE_RANGE {
                    return Some((kill_entity, info.position));
                }
            }
        }
    }

    None
}
