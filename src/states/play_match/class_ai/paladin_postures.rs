//! Paladin-specific posture state machine (healer movement AI, U8).
//!
//! Extracted from `paladin.rs` (P1: keep that file focused on the ability
//! rotation). Holds the Paladin's FREE/PRESSURED/ESCAPE/DIP entry point
//! (`evaluate_paladin_posture`), the DIP entry/abort predicates, and the
//! per-posture ticks. The PRESSURED tick delegates to
//! `healer_postures::healer_pressured_tick_shared`; the ability rotation, the
//! `PaladinMovementPlan` / `HojPlan` interface types, and the HoJ / dip-target
//! helpers stay in `paladin.rs`.
//!
//! Behavior is identical to the pre-extraction code (the U8 posture probes
//! pin this).

use bevy::prelude::*;

use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::decision_trace::{
    DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::movement_config::{MovementConfig, SharedMovementConfig};

use super::cast_guard::{pre_cast_ok, PreCastOpts};
use super::healer_postures::{
    compound_pressure_trigger, escape_tick, escape_window_from, healer_pressured_tick_shared,
    start_movement_event, start_movement_event_with_target,
};
use super::paladin::{
    dip_target_candidate, hoj_target_eligible, rotation_hoj_allowed, HojPlan, PaladinMovementPlan,
};
use super::CombatContext;

/// Evaluate the Paladin's movement posture and issue/refresh a
/// [`MovementDirective`] accordingly. Runs at the top of the Paladin's decide
/// tick (mirroring `evaluate_priest_posture`): BEFORE the GCD short-circuit,
/// only after gates open, never for casting Paladins (R12 is structural —
/// `decide_abilities` excludes `CastingState` entities and `move_to_target`
/// blocks directive execution while casting).
///
/// The Paladin state machine differs from the Priest's in three ways (R8):
///
/// - **FREE keeps the melee identity.** No formation point, no directive —
///   the legacy `preferred_range 2.0` pursuit governs. The only FREE-side
///   behavior is the DIP entry check.
/// - **PRESSURED adds the healing-heavy trigger**: the Priest compound
///   trigger (focused) OR the lowest HP fraction across living non-pet team
///   members (self included) below `paladin.healing_heavy_hp`. Movement
///   retreats from threats toward `paladin.fallback_range` (band-hold: once
///   every threat is at/beyond the band, the Paladin stands and heals)
///   while staying within heal range of the anchor ally.
/// - **DIP (FREE → DIP only)**: a committed walk to the enemy healer for
///   Hammer of Justice. Entry requires HoJ ready (same `pre_cast_ok` gate
///   as the rotation), a stable teammate (anchor ally above the urgency HP
///   threshold and not CC'd — vacuously stable with no living teammate, so
///   1v1 Paladin-vs-healer still dips), and an eligible enemy healer within
///   `HoJ range + dip_budget × effective speed`. The dip aborts (→ FREE,
///   `DipAbort`) on teammate HP dive (AE3 — without casting), target
///   dead/immune/DR-immune/stealthed, or budget expiry; becoming focused
///   preempts unconditionally (→ PRESSURED, `PressuredEnter` — never
///   `DipAbort`). When the Paladin's kill target IS the enemy healer and is
///   already within HoJ range, the dip still runs as a zero-length dip:
///   `DipEnter` and `DipComplete` fire on the same decide tick and the cast
///   goes through the dip path, keeping every unpressured HoJ-on-healer
///   attributable to a dip in the trace (the documented choice for the
///   plan's deferred zero-length-dip question).
///
/// Returns the [`PaladinMovementPlan`] for this tick: `cast_defer` is
/// `Some(urgency_hp_threshold)` while an ESCAPE window or DIP is live (the
/// heal ladder defers non-critical movement-locking casts), and `hoj` gates
/// rotation Hammer of Justice (reservation while a living enemy healer
/// exists and the Paladin is unpressured; `DipCast` on dip arrival).
#[allow(clippy::too_many_arguments)]
pub fn evaluate_paladin_posture(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    posture: Option<&mut HealerPosture>,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> PaladinMovementPlan {
    // First evaluation inserts the persistent component via Commands
    // (visible to this tick's executor through the existing apply_deferred,
    // and to next tick's query).
    let mut local = HealerPosture::new(now);
    let needs_insert = posture.is_none();
    let state: &mut HealerPosture = match posture {
        Some(p) => p,
        None => &mut local,
    };

    let shared = &movement.shared;
    let pal = &movement.paladin;

    // --- PRESSURED triggers (R8) ---
    // Focused: the Priest's compound trigger (R6), shared verbatim.
    let focused = compound_pressure_trigger(entity, my_pos, ctx, shared);
    // Healing-heavy (R8, observable + deterministic): a living non-pet team
    // member (self included) is below `healing_heavy_hp` AND a melee/pet
    // enemy is inside the danger radius. The melee-pressure conjunct is
    // load-bearing — without it a Paladin whose teammate routinely dips to
    // ~0.4 in a normal melee scrum would retreat permanently, deleting its
    // melee identity in matchups with NO enemy healer (the U2 identity
    // probe caught exactly this). With the conjunct, healing-heavy fires
    // only when the team is hurting AND a melee is in the Paladin's face —
    // the situation a retreat-to-heal actually helps.
    let melee_pressure = ctx
        .visible_enemies_within(entity, my_pos, shared.danger_radius)
        .iter()
        .any(|t| t.is_pet || t.class.is_melee());
    // One `alive_allies()` snapshot reused for both the healing-heavy and the
    // degenerate-case gates below (it was allocated twice per tick).
    let allies = ctx.alive_allies();
    let team_hurting = allies.iter().any(|a| a.health_pct() < pal.healing_heavy_hp);
    let healing_heavy = team_hurting && melee_pressure;
    // Degenerate-case gate (the Priest's R5 no-ally rule, applied to the
    // Paladin's retreat): PRESSURED exists to protect the team's healing
    // capacity — fall back, keep the team alive from safety. With no living
    // non-pet ally there is no team to retreat FOR, and falling back only
    // deletes the Paladin's melee output (it can heal itself from anywhere).
    // Validation caught the failure: every Paladin 1v1 collapsed (e.g. the
    // Paladin permanently kiting a Hunter's pet into a 300s draw, 85
    // PressuredEnter/Exit strobes per match). Melee identity governs when
    // alone; dips and rotation HoJ still apply (`alive_allies` includes
    // self, so require an ally other than us).
    let has_teammate = allies.iter().any(|a| a.entity != entity);
    let trigger = (focused || healing_heavy) && has_teammate;

    let prev = state.posture;

    // --- ESCAPE entry window (R7 machinery, reused) ---
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

    // --- DIP abort check (only while mid-dip and not being preempted) ---
    let dip_aborts = prev == Posture::Dip
        && !focused
        && dip_should_abort(state, combatant, ctx, shared, now);

    // --- DIP entry (FREE only, no pressure) ---
    let dip_entry = if prev == Posture::Free && !trigger {
        evaluate_dip_entry(entity, combatant, my_pos, auras, ctx, movement, abilities)
    } else {
        None
    };

    let next = match prev {
        // ESCAPE is committed for the whole window.
        Posture::Escape if now < state.escape_until => Posture::Escape,
        Posture::Escape if trigger => Posture::Pressured,
        Posture::Escape => Posture::Free,
        // PRESSURED hysteresis, then escape-window upgrade (same as Priest).
        Posture::Pressured if !trigger && now >= state.hold_until => Posture::Free,
        Posture::Pressured if escape_window_secs.is_some() => Posture::Escape,
        Posture::Pressured => Posture::Pressured,
        // DIP: becoming focused preempts UNCONDITIONALLY (PressuredEnter,
        // never DipAbort). Healing-heavy alone does NOT preempt a dip — the
        // teammate-HP abort (urgency threshold, below the healing-heavy
        // threshold) is the HP-based exit; after the abort lands in FREE,
        // healing-heavy flips the posture to PRESSURED on the next tick.
        Posture::Dip if focused => Posture::Pressured,
        Posture::Dip if dip_aborts => Posture::Free,
        Posture::Dip => Posture::Dip,
        // FREE: pressure first, then the dip opportunity.
        _ if trigger => Posture::Pressured,
        _ if dip_entry.is_some() => Posture::Dip,
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
                state.dip_target = None;
                state.dip_until = 0.0;
            }
            Posture::Escape => {
                state.escape_until = now + escape_window_secs.unwrap_or(0.0);
            }
            Posture::Dip => {
                state.dip_target = dip_entry;
                state.dip_until = now + pal.dip_budget;
                state.hold_until = 0.0;
                state.anchor = None;
            }
            _ => {
                state.hold_until = 0.0;
                state.anchor = None;
                state.dip_target = None;
                state.dip_until = 0.0;
            }
        }
    }

    let mut plan = PaladinMovementPlan::default();

    match next {
        Posture::Escape => {
            escape_tick(
                commands, entity, my_pos, ctx, state, directive, shared,
                &pal.weights, decision_trace, transitioned, prev,
            );
            plan.cast_defer = Some(shared.urgency_hp_threshold);
        }
        Posture::Pressured => paladin_pressured_tick(
            commands, entity, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
        Posture::Dip => {
            plan.cast_defer = Some(shared.urgency_hp_threshold);
            plan.hoj = paladin_dip_tick(
                commands, abilities, entity, my_pos, ctx, state, directive, now,
                decision_trace, transitioned, prev,
            );
        }
        _ => paladin_free_tick(commands, entity, ctx, decision_trace, transitioned, prev),
    }

    // HoJ reservation (R8) — unless the dip tick already claimed the cast.
    if !matches!(plan.hoj, HojPlan::DipCast { .. }) {
        let enemy_healer_alive = ctx
            .alive_enemies()
            .iter()
            .any(|e| e.class.is_healer());
        plan.hoj = if rotation_hoj_allowed(state.posture, enemy_healer_alive) {
            HojPlan::Rotation
        } else {
            HojPlan::Reserved
        };
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }

    plan
}

/// DIP entry predicate (R8): HoJ ready (the rotation's `pre_cast_ok` gate:
/// cooldown / mana / school lockout / silence), teammate stable (most
/// injured living non-pet teammate above the urgency HP threshold and not
/// CC'd; vacuously stable with no living teammate), and an eligible enemy
/// healer within `HoJ range + dip_budget × effective speed`. Returns the
/// dip target.
fn evaluate_dip_entry(
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    movement: &MovementConfig,
    abilities: &AbilityDefinitions,
) -> Option<Entity> {
    let def = abilities.get_unchecked(&AbilityType::HammerOfJustice);

    // HoJ ready — identical readiness gate to the rotation cast.
    if !pre_cast_ok(
        AbilityType::HammerOfJustice, def, combatant, my_pos, auras, None, ctx,
        PreCastOpts::default(),
    ) {
        return None;
    }

    // Teammate stable (AE3 precondition): the would-be anchor must not need
    // us mid-walk. No living teammate (1v1 / last alive) is vacuously stable.
    let teammate = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity)
        .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap());
    if let Some(t) = teammate {
        if t.health_pct() <= movement.shared.urgency_hp_threshold {
            return None;
        }
        if ctx.is_ccd(t.entity) {
            return None;
        }
    }

    // Enemy healer within reach: dip_budget seconds of (slow-adjusted)
    // walking plus the cast range itself.
    let reach = def.range
        + movement.paladin.dip_budget
            * combatant.base_movement_speed
            * ctx.movement_slow_multiplier(entity);
    dip_target_candidate(ctx, combatant.team, my_pos, reach)
}

