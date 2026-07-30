//! Per-map arena boundary shape — the walkable region, and the visual outline
//! derived from it.
//!
//! Arena bounds used to be three global constants (`ARENA_HALF_X`,
//! `ARENA_HALF_Z`, `ARENA_CORNER_SUM`) shared by every map, plus a second,
//! hand-maintained set for the visual floor (`ARENA_FLOOR_*`). That worked while
//! all maps were the same 76×46 cut-corner octagon. The Nagrand replica is a
//! ~120yd circular bowl with gate alcoves, so the shape has to be map data.
//!
//! ## Two shapes, one source
//!
//! - **Gameplay bounds** — what [`ArenaBounds::contains`] / [`ArenaBounds::clamp`]
//!   enforce. Inset from the walls by a wall-thickness + combatant buffer.
//! - **Visual outline** — where the floor mesh and wall meshes go, obtained by
//!   offsetting the gameplay bounds outward by [`WALL_OFFSET`].
//!
//! The visual outline is *derived*, not declared, so the two cannot drift — the
//! retired `ARENA_FLOOR_*` constants are gone rather than kept alongside. For
//! [`ArenaBounds::Octagon`] the derivation reproduces them exactly (36.5 + 1.5 =
//! 38, 21.5 + 1.5 = 23, and 48.88 + 1.5·√2 = 51 = 38 − 10 + 23), which is what
//! keeps BasicArena byte-identical, and a test pins that.
//!
//! Anything positioned against the arena's shape must go through this type. The
//! failure mode is silent: a consumer left on the old hard-coded octagon (the
//! Shaman totem ground decal was one) does not error on Nagrand, it just draws or
//! measures the wrong arena.
//!
//! ## Adding a shape
//!
//! Add a variant, then implement `contains`, `clamp`, `edge_closeness`,
//! `half_extents`, and `outline`. `edge_closeness` is the one with a behavioral
//! contract worth reading twice: it feeds the movement scorer's `corner_penalty`
//! term, so its 0..1 ramp must mean the same thing across shapes or healer
//! posture tuning stops transferring between maps.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Outward distance from the gameplay bounds to the wall centerlines: wall
/// half-thickness (0.5) + combatant buffer (1.0). The floor and wall meshes sit
/// on the offset outline; combatants are clamped to the inner one.
pub const WALL_OFFSET: f32 = 1.5;

/// Fraction of the way to the boundary at which the scorer's corner/edge penalty
/// begins ramping. Preserves the old `CORNER_PENALTY_ONSET = ARENA_CORNER_SUM *
/// 0.7`, so ~70% of the wall keeps the arena's center open.
///
/// `pub` because `movement_scoring::CORNER_PENALTY_ONSET` — the octagon-only
/// absolute threshold the movement probes assert against — is derived from it. The
/// two must not restate 0.7 independently, or retuning the onset moves the scorer
/// without moving the probes that police it.
pub const EDGE_PENALTY_ONSET_FRACTION: f32 = 0.7;

/// The walkable region of one arena, in XZ world units (yards).
///
/// `Copy` and allocation-free so it can ride along in `CombatContext` and
/// `ScorerInputs` next to the obstacle list without lifetime churn.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ArenaBounds {
    /// An axis-aligned rectangle with the four corners chamfered by a diagonal
    /// wall constraining `|x| + |z| <= corner_sum`. The original arena shape.
    Octagon {
        half_x: f32,
        half_z: f32,
        corner_sum: f32,
    },
    /// A roughly circular bowl (an ellipse, to allow a slightly elongated arena)
    /// with a rectangular gate alcove at each `±x` end — the Nagrand shape.
    ///
    /// The alcove band runs the full `x` range rather than only the part outside
    /// the ellipse; the inner portion is already inside the ellipse, so the union
    /// is a connected region with no notch where corridor meets bowl.
    Bowl {
        /// Ellipse semi-axis along x (gate-to-gate).
        semi_x: f32,
        /// Ellipse semi-axis along z.
        semi_z: f32,
        /// How far each starting room extends beyond the ellipse along x.
        alcove_depth: f32,
        /// Half-width of the starting-room corridor in z.
        alcove_half_width: f32,
    },
}

