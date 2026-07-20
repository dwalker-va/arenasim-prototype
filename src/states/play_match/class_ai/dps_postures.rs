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
    AuraType, Combatant, DpsPosture, KitePosture, MatchCountdown, MovementDirective, MovementGoal,
};
use crate::states::play_match::constants::MELEE_RANGE;
use crate::states::play_match::map_config::ActiveMapGeometry;
use crate::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use crate::states::play_match::match_config::CharacterClass;
use crate::states::play_match::decision_trace::{
    ActorView, DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::movement_config::{DpsMovementConfig, MovementConfig};

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
    occluded_in_range(ctx.obstacles, my_pos, my_class, info.position, info.is_alive)
}

/// Occluded-in-shot-range test from raw primitives (no `CombatContext`). Shared
/// by `should_seek_los` (ctx-based, ability pass) and `tick_kite_occlusion`
/// (per-frame accumulator, which has no snapshot). True when the kill target is
/// alive, within the kiter's preferred (idle/shot) range, and NOT in line of
/// sight — the R10 stall where a kiter stands in range yet every cast is
/// LoS-blocked. False when dead, beyond preferred range (pursuit closes the gap
/// and the collision resolver slides around the pillar), or sighted. Always
/// false on obstacle-free maps: sight holds, so nothing is ever occluded.
fn occluded_in_range(
    obstacles: &[ObstacleVolume],
    my_pos: Vec3,
    my_class: CharacterClass,
    target_pos: Vec3,
    target_alive: bool,
) -> bool {
    if !target_alive {
        return false;
    }
    if my_pos.distance(target_pos) > my_class.preferred_range() {
        return false;
    }
    let my_eye = Vec3::new(my_pos.x, EYE_HEIGHT, my_pos.z);
    let tgt_eye = Vec3::new(target_pos.x, EYE_HEIGHT, target_pos.z);
    !has_line_of_sight(obstacles, my_eye, tgt_eye)
}

/// Leaky-bucket occlusion accumulator update (pure, unit-tested).
///
/// The chase's arm signal. Replaces the old continuous-occlusion clock: instead
/// of requiring UNBROKEN occlusion, occlusion accrues into a bucket that fills
/// while occluded and drains (sub-fill) while sighted, so an intermittently
/// juking target — occlude mid-cast, flash back between casts — still ratchets
/// toward the arm threshold rather than resetting on every flicker.
///
/// `occluded` is the in-shot-range-and-occluded signal (`occluded_in_range`),
/// which already folds in "target alive" and "target in range" — a dead target,
/// an out-of-range target, or regained sight all present as `occluded == false`.
/// `kill_target` is the current LIVING kill target (the caller passes `None`
/// when there is no living target, which resets the bucket and unbinds).
///
/// Given the previous bucket (`prev_accum`) and the target it was bound to
/// (`prev_target`), returns the updated `(accum, bound_target)`:
/// - `kill_target == None` → reset to 0, unbind.
/// - target changed from the bound target → reset to 0 first (the swapped-to
///   target must re-earn the threshold), then apply this frame's fill/drain.
/// - `occluded` → fill by `dt` (a fixed 1.0/sec).
/// - sighted / out of range → drain by `decay * dt`, clamped at 0.
///
/// The caller ARMS the chase when the returned `accum >= timeout`
/// (`timeout == 0.0` disables the chase). Under CONTINUOUS occlusion the bucket
/// fills at 1.0/sec, so it arms at exactly `timeout` seconds — the static
/// pillar-hug case is byte-identical to the old clock. `decay == 0.0` never
/// drains (permanent arm once crossed); `decay >= 1.0` drains at least as fast
/// as it fills, restoring continuous-only arming.
fn seek_chase_accumulate(
    prev_accum: f32,
    prev_target: Option<Entity>,
    kill_target: Option<Entity>,
    occluded: bool,
    dt: f32,
    decay: f32,
) -> (f32, Option<Entity>) {
    let Some(target) = kill_target else {
        return (0.0, None); // no living target → reset + unbind
    };
    // A target swap (or first bind) resets the bucket so the new target re-earns
    // the threshold even if it is also occluded.
    let base = if prev_target == Some(target) { prev_accum } else { 0.0 };
    let accum = if occluded {
        base + dt
    } else {
        (base - decay * dt).max(0.0)
    };
    (accum.max(0.0), Some(target))
}