/// Mid-dip abort conditions (R8/AE3), checked each tick while DIP holds and
/// the focused preempt did not fire: budget exceeded, dip target no longer
/// HoJ-eligible (dead / immune / stun-DR-immune / stealthed), or the anchor
/// teammate's HP at/below the urgency threshold (the dip aborts WITHOUT
/// casting — the heal fires immediately after, un-deferred, because the
/// abort clears `cast_defer` before the ability pass runs this same tick).
pub fn dip_should_abort(
    state: &HealerPosture,
    combatant: &Combatant,
    ctx: &CombatContext,
    shared: &SharedMovementConfig,
    now: f32,
) -> bool {
    let Some(target) = state.dip_target else {
        return true; // defensive — DIP always carries a target
    };
    if now >= state.dip_until {
        return true; // budget exceeded
    }
    if !hoj_target_eligible(ctx, combatant.team, target) {
        return true; // target dead / immune / DR-immune / stealthed
    }
    // Teammate HP dive (AE3).
    ctx.alive_allies()
        .into_iter()
        .filter(|a| a.entity != ctx.self_entity)
        .any(|a| a.health_pct() <= shared.urgency_hp_threshold)
}

/// PRESSURED tick (R8): retreat from threats toward `fallback_range`,
/// band-holding once every threat is at/beyond the band (stand and heal /
/// self-peel), constrained to heal range of the sticky anchor ally. Reuses
/// the Priest's commitment-window + scored-direction machinery with the
/// Paladin's weights (no formation pull, no wand pull).
#[allow(clippy::too_many_arguments)]
fn paladin_pressured_tick(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
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
        &movement.paladin.weights,
        // No wand — the Paladin's wand_pull weight is 0 in config.
        None,
        // Enable the retreat band: gather threats out to fallback_range and
        // park at the band to stand-and-heal instead of face-tanking at melee.
        Some(movement.paladin.fallback_range),
        now,
        decision_trace,
        transitioned,
        prev,
    );
}

