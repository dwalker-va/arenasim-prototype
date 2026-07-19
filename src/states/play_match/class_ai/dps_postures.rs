//! Shared DPS-kiter movement posture machine (ENGAGE/KITE) on the
//! context-steering scorer. Used by the Mage and the Hunter.
//!
//! Two postures on the shared `score_directions` machinery:
//! - **ENGAGE** — no directive; the kiter falls through to normal pursuit
//!   (`move_to_target`) to preferred range, then stands and shoots/casts.
//! - **KITE** — orbit the kill target at `range_band` distance while repelling
//!   threats (arc-kiting). Issues a `MovementDirective` the executor runs — the
//!   sole kiting path now that the legacy `kiting_timer` branch is deleted.
//!
//! `evaluate_dps_posture` is the shared transition + scoring machine; the
//! caller supplies the class-specific entry/sustain predicate:
//! - **Mage** — aura-gated: KITE when a melee enemy carries the Mage's own
//!   root/slow (`mage_kite_entry` / `mage_kite_sustain`).
//! - **Hunter** — proximity-gated: KITE when a melee-DPS threat (Warrior/Rogue)
//!   is within closing range (`melee_within`); ranged classes are excluded so
//!   the Hunter holds and shoots a caster rather than fleeing it.
//!
//! A `kite_hold` hysteresis floor blocks exit for a minimum window (anti-strobe).
//! Evaluated at ability-decision time (not a per-frame system), so KITE exit can
//! lag up to one GCD after the sustain condition lapses — accepted.

use bevy::prelude::*;

use crate::states::play_match::combat_core::{
    compass_directions_16, mask_and_los_bitmask, score_directions, RangeBand, ScorerInputs,
};
use crate::states::play_match::components::{
    AuraType, DpsPosture, KitePosture, MovementDirective, MovementGoal,
};
use crate::states::play_match::constants::MELEE_RANGE;
use crate::states::play_match::map_geometry::{has_line_of_sight, EYE_HEIGHT};
use crate::states::play_match::match_config::CharacterClass;
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

/// Proximity-gated KITE entry/sustain (Hunter): is a melee-DPS threat closing
/// within `radius`? Entry uses the closing-range radius; sustain a slightly
/// larger one so KITE doesn't strobe at the boundary.
///
/// Only a class whose melee damage warrants kiting counts (`is_kite_threat` —
/// Warrior, Rogue). Ranged classes (Mage, Warlock, Priest, Hunter) are
/// excluded: against them the Hunter holds at shot range and trades shots, it
/// does NOT flee — fleeing a caster just forfeits its own DPS. The Paladin is
/// excluded too: its melee damage isn't meaningful pressure, and avoiding its
/// Hammer of Justice is a separate "avoid CC" movement concern (deferred).
/// Stealthed enemies are excluded — the kiter can't see a stealthed Rogue, so
/// it must not react to its position until stealth breaks. Enemy melee *pets*
/// are excluded for now (the `!is_pet` filter); folding them in is deferred.
pub fn melee_within(ctx: &CombatContext, me: Entity, my_pos: Vec3, radius: f32) -> bool {
    let team = self_team(ctx, me);
    ctx.combatants.values().any(|info| {
        !info.is_pet
            && info.team != team
            && info.is_alive
            && !info.stealthed
            && is_kite_threat(info.class)
            && info.position.distance(my_pos) <= radius
    })
}

/// A class whose melee damage warrants kiting (Warrior, Rogue). Deliberately
/// narrower than `CharacterClass::is_melee()`, which also counts the Paladin —
/// the Paladin's melee is not a kiting pressure threat here.
fn is_kite_threat(class: CharacterClass) -> bool {
    matches!(class, CharacterClass::Warrior | CharacterClass::Rogue)
}

