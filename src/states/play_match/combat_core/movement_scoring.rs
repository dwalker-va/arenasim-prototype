//! Pure position scorer for movement directives (healer postures + the
//! Mage/Hunter ENGAGE/KITE machine).
//!
//! A 16-direction argmax over named, RON-tunable weighted terms
//! (`assets/config/movement.ron` → `MovementWeights`). It replaced the legacy
//! `find_best_kiting_direction` (now deleted) as the sole kiting/positioning
//! path. Pure free functions
//! over caller-built inputs: no Bevy world, no queries — unit-testable in
//! isolation, and deterministic as long as callers build `ScorerInputs` from
//! BTree-ordered snapshots (floating-point summation order follows input
//! order).
//!
//! Terms (see the plan's scorer-terms table):
//! - **Threat repulsion** — away from each visible threat, proximity-weighted
//!   (PRESSURED).
//! - **Ally-anchor constraint** — boolean mask (`MASK_ANCHOR`): candidate
//!   positions outside heal range of the anchor ally are removed before
//!   scoring (PRESSURED).
//! - **Formation pull** — toward the formation point behind the engaged-ally
//!   centroid (FREE, Priest).
//! - **Boundary mask** — boolean mask (`MASK_BOUNDARY`): out-of-bounds
//!   candidate positions are removed (reuses `is_in_arena_bounds`).
//! - **Corner penalty** — graded penalty approaching the octagon's corners
//!   (the |x|+|z| diagonal walls), so escapes bend along walls instead of
//!   pinning into corners.
//! - **Wand-range pull** — low-weight pull toward wand range of the kill
//!   target (weight 0 disables; Paladin).
//! - **Range-band** — ring-attraction toward the kill target's `[min, max]`
//!   band (Mage kiting; weight 0 disables for healers).
//! - **Flee** — constant pull away from the nearest threat, NOT
//!   proximity-weighted (distance-maximization), so a chased ranged DPS
//!   (Hunter) outruns an un-impaired chaser at all ranges where
//!   `threat_repulsion` fades. Weight 0 disables for healers and the Mage.
//! - **Commitment bonus** — toward the previously committed direction,
//!   applied only AT re-evaluation while the commitment window is open. The
//!   hard `committed_until` window on `MovementDirective` decides WHEN
//!   re-evaluation happens; this term applies only at that moment — the two
//!   never stack.
//! - **LoS-seek** — flat bonus for a step that has line of sight to the kill
//!   target (`los_target`); weight 0 disables. Obstacle maps only.
//! - **Cover-pull** — flat bonus per threat a step is occluded from (sight to
//!   the threat broken by an obstacle); weight 0 disables. Obstacle maps only.
//!
//! **Obstacles** are handled two ways. A candidate STEP that walks into an
//! obstacle footprint is masked (`MASK_LOS`, agrees with the collision
//! resolver's touching-allowed policy), and the ally-anchor constraint requires
//! an unobstructed line to the anchor, not just distance. All obstacle-aware
//! logic is a no-op on obstacle-free maps (empty `obstacles` slice), so
//! BasicArena scoring is byte-identical.
//!
//! **Hard constraints are masks, not penalties.** Boundary and ally-anchor
//! are evaluated as a boolean *mask pass* that removes violating candidates
//! before the additive *interest pass* scores the survivors. This replaces
//! the old `-1000.0` dominance-penalty scheme (and the `validate()` invariant
//! that policed it): a masked candidate can never beat an unmasked one
//! because it is never scored. Equivalence with the old penalty argmax holds
//! whenever at least one candidate survives the mask pass — the penalties
//! dominated every soft-term sum, so the old argmax winner was always an
//! unmasked candidate too. When every candidate is masked, the fallback
//! ladder below applies.

use bevy::prelude::*;

use super::is_in_arena_bounds;
use super::super::map_geometry::{has_line_of_sight, position_blocked, ObstacleVolume, EYE_HEIGHT};
use super::super::movement_config::MovementWeights;
use super::super::arena_bounds::{ArenaBounds, EDGE_PENALTY_ONSET_FRACTION};
use super::super::ARENA_CORNER_SUM;

/// Mask bit: candidate's lookahead position is out of arena bounds.
pub const MASK_BOUNDARY: u16 = 1 << 0;
/// Mask bit: candidate's lookahead position leaves heal range of the anchor —
/// now *also* set when an obstacle occludes the anchor from that position (a
/// healer must both stay in range AND keep sight of the ally it heals).
pub const MASK_ANCHOR: u16 = 1 << 1;
/// Mask bit: candidate's lookahead step walks into an obstacle volume (the
/// movement-blocking footprint test, [`position_blocked`]). Always clear on
/// obstacle-free maps, so BasicArena scoring is unaffected.
pub const MASK_LOS: u16 = 1 << 2;

/// Where the graded corner penalty starts biting on the HISTORICAL octagon, as
/// |x|+|z|. Below this the term is zero; it ramps linearly to full weight at the
/// corner wall (`ARENA_CORNER_SUM`). ~70% of the wall keeps the center half of the
/// arena penalty-free.
///
/// The scorer itself reads `ArenaBounds::edge_closeness`, which is per-map; this is
/// the same onset expressed as an absolute |x|+|z| threshold for the octagon-only
/// movement probes to assert against. It shares
/// [`EDGE_PENALTY_ONSET_FRACTION`] with `edge_closeness` rather than restating
/// 0.7, so retuning the onset cannot move the scorer without moving the probes.
pub const CORNER_PENALTY_ONSET: f32 = ARENA_CORNER_SUM * EDGE_PENALTY_ONSET_FRACTION;

/// The 16 compass candidate directions (unit XZ vectors, TAU/16 apart). Index
/// order is fixed, so argmax tie-breaks are deterministic.
pub fn compass_directions_16() -> [Vec2; 16] {
    std::array::from_fn(|i| {
        let angle = (i as f32) * std::f32::consts::TAU / 16.0;
        Vec2::new(angle.cos(), angle.sin())
    })
}

/// Ring-attraction band toward a target (the Mage's `range_band` term):
/// reward stepping *toward* the target while farther than `max`, and *away*
/// while nearer than `min`; no pull while within `[min, max]`. Arc-kiting
/// emerges when this composes with `threat_repulsion` — the kiter orbits the
/// kill target at band distance instead of fleeing straight into a wall.
#[derive(Clone, Copy, Debug)]
pub struct RangeBand {
    /// Target world position (the kill target).
    pub target: Vec3,
    /// Inner ring radius: nearer than this, push out (keeps the kiter out of
    /// melee of a kill target that is also a threat).
    pub min: f32,
    /// Outer ring radius: farther than this, pull in (keeps the kill target in
    /// cast range).
    pub max: f32,
}

/// Ally-anchor constraint: candidate positions farther than `heal_range`
/// from `pos` are masked out (`MASK_ANCHOR`) before the interest pass scores.
#[derive(Clone, Copy, Debug)]
pub struct AnchorConstraint {
    /// Anchor ally world position.
    pub pos: Vec3,
    /// Maximum allowed XZ distance from the anchor (heal range, 40).
    pub heal_range: f32,
}