impl Default for ArenaBounds {
    /// The historical arena: 76×46 with 10yd corner chamfers, inset for walls.
    fn default() -> Self {
        ArenaBounds::Octagon {
            half_x: 36.5,
            half_z: 21.5,
            corner_sum: 48.88,
        }
    }
}

impl ArenaBounds {
    /// Whether an XZ position is inside the walkable region.
    pub fn contains(&self, pos: Vec3) -> bool {
        match *self {
            ArenaBounds::Octagon {
                half_x,
                half_z,
                corner_sum,
            } => {
                // Kept in this exact form (early returns, then the diagonal) so
                // it is bit-for-bit the old `is_in_arena_bounds`.
                if pos.x < -half_x || pos.x > half_x {
                    return false;
                }
                if pos.z < -half_z || pos.z > half_z {
                    return false;
                }
                pos.x.abs() + pos.z.abs() <= corner_sum
            }
            ArenaBounds::Bowl {
                semi_x,
                semi_z,
                alcove_depth,
                alcove_half_width,
            } => {
                let in_alcove =
                    pos.z.abs() <= alcove_half_width && pos.x.abs() <= semi_x + alcove_depth;
                in_alcove || ellipse_radius(pos.x, pos.z, semi_x, semi_z) <= 1.0
            }
        }
    }

    /// Project a position back inside the walkable region, preserving `y`.
    ///
    /// Idempotent: clamping an already-clamped position is a no-op.
    pub fn clamp(&self, mut pos: Vec3) -> Vec3 {
        match *self {
            ArenaBounds::Octagon {
                half_x,
                half_z,
                corner_sum,
            } => {
                pos.x = pos.x.clamp(-half_x, half_x);
                pos.z = pos.z.clamp(-half_z, half_z);
                // Diagonal corners: project inward along the 45° normal.
                let corner_excess = pos.x.abs() + pos.z.abs() - corner_sum;
                if corner_excess > 0.0 {
                    let half = corner_excess / 2.0;
                    pos.x -= half * pos.x.signum();
                    pos.z -= half * pos.z.signum();
                }
                pos
            }
            ArenaBounds::Bowl {
                semi_x,
                semi_z,
                alcove_depth,
                alcove_half_width,
            } => {
                if self.contains(pos) {
                    return pos;
                }
                // Inside the corridor band: the only way out is along x, so clamp
                // to the far end of the alcove.
                if pos.z.abs() <= alcove_half_width {
                    pos.x = pos.x.clamp(-(semi_x + alcove_depth), semi_x + alcove_depth);
                    return pos;
                }
                // Outside the corridor band in z. Two boundaries can be the
                // nearest one here, and picking the wrong one TELEPORTS the mover:
                //
                // - The ellipse, for a point out past the bowl's shoulder.
                // - The corridor's SIDE wall, for a point inside a starting room
                //   (`|x|` past the bowl wall) that has just stepped over
                //   `z = ±alcove_half_width`. Such a point is a fraction of a yard
                //   from that side wall but ~10yd from the ellipse, so radially
                //   projecting it would fling the mover back through the wall into
                //   the bowl in a single frame.
                //
                // So compute both projections and take the nearer. The radial one
                // is still a radial projection, not the true nearest point on the
                // ellipse (which needs iteration) — adequate for the same reason
                // the octagon's 45°-normal projection is, and cheap + deterministic.
                let r = ellipse_radius(pos.x, pos.z, semi_x, semi_z);
                let onto_bowl = if r > 1.0 {
                    Vec3::new(pos.x / r, pos.y, pos.z / r)
                } else {
                    pos
                };
                let onto_corridor = Vec3::new(
                    pos.x.clamp(-(semi_x + alcove_depth), semi_x + alcove_depth),
                    pos.y,
                    pos.z.clamp(-alcove_half_width, alcove_half_width),
                );
                if pos.distance(onto_corridor) < pos.distance(onto_bowl) {
                    onto_corridor
                } else {
                    onto_bowl
                }
            }
        }
    }

