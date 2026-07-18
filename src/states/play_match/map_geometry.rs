//! Pure analytic geometry for line-of-sight and collision.
//!
//! This is the deterministic math foundation of the line-of-sight feature:
//! maps declare a `Vec` of [`ObstacleVolume`]s (in declaration order), and this
//! module answers the three questions the sim needs about them:
//!
//! - **Point-in-volume** — [`contains_point`].
//! - **Segment-vs-volume** and the derived **line-of-sight** query
//!   ([`segment_intersects`], [`has_line_of_sight`]) — gates casts/heals/autos.
//! - **Collision-resolved movement** — [`resolve_movement`] slides a proposed
//!   step along an obstacle surface instead of clipping through it.
//!
//! **Bevy-free by construction.** No ECS, no `Plugin`, no `SystemParam` — just
//! pure functions over `bevy::math` (glam) vector types, which the whole crate
//! already uses. This keeps the math unit-testable in isolation and, more
//! importantly, *deterministic*: the sim's probe harness proves bit-identical
//! seeded runs, so nothing here may iterate a `HashMap`/`HashSet` (KTD16).
//! Obstacle lists are `&[ObstacleVolume]` walked in slice order; all arithmetic
//! is plain `f32` with a fixed evaluation order.
//!
//! ## Edge policies (pinned for determinism)
//!
//! - **Line-of-sight: touching = blocked.** [`segment_intersects`] and
//!   [`contains_point`] use *closed* (inclusive) tests, so a segment grazing
//!   tangent to a cylinder edge, or ending exactly on a box face, counts as an
//!   intersection. A small [`TOUCH_EPS`] slack biases genuine grazes toward
//!   "blocked" so a hair of float error never flips a tangent to "clear".
//! - **Zero-length segment (`from == to`): has line of sight iff the point is
//!   not inside any volume.** A point sees itself; it is blocked from itself
//!   only when it sits inside an obstacle. This falls out of treating the
//!   degenerate segment as a point-in-volume test.
//! - **Movement: touching = allowed.** [`resolve_movement`] uses *strict*
//!   (exclusive) penetration tests so a mover may come to rest flush against a
//!   wall. This is deliberately the opposite of the LoS policy — a unit pinned
//!   against a pillar should still be a legal position, whereas sight along a
//!   grazing line should be denied. Both are documented and tested.
//!
//! ## Mover model
//!
//! Movers are ground units at `y ≈ 1.0` treated as a small disc of radius
//! [`MOVER_RADIUS`] in the XZ plane. Collision is XZ-planar against a volume's
//! footprint, and only when the mover's `y` falls within the volume's `y` span
//! — an elevated platform whose span excludes `y ≈ 1.0` does not block ground
//! movement (though it can still block a diagonal line of sight).

use bevy::prelude::*;

/// Inclusive-test slack for line-of-sight / point-in-volume queries. A segment
/// within this distance of tangency is treated as *blocked* (touching =
/// blocked), so float noise never flips a genuine graze to "clear".
const TOUCH_EPS: f32 = 1e-4;

/// XZ radius of a mover's collision disc. There is no engine-wide body-radius
/// constant, so this is defined locally: 0.5 yd is a half-yard skin, small
/// relative to obstacle and arena scale, enough to keep a unit's center from
/// visually clipping a wall.
pub const MOVER_RADIUS: f32 = 0.5;

/// Margin by which a resolved slide position is pushed strictly outside a
/// footprint, so the returned point never re-tests as penetrating.
const PUSH_OUT_EPS: f32 = 1e-3;

/// A convex obstacle volume declared by a map. Both primitives are finite in
/// every axis. Volumes are stored in a `Vec` and iterated in declaration order
/// (never a hashed collection) to preserve determinism.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ObstacleVolume {
    /// A vertical, axis-aligned finite cylinder: a circle of `radius` centered
    /// at `center_xz` in the XZ plane, spanning `y ∈ [base_y, base_y + height]`.
    Cylinder {
        center_xz: Vec2,
        radius: f32,
        base_y: f32,
        height: f32,
    },
    /// A closed axis-aligned bounding box spanning `[min, max]` on every axis.
    Aabb { min: Vec3, max: Vec3 },
}