/// Caller-built context for one scoring pass. Build from BTree-ordered
/// snapshots so the term summation order — and therefore the chosen
/// direction — is deterministic at a fixed seed.
#[derive(Clone, Debug, Default)]
pub struct ScorerInputs {
    /// The active map's walkable region, for the boundary mask and the
    /// edge/corner penalty. Per-map data, so the same tuned `corner_penalty`
    /// weight works on the octagon maps and on Nagrand's circular bowl.
    pub bounds: ArenaBounds,
    /// Scoring entity's world position.
    pub my_pos: Vec3,
    /// Distance ahead at which candidate positions are evaluated (units).
    /// Larger values see walls/corners/anchor-range earlier; the directive
    /// executor still moves one frame-step at a time.
    pub lookahead: f32,
    /// Visible threat positions in deterministic (BTree-derived) order.
    /// Stealth filtering is the caller's job (`enemies_targeting` is already
    /// stealth-filtered).
    pub threats: Vec<Vec3>,
    /// PRESSURED heal-range constraint, if an anchor ally exists.
    pub anchor: Option<AnchorConstraint>,
    /// FREE formation point (Priest), if any.
    pub formation_point: Option<Vec3>,
    /// Kill-target position for the wand-range pull, if any.
    pub wand_target: Option<Vec3>,
    /// Desired wand range (shared config `wand_range`, 30). The pull is
    /// active only while farther than this from `wand_target`.
    pub wand_range: f32,
    /// Ring-attraction band toward the kill target (Mage kiting). `None`
    /// disables the term (no kill target, or non-Mage scorer).
    pub range_band: Option<RangeBand>,
    /// Nearest threat position, for the distance-maximization `flee` term
    /// (Hunter). `None` disables it (no threats, or non-fleeing scorer).
    pub nearest_threat: Option<Vec3>,
    /// Previously committed direction. `Some` ONLY while the commitment
    /// window is open — the caller passes `None` outside it, which disables
    /// the term entirely.
    pub committed_direction: Option<Vec2>,
    /// The active map's obstacle volumes (owned, matching the struct's
    /// owned-field idiom; cheap — a handful of pillars). Empty on obstacle-free
    /// maps (BasicArena), which makes every obstacle-aware check a no-op. Feeds
    /// the `MASK_LOS` step check, the anchor LoS constraint, and the
    /// `los_seek` / `cover_pull` interest terms.
    pub obstacles: Vec<ObstacleVolume>,
    /// Position of this unit's own HEALER, for the `healer_leash` term. `None`
    /// disables it — no healer alive, a non-leashed scorer, or the profile gate.
    pub healer_point: Option<Vec3>,
    /// Distance beyond which the leash starts pulling (the healer's cast range).
    /// Inside it the term contributes exactly nothing.
    pub healer_leash_range: f32,
    /// Kill-target position for the `los_seek` term (keep sight of the target
    /// you shoot). `None` disables the term (no kill target, or a scorer that
    /// does not seek LoS).
    pub los_target: Option<Vec3>,
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// Hard-constraint mask for one candidate: which of `MASK_BOUNDARY` /
/// `MASK_ANCHOR` its lookahead position violates. `0` means the candidate
/// survives the mask pass. Pure — depends only on `inputs` geometry.
pub fn candidate_mask(candidate: Vec2, inputs: &ScorerInputs) -> u16 {
    let next = inputs.my_pos + Vec3::new(candidate.x, 0.0, candidate.y) * inputs.lookahead;
    let mut mask = 0u16;
    if !is_in_arena_bounds(&inputs.bounds, next) {
        mask |= MASK_BOUNDARY;
    }
    // Obstacle mask: reject a step that walks into a wall (movement-blocking
    // footprint test — agrees with `resolve_movement`, touching = allowed).
    if position_blocked(&inputs.obstacles, next) {
        mask |= MASK_LOS;
    }
    if let Some(anchor) = inputs.anchor {
        // The anchor constraint is distance AND sight: a healer must stay in
        // heal range of the ally AND keep an unobstructed line to it. Probe LoS
        // at eye height on both ends to match the cast/heal gates.
        let out_of_range = xz(next).distance(xz(anchor.pos)) > anchor.heal_range;
        let cand_eye = Vec3::new(next.x, EYE_HEIGHT, next.z);
        let anchor_eye = Vec3::new(anchor.pos.x, EYE_HEIGHT, anchor.pos.z);
        if out_of_range || !has_line_of_sight(&inputs.obstacles, cand_eye, anchor_eye) {
            mask |= MASK_ANCHOR;
        }
    }
    mask
}

/// Score a single candidate direction (unit XZ) on the additive *interest*
/// terms only. Higher is better. Hard constraints (boundary, ally-anchor) are
/// handled by the mask pass in [`score_directions`], NOT here — this function
/// no longer subtracts the old dominance penalties.
pub fn score_direction(candidate: Vec2, inputs: &ScorerInputs, weights: &MovementWeights) -> f32 {
    let my_xz = xz(inputs.my_pos);
    let next = inputs.my_pos + Vec3::new(candidate.x, 0.0, candidate.y) * inputs.lookahead;

    let mut score = 0.0;

    // Threat repulsion: per visible threat, reward moving away, weighted by
    // proximity (a melee on top of us dominates a caster at 30yd).
    for threat in &inputs.threats {
        let offset = my_xz - xz(*threat);
        let distance = offset.length();
        let away = offset.normalize_or_zero(); // zero when exactly overlapping
        let proximity = 1.0 / (1.0 + distance);
        score += weights.threat_repulsion * candidate.dot(away) * proximity;
    }

    // Formation pull: toward the formation point, fading out on arrival so
    // the term doesn't thrash between opposing directions at the point.
    if let Some(point) = inputs.formation_point {
        let to_point = xz(point) - my_xz;
        let distance = to_point.length();
        if distance > f32::EPSILON {
            let arrival_fade = (distance / inputs.lookahead.max(f32::EPSILON)).min(1.0);
            score += weights.formation_pull * candidate.dot(to_point / distance) * arrival_fade;
        }
    }

    // Corner penalty: graded ramp toward the arena wall, normalized to 0..1
    // between the onset fraction and the wall itself. A soft interest term (not a
    // hard mask) — it shapes the least-bad choice. Shape-specific (the octagon
    // measures |x|+|z| toward its diagonal corners; the bowl measures normalized
    // ellipse radius), but identically normalized so the weight transfers.
    // HEALER LEASH: a kiter has no awareness of its own healer at all —
    // `flee` is unbounded distance-maximisation and `range_band` bounds it
    // relative to the TARGET, not the healer. So nothing stops a Hunter fleeing
    // a Rogue straight out of its own Priest's range, at which point it is both
    // unhealable and (past its shot range) unable to answer. Watching
    // `solve_hunter_s8` is what surfaced it.
    //
    // Shaped as a LEASH rather than a pull: exactly zero inside
    // `healer_leash_range`, then ramping with the overshoot. That is what keeps
    // it from fighting `flee` in the region where fleeing is simply correct —
    // the same shape as `corner_penalty`, which bounds `flee` away from corners
    // without opposing it in open ground.
    if let Some(healer) = inputs.healer_point {
        let to_healer = xz(healer) - xz(next);
        let overshoot = to_healer.length() - inputs.healer_leash_range;
        if overshoot > 0.0 && to_healer.length() > f32::EPSILON {
            let ramp = (overshoot / inputs.healer_leash_range.max(f32::EPSILON)).min(1.0);
            score += weights.healer_leash * candidate.dot(to_healer.normalize()) * ramp;
        }
    }

    let corner_closeness = inputs.bounds.edge_closeness(next.x, next.z);
    if corner_closeness > 0.0 {
        score -= weights.corner_penalty * corner_closeness;
    }

    // Wand-range pull: low-weight pull toward the kill target while outside
    // wand range; silent once inside (no push-out). Weight 0 disables.
    if weights.wand_pull > 0.0 {
        if let Some(target) = inputs.wand_target {
            let to_target = xz(target) - my_xz;
            let distance = to_target.length();
            if distance > inputs.wand_range {
                score += weights.wand_pull * candidate.dot(to_target / distance);
            }
        }
    }

    // Range-band ring attraction (Mage kiting): pull toward the kill target
    // outside `max`, push away inside `min`, silent in-band. Composed with
    // threat_repulsion this produces arc-kiting. Weight 0 disables.
    if weights.range_band > 0.0 {
        if let Some(band) = inputs.range_band {
            let to_target = xz(band.target) - my_xz;
            let distance = to_target.length();
            if distance > f32::EPSILON {
                let toward = to_target / distance;
                if distance > band.max {
                    score += weights.range_band * candidate.dot(toward);
                } else if distance < band.min {
                    score += weights.range_band * candidate.dot(-toward);
                }
            }
        }
    }

    // Flee (Hunter): constant pull away from the nearest threat, NOT weighted
    // by proximity — a chased ranged DPS must outrun an un-impaired chaser at
    // all ranges, where `threat_repulsion` (proximity-weighted) fades. Weight 0
    // disables for healers and the Mage.
    if weights.flee > 0.0 {
        if let Some(threat) = inputs.nearest_threat {
            let away = (my_xz - xz(threat)).normalize_or_zero();
            score += weights.flee * candidate.dot(away);
        }
    }

    // Commitment bonus: reward alignment with the previously committed
    // direction (only when the caller says the window is open). `.max(0.0)`
    // so reversal isn't punished beyond losing the bonus.
    if let Some(prev) = inputs.committed_direction {
        score += weights.commitment_bonus * candidate.dot(prev).max(0.0);
    }

    // LoS-seek: flat bonus for a step that can see the kill target — a ranged
    // attacker steers toward angles it can actually shoot from. Weight 0
    // disables; no-op on obstacle-free maps (sight always holds).
    if weights.los_seek > 0.0 {
        if let Some(target) = inputs.los_target {
            let cand_eye = Vec3::new(next.x, EYE_HEIGHT, next.z);
            let target_eye = Vec3::new(target.x, EYE_HEIGHT, target.z);
            if has_line_of_sight(&inputs.obstacles, cand_eye, target_eye) {
                score += weights.los_seek;
            }
        }
    }

    // Cover-pull: flat bonus per threat this step is OCCLUDED from (sight to the
    // threat is broken by an obstacle) — pulls a pressured unit into cover.
    // Weight 0 disables; no-op on obstacle-free maps (never occluded).
    if weights.cover_pull > 0.0 {
        let cand_eye = Vec3::new(next.x, EYE_HEIGHT, next.z);
        for threat in &inputs.threats {
            let threat_eye = Vec3::new(threat.x, EYE_HEIGHT, threat.z);
            if !has_line_of_sight(&inputs.obstacles, cand_eye, threat_eye) {
                score += weights.cover_pull;
            }
        }
    }

    score
}

/// Argmax of the interest pass over the subset of `candidates` whose mask is
/// in `allowed` (a candidate survives when `candidate_mask(c) & !allowed == 0`).
/// Strict `>` comparison preserves the fixed compass order, so ties go to the
/// earlier candidate. Returns `None` when no candidate survives the filter.
fn argmax_interest(
    candidates: &[Vec2],
    inputs: &ScorerInputs,
    weights: &MovementWeights,
    allowed: u16,
) -> Option<Vec2> {
    let mut best: Option<(Vec2, f32)> = None;
    for &candidate in candidates {
        if candidate_mask(candidate, inputs) & !allowed != 0 {
            continue; // violates a mask the caller did not allow — skip
        }
        let score = score_direction(candidate, inputs, weights);
        match best {
            Some((_, b)) if score <= b => {}
            _ => best = Some((candidate, score)),
        }
    }
    best.map(|(dir, _)| dir)
}

/// Bitmask over `candidates` (≤16): bit `i` is set when candidate `i`'s hard
/// constraint mask intersects `bits`. The shared fold behind [`mask_bitmask`]
/// (`bits = u16::MAX`, any constraint) and [`los_mask_bitmask`]
/// (`bits = MASK_LOS`, obstacle only).
fn bitmask_matching(candidates: &[Vec2], inputs: &ScorerInputs, bits: u16) -> u16 {
    candidates.iter().enumerate().fold(0u16, |acc, (i, &c)| {
        if candidate_mask(c, inputs) & bits != 0 {
            acc | (1u16 << i)
        } else {
            acc
        }
    })
}

/// Bitmask over `candidates` (≤16): bit `i` is set when candidate `i` is
/// eliminated by any hard constraint. Feeds the `masked` trace field; a value
/// of `0xFFFF` over the full compass means an all-masked frame (the R6
/// byte-identity attribution signal).
pub fn mask_bitmask(candidates: &[Vec2], inputs: &ScorerInputs) -> u16 {
    bitmask_matching(candidates, inputs, u16::MAX)
}

/// Bitmask over `candidates` (≤16) of only the candidates eliminated by the
/// obstacle mask (`MASK_LOS` — a step that walks into a wall). A strict subset
/// of [`mask_bitmask`], surfaced separately for the `los_masked` trace field so
/// consumers can attribute obstacle-driven divergence without decoding the
/// combined mask. Always `0` on obstacle-free maps.
pub fn los_mask_bitmask(candidates: &[Vec2], inputs: &ScorerInputs) -> u16 {
    bitmask_matching(candidates, inputs, MASK_LOS)
}

/// Both [`mask_bitmask`] and [`los_mask_bitmask`] in one pass, computing each
/// candidate's mask exactly once. Returns `(combined, los_only)` — bit-identical
/// to calling the two individually, but the production trace call sites emit
/// both fields together, so this halves the `candidate_mask` work.
pub fn mask_and_los_bitmask(candidates: &[Vec2], inputs: &ScorerInputs) -> (u16, u16) {
    candidates
        .iter()
        .enumerate()
        .fold((0u16, 0u16), |(all, los), (i, &c)| {
            let mask = candidate_mask(c, inputs);
            let bit = 1u16 << i;
            (
                if mask != 0 { all | bit } else { all },
                if mask & MASK_LOS != 0 { los | bit } else { los },
            )
        })
}

/// Pick a movement direction by argmax of the interest pass over candidates
/// that survive the hard-constraint mask pass (typically
/// [`compass_directions_16`]).
///
/// Fallback ladder when every candidate is masked, lifting constraints in
/// increasing order of harm so a unit is never stranded: (1) allow ally-anchor
/// violations (heal range/sight become a preference once unsatisfiable); (2) if
/// still none, also allow the obstacle mask (a step into a wall the collision
/// resolver will then slide); (3) finally lift the boundary mask too and run the
/// full interest pass — the executor clamps the resulting step back into the
/// arena. Returns `Vec2::ZERO` only for an empty candidate set.
pub fn score_directions(
    candidates: &[Vec2],
    inputs: &ScorerInputs,
    weights: &MovementWeights,
) -> Vec2 {
    // Mask pass: no hard violations allowed.
    if let Some(dir) = argmax_interest(candidates, inputs, weights, 0) {
        return dir;
    }
    // Fallback rung 1: drop the anchor mask.
    if let Some(dir) = argmax_interest(candidates, inputs, weights, MASK_ANCHOR) {
        return dir;
    }
    // Fallback rung 2: also drop the obstacle mask (resolver slides the step).
    if let Some(dir) = argmax_interest(candidates, inputs, weights, MASK_ANCHOR | MASK_LOS) {
        return dir;
    }
    // Fallback rung 3: lift everything; executor clamps the step in-bounds.
    argmax_interest(candidates, inputs, weights, MASK_BOUNDARY | MASK_ANCHOR | MASK_LOS)
        .unwrap_or(Vec2::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::movement_config::{load_movement_config, MovementWeights};

    fn priest_weights() -> MovementWeights {
        load_movement_config()
            .expect("assets/config/movement.ron must load")
            .priest
            .weights
    }

    #[test]
    fn compass_directions_are_unit_and_distinct() {
        let dirs = compass_directions_16();
        for (i, dir) in dirs.iter().enumerate() {
            assert!((dir.length() - 1.0).abs() < 1e-5, "dir {} not unit length", i);
        }
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert!(dirs[i].distance(dirs[j]) > 0.1, "dirs {} and {} coincide", i, j);
            }
        }
    }

