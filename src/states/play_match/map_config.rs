//! Data-Driven Map Geometry Configuration (line-of-sight obstacles)
//!
//! Mirrors the `movement_config.rs` loading pattern exactly: serde structs with
//! `#[serde(default)]`, direct `std::fs::read_to_string` + `ron::from_str` (no
//! asset server — required for headless), `validate()` collecting ALL violations
//! into a `Vec<String>`, a `Resource`, and a plugin that panics on failure. The
//! plugin is registered in BOTH the headless runner (`src/headless/runner.rs`,
//! next to `MovementConfigPlugin`) and the graphical stack (`src/main.rs`) — the
//! dual-mode registration failure class is the most-burned-by bug in this
//! repo's history.
//!
//! Per-map obstacle volumes and cover anchors live in
//! `assets/config/maps.ron`. The full config loads as [`MapGeometryConfig`] at
//! startup; match setup (both modes) derives an [`ActiveMapGeometry`] for the
//! selected [`ArenaMap`] and inserts it so the sim reads exactly the obstacles
//! for the active map.
//!
//! ## Usage
//! ```ignore
//! fn my_system(geometry: Res<ActiveMapGeometry>) {
//!     if has_line_of_sight(&geometry.volumes, from, to) { /* ... */ }
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::arena_bounds::ArenaBounds;
use super::map_geometry::{contains_point, ObstacleVolume};
use crate::states::match_config::ArenaMap;

/// RON-friendly obstacle-volume declaration. Mirrors [`ObstacleVolume`] using
/// plain tuples (glam `Vec2`/`Vec3` are not wired for serde in this crate), and
/// is converted to [`ObstacleVolume`] on load via [`VolumeDef::to_volume`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum VolumeDef {
    /// Vertical finite cylinder: circle of `radius` at XZ `center`, spanning
    /// `y ∈ [base_y, base_y + height]`.
    Cylinder {
        center: (f32, f32),
        radius: f32,
        base_y: f32,
        height: f32,
    },
    /// Closed axis-aligned box spanning `[min, max]` on every axis.
    Aabb {
        min: (f32, f32, f32),
        max: (f32, f32, f32),
    },
    /// Vertical prism over a regular `sides`-gon inscribed in a circle of
    /// `circumradius` at XZ `center`, first vertex at `rotation_deg` degrees,
    /// spanning `y ∈ [base_y, base_y + height]`. Degrees (not radians) because
    /// this is a hand-tuned authoring surface.
    Prism {
        center: (f32, f32),
        circumradius: f32,
        sides: u32,
        rotation_deg: f32,
        base_y: f32,
        height: f32,
    },
}

impl VolumeDef {
    /// Convert to the analytic [`ObstacleVolume`] the geometry math consumes.
    pub fn to_volume(self) -> ObstacleVolume {
        match self {
            VolumeDef::Cylinder {
                center,
                radius,
                base_y,
                height,
            } => ObstacleVolume::Cylinder {
                center_xz: Vec2::new(center.0, center.1),
                radius,
                base_y,
                height,
            },
            VolumeDef::Aabb { min, max } => ObstacleVolume::Aabb {
                min: Vec3::new(min.0, min.1, min.2),
                max: Vec3::new(max.0, max.1, max.2),
            },
            VolumeDef::Prism {
                center,
                circumradius,
                sides,
                rotation_deg,
                base_y,
                height,
            } => ObstacleVolume::Prism {
                center_xz: Vec2::new(center.0, center.1),
                circumradius,
                sides,
                rotation: rotation_deg.to_radians(),
                base_y,
                height,
            },
        }
    }
}

/// One map's obstacle set and (optional) cover anchors. `#[serde(default)]` at
/// the container level so a partial RON file (e.g. only `volumes`) fills the
/// rest from defaults.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MapDef {
    /// The walkable region's shape. Defaults to the historical 76×46 cut-corner
    /// octagon, so a map that omits it behaves exactly as it did before bounds
    /// became per-map data.
    pub bounds: ArenaBounds,
    /// Obstacle volumes in declaration order (walked in order — determinism).
    pub volumes: Vec<VolumeDef>,
    /// Cover anchor points (XZ) hand-authored positions a healer can duck
    /// behind. Optional — defaults to empty.
    pub cover_anchors: Vec<(f32, f32)>,
}

