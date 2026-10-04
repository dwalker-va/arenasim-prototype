//! The caster-healer posture machine (FREE / PRESSURED / ESCAPE).
//!
//! Shared by the two ranged healers with no offensive dip — the Shaman and the
//! Druid. Each runs the same machine against its own `movement.ron` block
//! ([`CasterHealerMovementConfig`]), so the two differ only in their tuning.
//! The Priest and the Paladin keep their own machines: both carry a dip.
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::states::play_match::combat_core::clamp_to_arena;
use crate::states::play_match::components::*;
use crate::states::play_match::decision_trace::{
    DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::movement_config::{CasterHealerMovementConfig, MovementConfig};

use super::healer_postures::{
    compound_pressure_trigger, escape_tick, escape_window_from, healer_pressured_tick_shared,
    medic_chase_override, medic_chase_tick, start_movement_event,
};
use super::CombatContext;

/// Per-tick output of [`evaluate_caster_healer_posture`] (mirrors
/// `PriestMovementPlan` minus the dip: neither caster healer has a Hammer of
/// Justice / Psychic Scream dip).
#[derive(Default)]
pub struct CasterHealerPlan {
    /// `Some(urgency_hp_threshold)` while an ESCAPE window or a medic walk is
    /// live: the rotation defers non-critical movement-locking casts
    /// meanwhile.
    pub escape_defer: Option<f32>,
    /// The live PRESSURED trigger this tick (`compound_pressure_trigger`),
    /// kept for parity with the Priest plan.
    pub pressured: bool,
    /// The teammate a live medic walk is walking to, if one is. The Druid's
    /// rotation holds its damage and utility casts while that teammate is out
    /// of its reach (`class_ai::druid`), so the global cooldown is free for
    /// the heal the walk is about to make possible.
    pub medic_walk: Option<Entity>,
}

// ============================================================================
// Posture evaluation (healer movement AI — FREE/PRESSURED/ESCAPE)
// ============================================================================
//
// A stripped copy of the Priest posture machine (`evaluate_priest_posture` /
// `pressured_tick` / `free_tick` / `compute_formation_point`) with ALL Dip
// branches removed. It reads the caller's own block (`movement.shaman` or
// `movement.druid`) instead of `movement.priest`, and uses the kill target as
// the FREE wand-pull source (a block with `wand_pull` > 0 drifts toward
// its caster's range of the kill target).

/// Evaluate a caster healer's movement posture (FREE/PRESSURED/ESCAPE) against
/// its own `block` and issue/refresh a [`MovementDirective`]. Runs at the top
/// of the healer's
/// decide tick (BEFORE the GCD short-circuit — the GCD locks casts, not legs),
/// and only after gates open (caller gates on `countdown.gates_opened`).
///
/// Returns a [`CasterHealerPlan`] whose `escape_defer` is
/// `Some(urgency_hp_threshold)` while an ESCAPE window is live — the rotation
/// defers non-critical movement-locking casts for the window.
///
/// `shifted` is the Druid's Travel Form (always `false` for the Shaman): a
/// shifted healer is in ESCAPE for as long as it stays shifted — the form
/// broke its roots and snares and it cannot cast, so running is the whole of
/// its job. It runs from every visible CHASER — a melee enemy or pet — inside
/// the intent radius (the form outruns legs, not spells) and re-scores its
/// direction each commit window.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_caster_healer_posture(
    commands: &mut Commands,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    posture: Option<&mut HealerPosture>,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    block: &CasterHealerMovementConfig,
    shifted: bool,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> CasterHealerPlan {
    let mut local = HealerPosture::new(now);
    let needs_insert = posture.is_none();
    let state: &mut HealerPosture = match posture {
        Some(p) => p,
        None => &mut local,
    };

    let shared = &movement.shared;

    // PRESSURED compound trigger (shared with the Priest/Paladin).
    let trigger = compound_pressure_trigger(entity, my_pos, ctx, shared);

    let prev = state.posture;

    // ESCAPE entry window: only while PRESSURED with the trigger still live and
    // every proximate visible threat movement-impaired.
    let escape_window_secs = if prev == Posture::Pressured && trigger {
        escape_window_from(
            ctx.visible_enemies_within(entity, my_pos, shared.danger_radius)
                .iter()
                .map(|t| ctx.attacker_escape_window(t.entity)),
            ctx.movement_slow_multiplier(entity),
            shared.escape_min_window,
        )
    } else {
        None
    };

    let next = match prev {
        _ if shifted => Posture::Escape,
        Posture::Escape if now < state.escape_until => Posture::Escape,
        Posture::Escape if trigger => Posture::Pressured,
        Posture::Escape => Posture::Free,
        Posture::Pressured if !trigger && now >= state.hold_until => Posture::Free,
        Posture::Pressured if escape_window_secs.is_some() => Posture::Escape,
        Posture::Pressured => Posture::Pressured,
        _ if trigger => Posture::Pressured,
        _ => Posture::Free,
    };

    let transitioned = next != prev;
    if transitioned {
        state.posture = next;
        state.since = now;
        state.last_direction = None;
        state.last_point = None;
        match next {
            Posture::Pressured => {
                state.hold_until = now + shared.pressured_hold;
            }
            Posture::Escape => {
                state.escape_until = now + escape_window_secs.unwrap_or(0.0);
            }
            _ => {
                state.hold_until = 0.0;
                state.anchor = None;
            }
        }
    }
    // A shift escape has no window to run out: it re-commits each commit
    // window for as long as the healer stays shifted.
    let shift_commit = shifted && (transitioned || now >= state.escape_until);
    if shift_commit {
        state.escape_until = now + shared.commit_window;
    }

    // Medic chase (shared) overrides FREE formation / PRESSURED denial when a
    // dying teammate is occluded — walk around cover to regain sight and heal.
    // The walk wins over a movement-locking cast that is not itself critical,
    // exactly as an ESCAPE window does: a hardcast heal on a less-hurt ally in
    // sight would root the healer short of the dying one.
    let medic_chase =
        medic_chase_override(entity, my_pos, next, ctx, shared, state, directive, now);
    let medic_walk = medic_chase.map(|ally| ally.entity);
    if let Some(ally) = medic_chase {
        medic_chase_tick(
            commands,
            entity,
            my_pos,
            ally,
            state,
            directive,
            shared,
            now,
            decision_trace,
            ctx,
        );
    } else {
        if state.medic_target.is_some() {
            commands.entity(entity).remove::<MovementDirective>();
            state.medic_target = None;
        }
        match next {
            Posture::Escape if shifted => escape_tick(
                commands,
                entity,
                my_pos,
                ctx,
                state,
                directive,
                shared,
                &block.weights,
                decision_trace,
                shift_commit,
                prev,
                if prev == Posture::Escape {
                    MovementTrigger::CommitExpired
                } else {
                    MovementTrigger::ShiftEscape
                },
                shared.threat_intent_radius,
                true,
            ),
            Posture::Escape => escape_tick(
                commands,
                entity,
                my_pos,
                ctx,
                state,
                directive,
                shared,
                &block.weights,
                decision_trace,
                transitioned,
                prev,
                MovementTrigger::EscapeWindowOpen,
                shared.danger_radius,
                false,
            ),
            Posture::Pressured => caster_healer_pressured_tick(
                commands,
                entity,
                combatant,
                my_pos,
                ctx,
                state,
                directive,
                movement,
                block,
                now,
                decision_trace,
                transitioned,
                prev,
            ),
            _ => caster_healer_free_tick(
                commands,
                entity,
                combatant,
                my_pos,
                ctx,
                state,
                directive,
                movement,
                block,
                now,
                decision_trace,
                transitioned,
                prev,
            ),
        }
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }

    let escape_defer = if medic_walk.is_some() || state.posture == Posture::Escape {
        Some(shared.urgency_hp_threshold)
    } else {
        None
    };

    CasterHealerPlan {
        escape_defer,
        pressured: trigger,
        medic_walk,
    }
}

/// PRESSURED tick: thin wrapper over [`healer_pressured_tick_shared`] with the
/// block's scorer weights, the kill target as the wand-pull (spell range)
/// source, and no retreat band (`fallback_range = None`).
#[allow(clippy::too_many_arguments)]
fn caster_healer_pressured_tick(
    commands: &mut Commands,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    block: &CasterHealerMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    healer_pressured_tick_shared(
        commands,
        entity,
        my_pos,
        ctx,
        state,
        directive,
        &movement.shared,
        &block.weights,
        combatant.target,
        None,
        // No ally dispel in the kit.
        None,
        now,
        decision_trace,
        transitioned,
        prev,
    );
}

/// FREE tick: formation-point anchoring. Degenerate case (no living non-pet
/// ally): NO directive — the legacy preferred_range pursuit governs.
#[allow(clippy::too_many_arguments)]
fn caster_healer_free_tick(
    commands: &mut Commands,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    block: &CasterHealerMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    let shared = &movement.shared;

    let exit_trigger = if prev == Posture::Escape {
        MovementTrigger::EscapeWindowClosed
    } else {
        MovementTrigger::PressuredExit
    };

    if transitioned {
        commands.entity(entity).remove::<MovementDirective>();
    }

    let Some(point) = compute_formation_point(entity, combatant, my_pos, ctx, movement, block)
    else {
        if transitioned {
            if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
                builder.transition(
                    prev.into(),
                    TracePosture::Free,
                    exit_trigger,
                    MovementGoalKind::Entity,
                );
                builder.finish();
            }
        }
        return;
    };

    let point_xz = Vec2::new(point.x, point.z);
    let my_xz = Vec2::new(my_pos.x, my_pos.z);
    let moved = state
        .last_point
        .is_none_or(|lp| lp.distance(point_xz) > block.formation_shift_threshold);
    let near = my_xz.distance(point_xz) <= block.formation_deadzone;

    let issue = |commands: &mut Commands| {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(point),
            expires: now + shared.directive_ttl,
            committed_until: now + shared.commit_window,
        });
    };

    if transitioned {
        issue(commands);
        state.last_point = Some(point_xz);
        if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
            builder.transition(
                prev.into(),
                TracePosture::Free,
                exit_trigger,
                MovementGoalKind::Point,
            );
            builder.finish();
        }
    } else if moved && !near {
        issue(commands);
        state.last_point = Some(point_xz);
        if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
            builder.direction_change(
                TracePosture::Free,
                MovementTrigger::FormationShift,
                MovementGoalKind::Point,
            );
            builder.finish();
        }
    } else if !near && directive.is_none_or(|d| d.expires - now < block.directive_refresh_margin) {
        issue(commands);
    }
}