/// Per-frame occlusion accumulator tick — the SOLE owner of
/// `KitePosture::occlusion_accum` / `occluded_target`. Runs every frame for
/// every living Mage/Hunter kiter REGARDLESS of casting state, so the mid-cast
/// juke (which the `Without<CastingState>` ability-decision query never sees) is
/// still observed and accrued. `evaluate_dps_posture` only READS the bucket to
/// decide whether the chase is armed.
///
/// Gated on gates-open (no pre-match accrual). Deterministic: uses fixed-step
/// `Time::delta_secs` and a `BTreeMap`-free direct target lookup. Provable no-op
/// on obstacle-free maps — `occluded_in_range` is always false there, so the
/// bucket only ever drains toward 0 and the chase never arms. A kiter without a
/// `KitePosture` yet (before its first ENGAGE evaluation inserts one) is simply
/// not matched by the query; the component lands before its first cast, so the
/// juke window is always covered.
pub fn tick_kite_occlusion(
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    movement_config: Res<MovementConfig>,
    map_geometry: Res<ActiveMapGeometry>,
    mut kiters: Query<(&Transform, &Combatant, &mut KitePosture)>,
    others: Query<(&Transform, &Combatant)>,
) {
    if !countdown.gates_opened {
        return;
    }
    let dt = time.delta_secs();
    let now = time.elapsed_secs();

    for (transform, combatant, mut kite) in kiters.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        let cfg = match combatant.class {
            CharacterClass::Mage => &movement_config.mage,
            CharacterClass::Hunter => &movement_config.hunter,
            _ => continue, // only the two kiter classes carry a bucket
        };
        // While a Hunter Freezing-Trap dip owns movement, the ENGAGE/KITE
        // machine is skipped, so the chase can't fire; freeze the bucket rather
        // than accrue dip-walk occlusion into it.
        if kite.dipping(now) {
            continue;
        }

        let my_pos = transform.translation;
        // Resolve the LIVING kill target's position, if any.
        let target = combatant
            .target
            .and_then(|t| others.get(t).ok().map(|(tf, c)| (t, tf.translation, c.is_alive())))
            .filter(|(_, _, alive)| *alive);
        let kill_target = target.map(|(e, _, _)| e);
        let occluded = target.is_some_and(|(_, tpos, alive)| {
            occluded_in_range(&map_geometry.volumes, my_pos, combatant.class, tpos, alive)
        });

        let (accum, bound) = seek_chase_accumulate(
            kite.occlusion_accum,
            kite.occluded_target,
            kill_target,
            occluded,
            dt,
            cfg.seek_chase_decay,
        );
        kite.occlusion_accum = accum;
        kite.occluded_target = bound;
    }
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

        // Occlusion-chase arm: READ the leaky-bucket accumulator that
        // `tick_kite_occlusion` fills every frame (including mid-cast). The
        // chase is armed once the bucket reaches `seek_chase_timeout`; it fires
        // only while ALSO currently occluded (`seeking`). Armed-but-sighted
        // proceeds to normal ENGAGE casting — each chase leg ratchets range
        // down, so the kiter casts from progressively closer as sight returns.
        // The bucket itself is the hysteresis; no extra latch. This branch does
        // NOT mutate the accumulator — `tick_kite_occlusion` owns it.
        let armed = config.seek_chase_timeout > 0.0
            && state.occlusion_accum >= config.seek_chase_timeout;
        let chase = armed && seeking;

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
    use super::{is_kite_threat, seek_chase_accumulate};
    use crate::states::play_match::match_config::CharacterClass;
    use bevy::prelude::Entity;

    /// Fixed simulation step (headless `TimeUpdateStrategy::ManualDuration`).
    const DT: f32 = 1.0 / 60.0;
    /// Shipped arm threshold (occlusion units) and drain rate.
    const TIMEOUT: f32 = 3.5;
    const DECAY: f32 = 0.5;

    /// Fill / drain / clamp / reset-on-target-change / reset-on-death for the
    /// pure leaky-bucket accumulator.
    #[test]
    fn seek_chase_accumulate_fills_drains_and_resets() {
        let t0 = Entity::from_raw(7);
        let t1 = Entity::from_raw(9);

        // First occluded frame: bind + fill by dt.
        let (accum, bound) = seek_chase_accumulate(0.0, None, Some(t0), true, DT, DECAY);
        assert!((accum - DT).abs() < 1e-6, "fills by dt on the first occluded frame");
        assert_eq!(bound, Some(t0), "binds to the occluded target");

        // Occluded, same target: keeps filling by dt.
        let (accum, bound) = seek_chase_accumulate(1.0, Some(t0), Some(t0), true, DT, DECAY);
        assert!((accum - (1.0 + DT)).abs() < 1e-6, "continues filling while occluded");
        assert_eq!(bound, Some(t0));

        // Sighted, same target: drains by decay*dt (does NOT reset — survives a
        // sight flicker, the whole point of the bucket).
        let (accum, bound) = seek_chase_accumulate(1.0, Some(t0), Some(t0), false, DT, DECAY);
        assert!((accum - (1.0 - DECAY * DT)).abs() < 1e-6, "drains by decay*dt while sighted");
        assert_eq!(bound, Some(t0), "a sight flicker does not unbind");

        // Drain clamps at 0 (never negative).
        let (accum, _) = seek_chase_accumulate(0.0, Some(t0), Some(t0), false, DT, DECAY);
        assert_eq!(accum, 0.0, "drain clamps at 0");

        // Target swap: reset to 0, then this frame's fill applies to the new
        // target — the swapped-to target re-earns the threshold from scratch.
        let (accum, bound) = seek_chase_accumulate(3.0, Some(t0), Some(t1), true, DT, DECAY);
        assert!((accum - DT).abs() < 1e-6, "a target swap resets the bucket to 0 before filling");
        assert_eq!(bound, Some(t1));

        // No living target (death / none): reset to 0 and unbind.
        let (accum, bound) = seek_chase_accumulate(3.4, Some(t0), None, false, DT, DECAY);
        assert_eq!(accum, 0.0, "a dead/absent kill target resets the bucket");
        assert_eq!(bound, None, "and unbinds");
    }

    /// Continuous occlusion arms at EXACTLY `TIMEOUT` seconds — byte-identical
    /// to the old continuous clock's "3.5 uninterrupted seconds", proving the
    /// static pillar-hug case is unchanged.
    #[test]
    fn continuous_occlusion_arms_at_timeout_like_the_old_clock() {
        let t = Entity::from_raw(3);
        let mut accum = 0.0f32;
        let mut prev = None;
        let mut frames_to_arm = None;
        // 5 simulated seconds of unbroken occlusion.
        for frame in 1..=300 {
            let (a, b) = seek_chase_accumulate(accum, prev, Some(t), true, DT, DECAY);
            accum = a;
            prev = b;
            if frames_to_arm.is_none() && accum >= TIMEOUT {
                frames_to_arm = Some(frame);
            }
        }
        let armed_at = frames_to_arm.expect("continuous occlusion must arm");
        // 3.5s / (1/60) = 210 frames ideally; summing 1/60 in f32 crosses 3.5 on
        // frame 211 (the old clock compared f32 `elapsed_secs` deltas, so it
        // rounds through the exact same path) — the static case is unchanged.
        assert!(
            (210..=211).contains(&armed_at),
            "continuous fill arms at ~3.5s (210–211 frames), like the clock; got {armed_at}",
        );
    }

    /// Run a fill/drain cadence frame-by-frame; return the frame at which the
    /// bucket first reaches `TIMEOUT`, or `None` if it never arms over `secs`.
    /// `segments` is `[(occluded, seconds), ...]` repeated to fill the horizon.
    fn frames_to_arm(segments: &[(bool, f32)], decay: f32, secs: f32) -> Option<usize> {
        let t = Entity::from_raw(1);
        let total = (secs / DT) as usize;
        let mut accum = 0.0f32;
        let mut prev = None;
        // Build a per-frame occluded schedule by repeating the cadence.
        let mut schedule: Vec<bool> = Vec::new();
        while schedule.len() < total {
            for &(occ, dur) in segments {
                for _ in 0..((dur / DT).round() as usize) {
                    schedule.push(occ);
                }
            }
        }
        for (frame, &occ) in schedule.iter().take(total).enumerate() {
            let (a, b) = seek_chase_accumulate(accum, prev, Some(t), occ, DT, decay);
            accum = a;
            prev = b;
            if accum >= TIMEOUT {
                return Some(frame + 1);
            }
        }
        None
    }

    /// The observed mid-cast juke cadence: ~1.5s occluded (a Frostbolt started
    /// sighted, juked mid-cast) then ~1.5s sighted (the flash-back between
    /// casts). Table-driven over a few plausible sighted gaps.
    ///
    /// - At the shipped `decay = 0.5` (drain < fill) the bucket ratchets up and
    ///   arms within a handful of cycles — the fix.
    /// - Under a drain fast enough to EMPTY the bucket during each sighted gap
    ///   (the analog of the old clock's reset-on-flicker) the same cadence NEVER
    ///   arms: each 1.5s occluded segment alone is below the 3.5s threshold and
    ///   nothing carries across the gap — exactly the failure the old continuous
    ///   clock exhibited on a juking target, and precisely what the sub-fill
    ///   drain fixes.
    #[test]
    fn juke_cadence_arms_at_shipped_decay_but_never_when_flicker_empties_the_bucket() {
        // Net per cycle at decay 0.5 = 1.5 - 0.5*gap > 0 for every gap here, so
        // the bucket ratchets up regardless of the exact between-cast gap.
        let cases = [1.0f32, 1.5, 2.0];
        for gap in cases {
            let cadence = [(true, 1.5f32), (false, gap)];

            // decay 0.5: arms within a handful of cycles (each cycle is
            // 1.5+gap s; a "handful" ≤ 8 cycles even at the widest gap).
            let armed = frames_to_arm(&cadence, 0.5, 300.0)
                .unwrap_or_else(|| panic!("gap {gap}: juke must arm at decay 0.5"));
            let cycle_secs = 1.5 + gap;
            let cycles = armed as f32 * DT / cycle_secs;
            assert!(
                cycles <= 8.0,
                "gap {gap}: armed after {:.1} juke cycles (> 8) — not a handful",
                cycles
            );

            // A very large drain empties the bucket well within any sighted gap,
            // so occlusion never carries across a flicker — the reset-on-flicker
            // behavior of the old clock. Each 1.5s occluded window < 3.5s → never
            // arms.
            assert_eq!(
                frames_to_arm(&cadence, 100.0, 300.0),
                None,
                "gap {gap}: a flicker-emptying drain must never arm on a juking target",
            );
        }
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