/// The full per-map geometry config, loaded from `assets/config/maps.ron`.
///
/// Keyed by named map fields (like `MovementConfig`'s per-class blocks) so a
/// partial file overriding one map leaves the others at their struct defaults.
#[derive(Resource, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MapGeometryConfig {
    pub basic_arena: MapDef,
    pub pillared_arena: MapDef,
    pub twin_pillars: MapDef,
    pub test_verticality: MapDef,
}

impl Default for MapGeometryConfig {
    fn default() -> Self {
        Self {
            // BasicArena: no obstacles.
            basic_arena: MapDef::default(),
            // PillaredArena: the Nagrand Arena replica. Four octagonal pillars in
            // a 40yd (lateral) × 80yd (gate-to-gate) rectangle inside a ~120yd
            // circular bowl, with a 10yd starting room at each gate. See
            // NAGRAND_* below for where each number comes from.
            pillared_arena: nagrand_arena(),
            // TwinPillars: the original two-cylinder cover map — two full-height
            // pillars mirrored about x=0 in the historical octagon. Preserved
            // verbatim (centres, radius, height, bounds) because the LoS probe
            // suite and the 2026-07-23 balance baseline are calibrated to it.
            twin_pillars: MapDef {
                bounds: ArenaBounds::default(),
                volumes: vec![
                    VolumeDef::Cylinder {
                        center: (9.0, 0.0),
                        radius: 2.5,
                        base_y: 0.0,
                        height: 5.0,
                    },
                    VolumeDef::Cylinder {
                        center: (-9.0, 0.0),
                        radius: 2.5,
                        base_y: 0.0,
                        height: 5.0,
                    },
                ],
                cover_anchors: Vec::new(),
            },
            // TestVerticality: a raised platform (top at y=3), a 3-box stepped
            // ramp ascending to it, and one pillar. Test asset — all inside
            // arena gameplay bounds.
            test_verticality: MapDef {
                // Headless LoS test asset — keeps the original octagon bounds.
                bounds: ArenaBounds::default(),
                volumes: vec![
                    // Raised platform, top surface at y=3.
                    VolumeDef::Aabb {
                        min: (4.0, 0.0, -6.0),
                        max: (16.0, 3.0, 6.0),
                    },
                    // Stepped ramp: three ascending boxes leading onto the
                    // platform (heights 1, 2, 3).
                    VolumeDef::Aabb {
                        min: (-4.0, 0.0, -3.0),
                        max: (-1.0, 1.0, 3.0),
                    },
                    VolumeDef::Aabb {
                        min: (-1.0, 0.0, -3.0),
                        max: (2.0, 2.0, 3.0),
                    },
                    VolumeDef::Aabb {
                        min: (2.0, 0.0, -3.0),
                        max: (4.0, 3.0, 3.0),
                    },
                    // A single full-height pillar off to the side.
                    VolumeDef::Cylinder {
                        center: (-12.0, 0.0),
                        radius: 2.0,
                        base_y: 0.0,
                        height: 5.0,
                    },
                ],
                cover_anchors: Vec::new(),
            },
        }
    }
}

// ============================================================================
// Nagrand Arena dimensions
//
// Every number here is a tuning knob: `assets/config/maps.ron` overrides them
// without a rebuild, and `tests/arena_layout_snapshot.rs` renders the result
// annotated so the layout can be eyeballed against a reference screenshot.
// These Rust values are the fallback when the RON file omits the map.
// ============================================================================

/// Pillar center offset along x (gate-to-gate axis). The two pillars on one side
/// of the arena sit at ±`NAGRAND_PILLAR_Z`, and the far pair mirrors across x=0,
/// giving 2 × 40 = 80yd between the near and far pairs.
const NAGRAND_PILLAR_X: f32 = 40.0;

/// Pillar center offset along z. The pair on each side is 2 × 20 = 40yd apart.
const NAGRAND_PILLAR_Z: f32 = 20.0;

