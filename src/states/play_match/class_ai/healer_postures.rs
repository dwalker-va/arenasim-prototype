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
    candidate_mask, compass_directions_16, mask_and_los_bitmask, score_directions, AnchorConstraint,
    ScorerInputs,
};
use crate::states::play_match::components::{HealerPosture, MovementDirective, MovementGoal, Posture};
use crate::states::play_match::decision_trace::{
    ActorView, DecisionTrace, MovementEventBuilder, MovementGoalKind, MovementTrigger,
    Posture as TracePosture, TargetView,
};
use crate::states::play_match::map_geometry::{
    has_line_of_sight, position_blocked, EYE_HEIGHT, MOVER_RADIUS,
};
use crate::states::play_match::movement_config::{MovementWeights, SharedMovementConfig};

use super::{pressing_when_ahead, CombatContext, CombatantInfo};

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
        MovementWeights { cover_pull: 0.0, ..*weights }
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

/// Extra clearance beyond an obstacle's own footprint when picking a spot in its
/// shadow, so the healer stands *behind* cover rather than flush against it (and
/// so the point is not rejected as blocked by its own collision skin).
const COVER_STANDOFF: f32 = 1.5;

/// Whether any compass candidate step breaks sight to a threat — i.e. whether
/// `cover_pull` has a local gradient to climb this tick.
///
/// When true, the scorer handles it and cover-seek must stay out of the way;
/// when false the term is flat and only navigation can find cover. Always true
/// on obstacle-free maps in the vacuous sense that it is always FALSE there — no
/// obstacles means nothing is ever occluded — so the caller's `cover_pull > 0`
/// gate is what keeps this inert, not this function.
///
/// MASKED candidates do not count. A step INTO a pillar is occluded from
/// everything, but the scorer removes it (`MASK_LOS`), so counting it would report
/// a gradient that `score_directions` cannot actually climb — and it would do so
/// exactly where cover-seek is most needed: a healer pinned on the threat's side
/// of a pillar, one step from cover it cannot walk through.
fn has_local_cover(inputs: &ScorerInputs) -> bool {
    if inputs.obstacles.is_empty() {
        return false;
    }
    compass_directions_16().into_iter().any(|dir| {
        if candidate_mask(dir, inputs) != 0 {
            return false;
        }
        let next = inputs.my_pos + Vec3::new(dir.x, 0.0, dir.y) * inputs.lookahead;
        let eye = Vec3::new(next.x, EYE_HEIGHT, next.z);
        inputs.threats.iter().any(|t| {
            !has_line_of_sight(
                &inputs.obstacles,
                eye,
                Vec3::new(t.x, EYE_HEIGHT, t.z),
            )
        })
    })
}

/// The nearest standing position that breaks sight to `threat`, or `None` if no
/// obstacle offers one.
///
/// For each obstacle, the candidate is the point in its shadow: straight out from
/// the obstacle centre along the direction away from the threat, clear of the
/// footprint by [`COVER_STANDOFF`]. Candidates are then verified against the
/// exact predicates — inside the arena, not inside an obstacle, and genuinely
/// occluded — because `footprint_disc` over-covers non-circular shapes and the
/// shadow point of a thin obstacle can miss.
///
/// Obstacles are walked in slice order and ties broken by distance-then-order, so
/// the choice is deterministic.
pub(super) fn cover_seek_target(
    my_pos: Vec3,
    threat: Vec3,
    ctx: &CombatContext,
) -> Option<Vec3> {
    let threat_eye = Vec3::new(threat.x, EYE_HEIGHT, threat.z);
    let mut best: Option<(f32, Vec3)> = None;

    for volume in ctx.obstacles {
        let (center, radius) = volume.footprint_disc();
        // Direction from the threat past the obstacle — its shadow axis.
        let away = (center - Vec2::new(threat.x, threat.z)).normalize_or_zero();
        if away == Vec2::ZERO {
            continue; // threat standing on the obstacle centre; no usable shadow
        }
        let spot_xz = center + away * (radius + MOVER_RADIUS + COVER_STANDOFF);
        let spot = Vec3::new(spot_xz.x, my_pos.y, spot_xz.y);

        // Exact checks — the disc is only a hint.
        if !ctx.bounds.contains(spot) || position_blocked(ctx.obstacles, spot) {
            continue;
        }
        let spot_eye = Vec3::new(spot.x, EYE_HEIGHT, spot.z);
        if has_line_of_sight(ctx.obstacles, spot_eye, threat_eye) {
            continue; // this shadow does not actually hide the healer
        }

        let d = my_pos.distance(spot);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, spot));
        }
    }

    best.map(|(_, spot)| spot)
}

