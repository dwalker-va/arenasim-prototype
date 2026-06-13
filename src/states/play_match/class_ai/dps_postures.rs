//! Shared DPS-kiter movement posture machine (ENGAGE/KITE) on the
//! context-steering scorer. Used by the Mage and the Hunter.
//!
//! Two postures on the shared `score_directions` machinery:
//! - **ENGAGE** — no directive; the kiter falls through to normal pursuit
//!   (`move_to_target`) to preferred range, then stands and shoots/casts.
//! - **KITE** — orbit the kill target at `range_band` distance while repelling
//!   threats (arc-kiting). Issues a `MovementDirective` the executor runs ahead
//!   of the legacy `kiting_timer` branch.
//!
//! `evaluate_dps_posture` is the shared transition + scoring machine; the
//! caller supplies the class-specific entry/sustain predicate:
//! - **Mage** — aura-gated: KITE when a melee enemy carries the Mage's own
//!   root/slow (`mage_kite_entry` / `mage_kite_sustain`).
//! - **Hunter** — proximity-gated: KITE when a melee threat is within closing
//!   range (`enemy_within`), matching its legacy three-band behavior.
//!
//! A `kite_hold` hysteresis floor blocks exit for a minimum window (anti-strobe).
//! Evaluated at ability-decision time (not a per-frame system), so KITE exit can
//! lag up to one GCD after the sustain condition lapses — accepted.

use bevy::prelude::*;