/// Pillar circumradius (12yd across). Set by COVER DENSITY, not by looks: over
/// random combat-range (<30yd) sightlines, r=2.5 blocks 4.3%, r=4.0 blocks 7.5%,
/// r=6.0 blocks 11.9%. The old 2-cylinder PillaredArena sat at 12.7%, so this
/// preserves roughly today's amount of line-of-sight play. Pillar *spacing*
/// barely moves cover at all (4.3-5.2% from 24yd to 80yd apart), so this is the
/// knob to reach for. See `assets/config/maps.ron` for the full note.
const NAGRAND_PILLAR_RADIUS: f32 = 6.0;

/// Pillars are octagonal in Nagrand, not round.
const NAGRAND_PILLAR_SIDES: u32 = 8;

/// Half-step turn (360/8/2), so a flat face — not a vertex — points down each
/// axis. This is gameplay-relevant: it pulls the pillar's axial reach in from the
/// circumradius (6.0) to the apothem (≈5.54), which changes which grazing
/// sightlines the pillar blocks.
const NAGRAND_PILLAR_ROTATION_DEG: f32 = 22.5;

/// Pillar height. Full-height cover, as the old cylinders were.
const NAGRAND_PILLAR_HEIGHT: f32 = 5.0;

/// Bowl radius: the pillar at (40, 20) is √2000 ≈ 44.72yd from center, and the
/// wall sits ~15yd beyond it at its nearest (radial) point.
const NAGRAND_BOWL_RADIUS: f32 = 59.72;

/// Starting-room depth beyond the bowl wall.
const NAGRAND_ROOM_DEPTH: f32 = 10.0;

/// Starting-room half-width. Wide enough for a 3v3 line abreast at the 3yd slot
/// spacing `setup_play_match` uses.
const NAGRAND_ROOM_HALF_WIDTH: f32 = 8.0;

/// The Nagrand Arena replica: a circular bowl with two gate alcoves, and four
/// octagonal pillars in a symmetric rectangle.
fn nagrand_arena() -> MapDef {
    let pillar = |x: f32, z: f32| VolumeDef::Prism {
        center: (x, z),
        circumradius: NAGRAND_PILLAR_RADIUS,
        sides: NAGRAND_PILLAR_SIDES,
        rotation_deg: NAGRAND_PILLAR_ROTATION_DEG,
        base_y: 0.0,
        height: NAGRAND_PILLAR_HEIGHT,
    };
    MapDef {
        bounds: ArenaBounds::Bowl {
            semi_x: NAGRAND_BOWL_RADIUS,
            semi_z: NAGRAND_BOWL_RADIUS,
            alcove_depth: NAGRAND_ROOM_DEPTH,
            alcove_half_width: NAGRAND_ROOM_HALF_WIDTH,
        },
        // Declaration order is deterministic and load-bearing (slice-order tie
        // breaks in steering / nearest-blocker); listed -x pair first, then +x.
        volumes: vec![
            pillar(-NAGRAND_PILLAR_X, -NAGRAND_PILLAR_Z),
            pillar(-NAGRAND_PILLAR_X, NAGRAND_PILLAR_Z),
            pillar(NAGRAND_PILLAR_X, -NAGRAND_PILLAR_Z),
            pillar(NAGRAND_PILLAR_X, NAGRAND_PILLAR_Z),
        ],
        cover_anchors: Vec::new(),
    }
}

impl MapGeometryConfig {
    /// The [`MapDef`] for a given [`ArenaMap`].
    fn map_def(&self, map: ArenaMap) -> &MapDef {
        match map {
            ArenaMap::BasicArena => &self.basic_arena,
            ArenaMap::PillaredArena => &self.pillared_arena,
            ArenaMap::TwinPillars => &self.twin_pillars,
            ArenaMap::TestVerticality => &self.test_verticality,
        }
    }

    /// Derive the [`ActiveMapGeometry`] resource for the selected map,
    /// converting RON `VolumeDef`s into analytic [`ObstacleVolume`]s.
    pub fn active_for(&self, map: ArenaMap) -> ActiveMapGeometry {
        let def = self.map_def(map);
        ActiveMapGeometry {
            bounds: def.bounds,
            volumes: def.volumes.iter().map(|v| v.to_volume()).collect(),
            cover_anchors: def
                .cover_anchors
                .iter()
                .map(|&(x, z)| Vec2::new(x, z))
                .collect(),
        }
    }

