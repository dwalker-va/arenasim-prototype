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

use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::{
    compass_directions_16, mask_and_los_bitmask, score_directions, AnchorConstraint, ScorerInputs,
};
use crate::states::play_match::components::{
    DispelWalkPhase, HealerPosture, MovementDirective, MovementGoal, Posture,
};
use crate::states::play_match::decision_trace::{
    ActorView, DecisionTrace, MovementEventBuilder, MovementGoalKind, MovementTrigger,
    Posture as TracePosture, TargetView,
};
use crate::states::play_match::map_geometry::{has_line_of_sight, nearest_standable, EYE_HEIGHT};
use crate::states::play_match::movement_config::{MovementWeights, SharedMovementConfig};
use crate::states::play_match::team_solve::DISPEL_REACH_MARGIN;

use super::{pressing_when_ahead, CombatContext, CombatantInfo, OwedDispel};

/// Distance ahead at which the position scorer evaluates candidate steps.
pub(super) const SCORER_LOOKAHEAD: f32 = 2.0;

// ============================================================================
// Deny-posture cover_pull: urgency suppression + trace term
// ============================================================================

/// Urgency suppression predicate (settled requirement R11 — the AE4
/// counter): is a living non-pet TEAMMATE (excluding self) below
/// `urgency_hp_threshold` AND within heal range — someone this healer must save
/// rather than hide from? Self being low is deliberately NOT a trigger: a low
/// healer taking cover is correct self-preservation, not abandonment of a dying
/// ally.
pub(super) fn teammate_needs_saving(
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    shared: &SharedMovementConfig,
) -> bool {
    ctx.alive_allies().into_iter().any(|a| {
        a.entity != entity
            && !a.is_pet
            && a.health_pct() < shared.urgency_hp_threshold
            && my_pos.distance(a.position) <= shared.heal_range
    })
}

/// Zero `cover_pull` when denial should be OFF this tick; otherwise the weights
/// pass through unchanged. The `suppress` decision is either urgency (a teammate
/// needs saving, R11) OR press (own team is clearly ahead) — both mean
/// "stop hiding". Pure over the boolean so the seam is unit-testable without
/// building a snapshot. When `cover_pull` is already 0 (the DPS blocks, or a
/// class with denial disabled) this is a no-op copy, so nothing off the deny
/// path is disturbed.
pub(super) fn apply_cover_suppression(
    weights: &MovementWeights,
    suppress: bool,
) -> MovementWeights {
    if suppress && weights.cover_pull > 0.0 {
        MovementWeights {
            cover_pull: 0.0,
            ..*weights
        }
    } else {
        *weights
    }
}

/// The scorer weights for one PRESSURED/ESCAPE decision: the class weights with
/// `cover_pull` suppressed while a teammate needs saving OR the team is
/// pressing its advantage. Short-circuits the snapshot scan when denial
/// is disabled for the class (`cover_pull == 0`).
fn deny_weights(
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    shared: &SharedMovementConfig,
    weights: &MovementWeights,
) -> MovementWeights {
    if weights.cover_pull <= 0.0 {
        return *weights;
    }
    let suppress = teammate_needs_saving(entity, my_pos, ctx, shared)
        || pressing_when_ahead(ctx.team_hp_advantage(), shared.press_advantage_margin);
    apply_cover_suppression(weights, suppress)
}

/// Cover-pull contribution of the winning direction — the *effective*
/// `cover_pull` weight times the number of threats the lookahead step is
/// occluded from. Emitted as the `cover_pull` scorer term so the deny posture
/// is trace-visible: a `0.0` here means either no cover was available at the
/// chosen step or the urgency suppression zeroed the weight this tick. Pure;
/// mirrors the `cover_pull` block in `score_direction` (obstacle-free ⇒ 0).
fn cover_pull_term(chosen: Vec2, inputs: &ScorerInputs, cover_weight: f32) -> f32 {
    if cover_weight <= 0.0 {
        return 0.0;
    }
    let next = inputs.my_pos + Vec3::new(chosen.x, 0.0, chosen.y) * inputs.lookahead;
    let cand_eye = Vec3::new(next.x, EYE_HEIGHT, next.z);
    let occluded = inputs
        .threats
        .iter()
        .filter(|t| {
            !has_line_of_sight(&inputs.obstacles, cand_eye, Vec3::new(t.x, EYE_HEIGHT, t.z))
        })
        .count();
    cover_weight * occluded as f32
}

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
        // Multi-attacker rule: one free proximate threat voids the window
        // (`?` returns None from this function, exactly as the old match arm did).
        let remaining = cc?;
        window = window.min(remaining);
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

/// A FREE formation point the healer can actually stand on (AS-190).
///
/// The formation point is the ally centroid offset "behind the line" and
/// clamped into wand range and the arena — arithmetic that takes no account of
/// cover, so on a pillar map it can land inside a footprint: a goal the
/// executor only approaches and stalls short of. It is moved out through the
/// footprint's nearest face at the mover's height. A point outside every
/// footprint (always, on an obstacle-free map) comes back bit-identical; one no
/// projection frees is returned as it stands, to the executor's hold.
///
/// Both AI profiles (AS-190 `TeamPlan`, AS-203 `Legacy`): the defect is in
/// the point's arithmetic, not in either AI's use of it.
pub fn standable_formation_point(ctx: &CombatContext, point: Vec3) -> Vec3 {
    nearest_standable(ctx.obstacles, Vec2::new(point.x, point.z), point.y)
        .map_or(point, |p| Vec3::new(p.x, point.y, p.y))
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

// ============================================================================
// Medic chase (heal-seeking movement)
// ============================================================================
//
// R5 made heals LoS-gated but nothing moves the healer to REGAIN sight of a
// dying ally: FREE formation-follow has no sight requirement, and the
// PRESSURED anchor mask only constrains SCORED steps (and the all-masked
// fallback ladder drops the anchor constraint first). A healer standing
// pillar-side from a sub-urgency ally therefore has nothing pulling it around
// the pillar — the ally dies with heals silently LoS-rejected at cast start.
//
// The medic chase closes that gap: when a living, healable, non-pet teammate
// is below `urgency_hp_threshold` AND occluded from the healer, a direct
// `MovementGoal::Point` walk toward the ally's live position overrides the
// FREE formation / PRESSURED cover-deny movement (the existing urgency
// suppression already encodes ally-dying > healer-hiding; this extends it to
// ally-dying > formation/denial). Keying on OCCLUSION (not range) makes this a
// provable no-op on obstacle-free maps — BasicArena has no obstacles, so
// `has_line_of_sight` is always true and the chase never arms. The chase ends
// naturally when sight is regained (predicate false → normal posture logic
// resumes and the heal fires).

/// Pure medic-chase target pick: among `(entity, health_pct, occluded)`
/// candidates in deterministic (BTree entity) order, the most-injured one that
/// is BOTH below `threshold` AND occluded. Ties (equal health) resolve to the
/// earlier candidate (lowest entity), consistent with the sticky-anchor
/// convention. `None` when nothing qualifies.
fn pick_medic_target(candidates: &[(Entity, f32, bool)], threshold: f32) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for &(e, hp, occluded) in candidates {
        if hp >= threshold || !occluded {
            continue;
        }
        best = match best {
            Some((_, bhp)) if bhp <= hp => best,
            _ => Some((e, hp)),
        };
    }
    best.map(|(e, _)| e)
}