/// Nearest kite-threat melee enemy (Warrior/Rogue) to `my_pos`, if any. The
/// Hunter's preferred Frost Trap peel target: a slow zone is most valuable under
/// the melee that's pressuring it, not on a pet or a stray-closest caster.
/// Stealthed and pet enemies are excluded (same visibility rules as
/// `melee_within`).
pub fn nearest_melee_threat(
    ctx: &CombatContext,
    me: Entity,
    my_pos: Vec3,
) -> Option<(Entity, Vec3)> {
    let team = self_team(ctx, me);
    ctx.combatants
        .values()
        .filter(|i| {
            !i.is_pet
                && i.team != team
                && i.is_alive
                && !i.stealthed
                && is_kite_threat(i.class)
        })
        .min_by(|a, b| {
            a.position
                .distance(my_pos)
                .partial_cmp(&b.position.distance(my_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|i| (i.entity, i.position))
}

/// Whether an ENGAGE kiter should reposition to regain line of sight: true when
/// it is within its preferred (idle/shot) range of a living kill target but
/// OCCLUDED from it — the R10 stall case where normal pursuit stands still yet
/// every cast is LoS-blocked. false when it has a clear line (fire away), is
/// beyond preferred range (pursuit walks in and the collision resolver slides it
/// around the pillar, self-healing occlusion), or has no kill target. Always
/// false on obstacle-free maps: sight holds, so nothing is ever occluded — the
/// seek path is a provable no-op there.
fn should_seek_los(
    ctx: &CombatContext,
    entity: Entity,
    my_pos: Vec3,
    kill_target: Option<Entity>,
) -> bool {
    let Some(my_class) = ctx.combatants.get(&entity).map(|i| i.class) else {
        return false;
    };
    let Some(info) = kill_target.and_then(|t| ctx.combatants.get(&t)) else {
        return false;
    };
    if !info.is_alive {
        return false;
    }
    if my_pos.distance(info.position) > my_class.preferred_range() {
        return false; // pursuit closes the gap and clears the pillar on its own
    }
    let my_eye = Vec3::new(my_pos.x, EYE_HEIGHT, my_pos.z);
    let tgt_eye = Vec3::new(info.position.x, EYE_HEIGHT, info.position.z);
    // Occluded from an in-range kill target → reposition to regain sight.
    !has_line_of_sight(ctx.obstacles, my_eye, tgt_eye)
}

/// Occlusion-timeout chase arm/reset/decide seam (pure, unit-tested).
///
/// `seeking` is the in-shot-range-and-occluded stall signal (`should_seek_los`),
/// which already folds in "target alive" and "target in range" — so a dead
/// target, an out-of-range target, or regained sight all present as
/// `seeking == false`. Given the previous continuous-occlusion clock
/// (`prev_since`) and the target that clock was tracking (`prev_target`),
/// returns the updated `(occluded_since, chase)`:
/// - `seeking == false` → reset the clock (`None`), never chase.
/// - `seeking == true`, target changed → restart the clock at `now` (a swap must
///   re-earn the timeout even if the new target is also occluded).
/// - `seeking == true`, same target → keep the clock; chase once it has run for
///   `timeout` seconds.
///
/// `timeout == 0.0` disables the chase (the kiter keeps orbit-seeking). The
/// caller stores `occluded_target = kill_target` when `seeking`, else `None`.
fn seek_chase_decision(
    prev_since: Option<f32>,
    prev_target: Option<Entity>,
    kill_target: Option<Entity>,
    seeking: bool,
    now: f32,
    timeout: f32,
) -> (Option<f32>, bool) {
    if !seeking {
        return (None, false);
    }
    let since = if prev_target != kill_target {
        now // target swap (or first arm) restarts the continuous-occlusion clock
    } else {
        prev_since.unwrap_or(now)
    };
    let chase = timeout > 0.0 && now - since >= timeout;
    (Some(since), chase)
}

/// `los_seek` contribution of the winning direction: the weight when the
/// lookahead step can see the kill target, else `0.0`. Mirrors the `los_seek`
/// block in `score_direction`; emitted as the `los_seek` scorer term so seek /
/// kite decisions are trace-visible (`0.0` ⇒ no sighted step was chosen this
/// tick, or an obstacle-free map).
fn los_seek_term(chosen: Vec2, inputs: &ScorerInputs, weight: f32) -> f32 {
    if weight <= 0.0 {
        return 0.0;
    }
    let Some(target) = inputs.los_target else {
        return 0.0;
    };
    let next = inputs.my_pos + Vec3::new(chosen.x, 0.0, chosen.y) * inputs.lookahead;
    let cand_eye = Vec3::new(next.x, EYE_HEIGHT, next.z);
    let target_eye = Vec3::new(target.x, EYE_HEIGHT, target.z);
    if has_line_of_sight(&inputs.obstacles, cand_eye, target_eye) {
        weight
    } else {
        0.0
    }
}

/// Build the kiter's `ScorerInputs` for one scoring pass — shared by the KITE
/// orbit and the ENGAGE seek-LoS repositioning so both see identical
/// threat / range-band / `los_target` wiring. The caller supplies the committed
/// direction (`None` outside the anti-zigzag window, which disables the term).
fn build_kiter_inputs(
    ctx: &CombatContext,
    entity: Entity,
    my_pos: Vec3,
    kill_target: Option<Entity>,
    config: &DpsMovementConfig,
    committed_direction: Option<Vec2>,
) -> ScorerInputs {
    let self_team = self_team(ctx, entity);
    // Stealthed enemies are excluded — the kiter can't see them, so it must not
    // flee from a stealthed Rogue's position until stealth breaks.
    let threats: Vec<Vec3> = ctx
        .combatants
        .values()
        .filter(|i| !i.is_pet && i.team != self_team && i.is_alive && !i.stealthed)
        .map(|i| i.position)
        .collect();

    // Nearest threat for the distance-max `flee` term (Hunter). Deterministic:
    // threats are collected from a BTreeMap, so equal distances tie-break by
    // entity order.
    let nearest_threat = threats.iter().copied().min_by(|a, b| {
        a.distance(my_pos)
            .partial_cmp(&b.distance(my_pos))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let kill_target_info = kill_target
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|i| i.is_alive);
    let range_band = kill_target_info.map(|i| RangeBand {
        target: i.position,
        min: config.range_band_min,
        max: config.range_band_max,
    });
    // LoS-seek target: the kill target the kiter shoots.
    let los_target = kill_target_info.map(|i| i.position);

    ScorerInputs {
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
        obstacles: ctx.obstacles.to_vec(),
        los_target,
    }
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
        // Trace the KITE → ENGAGE exit (unchanged: fires on any KITE→ENGAGE
        // transition, independent of the seek repositioning below).
        if transitioned {
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

        // Seek-LoS: a kiter idle in shot range but OCCLUDED from the kill
        // target can't fire — without this it stalls behind a pillar forever
        // (R10). Run the scorer (los_seek steers toward a sighted angle while
        // range_band holds distance) to reposition. Otherwise — a clear line,
        // or out of range where normal pursuit closes the gap and slides around
        // the pillar — clear any stale kite vector and fall through to pursuit.
        // That "else" is the exact pre-existing ENGAGE behavior, and a provable no-op
        // on obstacle-free maps (never occluded).
        let seeking = should_seek_los(ctx, entity, my_pos, kill_target);

        // Occlusion-timeout direct chase: track how long the seek stall has
        // persisted (updated every tick, before the recommit gate). When it
        // exceeds `seek_chase_timeout`, orbit-seeking (a greedy per-step
        // `los_seek` with no gradient once every candidate is occluded) is
        // abandoned in favor of walking straight at the target — the collision
        // resolver slides the chaser around the pillar until sight returns.
        let (occluded_since, chase) = seek_chase_decision(
            state.occluded_since,
            state.occluded_target,
            kill_target,
            seeking,
            now,
            config.seek_chase_timeout,
        );
        state.occluded_since = occluded_since;
        state.occluded_target = if seeking { kill_target } else { None };

        if !seeking {
            if directive.is_some() {
                commands.entity(entity).remove::<MovementDirective>();
            }
            if needs_insert {
                commands.entity(entity).try_insert(*state);
            }
            return;
        }

        // Hold the committed direction for the anti-zigzag window; re-score on
        // transition or when the window/directive expired. Shared by the direct
        // chase and the orbit-seek scorer below.
        let recommit = transitioned
            || directive.map_or(true, |d| now >= d.committed_until || now >= d.expires);
        if !recommit {
            if needs_insert {
                commands.entity(entity).try_insert(*state);
            }
            return;
        }

        // Direct-chase branch: issue a Point directive toward the target's live
        // position (re-targeted each recommit as the target moves). The commit
        // window handles anti-zigzag; `directive_ttl` keeps the walk alive
        // between recommits and past the longest cast. Reuses the SeekLos
        // trigger — the Point vs Direction goal kind distinguishes chase from
        // orbit-seek in the trace. When sight returns the next decision tick
        // sees `seeking == false` and removes this directive.
        if chase {
            if let Some(target_pos) =
                kill_target.and_then(|t| ctx.combatants.get(&t)).map(|i| i.position)
            {
                commands.entity(entity).try_insert(MovementDirective {
                    goal: MovementGoal::Point(target_pos),
                    expires: now + config.directive_ttl,
                    committed_until: now + config.commit_window,
                });
                // A Point chase has no scored direction; a stale kite vector
                // must not leak into a later orbit-seek recommit.
                state.last_direction = None;
                if let Some(info) = ctx.combatants.get(&entity) {
                    let actor = ActorView::from_info(info);
                    let mut builder = decision_trace.start_movement_decision(actor, None);
                    builder.direction_change(
                        TracePosture::Engage,
                        MovementTrigger::SeekLos,
                        MovementGoalKind::Point,
                    );
                    builder.finish();
                }
            }
            if needs_insert {
                commands.entity(entity).try_insert(*state);
            }
            return;
        }

        let committed_direction = directive
            .filter(|d| now < d.committed_until)
            .and(state.last_direction);
        let inputs =
            build_kiter_inputs(ctx, entity, my_pos, kill_target, config, committed_direction);
        let chosen = score_directions(&compass_directions_16(), &inputs, &config.weights);
        if chosen != Vec2::ZERO {
            commands.entity(entity).try_insert(MovementDirective {
                goal: MovementGoal::Direction(chosen),
                expires: now + config.directive_ttl,
                committed_until: now + config.commit_window,
            });
            let direction_changed =
                state.last_direction.map_or(true, |d| d.distance(chosen) > 1e-3);
            state.last_direction = Some(chosen);
            if transitioned || direction_changed {
                if let Some(info) = ctx.combatants.get(&entity) {
                    let actor = ActorView::from_info(info);
                    let mut builder = decision_trace.start_movement_decision(actor, None);
                    builder.direction_change(
                        TracePosture::Engage,
                        MovementTrigger::SeekLos,
                        MovementGoalKind::Direction,
                    );
                    builder.chosen_direction([chosen.x, chosen.y]);
                    let (masked, los) = mask_and_los_bitmask(&compass_directions_16(), &inputs);
                    builder.masked(masked);
                    if los != 0 {
                        builder.los_masked(los);
                    }
                    builder.scorer_term(
                        "los_seek",
                        los_seek_term(chosen, &inputs, config.weights.los_seek),
                    );
                    builder.finish();
                }
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

    let committed_direction = directive
        .filter(|d| now < d.committed_until)
        .and(state.last_direction);

    let inputs = build_kiter_inputs(ctx, entity, my_pos, kill_target, config, committed_direction);
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
            let (masked, los) = mask_and_los_bitmask(&compass_directions_16(), &inputs);
            builder.masked(masked);
            if los != 0 {
                builder.los_masked(los);
            }
            builder.scorer_term(
                "los_seek",
                los_seek_term(chosen, &inputs, config.weights.los_seek),
            );
            builder.finish();
        }
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }
}

#[cfg(test)]
mod tests {
    use super::{is_kite_threat, seek_chase_decision};
    use crate::states::play_match::match_config::CharacterClass;
    use bevy::prelude::Entity;

    /// The occlusion-timeout chase arm/reset/decide seam. Covers: arm-and-hold
    /// until the timeout, fire at/after it, reset on regained sight, reset on a
    /// dead/out-of-range target (both surface as `seeking == false`), restart on
    /// a target swap, and the `0.0` = disabled convention.
    #[test]
    fn seek_chase_decision_arms_fires_and_resets() {
        let t0 = Entity::from_raw(7);
        let t1 = Entity::from_raw(9);
        let timeout = 3.5;

        // First occluded tick: arm the clock at `now`, do not chase yet.
        let (since, chase) = seek_chase_decision(None, None, Some(t0), true, 10.0, timeout);
        assert_eq!(since, Some(10.0), "clock arms at the first occluded tick");
        assert!(!chase, "no chase before the timeout elapses");

        // Still occluded, same target, before the timeout: hold, no chase.
        let (since, chase) =
            seek_chase_decision(Some(10.0), Some(t0), Some(t0), true, 13.0, timeout);
        assert_eq!(since, Some(10.0), "clock persists while continuously occluded");
        assert!(!chase, "3.0s < 3.5s timeout — still no chase");

        // Timeout reached: chase, clock unchanged.
        let (since, chase) =
            seek_chase_decision(Some(10.0), Some(t0), Some(t0), true, 13.5, timeout);
        assert_eq!(since, Some(10.0));
        assert!(chase, "occluded for exactly the timeout → chase");

        // Sight regained (seeking == false, e.g. LoS clear / target dead / out
        // of range): reset the clock, never chase.
        let (since, chase) =
            seek_chase_decision(Some(10.0), Some(t0), Some(t0), false, 14.0, timeout);
        assert_eq!(since, None, "regained sight / lost target resets the clock");
        assert!(!chase);

        // Target swap while still occluded: restart the clock, do not chase on
        // the swap tick even though the old clock had aged past the timeout.
        let (since, chase) =
            seek_chase_decision(Some(10.0), Some(t0), Some(t1), true, 15.0, timeout);
        assert_eq!(since, Some(15.0), "a target swap restarts the continuous-occlusion clock");
        assert!(!chase, "the swapped-to target must re-earn the timeout");

        // `0.0` disables the chase even when occluded far past any window.
        let (_since, chase) =
            seek_chase_decision(Some(0.0), Some(t0), Some(t0), true, 100.0, 0.0);
        assert!(!chase, "seek_chase_timeout == 0.0 disables the direct chase");
    }

    /// Regression guard for the melee-only kite filter: the Hunter kites ONLY
    /// melee-DPS threats. Ranged classes are excluded (the Hunter holds at shot
    /// range and trades instead of fleeing a caster — the bug this fixed), and
    /// the Paladin is excluded too (its melee isn't pressure; avoiding its
    /// Hammer of Justice is a separate avoid-CC concern). Pinning the exact set
    /// catches a regression that re-adds a caster or drops Rogue — which an
    /// integration probe can't reliably catch, since ranged enemies rarely enter
    /// the kite radius in a real match.
    #[test]
    fn kite_threat_is_warrior_and_rogue_only() {
        assert!(is_kite_threat(CharacterClass::Warrior), "Warrior is a kite threat");
        assert!(is_kite_threat(CharacterClass::Rogue), "Rogue is a kite threat");
        for ranged in [
            CharacterClass::Mage,
            CharacterClass::Warlock,
            CharacterClass::Priest,
            CharacterClass::Paladin,
            CharacterClass::Hunter,
        ] {
            assert!(
                !is_kite_threat(ranged),
                "{:?} must NOT be a kite threat — the Hunter holds and shoots, it does not flee",
                ranged
            );
        }
    }
}