    /// Check value sanity across every map. Returns the list of violations on
    /// failure (all collected, not just the first).
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut issues: Vec<String> = Vec::new();
        for (name, def) in [
            ("basic_arena", &self.basic_arena),
            ("pillared_arena", &self.pillared_arena),
            ("twin_pillars", &self.twin_pillars),
            ("test_verticality", &self.test_verticality),
        ] {
            validate_map(name, def, &mut issues);
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}

/// Whether an XZ point lies within the map's own walkable bounds, at standing
/// height. Per-map now, so a Nagrand pillar at x=40 is legal there while still
/// being rejected on the smaller octagon maps.
fn in_arena_bounds(bounds: &ArenaBounds, x: f32, z: f32) -> bool {
    bounds.contains(Vec3::new(x, 1.0, z))
}

/// Validate a map's [`ArenaBounds`], pushing every violation.
///
/// `bounds` became RON-authored data with the Nagrand rework, so it needs the same
/// startup gate the volumes already had. Degenerate values are not merely ugly:
/// a zero `corner_sum` makes [`ArenaBounds::edge_closeness`] divide by zero and
/// feed NaN into the movement scorer (which then silently picks the last
/// candidate), and a non-positive `half_x` inverts `team_spawn_x` so the two teams
/// spawn on each other's side of the arena.
fn validate_bounds(map: &str, bounds: &ArenaBounds, issues: &mut Vec<String>) {
    /// Push an issue unless `v` is a usable dimension. Returns whether it is, so
    /// the cross-field checks below can skip comparing garbage.
    fn positive(map: &str, name: &str, v: f32, issues: &mut Vec<String>) -> bool {
        if v > 0.0 && v.is_finite() {
            return true;
        }
        issues.push(format!(
            "{map}.bounds {name} must be positive and finite, got {v}"
        ));
        false
    }
    match *bounds {
        ArenaBounds::Octagon {
            half_x,
            half_z,
            corner_sum,
        } => {
            let hx = positive(map, "octagon half_x", half_x, issues);
            let hz = positive(map, "octagon half_z", half_z, issues);
            let cs = positive(map, "octagon corner_sum", corner_sum, issues);
            // The chamfer constraint |x| + |z| <= corner_sum must leave a region
            // at least as wide as the longer axis, or the diagonals cut the arena
            // down to a sliver (or away entirely).
            if hx && hz && cs && corner_sum <= half_x.max(half_z) {
                issues.push(format!(
                    "{map}.bounds octagon corner_sum {corner_sum} must exceed \
                     max(half_x, half_z) = {}, or the corner chamfers cut the arena away",
                    half_x.max(half_z)
                ));
            }
        }
        ArenaBounds::Bowl {
            semi_x,
            semi_z,
            alcove_depth,
            alcove_half_width,
        } => {
            positive(map, "bowl semi_x", semi_x, issues);
            let sz = positive(map, "bowl semi_z", semi_z, issues);
            positive(map, "bowl alcove_depth", alcove_depth, issues);
            let aw = positive(map, "bowl alcove_half_width", alcove_half_width, issues);
            // The gate mouths must be strictly narrower than the bowl, or
            // `outline` degenerates: the arc between the two mouths vanishes and
            // the floor fan/wall loop collapse onto coincident points.
            if sz && aw && alcove_half_width >= semi_z {
                issues.push(format!(
                    "{map}.bounds bowl alcove_half_width {alcove_half_width} must be less than \
                     semi_z {semi_z}, or the gate mouths swallow the bowl wall"
                ));
            }
        }
    }
}

/// Validate one map's bounds, volumes, and cover anchors, pushing every violation.
fn validate_map(map: &str, def: &MapDef, issues: &mut Vec<String>) {
    let bounds = &def.bounds;
    validate_bounds(map, bounds, issues);
    for (i, volume) in def.volumes.iter().enumerate() {
        match *volume {
            VolumeDef::Cylinder {
                center,
                radius,
                base_y,
                height,
            } => {
                if !(radius > 0.0) || !radius.is_finite() {
                    issues.push(format!(
                        "{map}.volumes[{i}] cylinder radius must be positive and finite, got {radius}"
                    ));
                }
                if !(height > 0.0) || !height.is_finite() {
                    issues.push(format!(
                        "{map}.volumes[{i}] cylinder height must be positive and finite, got {height}"
                    ));
                }
                if !base_y.is_finite() {
                    issues.push(format!(
                        "{map}.volumes[{i}] cylinder base_y must be finite, got {base_y}"
                    ));
                }
                if radius.is_finite()
                    && (!in_arena_bounds(bounds, center.0 - radius, center.1 - radius)
                        || !in_arena_bounds(bounds, center.0 + radius, center.1 + radius))
                {
                    issues.push(format!(
                        "{map}.volumes[{i}] cylinder center {center:?} ± radius {radius} extends \
                         outside {map} bounds {bounds:?}"
                    ));
                }
            }
            VolumeDef::Aabb { min, max } => {
                if !(max.0 > min.0 && max.1 > min.1 && max.2 > min.2) {
                    issues.push(format!(
                        "{map}.volumes[{i}] box max {max:?} must exceed min {min:?} on every axis"
                    ));
                }
                let finite = min.0.is_finite()
                    && min.1.is_finite()
                    && min.2.is_finite()
                    && max.0.is_finite()
                    && max.1.is_finite()
                    && max.2.is_finite();
                if !finite {
                    issues.push(format!(
                        "{map}.volumes[{i}] box min {min:?} / max {max:?} must be finite"
                    ));
                }
                if finite && (!in_arena_bounds(bounds, min.0, min.2) || !in_arena_bounds(bounds, max.0, max.2)) {
                    issues.push(format!(
                        "{map}.volumes[{i}] box [{min:?}, {max:?}] extends \
                         outside {map} bounds {bounds:?}"
                    ));
                }
            }
            VolumeDef::Prism {
                center,
                circumradius,
                sides,
                rotation_deg,
                base_y,
                height,
            } => {
                if sides < 3 {
                    issues.push(format!(
                        "{map}.volumes[{i}] prism needs at least 3 sides, got {sides}"
                    ));
                }
                if !(circumradius > 0.0) || !circumradius.is_finite() {
                    issues.push(format!(
                        "{map}.volumes[{i}] prism circumradius must be positive and finite, \
                         got {circumradius}"
                    ));
                }
                if !(height > 0.0) || !height.is_finite() {
                    issues.push(format!(
                        "{map}.volumes[{i}] prism height must be positive and finite, got {height}"
                    ));
                }
                if !base_y.is_finite() {
                    issues.push(format!(
                        "{map}.volumes[{i}] prism base_y must be finite, got {base_y}"
                    ));
                }
                if !rotation_deg.is_finite() {
                    issues.push(format!(
                        "{map}.volumes[{i}] prism rotation_deg must be finite, got {rotation_deg}"
                    ));
                }
                // Bounded by the circumcircle, so the cylinder check applies.
                if circumradius.is_finite()
                    && (!in_arena_bounds(bounds, center.0 - circumradius, center.1 - circumradius)
                        || !in_arena_bounds(bounds, center.0 + circumradius, center.1 + circumradius))
                {
                    issues.push(format!(
                        "{map}.volumes[{i}] prism center {center:?} ± circumradius {circumradius} \
                         extends outside {map} bounds {bounds:?}"
                    ));
                }
            }
        }
    }

    for (i, &(x, z)) in def.cover_anchors.iter().enumerate() {
        if !in_arena_bounds(bounds, x, z) {
            issues.push(format!(
                "{map}.cover_anchors[{i}] ({x}, {z}) is outside {map} bounds {bounds:?}"
            ));
        }
        // A cover anchor is a standing position (ground unit at y≈1.0); it must
        // not sit inside any obstacle of the same map.
        let p = Vec3::new(x, 1.0, z);
        for (vi, volume) in def.volumes.iter().enumerate() {
            if contains_point(&volume.to_volume(), p) {
                issues.push(format!(
                    "{map}.cover_anchors[{i}] ({x}, {z}) lies inside volumes[{vi}]"
                ));
            }
        }
    }
}

/// The obstacle geometry for the active match's selected map. Inserted at match
/// setup (both modes) from [`MapGeometryConfig::active_for`].
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ActiveMapGeometry {
    /// The active map's walkable region. Read by the movement clamp, the scorer's
    /// boundary mask, and the AI's point-placement helpers.
    pub bounds: ArenaBounds,
    /// Obstacle volumes, in declaration order (deterministic slice walks).
    pub volumes: Vec<ObstacleVolume>,
    /// Cover anchor positions (XZ).
    pub cover_anchors: Vec<Vec2>,
}

/// Parse a map geometry config from RON text. `source` names the origin for
/// error messages (a path, or "inline" in tests).
pub fn parse_map_geometry_config(contents: &str, source: &str) -> Result<MapGeometryConfig, String> {
    let config: MapGeometryConfig =
        ron::from_str(contents).map_err(|e| format!("Failed to parse {}: {}", source, e))?;

    config.validate().map_err(|issues| {
        format!("Invalid map geometry config in {}:\n  {}", source, issues.join("\n  "))
    })?;

    Ok(config)
}

/// Load and validate a map geometry config from a RON file path.
pub fn load_map_geometry_config_from(path: &str) -> Result<MapGeometryConfig, String> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
    parse_map_geometry_config(&contents, path)
}

