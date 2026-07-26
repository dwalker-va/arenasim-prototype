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
//! seeded runs, so nothing here may iterate a `HashMap`/`HashSet`.
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

/// Height (y) at which line-of-sight segment endpoints sit — entity center-mass
/// height. Shared by the cast/heal/auto LoS gates and the movement scorer's
/// sight probes so every occlusion test agrees.
pub const EYE_HEIGHT: f32 = 1.0;

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
    /// A vertical prism over a **regular** `sides`-gon: the polygon inscribed in
    /// a circle of `circumradius` centered at `center_xz`, with vertex 0 at
    /// `rotation` radians, spanning `y ∈ [base_y, base_y + height]`.
    ///
    /// Parameterized (center/circumradius/sides/rotation) rather than as a vertex
    /// list so [`ObstacleVolume`] stays `Copy` and allocation-free — the
    /// determinism guarantee above depends on volumes being plain values walked
    /// in slice order.
    ///
    /// Treated as the intersection of `sides` half-planes whose outward normals
    /// sit at the edge midpoint angles, each at distance
    /// [`prism_apothem`] from the center. Every predicate below reuses that
    /// half-plane form, which makes the prism math a direct generalization of the
    /// [`ObstacleVolume::Aabb`] slab method (a box is four half-planes) — so the
    /// touching/graze edge policies match the box branch exactly.
    Prism {
        center_xz: Vec2,
        circumradius: f32,
        sides: u32,
        rotation: f32,
        base_y: f32,
        height: f32,
    },
}

/// Distance from a regular polygon's center to its edges (the inradius), given
/// the circumradius and side count. `sides < 3` degenerates to `0.0`.
pub fn prism_apothem(circumradius: f32, sides: u32) -> f32 {
    if sides < 3 {
        return 0.0;
    }
    circumradius * (std::f32::consts::PI / sides as f32).cos()
}

/// Outward unit normal of edge `i` of a regular `sides`-gon at `rotation`.
///
/// Vertex `i` sits at angle `rotation + i * TAU / sides`, so edge `i` (spanning
/// vertices `i` and `i+1`) faces the midpoint angle, offset by half a step.
fn prism_edge_normal(i: u32, sides: u32, rotation: f32) -> Vec2 {
    let ang = rotation + (i as f32 + 0.5) * std::f32::consts::TAU / sides as f32;
    Vec2::new(ang.cos(), ang.sin())
}

/// Vertex `i` of a regular `sides`-gon of `circumradius` at `rotation`.
fn prism_vertex(i: u32, sides: u32, rotation: f32, circumradius: f32) -> Vec2 {
    let ang = rotation + i as f32 * std::f32::consts::TAU / sides as f32;
    Vec2::new(ang.cos(), ang.sin()) * circumradius
}

/// World-space XZ vertices of a regular prism's cross-section, in edge order.
///
/// The single source of truth for a prism's outline: the collision predicates
/// above, the 3D pillar mesh, and the top-down schematic all derive from this, so
/// what the sim blocks and what the player sees agree by construction rather than
/// by two hand-kept-in-sync formulas. Allocates, so it belongs to setup/render
/// paths — the per-frame predicates use the half-plane form instead.
pub fn prism_vertices_world(
    center: Vec2,
    circumradius: f32,
    sides: u32,
    rotation: f32,
) -> Vec<Vec2> {
    (0..sides.max(3))
        .map(|i| center + prism_vertex(i, sides.max(3), rotation, circumradius))
        .collect()
}

/// Whether `rel` (a point relative to the prism center) satisfies every edge
/// half-plane at the given `limit` distance. `strict` selects `<` over `<=`,
/// which is how the inclusive line-of-sight policy (touching = inside) and the
/// strict movement policy (touching = allowed) share one routine.
fn prism_half_planes_contain(
    rel: Vec2,
    limit: f32,
    sides: u32,
    rotation: f32,
    strict: bool,
) -> bool {
    if sides < 3 {
        return false;
    }
    (0..sides).all(|i| {
        let d = rel.dot(prism_edge_normal(i, sides, rotation));
        if strict {
            d < limit
        } else {
            d <= limit
        }
    })
}

/// Clip the parameter interval `[lo, hi]` of the segment `from + t * d` against
/// a regular prism's edge half-planes, each at `limit` from the center.
///
/// Returns `false` if the segment lies wholly outside some half-plane (a
/// separating edge), in which case `lo`/`hi` are left in an unspecified state.
/// `flush_clear` picks the parallel-segment edge policy: `false` treats a
/// segment flush on the boundary as inside (line-of-sight, touching = blocked),
/// `true` treats it as outside (movement, touching = allowed) — mirroring the
/// `Aabb` branches of [`segment_intersects`] and [`footprint_sweep_entry`].
fn prism_clip_interval(
    rel_from: Vec2,
    d: Vec2,
    limit: f32,
    sides: u32,
    rotation: f32,
    lo: &mut f32,
    hi: &mut f32,
    flush_clear: bool,
) -> bool {
    if sides < 3 {
        return false;
    }
    for i in 0..sides {
        let n = prism_edge_normal(i, sides, rotation);
        let dn = d.dot(n);
        // Remaining slack to this edge from the segment start.
        let c = limit - rel_from.dot(n);
        if dn.abs() <= 1e-12 {
            // Parallel to this edge: the whole segment is on one side of it.
            let outside = if flush_clear { c <= 0.0 } else { c < 0.0 };
            if outside {
                return false;
            }
        } else if dn > 0.0 {
            *hi = hi.min(c / dn);
        } else {
            *lo = lo.max(c / dn);
        }
    }
    true
}

impl ObstacleVolume {
    /// The volume's XZ footprint reduced to a bounding disc: `(center, radius)`
    /// where the radius encloses the whole footprint.
    ///
    /// A deliberately coarse summary, for reasoning about an obstacle as "a blob
    /// at a place" — e.g. picking a standing spot in its shadow. Callers must
    /// verify the result against the exact predicates ([`has_line_of_sight`],
    /// [`position_blocked`]) rather than trusting the disc, since it over-covers
    /// every non-circular shape.
    pub fn footprint_disc(&self) -> (Vec2, f32) {
        match *self {
            ObstacleVolume::Cylinder {
                center_xz, radius, ..
            } => (center_xz, radius),
            ObstacleVolume::Prism {
                center_xz,
                circumradius,
                ..
            } => (center_xz, circumradius),
            ObstacleVolume::Aabb { min, max } => {
                let center = Vec2::new((min.x + max.x) * 0.5, (min.z + max.z) * 0.5);
                let half = Vec2::new((max.x - min.x) * 0.5, (max.z - min.z) * 0.5);
                (center, half.length())
            }
        }
    }