/// Medic-chase target: the most-injured living non-pet teammate (excluding
/// self) below `urgency_hp_threshold` AND currently OCCLUDED from the healer
/// (EYE_HEIGHT endpoints, same LoS convention as everywhere). `None` when no
/// such ally exists — including on every obstacle-free map, where sight always
/// holds. Out-of-range-but-sighted allies are deliberately NOT chased: the
/// anchor/formation machinery already keeps the healer near them; only a broken
/// SIGHT line needs the walk-around-cover behavior.
pub(super) fn medic_chase_target<'c>(
    entity: Entity,
    my_pos: Vec3,
    ctx: &'c CombatContext,
    shared: &SharedMovementConfig,
) -> Option<&'c CombatantInfo> {
    let my_eye = Vec3::new(my_pos.x, EYE_HEIGHT, my_pos.z);
    let candidates: Vec<(Entity, f32, bool)> = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity && !a.is_pet)
        .map(|a| {
            let ally_eye = Vec3::new(a.position.x, EYE_HEIGHT, a.position.z);
            let occluded = !has_line_of_sight(ctx.obstacles, my_eye, ally_eye);
            (a.entity, a.health_pct(), occluded)
        })
        .collect();
    let target = pick_medic_target(&candidates, shared.urgency_hp_threshold)?;
    ctx.combatants.get(&target)
}

// ---------------------------------------------------------------------------
// Cover-seek navigation (distant cover)
//
// `cover_pull` is a LOCAL gradient: it rewards a candidate step that is already
// occluded, evaluated one `SCORER_LOOKAHEAD` (2yd) ahead. That works when cover
// is within a step or two, which it always was in a 73x46 arena with pillars at
// x=+/-9. In a ~140yd arena the nearest pillar is tens of yards away, every
// candidate direction scores 0, and the term is flat — so a pressured healer
// never moves toward cover at all (measured: 0.00s occlusion per match, and only
// 2 cover_pull firings against 14-19 historically).
//
// Cover-seek supplies the missing navigation: when denial is active but NO local
// step is occluded, walk directly at the nearest standing spot that would break
// the threat's sight. Same shape as `medic_chase` — a `MovementGoal::Point` that
// overrides the scorer — and, like it, keyed on a condition that never holds on
// obstacle-free maps, so BasicArena stays byte-identical.
// ---------------------------------------------------------------------------

/// Whether the medic chase should override the normal movement tick this frame:
/// the walk is allowed ([`ally_walk_allowed`]) and a dying occluded teammate
/// exists. Returns that ally.
pub(super) fn medic_chase_override<'c>(
    entity: Entity,
    my_pos: Vec3,
    next: Posture,
    ctx: &'c CombatContext,
    shared: &SharedMovementConfig,
) -> Option<&'c CombatantInfo> {
    if !ally_walk_allowed(entity, next, ctx) {
        return None;
    }
    medic_chase_target(entity, my_pos, ctx, shared)
}

/// What a healer's `Legacy` dispel walk does this frame ([`dispel_chase_override`]).
pub(super) enum DispelStep<'c> {
    /// Walk toward this teammate: the dispel does not reach it yet.
    Walk(&'c CombatantInfo),
    /// The walk reached `ally` and the dispel is still owed: hold `at`, the
    /// point the walk reached, until it lands.
    Hold { ally: Entity, at: Vec3 },
}

/// Whether a healer's dispel walk should override the normal movement tick
/// this frame, and how. It runs while the walk is allowed
/// ([`ally_walk_allowed`]), the healer could cast `dispel` on arrival
/// ([`can_cast_dispel`](super::can_cast_dispel) — neither silenced nor locked
/// out of its school), and a teammate is held in urgent crowd control `dispel`
/// removes ([`owed_dispel`](super::owed_dispel)):
///
/// - beyond its range or behind cover: [`DispelStep::Walk`] to it, until it is
///   in range AND in sight, the two gates the dispel itself passes;
/// - reached, once the walk has run: on to `DISPEL_REACH_MARGIN` inside the
///   range, then [`DispelStep::Hold`] the point the walk reached until the
///   dispel lands or stops being owed. Releasing on reach
///   handed movement back to a posture that could step the healer straight
///   back out of range while the GCD ran, and the walk re-armed — a stutter at
///   the range edge.
///
/// **A dying teammate comes first**
/// ([`another_teammate_dying`](super::another_teammate_dying)): no walk and no
/// hold while any other living non-pet teammate is below
/// `urgency_hp_threshold` — the rule `TeamPlan`'s `DispelGoal` keeps. The medic
/// chase outranks the walk for a dying teammate the healer cannot see; this
/// covers the one it can.
#[allow(clippy::too_many_arguments)]
pub(super) fn dispel_chase_override<'c>(
    abilities: &AbilityDefinitions,
    entity: Entity,
    my_pos: Vec3,
    current_mana: f32,
    next: Posture,
    ctx: &'c CombatContext,
    dispel: AbilityType,
    shared: &SharedMovementConfig,
    state: &HealerPosture,
) -> Option<DispelStep<'c>> {
    if !ally_walk_allowed(entity, next, ctx)
        || !super::can_cast_dispel(ctx, abilities, entity, dispel)
    {
        return None;
    }
    let owed = super::owed_dispel(ctx, abilities, entity, my_pos, current_mana, dispel)?;
    let (OwedDispel::Reached(ally) | OwedDispel::Unreached(ally)) = owed;
    if super::another_teammate_dying(ctx, entity, ally, shared.urgency_hp_threshold) {
        return None;
    }
    let info = ctx.combatants.get(&ally)?;
    match (owed, state.dispel_walk) {
        (OwedDispel::Unreached(_), _) => Some(DispelStep::Walk(info)),
        (OwedDispel::Reached(_), DispelWalkPhase::Off) => None,
        // A live walk carries on to `DISPEL_REACH_MARGIN` inside the range
        // before it holds, and a hold resumes walking only once the teammate is
        // out of reach altogether: a band, so a teammate drifting along the
        // edge (a feared one running) does not flip walk and hold every frame.
        (OwedDispel::Reached(_), DispelWalkPhase::Walking) => {
            let range = abilities.get(&dispel)?.range;
            if my_pos.distance(info.position) > range - DISPEL_REACH_MARGIN {
                Some(DispelStep::Walk(info))
            } else {
                Some(DispelStep::Hold { ally, at: my_pos })
            }
        }
        (OwedDispel::Reached(_), DispelWalkPhase::Holding(at)) => {
            Some(DispelStep::Hold { ally, at })
        }
    }
}

