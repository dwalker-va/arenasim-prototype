//! Warlock AI Module
//!
//! Handles AI decision-making for the Warlock class.
//!
//! ## Priority Order
//! After the plays and the Death Coil peel:
//! 0. Life Tap (health into mana) — when mana-starved and safe: see
//!    [`life_tap_hold`]
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
use crate::states::match_config::{CharacterClass, WarlockCurse};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::{
    ActiveAuras, AuraPending, AuraType, CastingState, ChannelingState, Combatant, DRCategory,
    PlayMatchEntity, Projectile,
};
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, PlayStep, RejectionReason, TapHold,
};
use crate::states::play_match::movement_config::MovementConfig;
use crate::states::play_match::situation::{healer_can_heal, Role, SituationConfig};

use super::cast_guard::{cast_gate, PreCastOpts};

use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::CombatContext;

/// Check if the Warlock is being kited (slowed and out of preferred range).
fn is_being_kited(
    combatant: &Combatant,
    my_pos: Vec3,
    target_pos: Vec3,
    auras: Option<&ActiveAuras>,
) -> bool {
    let is_slowed = auras
        .map(|a| {
            a.auras
                .iter()
                .any(|aura| aura.effect_type == AuraType::MovementSpeedSlow)
        })
        .unwrap_or(false);

    let distance_to_target = my_pos.distance(target_pos);
    let preferred_range = combatant.class.preferred_range();
    let out_of_range = distance_to_target > preferred_range;

    is_slowed && out_of_range
}

/// Pick an enemy healer to lock down with Fear, or `None` if no useful target.
///
/// Returns a living enemy healer that is NOT the current kill target (we Fear the
/// healer to stop it saving the target we're killing), is castable-on (not immune,
/// not already CC'd), and is not yet Fear-DR-immune. DR on the Fears category is
/// the natural rate limiter — once the healer is DR-immune this returns `None` and
/// the Warlock resumes its damage rotation until the window reopens.
fn pick_healer_to_fear(kill_target: Entity, ctx: &CombatContext) -> Option<Entity> {
    ctx.alive_enemies()
        .into_iter()
        .filter(|info| info.class.is_healer())
        .map(|info| info.entity)
        // Don't Fear the target we're actively trying to kill.
        .filter(|&healer| healer != kill_target)
        .find(|&healer| {
            // Already held (a teammate's Polymorph, a stun)? No value in
            // re-CCing. A rooted healer still heals, so a root does not count.
            !ctx.entity_is_immune(healer)
                && !ctx.is_dr_immune(healer, DRCategory::Fears)
                // Fear breaks on damage.
                && ctx.held_in_hard_cc(healer, super::Incoming::Breaks).is_none()
        })
}

/// Distance (yards) within which an enemy is treated as "training" the Warlock
/// and worth a Death Coil peel.
const DEATH_COIL_PEEL_RADIUS: f32 = 8.0;

/// Pick an enemy to peel off the Warlock with Death Coil, or `None`.
///
/// A threat is an alive enemy within [`DEATH_COIL_PEEL_RADIUS`] of the Warlock
/// that is either explicitly targeting it or is a melee class (Warrior/Rogue) —
/// i.e. something that pins the cloth caster. Excludes targets that are immune,
/// Fear-DR-immune, already held or rooted (no value in re-CCing). Returns the
/// nearest such enemy. The caller still range/cooldown/mana-gates via
/// `try_death_coil`, so this only answers "is there someone to peel?".
fn pick_death_coil_peel(me: Entity, my_pos: Vec3, ctx: &CombatContext) -> Option<Entity> {
    let mut threats: Vec<(Entity, f32)> = ctx
        .alive_enemies()
        .into_iter()
        .filter(|info| {
            let dist = info.position.distance(my_pos);
            if dist > DEATH_COIL_PEEL_RADIUS {
                return false;
            }
            let is_training_me = info.target == Some(me);
            let is_melee = matches!(info.class, CharacterClass::Warrior | CharacterClass::Rogue);
            if !is_training_me && !is_melee {
                return false;
            }
            // Death Coil diminishes on the Horror bucket (not Fears), so gate on
            // Horror DR — a target that is Fear-DR-immune can still be horrified.
            if ctx.entity_is_immune(info.entity)
                || ctx.is_dr_immune(info.entity, DRCategory::Horror)
            {
                return false;
            }
            // Already held — no value in re-CCing it (the horror never breaks
            // on damage) — or rooted, which already peels a melee off the
            // Warlock.
            ctx.held_in_hard_cc(info.entity, super::Incoming::Lasts)
                .is_none()
                && !ctx
                    .active_auras
                    .get(&info.entity)
                    .is_some_and(|auras| auras.iter().any(|a| a.effect_type == AuraType::Root))
        })
        .map(|info| (info.entity, info.position.distance(my_pos)))
        .collect();
    threats.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    threats.first().map(|(e, _)| *e)
}