use crate::states::play_match::combat_core::{
    compass_directions_16, mask_bitmask, score_directions, RangeBand, ScorerInputs,
};
use crate::states::play_match::components::{
    AuraType, DpsPosture, KitePosture, MovementDirective, MovementGoal,
};
use crate::states::play_match::constants::MELEE_RANGE;
use crate::states::play_match::decision_trace::{
    ActorView, DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::movement_config::DpsMovementConfig;

use super::CombatContext;

/// One scorer-lookahead step distance (matches the healer scorer).
const SCORER_LOOKAHEAD: f32 = 2.0;

/// Does any alive enemy carry an aura the Mage itself applied of a
/// movement-impairing kind (Root / MovementSpeedSlow), optionally restricted to
/// within `max_dist` of `my_pos`? Used for KITE entry (melee-range) and the
/// Mage's Frostbolt close-range guard (within safe-kiting distance).
pub(super) fn mage_impaired_enemy(
    ctx: &CombatContext,
    me: Entity,
    my_pos: Vec3,
    max_dist: Option<f32>,
) -> bool {
    ctx.combatants.values().any(|info| {
        if info.is_pet || info.team == self_team(ctx, me) || !info.is_alive {
            return false;
        }
        if let Some(d) = max_dist {
            if info.position.distance(my_pos) > d {
                return false;
            }
        }
        ctx.active_auras.get(&info.entity).is_some_and(|auras| {
            auras.iter().any(|a| {
                a.caster == Some(me)
                    && matches!(a.effect_type, AuraType::Root | AuraType::MovementSpeedSlow)
            })
        })
    })
}

/// KITE sustain: a Mage-owned **Root** on any enemy at any range (a rooted
/// enemy is a committed kite window), OR a Mage-owned **slow** on an enemy
/// within `slow_radius` (the kite ring). The proximity gate on slows is
/// load-bearing: Frostbolt applies a never-breaking 5s slow on every cast, so
/// an unbounded slow-sustain would pin KITE forever on a distant slowed enemy
/// (e.g. a kited-away caster in 2v2). Gating slows to the ring lets KITE return
/// to ENGAGE once the threat has actually been kited out.
fn kite_sustained(ctx: &CombatContext, me: Entity, my_pos: Vec3, slow_radius: f32) -> bool {
    let team = self_team(ctx, me);
    ctx.combatants.values().any(|info| {
        if info.is_pet || info.team == team || !info.is_alive {
            return false;
        }
        let dist = info.position.distance(my_pos);
        ctx.active_auras.get(&info.entity).is_some_and(|auras| {
            auras.iter().any(|a| {
                a.caster == Some(me)
                    && match a.effect_type {
                        AuraType::Root => true,
                        AuraType::MovementSpeedSlow => dist <= slow_radius,
                        _ => false,
                    }
            })
        })
    })
}

fn self_team(ctx: &CombatContext, me: Entity) -> u8 {
    ctx.combatants.get(&me).map_or(u8::MAX, |i| i.team)
}

/// Aura-gated KITE entry (Mage): a melee-range enemy carries a Mage-owned
/// root/slow.
pub fn mage_kite_entry(ctx: &CombatContext, me: Entity, my_pos: Vec3) -> bool {
    mage_impaired_enemy(ctx, me, my_pos, Some(MELEE_RANGE))
}

/// Aura-gated KITE sustain (Mage): a rooted enemy at any range, or a slowed
/// enemy still within `ring` (so Frostbolt's never-breaking slow can't pin KITE
/// on a kited-away enemy).
pub fn mage_kite_sustain(ctx: &CombatContext, me: Entity, my_pos: Vec3, ring: f32) -> bool {
    kite_sustained(ctx, me, my_pos, ring)
}

/// Proximity-gated KITE entry/sustain (Hunter): is any alive enemy within
/// `radius`? Entry uses the closing-range radius; sustain a slightly larger one
/// so KITE doesn't strobe at the boundary. Class-agnostic by design — the
/// Hunter kites whatever is closing, matching its legacy three-band behavior.
pub fn enemy_within(ctx: &CombatContext, me: Entity, my_pos: Vec3, radius: f32) -> bool {
    let team = self_team(ctx, me);
    ctx.combatants.values().any(|info| {
        !info.is_pet && info.team != team && info.is_alive && info.position.distance(my_pos) <= radius
    })
}

/// Evaluate a DPS kiter's ENGAGE/KITE posture and (in KITE) issue a movement
/// directive. Shared by the Mage (aura-gated) and Hunter (proximity-gated) —
/// the caller computes `entry_trigger`/`sustain` with the class-specific
/// predicate above; this drives the common transition + scoring machine. Runs
/// before the ability pass, outside the GCD short-circuit (so a directive
/// refreshes while only the GCD is up). A *casting* kiter is excluded from the
/// dispatch query, so KITE does not re-evaluate mid-cast; `directive_ttl` is
/// sized to outlast the longest cast so the pre-cast directive survives and
/// resumes post-cast. Gated on gates-open by the caller.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_dps_posture(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    kill_target: Option<Entity>,
    ctx: &CombatContext,
    posture: Option<&mut KitePosture>,
    directive: Option<&MovementDirective>,
    config: &DpsMovementConfig,
    entry_trigger: bool,
    sustain: bool,
    now: f32,
    decision_trace: &mut DecisionTrace,
) {
    // Persistent state (local fallback if the component isn't inserted yet).
    let mut local = KitePosture::new(now);
    let needs_insert = posture.is_none();
    let state: &mut KitePosture = match posture {
        Some(p) => p,
        None => &mut local,
    };

    let prev = state.posture;

    let next = match prev {
        DpsPosture::Kite if now < state.hold_until => DpsPosture::Kite, // hysteresis hold
        DpsPosture::Kite if sustain => DpsPosture::Kite,
        DpsPosture::Kite => DpsPosture::Engage,
        _ if entry_trigger => DpsPosture::Kite, // ENGAGE (or any) → KITE
        _ => DpsPosture::Engage,
    };

    let transitioned = next != prev;
    if transitioned {
        state.posture = next;
        state.since = now;
        state.last_direction = None;
        state.hold_until = if next == DpsPosture::Kite { now + config.kite_hold } else { 0.0 };
    }

    if next == DpsPosture::Engage {
        // ENGAGE: no directive — clear any stale kite vector so the Mage closes
        // to preferred range via normal pursuit instead of coasting.
        if directive.is_some() {
            commands.entity(entity).remove::<MovementDirective>();
        }
        if transitioned {
            // Trace the KITE → ENGAGE exit.
            if let Some(info) = ctx.combatants.get(&entity) {
                let actor = ActorView::from_info(info);
                let mut builder = decision_trace.start_movement_decision(actor, None);
                builder.transition(
                    prev.into(),
                    TracePosture::Engage,
                    MovementTrigger::KiteExit,
                    MovementGoalKind::Direction,
                );
                builder.finish();
            }
        }
        if needs_insert {
            commands.entity(entity).try_insert(*state);
        }
        return;
    }

    // KITE: re-score only on transition or when the commit window expired, to
    // hold a direction for the anti-zigzag window.
    let recommit = transitioned
        || directive.map_or(true, |d| now >= d.committed_until || now >= d.expires);
    if !recommit {
        if needs_insert {
            commands.entity(entity).try_insert(*state);
        }
        return;
    }

    let self_team = self_team(ctx, entity);
    let threats: Vec<Vec3> = ctx
        .combatants
        .values()
        .filter(|i| !i.is_pet && i.team != self_team && i.is_alive)
        .map(|i| i.position)
        .collect();

    // Nearest threat for the distance-max `flee` term (Hunter). Deterministic:
    // threats are collected from a BTreeMap, so equal distances tie-break by
    // entity order.
    let nearest_threat = threats
        .iter()
        .copied()
        .min_by(|a, b| {
            a.distance(my_pos)
                .partial_cmp(&b.distance(my_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    let range_band = kill_target
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|i| i.is_alive)
        .map(|i| RangeBand {
            target: i.position,
            min: config.range_band_min,
            max: config.range_band_max,
        });

    let committed_direction = directive
        .filter(|d| now < d.committed_until)
        .and(state.last_direction);

    let inputs = ScorerInputs {
        my_pos,
        lookahead: SCORER_LOOKAHEAD,
        threats,
        anchor: None,
        formation_point: None,
        wand_target: None,
        wand_range: 0.0,
        range_band,
        nearest_threat,
        committed_direction,
    };
    let chosen = score_directions(&compass_directions_16(), &inputs, &config.weights);
    if chosen == Vec2::ZERO {
        if needs_insert {
            commands.entity(entity).try_insert(*state);
        }
        return; // defensive — 16 candidates always yield a direction
    }

    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Direction(chosen),
        expires: now + config.directive_ttl,
        committed_until: now + config.commit_window,
    });

    let direction_changed = state.last_direction.map_or(true, |d| d.distance(chosen) > 1e-3);
    state.last_direction = Some(chosen);

    if transitioned || direction_changed {
        if let Some(info) = ctx.combatants.get(&entity) {
            let actor = ActorView::from_info(info);
            let mut builder = decision_trace.start_movement_decision(actor, None);
            if transitioned {
                builder.transition(
                    prev.into(),
                    TracePosture::Kite,
                    MovementTrigger::KiteEnter,
                    MovementGoalKind::Direction,
                );
            } else {
                builder.direction_change(
                    TracePosture::Kite,
                    MovementTrigger::CommitExpired,
                    MovementGoalKind::Direction,
                );
            }
            builder.chosen_direction([chosen.x, chosen.y]);
            builder.masked(mask_bitmask(&compass_directions_16(), &inputs));
            builder.finish();
        }
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }
}