/// Load map geometry configuration from assets/config/maps.ron
pub fn load_map_geometry_config() -> Result<MapGeometryConfig, String> {
    let config_path = "assets/config/maps.ron";
    let config = load_map_geometry_config_from(config_path)?;
    info!("Loaded map geometry configuration from {}", config_path);
    Ok(config)
}

/// Bevy plugin for map geometry configuration loading.
///
/// Must be registered in BOTH `src/headless/runner.rs` (next to
/// `MovementConfigPlugin`) and `src/main.rs` (graphical plugin tuple).
pub struct MapConfigPlugin;

impl Plugin for MapConfigPlugin {
    fn build(&self, app: &mut App) {
        match load_map_geometry_config() {
            Ok(config) => {
                app.insert_resource(config);
            }
            Err(e) => {
                // Panic to ensure the config is always valid at startup —
                // same policy as MovementConfigPlugin.
                panic!("Failed to load map geometry configuration: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scenario 1: the shipped maps.ron loads, parses, and validates.
    #[test]
    fn shipped_maps_ron_loads_and_validates() {
        let config =
            load_map_geometry_config().expect("assets/config/maps.ron must load and validate");
        // BasicArena has no obstacles.
        assert!(config.basic_arena.volumes.is_empty());
    }

    /// Scenario 2: malformed RON errors (the plugin panics with this string)
    /// and the message names the source.
    #[test]
    fn malformed_ron_yields_parse_error() {
        let err = parse_map_geometry_config("(pillared_arena: (volumes: [not valid]))", "inline")
            .expect_err("malformed RON must fail");
        assert!(err.contains("Failed to parse inline"), "got: {}", err);
    }

    /// Missing file → loader error naming the path. The plugin panics with this
    /// exact string, so testing the loader covers the panic path.
    #[test]
    fn missing_file_yields_clear_error() {
        let err = load_map_geometry_config_from("assets/config/does_not_exist.ron")
            .expect_err("missing file must fail");
        assert!(
            err.contains("Failed to read assets/config/does_not_exist.ron"),
            "error should name the missing path: {}",
            err
        );
    }

    /// Scenario 3: a pillar centered outside the map's own bounds is rejected.
    /// Bounds are per-map now, so this deliberately exceeds Nagrand's bowl
    /// (radius ≈59.7 + a 10yd room) rather than the retired global constant.
    #[test]
    fn validate_rejects_out_of_bounds_pillar() {
        let mut config = MapGeometryConfig::default();
        config.pillared_arena.volumes = vec![VolumeDef::Cylinder {
            center: (200.0, 0.0),
            radius: 2.5,
            base_y: 0.0,
            height: 5.0,
        }];
        let issues = config
            .validate()
            .expect_err("out-of-bounds pillar must fail validation");
        assert!(
            issues.iter().any(|i| i.contains("pillared_arena.volumes[0]")
                && i.contains("outside pillared_arena bounds")),
            "issues should flag the out-of-bounds pillar: {:?}",
            issues
        );
    }

    /// Degenerate `bounds` must be rejected at startup like any other bad map
    /// data. A zero `corner_sum` in particular makes `edge_closeness` divide by
    /// zero and feeds NaN into the movement scorer, which is invisible at runtime.
    #[test]
    fn validate_rejects_degenerate_bounds() {
        let cases: Vec<(ArenaBounds, &str)> = vec![
            (
                ArenaBounds::Octagon {
                    half_x: 36.5,
                    half_z: 21.5,
                    corner_sum: 0.0,
                },
                "octagon corner_sum",
            ),
            (
                ArenaBounds::Octagon {
                    half_x: -36.5,
                    half_z: 21.5,
                    corner_sum: 48.88,
                },
                "octagon half_x",
            ),
            (
                ArenaBounds::Octagon {
                    half_x: 36.5,
                    half_z: 21.5,
                    // Inside the rectangle but below its long axis: the chamfers
                    // would cut the arena down to a sliver.
                    corner_sum: 30.0,
                },
                "must exceed",
            ),
            (
                ArenaBounds::Bowl {
                    semi_x: 59.72,
                    semi_z: 59.72,
                    alcove_depth: 0.0,
                    alcove_half_width: 8.0,
                },
                "bowl alcove_depth",
            ),
            (
                ArenaBounds::Bowl {
                    semi_x: 59.72,
                    semi_z: 8.0,
                    alcove_depth: 10.0,
                    // Mouth as wide as the bowl: `outline` degenerates.
                    alcove_half_width: 8.0,
                },
                "alcove_half_width",
            ),
        ];
        for (bounds, expected) in cases {
            let mut config = MapGeometryConfig::default();
            config.basic_arena.bounds = bounds;
            let issues = match config.validate() {
                Err(issues) => issues,
                Ok(()) => panic!("{bounds:?} must fail validation"),
            };
            assert!(
                issues.iter().any(|i| i.contains("basic_arena") && i.contains(expected)),
                "{bounds:?} should be flagged with {expected:?}: {issues:?}"
            );
        }
    }

    /// TwinPillars is the map the whole line-of-sight probe suite and the
    /// 2026-07-23 balance baseline are calibrated against, so its shipped geometry
    /// is a pinned invariant — "preserved verbatim" needs a test, not a comment.
    #[test]
    fn twin_pillars_geometry_is_preserved_verbatim() {
        let config = load_map_geometry_config().expect("maps.ron must load");
        let active = config.active_for(ArenaMap::TwinPillars);
        assert_eq!(
            active.bounds,
            ArenaBounds::default(),
            "TwinPillars must keep the historical 76x46 octagon"
        );
        let mut centers: Vec<f32> = active
            .volumes
            .iter()
            .map(|v| match v {
                ObstacleVolume::Cylinder {
                    center_xz,
                    radius,
                    base_y,
                    height,
                } => {
                    assert_eq!(*radius, 2.5, "pillar radius");
                    assert_eq!(center_xz.y, 0.0, "pillar z");
                    assert_eq!(*base_y, 0.0, "pillar base");
                    assert_eq!(*height, 5.0, "pillar height");
                    center_xz.x
                }
                other => panic!("TwinPillars pillars must be cylinders, got {other:?}"),
            })
            .collect();
        centers.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(centers, vec![-9.0, 9.0], "mirrored pillar x-centers");
    }

    /// A cover anchor inside a volume is rejected.
    #[test]
    fn validate_rejects_cover_anchor_inside_volume() {
        let mut config = MapGeometryConfig::default();
        // Dead center of a Nagrand pillar.
        config.pillared_arena.cover_anchors = vec![(40.0, 20.0)];
        let issues = config
            .validate()
            .expect_err("cover anchor inside a pillar must fail");
        assert!(
            issues.iter().any(|i| i.contains("cover_anchors[0]") && i.contains("inside")),
            "issues should flag the anchor inside a volume: {:?}",
            issues
        );
    }

    /// The built-in defaults are internally consistent.
    #[test]
    fn defaults_pass_validation() {
        MapGeometryConfig::default()
            .validate()
            .expect("built-in defaults must validate");
    }

    /// Scenario 4: a partial file (one map only) leaves other maps at defaults.
    #[test]
    fn partial_ron_uses_defaults() {
        // Override only BasicArena (give it a volume); the other two maps fall
        // back to their struct defaults.
        let config = parse_map_geometry_config(
            "(basic_arena: (volumes: [Cylinder(center: (0.0, 0.0), radius: 1.0, base_y: 0.0, height: 3.0)]))",
            "inline",
        )
        .expect("partial config must parse");
        assert_eq!(config.basic_arena.volumes.len(), 1);
        // PillaredArena is untouched → default Nagrand layout (four pillars).
        assert_eq!(config.pillared_arena.volumes.len(), 4);
        // ...and BasicArena, which declared no `bounds`, falls back to the
        // historical octagon rather than inheriting anything from Nagrand.
        assert_eq!(config.basic_arena.bounds, ArenaBounds::default());
        // TestVerticality untouched → default geometry present.
        assert!(!config.test_verticality.volumes.is_empty());
    }

    /// Scenario 6: ArenaMap::all() does NOT contain TestVerticality (it must
    /// stay out of the map-select UI).
    #[test]
    fn arena_map_all_excludes_test_verticality() {
        assert!(
            !ArenaMap::all().contains(&ArenaMap::TestVerticality),
            "TestVerticality must not appear in the map-select list"
        );
    }

    /// Scenario 7: PillaredArena's loaded volumes are exactly two cylinders at
    /// (±9, 0), from the shipped file via active_for.
    #[test]
    fn pillared_arena_is_the_nagrand_layout() {
        let config = load_map_geometry_config().expect("maps.ron must load");
        let active = config.active_for(ArenaMap::PillaredArena);
        assert_eq!(active.volumes.len(), 4, "Nagrand has four pillars");

        // Every pillar is an identical octagonal prism; collect their centers.
        let mut centers: Vec<(f32, f32)> = active
            .volumes
            .iter()
            .map(|v| match v {
                ObstacleVolume::Prism {
                    center_xz,
                    circumradius,
                    sides,
                    rotation,
                    ..
                } => {
                    assert_eq!(*sides, 8, "Nagrand pillars are octagonal");
                    // Sized for cover density, not looks — see NAGRAND_PILLAR_RADIUS.
                    assert_eq!(*circumradius, 6.0, "pillar circumradius");
                    // Half-step turn: a face, not a vertex, faces each axis.
                    assert!(
                        (*rotation - 22.5_f32.to_radians()).abs() < 1e-5,
                        "pillar rotation should be a 22.5° half step, got {rotation}"
                    );
                    (center_xz.x, center_xz.y)
                }
                other => panic!("expected an octagonal prism, got {:?}", other),
            })
            .collect();
        centers.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            centers,
            vec![(-40.0, -20.0), (-40.0, 20.0), (40.0, -20.0), (40.0, 20.0)],
            "pillars form a symmetric 40yd (lateral) x 80yd (gate-to-gate) rectangle"
        );

        // The spacings the layout is specified by, asserted directly so a future
        // retune that breaks the intended geometry fails loudly.
        let lateral = (centers[1].1 - centers[0].1).abs();
        let long_axis = (centers[2].0 - centers[0].0).abs();
        assert_eq!(lateral, 40.0, "same-side pillar pair spacing");
        assert_eq!(long_axis, 80.0, "near-pair to far-pair spacing");

        // The bowl clears the pillars by ~15yd at the nearest (radial) point.
        let ArenaBounds::Bowl { semi_x, semi_z, .. } = active.bounds else {
            panic!("Nagrand must use Bowl bounds, got {:?}", active.bounds);
        };
        assert_eq!(semi_x, semi_z, "the bowl is circular");
        let pillar_radius = (40.0_f32.powi(2) + 20.0_f32.powi(2)).sqrt();
        assert!(
            ((semi_x - pillar_radius) - 15.0).abs() < 0.05,
            "wall should clear the pillars by ~15yd, got {}",
            semi_x - pillar_radius
        );
    }

    /// BasicArena resolves to an empty obstacle set.
    #[test]
    fn basic_arena_has_no_obstacles() {
        let config = load_map_geometry_config().expect("maps.ron must load");
        let active = config.active_for(ArenaMap::BasicArena);
        assert!(active.volumes.is_empty());
    }
}