/// Run the dispel walk's `step`: the walk to the teammate (traced
/// `DispelChase`, per [`ally_walk_tick`]), or the hold at its reach point
/// (traced `DispelHold` once, on reaching).
#[allow(clippy::too_many_arguments)]
pub(super) fn dispel_walk_tick(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    step: DispelStep,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    shared: &SharedMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
) {
    match step {
        DispelStep::Walk(ally) => {
            if matches!(state.dispel_walk, DispelWalkPhase::Holding(_)) {
                // The teammate left reach (a feared one runs): walk again,
                // re-targeted now rather than when the hold's window ends.
                state.medic_target = None;
            }
            state.dispel_walk = DispelWalkPhase::Walking;
            ally_walk_tick(
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
                MovementTrigger::DispelChase,
            );
        }
        DispelStep::Hold { ally, at } => {
            let reached_now = !matches!(state.dispel_walk, DispelWalkPhase::Holding(_));
            if reached_now || directive.is_none_or(|d| now >= d.expires) {
                commands.entity(entity).try_insert(MovementDirective {
                    goal: MovementGoal::Point(at),
                    expires: now + shared.directive_ttl,
                    committed_until: now + shared.commit_window,
                });
            }
            state.dispel_walk = DispelWalkPhase::Holding(at);
            state.medic_target = None;
            state.last_direction = None;
            state.last_point = None;
            if reached_now {
                if let Some(mut builder) =
                    start_movement_event_with_target(decision_trace, ctx, ally, my_pos)
                {
                    builder.direction_change(
                        state.posture.into(),
                        MovementTrigger::DispelHold,
                        MovementGoalKind::Point,
                    );
                    builder.finish();
                }
            }
        }
    }
}

/// End a live ally walk — the medic chase, the dispel walk or its hold — so the
/// posture's own tick takes movement back: drop the walk's directive and clear
/// its state. A no-op when no walk is live.
pub(super) fn release_ally_walk(
    commands: &mut Commands,
    entity: Entity,
    state: &mut HealerPosture,
) {
    if state.medic_target.is_some() || state.dispel_walk != DispelWalkPhase::Off {
        commands.entity(entity).remove::<MovementDirective>();
        state.medic_target = None;
        state.dispel_walk = DispelWalkPhase::Off;
    }
}

/// Whether a healer may take a direct walk to a teammate this frame: current
/// posture FREE or PRESSURED (never DIP — its own teammate-HP abort composes,
/// handing control back so the walk picks up the next decision — nor the
/// committed ESCAPE window), and the healer not itself hard-CC'd (a CC'd healer
/// can't move, and the directive would be stale on release; `is_ccd` includes
/// Root, which blocks movement too).
fn ally_walk_allowed(entity: Entity, next: Posture, ctx: &CombatContext) -> bool {
    if !matches!(next, Posture::Free | Posture::Pressured) || ctx.is_ccd(entity) {
        return false;
    }
    // RETIRED under `TeamPlan` IN PRESSURED ONLY — the sole posture the solve
    // runs in, so the only place the subsumption argument holds. A review
    // caught the first version disabling this in FREE as well, where a TeamPlan
    // healer with a dying occluded teammate had NOTHING walking it around the
    // pillar (the solve never executes in FREE).
    //
    // Medic-chase exists because `cover_pull` and `cover_seek` are mutually
    // exclusive with seeing your ally, so a healer hiding from threats needed a
    // separate override to walk back around cover and heal a dying teammate.
    // `OccupyCover` asks for cover AND sight of the ally in ONE query, so the
    // case medic-chase was invented for cannot arise: a position that loses the
    // ally's line is already a constraint violation.
    //
    // Measured before removing it, rather than assumed — disabling it under
    // TeamPlan left the 12-seed sweep materially unchanged (11/12 wins either
    // way, heal 348 vs 349, Warrior deaths 1/12 either way; the only movement
    // was occlusion 22% -> 20%, so it did still fire, just never decisively).
    // Leaving it live would mean two positioning authorities under one profile,
    // which is exactly the hand-arbitration step 4 exists to remove.
    //
    // The dispel walk (Paladin AS-180, Priest AS-187) inherits the same
    // retirement: under `TeamPlan` PRESSURED the solve is the one positioning
    // authority, and it walks to the teammate itself — a teammate owed a dispel
    // the healer cannot reach is the solve's `DispelGoal`
    // (`healer_pressured_tick_shared`).
    !(ctx.ai_profile.is_team_plan() && next == Posture::Pressured)
}

