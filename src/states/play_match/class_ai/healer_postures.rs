//! Shared healer-posture helpers (healer movement AI, U6–U8).
//!
//! Code shared verbatim between the Priest (`priest.rs`, U6/U7) and Paladin
//! (`paladin.rs`, U8) posture state machines lives here: the PRESSURED
//! compound trigger, sticky anchor selection, the ESCAPE tick, and the
//! `movement_decision` builder plumbing. The per-class ENTRY POINTS
//! (`evaluate_priest_posture` / `evaluate_paladin_posture`) stay in their
//! class files — this module is mechanics, not policy.
//!
//! Everything here was extracted unchanged from `priest.rs` (U6/U7) when the
//! Paladin postures landed; Priest behavior is identical before and after the
//! extraction (the U6/U7 probe suites pin this).

use bevy::prelude::*;

use crate::states::play_match::combat_core::{
    compass_directions_16, mask_bitmask, score_directions, AnchorConstraint, ScorerInputs,
};
use crate::states::play_match::components::{HealerPosture, MovementDirective, MovementGoal, Posture};
use crate::states::play_match::decision_trace::{
    ActorView, DecisionTrace, MovementEventBuilder, MovementGoalKind, MovementTrigger,
    Posture as TracePosture, TargetView,
};
use crate::states::play_match::movement_config::{MovementWeights, SharedMovementConfig};

use super::{CombatContext, CombatantInfo};

/// Distance ahead at which the position scorer evaluates candidate steps.
pub(super) const SCORER_LOOKAHEAD: f32 = 2.0;

/// ESCAPE window math (R7), pure for unit testing.
///
/// `proximate_cc_remaining` holds, per threat within the danger radius, the
/// remaining Root/Stun/Incapacitate duration (`attacker_escape_window`) or
/// `None` for an unimpaired threat. Rules:
///
/// - **Multi-attacker rule:** a single unimpaired proximate threat voids the
///   window (`None` anywhere → no ESCAPE).
/// - **Empty set:** no proximate threat → nothing to escape from → no window.
/// - **Window duration:** min over the impaired threats of their remaining CC
///   (the first attacker to break free ends the useful window).
/// - **Sub-cutoff rule (slow-adjusted):** the window is only worth a heal
///   deferral if it buys real distance. Distance gained ≈ window ×
///   base_speed × slow_multiplier (see [`escape_distance_gained`]), so the
///   slow-adjusted *effective* window is `window × slow_multiplier`. If that
///   falls below `min_window` (config `shared.escape_min_window`, calibrated
///   at full speed), do not enter ESCAPE — a 50%-slowed Priest needs twice
///   the CC time to gain the same separation.
///
/// Returns the RAW window duration in seconds (the directive/posture hold
/// time — the slowed Priest still escapes for the full CC duration once the
/// window is worth entering).
pub fn escape_window(
    proximate_cc_remaining: &[Option<f32>],
    slow_multiplier: f32,
    min_window: f32,
) -> Option<f32> {
    escape_window_from(
        proximate_cc_remaining.iter().copied(),
        slow_multiplier,
        min_window,
    )
}

/// Streaming form of [`escape_window`]: folds the per-attacker CC windows
/// straight off an iterator instead of collecting them into a
/// `Vec<Option<f32>>` first (the posture eval runs this every PRESSURED tick).
/// Result is identical to `escape_window(&collected, slow_multiplier,
/// min_window)` — same multi-attacker void, empty-set void, min-window, and
/// slow-adjusted sub-cutoff rules.
pub(super) fn escape_window_from<I: IntoIterator<Item = Option<f32>>>(
    proximate_cc_remaining: I,
    slow_multiplier: f32,
    min_window: f32,
) -> Option<f32> {
    let mut window = f32::MAX;
    let mut any = false;
    for cc in proximate_cc_remaining {
        any = true;
        match cc {
            Some(remaining) => window = window.min(remaining),
            // Multi-attacker rule: one free proximate threat voids the window.
            None => return None,
        }
    }
    // Empty set: no proximate threat → nothing to escape from → no window.
    if !any {
        return None;
    }
    // Sub-cutoff rule, slow-adjusted: effective window = raw × slow multiplier.
    if window * slow_multiplier < min_window {
        return None;
    }
    Some(window)
}

/// Distance gained over an ESCAPE window: `window × base_speed ×
/// slow_multiplier`. A 50% slow (`slow_multiplier = 0.5`) halves the
/// effective escape distance — this is the relationship the sub-cutoff rule
/// in [`escape_window`] is built on.
pub fn escape_distance_gained(window: f32, base_speed: f32, slow_multiplier: f32) -> f32 {
    window * base_speed * slow_multiplier
}

