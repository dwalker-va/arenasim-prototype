//! Annotated top-down arena layout view — the fast iteration surface for tuning
//! map dimensions.
//!
//! Tuning a map by launching the client and flying the camera around is a slow
//! loop. [`draw_arena_layout`] is a pure `egui` painter (no Bevy ECS, no
//! textures) that draws a map's real geometry from above with its dimensions
//! *measured off that geometry* and labelled, so the numbers on screen cannot
//! drift from what the sim actually uses. `tests/arena_layout_snapshot.rs`
//! renders it offscreen to a PNG in a fraction of a second.
//!
//! ## Loop
//! ```bash
//! # Render every map; writes tests/snapshots/arena_layout_<map>.new.png
//! cargo test --release --test arena_layout_snapshot -- --ignored
//! # ...open the PNG, edit assets/config/maps.ron, repeat. No rebuild needed:
//! # the geometry is RON data, so only the render step re-runs.
//!
//! # Once the layout is right, bless the baselines so the test guards them:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test arena_layout_snapshot -- --ignored
//! ```
//!
//! Measurements are *derived*, never passed in: pillar spacings come from the
//! actual volume centers, and wall clearance is found by marching outward from
//! each pillar until the point leaves the bounds. So if a RON edit produces a
//! layout that does not match intent, the labels say so.

use bevy::prelude::*;
use bevy_egui::egui;

use super::match_config::ArenaMap;
use super::play_match::arena_bounds::{outline_half_extents, ArenaBounds, WALL_OFFSET};
use super::play_match::map_config::MapGeometryConfig;
use super::play_match::map_geometry::{prism_vertices_world, ObstacleVolume};

/// Angular resolution for the wall-clearance probe.
const CLEARANCE_PROBE_DIRECTIONS: u32 = 360;
/// Radial step for the wall-clearance probe (yards). Fine enough that a reported
/// clearance is accurate to well under a yard.
const CLEARANCE_PROBE_STEP: f32 = 0.05;

/// A pillar's footprint reduced to what the annotations need.
struct Pillar {
    center: Vec2,
    /// Greatest distance from `center` to the footprint's edge, so the wall
    /// clearance label can report pillar-EDGE to wall as well as centre to wall.
    /// Carried explicitly rather than derived from an outline, because a cylinder
    /// has no vertex list and folding an empty one yields 0 — which silently made
    /// the "from edge" figure equal to the "from centre" figure on TwinPillars.
    edge_radius: f32,
}

/// Shortest distance from `from` to the edge of `bounds`, found by marching
/// outward in many directions and taking the minimum crossing distance.
///
/// Shape-agnostic on purpose — it works for any [`ArenaBounds`] variant present
/// or future, including the non-convex bowl-plus-alcoves union, where a closed
/// form would need special-casing per shape.
fn distance_to_wall(bounds: &ArenaBounds, from: Vec2, max_radius: f32) -> Option<f32> {
    if !bounds.contains(Vec3::new(from.x, 1.0, from.y)) {
        return None;
    }
    let mut best = f32::MAX;
    for i in 0..CLEARANCE_PROBE_DIRECTIONS {
        let ang = std::f32::consts::TAU * i as f32 / CLEARANCE_PROBE_DIRECTIONS as f32;
        let dir = Vec2::new(ang.cos(), ang.sin());
        let mut r = 0.0_f32;
        while r < max_radius && r < best {
            r += CLEARANCE_PROBE_STEP;
            let p = from + dir * r;
            if !bounds.contains(Vec3::new(p.x, 1.0, p.y)) {
                best = best.min(r);
                break;
            }
        }
    }
    (best < f32::MAX).then_some(best)
}

/// Fill an entire egui context with the annotated layout for `map`.
///
/// The harness-facing entry point, mirroring `draw_results_screen`: it owns its
/// panel so callers (and the snapshot test) need no egui types of their own.
pub fn draw_arena_layout_screen(
    ctx: &egui::Context,
    map: ArenaMap,
    map_geometry: &MapGeometryConfig,
) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            draw_arena_layout(ui.painter(), rect, map, map_geometry);
        });
}

