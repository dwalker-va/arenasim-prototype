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

use super::map_geometry::{contains_point, ObstacleVolume};
use super::{ARENA_HALF_X, ARENA_HALF_Z};
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
        }
    }
}

/// One map's obstacle set and (optional) cover anchors. `#[serde(default)]` at
/// the container level so a partial RON file (e.g. only `volumes`) fills the
/// rest from defaults.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MapDef {
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
    pub test_verticality: MapDef,
}

impl Default for MapGeometryConfig {
    fn default() -> Self {
        Self {
            // BasicArena: no obstacles.
            basic_arena: MapDef::default(),
            // PillaredArena: two full-height cylinders, mirrored about
            // x=0, radius 2.5, floor to y=5.
            pillared_arena: MapDef {
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

impl MapGeometryConfig {
    /// The [`MapDef`] for a given [`ArenaMap`].
    fn map_def(&self, map: ArenaMap) -> &MapDef {
        match map {
            ArenaMap::BasicArena => &self.basic_arena,
            ArenaMap::PillaredArena => &self.pillared_arena,
            ArenaMap::TestVerticality => &self.test_verticality,
        }
    }

    /// Derive the [`ActiveMapGeometry`] resource for the selected map,
    /// converting RON `VolumeDef`s into analytic [`ObstacleVolume`]s.
    pub fn active_for(&self, map: ArenaMap) -> ActiveMapGeometry {
        let def = self.map_def(map);
        ActiveMapGeometry {
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

/// Whether an XZ point lies within the arena's rectangular gameplay bounds.
fn in_arena_bounds(x: f32, z: f32) -> bool {
    x >= -ARENA_HALF_X && x <= ARENA_HALF_X && z >= -ARENA_HALF_Z && z <= ARENA_HALF_Z
}

/// Validate one map's volumes and cover anchors, pushing every violation.
fn validate_map(map: &str, def: &MapDef, issues: &mut Vec<String>) {
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
                    && (!in_arena_bounds(center.0 - radius, center.1 - radius)
                        || !in_arena_bounds(center.0 + radius, center.1 + radius))
                {
                    issues.push(format!(
                        "{map}.volumes[{i}] cylinder center {center:?} ± radius {radius} extends \
                         outside arena bounds (±{ARENA_HALF_X} x, ±{ARENA_HALF_Z} z)"
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
                if finite && (!in_arena_bounds(min.0, min.2) || !in_arena_bounds(max.0, max.2)) {
                    issues.push(format!(
                        "{map}.volumes[{i}] box [{min:?}, {max:?}] extends outside arena bounds \
                         (±{ARENA_HALF_X} x, ±{ARENA_HALF_Z} z)"
                    ));
                }
            }
        }
    }

    for (i, &(x, z)) in def.cover_anchors.iter().enumerate() {
        if !in_arena_bounds(x, z) {
            issues.push(format!(
                "{map}.cover_anchors[{i}] ({x}, {z}) is outside arena bounds"
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

    /// Scenario 3: a pillar centered outside arena bounds is rejected.
    #[test]
    fn validate_rejects_out_of_bounds_pillar() {
        let mut config = MapGeometryConfig::default();
        config.pillared_arena.volumes = vec![VolumeDef::Cylinder {
            center: (ARENA_HALF_X + 5.0, 0.0),
            radius: 2.5,
            base_y: 0.0,
            height: 5.0,
        }];
        let issues = config
            .validate()
            .expect_err("out-of-bounds pillar must fail validation");
        assert!(
            issues.iter().any(|i| i.contains("pillared_arena.volumes[0]")
                && i.contains("outside arena bounds")),
            "issues should flag the out-of-bounds pillar: {:?}",
            issues
        );
    }

    /// A cover anchor inside a volume is rejected.
    #[test]
    fn validate_rejects_cover_anchor_inside_volume() {
        let mut config = MapGeometryConfig::default();
        config.pillared_arena.cover_anchors = vec![(9.0, 0.0)]; // dead center of a pillar
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
        // PillaredArena is untouched → default two cylinders.
        assert_eq!(config.pillared_arena.volumes.len(), 2);
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
    fn pillared_arena_has_two_mirrored_cylinders() {
        let config = load_map_geometry_config().expect("maps.ron must load");
        let active = config.active_for(ArenaMap::PillaredArena);
        assert_eq!(active.volumes.len(), 2, "expected exactly two pillars");
        let mut centers: Vec<f32> = active
            .volumes
            .iter()
            .map(|v| match v {
                ObstacleVolume::Cylinder { center_xz, radius, .. } => {
                    assert_eq!(*radius, 2.5, "pillar radius");
                    assert_eq!(center_xz.y, 0.0, "pillar z");
                    center_xz.x
                }
                other => panic!("expected a cylinder, got {:?}", other),
            })
            .collect();
        centers.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(centers, vec![-9.0, 9.0], "mirrored pillar x-centers");
    }

    /// BasicArena resolves to an empty obstacle set.
    #[test]
    fn basic_arena_has_no_obstacles() {
        let config = load_map_geometry_config().expect("maps.ron must load");
        let active = config.active_for(ArenaMap::BasicArena);
        assert!(active.volumes.is_empty());
    }
}