/// Whether cover-seek should override the PRESSURED tick, and where to walk.
///
/// PRESSURED-only by construction: the sole caller is the PRESSURED tick. Denial
/// is a pressured behaviour — a FREE healer has formation duties and an ESCAPE/DIP
/// healer has a committed window — so nothing else may call this.
///
/// Gates, in order of what they protect:
/// - `cover_pull` must be active *after* suppression, so the urgency and press
///   rules (a teammate dying, or the team pressing an advantage) still switch
///   denial off — cover-seek must not smuggle hiding back in when the healer
///   should be healing. This is why it takes the EFFECTIVE weights.
/// - Not hard-CC'd (`is_ccd` includes Root): a stationary healer's directive
///   would be stale on release.
/// - No local cover, or the scorer already has a gradient and should own the tick.
pub(super) fn cover_seek_override(
    entity: Entity,
    ctx: &CombatContext,
    inputs: &ScorerInputs,
    eff_weights: &MovementWeights,
) -> Option<Vec3> {
    // Gated on the AI profile: this is the first team-level-flavoured behaviour
    // and is opt-in. Landing it unconditionally drifted 6 fixed-seed probes on
    // every obstacle map — see `ai_profile.rs`.
    if !ctx.ai_profile.is_team_plan()
        || eff_weights.cover_pull <= 0.0
        || ctx.is_ccd(entity)
        || has_local_cover(inputs)
    {
        return None;
    }
    // Hide from the nearest threat: it is the one applying pressure, and cover
    // that breaks its sight most often breaks its allies' too.
    let nearest = inputs.threats.iter().copied().fold(None, |acc, t| {
        let d = inputs.my_pos.distance(t);
        match acc {
            Some((bd, _)) if bd <= d => acc,
            _ => Some((d, t)),
        }
    })?;
    cover_seek_target(inputs.my_pos, nearest.1, ctx)
}

/// Issue/refresh the cover-seek directive toward `spot` and emit the movement
/// event with a `Point` goal.
///
/// `transitioned`/`prev` are threaded through because this tick REPLACES the
/// normal PRESSURED path, including its trace emission. A posture transition must
/// still be recorded as a transition (`PressuredEnter`, or `EscapeWindowClosed`
/// out of ESCAPE) — the `PressuredEnter`/`PressuredExit` pairing is what
/// `pressured_windows` and the jq recipes key on, and silently swallowing it makes
/// every PRESSURED window vanish from the trace. Only non-transition re-commits
/// are tagged `SeekLos`, matching `medic_chase_tick` and the DPS occlusion chase.
#[allow(clippy::too_many_arguments)]
pub(super) fn cover_seek_tick(
    commands: &mut Commands,
    entity: Entity,
    spot: Vec3,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    shared: &SharedMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    transitioned: bool,
    prev: Posture,
) {
    // Re-commit when the window lapses or the destination moved materially (the
    // threat circled, putting the shadow somewhere else).
    let spot_xz = Vec2::new(spot.x, spot.z);
    let moved = state
        .last_point
        .map_or(true, |p| p.distance(spot_xz) > COVER_STANDOFF);
    let recommit =
        moved || directive.map_or(true, |d| now >= d.committed_until || now >= d.expires);
    // A transition must always be traced, even when the walk itself is still
    // committed and needs no new directive.
    if !recommit && !transitioned {
        return;
    }

    if recommit {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(spot),
            expires: now + shared.directive_ttl,
            committed_until: now + shared.commit_window,
        });
        state.last_point = Some(spot_xz);
        // A point walk is not a scored direction — clear it so the normal
        // PRESSURED tick re-scores cleanly once local cover exists.
        state.last_direction = None;
    }

    if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
        if transitioned {
            // Mirrors the normal PRESSURED path's trigger choice exactly.
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
        } else {
            builder.direction_change(
                TracePosture::Pressured,
                MovementTrigger::SeekLos,
                MovementGoalKind::Point,
            );
        }
        builder.finish();
    }
}