/// Draw `map`'s geometry top-down into `rect`, with measured dimension
/// annotations. World `x` maps to the horizontal axis and world `z` to the
/// vertical, preserving aspect ratio.
///
/// Pure painter work, so it renders identically in the offscreen egui harness and
/// (if ever wired up) the real client.
pub fn draw_arena_layout(
    painter: &egui::Painter,
    rect: egui::Rect,
    map: ArenaMap,
    map_geometry: &MapGeometryConfig,
) {
    let active = map_geometry.active_for(map);
    let bounds = active.bounds;

    // Fit the walls (not just the gameplay bounds) plus room for edge labels.
    let outline = bounds.outline(96);
    let half = outline_half_extents(&outline);
    let label_margin = 46.0;
    let inner = rect.shrink(label_margin);
    let scale = (inner.width() / (half.x * 2.0)).min(inner.height() / (half.y * 2.0));
    let center = rect.center();
    let to_screen = |w: Vec2| egui::pos2(center.x + w.x * scale, center.y + w.y * scale);

    let ink = egui::Color32::from_rgb(214, 220, 236);
    let dim = egui::Color32::from_rgb(126, 134, 160);
    let accent = egui::Color32::from_rgb(255, 186, 92);
    let measure = egui::Color32::from_rgb(120, 210, 180);

    painter.rect_filled(rect, 6.0, egui::Color32::from_rgb(15, 16, 22));

    // ---- Floor + walls -----------------------------------------------------
    // The floor is filled as a union of CONVEX pieces, then stroked as one
    // outline. A bowl-with-alcoves outline is concave, and egui's tessellator
    // cannot fill a concave path correctly (`convex_polygon` on one produces
    // spurious triangles across the concavity) — which would put artifacts in the
    // very picture being used to judge the geometry.
    let floor_fill = egui::Color32::from_rgb(31, 33, 44);
    match bounds {
        ArenaBounds::Octagon { .. } => {
            painter.add(egui::Shape::convex_polygon(
                outline.iter().map(|p| to_screen(*p)).collect(),
                floor_fill,
                egui::Stroke::NONE,
            ));
        }
        ArenaBounds::Bowl {
            semi_x,
            semi_z,
            alcove_depth,
            alcove_half_width,
        } => {
            // The bowl proper (convex).
            let (ax, az) = (semi_x + WALL_OFFSET, semi_z + WALL_OFFSET);
            let bowl: Vec<egui::Pos2> = (0..96)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / 96.0;
                    to_screen(Vec2::new(ax * t.cos(), az * t.sin()))
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(
                bowl,
                floor_fill,
                egui::Stroke::NONE,
            ));
            // Each gate alcove (convex), overlapping the bowl so the union is seamless.
            let mouth = alcove_half_width + WALL_OFFSET;
            let far = semi_x + alcove_depth + WALL_OFFSET;
            for sign in [-1.0_f32, 1.0] {
                painter.rect_filled(
                    egui::Rect::from_two_pos(
                        to_screen(Vec2::new(0.0, -mouth)),
                        to_screen(Vec2::new(sign * far, mouth)),
                    ),
                    0.0,
                    floor_fill,
                );
            }
        }
    }
    painter.add(egui::Shape::closed_line(
        outline.iter().map(|p| to_screen(*p)).collect(),
        egui::Stroke::new(2.0, ink),
    ));

    // The gameplay bounds, sampled by testing containment on a grid ray per
    // direction — drawn dashed so the wall//walkable inset is visible.
    let walkable: Vec<egui::Pos2> = (0..192)
        .filter_map(|i| {
            let ang = std::f32::consts::TAU * i as f32 / 192.0;
            let dir = Vec2::new(ang.cos(), ang.sin());
            let mut r = 0.0_f32;
            let mut last = None;
            while r < half.length() * 1.2 {
                let p = dir * r;
                if bounds.contains(Vec3::new(p.x, 1.0, p.y)) {
                    last = Some(p);
                } else if last.is_some() {
                    break;
                }
                r += 0.4;
            }
            last.map(to_screen)
        })
        .collect();
    if walkable.len() > 2 {
        painter.add(egui::Shape::closed_line(
            walkable,
            egui::Stroke::new(1.0, dim),
        ));
    }

    // ---- Center crosshair --------------------------------------------------
    painter.line_segment(
        [to_screen(Vec2::new(-half.x, 0.0)), to_screen(Vec2::new(half.x, 0.0))],
        egui::Stroke::new(0.5, egui::Color32::from_rgb(70, 76, 94)),
    );
    painter.line_segment(
        [to_screen(Vec2::new(0.0, -half.y)), to_screen(Vec2::new(0.0, half.y))],
        egui::Stroke::new(0.5, egui::Color32::from_rgb(70, 76, 94)),
    );

    // ---- Obstacles ---------------------------------------------------------
    let mut pillars: Vec<Pillar> = Vec::new();
    for volume in &active.volumes {
        match *volume {
            ObstacleVolume::Prism {
                center_xz,
                circumradius,
                sides,
                rotation,
                ..
            } => {
                let verts = prism_vertices_world(center_xz, circumradius, sides, rotation);
                painter.add(egui::Shape::convex_polygon(
                    verts.iter().map(|v| to_screen(*v)).collect(),
                    egui::Color32::from_rgb(96, 102, 128),
                    egui::Stroke::new(1.2, egui::Color32::from_rgb(168, 176, 204)),
                ));
                pillars.push(Pillar {
                    center: center_xz,
                    // Vertices sit on the circumcircle, so that IS the edge reach.
                    edge_radius: circumradius,
                });
            }
            ObstacleVolume::Cylinder {
                center_xz, radius, ..
            } => {
                painter.circle(
                    to_screen(center_xz),
                    radius * scale,
                    egui::Color32::from_rgb(96, 102, 128),
                    egui::Stroke::new(1.2, egui::Color32::from_rgb(168, 176, 204)),
                );
                pillars.push(Pillar {
                    center: center_xz,
                    edge_radius: radius,
                });
            }
            ObstacleVolume::Aabb { min, max } => {
                painter.rect(
                    egui::Rect::from_two_pos(
                        to_screen(Vec2::new(min.x, min.z)),
                        to_screen(Vec2::new(max.x, max.z)),
                    ),
                    0.0,
                    egui::Color32::from_rgb(96, 102, 128),
                    egui::Stroke::new(1.2, egui::Color32::from_rgb(168, 176, 204)),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    // ---- Measured dimension annotations ------------------------------------
    let font = egui::FontId::monospace(11.0);
    let label = |w: Vec2, text: String, color: egui::Color32, anchor: egui::Align2| {
        painter.text(to_screen(w), anchor, text, font.clone(), color);
    };

    // Pillar pair spacings, derived from actual centers.
    let mut xs: Vec<f32> = pillars.iter().map(|p| p.center.x).collect();
    let mut zs: Vec<f32> = pillars.iter().map(|p| p.center.y).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    zs.dedup_by(|a, b| (*a - *b).abs() < 0.01);

    // Lateral spacing (within one side's pair), drawn on the -x pair.
    if zs.len() >= 2 && !xs.is_empty() {
        let x = xs[0];
        let (z0, z1) = (zs[0], zs[zs.len() - 1]);
        painter.line_segment(
            [to_screen(Vec2::new(x, z0)), to_screen(Vec2::new(x, z1))],
            egui::Stroke::new(1.5, measure),
        );
        label(
            Vec2::new(x, (z0 + z1) * 0.5),
            format!(" {:.0} yd lateral", (z1 - z0).abs()),
            measure,
            egui::Align2::LEFT_CENTER,
        );
    }

    // Long-axis spacing (near pair to far pair), drawn along z = min.
    if xs.len() >= 2 && !zs.is_empty() {
        let z = zs[0];
        let (x0, x1) = (xs[0], xs[xs.len() - 1]);
        painter.line_segment(
            [to_screen(Vec2::new(x0, z)), to_screen(Vec2::new(x1, z))],
            egui::Stroke::new(1.5, measure),
        );
        label(
            Vec2::new((x0 + x1) * 0.5, z),
            format!("{:.0} yd gate-to-gate", (x1 - x0).abs()),
            measure,
            egui::Align2::CENTER_BOTTOM,
        );
    }

    // Wall clearance, probed from each pillar center (report the nearest).
    let max_probe = half.length() * 1.5;
    if let Some(p) = pillars.first() {
        if let Some(clear) = distance_to_wall(&bounds, p.center, max_probe) {
            // Subtract the footprint so the number reads as pillar-edge to wall
            // as well as center-to-wall.
            label(
                p.center,
                format!(
                    "\n wall {clear:.1} from center\n ({:.1} from edge)",
                    clear - p.edge_radius
                ),
                accent,
                egui::Align2::LEFT_TOP,
            );
        }
    }

    // Overall extents + shape-specific parameters.
    let (shape, params) = match bounds {
        ArenaBounds::Octagon {
            half_x,
            half_z,
            corner_sum,
        } => (
            "Octagon",
            format!("half_x {half_x}  half_z {half_z}  corner_sum {corner_sum}"),
        ),
        ArenaBounds::Bowl {
            semi_x,
            semi_z,
            alcove_depth,
            alcove_half_width,
        } => (
            "Bowl",
            format!(
                "semi_x {semi_x}  semi_z {semi_z}  room {alcove_depth} deep x {} wide",
                alcove_half_width * 2.0
            ),
        ),
    };

    // Gate alcove depth marker.
    if let ArenaBounds::Bowl {
        semi_x,
        alcove_depth,
        alcove_half_width,
        ..
    } = bounds
    {
        painter.line_segment(
            [
                to_screen(Vec2::new(semi_x, alcove_half_width)),
                to_screen(Vec2::new(semi_x + alcove_depth, alcove_half_width)),
            ],
            egui::Stroke::new(1.5, accent),
        );
        label(
            Vec2::new(semi_x + alcove_depth * 0.5, alcove_half_width),
            format!("{alcove_depth:.0} yd room "),
            accent,
            egui::Align2::RIGHT_BOTTOM,
        );
    }

    let extents = bounds.half_extents();
    painter.text(
        rect.left_top() + egui::vec2(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{}   [{}]  {}\nwalkable {:.0} x {:.0} yd   {} pillar(s)",
            map.name(),
            shape,
            params,
            extents.x * 2.0,
            extents.y * 2.0,
            active.volumes.len(),
        ),
        egui::FontId::monospace(12.0),
        ink,
    );
    painter.text(
        rect.right_bottom() + egui::vec2(-8.0, -6.0),
        egui::Align2::RIGHT_BOTTOM,
        "+x right, +z down   dashed = walkable bound, solid = wall",
        egui::FontId::monospace(10.0),
        dim,
    );
}