/// Warlock AI: Decides and executes abilities for a Warlock combatant.
pub fn decide_warlock_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    movement: &MovementConfig,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // No target — no decision (emission gate).
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        return false;
    };
    let target_pos = target_info.position;

    // GCD short-circuit — no event.
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let target_immune = ctx.entity_is_immune(target_entity);
    let being_kited = is_being_kited(combatant, my_pos, target_pos, auras);

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, Some(target_entity), my_pos)
    else {
        return false;
    };

    // The Peel play (TeamPlan): a teammate is in danger, so crowd control on
    // its attacker heads the chain — Death Coil (an instant horror that never
    // breaks), else Fear on one the Warlock is not damaging itself.
    if let Some(call) = super::peel::peel_call(ctx, Role::RangedDps) {
        let peeled = call.attackers.iter().any(|&attacker| {
            let Some(info) = ctx.combatants.get(&attacker) else {
                return false;
            };
            let pos = info.position;
            let coil = if ctx.entity_is_immune(attacker) {
                builder.reject(AbilityType::DeathCoil, RejectionReason::TargetImmune);
                false
            } else if ctx.is_dr_immune(attacker, DRCategory::Horror) {
                builder.reject(
                    AbilityType::DeathCoil,
                    RejectionReason::DRImmune {
                        category: DRCategory::Horror,
                    },
                );
                false
            } else {
                try_death_coil(
                    commands,
                    combat_log,
                    abilities,
                    entity,
                    combatant,
                    my_pos,
                    auras,
                    attacker,
                    pos,
                    ctx,
                    &mut builder,
                )
            };
            coil || try_peel_fear(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                attacker,
                pos,
                ctx,
                &mut builder,
            )
        });
        if peeled {
            builder.play_step(PlayStep::Peel);
            builder.finish();
            return true;
        }
    }

    // Priority 0: Death Coil peel. A melee attacker on the cloth Warlock is its
    // worst case (the kit otherwise has no instant CC that survives damage — Fear
    // breaks). Death Coil's horror never breaks on damage, so it guarantees a 3s
    // peel and heals the Warlock for the hit. Reactive defensive cooldown, so it
    // outranks the damage rotation.
    if let Some(peel_target) = pick_death_coil_peel(entity, my_pos, ctx) {
        if let Some(peel_info) = ctx.combatants.get(&peel_target) {
            let peel_pos = peel_info.position;
            if try_death_coil(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                peel_target,
                peel_pos,
                ctx,
                &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }
    }

    // The Chain play (TeamPlan): a lock the team holds on an enemy is about to
    // end, so Fear (else Death Coil) hands off onto it, landing in its last
    // moments — or the global cooldown is held for that moment.
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
        let pos = ctx.combatants.get(&call.subject).map(|i| i.position);
        let handed = pos.is_some_and(|pos| {
            plan.due.iter().any(|&tool| match tool {
                AbilityType::Fear if being_kited => {
                    builder.reject(
                        tool,
                        RejectionReason::PreconditionUnmet {
                            note: "being kited — cast would be interrupted".into(),
                        },
                    );
                    false
                }
                AbilityType::Fear => try_peel_fear(
                    commands,
                    combat_log,
                    abilities,
                    entity,
                    combatant,
                    my_pos,
                    auras,
                    call.subject,
                    pos,
                    ctx,
                    &mut builder,
                ),
                AbilityType::DeathCoil => try_death_coil(
                    commands,
                    combat_log,
                    abilities,
                    entity,
                    combatant,
                    my_pos,
                    auras,
                    call.subject,
                    pos,
                    ctx,
                    &mut builder,
                ),
                _ => false,
            })
        });
        if handed || plan.hold {
            builder.play_step(PlayStep::Chain);
            builder.finish();
            return handed;
        }
    }

    // The Burst play (TeamPlan): a kill window is open, so its enemy takes
    // Death Coil (damage, and a horror that never breaks) and then Shadow Bolt
    // — each only if it lands before the window's `until` (the cast gate's
    // fit). No Shadow Bolt while being kited: the cast would be interrupted.
    if let Some(call) = super::burst::burst_call(ctx) {
        if let Some(pos) = ctx.combatants.get(&call.subject).map(|i| i.position) {
            let burst = if ctx.entity_is_immune(call.subject) {
                builder.reject(AbilityType::DeathCoil, RejectionReason::TargetImmune);
                builder.reject(AbilityType::Shadowbolt, RejectionReason::TargetImmune);
                false
            } else {
                let coil = if ctx.is_dr_immune(call.subject, DRCategory::Horror) {
                    builder.reject(
                        AbilityType::DeathCoil,
                        RejectionReason::DRImmune {
                            category: DRCategory::Horror,
                        },
                    );
                    false
                } else {
                    try_death_coil(
                        commands,
                        combat_log,
                        abilities,
                        entity,
                        combatant,
                        my_pos,
                        auras,
                        call.subject,
                        pos,
                        ctx,
                        &mut builder,
                    )
                };
                coil || if being_kited {
                    builder.reject(
                        AbilityType::Shadowbolt,
                        RejectionReason::PreconditionUnmet {
                            note: "being kited — cast would be interrupted".into(),
                        },
                    );
                    false
                } else {
                    try_shadowbolt(
                        commands,
                        combat_log,
                        abilities,
                        entity,
                        combatant,
                        my_pos,
                        auras,
                        call.subject,
                        pos,
                        ctx,
                        &mut builder,
                    )
                }
            };
            if burst {
                builder.play_step(PlayStep::Burst);
                builder.finish();
                return true;
            }
        }
    }

    // Life Tap: short of mana and safe to spend health on it, the Warlock
    // converts health into mana before its rotation asks for any.
    if try_life_tap(
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        movement,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    let enemy_has_dispeller = ctx
        .alive_enemies()
        .iter()
        .any(|e| matches!(e.class, CharacterClass::Priest | CharacterClass::Paladin));

    let mut ua_attempted = false;

    // Dispeller-priority: try UA before Corruption when enemy can dispel.
    if enemy_has_dispeller && !target_immune {
        if try_unstable_affliction(
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
        ua_attempted = true;
    }

    // Priority 1: Corruption (instant Shadow DoT) — skip if target immune.
    if target_immune {
        builder.reject(AbilityType::Corruption, RejectionReason::TargetImmune);
    } else if try_corruption(
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

    // Priority 1.5: Unstable Affliction (only if not already attempted in the
    // dispeller-priority gate above).
    if target_immune {
        if !ua_attempted {
            builder.reject(
                AbilityType::UnstableAffliction,
                RejectionReason::TargetImmune,
            );
        }
    } else if !ua_attempted
        && try_unstable_affliction(
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
        )
    {
        builder.finish();
        return true;
    }

    // Priority 1.75: Lock the enemy healer with Fear to open a kill window.
    //
    // The Warlock's pressure is pure DoT/Shadow Bolt — trivially out-healed in a
    // healer comp. Its answer (long present in the kit, never used) is an 8s Fear
    // on the *healer* while the team burns the kill target. Fear targets the
    // healer, not the focused kill target (where a DoT tick would instantly break
    // it). Diminishing Returns on the Fears category self-limits the chain, so
    // this can't perma-lock; between DR windows the Warlock falls through to its
    // normal rotation below.
    if let Some(healer_entity) = pick_healer_to_fear(target_entity, ctx) {
        if let Some(healer_info) = ctx.combatants.get(&healer_entity) {
            let healer_pos = healer_info.position;
            if try_fear(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                healer_entity,
                healer_pos,
                ctx,
                &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }
    }

    // Priority 2: Spread curses to all enemies.
    if try_spread_curses(
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

    // Priority 3: Immolate.
    if target_immune {
        builder.reject(AbilityType::Immolate, RejectionReason::TargetImmune);
    } else if being_kited {
        builder.reject(
            AbilityType::Immolate,
            RejectionReason::PreconditionUnmet {
                note: "being kited — cast would be interrupted".into(),
            },
        );
    } else if try_immolate(
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

    // Priority 4: Fear (cc_target or kill target).
    let fear_target = combatant.cc_target.or(combatant.target);
    if let Some(fear_target_entity) = fear_target {
        if ctx.entity_is_immune(fear_target_entity) {
            builder.reject(AbilityType::Fear, RejectionReason::TargetImmune);
        } else if ctx.is_dr_immune(fear_target_entity, DRCategory::Fears) {
            builder.reject(
                AbilityType::Fear,
                RejectionReason::DRImmune {
                    category: DRCategory::Fears,
                },
            );
        } else if let Some(fear_target_info) = ctx.combatants.get(&fear_target_entity) {
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
                &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }
    } else {
        builder.reject(AbilityType::Fear, RejectionReason::NoValidTarget);
    }

    // Priority 5: Drain Life.
    if target_immune {
        builder.reject(AbilityType::DrainLife, RejectionReason::TargetImmune);
    } else if being_kited {
        builder.reject(
            AbilityType::DrainLife,
            RejectionReason::PreconditionUnmet {
                note: "being kited — channel would be interrupted".into(),
            },
        );
    } else if try_drain_life(
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

    // Priority 6: Shadow Bolt — skipped when being kited or target immune.
    if target_immune {
        builder.reject(AbilityType::Shadowbolt, RejectionReason::TargetImmune);
        builder.finish();
        return false;
    }
    if being_kited {
        builder.reject(
            AbilityType::Shadowbolt,
            RejectionReason::PreconditionUnmet {
                note: "being kited — cast would be interrupted".into(),
            },
        );
        builder.finish();
        return false;
    }

    let acted = try_shadowbolt(
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
    );
    builder.finish();
    acted
}

/// Below this fraction of its mana pool the Warlock is MANA-STARVED, and a
/// Life Tap is worth its health. A tap at the shipped spell power gives back
/// about two fifths of the pool, so a tap from here never overflows it.
pub const LIFE_TAP_STARVED_FRACTION: f32 = 0.3;

/// Whether the Warlock `me` should hold its Life Tap, which would convert
/// `amount` of its health into mana: `Err` names the first gate that holds
/// it, in this order.
///
/// 1. **Not starved** — mana at or above [`LIFE_TAP_STARVED_FRACTION`] of
///    the pool.
/// 2. **Focused** — an enemy it can see, pet or not, has it targeted. Health
///    spent under attack is health the attacker no longer has to take.
/// 3. **No healer** — no living teammate healer can heal it now
///    ([`healer_can_heal`]: not locked, not out of mana, in reach and in
///    sight). A team with no healer has none to give the health back.
/// 4. **Death range** — the tap would leave it dying: below
///    `urgency_hp_threshold` of its maximum health, the line the situation
///    calls a teammate dying at.
pub fn life_tap_hold(
    me: Entity,
    combatant: &Combatant,
    amount: f32,
    ctx: &CombatContext,
    movement: &MovementConfig,
) -> Result<(), TapHold> {
    let starved_below = combatant.max_mana * LIFE_TAP_STARVED_FRACTION;
    if combatant.current_mana >= starved_below {
        return Err(TapHold::NotStarved {
            mana: combatant.current_mana,
            starved_below,
        });
    }
    if let Some(by) = ctx
        .combatants
        .values()
        .find(|e| e.team != combatant.team && e.is_alive && e.target == Some(me))
    {
        return Err(TapHold::Focused {
            by: by.entity.index(),
        });
    }
    let cfg = SituationConfig::from_movement(movement);
    let healed = ctx.combatants.get(&me).is_some_and(|me_info| {
        ctx.combatants.values().any(|ally| {
            ally.team == combatant.team
                && ally.is_alive
                && ally.class.is_healer()
                && healer_can_heal(ally, me_info, ctx.active_auras, ctx.obstacles, &cfg)
        })
    });
    if !healed {
        return Err(TapHold::NoHealer);
    }
    let floor = combatant.max_health * movement.shared.urgency_hp_threshold;
    let health_after = combatant.current_health - amount;
    if health_after < floor {
        return Err(TapHold::DeathRange {
            health_after,
            floor,
        });
    }
    Ok(())
}

/// Life Tap, when [`life_tap_hold`] lets it go and the cast gate clears it:
/// instant, on the global cooldown, no target. The conversion is applied
/// here, at its one site: the health is taken outright (not damage — nothing
/// absorbs, breaks, reveals or enrages on it) and the mana given outright
/// (not healing — arena dampening does not cut it), capped at the pool.
fn try_life_tap(
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    movement: &MovementConfig,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let life_tap = AbilityType::LifeTap;
    let def = abilities.get_unchecked(&life_tap);
    let Some(conversion) = def.health_to_mana else {
        return false;
    };
    let amount = conversion.amount(combatant.spell_power);

    if let Err(hold) = life_tap_hold(entity, combatant, amount, ctx, movement) {
        builder.reject(life_tap, RejectionReason::TapHeld { hold });
        return false;
    }
    let cleared = match cast_gate(
        life_tap,
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
            builder.reject(life_tap, reason);
            return false;
        }
    };

    builder.choose(cleared, None, true);

    // Never kills its caster: the death-range gate keeps the AI well clear,
    // and the floor is the backstop.
    combatant.current_health = (combatant.current_health - amount).max(1.0);
    combatant.current_mana = (combatant.current_mana + amount).min(combatant.max_mana);
    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Life Tap",
        None,
        "casts",
    );
    let msg = format!(
        "[LIFE TAP] {} converts {:.0} health into {:.0} mana ({:.0}/{:.0} health, {:.0}/{:.0} mana)",
        combatant_id(combatant.team, combatant.slot, combatant.class),
        amount,
        amount,
        combatant.current_health,
        combatant.max_health,
        combatant.current_mana,
        combatant.max_mana,
    );
    combat_log.log(CombatLogEventType::Buff, msg.clone());
    info!("{}", msg);

    true
}

/// Try to apply Corruption DoT to target.
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let corruption = AbilityType::Corruption;
    let corruption_def = abilities.get_unchecked(&corruption);

    let target_has_corruption = ctx
        .active_auras
        .get(&target_entity)
        .map(|auras| {
            auras.iter().any(|a| {
                a.effect_type == AuraType::DamageOverTime && a.ability_name == "Corruption"
            })
        })
        .unwrap_or(false);

    if target_has_corruption {
        builder.reject(corruption, RejectionReason::AlreadyApplied);
        return false;
    }

    let opts = PreCastOpts {
        check_friendly_cc: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        corruption,
        corruption_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(corruption, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), true);

    combatant.current_mana -= corruption_def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Corruption",
        target_tuple,
        "casts",
    );

    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, corruption_def) {
        commands.spawn(aura_pending);
    }

    combat_log.log(
        CombatLogEventType::Buff,
        format!(
            "{} applies Corruption to enemy (10 damage per 3s for 18s)",
            combatant_id(combatant.team, combatant.slot, combatant.class)
        ),
    );

    info!(
        "{} applies Corruption to enemy (10 damage per 3s for 18s)",
        combatant_id(combatant.team, combatant.slot, combatant.class)
    );

    true
}

/// Try to cast Unstable Affliction on target.
fn try_unstable_affliction(
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
    let ua = AbilityType::UnstableAffliction;
    let ua_def = abilities.get_unchecked(&ua);

    let target_has_ua = ctx
        .active_auras
        .get(&target_entity)
        .map(|auras| {
            auras.iter().any(|a| {
                a.effect_type == AuraType::DamageOverTime && a.ability_name == "Unstable Affliction"
            })
        })
        .unwrap_or(false);

    if target_has_ua {
        builder.reject(ua, RejectionReason::AlreadyApplied);
        return false;
    }

    let opts = PreCastOpts {
        check_friendly_cc: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        ua,
        ua_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ua, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(ua_def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(ua, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Unstable Affliction",
        target_tuple,
        "begins casting",
    );

    info!(
        "Team {} {} begins casting Unstable Affliction on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Immolate on target.
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let immolate = AbilityType::Immolate;
    let immolate_def = abilities.get_unchecked(&immolate);

    let target_has_immolate = ctx
        .active_auras
        .get(&target_entity)
        .map(|auras| {
            auras
                .iter()
                .any(|a| a.effect_type == AuraType::DamageOverTime && a.ability_name == "Immolate")
        })
        .unwrap_or(false);

    if target_has_immolate {
        builder.reject(immolate, RejectionReason::AlreadyApplied);
        return false;
    }

    let opts = PreCastOpts {
        check_friendly_cc: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        immolate,
        immolate_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(immolate, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(immolate_def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(immolate, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Immolate",
        target_tuple,
        "begins casting",
    );

    info!(
        "Team {} {} starts casting Immolate on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// The Peel play's Fear on `attacker`: not on the Warlock's own target (its
/// damage would break the Fear at once), nor on one immune to damage or to
/// Fear by diminishing returns; then [`try_fear`].
#[allow(clippy::too_many_arguments)]
fn try_peel_fear(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    attacker: Entity,
    attacker_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let fear = AbilityType::Fear;
    if combatant.target == Some(attacker) {
        builder.reject(
            fear,
            RejectionReason::PreconditionUnmet {
                note: "peel: the attacker is the Warlock's own target — its damage breaks the \
                       Fear"
                    .into(),
            },
        );
        return false;
    }
    if ctx.entity_is_immune(attacker) {
        builder.reject(fear, RejectionReason::TargetImmune);
        return false;
    }
    if ctx.is_dr_immune(attacker, DRCategory::Fears) {
        builder.reject(
            fear,
            RejectionReason::DRImmune {
                category: DRCategory::Fears,
            },
        );
        return false;
    }
    try_fear(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        attacker,
        attacker_pos,
        ctx,
        builder,
    )
}

/// Try to cast Fear on target.
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let fear = AbilityType::Fear;
    let fear_def = abilities.get_unchecked(&fear);

    let opts = PreCastOpts::default();
    let cleared = match cast_gate(
        fear,
        fear_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(fear, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(fear_def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(fear, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Fear",
        target_tuple,
        "begins casting",
    );

    info!(
        "Team {} {} starts casting Fear on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to fire Death Coil at a peel target: instant Shadow projectile that, on
/// impact, deals damage, heals the Warlock for that damage (see
/// `projectiles.rs`), and applies a 3s never-breaking horror via the def's Fear
/// aura. Gated through `pre_cast_ok` (range/mana/cooldown/school-lock, a held
/// target, and a friendly CC its damage would break).
fn try_death_coil(
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
    let death_coil = AbilityType::DeathCoil;
    let def = abilities.get_unchecked(&death_coil);

    // Its damage would break a friendly Polymorph or Freezing Trap, even one
    // inside the chain window that no longer counts as a hold.
    let opts = PreCastOpts {
        check_friendly_cc: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        death_coil,
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
            builder.reject(death_coil, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), true);

    let projectile_speed = def.projectile_speed.unwrap_or(40.0);
    commands.spawn((
        Projectile {
            caster: entity,
            target: target_entity,
            ability: death_coil,
            speed: projectile_speed,
            caster_team: combatant.team,
            caster_slot: combatant.slot,
            caster_class: combatant.class,
            caster_pet_type: None,
        },
        Transform::from_translation(my_pos + Vec3::new(0.0, 1.5, 0.0)),
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(death_coil, abilities);
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Death Coil",
        target_tuple,
        "fires",
    );

    info!(
        "Team {} {} fires Death Coil (peel + lifesteal)",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Shadow Bolt on target.
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let shadowbolt = AbilityType::Shadowbolt;
    let shadowbolt_def = abilities.get_unchecked(&shadowbolt);

    let opts = PreCastOpts {
        check_friendly_cc: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        shadowbolt,
        shadowbolt_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(shadowbolt, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(shadowbolt_def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(shadowbolt, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Shadow Bolt",
        target_tuple,
        "begins casting",
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let drain_life = AbilityType::DrainLife;
    let drain_life_def = abilities.get_unchecked(&drain_life);

    let hp_percent = combatant.current_health / combatant.max_health;
    if hp_percent >= 0.8 {
        builder.reject(
            drain_life,
            RejectionReason::PreconditionUnmet {
                note: "HP above 80% — Drain Life is healing-gated".into(),
            },
        );
        return false;
    }

    let target_has_dot = ctx
        .active_auras
        .get(&target_entity)
        .map(|auras| {
            auras
                .iter()
                .any(|a| a.effect_type == AuraType::DamageOverTime)
        })
        .unwrap_or(false);

    if !target_has_dot {
        builder.reject(
            drain_life,
            RejectionReason::PreconditionUnmet {
                note: "no DoT on target — Drain Life maintains pressure".into(),
            },
        );
        return false;
    }

    let opts = PreCastOpts {
        check_friendly_cc: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        drain_life,
        drain_life_def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(drain_life, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target_entity), false);

    combatant.current_mana -= drain_life_def.mana_cost;
    combatant.global_cooldown = GCD;

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

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Drain Life",
        target_tuple,
        "begins channeling",
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
fn try_spread_curses(
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
    if combatant.warlock_curse_prefs.is_empty() {
        // No curse preferences configured — silently skip (don't litter the trace
        // with NoValidTarget for a feature this Warlock isn't using).
        return false;
    }

    let mut enemies: Vec<(Entity, Vec3, u8)> = ctx
        .combatants
        .iter()
        .filter_map(|(&enemy_entity, info)| {
            if info.team != combatant.team && info.current_health > 0.0 && !info.is_pet {
                Some((enemy_entity, info.position, info.slot))
            } else {
                None
            }
        })
        .collect();

    enemies.sort_by_key(|(_, _, slot)| *slot);

    for (enemy_entity, enemy_pos, enemy_slot) in enemies {
        if ctx.entity_is_immune(enemy_entity) {
            continue;
        }

        let curse_pref = combatant
            .warlock_curse_prefs
            .get(enemy_slot as usize)
            .copied()
            .unwrap_or(WarlockCurse::Agony);

        let has_our_curse = ctx
            .active_auras
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
            builder,
        ) {
            return true;
        }
    }

    false
}

/// Cast a specific curse on a target.
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability_def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts {
        check_friendly_cc: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        ability,
        ability_def,
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

    combatant.current_mana -= ability_def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        ability_name,
        target_tuple,
        "casts",
    );

    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, ability_def) {
        commands.spawn(aura_pending);
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
            "{} applies {} to enemy ({})",
            combatant_id(combatant.team, combatant.slot, combatant.class),
            ability_name,
            effect_description
        ),
    );

    info!(
        "{} applies {} to enemy ({})",
        combatant_id(combatant.team, combatant.slot, combatant.class),
        ability_name,
        effect_description
    );

    true
}