impl ObstacleVolume {
    /// Whether the mover's `y` (a ground unit at `y ≈ 1.0`) falls within this
    /// volume's `y` span. Movement collision only applies when this is true.
    fn y_span_contains(&self, y: f32) -> bool {
        match *self {
            ObstacleVolume::Cylinder { base_y, height, .. } => {
                y >= base_y - TOUCH_EPS && y <= base_y + height + TOUCH_EPS
            }
            ObstacleVolume::Aabb { min, max } => y >= min.y - TOUCH_EPS && y <= max.y + TOUCH_EPS,
        }
    }
}

/// Whether `p` lies inside (or on the closed boundary of) the volume.
///
/// Inclusive on every face (touching = inside), matching the line-of-sight
/// edge policy.
pub fn contains_point(volume: &ObstacleVolume, p: Vec3) -> bool {
    match *volume {
        ObstacleVolume::Cylinder {
            center_xz,
            radius,
            base_y,
            height,
        } => {
            let radial = Vec2::new(p.x - center_xz.x, p.z - center_xz.y).length();
            radial <= radius + TOUCH_EPS
                && p.y >= base_y - TOUCH_EPS
                && p.y <= base_y + height + TOUCH_EPS
        }
        ObstacleVolume::Aabb { min, max } => {
            p.x >= min.x - TOUCH_EPS
                && p.x <= max.x + TOUCH_EPS
                && p.y >= min.y - TOUCH_EPS
                && p.y <= max.y + TOUCH_EPS
                && p.z >= min.z - TOUCH_EPS
                && p.z <= max.z + TOUCH_EPS
        }
    }
}

/// Whether the segment `a → b` intersects the volume in true 3D.
///
/// Cylinder checks are finite in `y` (the segment must overlap
/// `[base_y, base_y + height]` where its XZ projection crosses the circle);
/// boxes use the closed-interval slab method. Tangential contact counts as an
/// intersection (touching = blocked). A zero-length segment reduces to
/// [`contains_point`].
pub fn segment_intersects(volume: &ObstacleVolume, a: Vec3, b: Vec3) -> bool {
    // A zero-length segment reduces to a point-in-volume test (zero-length
    // sight policy). Guard here so the interval math below never divides by a
    // zero-length direction.
    if a.distance_squared(b) <= 1e-12 {
        return contains_point(volume, a);
    }
    let d = b - a;
    match *volume {
        ObstacleVolume::Cylinder {
            center_xz,
            radius,
            base_y,
            height,
        } => {
            // Interval of `t ∈ [0,1]` where the XZ projection is within the
            // circle, intersected with the interval where `y` is within the
            // finite span. The circle radius is inflated by TOUCH_EPS so a true
            // tangent lands strictly inside the solved interval (touching =
            // blocked).
            let reff = radius + TOUCH_EPS;
            let fx = a.x - center_xz.x;
            let fz = a.z - center_xz.y;
            let a2 = d.x * d.x + d.z * d.z;
            let (xz_lo, xz_hi) = if a2 <= 1e-12 {
                // No XZ motion: either the whole segment projects inside the
                // circle, or none of it does.
                if fx * fx + fz * fz <= reff * reff {
                    (f32::NEG_INFINITY, f32::INFINITY)
                } else {
                    return false;
                }
            } else {
                let b2 = 2.0 * (fx * d.x + fz * d.z);
                let c2 = fx * fx + fz * fz - reff * reff;
                let disc = b2 * b2 - 4.0 * a2 * c2;
                if disc < 0.0 {
                    return false;
                }
                let sq = disc.sqrt();
                ((-b2 - sq) / (2.0 * a2), (-b2 + sq) / (2.0 * a2))
            };

            let top = base_y + height;
            let (y_lo, y_hi) = if d.y.abs() <= 1e-12 {
                if a.y >= base_y - TOUCH_EPS && a.y <= top + TOUCH_EPS {
                    (f32::NEG_INFINITY, f32::INFINITY)
                } else {
                    return false;
                }
            } else {
                let t0 = (base_y - TOUCH_EPS - a.y) / d.y;
                let t1 = (top + TOUCH_EPS - a.y) / d.y;
                (t0.min(t1), t0.max(t1))
            };

            let lo = xz_lo.max(y_lo).max(0.0);
            let hi = xz_hi.min(y_hi).min(1.0);
            lo <= hi
        }
        ObstacleVolume::Aabb { min, max } => {
            // Closed-interval slab method: intersect the per-axis `t` ranges
            // where the segment is within `[min, max]` (each inflated by
            // TOUCH_EPS) with `[0,1]`.
            let mut lo = 0.0_f32;
            let mut hi = 1.0_f32;
            let axes = [
                (a.x, d.x, min.x, max.x),
                (a.y, d.y, min.y, max.y),
                (a.z, d.z, min.z, max.z),
            ];
            for (a_c, d_c, mn, mx) in axes {
                if d_c.abs() <= 1e-12 {
                    if a_c < mn - TOUCH_EPS || a_c > mx + TOUCH_EPS {
                        return false;
                    }
                } else {
                    let t0 = (mn - TOUCH_EPS - a_c) / d_c;
                    let t1 = (mx + TOUCH_EPS - a_c) / d_c;
                    lo = lo.max(t0.min(t1));
                    hi = hi.min(t0.max(t1));
                }
            }
            lo <= hi
        }
    }
}