/// Issue/refresh the medic-chase directive toward `ally`'s live position and
/// emit the `SeekLos` movement event (reusing the attacker-chase convention:
/// SeekLos trigger + Point goal + the ally in the target view). Re-targets the
/// ally per commit window — mirrors the DPS direct chase in `dps_postures.rs`.
/// Sets `state.medic_target` so a first-arm / target-swap forces an immediate
/// re-target even mid-commit-window (a leftover formation/PRESSURED directive
/// never suppresses the takeover).
#[allow(clippy::too_many_arguments)]
pub(super) fn medic_chase_tick(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    ally: &CombatantInfo,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    shared: &SharedMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
) {
    // The medic chase takes over from any dispel walk: a later return to the
    // dispel starts a fresh walk rather than holding a point this chase left.
    state.dispel_walk = DispelWalkPhase::Off;
    ally_walk_tick(
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
        MovementTrigger::SeekLos,
    );
}

/// Issue/refresh a direct walk toward `ally`'s live position, traced under
/// `trigger`: the one walk both the medic chase (`SeekLos`) and the Paladin's
/// healers' dispel walk (`DispelChase`) run. `state.medic_target` marks EITHER walk as
/// live — its release (drop the directive, hand movement back to the posture)
/// is the same for both.
#[allow(clippy::too_many_arguments)]
pub(super) fn ally_walk_tick(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    ally: &CombatantInfo,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    shared: &SharedMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    trigger: MovementTrigger,
) {
    let recommit = state.medic_target != Some(ally.entity)
        || directive.is_none_or(|d| now >= d.committed_until || now >= d.expires);
    if !recommit {
        return; // still committed toward this ally — the walk continues, no re-emit
    }

    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Point(ally.position),
        expires: now + shared.directive_ttl,
        committed_until: now + shared.commit_window,
    });
    state.medic_target = Some(ally.entity);
    // No scored direction / formation point governs a chase — clear both so the
    // normal tick re-anchors cleanly once sight is regained.
    state.last_direction = None;
    state.last_point = None;

    if let Some(mut builder) =
        start_movement_event_with_target(decision_trace, ctx, ally.entity, my_pos)
    {
        builder.direction_change(state.posture.into(), trigger, MovementGoalKind::Point);
        builder.finish();
    }
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
///
/// `entry_trigger`, `threat_radius` and `chasers_only` describe the window:
/// an impairment window is `EscapeWindowOpen` from every threat inside the
/// danger radius; the Druid's Travel Form escape (`ShiftEscape`) runs from
/// the CHASERS (melee and pets) inside the intent radius — the form outruns
/// those and buys nothing against a spell, so a ranged enemy must not bend
/// the run back toward a chaser — and re-commits in windows while it stays
/// shifted, fleeing the nearest chaser and holding its last heading. A
/// re-commit (`prev` already ESCAPE) is traced as a direction change, not a
/// transition.
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
    entry_trigger: MovementTrigger,
    threat_radius: f32,
    chasers_only: bool,
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
    for t in ctx.visible_enemies_within(entity, my_pos, threat_radius) {
        if chasers_only && !(t.class.is_melee() || t.is_pet) {
            continue;
        }
        threat_positions.insert(t.entity, t.position);
    }

    // A chaser escape (Travel Form) outruns its chasers, so it maximises
    // distance from the nearest one (`flee`, constant at every range) rather
    // than only repelling by proximity — a chaser 10yd off is worth 1/11 of a
    // point of repulsion, too little to hold a heading against the edge and
    // cover terms, which then flip it back toward the chaser each window. It
    // also keeps its heading through the windows it chains (`commitment_bonus`
    // toward the last one). An impairment window carries neither: it is one
    // window, scored once.
    let xz_dist = |p: Vec3| Vec2::new(p.x - my_pos.x, p.z - my_pos.z).length();
    let (nearest_threat, committed_direction) = if chasers_only {
        (
            threat_positions
                .values()
                .copied()
                .min_by(|a, b| xz_dist(*a).total_cmp(&xz_dist(*b))),
            if prev == Posture::Escape {
                state.last_direction
            } else {
                None
            },
        )
    } else {
        (None, None)
    };

    let inputs = ScorerInputs {
        bounds: ctx.bounds,
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
        range_band: None,
        nearest_threat,
        committed_direction,
        obstacles: ctx.obstacles.to_vec(),
        // A healer does not leash to itself.
        healer_point: None,
        healer_leash_range: 0.0,
        // A healer throws no trap.
        trap_setup_point: None,
        trap_setup_range: 0.0,
        // No kill target tracked during an escape — repulsion, not LoS-seek,
        // drives the direction (and los_seek is 0.0 for healers regardless).
        los_target: None,
    };
    // Deny posture: use cover to break attacker LoS while escaping, unless a
    // teammate needs saving (urgency suppression zeroes cover_pull that tick).
    let eff_weights = deny_weights(entity, my_pos, ctx, shared, weights);
    let chosen = score_directions(&compass_directions_16(), &inputs, &eff_weights);
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
        if prev == Posture::Escape {
            builder.direction_change(
                TracePosture::Escape,
                entry_trigger,
                MovementGoalKind::Direction,
            );
        } else {
            builder.transition(
                prev.into(),
                TracePosture::Escape,
                entry_trigger,
                MovementGoalKind::Direction,
            );
        }
        builder.chosen_direction([chosen.x, chosen.y]);
        let (masked, los) = mask_and_los_bitmask(&compass_directions_16(), &inputs);
        builder.masked(masked);
        builder.scorer_term(
            "cover_pull",
            cover_pull_term(chosen, &inputs, eff_weights.cover_pull),
        );
        if los != 0 {
            builder.los_masked(los);
        }
        builder.finish();
    }
}

