//! On-use items: when the wearer's AI fires one. One rule per effect KIND,
//! never one per item (the talents proposal §6.4).
//!
//! **The crowd-control break** ([`OnUseEffect::CcBreak`], the Medallion;
//! user rulings, 2026-10-31). It is asked only while the wearer is HELD — a
//! stun, fear, polymorph or incapacitate, through
//! `combat_ai::dispatch_while_held` — and fires on the first LONG hold that
//! lands while it is ready: one with [`CC_BREAK_MIN_HOLD_REMAINING`] or more
//! left on the holds it removes. A shorter hold runs out on its own, and a
//! once-a-match break spent on it would leave the next long one unanswered.
//!
//! **It waits for a teammate's dispel.** When every long hold on the wearer is
//! one a teammate is going to dispel ([`expected_dispeller`]), the wearer
//! holds the break for at most [`DISPEL_WAIT_GRACE_SECS`] and keeps it if the
//! dispel lands. The question is asked again every tick, so a dispeller that
//! is crowd-controlled, silenced or runs dry stops the wait at once.
//!
//! A root or slow alone never asks it: a rooted wearer still casts and swings
//! what is in reach, and a melee wearer rooted away from its target is the
//! Paladin's Cleanse's to answer (`ROOT_DISPEL_PRIORITY`), not a once-a-match
//! break the next stun would want.

use bevy::prelude::*;

use super::{
    ally_dispel_priority, ally_removal, can_cast_dispel, holds, owed_dispel, CombatContext,
    OwedDispel, URGENT_DISPEL_PRIORITY,
};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::{Aura, Combatant};
use crate::states::play_match::constants::{CC_BREAK_MIN_HOLD_REMAINING, GCD};
use crate::states::play_match::decision_trace::{ActorView, DecisionTrace, RejectionReason};
use crate::states::play_match::on_use_items::{
    cc_break_breaks, OnUseEffect, OnUsePending, WAIT_GAP_SECS,
};

/// How long a held wearer waits for a teammate's dispel before it breaks free
/// anyway: one global cooldown — the dispeller may be mid-GCD when the hold
/// lands — and half a second to react.
pub const DISPEL_WAIT_GRACE_SECS: f32 = GCD + 0.5;

/// A hold the break would remove that still has `CC_BREAK_MIN_HOLD_REMAINING`
/// or more to run: the holds that make it worth breaking.
fn long_holds(auras: &[Aura]) -> impl Iterator<Item = &Aura> {
    auras.iter().filter(|a| {
        holds(a.effect_type)
            && cc_break_breaks(a.effect_type)
            && a.duration >= CC_BREAK_MIN_HOLD_REMAINING
    })
}

/// The teammate going to dispel `wearer`'s hold, and the dispel it will use —
/// `None` when no teammate is. A living non-pet teammate counts when:
///
/// - one dispel in its kit removes EVERY long hold on the wearer, at the
///   urgent bar its rotation spends a dispel on ahead of healing
///   (`URGENT_DISPEL_PRIORITY`: Polymorph, Freezing Trap, Fear) — a stun no
///   healer dispels leaves nothing to wait for;
/// - it can cast that dispel now: not held, not silenced or locked out of the
///   school ([`can_cast_dispel`]), and the dispel is off cooldown;
/// - the dispel is owed to the WEARER ([`owed_dispel`]): the rotation's urgent
///   dispel reaches it from where the healer stands, or the dispel walk is
///   carrying the healer to it. That also asks that the healer can afford it.
pub fn expected_dispeller(ctx: &CombatContext, wearer: Entity) -> Option<(Entity, AbilityType)> {
    let holds_on_wearer: Vec<&Aura> = long_holds(ctx.active_auras.get(&wearer)?).collect();
    if holds_on_wearer.is_empty() {
        return None;
    }
    let wearer_team = ctx.combatants.get(&wearer)?.team;
    ctx.combatants.values().find_map(|mate| {
        if mate.entity == wearer || mate.team != wearer_team || !mate.is_alive || mate.is_pet {
            return None;
        }
        let held = ctx
            .active_auras
            .get(&mate.entity)
            .is_some_and(|a| a.iter().any(|aura| holds(aura.effect_type)));
        if held {
            return None;
        }
        mate.book.iter().find_map(|(dispel, _)| {
            let removal = ally_removal(*dispel)?;
            let frees_every_hold = holds_on_wearer.iter().all(|aura| {
                removal.scope.takes(aura) && ally_dispel_priority(aura) >= URGENT_DISPEL_PRIORITY
            });
            let ready = ctx
                .ability_cooldowns
                .get(&mate.entity)
                .and_then(|cds| cds.get(dispel))
                .is_none_or(|remaining| *remaining <= 0.0);
            let owed = matches!(
                owed_dispel(ctx, &mate.book, mate.entity, mate.position, mate.current_mana, *dispel),
                Some(OwedDispel::Reached(e) | OwedDispel::Unreached(e)) if e == wearer
            );
            (frees_every_hold
                && ready
                && can_cast_dispel(ctx, &mate.book, mate.entity, *dispel)
                && owed)
                .then_some((mate.entity, *dispel))
        })
    })
}