    /// Graded 0..1 closeness to the boundary, for the movement scorer's
    /// `corner_penalty` term: `<= 0` outside the penalty zone, `1.0` at the wall.
    ///
    /// Shape-specific by necessity — an ellipse has no corners — but normalized
    /// identically so a tuned `corner_penalty` weight means the same thing on
    /// every map.
    pub fn edge_closeness(&self, x: f32, z: f32) -> f32 {
        match *self {
            ArenaBounds::Octagon { corner_sum, .. } => {
                // Distance along the diagonal, exactly as before.
                let onset = corner_sum * EDGE_PENALTY_ONSET_FRACTION;
                ((x.abs() + z.abs()) - onset) / (corner_sum - onset)
            }
            ArenaBounds::Bowl {
                semi_x, semi_z, ..
            } => {
                // Normalized ellipse radius is already 1.0 at the wall.
                let r = ellipse_radius(x, z, semi_x, semi_z);
                (r - EDGE_PENALTY_ONSET_FRACTION) / (1.0 - EDGE_PENALTY_ONSET_FRACTION)
            }
        }
    }

    /// The `|x|` at which a team lines up before the gates open. Team 1 spawns at
    /// `-x`, team 2 at `+x`.
    ///
    /// This is load-bearing for gameplay, not just presentation: combatants must
    /// start *outboard* of the cover so that closing to engage carries them past
    /// it. Spawning inboard of the pillars leaves both teams inside the pillar
    /// rectangle, converging on an empty arena center with the nearest cover tens
    /// of yards behind them — which measures as literally zero occlusion over a
    /// whole match, no matter how large the pillars are.
    ///
    /// For [`ArenaBounds::Octagon`] this is `half_x - WALL_OFFSET`, which
    /// reproduces the historical hard-coded `±35.0` exactly (36.5 − 1.5), keeping
    /// BasicArena's spawns bit-identical.
    pub fn team_spawn_x(&self) -> f32 {
        match *self {
            ArenaBounds::Octagon { half_x, .. } => half_x - WALL_OFFSET,
            // Mid-room, so a 3v3 line abreast fits with clearance at both ends
            // and the walk to the arena proper crosses the pillar line.
            ArenaBounds::Bowl {
                semi_x,
                alcove_depth,
                ..
            } => semi_x + alcove_depth * 0.5,
        }
    }

    /// Where a team's gate sits: `(|x| of the gate plane, half-width in z)`.
    ///
    /// The gate spans the mouth of the starting area, so the bars visually seal
    /// the team in during the countdown. Purely presentational today —
    /// `move_to_target` early-returns until the gates open, so nothing can walk
    /// through a gate regardless — but it should still line up with the geometry.
    pub fn gate_plane(&self) -> (f32, f32) {
        match *self {
            // Historical placement: just inboard of the spawn line, with the bar
            // fan roughly matching the arena's short axis.
            ArenaBounds::Octagon { half_x, half_z, .. } => (half_x - WALL_OFFSET - 3.0, half_z * 0.4),
            // The room mouth, where the corridor meets the bowl.
            ArenaBounds::Bowl {
                semi_x,
                alcove_half_width,
                ..
            } => (semi_x, alcove_half_width),
        }
    }

    /// Half-extents of the gameplay bounds' bounding box. Used for preview aspect
    /// ratios and camera framing.
    pub fn half_extents(&self) -> Vec2 {
        match *self {
            ArenaBounds::Octagon { half_x, half_z, .. } => Vec2::new(half_x, half_z),
            ArenaBounds::Bowl {
                semi_x,
                semi_z,
                alcove_depth,
                ..
            } => Vec2::new(semi_x + alcove_depth, semi_z),
        }
    }