/// FREE tick (R8): the Paladin's FREE is the legacy melee pursuit — NO
/// directive is ever issued (melee identity preserved). On transitions into
/// FREE the lingering directive is removed (a dip walk must stop
/// immediately) and the exit transition is traced; `DipComplete` exits are
/// emitted by the dip-cast path in `decide_paladin_action`, so a Dip → Free
/// transition seen HERE is always an abort.
fn paladin_free_tick(
    commands: &mut Commands,
    entity: Entity,
    ctx: &CombatContext,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    if !transitioned {
        return;
    }
    // Stop any committed walk from the previous posture immediately — FREE
    // must hand movement back to legacy pursuit, and a dip directive's
    // expiry can be seconds away.
    commands.entity(entity).remove::<MovementDirective>();

    let trigger = match prev {
        Posture::Dip => MovementTrigger::DipAbort,
        Posture::Escape => MovementTrigger::EscapeWindowClosed,
        _ => MovementTrigger::PressuredExit,
    };
    if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
        // goal_kind Entity records "legacy target pursuit governs" (same
        // convention as the Priest's degenerate FREE).
        builder.transition(prev.into(), TracePosture::Free, trigger, MovementGoalKind::Entity);
        builder.finish();
    }
}

/// DIP tick (R8): keep the Entity-goal pursuit directive alive for the whole
/// budget; on arrival (within HoJ range of the dip target) hand the cast to
/// the ability pass via [`HojPlan::DipCast`]. The directive expires at the
/// budget deadline, so a stunned/feared Paladin's stale dip walk dies with
/// it (executor-side expiry).
#[allow(clippy::too_many_arguments)]
fn paladin_dip_tick(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    now: f32,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) -> HojPlan {
    let Some(target) = state.dip_target else {
        return HojPlan::Reserved; // defensive — DIP always carries a target
    };

    let issue = |commands: &mut Commands| {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Entity(target),
            expires: state.dip_until,
            committed_until: state.dip_until,
        });
    };

    if transitioned {
        issue(commands);
        // DipEnter carries the goal entity context via the target view.
        if let Some(mut builder) =
            start_movement_event_with_target(decision_trace, ctx, target, my_pos)
        {
            builder.transition(
                prev.into(),
                TracePosture::Dip,
                MovementTrigger::DipEnter,
                MovementGoalKind::Entity,
            );
            builder.finish();
        }
    } else if directive.is_none() {
        // Defensive re-issue (e.g., the directive died across a short CC
        // that ended before the budget) — refreshes are not decisions.
        issue(commands);
    }

    // Arrival check: within HoJ range → command the cast. The ability pass
    // re-checks readiness/eligibility/range (try_dip_hammer_of_justice) and
    // on success installs `completed_state` (DipComplete → FREE).
    let def = abilities.get_unchecked(&AbilityType::HammerOfJustice);
    let in_range = ctx
        .combatants
        .get(&target)
        .map_or(false, |t| my_pos.distance(t.position) <= def.range);
    if in_range {
        let mut completed = *state;
        completed.posture = Posture::Free;
        completed.since = now;
        completed.hold_until = 0.0;
        completed.anchor = None;
        completed.dip_target = None;
        completed.dip_until = 0.0;
        completed.last_direction = None;
        completed.last_point = None;
        HojPlan::DipCast {
            target,
            completed_state: completed,
        }
    } else {
        HojPlan::Reserved
    }
}