    /// Whether the mover's `y` (a ground unit at `y ≈ 1.0`) falls within this
    /// volume's `y` span. Movement collision only applies when this is true.
    fn y_span_contains(&self, y: f32) -> bool {
        match *self {
            ObstacleVolume::Cylinder { base_y, height, .. }
            | ObstacleVolume::Prism { base_y, height, .. } => {
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
        ObstacleVolume::Prism {
            center_xz,
            circumradius,
            sides,
            rotation,
            base_y,
            height,
        } => {
            p.y >= base_y - TOUCH_EPS
                && p.y <= base_y + height + TOUCH_EPS
                && prism_half_planes_contain(
                    Vec2::new(p.x - center_xz.x, p.z - center_xz.y),
                    prism_apothem(circumradius, sides) + TOUCH_EPS,
                    sides,
                    rotation,
                    false,
                )
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
        ObstacleVolume::Prism {
            center_xz,
            circumradius,
            sides,
            rotation,
            base_y,
            height,
        } => {
            // XZ half-plane clip (edges inflated by TOUCH_EPS so a true tangent
            // lands inside the interval: touching = blocked), intersected with
            // the finite y span — the same two-stage structure as the cylinder.
            let mut lo = 0.0_f32;
            let mut hi = 1.0_f32;
            if !prism_clip_interval(
                Vec2::new(a.x - center_xz.x, a.z - center_xz.y),
                Vec2::new(d.x, d.z),
                prism_apothem(circumradius, sides) + TOUCH_EPS,
                sides,
                rotation,
                &mut lo,
                &mut hi,
                false,
            ) {
                return false;
            }

            let top = base_y + height;
            if d.y.abs() <= 1e-12 {
                if a.y < base_y - TOUCH_EPS || a.y > top + TOUCH_EPS {
                    return false;
                }
            } else {
                let t0 = (base_y - TOUCH_EPS - a.y) / d.y;
                let t1 = (top + TOUCH_EPS - a.y) / d.y;
                lo = lo.max(t0.min(t1));
                hi = hi.min(t0.max(t1));
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

/// Whether a mover standing at `p` would penetrate any obstacle's footprint —
/// the movement-blocking test the scorer's obstacle mask uses to reject a
/// candidate step that walks into a wall. Consistent with [`resolve_movement`]:
/// same strict (touching = allowed) footprint test, honoring [`MOVER_RADIUS`]
/// and each volume's `y` span, walked in slice order. Empty obstacle lists
/// (e.g. BasicArena) always return `false`, so the mask is a no-op there.
pub fn position_blocked(obstacles: &[ObstacleVolume], p: Vec3) -> bool {
    let p_xz = Vec2::new(p.x, p.z);
    obstacles
        .iter()
        .any(|v| penetrates_footprint(v, p_xz, p.y))
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
        // Half-planes pushed out by MOVER_RADIUS. Like the box branch, this
        // squares off the true rounded Minkowski corners, so it over-blocks by
        // at most a sliver at each vertex — the same conservative approximation,
        // and `resolve_movement` stays the no-clip backstop either way.
        ObstacleVolume::Prism {
            center_xz,
            circumradius,
            sides,
            rotation,
            ..
        } => prism_half_planes_contain(
            p_xz - center_xz,
            prism_apothem(circumradius, sides) + MOVER_RADIUS - TOUCH_EPS,
            sides,
            rotation,
            true,
        ),
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
        ObstacleVolume::Prism {
            center_xz,
            circumradius,
            sides,
            rotation,
            ..
        } => {
            let limit = prism_apothem(circumradius, sides) + MOVER_RADIUS;
            // Choose the exit face the same way the box branch chooses its axis:
            // prefer an edge the mover is *already outside of* (the side it came
            // from), and among the candidates take the least-penetrated edge —
            // the nearest way out. Fixed edge order gives a deterministic
            // tie-break.
            let mut best: Option<(f32, Vec2)> = None;
            let mut best_from_outside: Option<(f32, Vec2)> = None;
            for i in 0..sides {
                let n = prism_edge_normal(i, sides, rotation);
                // Signed distance past the inflated edge; negative = penetrating.
                let s_desired = (desired_xz - center_xz).dot(n) - limit;
                let s_pos = (pos_xz - center_xz).dot(n) - limit;
                if best.is_none_or(|(b, _)| s_desired > b) {
                    best = Some((s_desired, n));
                }
                if s_pos >= 0.0 && best_from_outside.is_none_or(|(b, _)| s_desired > b) {
                    best_from_outside = Some((s_desired, n));
                }
            }

            // `slide_against` is only reached for a penetrating `desired`, so
            // every `s` here is negative and the correction pushes outward.
            // Removing exactly the normal overshoot keeps the full tangential
            // component, which is what makes the slide preserve lateral progress.
            match best_from_outside.or(best) {
                Some((s, n)) => desired_xz + n * (PUSH_OUT_EPS - s),
                // Degenerate prism (`sides < 3`, rejected by config validation).
                None => pos_xz,
            }
        }
    }
}

/// Tangent-steering angular slack (radians). When the two cylinder tangents make
/// near-equal progress toward the goal — the goal sits almost directly behind the
/// obstacle center — the side choice is a coin flip that float noise could flip
/// frame to frame. Within this band we take the deterministic default (the
/// `+alpha` / left tangent) so the mover commits to one side instead of jittering
/// across the center line.
const STEER_TIE_EPS: f32 = 1e-4;

/// Entry parameter `t ∈ [0,1]` at which a mover's swept disc first penetrates a
/// volume's MOVER_RADIUS-inflated footprint along the segment `from → to`, or
/// `None` if the path stays clear. This is the *movement* footprint test
/// (touching = allowed, radius inflated by [`MOVER_RADIUS`]) — the sweep analog
/// of [`penetrates_footprint`] — NOT the eye-height line-of-sight test. Used to
/// decide whether a goal-directed mover can walk straight at its goal, and to
/// pick the nearest obstacle in the way. Honors each volume's `y` span, so an
/// elevated platform never blocks a ground path.
fn footprint_sweep_entry(volume: &ObstacleVolume, from: Vec2, to: Vec2, mover_y: f32) -> Option<f32> {
    if !volume.y_span_contains(mover_y) {
        return None;
    }
    match *volume {
        ObstacleVolume::Cylinder {
            center_xz, radius, ..
        } => {
            let eff = radius + MOVER_RADIUS;
            let d = to - from;
            let f = from - center_xz;
            let a2 = d.dot(d);
            if a2 <= 1e-12 {
                // No motion: blocked iff the start point is strictly inside the
                // inflated circle (touching the skin is allowed).
                return if f.length_squared() < eff * eff - TOUCH_EPS {
                    Some(0.0)
                } else {
                    None
                };
            }
            let b2 = 2.0 * f.dot(d);
            let c2 = f.dot(f) - eff * eff;
            let disc = b2 * b2 - 4.0 * a2 * c2;
            // disc <= 0 ⇒ the segment misses or is exactly tangent; a tangent
            // graze is "touching = allowed" for movement, so treat it as clear.
            if disc <= 0.0 {
                return None;
            }
            let sq = disc.sqrt();
            let t0 = (-b2 - sq) / (2.0 * a2);
            let t1 = (-b2 + sq) / (2.0 * a2);
            if t1 < 0.0 || t0 > 1.0 {
                return None;
            }
            Some(t0.max(0.0))
        }
        ObstacleVolume::Aabb { min, max } => {
            let (mnx, mxx) = (min.x - MOVER_RADIUS, max.x + MOVER_RADIUS);
            let (mnz, mxz) = (min.z - MOVER_RADIUS, max.z + MOVER_RADIUS);
            let d = to - from;
            let mut lo = 0.0_f32;
            let mut hi = 1.0_f32;
            // Slab method in XZ against the inflated box (Vec2.y carries world Z).
            for (a_c, d_c, mn, mx) in [(from.x, d.x, mnx, mxx), (from.y, d.y, mnz, mxz)] {
                if d_c.abs() <= 1e-12 {
                    // Parallel to this slab: outside (or flush on) it ⇒ clear.
                    if a_c <= mn || a_c >= mx {
                        return None;
                    }
                } else {
                    let t0 = (mn - a_c) / d_c;
                    let t1 = (mx - a_c) / d_c;
                    lo = lo.max(t0.min(t1));
                    hi = hi.min(t0.max(t1));
                }
            }
            // Strict overlap: a single-point graze (lo == hi) is touching = clear.
            if lo < hi {
                Some(lo.max(0.0))
            } else {
                None
            }
        }
        ObstacleVolume::Prism {
            center_xz,
            circumradius,
            sides,
            rotation,
            ..
        } => {
            let mut lo = 0.0_f32;
            let mut hi = 1.0_f32;
            if !prism_clip_interval(
                from - center_xz,
                to - from,
                prism_apothem(circumradius, sides) + MOVER_RADIUS,
                sides,
                rotation,
                &mut lo,
                &mut hi,
                true,
            ) {
                return None;
            }
            // Strict overlap: a single-point graze (lo == hi) is touching = clear.
            if lo < hi {
                Some(lo.max(0.0))
            } else {
                None
            }
        }
    }
}

/// Steer a goal-directed mover around a blocking obstacle instead of oozing along
/// its surface.
///
/// Returns the unit XZ direction the mover should travel *this frame*:
/// - `None` — the straight segment `from → goal` is clear of every obstacle (or
///   there are no obstacles), so the caller should head directly at the goal.
///   Returning `None` rather than the direct vector lets the caller reuse its own
///   existing normalization, keeping obstacle-free maps byte-identical.
/// - `Some(dir)` — the direct line is blocked; `dir` aims at the tangent point of
///   the nearest blocking obstacle on the side that makes better progress toward
///   the goal, so the mover travels at full speed along a path that clears the
///   obstacle and resumes direct pursuit once the line opens up.
///
/// Purely geometric and deterministic (plain `f32`, obstacles walked in slice
/// order, deterministic side tie-break). The [`resolve_movement`] collision
/// resolver remains the final no-clip guarantee downstream; steering merely aims
/// the step along a clear tangent so the resolver rarely has to bite.
///
/// **Side commitment is emergent, not stored.** The "better progress" tangent is
/// self-reinforcing: once the mover steps off the center line toward one side,
/// that side's tangent keeps winning the progress comparison, so the choice holds
/// without any per-frame committed-side state. The only ambiguous instant — the
/// goal exactly behind the obstacle center — is resolved by [`STEER_TIE_EPS`] to a
/// fixed default, so the selection is a stable function of geometry that cannot
/// flip-flop. (The unit tests simulate the step loop and assert convergence,
/// which would fail on any oscillation.)
pub fn steer_toward_goal(
    obstacles: &[ObstacleVolume],
    from: Vec2,
    goal: Vec2,
    mover_y: f32,
) -> Option<Vec2> {
    if obstacles.is_empty() {
        return None;
    }
    let to_goal = goal - from;
    if to_goal.length_squared() <= 1e-12 {
        return None;
    }

    // Nearest blocker along the segment: smallest entry parameter; ties (and the
    // empty case) resolved by slice order — the first-declared volume wins.
    let mut best: Option<(f32, usize)> = None;
    for (i, v) in obstacles.iter().enumerate() {
        if let Some(t) = footprint_sweep_entry(v, from, goal, mover_y) {
            match best {
                Some((bt, _)) if t >= bt => {}
                _ => best = Some((t, i)),
            }
        }
    }
    let (_, idx) = best?; // path clear ⇒ go direct

    Some(match obstacles[idx] {
        ObstacleVolume::Cylinder {
            center_xz, radius, ..
        } => steer_around_cylinder(center_xz, radius, from, goal),
        ObstacleVolume::Aabb { min, max } => steer_around_box(min, max, from, goal, mover_y),
        ObstacleVolume::Prism {
            center_xz,
            circumradius,
            sides,
            rotation,
            ..
        } => steer_around_prism(center_xz, circumradius, sides, rotation, from, goal),
    })
}

/// Unit direction toward the better-progress tangent point of a cylinder (radius
/// inflated by [`MOVER_RADIUS`]). See [`steer_toward_goal`].
fn steer_around_cylinder(center: Vec2, radius: f32, from: Vec2, goal: Vec2) -> Vec2 {
    let eff = radius + MOVER_RADIUS;
    let goal_dir = (goal - from).normalize_or_zero();
    let d = center - from;
    let dist = d.length();

    if dist <= eff {
        // Already at/inside the collision skin (a hugging chase): there are no
        // external tangents — peel off perpendicular to the center direction on
        // whichever side heads more toward the goal.
        let dn = if dist > 1e-6 { d / dist } else { goal_dir };
        let dn = dn.normalize_or(Vec2::X);
        let perp = Vec2::new(-dn.y, dn.x);
        let s = if goal_dir.dot(perp) >= 0.0 { 1.0 } else { -1.0 };
        return perp * s;
    }

    let dn = d / dist;
    // Half-angle subtended by the inflated circle from `from`.
    let alpha = (eff / dist).clamp(-1.0, 1.0).asin();
    let (sin_a, cos_a) = alpha.sin_cos();
    // Rotate the center direction by ±alpha to get the two tangent directions.
    let t_left = Vec2::new(dn.x * cos_a - dn.y * sin_a, dn.x * sin_a + dn.y * cos_a);
    let t_right = Vec2::new(dn.x * cos_a + dn.y * sin_a, -dn.x * sin_a + dn.y * cos_a);

    let dot_l = t_left.dot(goal_dir);
    let dot_r = t_right.dot(goal_dir);
    if (dot_l - dot_r).abs() < STEER_TIE_EPS {
        // Goal ~directly behind the center: deterministic default (left tangent).
        t_left
    } else if dot_l >= dot_r {
        t_left
    } else {
        t_right
    }
}

/// Unit direction toward the best visible (silhouette) corner of a box (inflated
/// by [`MOVER_RADIUS`]) — the box analog of a tangent point. Boxes are the stub
/// map's primitive; this keeps the mover rounding the correct side without the
/// full rounded-rectangle tangent geometry. See [`steer_toward_goal`].
fn steer_around_box(min: Vec3, max: Vec3, from: Vec2, goal: Vec2, mover_y: f32) -> Vec2 {
    let goal_dir = (goal - from).normalize_or_zero();
    let (mnx, mxx) = (min.x - MOVER_RADIUS, max.x + MOVER_RADIUS);
    let (mnz, mxz) = (min.z - MOVER_RADIUS, max.z + MOVER_RADIUS);
    let corners = [
        Vec2::new(mnx, mnz),
        Vec2::new(mnx, mxz),
        Vec2::new(mxx, mnz),
        Vec2::new(mxx, mxz),
    ];
    let mut best = goal_dir;
    let mut best_dot = f32::NEG_INFINITY;
    for c in corners {
        // A far-side corner's segment passes through the box interior (blocked);
        // a visible silhouette/near corner only grazes the boundary at its
        // endpoint (clear). Round toward the visible corner most aligned with the
        // goal. Fixed corner order gives a deterministic tie-break.
        if footprint_sweep_entry(&ObstacleVolume::Aabb { min, max }, from, c, mover_y).is_some() {
            continue;
        }
        let dir = (c - from).normalize_or_zero();
        let dot = dir.dot(goal_dir);
        if dot > best_dot {
            best_dot = dot;
            best = dir;
        }
    }
    best
}

/// Unit direction toward a regular prism's better-progress **tangent vertex** —
/// the polygon analog of [`steer_around_cylinder`], which it deliberately mirrors
/// (two candidate tangents, pick by goal alignment, [`STEER_TIE_EPS`] default).
///
/// The tangents are the two *angular extremes* of the inflated polygon as seen
/// from `from`. Because the polygon is convex, the ray through an angular extreme
/// is a supporting line: every other vertex lies to one side of it, so the ray
/// touches the footprint only at that vertex and is therefore a clear heading.
///
/// Angular extremes must be found by signed **angle** (`atan2`), not by the
/// cheaper signed sine: once the mover is close enough that the polygon subtends
/// more than a right angle, `sin` folds back and would pick an interior vertex.
///
/// Selecting "any unblocked vertex most aligned with the goal" — the approach
/// [`steer_around_box`] can afford — is wrong here: a polygon can have a vertex
/// pointing straight back at the mover, and that vertex is both unblocked and
/// perfectly goal-aligned, so it would steer directly into the obstacle. An
/// axis-aligned box never presents a corner along an approach axis, which is why
/// the box branch gets away with it.
fn steer_around_prism(
    center: Vec2,
    circumradius: f32,
    sides: u32,
    rotation: f32,
    from: Vec2,
    goal: Vec2,
) -> Vec2 {
    let goal_dir = (goal - from).normalize_or_zero();
    let skin = prism_apothem(circumradius, sides) + MOVER_RADIUS;
    let d = center - from;
    let dist = d.length();

    // Already within the collision skin (a hugging chase): no external tangents
    // exist, so peel off perpendicular to the center direction on whichever side
    // heads more toward the goal — identical to the cylinder's inside-skin case.
    // Tested exactly (half-planes), not by the inner-bound radius, because the
    // skin distance varies with angle between `skin` and the inflated
    // circumradius.
    if prism_half_planes_contain(from - center, skin, sides, rotation, false) {
        let dn = if dist > 1e-6 { d / dist } else { goal_dir };
        let dn = dn.normalize_or(Vec2::X);
        let perp = Vec2::new(-dn.y, dn.x);
        let s = if goal_dir.dot(perp) >= 0.0 { 1.0 } else { -1.0 };
        return perp * s;
    }

    // Circumradius of the polygon whose edges sit at `apothem + MOVER_RADIUS`.
    let inflated_circumradius = if sides < 3 {
        circumradius + MOVER_RADIUS
    } else {
        skin / (std::f32::consts::PI / sides as f32).cos()
    };
    let c_dir = if dist > 1e-6 {
        d / dist
    } else {
        return goal_dir;
    };

    let mut left: Option<(f32, Vec2)> = None; // greatest signed angle
    let mut right: Option<(f32, Vec2)> = None; // least signed angle
    for i in 0..sides {
        let v = center + prism_vertex(i, sides, rotation, inflated_circumradius);
        let dir = (v - from).normalize_or_zero();
        let angle = c_dir.perp_dot(dir).atan2(c_dir.dot(dir));
        if left.is_none_or(|(a, _)| angle > a) {
            left = Some((angle, dir));
        }
        if right.is_none_or(|(a, _)| angle < a) {
            right = Some((angle, dir));
        }
    }

    let t_left = left.map_or(goal_dir, |(_, dir)| dir);
    let t_right = right.map_or(goal_dir, |(_, dir)| dir);
    let dot_l = t_left.dot(goal_dir);
    let dot_r = t_right.dot(goal_dir);
    if (dot_l - dot_r).abs() < STEER_TIE_EPS || dot_l >= dot_r {
        // Goal ~directly behind the prism: deterministic default (left tangent).
        t_left
    } else {
        t_right
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

    fn aabb(min_x: f32, min_y: f32, min_z: f32, max_x: f32, max_y: f32, max_z: f32) -> ObstacleVolume {
        ObstacleVolume::Aabb {
            min: Vec3::new(min_x, min_y, min_z),
            max: Vec3::new(max_x, max_y, max_z),
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

    /// Scenario 10a: a step driven into a box's `-X` face from outside on X only
    /// slides tangentially along Z — the mover clamps to the X face (never
    /// entering the MOVER_RADIUS-inflated footprint) while keeping its lateral Z
    /// progress. Mirrors the cylinder slide test against an `Aabb`.
    #[test]
    fn resolve_movement_slides_along_box_x_face() {
        let box_vol = aabb(-5.0, 0.0, -5.0, 5.0, 2.0, 5.0);
        let pos = Vec3::new(-8.0, 1.0, 0.0); // outside on X only
        let desired = Vec3::new(0.0, 1.0, 2.0); // heads +X into the box with a +Z lateral bias
        let out = resolve_movement(&[box_vol], pos, desired);

        assert!(out.is_finite(), "no NaN/inf");
        assert_ne!(out, desired, "the blocked step must be modified");
        assert!(
            !position_blocked(&[box_vol], out),
            "resolved position {:?} must sit outside the inflated footprint",
            out
        );
        assert!(out.z > pos.z, "lateral (Z) progress must be preserved, got z={}", out.z);
        assert!(out.x <= -5.0, "must be clamped to the -X face, got x={}", out.x);
    }

    /// Scenario 10b: the Z-face mirror — approaching a box's `-Z` face from
    /// outside on Z only slides tangentially along X.
    #[test]
    fn resolve_movement_slides_along_box_z_face() {
        let box_vol = aabb(-5.0, 0.0, -5.0, 5.0, 2.0, 5.0);
        let pos = Vec3::new(0.0, 1.0, -8.0); // outside on Z only
        let desired = Vec3::new(2.0, 1.0, 0.0); // heads +Z into the box with a +X lateral bias
        let out = resolve_movement(&[box_vol], pos, desired);

        assert!(out.is_finite(), "no NaN/inf");
        assert_ne!(out, desired, "the blocked step must be modified");
        assert!(
            !position_blocked(&[box_vol], out),
            "resolved position {:?} must sit outside the inflated footprint",
            out
        );
        assert!(out.x > pos.x, "lateral (X) progress must be preserved, got x={}", out.x);
        assert!(out.z <= -5.0, "must be clamped to the -Z face, got z={}", out.z);
    }

    /// Scenario 10c: a corner approach (mover outside on BOTH axes) exercises the
    /// least-penetration-axis tie-break in `slide_against`: the desired step
    /// penetrates the `-X` face more shallowly than the `-Z` face, so the mover
    /// is pushed out along X (the axis of least penetration) and keeps its Z.
    #[test]
    fn resolve_movement_box_corner_clamps_least_penetration_axis() {
        let box_vol = aabb(-5.0, 0.0, -5.0, 5.0, 2.0, 5.0);
        let pos = Vec3::new(-8.0, 1.0, -8.0); // diagonally outside the corner (both axes)
        // Shallow X penetration (just past the -5.5 inflated face), deep Z
        // penetration (well inside), so the tie-break clamps X.
        let desired = Vec3::new(-4.0, 1.0, 0.0);
        let out = resolve_movement(&[box_vol], pos, desired);

        assert!(out.is_finite(), "no NaN/inf");
        assert!(
            !position_blocked(&[box_vol], out),
            "resolved position {:?} must sit outside the inflated footprint",
            out
        );
        assert!(
            out.x <= -5.0,
            "least-penetration tie-break must push out along X, got x={}",
            out.x
        );
        assert!(
            (out.z - desired.z).abs() < 1e-3,
            "clamping X must leave Z at the desired value, got z={}",
            out.z
        );
    }

    /// Scenario 10d: a mover fully enclosed by boxes (an inner box it sits in,
    /// wrapped by a huge outer box so any tangential slide is still inside the
    /// outer volume) stays put — returns `pos`, no NaN, no clip-through.
    #[test]
    fn resolve_movement_box_fully_enclosed_stays_put() {
        let inner = aabb(-5.0, 0.0, -5.0, 5.0, 20.0, 5.0);
        let outer = aabb(-30.0, 0.0, -30.0, 30.0, 20.0, 30.0);
        let pos = Vec3::new(0.0, 1.0, 0.0);
        let desired = Vec3::new(2.0, 1.0, 0.0);
        let out = resolve_movement(&[inner, outer], pos, desired);

        assert!(out.is_finite(), "no NaN/inf");
        assert!(out.distance(pos) < 1e-3, "enclosed mover must stay put, got {:?}", out);
    }

    /// Scenario 10e: a box whose Y-span sits entirely above the mover's `y ≈ 1.0`
    /// (an elevated platform) does not block ground movement — the step passes
    /// through unchanged even though its XZ projection lands inside the box.
    #[test]
    fn resolve_movement_box_above_mover_y_does_not_block() {
        let platform = aabb(-5.0, 5.0, -5.0, 5.0, 7.0, 5.0); // y ∈ [5, 7], above y=1
        let pos = Vec3::new(-8.0, 1.0, 0.0);
        let desired = Vec3::new(0.0, 1.0, 0.0); // XZ interior of the platform, but at ground y
        let out = resolve_movement(&[platform], pos, desired);

        assert_eq!(
            out, desired,
            "a platform whose y-span excludes the mover must not block ground movement"
        );
    }

    // -----------------------------------------------------------------------
    // Tangent steering (steer_toward_goal)
    // -----------------------------------------------------------------------

    /// Perpendicular distance from `center` to the infinite ray `from + t·dir`.
    fn ray_clearance(center: Vec2, from: Vec2, dir: Vec2) -> f32 {
        (center - from).perp_dot(dir).abs()
    }

    /// A clear straight line to the goal yields `None` (caller goes direct).
    #[test]
    fn steer_clear_line_returns_none() {
        let pillar = cylinder(0.0, 0.0, 2.5, 0.0, 5.0);
        // A path that passes well clear of the pillar (offset far in +z).
        let from = Vec2::new(-20.0, 20.0);
        let goal = Vec2::new(20.0, 20.0);
        assert_eq!(steer_toward_goal(&[pillar], from, goal, 1.0), None);
    }

    /// No obstacles ⇒ always `None`, regardless of geometry.
    #[test]
    fn steer_no_obstacles_returns_none() {
        let from = Vec2::new(-20.0, 0.0);
        let goal = Vec2::new(20.0, 0.0);
        assert_eq!(steer_toward_goal(&[], from, goal, 1.0), None);
    }

    /// Goal directly behind a cylinder ⇒ steer toward a TANGENT point, not the
    /// center: the returned direction is offset from straight-at-goal, and the
    /// ray along it clears the solid pillar (perpendicular distance ≥ radius).
    #[test]
    fn steer_goal_behind_cylinder_aims_at_tangent() {
        let pillar = cylinder(0.0, 0.0, 2.5, 0.0, 5.0);
        let from = Vec2::new(-20.0, 0.0);
        let goal = Vec2::new(20.0, 0.0);
        let dir = steer_toward_goal(&[pillar], from, goal, 1.0).expect("path is blocked");
        assert!((dir.length() - 1.0).abs() < 1e-4, "direction must be unit, got {dir:?}");

        let goal_dir = (goal - from).normalize();
        assert!(
            dir.dot(goal_dir) < 1.0 - 1e-6,
            "steering must deflect off the straight-at-goal line, got dot {}",
            dir.dot(goal_dir)
        );
        // The ray from `from` along `dir` must clear the solid pillar (radius
        // 2.5); the tangent is against the MOVER_RADIUS-inflated circle so the
        // clearance is ~eff = 3.0.
        let clr = ray_clearance(Vec2::ZERO, from, dir);
        assert!(
            clr >= 2.5,
            "tangent ray must clear the r=2.5 pillar, clearance was {clr}"
        );
    }

    /// The side choice takes the SHORTER way around: with the goal offset to +z,
    /// the mover steers to the +z tangent (dir.y > 0).
    #[test]
    fn steer_picks_shorter_side() {
        let pillar = cylinder(0.0, 0.0, 2.5, 0.0, 5.0);
        let from = Vec2::new(-20.0, 0.0);
        let goal = Vec2::new(20.0, 3.0); // slightly +z beyond the pillar
        let dir = steer_toward_goal(&[pillar], from, goal, 1.0).expect("path is blocked");
        assert!(
            dir.y > 0.0,
            "goal offset +z ⇒ round the +z side (dir.y > 0), got {dir:?}"
        );
    }

    /// A mover that repeatedly steps along the steering direction reaches a point
    /// with a clear straight line to the goal in a bounded number of steps, and
    /// never enters the pillar footprint on the way. This is the anti-oscillation
    /// guarantee: a flip-flopping selection would never converge.
    #[test]
    fn steer_converges_and_never_clips() {
        let pillar = cylinder(0.0, 0.0, 2.5, 0.0, 5.0);
        let goal = Vec2::new(20.0, 0.0);
        let mut pos = Vec2::new(-20.0, 0.0);
        let step = 0.5;
        let mut steps = 0;
        loop {
            assert!(
                pos.distance(Vec2::ZERO) >= 2.5 - 1e-3,
                "mover clipped the pillar at {pos:?}"
            );
            match steer_toward_goal(&[pillar], pos, goal, 1.0) {
                None => break, // straight line to goal is clear — arrived at sight
                Some(dir) => {
                    pos += dir * step;
                    steps += 1;
                    assert!(steps < 500, "steering did not converge (oscillation?)");
                }
            }
        }
        // Sanity: convergence took real work but a bounded amount (a clean arc
        // around a r=3 inflated circle from 20yd out is well under 200 steps).
        assert!(steps > 0, "the path should have been blocked initially");
        assert!(steps < 200, "convergence took {steps} steps — unexpectedly long");
    }

    /// Box analog: goal behind an AABB ⇒ steer toward a visible silhouette corner
    /// (off the straight-at-goal line), making forward progress toward the goal.
    #[test]
    fn steer_around_box_aims_at_corner() {
        let box_vol = aabb(-2.5, 0.0, -2.5, 2.5, 3.0, 2.5);
        let from = Vec2::new(-20.0, 0.0);
        let goal = Vec2::new(20.0, 0.0);
        let dir = steer_toward_goal(&[box_vol], from, goal, 1.0).expect("path is blocked");
        assert!((dir.length() - 1.0).abs() < 1e-4, "unit direction, got {dir:?}");
        assert!(dir.x > 0.0, "must still make +x progress toward the goal, got {dir:?}");
        assert!(
            dir.y.abs() > 1e-3,
            "must deflect off-axis toward a corner, got {dir:?}"
        );
    }

    /// A mover already at the collision skin of a cylinder (a hugging chase) has
    /// no external tangent; it peels off perpendicular toward the goal side, and
    /// the direction is unit and non-degenerate.
    #[test]
    fn steer_from_inside_skin_peels_perpendicular() {
        let pillar = cylinder(0.0, 0.0, 2.5, 0.0, 5.0);
        // Just inside the inflated skin (eff = 3.0) on the -x side.
        let from = Vec2::new(-2.9, 0.0);
        let goal = Vec2::new(20.0, 0.0); // straight through the pillar
        let dir = steer_toward_goal(&[pillar], from, goal, 1.0).expect("path is blocked");
        assert!((dir.length() - 1.0).abs() < 1e-4, "unit direction, got {dir:?}");
        // Perpendicular to the center direction (which is +x here) ⇒ ~pure ±z.
        assert!(dir.y.abs() > 0.9, "should peel sideways (±z), got {dir:?}");
    }

    /// An elevated platform whose y-span excludes the mover does not trigger
    /// steering — a ground path under it is clear.
    #[test]
    fn steer_platform_above_mover_does_not_deflect() {
        let platform = aabb(-5.0, 5.0, -5.0, 5.0, 7.0, 5.0); // y ∈ [5, 7]
        let from = Vec2::new(-20.0, 0.0);
        let goal = Vec2::new(20.0, 0.0); // XZ crosses the platform, but at ground y
        assert_eq!(steer_toward_goal(&[platform], from, goal, 1.0), None);
    }

    // ========================================================================
    // Regular-prism volumes (the Nagrand octagonal pillars)
    //
    // The through-line of these tests is that a prism is NOT its circumcircle:
    // between the apothem and the circumradius, whether a point is inside
    // depends on the angle. A prism silently implemented as a cylinder would
    // pass a naive "blocks through the center" test but fail these.
    // ========================================================================

    /// An octagonal pillar of `circumradius` centered at `(cx, cz)`, unrotated,
    /// spanning the standard pillar height.
    fn octagon(cx: f32, cz: f32, circumradius: f32) -> ObstacleVolume {
        ObstacleVolume::Prism {
            center_xz: Vec2::new(cx, cz),
            circumradius,
            sides: 8,
            rotation: 0.0,
            base_y: 0.0,
            height: 5.0,
        }
    }

    #[test]
    fn prism_apothem_matches_regular_polygon_geometry() {
        // Octagon: R·cos(π/8).
        assert!((prism_apothem(1.0, 8) - 0.923_879_5).abs() < 1e-6);
        // Square: R/√2.
        assert!((prism_apothem(1.0, 4) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        // Scales linearly with the circumradius.
        assert!((prism_apothem(2.5, 8) - 2.5 * 0.923_879_5).abs() < 1e-5);
        // Degenerate side counts collapse to zero rather than producing NaN.
        assert_eq!(prism_apothem(2.5, 2), 0.0);
    }

    /// The defining property: at a radius between the apothem and the
    /// circumradius, a vertex direction is inside and an edge-normal direction is
    /// outside. A circle of either radius cannot reproduce both.
    #[test]
    fn prism_contains_point_has_flat_edges_not_a_circle() {
        let r = 2.5_f32;
        let pillar = octagon(0.0, 0.0, r);
        let apothem = prism_apothem(r, 8); // ≈ 2.310

        // Vertex 0 is at angle 0 (+x), so just inside/outside along +x brackets
        // the circumradius.
        assert!(contains_point(&pillar, Vec3::new(r - 0.01, 1.0, 0.0)));
        assert!(!contains_point(&pillar, Vec3::new(r + 0.01, 1.0, 0.0)));

        // Edge 0's outward normal is at half a step (22.5°). Along that
        // direction the boundary is the apothem, which is *nearer* than the
        // circumradius — this is the flat-edge bite.
        let n = Vec2::from_angle(std::f32::consts::TAU / 16.0);
        let just_in = n * (apothem - 0.01);
        let just_out = n * (apothem + 0.01);
        assert!(contains_point(&pillar, Vec3::new(just_in.x, 1.0, just_in.y)));
        assert!(!contains_point(&pillar, Vec3::new(just_out.x, 1.0, just_out.y)));

        // The discriminating case: radius 2.4 sits between apothem and R, so it
        // is inside toward a vertex and outside toward an edge.
        let mid = 2.4_f32;
        assert!(
            contains_point(&pillar, Vec3::new(mid, 1.0, 0.0)),
            "r=2.4 toward a vertex must be inside"
        );
        let edge_pt = n * mid;
        assert!(
            !contains_point(&pillar, Vec3::new(edge_pt.x, 1.0, edge_pt.y)),
            "r=2.4 toward an edge must be outside (apothem is {apothem})"
        );
        assert!(mid > apothem && mid < r, "the test radius must straddle");
    }

    /// Rotation actually turns the polygon: rotating an octagon by half a step
    /// swaps which directions are vertices and which are edge normals.
    #[test]
    fn prism_rotation_turns_the_footprint() {
        let r = 2.5_f32;
        let half_step = std::f32::consts::TAU / 16.0;
        let rotated = ObstacleVolume::Prism {
            center_xz: Vec2::ZERO,
            circumradius: r,
            sides: 8,
            rotation: half_step,
            base_y: 0.0,
            height: 5.0,
        };
        // +x was a vertex direction unrotated; after a half-step turn it is an
        // edge normal, so a point that was inside is now outside.
        let mid = 2.4_f32;
        assert!(contains_point(&octagon(0.0, 0.0, r), Vec3::new(mid, 1.0, 0.0)));
        assert!(!contains_point(&rotated, Vec3::new(mid, 1.0, 0.0)));
    }

    #[test]
    fn prism_blocks_and_clears_line_of_sight() {
        let pillar = octagon(0.0, 0.0, 2.5);
        // Straight through the middle.
        assert!(!has_line_of_sight(
            &[pillar],
            Vec3::new(-20.0, EYE_HEIGHT, 0.0),
            Vec3::new(20.0, EYE_HEIGHT, 0.0),
        ));
        // Passing wide of the circumcircle.
        assert!(has_line_of_sight(
            &[pillar],
            Vec3::new(-20.0, EYE_HEIGHT, 6.0),
            Vec3::new(20.0, EYE_HEIGHT, 6.0),
        ));

        // Unrotated, an octagon has a vertex at 90°, so its +z extent IS the
        // circumradius and a graze at z=2.4 legitimately clips it.
        assert!(!has_line_of_sight(
            &[pillar],
            Vec3::new(-20.0, EYE_HEIGHT, 2.4),
            Vec3::new(20.0, EYE_HEIGHT, 2.4),
        ));

        // Turn the octagon a half step so +z becomes an edge normal: now the +z
        // extent is the apothem (≈2.31) and the same graze is clear. This is the
        // sightline a cylinder of equal circumradius would wrongly block, and it
        // is why pillar `rotation_deg` is a gameplay-relevant knob and not just
        // cosmetic.
        let turned = ObstacleVolume::Prism {
            center_xz: Vec2::ZERO,
            circumradius: 2.5,
            sides: 8,
            rotation: std::f32::consts::PI / 8.0,
            base_y: 0.0,
            height: 5.0,
        };
        assert!(prism_apothem(2.5, 8) < 2.4, "test premise: apothem is inside 2.4");
        assert!(has_line_of_sight(
            &[turned],
            Vec3::new(-20.0, EYE_HEIGHT, 2.4),
            Vec3::new(20.0, EYE_HEIGHT, 2.4),
        ));
        // ...and still blocks a graze inside the apothem.
        assert!(!has_line_of_sight(
            &[turned],
            Vec3::new(-20.0, EYE_HEIGHT, 2.2),
            Vec3::new(20.0, EYE_HEIGHT, 2.2),
        ));
    }

    /// The y-span is finite, so a raised prism never occludes a ground sightline.
    #[test]
    fn prism_y_span_is_finite() {
        let elevated = ObstacleVolume::Prism {
            center_xz: Vec2::ZERO,
            circumradius: 2.5,
            sides: 8,
            rotation: 0.0,
            base_y: 6.0,
            height: 4.0,
        };
        assert!(has_line_of_sight(
            &[elevated],
            Vec3::new(-20.0, EYE_HEIGHT, 0.0),
            Vec3::new(20.0, EYE_HEIGHT, 0.0),
        ));
        assert!(!has_line_of_sight(
            &[elevated],
            Vec3::new(-20.0, 8.0, 0.0),
            Vec3::new(20.0, 8.0, 0.0),
        ));
    }

    #[test]
    fn prism_footprint_honors_mover_radius() {
        let pillar = octagon(0.0, 0.0, 2.5);
        let apothem = prism_apothem(2.5, 8);
        // Approaching along an edge normal, the movement skin is apothem + r.
        let n = Vec2::from_angle(std::f32::consts::TAU / 16.0);
        let inside = n * (apothem + MOVER_RADIUS - 0.05);
        let outside = n * (apothem + MOVER_RADIUS + 0.05);
        assert!(position_blocked(&[pillar], Vec3::new(inside.x, 1.0, inside.y)));
        assert!(!position_blocked(&[pillar], Vec3::new(outside.x, 1.0, outside.y)));
    }

    /// Walking straight into a prism resolves to a non-penetrating position that
    /// keeps lateral progress — the same contract the cylinder/box branches hold.
    #[test]
    fn resolve_movement_slides_along_prism_without_clipping() {
        let pillar = octagon(0.0, 0.0, 2.5);
        let pos = Vec3::new(-4.0, 1.0, 0.3);
        let desired = Vec3::new(-2.0, 1.0, 0.3); // into the footprint
        assert!(position_blocked(&[pillar], desired), "test setup: step is blocked");

        let resolved = resolve_movement(&[pillar], pos, desired);
        assert!(
            !position_blocked(&[pillar], resolved),
            "resolved position {resolved:?} still penetrates the pillar"
        );
        assert_eq!(resolved.y, desired.y, "slides are XZ-only");
        // The blocked normal component is removed but the tangential slide keeps
        // the mover moving rather than freezing it in place.
        assert!(
            resolved.distance(pos) > 1e-3,
            "expected a tangential slide, got {resolved:?} from {pos:?}"
        );
    }

    /// Prism analog of `steer_converges_and_never_clips`: a mover stepping along
    /// the steering direction rounds an octagonal pillar and reaches a clear line
    /// to the goal, never entering the footprint. Catches oscillation, which is
    /// the failure mode a vertex-selection tie-break can introduce.
    #[test]
    fn steer_converges_around_prism_without_clipping() {
        let pillar = octagon(0.0, 0.0, 2.5);
        let goal = Vec2::new(20.0, 0.0);
        let mut pos = Vec2::new(-20.0, 0.0);
        let step = 0.5;
        let mut steps = 0;
        loop {
            assert!(
                !position_blocked(&[pillar], Vec3::new(pos.x, 1.0, pos.y)),
                "mover clipped the pillar at {pos:?}"
            );
            match steer_toward_goal(&[pillar], pos, goal, 1.0) {
                None => break, // clear straight line to the goal
                Some(dir) => {
                    assert!(
                        (dir.length() - 1.0).abs() < 1e-4,
                        "steering must return a unit direction, got {dir:?}"
                    );
                    pos += dir * step;
                    steps += 1;
                    assert!(steps < 500, "steering did not converge (oscillation?)");
                }
            }
        }
        assert!(steps > 0, "the path should have been blocked initially");
        assert!(steps < 200, "convergence took {steps} steps — unexpectedly long");
    }

    /// Steering is a deterministic function of geometry: same inputs, same
    /// direction, every time (no hashing or float-order dependence).
    #[test]
    fn steer_around_prism_is_deterministic() {
        let pillar = octagon(0.0, 0.0, 2.5);
        let from = Vec2::new(-12.0, 0.0);
        let goal = Vec2::new(12.0, 0.0);
        let first = steer_toward_goal(&[pillar], from, goal, 1.0).expect("blocked");
        for _ in 0..16 {
            assert_eq!(steer_toward_goal(&[pillar], from, goal, 1.0), Some(first));
        }
    }

    /// A prism whose y-span is above the mover does not deflect a ground path,
    /// mirroring the platform case for boxes.
    #[test]
    fn steer_prism_above_mover_does_not_deflect() {
        let elevated = ObstacleVolume::Prism {
            center_xz: Vec2::ZERO,
            circumradius: 3.0,
            sides: 8,
            rotation: 0.0,
            base_y: 5.0,
            height: 2.0,
        };
        assert_eq!(
            steer_toward_goal(&[elevated], Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), 1.0),
            None
        );
    }

    /// `prism_vertices_world` is the shared outline the 3D mesh and the top-down
    /// schematic both draw; its vertices must sit on the circumcircle, be
    /// `sides` in count, and lie on the volume's closed boundary.
    #[test]
    fn prism_vertices_world_lie_on_the_volume_boundary() {
        let center = Vec2::new(-40.0, 20.0);
        let r = 2.5_f32;
        let verts = prism_vertices_world(center, r, 8, 0.0);
        assert_eq!(verts.len(), 8);
        let pillar = ObstacleVolume::Prism {
            center_xz: center,
            circumradius: r,
            sides: 8,
            rotation: 0.0,
            base_y: 0.0,
            height: 5.0,
        };
        for v in verts {
            assert!(
                (v.distance(center) - r).abs() < 1e-4,
                "vertex {v:?} is not on the circumcircle"
            );
            // Inclusive boundary policy: a vertex counts as inside.
            assert!(contains_point(&pillar, Vec3::new(v.x, 1.0, v.y)));
        }
    }
}