/// PRESSURED compound trigger (R6): targeted by a VISIBLE enemy
/// (`enemies_targeting` is stealth-filtered — AE2: no pre-dodging invisible
/// Rogues; pets included) AND a proximity / intent condition: within the
/// danger radius, or a melee-class / pet / closing threat within the intent
/// radius. A distant caster holding position while targeting me does NOT
/// flip the posture (AE5), and neither does a melee targeting me from across
/// the arena — pressure requires the threat to be near enough that intent
/// matters.
pub(super) fn compound_pressure_trigger(
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    shared: &SharedMovementConfig,
) -> bool {
    ctx.enemies_targeting(entity).iter().any(|t| {
        let distance = my_pos.distance(t.position);
        distance <= shared.danger_radius
            || (distance <= shared.threat_intent_radius
                && (t.is_pet || t.class.is_melee() || ctx.is_closing(t.entity, entity)))
    })
}

/// Sticky anchor ally (R6): most-injured living non-pet ally, excluding
/// self (the constraint keeps US within heal range of THEM). Switching
/// requires the candidate to be more injured than the current anchor by
/// `anchor_switch_margin`, so two similarly-injured allies don't flap the
/// constraint region tick to tick. BTree iteration + strict `<` keeps
/// ties deterministic. Shared by PRESSURED and ESCAPE (the escape direction
/// honors the same heal-range constraint). Updates `state.anchor`.
pub(super) fn select_sticky_anchor<'c>(
    entity: Entity,
    ctx: &'c CombatContext,
    state: &mut HealerPosture,
    shared: &SharedMovementConfig,
) -> Option<&'c CombatantInfo> {
    let candidate = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity)
        .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap());
    let current = state
        .anchor
        .and_then(|a| ctx.combatants.get(&a))
        .filter(|i| i.is_alive && !i.is_pet);
    let anchor_info: Option<&CombatantInfo> = match (current, candidate) {
        (Some(cur), Some(cand))
            if cand.entity != cur.entity
                && cand.health_pct() + shared.anchor_switch_margin < cur.health_pct() =>
        {
            Some(cand)
        }
        (Some(cur), _) => Some(cur),
        (None, cand) => cand,
    };
    state.anchor = anchor_info.map(|i| i.entity);
    anchor_info
}

/// ESCAPE tick (R7): on entry, score one direction with attacker repulsion
/// dominant — threats are the impaired proximate attackers; the formation
/// and wand pulls are OFF so repulsion is the only directional soft term,
/// while the ally-anchor heal-range constraint and the boundary/corner
/// penalties stay ACTIVE (escapes bend along walls instead of pinning into
/// them, and never leave heal range of the anchor). The directive is
/// committed for the whole window (`expires == committed_until ==
/// escape_until`): mid-window ticks re-issue defensively but never re-score
/// or re-emit.
///
/// `weights` selects the per-class scorer weights (Priest U7, Paladin U8) —
/// everything else is class-independent.
#[allow(clippy::too_many_arguments)]
pub(super) fn escape_tick(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    shared: &SharedMovementConfig,
    weights: &MovementWeights,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    if !transitioned {
        // Committed mid-window: keep the directive alive if it somehow died
        // (its expiry equals the window end, so this is defensive only) —
        // refreshes are not decisions, so no re-score and no trace event.
        if directive.is_none() {
            if let Some(dir) = state.last_direction {
                commands.entity(entity).try_insert(MovementDirective {
                    goal: MovementGoal::Direction(dir),
                    expires: state.escape_until,
                    committed_until: state.escape_until,
                });
            }
        }
        return;
    }

    // Same sticky anchor as PRESSURED — the heal-range constraint stays hard
    // during the escape (a window must never carry the healer out of range
    // of the ally it exists to keep healing).
    let anchor_info = select_sticky_anchor(entity, ctx, state, shared);

    // Threats: the impaired proximate attackers (ESCAPE entry guarantees
    // every visible enemy inside the danger radius is impaired right now).
    // BTreeMap for deterministic scorer input order.
    let mut threat_positions: std::collections::BTreeMap<Entity, Vec3> = Default::default();
    for t in ctx.visible_enemies_within(entity, my_pos, shared.danger_radius) {
        threat_positions.insert(t.entity, t.position);
    }

    let inputs = ScorerInputs {
        my_pos,
        lookahead: SCORER_LOOKAHEAD,
        threats: threat_positions.into_values().collect(),
        anchor: anchor_info.map(|i| AnchorConstraint {
            pos: i.position,
            heal_range: shared.heal_range,
        }),
        formation_point: None,
        // No wand pull during an escape — repulsion must dominate, and a
        // pull toward any enemy would shrink the separation the window buys.
        wand_target: None,
        wand_range: shared.wand_range,
        committed_direction: None,
    };
    let chosen = score_directions(&compass_directions_16(), &inputs, weights);
    if chosen == Vec2::ZERO {
        return; // defensive — 16 candidates always yield a direction
    }

    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Direction(chosen),
        expires: state.escape_until,
        committed_until: state.escape_until,
    });
    state.last_direction = Some(chosen);

    if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
        builder.transition(
            prev.into(),
            TracePosture::Escape,
            MovementTrigger::EscapeWindowOpen,
            MovementGoalKind::Direction,
        );
        builder.chosen_direction([chosen.x, chosen.y]);
        builder.masked(mask_bitmask(&compass_directions_16(), &inputs));
        builder.finish();
    }
}