/// FREE formation point: centroid of living non-pet ENGAGED allies (excluding
/// self), offset behind the line and biased toward arena center, clamped into
/// spell (wand) range of the kill target and into arena bounds.
/// `None` when no living non-pet ally exists (degenerate case).
fn compute_formation_point(
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    movement: &MovementConfig,
    block: &CasterHealerMovementConfig,
) -> Option<Vec3> {
    let shared = &movement.shared;
    let mut all_sum = Vec3::ZERO;
    let mut all_count = 0u32;
    let mut engaged_sum = Vec3::ZERO;
    let mut engaged_count = 0u32;
    for a in ctx.alive_allies() {
        if a.entity == entity {
            continue;
        }
        all_sum += a.position;
        all_count += 1;
        if a.target.is_some() {
            engaged_sum += a.position;
            engaged_count += 1;
        }
    }
    if all_count == 0 {
        return None;
    }
    let centroid = if engaged_count > 0 {
        engaged_sum / engaged_count as f32
    } else {
        all_sum / all_count as f32
    };

    let nearest_enemy = ctx
        .visible_enemies_within(entity, centroid, f32::MAX)
        .into_iter()
        .min_by(|a, b| {
            centroid
                .distance(a.position)
                .partial_cmp(&centroid.distance(b.position))
                .unwrap()
        });
    let away = match nearest_enemy {
        Some(e) => {
            Vec2::new(centroid.x - e.position.x, centroid.z - e.position.z).normalize_or_zero()
        }
        None => Vec2::new(centroid.x, centroid.z).normalize_or_zero(),
    };
    let to_center = Vec2::new(-centroid.x, -centroid.z).normalize_or_zero();
    let mut dir =
        (away * (1.0 - shared.center_bias) + to_center * shared.center_bias).normalize_or_zero();
    if dir == Vec2::ZERO {
        dir = away;
    }
    let mut point = Vec3::new(
        centroid.x + dir.x * shared.formation_offset,
        my_pos.y,
        centroid.z + dir.y * shared.formation_offset,
    );

    // Wand-range pull: clamp the point into spell range of the kill
    // target when it would sit outside it (weight 0 disables).
    if block.weights.wand_pull > 0.0 {
        if let Some(target) = combatant
            .target
            .and_then(|t| ctx.combatants.get(&t))
            .filter(|i| i.is_alive)
        {
            let offset = Vec2::new(point.x - target.position.x, point.z - target.position.z);
            let dist = offset.length();
            if dist > shared.wand_range {
                let clamped = offset / dist * shared.wand_range;
                point.x = target.position.x + clamped.x;
                point.z = target.position.z + clamped.y;
            }
        }
    }

    Some(super::healer_postures::standable_formation_point(
        ctx,
        clamp_to_arena(&ctx.bounds, point),
    ))
}