/// Whether an unobstructed line of sight runs from `from` to `to`: true iff no
/// volume intersects the segment. Volumes are walked in slice order.
pub fn has_line_of_sight(obstacles: &[ObstacleVolume], from: Vec3, to: Vec3) -> bool {
    !obstacles
        .iter()
        .any(|volume| segment_intersects(volume, from, to))
}

/// Resolve a proposed step to a collision-free position.
///
/// `pos` is the mover's current position; `desired` is the proposed *new*
/// position (`pos + delta`). If `desired` does not enter any (Y-overlapping)
/// volume it is returned unchanged. Otherwise the blocked step is projected
/// along the obstacle surface tangent, preserving as much lateral progress as
/// possible. The result is never inside a volume; if no tangential slide is
/// valid (the mover is boxed in), the original `pos` is returned rather than
/// clipping or oscillating.
pub fn resolve_movement(obstacles: &[ObstacleVolume], pos: Vec3, desired: Vec3) -> Vec3 {
    if obstacles.is_empty() {
        return desired;
    }
    let mover_y = pos.y;
    let pos_xz = Vec2::new(pos.x, pos.z);
    let desired_xz = Vec2::new(desired.x, desired.z);

    // The blocking volume `desired` first enters (slice order → deterministic).
    let Some(blocker) = obstacles
        .iter()
        .find(|v| penetrates_footprint(v, desired_xz, mover_y))
    else {
        return desired; // the step is already clear
    };

    let slid_xz = slide_against(blocker, pos_xz, desired_xz);
    // A valid slide must clear *every* volume, not just the one we slid off; if
    // the tangent carries us into another volume the mover is boxed in — stay
    // put rather than clip.
    if obstacles
        .iter()
        .any(|v| penetrates_footprint(v, slid_xz, mover_y))
    {
        return pos;
    }
    Vec3::new(slid_xz.x, desired.y, slid_xz.y)
}

/// Whether the mover's XZ collision disc strictly penetrates the volume's
/// footprint (only when the mover's `y` overlaps the volume's span). Strict, so
/// a mover resting flush on the expanded boundary is *not* penetrating
/// (touching = allowed for movement).
fn penetrates_footprint(volume: &ObstacleVolume, p_xz: Vec2, mover_y: f32) -> bool {
    if !volume.y_span_contains(mover_y) {
        return false;
    }
    match *volume {
        ObstacleVolume::Cylinder {
            center_xz, radius, ..
        } => p_xz.distance(center_xz) < radius + MOVER_RADIUS - TOUCH_EPS,
        ObstacleVolume::Aabb { min, max } => {
            p_xz.x > min.x - MOVER_RADIUS + TOUCH_EPS
                && p_xz.x < max.x + MOVER_RADIUS - TOUCH_EPS
                && p_xz.y > min.z - MOVER_RADIUS + TOUCH_EPS
                && p_xz.y < max.z + MOVER_RADIUS - TOUCH_EPS
        }
    }
}