    /// The visual outline (wall centerlines / floor edge) as a closed polygon in
    /// counter-clockwise XZ order: the gameplay bounds offset outward by
    /// [`WALL_OFFSET`].
    ///
    /// `arc_segments` controls curve tessellation and is ignored by straight-edged
    /// shapes. Shared by the 3D floor mesh, the wall placement, the top-down
    /// schematic, and the ground decals, so all four agree by construction.
    pub fn outline(&self, arc_segments: usize) -> Vec<Vec2> {
        match *self {
            ArenaBounds::Octagon {
                half_x,
                half_z,
                corner_sum,
            } => {
                let hx = half_x + WALL_OFFSET;
                let hz = half_z + WALL_OFFSET;
                // The chamfer, recovered from the diagonal constraint: the visual
                // corner sum is offset by WALL_OFFSET along the 45° normal, and
                // `cut = hx - (corner_sum_visual - hz)`.
                let visual_sum = corner_sum + WALL_OFFSET * std::f32::consts::SQRT_2;
                let cut = (hx + hz - visual_sum).max(0.0);
                vec![
                    Vec2::new(hx - cut, -hz),
                    Vec2::new(hx, -hz + cut),
                    Vec2::new(hx, hz - cut),
                    Vec2::new(hx - cut, hz),
                    Vec2::new(-hx + cut, hz),
                    Vec2::new(-hx, hz - cut),
                    Vec2::new(-hx, -hz + cut),
                    Vec2::new(-hx + cut, -hz),
                ]
            }
            ArenaBounds::Bowl {
                semi_x,
                semi_z,
                alcove_depth,
                alcove_half_width,
            } => {
                let ax = semi_x + WALL_OFFSET;
                let az = semi_z + WALL_OFFSET;
                let mouth = alcove_half_width + WALL_OFFSET;
                let far = semi_x + alcove_depth + WALL_OFFSET;
                // Walk the ellipse, breaking out at each gate mouth to trace the
                // alcove's three walls, so the outline is one closed loop
                // enclosing bowl + both starting rooms.
                let segments = arc_segments.max(8);
                let mut pts: Vec<Vec2> = Vec::with_capacity(segments + 8);
                let mouth_angle = (mouth / az).clamp(-1.0, 1.0).asin();

                // +x gate mouth, going counter-clockwise (from -z to +z).
                pts.push(Vec2::new(ellipse_x_at(mouth, ax, az), -mouth));
                pts.push(Vec2::new(far, -mouth));
                pts.push(Vec2::new(far, mouth));
                pts.push(Vec2::new(ellipse_x_at(mouth, ax, az), mouth));
                // Upper arc: from just past the +x mouth around to the -x mouth.
                arc_points(ax, az, mouth_angle, std::f32::consts::PI - mouth_angle, segments, &mut pts);
                // -x gate mouth.
                pts.push(Vec2::new(-ellipse_x_at(mouth, ax, az), mouth));
                pts.push(Vec2::new(-far, mouth));
                pts.push(Vec2::new(-far, -mouth));
                pts.push(Vec2::new(-ellipse_x_at(mouth, ax, az), -mouth));
                // Lower arc: back around to the +x mouth.
                arc_points(
                    ax,
                    az,
                    std::f32::consts::PI + mouth_angle,
                    std::f32::consts::TAU - mouth_angle,
                    segments,
                    &mut pts,
                );
                pts
            }
        }
    }
}