/// Shared PRESSURED tick (R6/R8): sticky anchor selection, hard-commitment
/// window, scored retreat direction, directive issuance, and the
/// transition/direction-change trace events. Extracted verbatim from the
/// Priest (`pressured_tick`) and Paladin (`paladin_pressured_tick`) copies;
/// the two class wrappers differ only in these parameters:
///
/// - `weights` — per-class scorer weights (Priest U7 vs Paladin U8).
/// - `wand_kill_target` — `Some(combatant.target)` for the wand-pull healer
///   (Priest); `None` for the wandless Paladin. The wand target is filtered
///   against the threat set INSIDE this function (a Priest never drifts toward
///   an enemy that is itself a threat — see the statue-probe guard), so it
///   takes the kill-target Entity, not a pre-resolved position.
/// - `fallback_range` — `Some(pal.fallback_range)` enables the Paladin's
///   retreat band: the threat set is gathered out to the band (wider than the
///   Priest's `danger_radius`), and once every threat is at/beyond the band
///   (or there is no proximate threat at all) a Point directive parks the
///   Paladin to stand-and-heal instead of face-tanking at melee. `None`
///   (Priest) skips the band-hold and gathers threats out to `danger_radius`.
///
/// Behavior is identical to the two pre-extraction copies on identical inputs
/// (the U6/U7/U8 posture probes pin this).
#[allow(clippy::too_many_arguments)]
pub(super) fn healer_pressured_tick_shared(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    shared: &SharedMovementConfig,
    weights: &MovementWeights,
    wand_kill_target: Option<Entity>,
    fallback_range: Option<f32>,
    now: f32,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    let anchor_info = select_sticky_anchor(entity, ctx, state, shared);

    // Hard commitment window (R11): re-evaluation happens only once the
    // committed window lapses (or the directive died — e.g. expired across a
    // heal cast). The scorer's commitment bonus applies only AT re-evaluation;
    // the two governors never stack.
    let window_open =
        directive.map_or(false, |d| now < d.committed_until && now < d.expires);
    if window_open && !transitioned {
        return;
    }

    // Threat set: visible enemies targeting me + any visible enemy inside the
    // threat radius (an enemy in my face is a threat even while it targets
    // someone else). The radius is the Paladin's retreat band when present,
    // else the Priest's danger radius. BTreeMap dedupes in deterministic order.
    let threat_radius = fallback_range.unwrap_or(shared.danger_radius);
    let mut threat_positions: std::collections::BTreeMap<Entity, Vec3> = Default::default();
    for t in ctx.enemies_targeting(entity) {
        threat_positions.insert(t.entity, t.position);
    }
    for t in ctx.visible_enemies_within(entity, my_pos, threat_radius) {
        threat_positions.insert(t.entity, t.position);
    }

    // Band-hold (Paladin only): once every threat is at/beyond fallback_range,
    // STOP — a Point directive at the current position parks the Paladin at the
    // band to heal (and self-peel: the reservation is released while
    // PRESSURED). Without the hold, the absent directive would fall through to
    // legacy melee pursuit and walk the Paladin straight back into the pressure
    // it just retreated from. Also covers healing-heavy pressure with no
    // proximate threat at all: no aimless wandering, no re-engage.
    if let Some(band) = fallback_range {
        let nearest = threat_positions
            .values()
            .map(|p| my_pos.distance(*p))
            .fold(f32::MAX, f32::min);
        if threat_positions.is_empty() || nearest >= band {
            commands.entity(entity).try_insert(MovementDirective {
                goal: MovementGoal::Point(my_pos),
                expires: now + shared.directive_ttl,
                committed_until: now + shared.commit_window,
            });
            state.last_direction = None;
            if transitioned {
                if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
                    let trigger = if prev == Posture::Escape {
                        MovementTrigger::EscapeWindowClosed
                    } else {
                        MovementTrigger::PressuredEnter
                    };
                    builder.transition(
                        prev.into(),
                        TracePosture::Pressured,
                        trigger,
                        // The band-hold is a Point goal (park at the band).
                        MovementGoalKind::Point,
                    );
                    builder.finish();
                }
            }
            return;
        }
    }

    // Wand pull (Priest only) — but never toward an enemy that is itself in the
    // threat set: drifting toward your own attacker would cancel the repulsion
    // term at mid range and park the healer at a standoff distance instead of
    // escaping (observed in the statue probe before this guard).
    let wand_target = wand_kill_target
        .filter(|t| !threat_positions.contains_key(t))
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|i| i.is_alive)
        .map(|i| i.position);

    let inputs = ScorerInputs {
        my_pos,
        lookahead: SCORER_LOOKAHEAD,
        threats: threat_positions.into_values().collect(),
        anchor: anchor_info.map(|i| AnchorConstraint {
            pos: i.position,
            heal_range: shared.heal_range,
        }),
        formation_point: None,
        wand_target,
        wand_range: shared.wand_range,
        // Committed direction is passed as-is. No mask guard is needed: a
        // masked committed bearing already loses (it is removed from the pool),
        // and commitment_bonus on the SURVIVING candidates is computed per
        // candidate from alignment with this reference vector — unaffected by
        // whether the reference's own candidate is masked. The mask refactor is
        // therefore identical to the old penalty scheme here, with or without a
        // guard; adding one would only inject a real (unwanted) trajectory delta.
        committed_direction: state.last_direction,
    };
    let chosen = score_directions(&compass_directions_16(), &inputs, weights);
    if chosen == Vec2::ZERO {
        return; // defensive — 16 candidates always yield a direction
    }

    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Direction(chosen),
        expires: now + shared.directive_ttl,
        committed_until: now + shared.commit_window,
    });

    let direction_changed = state
        .last_direction
        .map_or(true, |d| d.distance(chosen) > 1e-3);
    state.last_direction = Some(chosen);

    // Trace (R3): posture transitions and committed direction CHANGES only.
    if transitioned || direction_changed {
        if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
            if transitioned {
                // ESCAPE → PRESSURED is the window-expiry exit, not a fresh
                // pressure onset — trace it as EscapeWindowClosed. PressuredEnter
                // otherwise covers FREE → PRESSURED and the Paladin's DIP →
                // PRESSURED preempt.
                let trigger = if prev == Posture::Escape {
                    MovementTrigger::EscapeWindowClosed
                } else {
                    MovementTrigger::PressuredEnter
                };
                builder.transition(
                    prev.into(),
                    TracePosture::Pressured,
                    trigger,
                    MovementGoalKind::Direction,
                );
            } else {
                builder.direction_change(
                    TracePosture::Pressured,
                    MovementTrigger::CommitExpired,
                    MovementGoalKind::Direction,
                );
            }
            builder.chosen_direction([chosen.x, chosen.y]);
            builder.masked(mask_bitmask(&compass_directions_16(), &inputs));
            builder.finish();
        }
    }
}

/// Start a `movement_decision` builder for the current actor. `None` only
/// when the snapshot lacks self (defensive — shouldn't happen in dispatch).
pub(super) fn start_movement_event<'t>(
    decision_trace: &'t mut DecisionTrace,
    ctx: &CombatContext,
) -> Option<MovementEventBuilder<'t>> {
    let actor = ActorView::from_info(ctx.self_info()?);
    Some(decision_trace.start_movement_decision(actor, None))
}

/// Start a `movement_decision` builder carrying a goal-entity target view
/// (DIP events: the enemy healer the walk pursues). Falls back to no target
/// when the goal entity is missing from the snapshot.
pub(super) fn start_movement_event_with_target<'t>(
    decision_trace: &'t mut DecisionTrace,
    ctx: &CombatContext,
    goal: Entity,
    my_pos: Vec3,
) -> Option<MovementEventBuilder<'t>> {
    let actor = ActorView::from_info(ctx.self_info()?);
    let target = ctx
        .combatants
        .get(&goal)
        .map(|info| TargetView::from_info(info, my_pos));
    Some(decision_trace.start_movement_decision(actor, target))
}
