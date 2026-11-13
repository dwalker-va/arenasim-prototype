//! The Paladin's Holy talent abilities (AS-420): Divine Favor and Aura
//! Mastery.
//!
//! Each is a `talent: true` ability, in a Paladin's book only when its build
//! takes the node that grants it, so each rule first asks the book and does
//! nothing at all without it: an untalented Paladin decides exactly as it did
//! before these existed (`tests/talent_ai_audit.rs` holds every rule to both
//! halves).
//!
//! - **Divine Favor** ahead of the emergency Holy Shock heal: a teammate is
//!   below the emergency line and the heal would go out now, so the sure crit
//!   lands on it. Off the global cooldown (TBC 20216), so the heal follows on
//!   the Paladin's next decision.
//! - **Aura Mastery** when an enemy is about to stop a casting teammate's
//!   cast ([`interrupt_threat`]): an interrupt or a silence it could start
//!   within [`AURA_MASTERY_HORIZON`] ([`super::ready_interrupt_reach`],
//!   [`super::ready_silence_reach`]) and the teammate within that reach (plus
//!   [`AURA_MASTERY_REACH_MARGIN`]) and its sight. Every living teammate is then immune to silence and interrupt
//!   effects for its duration. Off the global cooldown.
//!
//! The decisions are pure ([`interrupt_threat`]) and the casts are `try_*`
//! steps the Paladin's chain runs (`paladin::decide_paladin_action`).
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::LOW_HP_THRESHOLD;
use crate::states::play_match::decision_trace::{DecisionEventBuilder, RejectionReason};
use crate::states::play_match::utils::log_ability_use;

use super::cast_guard::{cast_gate, PreCastOpts};
use super::{cast_reach, ready_interrupt_reach, ready_silence_reach, CastReach, CombatContext};

/// How soon an enemy's interrupt must be ready to count as about to land: the
/// global cooldown, the time a casting teammate's cast has in it.
pub const AURA_MASTERY_HORIZON: f32 = 1.5;

/// Yards beyond an interrupt's range an enemy may stand and still threaten
/// the cast: what a melee closes in under a second.
pub const AURA_MASTERY_REACH_MARGIN: f32 = 5.0;

/// A casting teammate an enemy is about to interrupt or silence, and that
/// enemy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptThreat {
    pub teammate: Entity,
    pub enemy: Entity,
}

/// The first casting teammate of `me` (itself included, in entity order) that
/// a visible enemy is about to stop casting, with an interrupt or a silence:
///
/// - an INTERRUPT takes a cast bar a silence prevents, or a channel; the enemy
///   has one ready within [`AURA_MASTERY_HORIZON`]
///   ([`super::ready_interrupt_reach`]);
/// - a SILENCE (the Priest's) stops a cast a silence prevents that costs mana,
///   by a mana user (`AbilityConfig::silence_stops`); the enemy has one ready
///   within the same horizon ([`super::ready_silence_reach`]).
///
/// The teammate is not already immune to interrupts, and stands inside the
/// enemy's reach for it plus [`AURA_MASTERY_REACH_MARGIN`] and in its sight.
/// `None` when no teammate is threatened.
pub fn interrupt_threat(ctx: &CombatContext, me: Entity) -> Option<InterruptThreat> {
    let team = ctx.combatants.get(&me)?.team;
    let casting = ctx.combatants.values().filter_map(|c| {
        if c.team != team || !c.is_alive || ctx.entity_is_interrupt_immune(c.entity) {
            return None;
        }
        let def = c.book.get(&c.casting_ability?)?;
        let interruptible = def.can_be_interrupted() || def.is_channel();
        let silenceable = def.silence_stops() && c.class.uses_mana();
        (interruptible || silenceable).then_some((c, interruptible, silenceable))
    });
    for (teammate, interruptible, silenceable) in casting {
        let reaches = |reach: Option<f32>, e: &super::CombatantInfo| {
            reach.is_some_and(|reach| {
                cast_reach(
                    ctx,
                    reach + AURA_MASTERY_REACH_MARGIN,
                    e.position,
                    teammate.position,
                ) == CastReach::Reaches
            })
        };
        let enemy = ctx.combatants.values().find(|e| {
            e.team != team
                && e.is_alive
                && ((interruptible
                    && reaches(ready_interrupt_reach(ctx, e, AURA_MASTERY_HORIZON), e))
                    || (silenceable
                        && reaches(ready_silence_reach(ctx, e, AURA_MASTERY_HORIZON), e)))
        });
        if let Some(enemy) = enemy {
            return Some(InterruptThreat {
                teammate: teammate.entity,
                enemy: enemy.entity,
            });
        }
    }
    None
}

/// Whether `auras` hold a next-cast modifier named `name` (the talent
/// ability's own, primed and unspent).
fn primed_by(auras: Option<&ActiveAuras>, name: &str) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|aura| aura.effect_type == AuraType::NextCast && aura.ability_name == name)
    })
}

/// Divine Favor, ahead of the emergency Holy Shock heal: asked only while a
/// teammate is below the emergency line, and cast only when that heal would
/// go out now (its own gate clears with Divine Favor's cost already paid, and
/// a teammate below [`LOW_HP_THRESHOLD`] is in its reach), so the sure crit
/// is never left waiting. Not while its own modifier is still unspent.
pub fn try_divine_favor(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::DivineFavor;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    if primed_by(auras, &def.name) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }
    let shock = AbilityType::HolyShock;
    let shock_def = abilities.get_unchecked(&shock);
    // The heal is judged as the Paladin will stand after paying for Divine
    // Favor: a favor that leaves too little mana for the shock would strand
    // its sure crit on a Flash of Light.
    let mut after_favor = combatant.clone();
    after_favor.current_mana -= def.mana_cost;
    let heal_goes_out = cast_gate(
        shock,
        shock_def,
        &after_favor,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    )
    .is_ok()
        && ctx
            .lowest_health_ally_in_reach(LOW_HP_THRESHOLD, shock_def.range, my_pos)
            .is_ok();
    if !heal_goes_out {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "the emergency Holy Shock heal cannot go out now".into(),
            },
        );
        return false;
    }
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

    builder.choose(cleared, None, true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        None,
        "casts",
    );
    if let Some(pending) = AuraPending::from_ability(entity, entity, def) {
        commands.spawn(pending);
    }
    true
}

/// Aura Mastery, when an enemy is about to interrupt or silence a casting
/// teammate ([`interrupt_threat`]): every living teammate (pets aside, as the
/// Paladin's auras) in its range gains the immunity.
pub fn try_aura_mastery(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::AuraMastery;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
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
    let Some(threat) = interrupt_threat(ctx, entity) else {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "no casting teammate an enemy is about to interrupt".into(),
            },
        );
        return false;
    };

    builder.choose(cleared, Some(threat.teammate), true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        None,
        "casts",
    );
    for ally in ctx.combatants.values().filter(|c| {
        c.team == combatant.team
            && c.is_alive
            && !c.is_pet
            && my_pos.distance(c.position) <= def.range
    }) {
        if let Some(pending) = AuraPending::from_ability(ally.entity, entity, def) {
            commands.spawn(pending);
        }
    }
    true
}