/// Half-extents of an [`ArenaBounds::outline`] polygon's bounding box — the
/// extents to fit when scaling or framing a drawing of the whole arena, walls
/// included.
///
/// Shared by every consumer that has to size the arena (the Configure Match
/// preview pane and its schematic, the annotated layout view) so they cannot
/// disagree about how big it is; floored at 1 so a degenerate outline cannot make
/// a caller divide by zero.
pub fn outline_half_extents(outline: &[Vec2]) -> Vec2 {
    outline
        .iter()
        .fold(Vec2::ZERO, |acc, p| acc.max(p.abs()))
        .max(Vec2::splat(1.0))
}

/// Normalized ellipse radius: `1.0` exactly on the boundary, `< 1` inside.
fn ellipse_radius(x: f32, z: f32, semi_x: f32, semi_z: f32) -> f32 {
    let sx = semi_x.max(1e-6);
    let sz = semi_z.max(1e-6);
    ((x / sx).powi(2) + (z / sz).powi(2)).sqrt()
}

/// The `+x` ellipse abscissa at height `z` — where a gate mouth meets the bowl.
fn ellipse_x_at(z: f32, semi_x: f32, semi_z: f32) -> f32 {
    let sz = semi_z.max(1e-6);
    let inner = (1.0 - (z / sz).powi(2)).max(0.0);
    semi_x * inner.sqrt()
}

