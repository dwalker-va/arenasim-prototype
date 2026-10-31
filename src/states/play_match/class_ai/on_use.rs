//! On-use items: when the wearer's AI fires one. One rule per effect KIND,
//! never one per item (the talents proposal §6.4).
//!
//! **The crowd-control break** ([`OnUseEffect::CcBreak`], the Medallion)
//! reuses the shape of the Paladin's while-CC Divine Shield
//! ([`divine_shield_while_cc_should_fire`](super::paladin::divine_shield_while_cc_should_fire)).
//! It is asked only while the wearer is HELD (a stun, fear, polymorph or
//! incapacitate — `combat_ai::dispatch_while_held`), and fires when:
//!
//! - a teammate is low (below `LOW_HP_THRESHOLD`) and the hold has
//!   [`CC_BREAK_MIN_HOLD_REMAINING`] or more left to run — the break buys
//!   real acting time for a teammate who needs it; or
//! - the wearer is the enemy's kill target under burst: the enemy team's
//!   `kill_window` names it. That is Divine Shield's self trigger, read from
//!   the situation the enemy's burst itself reads.
//!
//! A root or slow alone never asks it: the break frees them too, but a
//! once-a-match break is not spent on a unit that can still act.

use bevy::prelude::*;

use super::CombatContext;
use crate::states::play_match::components::Combatant;
use crate::states::play_match::constants::{CC_BREAK_MIN_HOLD_REMAINING, LOW_HP_THRESHOLD};
use crate::states::play_match::decision_trace::{ActorView, DecisionTrace, RejectionReason};
use crate::states::play_match::on_use_items::{cc_break_breaks, OnUseEffect, OnUsePending};

/// Pure crowd-control-break decision: should a held wearer break free now?
///
/// - `lowest_ally_hp_pct`: the lowest health fraction among its living
///   non-pet teammates, itself excluded (`f32::INFINITY` with none);
/// - `hold_remaining`: the longest time left on a hold the break removes;
/// - `kill_target`: whether the enemy team's kill window names the wearer.
pub fn cc_break_should_fire(
    lowest_ally_hp_pct: f32,
    hold_remaining: f32,
    kill_target: bool,
) -> bool {
    let teammate_in_danger =
        lowest_ally_hp_pct < LOW_HP_THRESHOLD && hold_remaining >= CC_BREAK_MIN_HOLD_REMAINING;
    teammate_in_danger || kill_target
}

/// Ask every ready crowd-control-break item the wearer carries whether to fire
/// it, and fire the first that should: put it on its cooldown and spawn its
/// [`OnUsePending`]. Each one asked leaves an `item_decision` in the trace.
/// Returns whether one fired.
///
/// Called from `combat_ai::dispatch_while_held` while the wearer is held;
/// `enemy_kill_target` is the enemy team's `kill_window` subject.
pub fn try_cc_break_while_held(
    commands: &mut Commands,
    decision_trace: &mut DecisionTrace,
    entity: Entity,
    combatant: &mut Combatant,
    ctx: &CombatContext,
    enemy_kill_target: Option<Entity>,
) -> bool {
    let Some(actor) = ctx.self_info().map(ActorView::from_info) else {
        return false;
    };
    let hold_remaining = ctx.active_auras.get(&entity).map_or(0.0, |auras| {
        auras
            .iter()
            .filter(|a| super::holds(a.effect_type) && cc_break_breaks(a.effect_type))
            .map(|a| a.duration)
            .fold(0.0_f32, f32::max)
    });
    let lowest_ally_hp_pct = ctx
        .combatants
        .values()
        .filter(|info| {
            info.team == combatant.team
                && info.entity != entity
                && info.current_health > 0.0
                && info.max_health > 0.0
                && !info.is_pet
        })
        .map(|info| info.current_health / info.max_health)
        .fold(f32::INFINITY, f32::min);
    let kill_target = enemy_kill_target == Some(entity);

    for slot in combatant.on_use_items.iter_mut() {
        match slot.config.effect {
            OnUseEffect::CcBreak => {}
        }
        if slot.spent {
            continue;
        }
        if slot.remaining > 0.0 {
            decision_trace.record_item_decision(
                actor.clone(),
                ctx.situation(),
                slot.item,
                Err(RejectionReason::OnCooldown {
                    remaining: slot.remaining,
                }),
            );
            continue;
        }
        if !cc_break_should_fire(lowest_ally_hp_pct, hold_remaining, kill_target) {
            decision_trace.record_item_decision(
                actor.clone(),
                ctx.situation(),
                slot.item,
                Err(RejectionReason::PreconditionUnmet {
                    note: format!(
                        "not the enemy's kill target, and no ally < {:.0}% with >= {:.1}s of \
                         the hold left (ally {:.0}%, {:.1}s left)",
                        LOW_HP_THRESHOLD * 100.0,
                        CC_BREAK_MIN_HOLD_REMAINING,
                        lowest_ally_hp_pct * 100.0,
                        hold_remaining,
                    ),
                }),
            );
            continue;
        }
        decision_trace.record_item_decision(actor, ctx.situation(), slot.item, Ok(()));
        slot.start_cooldown();
        commands.spawn(OnUsePending {
            wearer: entity,
            item: slot.item,
            item_name: slot.name.clone(),
            effect: slot.config.effect,
        });
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_low_teammate_needs_a_hold_worth_breaking() {
        assert!(cc_break_should_fire(0.3, 2.0, false));
        assert!(
            !cc_break_should_fire(0.3, 1.9, false),
            "too little hold left"
        );
        assert!(!cc_break_should_fire(0.6, 6.0, false), "no teammate low");
        assert!(
            !cc_break_should_fire(f32::INFINITY, 6.0, false),
            "no teammate"
        );
    }

    #[test]
    fn the_kill_target_breaks_whatever_is_left() {
        assert!(cc_break_should_fire(f32::INFINITY, 0.5, true));
        assert!(cc_break_should_fire(0.9, 0.1, true));
    }
}
