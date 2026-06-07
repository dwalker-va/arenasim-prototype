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
    compass_directions_16, score_directions, AnchorConstraint, ScorerInputs,
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
        builder.finish();
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
