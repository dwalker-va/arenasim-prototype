//! The Paladin's Retribution talent abilities (AS-422): Repentance and
//! Crusader Strike.
//!
//! Each is a `talent: true` ability, in a Paladin's book only when its build
//! takes the node that grants it, so each rule first asks the book and does
//! nothing at all without it: an untalented Paladin decides exactly as it did
//! before these existed (`tests/talent_ai_audit.rs` holds every rule to both
//! halves).
//!
//! - **Repentance** on an enemy the team is leaving alone
//!   ([`repentance_target`]): any damage ends it, so never on an enemy a
//!   teammate (or a teammate's pet) has targeted, or one carrying the team's
//!   damage over time. The enemy healer first, so the team kills its target
//!   with the healer out; otherwise the nearest such enemy, a peel.
//! - **Crusader Strike** on the Paladin's own target in melee, while no
//!   teammate is low enough to need the heals first ([`teammate_low`]).
//!
//! Both are paid from the healer's mana, so each keeps back what the
//! emergency Holy Shock heal costs ([`leaves_emergency_mana`]): a strike or a
//! crowd control never leaves the Paladin unable to save a dying teammate.
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::ability_config::AbilityConfig;
use crate::states::play_match::combat_core::{get_crit_chance_bonus_from_slice, roll_crit};
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{CRIT_DAMAGE_MULTIPLIER, GCD, LOW_HP_THRESHOLD};
use crate::states::play_match::decision_trace::{DecisionEventBuilder, RejectionReason};
use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::cast_guard::{cast_gate, PreCastOpts};
use super::{cast_reach, CastReach, CombatContext, QueuedInstantAttack};

/// Whether `combatant` keeps the mana its emergency Holy Shock heal costs
/// after paying `def`'s: the reserve both Retribution casts hold back.
pub fn leaves_emergency_mana(
    abilities: &AbilityBook,
    combatant: &Combatant,
    def: &AbilityConfig,
) -> bool {
    let reserve = abilities
        .get(&AbilityType::HolyShock)
        .map_or(0.0, |shock| shock.mana_cost);
    combatant.current_mana - def.mana_cost >= reserve
}

/// Whether a living non-pet teammate of the deciding unit (itself included)
/// is below [`LOW_HP_THRESHOLD`]: the heals come before a strike.
pub fn teammate_low(ctx: &CombatContext) -> bool {
    ctx.alive_allies()
        .iter()
        .any(|a| a.max_health > 0.0 && a.current_health / a.max_health < LOW_HP_THRESHOLD)
}

/// The enemy Repentance goes on, or `None`: a living, visible, non-pet enemy
/// that no living teammate (pets included) has as its target and that carries
/// none of the team's damage over time — any damage would end it — and that
/// the incapacitate can land on: not immune to damage (a Cyclone included,
/// the one aura the engine refuses an incapacitate for), not diminished to
/// immunity, not already held. Among those in reach (range,
/// then sight), the enemy healer first, then the nearest. `Err(true)` when
/// one was out of sight and none in reach (`LosBlocked`), `Err(false)` when
/// there was none at all.
pub fn repentance_target(
    ctx: &CombatContext,
    def: &AbilityConfig,
    my_pos: Vec3,
) -> Result<Entity, bool> {
    let mut occluded = false;
    let mut reached: Vec<(&super::CombatantInfo, f32)> = ctx
        .alive_enemies()
        .into_iter()
        .filter(|e| {
            !ctx.attacked_by_my_team(e.entity)
                && !ctx.has_friendly_dots_on_target(e.entity)
                && !ctx.entity_is_immune(e.entity)
                && !ctx.is_dr_immune(e.entity, DRCategory::Incapacitates)
                && ctx.held_in_hard_cc(e.entity).is_none()
        })
        .filter_map(|e| match cast_reach(ctx, def.range, my_pos, e.position) {
            CastReach::Reaches => Some((e, my_pos.distance(e.position))),
            CastReach::LosBlocked => {
                occluded = true;
                None
            }
            CastReach::OutOfRange { .. } => None,
        })
        .collect();
    // Healers first, then nearest; entity order breaks a tie.
    reached.sort_by(|(a, da), (b, db)| {
        b.is_healer()
            .cmp(&a.is_healer())
            .then(da.total_cmp(db))
            .then(a.entity.cmp(&b.entity))
    });
    reached.first().map(|(e, _)| e.entity).ok_or(occluded)
}

