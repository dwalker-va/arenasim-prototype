//! Pure position scorer for healer movement directives (U5).
//!
//! Extends the shape of `find_best_kiting_direction` (16-direction argmax)
//! into a multi-term scorer with named, RON-tunable weights
//! (`assets/config/movement.ron` → `MovementWeights`). Pure free functions
//! over caller-built inputs: no Bevy world, no queries — unit-testable in
//! isolation, and deterministic as long as callers build `ScorerInputs` from
//! BTree-ordered snapshots (floating-point summation order follows input
//! order).
//!
//! Terms (see the plan's scorer-terms table):
//! - **Threat repulsion** — away from each visible threat, proximity-weighted
//!   (PRESSURED).
//! - **Ally-anchor constraint** — HARD penalty for candidate positions
//!   outside heal range of the anchor ally (PRESSURED).
//! - **Formation pull** — toward the formation point behind the engaged-ally
//!   centroid (FREE, Priest).
//! - **Boundary penalty** — HARD penalty for out-of-bounds candidate
//!   positions (reuses `is_in_arena_bounds`, like the kiting branch).
//! - **Corner penalty** — graded penalty approaching the octagon's corners
//!   (the |x|+|z| diagonal walls), so escapes bend along walls instead of
//!   pinning into corners.
//! - **Wand-range pull** — low-weight pull toward wand range of the kill
//!   target (weight 0 disables; Paladin).
//! - **Commitment bonus** — toward the previously committed direction,
//!   applied only AT re-evaluation while the commitment window is open. The
//!   hard `committed_until` window on `MovementDirective` decides WHEN
//!   re-evaluation happens; this term applies only at that moment — the two
//!   never stack.
//!
//! `find_best_kiting_direction` and the kiting branch are deliberately NOT
//! touched (R14: Mage/Hunter kiting stays on its existing mechanism).

use bevy::prelude::*;

use super::is_in_arena_bounds;
use super::super::movement_config::MovementWeights;
use super::super::ARENA_CORNER_SUM;

/// Where the graded corner penalty starts biting, as |x|+|z|. Below this the
/// term is zero; it ramps linearly to full weight at the corner wall
/// (`ARENA_CORNER_SUM`). ~70% of the wall keeps the center half of the arena
/// penalty-free.
pub const CORNER_PENALTY_ONSET: f32 = ARENA_CORNER_SUM * 0.7;

/// The 16 compass candidate directions (unit XZ vectors, TAU/16 apart),
/// matching `find_best_kiting_direction`'s candidate scan. Index order is
/// fixed, so argmax tie-breaks are deterministic.
pub fn compass_directions_16() -> [Vec2; 16] {
    std::array::from_fn(|i| {
        let angle = (i as f32) * std::f32::consts::TAU / 16.0;
        Vec2::new(angle.cos(), angle.sin())
    })
}

/// Ally-anchor constraint: candidate positions farther than `heal_range`
/// from `pos` take the hard `ally_anchor` penalty.
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
    /// Previously committed direction. `Some` ONLY while the commitment
    /// window is open — the caller passes `None` outside it, which disables
    /// the term entirely.
    pub committed_direction: Option<Vec2>,
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// Score a single candidate direction (unit XZ). Higher is better.
pub fn score_direction(candidate: Vec2, inputs: &ScorerInputs, weights: &MovementWeights) -> f32 {
    let my_xz = xz(inputs.my_pos);
    let next = inputs.my_pos + Vec3::new(candidate.x, 0.0, candidate.y) * inputs.lookahead;
    let next_xz = xz(next);

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

    // Ally-anchor hard penalty: candidate positions outside heal range of
    // the anchor never beat an in-range candidate (weight dominates all soft
    // terms — enforced by MovementConfig::validate()).
    if let Some(anchor) = inputs.anchor {
        if next_xz.distance(xz(anchor.pos)) > anchor.heal_range {
            score -= weights.ally_anchor;
        }
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

    // Boundary hard penalty: same bounds test as the kiting branch, but as a
    // dominant penalty instead of a skip so the argmax still returns the
    // least-bad direction when every candidate is constrained.
    if !is_in_arena_bounds(next) {
        score -= weights.boundary_penalty;
    }

    // Corner penalty: graded ramp toward the octagon's diagonal corner walls
    // (|x|+|z|), normalized to 0..1 between onset and the wall itself.
    let corner_closeness =
        ((next.x.abs() + next.z.abs()) - CORNER_PENALTY_ONSET) / (ARENA_CORNER_SUM - CORNER_PENALTY_ONSET);
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

    // Commitment bonus: reward alignment with the previously committed
    // direction (only when the caller says the window is open). `.max(0.0)`
    // so reversal isn't punished beyond losing the bonus.
    if let Some(prev) = inputs.committed_direction {
        score += weights.commitment_bonus * candidate.dot(prev).max(0.0);
    }

    score
}

/// Argmax over `candidates` (typically [`compass_directions_16`]). Strict
/// `>` comparison: ties go to the earlier candidate, so the fixed compass
/// order makes the choice deterministic. Returns `Vec2::ZERO` for an empty
/// candidate set.
pub fn score_directions(
    candidates: &[Vec2],
    inputs: &ScorerInputs,
    weights: &MovementWeights,
) -> Vec2 {
    let mut best_direction = Vec2::ZERO;
    let mut best_score = f32::MIN;

    for &candidate in candidates {
        let score = score_direction(candidate, inputs, weights);
        if score > best_score {
            best_score = score;
            best_direction = candidate;
        }
    }

    best_direction
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
    #[test]
    fn commitment_bonus_wins_within_window_only() {
        // Isolate the interaction: only threat repulsion + commitment active.
        let weights = MovementWeights {
            threat_repulsion: 1.0,
            ally_anchor: 0.0,
            formation_pull: 0.0,
            boundary_penalty: 0.0,
            corner_penalty: 0.0,
            wand_pull: 0.0,
            commitment_bonus: 0.5,
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
}