    /// (a) Lone threat → the chosen direction points away from it.
    #[test]
    fn lone_threat_scores_away() {
        let inputs = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            threats: vec![Vec3::new(5.0, 1.0, 0.0)], // threat at +X
            wand_range: 30.0,
            ..Default::default()
        };
        let chosen = score_directions(&compass_directions_16(), &inputs, &priest_weights());
        assert!(
            chosen.x < -0.99 && chosen.y.abs() < 0.01,
            "expected ~(-1, 0) away from a +X threat, got {:?}",
            chosen
        );
    }

    /// (b) Ally-anchor constraint: a candidate that leaves heal range of the
    /// anchor never beats an in-range candidate — even when threat repulsion
    /// pushes outward.
    #[test]
    fn anchor_constraint_keeps_chosen_direction_in_heal_range() {
        let anchor = AnchorConstraint {
            pos: Vec3::new(-30.0, 1.0, 0.0),
            heal_range: 40.0,
        };
        let my_pos = Vec3::new(9.7, 1.0, 0.0); // 39.7 from anchor — at the edge
        let inputs = ScorerInputs {
            my_pos,
            lookahead: 2.0,
            // Threat between anchor and healer: pure repulsion would push +X,
            // straight out of heal range.
            threats: vec![Vec3::new(5.0, 1.0, 0.0)],
            anchor: Some(anchor),
            wand_range: 30.0,
            ..Default::default()
        };
        let weights = priest_weights();
        let chosen = score_directions(&compass_directions_16(), &inputs, &weights);

        let next = my_pos + Vec3::new(chosen.x, 0.0, chosen.y) * inputs.lookahead;
        let dist_to_anchor = Vec2::new(next.x - anchor.pos.x, next.z - anchor.pos.z).length();
        assert!(
            dist_to_anchor <= anchor.heal_range,
            "chosen direction {:?} ends {} from anchor — outside heal range {}",
            chosen,
            dist_to_anchor,
            anchor.heal_range
        );

        // And the pure-repulsion direction (+X) really does violate the
        // constraint here — i.e., the test is non-vacuous.
        let outward_next = my_pos + Vec3::new(2.0, 0.0, 0.0);
        let outward_dist =
            Vec2::new(outward_next.x - anchor.pos.x, outward_next.z - anchor.pos.z).length();
        assert!(outward_dist > anchor.heal_range, "test setup: +X must violate heal range");
    }

    /// (c) Corner setup: directions deeper into the corner lose to
    /// center-ward directions.
    #[test]
    fn corner_ward_directions_lose() {
        // Near the +X/+Z corner wall: |x|+|z| = 44 of ARENA_CORNER_SUM 48.88.
        let inputs = ScorerInputs {
            my_pos: Vec3::new(30.0, 1.0, 14.0),
            lookahead: 2.0,
            wand_range: 30.0,
            ..Default::default()
        };
        let weights = priest_weights();

        let corner_ward = Vec2::new(1.0, 1.0).normalize();
        let center_ward = -corner_ward;
        assert!(
            score_direction(corner_ward, &inputs, &weights)
                < score_direction(center_ward, &inputs, &weights),
            "corner-ward must score below center-ward near a corner"
        );

        let chosen = score_directions(&compass_directions_16(), &inputs, &weights);
        assert!(
            chosen.dot(corner_ward) < 0.0,
            "chosen direction {:?} should move away from the corner",
            chosen
        );
    }

    /// (d) Commitment bonus: the previously committed direction wins over a
    /// marginally better alternative within the window, and loses outside it.
    /// THE LEASH IS INERT INSIDE HEAL RANGE. This is the property that lets it
    /// coexist with `flee`: in the region where fleeing is simply correct, the
    /// term contributes exactly nothing, so it cannot blunt an escape.
    #[test]
    fn healer_leash_does_nothing_inside_heal_range() {
        let weights = MovementWeights { healer_leash: 4.0, ..priest_weights() };
        let dirs = compass_directions_16();
        let mut inputs = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            threats: vec![Vec3::new(5.0, 1.0, 0.0)],
            wand_range: 30.0,
            healer_leash_range: 40.0,
            ..Default::default()
        };
        // Healer 10yd away — well inside the leash.
        inputs.healer_point = Some(Vec3::new(10.0, 0.0, 0.0));
        let with_healer = score_directions(&dirs, &inputs, &weights);
        inputs.healer_point = None;
        let without = score_directions(&dirs, &inputs, &weights);
        assert_eq!(
            with_healer, without,
            "a healer inside leash range must not change the chosen direction at all"
        );
    }

    /// ...and it DOES pull once beyond it, which is the whole point: a Hunter
    /// fleeing a Rogue has no other term that knows its healer exists.
    #[test]
    fn healer_leash_pulls_back_once_beyond_heal_range() {
        let weights = MovementWeights {
            healer_leash: 40.0,
            flee: 0.0,
            threat_repulsion: 0.0,
            formation_pull: 0.0,
            corner_penalty: 0.0,
            ..priest_weights()
        };
        let dirs = compass_directions_16();
        let mut inputs = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            wand_range: 30.0,
            healer_leash_range: 40.0,
            ..Default::default()
        };
        // Healer far behind us at -X: the leash should point that way.
        inputs.healer_point = Some(Vec3::new(-80.0, 0.0, 0.0));
        let chosen = score_directions(&dirs, &inputs, &weights);
        assert!(
            chosen.x < -0.5,
            "expected a pull back toward the healer at -X, got {chosen:?}"
        );
    }

    #[test]
    fn commitment_bonus_wins_within_window_only() {
        // Isolate the interaction: only threat repulsion + commitment active.
        let weights = MovementWeights {
            threat_repulsion: 1.0,
            formation_pull: 0.0,
            corner_penalty: 0.0,
            wand_pull: 0.0,
            range_band: 0.0,
            flee: 0.0,
            burn_pull: 0.0,
            commitment_bonus: 0.5,
            los_seek: 0.0,
            cover_pull: 0.0,
            healer_leash: 0.0,
        };
        let dirs = compass_directions_16();
        let ideal_away = dirs[8]; // ~(-1, 0): exactly away from a +X threat
        let committed = dirs[9]; // one compass step off ideal (22.5°)

        let mut inputs = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            threats: vec![Vec3::new(5.0, 1.0, 0.0)],
            wand_range: 30.0,
            committed_direction: Some(committed),
            ..Default::default()
        };

        // Within the window: the committed direction's bonus outweighs the
        // marginal repulsion advantage of the ideal direction.
        let chosen = score_directions(&dirs, &inputs, &weights);
        assert_eq!(
            chosen, committed,
            "within the commitment window the committed direction must win"
        );

        // Outside the window (caller passes None): the ideal direction wins.
        inputs.committed_direction = None;
        let chosen = score_directions(&dirs, &inputs, &weights);
        assert_eq!(
            chosen, ideal_away,
            "outside the window the marginally better direction must win"
        );
    }

    #[test]
    fn empty_candidates_yield_zero() {
        let inputs = ScorerInputs::default();
        assert_eq!(score_directions(&[], &inputs, &priest_weights()), Vec2::ZERO);
    }

    /// Oracle reproducing the OLD penalty scheme: soft interest terms minus a
    /// dominant `ally_anchor` / `boundary_penalty` for each violated hard
    /// constraint, argmax with the same strict-`>` index tie-break. Used to
    /// prove the mask refactor is behaviorally identical whenever at least one
    /// candidate survives the mask pass.
    fn penalty_argmax(
        candidates: &[Vec2],
        inputs: &ScorerInputs,
        weights: &MovementWeights,
    ) -> Vec2 {
        // The old scheme's dominant penalty (shipped `ally_anchor` /
        // `boundary_penalty` were both 1000.0, retired in U3). Hardcoded here so
        // the oracle reproduces the old behavior independent of the live config.
        const DOMINANT_PENALTY: f32 = 1000.0;
        let mut best_direction = Vec2::ZERO;
        let mut best_score = f32::MIN;
        for &candidate in candidates {
            let mut score = score_direction(candidate, inputs, weights);
            let mask = candidate_mask(candidate, inputs);
            if mask & MASK_ANCHOR != 0 {
                score -= DOMINANT_PENALTY;
            }
            if mask & MASK_BOUNDARY != 0 {
                score -= DOMINANT_PENALTY;
            }
            if score > best_score {
                best_score = score;
                best_direction = candidate;
            }
        }
        best_direction
    }

    /// Mask-argmax equals penalty-argmax for every frame with at least one
    /// unmasked candidate — the core near-identity claim of the refactor.
    /// Sweeps a grid of healer positions (with a fixed anchor + threat) rather
    /// than hand-picked coordinates, and verifies the sweep actually exercises
    /// boundary-only, anchor-only, and CROSS-CONSTRAINT frames (a frame with
    /// both an anchor-only-masked and a boundary-only-masked candidate — the
    /// case the adversarial review flagged, where the old penalty scheme tied
    /// both at -1000 and the mask scheme drops both).
    #[test]
    fn mask_argmax_equals_penalty_argmax_when_a_candidate_survives() {
        let weights = priest_weights();
        let dirs = compass_directions_16();
        // Anchor near the +X wall so positions near it can be simultaneously
        // close to the heal-range edge AND close to the boundary.
        let anchor = AnchorConstraint { pos: Vec3::new(34.0, 1.0, 0.0), heal_range: 40.0 };
        let threat = Vec3::new(0.0, 1.0, 0.0);

        let mut checked = 0u32;
        let mut saw_boundary_only = 0u32;
        let mut saw_anchor_only = 0u32;

        // Sweep the arena interior on a coarse grid.
        let mut x = -34.0_f32;
        while x <= 34.0 {
            let mut z = -34.0_f32;
            while z <= 34.0 {
                let inputs = ScorerInputs {
                    my_pos: Vec3::new(x, 1.0, z),
                    lookahead: 2.0,
                    threats: vec![threat],
                    anchor: Some(anchor),
                    wand_range: 30.0,
                    ..Default::default()
                };

                let masks: Vec<u16> = dirs.iter().map(|&d| candidate_mask(d, &inputs)).collect();
                let survivors = masks.iter().filter(|&&m| m == 0).count();
                let boundary_only = masks.iter().any(|&m| m == MASK_BOUNDARY);
                let anchor_only = masks.iter().any(|&m| m == MASK_ANCHOR);
                if boundary_only {
                    saw_boundary_only += 1;
                }
                if anchor_only {
                    saw_anchor_only += 1;
                }

                // Equivalence only claimed when a candidate survives the mask.
                if survivors >= 1 {
                    checked += 1;
                    assert_eq!(
                        score_directions(&dirs, &inputs, &weights),
                        penalty_argmax(&dirs, &inputs, &weights),
                        "mask-argmax must equal penalty-argmax at ({x}, {z})",
                    );
                }
                z += 4.0;
            }
            x += 4.0;
        }

        // The sweep must be non-vacuous on every masking shape, or it proves
        // nothing about the cases that matter.
        assert!(checked > 50, "expected a broad sweep, only checked {checked} frames");
        assert!(saw_boundary_only > 0, "sweep never produced a boundary-only-masked candidate");
        assert!(saw_anchor_only > 0, "sweep never produced an anchor-only-masked candidate");
        // Note: a single frame holding BOTH an anchor-only and a boundary-only
        // candidate is geometrically coupled at this lookahead (near a flat
        // wall, the outward step exits bounds and heal-range together, masking
        // as both). Independent coverage of each single-mask kind across the
        // sweep is what exercises the adversarial "two candidates tied at the
        // old -1000 penalty" risk: the argmax winner is always a survivor, so
        // dropping the tied losers never changes it.
    }

    /// AE1: every candidate leaves heal range of the anchor, but the healer
    /// is in open space — fallback rung 1 drops the anchor mask and returns an
    /// in-bounds direction (never `Vec2::ZERO`, never a directive drop).
    #[test]
    fn all_anchor_masked_drops_anchor_and_picks_in_bounds() {
        let weights = priest_weights();
        let dirs = compass_directions_16();
        // Anchor 50yd away with a 10yd heal range: every candidate is
        // anchor-masked, none boundary-masked (open center).
        let inputs = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            threats: vec![Vec3::new(5.0, 1.0, 0.0)],
            anchor: Some(AnchorConstraint { pos: Vec3::new(0.0, 1.0, -50.0), heal_range: 10.0 }),
            wand_range: 30.0,
            ..Default::default()
        };
        assert!(
            dirs.iter().all(|&d| candidate_mask(d, &inputs) & MASK_ANCHOR != 0),
            "setup: every candidate must be anchor-masked",
        );
        let chosen = score_directions(&dirs, &inputs, &weights);
        assert_ne!(chosen, Vec2::ZERO, "anchor-drop fallback must return a direction");
        assert_eq!(
            candidate_mask(chosen, &inputs) & MASK_BOUNDARY,
            0,
            "chosen direction must remain in bounds",
        );
        // Threat at +X: with the anchor mask dropped, repulsion still points -X.
        assert!(chosen.x < -0.9, "expected ~(-1,0) away from a +X threat, got {chosen:?}");
    }

    /// range_band: pull inward when beyond `max`, push outward when inside
    /// `min`, no contribution while in-band or when the target is absent.
    #[test]
    fn range_band_pulls_toward_band_and_pushes_out_of_min() {
        let weights = MovementWeights { range_band: 1.0, ..MovementWeights::default() };
        let dirs = compass_directions_16();
        let band = RangeBand { target: Vec3::new(40.0, 1.0, 0.0), min: 8.0, max: 30.0 };

        // (a) FAR (>max): kill target at +X 40yd away → pull toward +X.
        let far = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            range_band: Some(band),
            wand_range: 30.0,
            ..Default::default()
        };
        let chosen = score_directions(&dirs, &far, &weights);
        assert!(chosen.x > 0.9, "far from band must pull toward +X target, got {chosen:?}");

        // (b) TOO CLOSE (<min): target 4yd away at +X → push toward -X.
        let near = ScorerInputs {
            my_pos: Vec3::new(36.0, 1.0, 0.0),
            range_band: Some(RangeBand { target: Vec3::new(40.0, 1.0, 0.0), min: 8.0, max: 30.0 }),
            ..far.clone()
        };
        let chosen = score_directions(&dirs, &near, &weights);
        assert!(chosen.x < -0.9, "inside min must push away from target, got {chosen:?}");

        // (c) IN-BAND (min<=d<=max): target 20yd away → range_band silent, so a
        // lone threat term decides. Threat at +X → away (-X).
        let in_band = ScorerInputs {
            my_pos: Vec3::new(20.0, 1.0, 0.0),
            threats: vec![Vec3::new(25.0, 1.0, 0.0)],
            range_band: Some(RangeBand { target: Vec3::new(40.0, 1.0, 0.0), min: 8.0, max: 30.0 }),
            ..far.clone()
        };
        let w2 = MovementWeights { range_band: 1.0, threat_repulsion: 3.0, ..MovementWeights::default() };
        let chosen = score_directions(&dirs, &in_band, &w2);
        assert!(chosen.x < -0.9, "in-band range_band is silent; threat repulsion decides, got {chosen:?}");

        // (d) No target → term contributes nothing (no panic, threat decides).
        let none = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            threats: vec![Vec3::new(5.0, 1.0, 0.0)],
            range_band: None,
            ..far.clone()
        };
        let chosen = score_directions(&dirs, &none, &w2);
        assert!(chosen.x < -0.9, "no band target → threat repulsion decides, got {chosen:?}");
    }

    /// AE4: arc-kiting — a Mage fleeing a pursuer while its kill target is a
    /// DIFFERENT enemy near max cast range bends along the band (keeps the kill
    /// target in range) instead of running straight away from the pursuer.
    #[test]
    fn arc_kiting_keeps_kill_target_in_band() {
        // Pursuer behind the Mage (−X), kill target ahead (+X) at the outer
        // ring. Pure repulsion would flee +X (away from pursuer) and overshoot
        // the kill target out of range; range_band bends the step so the kill
        // target stays within max.
        let weights = MovementWeights {
            threat_repulsion: 3.0,
            range_band: 2.0,
            ..MovementWeights::default()
        };
        let dirs = compass_directions_16();
        let my_pos = Vec3::new(0.0, 1.0, 0.0);
        let kill_target = Vec3::new(-2.0, 1.0, 28.0); // +Z, ~28yd: just inside max
        let pursuer = Vec3::new(0.0, 1.0, -6.0); // −Z, close behind
        let inputs = ScorerInputs {
            my_pos,
            lookahead: 2.0,
            threats: vec![pursuer],
            range_band: Some(RangeBand { target: kill_target, min: 8.0, max: 30.0 }),
            wand_range: 30.0,
            ..Default::default()
        };
        let chosen = score_directions(&dirs, &inputs, &weights);
        // Straight flee from the −Z pursuer would be +Z toward the kill target
        // (fine here) — the real arc test: the chosen step must not increase
        // distance to the kill target beyond max. Verify the step keeps the
        // kill target within max.
        let next = my_pos + Vec3::new(chosen.x, 0.0, chosen.y) * 2.0;
        let dist_after = Vec2::new(next.x - kill_target.x, next.z - kill_target.z).length();
        assert!(
            dist_after <= 30.0,
            "arc-kite step must keep the kill target within max range, got {dist_after}",
        );
        // And it must still move away from the pursuer (z component positive).
        assert!(chosen.y > 0.0, "must still gain separation from the −Z pursuer, got {chosen:?}");
    }

    /// Corner-escape (the Hunter migration's load-bearing fix): a kiter pinned
    /// near a corner with the threat between it and open space must bend OUT of
    /// the corner, not pin into it. The legacy `find_best_kiting_direction` fled
    /// straight away (into the corner, then hugged the wall); here the boundary
    /// mask removes the corner-ward steps, `corner_penalty` discourages the
    /// remaining wall-ward ones, and `flee` + the surviving directions push the
    /// kiter back toward center — the chosen step reduces |x|+|z|.
    #[test]
    fn cornered_kiter_bends_out_of_the_corner() {
        // Hunter flee profile (matches the shipped hunter block). corner_penalty
        // must exceed flee so the corner term wins near the octagon wall —
        // otherwise the kiter flees straight into the corner.
        let weights = MovementWeights {
            threat_repulsion: 1.0,
            formation_pull: 0.0,
            corner_penalty: 8.0,
            wand_pull: 0.0,
            range_band: 0.0,
            flee: 6.0,
            burn_pull: 0.0,
            commitment_bonus: 0.0,
            los_seek: 0.0,
            cover_pull: 0.0,
            healer_leash: 0.0,
        };
        let dirs = compass_directions_16();
        // Near the +X/+Z corner (|x|+|z| = 48, just inside ARENA_CORNER_SUM),
        // threat on the diagonal between the Hunter and center — so "flee away
        // from threat" points deeper into the corner (out of bounds).
        // Arena is 36.5 (x) × 21.5 (z) with octagon corners at |x|+|z| ≤ 48.88.
        // Place the kiter near the +X/+Z octagon corner (in bounds), threat on
        // the diagonal between it and center — so "flee away" points into the
        // corner, where the octagon boundary mask removes the deepest steps.
        let threat = Vec3::new(22.0, 1.0, 13.0);
        // Walk a few steps under the scorer; the kiter should round the corner
        // (perpendicular slide) and end with a SMALLER |x|+|z| — i.e. escape,
        // not pin. A single frame can be a lateral slide (corner-distance flat);
        // over the walk it must actually decrease.
        let mut pos = Vec3::new(28.0, 1.0, 19.0);
        let corner_before = pos.x.abs() + pos.z.abs();
        for step in 0..10 {
            let inputs = ScorerInputs {
                my_pos: pos,
                lookahead: 2.0,
                threats: vec![threat],
                nearest_threat: Some(threat),
                wand_range: 30.0,
                ..Default::default()
            };
            let chosen = score_directions(&dirs, &inputs, &weights);
            assert_ne!(chosen, Vec2::ZERO, "step {step}: must pick a direction, not pin");
            pos += Vec3::new(chosen.x, 0.0, chosen.y) * 2.0;
            // Never pinned against the octagon wall (the corner-stuck symptom).
            assert!(
                pos.x.abs() + pos.z.abs() <= super::ARENA_CORNER_SUM + 1e-3,
                "step {step}: pinned past the octagon wall, |x|+|z|={}",
                pos.x.abs() + pos.z.abs()
            );
        }
        // Over the walk the kiter rounds the corner and ends meaningfully out of
        // it — not pinned. (Single steps may slide laterally; the walk escapes.)
        let corner_after = pos.x.abs() + pos.z.abs();
        assert!(
            corner_after < corner_before - 2.0,
            "cornered kiter must escape the corner over the walk (|x|+|z| {corner_before} -> {corner_after})",
        );
    }

    /// Double-fallback: the healer is out of bounds (every candidate is
    /// boundary-masked even after the anchor mask drops). Rung 2 lifts the
    /// boundary mask and returns a finite, non-zero direction for the executor
    /// to clamp — the directive is never silently dropped.
    #[test]
    fn all_boundary_masked_lifts_boundary_and_returns_finite() {
        let weights = priest_weights();
        let dirs = compass_directions_16();
        // Far outside the arena: every lookahead step is still out of bounds.
        let inputs = ScorerInputs {
            my_pos: Vec3::new(500.0, 1.0, 500.0),
            lookahead: 2.0,
            threats: vec![Vec3::new(0.0, 1.0, 0.0)],
            wand_range: 30.0,
            ..Default::default()
        };
        assert!(
            dirs.iter().all(|&d| candidate_mask(d, &inputs) & MASK_BOUNDARY != 0),
            "setup: every candidate must be boundary-masked",
        );
        let chosen = score_directions(&dirs, &inputs, &weights);
        assert_ne!(chosen, Vec2::ZERO, "boundary-lift fallback must return a direction");
        assert!(chosen.is_finite(), "chosen direction must be finite, got {chosen:?}");
    }

    // ---- obstacle (LoS) mask + terms ----------------------------------

    /// A tall cylinder pillar spanning ground height, centered at `(cx, cz)`.
    fn pillar(cx: f32, cz: f32, r: f32) -> ObstacleVolume {
        ObstacleVolume::Cylinder {
            center_xz: Vec2::new(cx, cz),
            radius: r,
            base_y: 0.0,
            height: 10.0,
        }
    }

    fn zeroed_weights() -> MovementWeights {
        MovementWeights {
            threat_repulsion: 0.0,
            formation_pull: 0.0,
            corner_penalty: 0.0,
            wand_pull: 0.0,
            burn_pull: 0.0,
            range_band: 0.0,
            flee: 0.0,
            commitment_bonus: 0.0,
            los_seek: 0.0,
            cover_pull: 0.0,
            healer_leash: 0.0,
        }
    }

    /// A candidate whose step walks into a pillar is `MASK_LOS`-masked,
    /// while an open direction survives the mask pass.
    #[test]
    fn step_into_pillar_is_los_masked() {
        // Pillar centered at origin (r=5); the mover sits just west of it.
        let inputs = ScorerInputs {
            my_pos: Vec3::new(-6.0, 1.0, 0.0),
            lookahead: 2.0,
            obstacles: vec![pillar(0.0, 0.0, 5.0)],
            ..Default::default()
        };
        // +X steps to (-4, 0), inside the r=5 footprint (+ mover radius) → blocked.
        let into = Vec2::new(1.0, 0.0);
        assert_ne!(
            candidate_mask(into, &inputs) & MASK_LOS,
            0,
            "a step into the pillar must set MASK_LOS",
        );
        // -X steps to (-8, 0), clear of the footprint → no LoS mask, survives.
        let away = Vec2::new(-1.0, 0.0);
        assert_eq!(candidate_mask(away, &inputs), 0, "an open step must be unmasked");
    }

    /// The fallback ladder drops the ANCHOR mask before the LoS mask.
    /// Two candidates — one anchor-only-masked, one LoS-masked — with none
    /// clear: the ladder must pick the anchor-only one (LoS bit still clear).
    #[test]
    fn ladder_drops_anchor_before_los() {
        let weights = zeroed_weights();
        // Anchor far west; +X leaves heal range (anchor mask), -X steps into a
        // pillar (LoS mask). No candidate is clear.
        let inputs = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            anchor: Some(AnchorConstraint { pos: Vec3::new(-10.0, 1.0, 0.0), heal_range: 11.0 }),
            obstacles: vec![pillar(-2.0, 0.0, 1.0)],
            ..Default::default()
        };
        let a = Vec2::new(1.0, 0.0); // +X: out of heal range → anchor-masked
        let b = Vec2::new(-1.0, 0.0); // -X: into the pillar → LoS-masked
        assert_ne!(candidate_mask(a, &inputs) & MASK_ANCHOR, 0, "setup: +X anchor-masked");
        assert_ne!(candidate_mask(b, &inputs) & MASK_LOS, 0, "setup: -X LoS-masked");
        // Ladder: rung 1 drops anchor → +X becomes eligible while -X (LoS) does not.
        let chosen = score_directions(&[a, b], &inputs, &weights);
        assert_eq!(chosen, a, "anchor must lift before LoS, choosing the anchor-only candidate");
    }

    /// The fallback ladder drops the LoS mask before the BOUNDARY mask.
    /// One candidate LoS-masked (in bounds), one boundary-masked: the ladder
    /// must pick the LoS one.
    #[test]
    fn ladder_drops_los_before_boundary() {
        let weights = zeroed_weights();
        // Near the +X wall: +X exits bounds; -Z steps into a pillar (in bounds).
        let inputs = ScorerInputs {
            my_pos: Vec3::new(35.0, 1.0, 0.0),
            lookahead: 2.0,
            obstacles: vec![pillar(35.0, -4.0, 2.0)],
            ..Default::default()
        };
        let c = Vec2::new(0.0, -1.0); // -Z: into the pillar, still in bounds
        let d = Vec2::new(1.0, 0.0); // +X: out of arena bounds
        assert_eq!(candidate_mask(c, &inputs), MASK_LOS, "setup: -Z LoS-masked only");
        assert_eq!(candidate_mask(d, &inputs), MASK_BOUNDARY, "setup: +X boundary-masked only");
        let chosen = score_directions(&[c, d], &inputs, &weights);
        assert_eq!(chosen, c, "LoS must lift before boundary, choosing the in-bounds candidate");
    }

    /// With every candidate masked, the ladder still returns a direction
    /// (never strands the unit).
    #[test]
    fn all_masked_ladder_returns_direction() {
        let weights = zeroed_weights();
        // Far outside the arena AND inside a giant pillar: every compass step is
        // both boundary- and LoS-masked.
        let inputs = ScorerInputs {
            my_pos: Vec3::new(500.0, 1.0, 500.0),
            lookahead: 2.0,
            obstacles: vec![pillar(500.0, 500.0, 50.0)],
            ..Default::default()
        };
        let dirs = compass_directions_16();
        assert!(
            dirs.iter().all(|&d| candidate_mask(d, &inputs) != 0),
            "setup: every candidate masked",
        );
        let chosen = score_directions(&dirs, &inputs, &weights);
        assert_ne!(chosen, Vec2::ZERO, "the ladder must never strand the unit");
    }

    /// The anchor constraint is distance AND sight. A candidate within
    /// heal range but OCCLUDED from the anchor is anchor-masked; removing the
    /// pillar clears it (proving occlusion, not distance, caused the mask).
    #[test]
    fn anchor_masked_when_occluded_in_range() {
        let anchor = AnchorConstraint { pos: Vec3::new(0.0, 1.0, -5.0), heal_range: 40.0 };
        let candidate = Vec2::new(0.0, 1.0); // +Z: steps to (0, 2), 7yd from anchor (in range)
        let occluded = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            anchor: Some(anchor),
            obstacles: vec![pillar(0.0, -2.0, 1.0)], // between the step and the anchor
            ..Default::default()
        };
        assert_ne!(
            candidate_mask(candidate, &occluded) & MASK_ANCHOR,
            0,
            "in range but occluded → anchor-masked",
        );
        let clear = ScorerInputs { obstacles: vec![], ..occluded.clone() };
        assert_eq!(
            candidate_mask(candidate, &clear) & MASK_ANCHOR,
            0,
            "same geometry without the pillar is NOT anchor-masked — occlusion caused it",
        );
    }

    /// (reverse) a candidate in clear LoS but OUT of heal range is still
    /// anchor-masked (distance half of the AND).
    #[test]
    fn anchor_masked_when_out_of_range_in_sight() {
        let inputs = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            anchor: Some(AnchorConstraint { pos: Vec3::new(0.0, 1.0, -50.0), heal_range: 10.0 }),
            obstacles: vec![], // clear sight
            ..Default::default()
        };
        let candidate = Vec2::new(0.0, 1.0); // steps AWAY from the anchor
        assert_ne!(
            candidate_mask(candidate, &inputs) & MASK_ANCHOR,
            0,
            "out of range (even in clear sight) → anchor-masked",
        );
    }

    /// `cover_pull` scores a step that hides from a threat above an
    /// exposed one; `los_seek` scores a step that sees the target above an
    /// occluded one. Same geometry, opposite sign.
    #[test]
    fn cover_pull_and_los_seek_shape() {
        let point = Vec3::new(0.0, 1.0, 10.0); // the threat / the kill target
        let my_pos = Vec3::new(0.0, 1.0, -3.0);
        let obstacles = vec![pillar(0.0, 5.0, 0.5)]; // thin pillar between us and the point
        let hiding = Vec2::new(0.0, 1.0); // +Z: stays on the axis, occluded by the pillar
        let exposed = Vec2::new(1.0, 0.0); // +X: steps off-axis, clear line

        // cover_pull: hiding (occluded from the threat) beats exposed.
        let cover_inputs = ScorerInputs {
            my_pos,
            lookahead: 2.0,
            threats: vec![point],
            obstacles: obstacles.clone(),
            ..Default::default()
        };
        let cover_w = MovementWeights { cover_pull: 1.0, ..zeroed_weights() };
        assert!(
            score_direction(hiding, &cover_inputs, &cover_w)
                > score_direction(exposed, &cover_inputs, &cover_w),
            "cover_pull must reward the occluded (hiding) direction",
        );

        // los_seek: exposed (sees the target) beats hiding (occluded).
        let seek_inputs = ScorerInputs {
            my_pos,
            lookahead: 2.0,
            los_target: Some(point),
            obstacles,
            ..Default::default()
        };
        let seek_w = MovementWeights { los_seek: 1.0, ..zeroed_weights() };
        assert!(
            score_direction(exposed, &seek_inputs, &seek_w)
                > score_direction(hiding, &seek_inputs, &seek_w),
            "los_seek must reward the direction that keeps sight of the target",
        );
    }

    /// Mask bits are deterministic — identical across repeated evaluation
    /// of the same inputs.
    #[test]
    fn mask_bits_are_deterministic() {
        let inputs = ScorerInputs {
            my_pos: Vec3::new(-6.0, 1.0, 0.0),
            lookahead: 2.0,
            anchor: Some(AnchorConstraint { pos: Vec3::new(-20.0, 1.0, 0.0), heal_range: 18.0 }),
            obstacles: vec![pillar(0.0, 0.0, 5.0), pillar(-10.0, 4.0, 2.0)],
            ..Default::default()
        };
        let dirs = compass_directions_16();
        let first = mask_bitmask(&dirs, &inputs);
        let first_los = los_mask_bitmask(&dirs, &inputs);
        for _ in 0..8 {
            assert_eq!(mask_bitmask(&dirs, &inputs), first, "combined mask must be stable");
            assert_eq!(los_mask_bitmask(&dirs, &inputs), first_los, "LoS mask must be stable");
        }
        // The LoS-only mask is a subset of the combined mask.
        assert_eq!(first_los & !first, 0, "los_mask must be a subset of mask");
    }

    /// Obstacle-free no-op. With no obstacles, MASK_LOS is never set, and
    /// the new `los_seek`/`cover_pull` terms (even at high weight) do not change
    /// the chosen direction versus zero-weight — the BasicArena byte-identity
    /// guarantee at the scorer level.
    #[test]
    fn obstacle_free_is_a_noop() {
        let dirs = compass_directions_16();
        let threat = Vec3::new(5.0, 1.0, 0.0);
        let base = ScorerInputs {
            my_pos: Vec3::new(0.0, 1.0, 0.0),
            lookahead: 2.0,
            threats: vec![threat],
            wand_range: 30.0,
            obstacles: vec![], // BasicArena
            ..Default::default()
        };
        // No candidate can ever be LoS-masked without obstacles.
        assert!(
            dirs.iter().all(|&d| candidate_mask(d, &base) & MASK_LOS == 0),
            "empty obstacles must never set MASK_LOS",
        );
        let weights = priest_weights();
        let chosen_plain = score_directions(&dirs, &base, &weights);

        // Same inputs + a target, with los_seek/cover_pull turned way up: on an
        // obstacle-free map cover_pull never fires and los_seek adds a uniform
        // constant, so the argmax is unchanged.
        let with_target = ScorerInputs { los_target: Some(threat), ..base.clone() };
        let loud = MovementWeights { los_seek: 100.0, cover_pull: 100.0, ..weights };
        let chosen_loud = score_directions(&dirs, &with_target, &loud);
        assert_eq!(
            chosen_plain, chosen_loud,
            "obstacle-free scoring must be invariant to the new terms",
        );
    }
}