/// Project a penetrating `desired` step into a tangential slide against a
/// single volume, guaranteeing the result sits strictly outside that volume's
/// footprint (inflated by [`MOVER_RADIUS`]). The component of the step normal
/// to the surface is removed (keeping any outward component), preserving as
/// much lateral progress as the surface allows.
fn slide_against(volume: &ObstacleVolume, pos_xz: Vec2, desired_xz: Vec2) -> Vec2 {
    match *volume {
        ObstacleVolume::Cylinder {
            center_xz, radius, ..
        } => {
            let eff = radius + MOVER_RADIUS;
            // Contact normal points outward from the center toward the side the
            // mover came from; fall back to the desired side, then +X, if
            // either reference coincides with the center.
            let mut normal = pos_xz - center_xz;
            if normal.length_squared() <= 1e-12 {
                normal = desired_xz - center_xz;
            }
            let normal = normal.normalize_or(Vec2::X);

            let delta = desired_xz - pos_xz;
            // Remove only the inward component (keep any outward motion).
            let tangential = delta - normal * delta.dot(normal).min(0.0);
            let mut slid = pos_xz + tangential;

            if slid.distance(center_xz) < eff {
                let out = (slid - center_xz).normalize_or(normal);
                slid = center_xz + out * (eff + PUSH_OUT_EPS);
            }
            slid
        }
        ObstacleVolume::Aabb { min, max } => {
            let (min_x, max_x) = (min.x - MOVER_RADIUS, max.x + MOVER_RADIUS);
            let (min_z, max_z) = (min.z - MOVER_RADIUS, max.z + MOVER_RADIUS);
            let center_x = 0.5 * (min.x + max.x);
            let center_z = 0.5 * (min.z + max.z);

            let outside_x = pos_xz.x <= min_x || pos_xz.x >= max_x;
            let outside_z = pos_xz.y <= min_z || pos_xz.y >= max_z;

            // Clamp along the face the mover is approaching: prefer the axis the
            // mover is already outside on; on a corner (both) or an interior
            // start (neither), clamp the axis of least penetration.
            let clamp_x = if outside_x && !outside_z {
                true
            } else if outside_z && !outside_x {
                false
            } else {
                let pen_x = (desired_xz.x - min_x).min(max_x - desired_xz.x);
                let pen_z = (desired_xz.y - min_z).min(max_z - desired_xz.y);
                pen_x <= pen_z
            };

            if clamp_x {
                let face = if pos_xz.x <= center_x {
                    min_x - PUSH_OUT_EPS
                } else {
                    max_x + PUSH_OUT_EPS
                };
                Vec2::new(face, desired_xz.y)
            } else {
                let face = if pos_xz.y <= center_z {
                    min_z - PUSH_OUT_EPS
                } else {
                    max_z + PUSH_OUT_EPS
                };
                Vec2::new(desired_xz.x, face)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cylinder(cx: f32, cz: f32, r: f32, base_y: f32, height: f32) -> ObstacleVolume {
        ObstacleVolume::Cylinder {
            center_xz: Vec2::new(cx, cz),
            radius: r,
            base_y,
            height,
        }
    }

    fn footprint_dist(v: &ObstacleVolume, p: Vec3) -> f32 {
        match *v {
            ObstacleVolume::Cylinder { center_xz, .. } => {
                Vec2::new(p.x - center_xz.x, p.z - center_xz.y).length()
            }
            _ => unreachable!("test only measures cylinder footprint distance"),
        }
    }

    /// Scenario 1: a segment passing clearly over a pillar's top has sight —
    /// elevation grants line of sight even though the XZ projection crosses the
    /// circle.
    #[test]
    fn segment_over_pillar_top_has_sight() {
        let pillar = cylinder(0.0, 0.0, 5.0, 0.0, 3.0); // top at y = 3
        let from = Vec3::new(-10.0, 5.0, 0.0);
        let to = Vec3::new(10.0, 5.0, 0.0);
        assert!(!segment_intersects(&pillar, from, to), "y=5 clears the y∈[0,3] pillar");
        assert!(has_line_of_sight(&[pillar], from, to));
    }

    /// Scenario 2: a segment threading through a cylinder's side is blocked.
    #[test]
    fn segment_through_cylinder_side_blocked() {
        let pillar = cylinder(0.0, 0.0, 5.0, 0.0, 3.0);
        let from = Vec3::new(-10.0, 1.0, 0.0);
        let to = Vec3::new(10.0, 1.0, 0.0);
        assert!(segment_intersects(&pillar, from, to));
        assert!(!has_line_of_sight(&[pillar], from, to));
    }

    /// Scenario 3: a segment exactly tangent to a cylinder's edge is blocked
    /// (touching = blocked), and the geometry is chosen so the discriminant is
    /// an exact zero — the policy is deterministic, not float-fuzzy.
    #[test]
    fn segment_grazing_tangent_blocked_deterministically() {
        let pillar = cylinder(0.0, 0.0, 5.0, 0.0, 10.0);
        // Line z = 5 is tangent to the r=5 circle at (0, 5).
        let from = Vec3::new(-10.0, 1.0, 5.0);
        let to = Vec3::new(10.0, 1.0, 5.0);
        assert!(segment_intersects(&pillar, from, to), "tangent contact must count as blocked");
    }

    /// Scenario 4: a segment ending inside a volume is blocked.
    #[test]
    fn segment_ending_inside_volume_blocked() {
        let pillar = cylinder(0.0, 0.0, 5.0, 0.0, 10.0);
        let from = Vec3::new(-10.0, 1.0, 0.0);
        let to = Vec3::new(0.0, 1.0, 0.0); // endpoint at the center
        assert!(segment_intersects(&pillar, from, to));
    }

    /// Scenario 5: a vertical segment vs an elevated platform box — from below
    /// the platform it is blocked; starting atop the surface (body center above
    /// it) it is clear.
    #[test]
    fn vertical_segment_vs_platform_box() {
        let platform = ObstacleVolume::Aabb {
            min: Vec3::new(-5.0, 5.0, -5.0),
            max: Vec3::new(5.0, 7.0, 5.0),
        };
        // From the ground up through the platform.
        let below_from = Vec3::new(0.0, 0.0, 0.0);
        let below_to = Vec3::new(0.0, 10.0, 0.0);
        assert!(segment_intersects(&platform, below_from, below_to), "rising through y∈[5,7] is blocked");

        // Standing atop the platform (body center above the y=7 surface),
        // looking up/away is clear.
        let atop_from = Vec3::new(0.0, 8.0, 0.0);
        let atop_to = Vec3::new(0.0, 12.0, 0.0);
        assert!(!segment_intersects(&platform, atop_from, atop_to));
        assert!(has_line_of_sight(&[platform], atop_from, atop_to));
    }

    /// Scenario 6: a step driven into a cylinder slides tangentially — it gains
    /// lateral progress and does not advance into the pillar, and the result is
    /// outside the footprint.
    #[test]
    fn resolve_movement_slides_along_cylinder() {
        let pillar = cylinder(0.0, 0.0, 5.0, 0.0, 10.0);
        let pos = Vec3::new(-10.0, 1.0, 0.0);
        let desired = Vec3::new(0.0, 1.0, 3.0); // heads +X into the pillar with a +Z lateral bias
        let out = resolve_movement(&[pillar], pos, desired);

        assert!(out.is_finite(), "no NaN/inf");
        assert_ne!(out, desired, "the blocked step must be modified");
        assert!(
            footprint_dist(&pillar, out) >= 5.0,
            "resolved position {:?} must be outside the r=5 footprint",
            out
        );
        assert!(out.z > pos.z, "lateral (Z) progress must be preserved, got z={}", out.z);
    }

    /// Scenario 7: a mover boxed in by geometry (every candidate lands inside a
    /// volume) returns its original position — no NaN, no escape through a
    /// volume.
    #[test]
    fn resolve_movement_fully_enclosed_stays_put() {
        // Inner pillar the mover sits inside, wrapped by a huge outer cylinder
        // so any slide off the inner surface is still inside the outer volume.
        let inner = cylinder(0.0, 0.0, 5.0, 0.0, 20.0);
        let outer = cylinder(0.0, 0.0, 30.0, 0.0, 20.0);
        let pos = Vec3::new(0.0, 1.0, 0.0);
        let desired = Vec3::new(2.0, 1.0, 0.0);
        let out = resolve_movement(&[inner, outer], pos, desired);

        assert!(out.is_finite(), "no NaN/inf");
        assert!(out.distance(pos) < 1e-3, "enclosed mover must stay put, got {:?}", out);
    }

    /// Scenario 8: a zero-length segment has sight unless the point is inside a
    /// volume.
    #[test]
    fn zero_length_segment_sight_policy() {
        let pillar = cylinder(0.0, 0.0, 5.0, 0.0, 10.0);
        let outside = Vec3::new(20.0, 1.0, 0.0);
        let inside = Vec3::new(0.0, 1.0, 0.0);

        assert!(has_line_of_sight(&[pillar], outside, outside), "a point outside sees itself");
        assert!(!has_line_of_sight(&[pillar], inside, inside), "a point inside a volume is blocked");
        assert!(has_line_of_sight(&[], outside, outside), "no obstacles → always sight");
    }

    /// Scenario 9: with no obstacles, `resolve_movement` returns `desired`
    /// unchanged.
    #[test]
    fn resolve_movement_no_obstacles_is_identity() {
        let pos = Vec3::new(1.0, 1.0, 2.0);
        let desired = Vec3::new(4.0, 1.0, 6.0);
        assert_eq!(resolve_movement(&[], pos, desired), desired);
    }
}