/// Whether the medic chase should override the normal movement tick this frame:
/// current posture FREE or PRESSURED (never DIP — its own teammate-HP abort
/// composes, handing control back so the medic picks up the next decision — nor
/// the committed ESCAPE window), the healer not itself hard-CC'd (a CC'd healer
/// can't move, and the directive would be stale on release; `is_ccd` includes
/// Root, which blocks movement too), and a dying occluded teammate exists.
/// Returns that ally.
pub(super) fn medic_chase_override<'c>(
    entity: Entity,
    my_pos: Vec3,
    next: Posture,
    ctx: &'c CombatContext,
    shared: &SharedMovementConfig,
) -> Option<&'c CombatantInfo> {
    if !matches!(next, Posture::Free | Posture::Pressured) || ctx.is_ccd(entity) {
        return None;
    }
    // RETIRED under `TeamPlan`: the solve subsumes this.
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
    if ctx.ai_profile.is_team_plan() {
        return None;
    }
    medic_chase_target(entity, my_pos, ctx, shared)
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
    let recommit = state.medic_target != Some(ally.entity)
        || directive.map_or(true, |d| now >= d.committed_until || now >= d.expires);
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
        builder.direction_change(
            state.posture.into(),
            MovementTrigger::SeekLos,
            MovementGoalKind::Point,
        );
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
        nearest_threat: None,
        committed_direction: None,
        obstacles: ctx.obstacles.to_vec(),
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
        builder.transition(
            prev.into(),
            TracePosture::Escape,
            MovementTrigger::EscapeWindowOpen,
            MovementGoalKind::Direction,
        );
        builder.chosen_direction([chosen.x, chosen.y]);
        let (masked, los) = mask_and_los_bitmask(&compass_directions_16(), &inputs);
        builder.masked(masked);
        builder.scorer_term("cover_pull", cover_pull_term(chosen, &inputs, eff_weights.cover_pull));
        if los != 0 {
            builder.los_masked(los);
        }
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
        );
        let spot = crate::states::play_match::team_solve::solve_position(
            crate::states::play_match::team_plan::RoleIntent::OccupyCover,
            entity,
            &world,
            // `OccupyCover` is defined against the healer's own ally and the
            // enemy casters, not against a focal unit.
            None,
        );
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
        return;
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
        los_target,
    };
    // Deny posture: prefer a step that breaks attacker LoS (cover_pull),
    // unless a teammate needs saving — then urgency suppression zeroes it so the
    // healer is never pulled into cover while an ally is dying (R11).
    let eff_weights = deny_weights(entity, my_pos, ctx, shared, weights);

    // Cover-seek: `cover_pull` is only a 2yd-lookahead gradient, so on a large
    // map where the nearest pillar is tens of yards away it is flat everywhere and
    // the healer never approaches cover at all. When denial is active but no local
    // step is occluded, walk directly at the nearest hiding spot instead of
    // scoring. Placed AFTER `deny_weights` so urgency/press suppression still wins.
    if let Some(spot) = cover_seek_override(entity, ctx, &inputs, &eff_weights) {
        cover_seek_tick(
            commands, entity, spot, state, directive, shared, now, decision_trace, ctx,
            transitioned, prev,
        );
        return;
    }

    let chosen = score_directions(&compass_directions_16(), &inputs, &eff_weights);
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
            let (masked, los) = mask_and_los_bitmask(&compass_directions_16(), &inputs);
            builder.masked(masked);
            builder.scorer_term("cover_pull", cover_pull_term(chosen, &inputs, eff_weights.cover_pull));
            if los != 0 {
                builder.los_masked(los);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::map_geometry::ObstacleVolume;

    fn priest_like() -> MovementWeights {
        MovementWeights { cover_pull: 1.5, threat_repulsion: 3.0, ..MovementWeights::default() }
    }

    /// Scenario 1 (the suppression seam): while a teammate needs saving, the
    /// effective weights zero `cover_pull` — the healer must not be pulled into
    /// cover — and every other term is untouched.
    #[test]
    fn cover_suppressed_when_teammate_needs_saving() {
        let w = priest_like();
        let eff = apply_cover_suppression(&w, true);
        assert_eq!(eff.cover_pull, 0.0, "cover_pull must be zeroed under urgency");
        assert_eq!(eff.threat_repulsion, w.threat_repulsion, "threat_repulsion untouched");
        assert_eq!(eff.corner_penalty, w.corner_penalty, "corner_penalty untouched");
        assert_eq!(eff.commitment_bonus, w.commitment_bonus, "commitment_bonus untouched");
    }

    /// No teammate in danger → weights pass through unchanged (denial stays on).
    #[test]
    fn cover_active_when_team_healthy() {
        let w = priest_like();
        let eff = apply_cover_suppression(&w, false);
        assert_eq!(eff.cover_pull, w.cover_pull, "cover_pull stays on when no teammate is dying");
    }

    /// A class with denial disabled (`cover_pull == 0`) is a no-op copy even
    /// while a teammate is dying — no accidental sign flips off the deny path.
    #[test]
    fn suppression_noop_when_cover_disabled() {
        let w = MovementWeights { cover_pull: 0.0, ..MovementWeights::default() };
        assert_eq!(apply_cover_suppression(&w, true).cover_pull, 0.0);
    }

    /// Press gate: the margin is a `>=` threshold. Exactly-at-margin
    /// presses (denial off); a hair below does not.
    #[test]
    fn pressing_when_ahead_is_inclusive_at_margin() {
        let margin = 0.2;
        assert!(pressing_when_ahead(margin, margin), ">= is inclusive at the margin");
        assert!(pressing_when_ahead(0.5, margin), "clearly ahead presses");
        assert!(!pressing_when_ahead(margin - 1e-4, margin), "just under the margin does not press");
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
        assert_eq!(behind.cover_pull, w.cover_pull, "trailing team keeps denying");
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
        assert_eq!(pick_medic_target(&[(e(1), 0.2, true)], threshold), Some(e(1)));
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
}