/// Repentance on [`repentance_target`], keeping the emergency heal's mana.
pub fn try_repentance(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Repentance;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let opts = PreCastOpts {
        check_friendly_cc: true,
        check_friendly_dots: true,
        check_target_immune: true,
        ..Default::default()
    };
    // The Paladin's own state first (cooldown, silence, its mana), so a
    // refusal names it before the target or the reserve.
    if let Err(reason) = cast_gate(ability, def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(ability, reason);
        return false;
    }
    if !leaves_emergency_mana(abilities, combatant, def) {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "keeps the emergency Holy Shock's mana".into(),
            },
        );
        return false;
    }
    let target = match repentance_target(ctx, def, my_pos) {
        Ok(target) => target,
        Err(occluded) => {
            builder.reject(
                ability,
                if occluded {
                    RejectionReason::LosBlocked
                } else {
                    RejectionReason::PreconditionUnmet {
                        note: "no enemy the team is leaving alone".into(),
                    }
                },
            );
            return false;
        }
    };
    let Some(info) = ctx.combatants.get(&target) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_id = info.log_id();
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target, info.position)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target), true);
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.start_cooldown(ability);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(target_id.clone()),
        "casts",
    );
    if let Some(pending) = AuraPending::from_ability(target, entity, def) {
        let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
        combat_log.log_crowd_control(
            caster_id.clone(),
            target_id.clone(),
            "Incapacitate".to_string(),
            pending.aura.duration,
            format!(
                "{}'s Repentance incapacitates {} ({:.1}s)",
                caster_id, target_id, pending.aura.duration
            ),
        );
        same_frame_cc_queue.push((target, pending.aura.clone()));
        commands.spawn(pending);
    }
    true
}

/// Crusader Strike on the Paladin's own target, while no teammate is low and
/// the emergency heal's mana is kept. Not on a target the team holds in crowd
/// control that any damage breaks, nor one immune to damage.
pub fn try_crusader_strike(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<QueuedInstantAttack>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::CrusaderStrike;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
    if let Err(reason) = cast_gate(ability, def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(ability, reason);
        return false;
    }
    if teammate_low(ctx) {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "a teammate is low: the heals come first".into(),
            },
        );
        return false;
    }
    if !leaves_emergency_mana(abilities, combatant, def) {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "keeps the emergency Holy Shock's mana".into(),
            },
        );
        return false;
    }
    let Some(info) = combatant
        .target
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|i| i.is_alive && i.team != combatant.team)
    else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target = info.entity;
    let target_id = info.log_id();
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target, info.position)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target), true);
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.start_cooldown(ability);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(target_id),
        "uses",
    );
    // A dodged or parried strike deals nothing; its mana and cooldown are
    // spent, as Mortal Strike's are.
    if ctx.melee_attack_avoided(commands, game_rng, target, &def.name) {
        return true;
    }
    let self_auras = ctx
        .active_auras
        .get(&entity)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    let mut damage = combatant.calculate_ability_damage_config(def, game_rng, 0.0, 0.0);
    let is_crit = roll_crit(
        combatant.crit_chance + crit_bonus + def.crit_chance,
        game_rng,
    );
    if is_crit {
        damage *= CRIT_DAMAGE_MULTIPLIER;
    }
    instant_attacks.push(QueuedInstantAttack {
        attacker: entity,
        target,
        damage,
        attacker_team: combatant.team,
        attacker_slot: combatant.slot,
        attacker_class: combatant.class,
        ability,
        is_crit,
    });
    true
}