/// A healer's ally dispel, for the `TeamPlan` solve's dispel goal: which
/// dispel, the definitions to read its range and cost from, and the mana the
/// healer has to pay for it.
pub(super) struct HealerDispel<'a> {
    pub abilities: &'a AbilityDefinitions,
    pub ability: AbilityType,
    pub current_mana: f32,
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
/// - `dispel` — the healer's ally dispel (Priest, Paladin), `None` for a healer
///   without one. Read only under `TeamPlan`, where a teammate it owes an urgent
///   dispel it cannot reach becomes the solve's [`DispelGoal`](crate::states::play_match::team_solve::DispelGoal):
///   the PRESSURED healer walks until the dispel reaches, the one positioning
///   authority doing what the `Legacy` dispel walk does there.
///
/// Returns whether that walk drives the position this tick — the caller defers
/// non-critical casts for it, as for the `Legacy` walk. Always `false` under
/// `Legacy`.
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
    dispel: Option<HealerDispel>,
    now: f32,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) -> bool {
    let anchor_info = select_sticky_anchor(entity, ctx, state, shared);

    // Hard commitment window (R11): re-evaluation happens only once the
    // committed window lapses (or the directive died — e.g. expired across a
    // heal cast). The scorer's commitment bonus applies only AT re-evaluation;
    // the two governors never stack.
    let window_open = directive.is_some_and(|d| now < d.committed_until && now < d.expires);
    if window_open && !transitioned {
        // Still committed — to the dispel walk too, if that is what it is.
        return state.solve_dispel.is_some();
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

    // TeamPlan: a teammate this healer owes an urgent dispel it cannot reach
    // (AS-187). The solve walks to it; the band-hold below must not park the
    // Paladin short of it first.
    let dispel_goal = if ctx.ai_profile.is_team_plan() {
        dispel.and_then(|d| {
            crate::states::play_match::team_solve::dispel_goal(
                ctx,
                d.abilities,
                entity,
                my_pos,
                d.current_mana,
                d.ability,
                shared.urgency_hp_threshold,
                state.solve_dispel,
            )
        })
    } else {
        None
    };

    // Band-hold (Paladin only): once every threat is at/beyond fallback_range,
    // STOP — a Point directive at the current position parks the Paladin at the
    // band to heal (and self-peel: the reservation is released while
    // PRESSURED). Without the hold, the absent directive would fall through to
    // legacy melee pursuit and walk the Paladin straight back into the pressure
    // it just retreated from. Also covers healing-heavy pressure with no
    // proximate threat at all: no aimless wandering, no re-engage.
    if let Some(band) = fallback_range.filter(|_| dispel_goal.is_none()) {
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
            state.solve_dispel = None;
            return false;
        }
    }

    // STEP 4: under `TeamPlan`, the healer's position comes from the team solve
    // instead of the additive scorer. `OccupyCover` is one query for what
    // `cover_pull`, `cover_seek` and `medic_chase` express as three mutually
    // exclusive mechanisms — "hidden from their casters, in range of my ally,
    // and able to SEE my ally" — so all three are skipped here rather than
    // arbitrated against it.
    //
    // Placed BEFORE the scorer inputs are assembled: `ScorerInputs` clones the
    // whole obstacle list every tick and `deny_weights` walks the team, and none
    // of it is read on this path.
    //
    // Gated on the profile, so `Legacy` (which every recorded baseline and every
    // calibrated probe runs) is untouched and any drift is attributable to this
    // line alone. The ESCAPE and DIP windows are deliberately NOT rerouted: they
    // are committed scripts with their own abort conditions, not positioning.
    if ctx.ai_profile.is_team_plan() {
        let world = crate::states::play_match::team_solve::world_from_context(
            ctx,
            shared.heal_range,
            threat_radius,
            None,
        )
        .with_urgency_hp_threshold(shared.urgency_hp_threshold)
        .with_dispel_goal(entity, dispel_goal);
        // A healer with no living non-pet partner is not a healer any more:
        // `OccupyCover`'s sight and leash constraints go vacuous and the last
        // unit standing hides forever next to a corpse. It fights at range
        // instead (`HoldRange`, focused on its own current target). This rule
        // is applied HERE, at the live site — a review found the first version
        // lived only in `assign_intents`, which has no production caller, so
        // the behaviour commit 9318bdc advertised never actually executed.
        let has_partner = ctx
            .alive_allies()
            .iter()
            .any(|a| a.entity != entity && !a.is_pet);
        let (intent, focus) = if has_partner {
            // `OccupyCover` is defined against the healer's own ally and the
            // enemy casters, not against a focal unit.
            (
                crate::states::play_match::team_plan::RoleIntent::OccupyCover,
                None,
            )
        } else {
            let target_pos = ctx
                .self_info()
                .and_then(|me| me.target)
                .and_then(|t| ctx.combatants.get(&t))
                .filter(|t| t.is_alive)
                .map(|t| Vec2::new(t.position.x, t.position.z));
            (
                crate::states::play_match::team_plan::RoleIntent::HoldRange,
                target_pos,
            )
        };
        let spot =
            crate::states::play_match::team_solve::solve_position(intent, entity, &world, focus);
        // A `Point` goal, not a bearing: the chosen spot can be tens of yards
        // off and behind a pillar, and `Point` is the branch that tangent-steers
        // around one. `None` means the solve is already satisfied here — hold,
        // rather than falling through to the scorer, which would move the healer
        // off a good position for an interest term the solve has retired.
        let goal = spot.map_or(my_pos, |s| Vec3::new(s.x, my_pos.y, s.y));
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(goal),
            expires: now + shared.directive_ttl,
            committed_until: now + shared.commit_window,
        });
        state.last_direction =
            spot.map(|s| (s - Vec2::new(my_pos.x, my_pos.z)).normalize_or_zero());

        // Trace the transition on the same terms every other exit from this
        // function does. Without it a `TeamPlan` match records no PRESSURED
        // entries at all, and the documented `movement_decision` recipes go
        // blind on exactly the profile being investigated.
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
                    MovementGoalKind::Point,
                );
                builder.finish();
            }
        }

        // The dispel walk, traced as the `Legacy` one is: a `DispelChase` with
        // the teammate in the target view, once per walk (on arming, on a new
        // teammate, and on re-entering PRESSURED mid-walk) — never per re-solve.
        let walking_to = dispel_goal.map(|g| g.ally);
        if let Some(ally) = walking_to {
            if transitioned || state.solve_dispel != Some(ally) {
                if let Some(mut builder) =
                    start_movement_event_with_target(decision_trace, ctx, ally, my_pos)
                {
                    builder.direction_change(
                        TracePosture::Pressured,
                        MovementTrigger::DispelChase,
                        MovementGoalKind::Point,
                    );
                    builder.finish();
                }
            }
        }
        state.solve_dispel = walking_to;
        return walking_to.is_some();
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

    // LoS-seek target: the kill target the healer tracks (unfiltered — keeping
    // sight of it matters even when it is itself a threat). los_seek is 0.0 for
    // healers today, so this is faithful wiring, not yet a behavior change.
    let los_target = wand_kill_target
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|i| i.is_alive)
        .map(|i| i.position);

    let inputs = ScorerInputs {
        bounds: ctx.bounds,
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
        range_band: None,
        nearest_threat: None,
        // Committed direction is passed as-is. No mask guard is needed: a
        // masked committed bearing already loses (it is removed from the pool),
        // and commitment_bonus on the SURVIVING candidates is computed per
        // candidate from alignment with this reference vector — unaffected by
        // whether the reference's own candidate is masked. The mask refactor is
        // therefore identical to the old penalty scheme here, with or without a
        // guard; adding one would only inject a real (unwanted) trajectory delta.
        committed_direction: state.last_direction,
        obstacles: ctx.obstacles.to_vec(),
        // A healer does not leash to itself.
        healer_point: None,
        healer_leash_range: 0.0,
        // A healer throws no trap.
        trap_setup_point: None,
        trap_setup_range: 0.0,
        los_target,
    };
    // Deny posture: prefer a step that breaks attacker LoS (cover_pull),
    // unless a teammate needs saving — then urgency suppression zeroes it so the
    // healer is never pulled into cover while an ally is dying (R11).
    let eff_weights = deny_weights(entity, my_pos, ctx, shared, weights);

    let chosen = score_directions(&compass_directions_16(), &inputs, &eff_weights);
    if chosen == Vec2::ZERO {
        return false; // defensive — 16 candidates always yield a direction
    }

    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Direction(chosen),
        expires: now + shared.directive_ttl,
        committed_until: now + shared.commit_window,
    });

    let direction_changed = state
        .last_direction
        .is_none_or(|d| d.distance(chosen) > 1e-3);
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
            let (masked, los) = mask_and_los_bitmask(&compass_directions_16(), &inputs);
            builder.masked(masked);
            builder.scorer_term(
                "cover_pull",
                cover_pull_term(chosen, &inputs, eff_weights.cover_pull),
            );
            if los != 0 {
                builder.los_masked(los);
            }
            builder.finish();
        }
    }
    false
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::map_geometry::ObstacleVolume;

    fn priest_like() -> MovementWeights {
        MovementWeights {
            cover_pull: 1.5,
            threat_repulsion: 3.0,
            ..MovementWeights::default()
        }
    }

    /// Scenario 1 (the suppression seam): while a teammate needs saving, the
    /// effective weights zero `cover_pull` — the healer must not be pulled into
    /// cover — and every other term is untouched.
    #[test]
    fn cover_suppressed_when_teammate_needs_saving() {
        let w = priest_like();
        let eff = apply_cover_suppression(&w, true);
        assert_eq!(
            eff.cover_pull, 0.0,
            "cover_pull must be zeroed under urgency"
        );
        assert_eq!(
            eff.threat_repulsion, w.threat_repulsion,
            "threat_repulsion untouched"
        );
        assert_eq!(
            eff.corner_penalty, w.corner_penalty,
            "corner_penalty untouched"
        );
        assert_eq!(
            eff.commitment_bonus, w.commitment_bonus,
            "commitment_bonus untouched"
        );
    }

    /// No teammate in danger → weights pass through unchanged (denial stays on).
    #[test]
    fn cover_active_when_team_healthy() {
        let w = priest_like();
        let eff = apply_cover_suppression(&w, false);
        assert_eq!(
            eff.cover_pull, w.cover_pull,
            "cover_pull stays on when no teammate is dying"
        );
    }

    /// A class with denial disabled (`cover_pull == 0`) is a no-op copy even
    /// while a teammate is dying — no accidental sign flips off the deny path.
    #[test]
    fn suppression_noop_when_cover_disabled() {
        let w = MovementWeights {
            cover_pull: 0.0,
            ..MovementWeights::default()
        };
        assert_eq!(apply_cover_suppression(&w, true).cover_pull, 0.0);
    }

    /// Press gate: the margin is a `>=` threshold. Exactly-at-margin
    /// presses (denial off); a hair below does not.
    #[test]
    fn pressing_when_ahead_is_inclusive_at_margin() {
        let margin = 0.2;
        assert!(
            pressing_when_ahead(margin, margin),
            ">= is inclusive at the margin"
        );
        assert!(pressing_when_ahead(0.5, margin), "clearly ahead presses");
        assert!(
            !pressing_when_ahead(margin - 1e-4, margin),
            "just under the margin does not press"
        );
        assert!(!pressing_when_ahead(0.0, margin), "level does not press");
        assert!(!pressing_when_ahead(-0.5, margin), "behind never presses");
    }

    /// Press at the suppression seam: an ahead-by-margin team zeroes `cover_pull`
    /// (press = denial off), exactly as the urgency path does; a level/behind
    /// team leaves it on. Drives `apply_cover_suppression` through the same
    /// boolean `deny_weights` computes from the press predicate.
    #[test]
    fn press_zeroes_cover_pull_only_when_ahead() {
        let w = priest_like();
        let margin = 0.2;
        // Ahead by the margin → suppressed.
        let ahead = apply_cover_suppression(&w, pressing_when_ahead(0.4, margin));
        assert_eq!(ahead.cover_pull, 0.0, "pressing zeroes cover_pull");
        // Level → denial stays on.
        let level = apply_cover_suppression(&w, pressing_when_ahead(0.0, margin));
        assert_eq!(level.cover_pull, w.cover_pull, "level team keeps denying");
        // Behind → denial stays on.
        let behind = apply_cover_suppression(&w, pressing_when_ahead(-0.5, margin));
        assert_eq!(
            behind.cover_pull, w.cover_pull,
            "trailing team keeps denying"
        );
    }

    /// The `cover_pull` trace term reports 0 on an obstacle-free map (no
    /// occlusion possible) and `weight × occluded-count` when a pillar hides the
    /// chosen step from the threat — and 0 once the effective weight is
    /// suppressed, so the trace shows the suppression directly.
    #[test]
    fn cover_pull_term_counts_occluded_threats() {
        let threat = Vec3::new(0.0, 1.0, 10.0);
        let base = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, -3.0),
            lookahead: 2.0,
            threats: vec![threat],
            ..Default::default()
        };
        let chosen = Vec2::new(0.0, 1.0); // +Z: steps to (0, -1), on the axis
                                          // Obstacle-free: never occluded → 0 regardless of weight.
        assert_eq!(cover_pull_term(chosen, &base, 1.5), 0.0);

        // A thin pillar between the step and the threat occludes it → weight × 1.
        let occluded = ScorerInputs {
            obstacles: vec![ObstacleVolume::Cylinder {
                center_xz: Vec2::new(0.0, 3.0),
                radius: 0.5,
                base_y: 0.0,
                height: 10.0,
            }],
            ..base.clone()
        };
        assert_eq!(cover_pull_term(chosen, &occluded, 1.5), 1.5);
        // Suppressed (effective weight 0) → 0 contribution even when occluded.
        assert_eq!(cover_pull_term(chosen, &occluded, 0.0), 0.0);
    }

    // ------------------------------------------------------------------------
    // Fix 1: medic-chase target selection (`pick_medic_target`)
    // ------------------------------------------------------------------------

    fn e(raw: u32) -> Entity {
        Entity::from_raw(raw)
    }

    /// A candidate must be BOTH below the threshold AND occluded to qualify.
    #[test]
    fn medic_target_requires_low_hp_and_occlusion() {
        let threshold = 0.5;
        // Below threshold but SIGHTED → not a chase target (formation/anchor
        // machinery handles a sighted low ally). No obstacle-free map ever
        // produces an occluded candidate, so this is also the BasicArena no-op.
        assert_eq!(pick_medic_target(&[(e(1), 0.2, false)], threshold), None);
        // Occluded but healthy → not in danger.
        assert_eq!(pick_medic_target(&[(e(1), 0.8, true)], threshold), None);
        // Occluded AND low → chase.
        assert_eq!(
            pick_medic_target(&[(e(1), 0.2, true)], threshold),
            Some(e(1))
        );
        // Exactly at the threshold does NOT qualify (strict <).
        assert_eq!(pick_medic_target(&[(e(1), 0.5, true)], threshold), None);
    }

    /// Among qualifying (low + occluded) allies the MOST-injured is chosen.
    #[test]
    fn medic_target_picks_most_injured_qualifier() {
        let threshold = 0.5;
        // e(2) is more injured than e(1); both occluded and below threshold.
        let cands = [(e(1), 0.4, true), (e(2), 0.1, true)];
        assert_eq!(pick_medic_target(&cands, threshold), Some(e(2)));
        // A more-injured but SIGHTED ally must not steal the pick from a
        // less-injured OCCLUDED one — occlusion is a hard gate.
        let cands = [(e(1), 0.05, false), (e(2), 0.3, true)];
        assert_eq!(pick_medic_target(&cands, threshold), Some(e(2)));
    }

    /// Ties (equal HP) resolve to the earlier candidate (caller passes BTree
    /// entity order), keeping selection deterministic.
    #[test]
    fn medic_target_tie_breaks_on_entity_order() {
        let threshold = 0.5;
        let cands = [(e(3), 0.2, true), (e(7), 0.2, true)];
        assert_eq!(pick_medic_target(&cands, threshold), Some(e(3)));
        // Order-independence of the tie-break: reversing input still yields the
        // lowest-entity candidate because the caller sorts by entity, but verify
        // the "keep earlier on tie" rule directly with the reversed slice.
        let cands_rev = [(e(7), 0.2, true), (e(3), 0.2, true)];
        assert_eq!(pick_medic_target(&cands_rev, threshold), Some(e(7)));
    }

    /// No candidates / no qualifiers → None (the common every-frame case).
    #[test]
    fn medic_target_none_when_nothing_qualifies() {
        assert_eq!(pick_medic_target(&[], 0.5), None);
        assert_eq!(
            pick_medic_target(&[(e(1), 0.9, false), (e(2), 0.7, true)], 0.5),
            None
        );
    }

    /// The dispel walk's own gate (`ally_walk_allowed`), one tick per case: a
    /// Paladin 15yd beyond Cleanse's range of a Freezing-Trapped Warrior walks
    /// in FREE and PRESSURED under `Legacy`, and in FREE under `TeamPlan` — and
    /// never in ESCAPE or DIP, never while it is hard-CC'd itself, and never in
    /// `TeamPlan` PRESSURED, where the team solve is the one positioning
    /// authority.
    #[test]
    fn the_dispel_walk_runs_only_where_an_ally_walk_is_allowed() {
        use crate::states::match_config::CharacterClass;
        use crate::states::play_match::ai_profile::AiProfile;
        use crate::states::play_match::traps::freezing_trap_aura;
        use std::collections::BTreeMap;

        let defs = AbilityDefinitions::default();
        let cleanse = AbilityType::PaladinCleanse;
        let range = defs.get(&cleanse).unwrap().range;
        let (paladin, warrior) = (Entity::from_raw(1), Entity::from_raw(2));
        let unit = |entity, class, position| CombatantInfo {
            entity,
            team: 1,
            slot: 0,
            class,
            current_health: 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
        };
        let roster: BTreeMap<Entity, CombatantInfo> = [
            (paladin, unit(paladin, CharacterClass::Paladin, Vec3::ZERO)),
            (
                warrior,
                unit(
                    warrior,
                    CharacterClass::Warrior,
                    Vec3::new(range + 15.0, 0.0, 0.0),
                ),
            ),
        ]
        .into_iter()
        .collect();
        let trap = freezing_trap_aura(Entity::from_raw(9));
        let dr = BTreeMap::new();
        let cds = BTreeMap::new();

        let walks = |next: Posture, paladin_ccd: bool, profile: AiProfile| {
            let mut auras = BTreeMap::new();
            auras.insert(warrior, vec![trap.clone()]);
            if paladin_ccd {
                auras.insert(paladin, vec![trap.clone()]);
            }
            let ctx = CombatContext::new(
                paladin,
                1,
                &roster,
                &auras,
                &dr,
                &cds,
                &[],
                Default::default(),
                profile,
            );
            match dispel_chase_override(
                &defs,
                paladin,
                Vec3::ZERO,
                100.0,
                next,
                &ctx,
                cleanse,
                &SharedMovementConfig::default(),
                &HealerPosture::new(0.0),
            ) {
                Some(DispelStep::Walk(ally)) => Some(ally.entity),
                Some(DispelStep::Hold { .. }) => panic!("held a walk that never ran"),
                None => None,
            }
        };

        use AiProfile::{Legacy, TeamPlan};
        use Posture::{Dip, Escape, Free, Pressured};
        assert_eq!(walks(Free, false, Legacy), Some(warrior));
        assert_eq!(walks(Pressured, false, Legacy), Some(warrior));
        assert_eq!(walks(Free, false, TeamPlan), Some(warrior));
        assert_eq!(walks(Escape, false, Legacy), None, "never in ESCAPE");
        assert_eq!(walks(Dip, false, Legacy), None, "never in DIP");
        assert_eq!(walks(Free, true, Legacy), None, "never while hard-CC'd");
        assert_eq!(
            walks(Pressured, false, TeamPlan),
            None,
            "retired under TeamPlan PRESSURED"
        );
    }

    /// The three rules over the walk itself (AS-197): no walk while the
    /// Paladin cannot Cleanse; a walk that reaches HOLDS its reach point while
    /// Cleanse is still owed (and only a walk that ran — a teammate CC'd in
    /// reach is the rotation's alone); and nothing while another teammate the
    /// Paladin could heal instead is dying — but the CC'd teammate itself
    /// dying does not count.
    #[test]
    fn the_dispel_walk_gates_on_cleanse_holds_on_reach_and_yields_to_the_dying() {
        use crate::states::match_config::CharacterClass;
        use crate::states::play_match::abilities::SpellSchool;
        use crate::states::play_match::components::{Aura, AuraType};
        use crate::states::play_match::traps::freezing_trap_aura;
        use std::collections::BTreeMap;

        let defs = AbilityDefinitions::default();
        let cleanse = AbilityType::PaladinCleanse;
        let range = defs.get(&cleanse).unwrap().range;
        let (paladin, warrior, rogue) = (
            Entity::from_raw(1),
            Entity::from_raw(2),
            Entity::from_raw(3),
        );
        let unit = |entity, class, position, hp| CombatantInfo {
            entity,
            team: 1,
            slot: 0,
            class,
            current_health: hp,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
        };
        let trap = freezing_trap_aura(Entity::from_raw(9));
        let mut silence = trap.clone();
        silence.effect_type = AuraType::Silence;
        let mut holy_lock = trap.clone();
        holy_lock.effect_type = AuraType::SpellSchoolLockout;
        holy_lock.magnitude = SpellSchool::Holy.to_lockout_magnitude();
        let dr = BTreeMap::new();
        let cds = BTreeMap::new();

        #[derive(Debug, PartialEq)]
        enum Did {
            Walk,
            Hold(Vec3),
            Nothing,
        }
        let step = |warrior_x: f32,
                    warrior_hp: f32,
                    rogue_hp: f32,
                    paladin_auras: Vec<Aura>,
                    phase: DispelWalkPhase| {
            let roster: BTreeMap<Entity, CombatantInfo> = [
                (
                    paladin,
                    unit(paladin, CharacterClass::Paladin, Vec3::ZERO, 100.0),
                ),
                (
                    warrior,
                    unit(
                        warrior,
                        CharacterClass::Warrior,
                        Vec3::new(warrior_x, 0.0, 0.0),
                        warrior_hp,
                    ),
                ),
                (
                    rogue,
                    unit(
                        rogue,
                        CharacterClass::Rogue,
                        Vec3::new(-10.0, 0.0, 0.0),
                        rogue_hp,
                    ),
                ),
            ]
            .into_iter()
            .collect();
            let mut auras = BTreeMap::new();
            auras.insert(warrior, vec![trap.clone()]);
            auras.insert(paladin, paladin_auras);
            let ctx = CombatContext::new(
                paladin,
                1,
                &roster,
                &auras,
                &dr,
                &cds,
                &[],
                Default::default(),
                Default::default(),
            );
            let mut state = HealerPosture::new(0.0);
            state.dispel_walk = phase;
            match dispel_chase_override(
                &defs,
                paladin,
                Vec3::ZERO,
                100.0,
                Posture::Free,
                &ctx,
                cleanse,
                &SharedMovementConfig::default(),
                &state,
            ) {
                Some(DispelStep::Walk(ally)) => {
                    assert_eq!(ally.entity, warrior);
                    Did::Walk
                }
                Some(DispelStep::Hold { ally, at }) => {
                    assert_eq!(ally, warrior);
                    Did::Hold(at)
                }
                None => Did::Nothing,
            }
        };
        use DispelWalkPhase::{Holding, Off, Walking};
        let (far, near) = (range + 15.0, range - 2.0);
        let held = Vec3::new(1.0, 0.0, 2.0);

        // The control, then the Cleanse gate.
        assert_eq!(step(far, 100.0, 100.0, vec![], Off), Did::Walk);
        assert_eq!(
            step(far, 100.0, 100.0, vec![silence.clone()], Off),
            Did::Nothing
        );
        assert_eq!(
            step(far, 100.0, 100.0, vec![holy_lock.clone()], Off),
            Did::Nothing
        );

        // Reach: hold where the walk reached, then keep that point.
        assert_eq!(step(near, 100.0, 100.0, vec![], Off), Did::Nothing);
        assert_eq!(
            step(near, 100.0, 100.0, vec![], Walking),
            Did::Hold(Vec3::ZERO)
        );
        // The band: a live walk carries on past the bare edge before holding,
        // and a hold does not give way there.
        let edge = range - DISPEL_REACH_MARGIN / 2.0;
        assert_eq!(step(edge, 100.0, 100.0, vec![], Walking), Did::Walk);
        assert_eq!(
            step(edge, 100.0, 100.0, vec![], Holding(held)),
            Did::Hold(held)
        );
        assert_eq!(
            step(near, 100.0, 100.0, vec![], Holding(held)),
            Did::Hold(held)
        );
        // ...and stop holding once Cleanse cannot be cast.
        assert_eq!(
            step(near, 100.0, 100.0, vec![silence], Holding(held)),
            Did::Nothing
        );
        // A teammate leaving reach is walked to again.
        assert_eq!(step(far, 100.0, 100.0, vec![], Holding(held)), Did::Walk);

        // Dying first: the sighted Rogue below half holds both walk and hold.
        assert_eq!(step(far, 100.0, 20.0, vec![], Off), Did::Nothing);
        assert_eq!(step(near, 100.0, 20.0, vec![], Holding(held)), Did::Nothing);
        // The trapped Warrior's own HP does not count.
        assert_eq!(step(far, 20.0, 100.0, vec![], Off), Did::Walk);
    }
}