/// Append points along an ellipse arc from `start` to `end` radians (exclusive of
/// the endpoints, which callers supply exactly).
fn arc_points(
    semi_x: f32,
    semi_z: f32,
    start: f32,
    end: f32,
    segments: usize,
    out: &mut Vec<Vec2>,
) {
    let steps = segments.max(2);
    for i in 1..steps {
        let t = start + (end - start) * (i as f32 / steps as f32);
        out.push(Vec2::new(semi_x * t.cos(), semi_z * t.sin()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default octagon must reproduce the retired global constants exactly —
    /// this is the guard that BasicArena's gameplay bounds did not move.
    #[test]
    fn default_octagon_matches_historical_constants() {
        let ArenaBounds::Octagon {
            half_x,
            half_z,
            corner_sum,
        } = ArenaBounds::default()
        else {
            panic!("default must be the octagon");
        };
        assert_eq!(half_x, 36.5);
        assert_eq!(half_z, 21.5);
        assert_eq!(corner_sum, 48.88);
    }

    /// ...and its derived visual outline must reproduce the retired
    /// `ARENA_FLOOR_*` constants (38, 23, corner cut 10).
    #[test]
    fn default_octagon_outline_matches_historical_floor() {
        let outline = ArenaBounds::default().outline(0);
        assert_eq!(outline.len(), 8);
        let max_x = outline.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let max_z = outline.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!((max_x - 38.0).abs() < 1e-3, "floor half-x should be 38, got {max_x}");
        assert!((max_z - 23.0).abs() < 1e-3, "floor half-z should be 23, got {max_z}");
        // The chamfer: the vertex on the +x edge sits at z = 23 - 10 = 13.
        let cut_vertex = outline
            .iter()
            .find(|p| (p.x - 38.0).abs() < 1e-3 && p.y > 0.0)
            .expect("a +x edge vertex at positive z");
        assert!(
            (cut_vertex.y - 13.0).abs() < 1e-2,
            "corner cut should be 10 (vertex at z=13), got {cut_vertex:?}"
        );
    }

    #[test]
    fn octagon_containment_and_clamp_are_unchanged() {
        let b = ArenaBounds::default();
        assert!(b.contains(Vec3::ZERO));
        assert!(!b.contains(Vec3::new(40.0, 0.0, 0.0)));
        assert!(!b.contains(Vec3::new(0.0, 0.0, 25.0)));
        // Diagonal corner: 30 + 20 = 50 > 48.88.
        assert!(!b.contains(Vec3::new(30.0, 0.0, 20.0)));
        assert!(b.contains(Vec3::new(25.0, 0.0, 15.0)));

        assert_eq!(b.clamp(Vec3::new(50.0, 1.0, 0.0)).x, 36.5);
        // y is preserved through a clamp.
        assert_eq!(b.clamp(Vec3::new(50.0, 3.5, 30.0)).y, 3.5);
        // Corner projection lands on the diagonal.
        let c = b.clamp(Vec3::new(35.0, 1.0, 20.0));
        assert!((c.x.abs() + c.z.abs() - 48.88).abs() < 0.01);
    }

    #[test]
    fn clamp_is_idempotent_for_both_shapes() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 60.0,
            semi_z: 60.0,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        for bounds in [ArenaBounds::default(), bowl] {
            for p in [
                Vec3::new(200.0, 1.0, 0.0),
                Vec3::new(-90.0, 1.0, 55.0),
                Vec3::new(0.0, 1.0, -300.0),
                Vec3::new(45.0, 1.0, 45.0),
            ] {
                let once = bounds.clamp(p);
                let twice = bounds.clamp(once);
                assert!(
                    once.distance(twice) < 1e-3,
                    "{bounds:?} clamp not idempotent for {p:?}: {once:?} -> {twice:?}"
                );
                assert!(
                    bounds.contains(once),
                    "{bounds:?} clamp left {p:?} outside at {once:?}"
                );
            }
        }
    }

    #[test]
    fn bowl_alcoves_are_inside_and_connected() {
        let b = ArenaBounds::Bowl {
            semi_x: 60.0,
            semi_z: 60.0,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        // Deep in a starting room, past the bowl wall.
        assert!(b.contains(Vec3::new(69.0, 1.0, 0.0)));
        assert!(b.contains(Vec3::new(-69.0, 1.0, 7.0)));
        // Past the far end of the room.
        assert!(!b.contains(Vec3::new(71.0, 1.0, 0.0)));
        // Beside the room mouth, outside the bowl.
        assert!(!b.contains(Vec3::new(62.0, 1.0, 20.0)));
        // The bowl itself.
        assert!(b.contains(Vec3::new(0.0, 1.0, 59.0)));
        assert!(!b.contains(Vec3::new(0.0, 1.0, 61.0)));
        // A walk down the corridor centerline never leaves the region — the
        // corridor/bowl union has no gap.
        for i in 0..=70 {
            let x = i as f32;
            assert!(b.contains(Vec3::new(x, 1.0, 0.0)), "gap at x={x}");
        }
    }

    /// Stepping over a starting room's SIDE wall must clamp onto that wall, not
    /// radially onto the ellipse — the latter is ~10yd away and would teleport the
    /// mover back through the wall into the bowl in a single frame.
    ///
    /// A movement step is at most ~0.12yd (7yd/s at 60Hz), so the clamp must never
    /// move a position further than a step's worth when it only just left bounds.
    #[test]
    fn bowl_clamp_never_teleports_out_of_a_starting_room() {
        let b = ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        // Walk down the room's side wall, nudging just outside it each time.
        for x in [61.0_f32, 64.0, 66.0, 69.0, 69.7] {
            for (px, pz) in [(x, 8.06_f32), (x, -8.06), (-x, 8.06), (-x, -8.06)] {
                let p = Vec3::new(px, 1.0, pz);
                assert!(!b.contains(p), "{p:?} should be just outside the corridor");
                let c = b.clamp(p);
                assert!(b.contains(c), "clamp left {p:?} outside at {c:?}");
                assert!(
                    p.distance(c) < 0.5,
                    "clamp teleported {p:?} to {c:?} ({:.2}yd) instead of onto the \
                     corridor's side wall",
                    p.distance(c)
                );
            }
        }
        // ...while a point out past the bowl's shoulder still projects radially
        // onto the ellipse rather than being dragged into the corridor.
        let shoulder = Vec3::new(62.0, 1.0, 20.0);
        let c = b.clamp(shoulder);
        assert!(b.contains(c), "shoulder clamp left {shoulder:?} outside at {c:?}");
        assert!(
            c.z.abs() > 8.0,
            "shoulder point should clamp onto the bowl, not the corridor: {c:?}"
        );
    }

    /// `edge_closeness` must mean the same thing on both shapes: <= 0 in the open
    /// middle, ~1.0 at the wall, monotonically rising in between.
    #[test]
    fn edge_closeness_is_comparable_across_shapes() {
        let oct = ArenaBounds::default();
        let bowl = ArenaBounds::Bowl {
            semi_x: 60.0,
            semi_z: 60.0,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        assert!(oct.edge_closeness(0.0, 0.0) <= 0.0);
        assert!(bowl.edge_closeness(0.0, 0.0) <= 0.0);

        // At the boundary both read ~1.0.
        assert!((oct.edge_closeness(48.88, 0.0) - 1.0).abs() < 1e-2);
        assert!((bowl.edge_closeness(60.0, 0.0) - 1.0).abs() < 1e-2);

        // Monotonic outward.
        let mut prev = f32::MIN;
        for i in 0..=60 {
            let c = bowl.edge_closeness(i as f32, 0.0);
            assert!(c >= prev - 1e-6, "edge_closeness dipped at x={i}");
            prev = c;
        }
    }

    /// The octagon's `edge_closeness` must be the retired
    /// `(|x|+|z| - CORNER_PENALTY_ONSET) / (ARENA_CORNER_SUM - CORNER_PENALTY_ONSET)`
    /// to the bit, or healer corner behavior on BasicArena shifts.
    #[test]
    fn octagon_edge_closeness_matches_retired_formula() {
        let corner_sum = 48.88_f32;
        let onset = corner_sum * 0.7;
        let b = ArenaBounds::default();
        for (x, z) in [
            (20.0_f32, 10.0_f32),
            (30.0, 15.0),
            (36.0, 12.0),
            (5.0, 5.0),
        ] {
            let expected = ((x.abs() + z.abs()) - onset) / (corner_sum - onset);
            assert_eq!(b.edge_closeness(x, z), expected, "at ({x}, {z})");
        }
    }

    /// The octagon's spawn `|x|` must stay at the historical hard-coded 35.0, or
    /// BasicArena matches shift off their recorded baselines.
    #[test]
    fn octagon_spawn_x_matches_historical_value() {
        assert_eq!(ArenaBounds::default().team_spawn_x(), 35.0);
    }

    /// Spawns must be inside the walkable region and, on the bowl, inside a
    /// starting room rather than out in the arena proper.
    #[test]
    fn spawns_land_inside_the_starting_rooms() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        let x = bowl.team_spawn_x();
        // Past the bowl wall, short of the room's far end.
        assert!(x > 59.72, "spawn should be beyond the bowl wall, got {x}");
        assert!(x < 69.72, "spawn should be inside the room, got {x}");

        // A 3v3 line abreast (slots at z = -3, 0, +3) must all be walkable.
        for bounds in [ArenaBounds::default(), bowl] {
            let sx = bounds.team_spawn_x();
            for slot in [-3.0_f32, 0.0, 3.0] {
                for sign in [-1.0_f32, 1.0] {
                    let p = Vec3::new(sign * sx, 1.0, slot);
                    assert!(
                        bounds.contains(p),
                        "{bounds:?}: spawn slot {p:?} is outside the arena"
                    );
                }
            }
        }
    }

    /// Spawns must sit OUTBOARD of Nagrand's pillars (|x| = 40), so closing to
    /// engage carries a combatant past the cover. Spawning inboard is what
    /// produced zero measured occlusion across a whole match.
    #[test]
    fn bowl_spawns_are_outboard_of_the_pillars() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        assert!(
            bowl.team_spawn_x() > 40.0,
            "spawn {} must be outboard of the pillar line at 40",
            bowl.team_spawn_x()
        );
    }

    /// The outline drives both the floor mesh and the wall segments, so it must be
    /// a clean closed loop: no duplicate/coincident consecutive points (which
    /// would emit a degenerate wall) and no non-finite coordinates (which would
    /// poison the mesh and render as nothing).
    #[test]
    fn outline_is_a_clean_closed_loop() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        for bounds in [ArenaBounds::default(), bowl] {
            let outline = bounds.outline(64);
            for (i, p) in outline.iter().enumerate() {
                assert!(
                    p.x.is_finite() && p.y.is_finite(),
                    "{bounds:?} outline[{i}] is not finite: {p:?}"
                );
            }
            for i in 0..outline.len() {
                let a = outline[i];
                let b = outline[(i + 1) % outline.len()];
                assert!(
                    a.distance(b) > 1e-3,
                    "{bounds:?} has a degenerate edge at {i}: {a:?} -> {b:?}"
                );
            }
        }
    }

    /// The outline must wind consistently counter-clockwise, since the wall
    /// segments derive their facing from edge direction and the floor fan derives
    /// its triangle winding (and therefore its normals) the same way. A mixed
    /// winding would render parts of the floor or walls inside-out.
    #[test]
    fn outline_winds_counter_clockwise() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        for bounds in [ArenaBounds::default(), bowl] {
            let outline = bounds.outline(64);
            // Shoelace formula: positive area == counter-clockwise.
            let mut area = 0.0_f32;
            for i in 0..outline.len() {
                let a = outline[i];
                let b = outline[(i + 1) % outline.len()];
                area += a.x * b.y - b.x * a.y;
            }
            assert!(
                area > 0.0,
                "{bounds:?} outline winds clockwise (signed area {area})"
            );
        }
    }

    /// Even-odd point-in-polygon test, for asserting on the outline polygon
    /// itself. Note this is a different polygon from the walkable region:
    /// `contains` tests the gameplay bounds, which are INSET from the outline by
    /// `WALL_OFFSET`, so the two disagree in the sliver between them by design.
    fn point_in_polygon(poly: &[Vec2], p: Vec2) -> bool {
        let mut inside = false;
        let n = poly.len();
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let straddles = (a.y > p.y) != (b.y > p.y);
            if straddles {
                let t = (p.y - a.y) / (b.y - a.y);
                if p.x < a.x + t * (b.x - a.x) {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// The outline polygon must be star-shaped about the origin, which is the
    /// precondition for triangulating the floor as a fan from the centre (see
    /// `create_arena_floor_mesh`). If a future shape adds an off-axis recess this
    /// test fails and the floor needs a real triangulator instead.
    #[test]
    fn outline_is_star_shaped_about_the_origin() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        for bounds in [ArenaBounds::default(), bowl] {
            let outline = bounds.outline(64);
            for p in &outline {
                // Every interior sample along origin -> vertex must lie inside the
                // outline, or the fan triangle for that edge would cover area
                // outside the arena.
                for step in 1..20 {
                    let s = *p * (step as f32 / 20.0);
                    assert!(
                        point_in_polygon(&outline, s),
                        "{bounds:?} is not star-shaped: origin -> {p:?} leaves the \
                         outline at {s:?}"
                    );
                }
            }
        }
    }

    /// Every outline vertex must sit outside the gameplay bounds (walls enclose
    /// the walkable region) — a sign error in the offset would put walls inside
    /// the playable floor.
    #[test]
    fn outline_encloses_the_gameplay_region() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 60.0,
            semi_z: 60.0,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        for bounds in [ArenaBounds::default(), bowl] {
            let outline = bounds.outline(48);
            assert!(outline.len() >= 8);
            for p in &outline {
                // Nudge inward by slightly more than the offset and it must be
                // back inside; the vertex itself must be outside.
                assert!(
                    !bounds.contains(Vec3::new(p.x, 1.0, p.y)),
                    "{bounds:?} outline vertex {p:?} is inside the gameplay bounds"
                );
            }
        }
    }
}