/// What a held wearer does with a ready crowd-control break this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BreakTurn {
    /// Break free now.
    Break,
    /// No long hold to break (`hold_remaining` is the longest one left).
    ShortHold { hold_remaining: f32 },
    /// Wait for `dispeller`'s `dispel`; the wait began at `since`.
    WaitForDispel {
        dispeller: Entity,
        dispel: AbilityType,
        since: f32,
    },
}

/// Pure crowd-control-break decision. `hold_remaining` is the longest time
/// left on a hold the break removes; `dispeller` is [`expected_dispeller`];
/// `waiting_since` when the current wait began, if one has; `now` the clock.
pub fn cc_break_turn(
    hold_remaining: f32,
    dispeller: Option<(Entity, AbilityType)>,
    waiting_since: Option<f32>,
    now: f32,
) -> BreakTurn {
    if hold_remaining < CC_BREAK_MIN_HOLD_REMAINING {
        return BreakTurn::ShortHold { hold_remaining };
    }
    match dispeller {
        Some((dispeller, dispel)) => {
            let since = waiting_since.unwrap_or(now);
            if now - since < DISPEL_WAIT_GRACE_SECS {
                BreakTurn::WaitForDispel {
                    dispeller,
                    dispel,
                    since,
                }
            } else {
                BreakTurn::Break
            }
        }
        None => BreakTurn::Break,
    }
}

/// Ask every crowd-control-break item the wearer carries whether to fire it,
/// and fire the first that should: put it on its cooldown and spawn its
/// [`OnUsePending`]. Each one asked leaves an `item_decision` in the trace.
/// Returns whether one fired. Called from `combat_ai::dispatch_while_held`.
pub fn try_cc_break_while_held(
    commands: &mut Commands,
    decision_trace: &mut DecisionTrace,
    entity: Entity,
    combatant: &mut Combatant,
    ctx: &CombatContext,
) -> bool {
    let Some(actor) = ctx.self_info().map(ActorView::from_info) else {
        return false;
    };
    let now = ctx.situation().now;
    let hold_remaining = ctx.active_auras.get(&entity).map_or(0.0, |auras| {
        auras
            .iter()
            .filter(|a| holds(a.effect_type) && cc_break_breaks(a.effect_type))
            .map(|a| a.duration)
            .fold(0.0_f32, f32::max)
    });
    let dispeller = expected_dispeller(ctx, entity);

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
        // A wait not asked within the gap belongs to an earlier hold.
        let waiting_since = slot
            .waiting
            .filter(|(_, last)| now - last <= WAIT_GAP_SECS)
            .map(|(since, _)| since);
        match cc_break_turn(hold_remaining, dispeller, waiting_since, now) {
            BreakTurn::ShortHold { hold_remaining } => {
                slot.waiting = None;
                decision_trace.record_item_decision(
                    actor.clone(),
                    ctx.situation(),
                    slot.item,
                    Err(RejectionReason::PreconditionUnmet {
                        note: format!(
                            "short hold: {hold_remaining:.1}s left, under the \
                             {CC_BREAK_MIN_HOLD_REMAINING:.1}s floor"
                        ),
                    }),
                );
            }
            BreakTurn::WaitForDispel {
                dispeller,
                dispel,
                since,
            } => {
                slot.waiting = Some((since, now));
                decision_trace.record_item_decision(
                    actor.clone(),
                    ctx.situation(),
                    slot.item,
                    Err(RejectionReason::PreconditionUnmet {
                        note: format!(
                            "waiting for {}'s {dispel:?} ({:.1}s of {DISPEL_WAIT_GRACE_SECS:.1}s)",
                            dispeller.index(),
                            now - since
                        ),
                    }),
                );
            }
            BreakTurn::Break => {
                slot.waiting = None;
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
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEALER: Entity = Entity::from_raw(7);

    #[test]
    fn a_long_hold_breaks_and_a_short_one_does_not() {
        assert_eq!(cc_break_turn(2.0, None, None, 10.0), BreakTurn::Break);
        assert_eq!(
            cc_break_turn(1.9, None, None, 10.0),
            BreakTurn::ShortHold {
                hold_remaining: 1.9
            }
        );
    }

    #[test]
    fn an_expected_dispel_is_waited_for_until_the_grace_runs_out() {
        let d = Some((HEALER, AbilityType::DispelMagic));
        assert_eq!(
            cc_break_turn(6.0, d, None, 10.0),
            BreakTurn::WaitForDispel {
                dispeller: HEALER,
                dispel: AbilityType::DispelMagic,
                since: 10.0
            }
        );
        assert!(matches!(
            cc_break_turn(6.0, d, Some(10.0), 10.0 + DISPEL_WAIT_GRACE_SECS - 0.01),
            BreakTurn::WaitForDispel { .. }
        ));
        assert_eq!(
            cc_break_turn(6.0, d, Some(10.0), 10.0 + DISPEL_WAIT_GRACE_SECS),
            BreakTurn::Break
        );
        // The floor still holds while waiting.
        assert!(matches!(
            cc_break_turn(1.0, d, Some(10.0), 20.0),
            BreakTurn::ShortHold { .. }
        ));
    }
}
