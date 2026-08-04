//! Play Match Scene - 3D Combat Arena
//!
//! This module handles the active match simulation where combatants battle each other.
//! Inspired by World of Warcraft's combat mechanics, it features:
//!
//! ## Combat System
//! - **Target Acquisition**: Combatants automatically find the nearest alive enemy
//! - **Movement**: Combatants move towards targets if out of range
//! - **Range Mechanics**: Melee attacks require being in melee range (2.5 units)
//! - **Auto-Attacks**: Each combatant attacks when in range, based on attack speed
//! - **Damage & Stats**: Tracks damage dealt/taken for each combatant
//! - **Win Conditions**: Match ends when all combatants of one team are eliminated
//!
//! ## Visual Representation
//! - 3D capsule meshes represent combatants, colored by class
//! - Health bars rendered above each combatant's head using 2D overlay
//! - Combatants rotate to face their targets
//! - Simple arena floor (60x60 plane)
//! - Isometric camera view
//!
//! ## Flow
//! 1. `setup_play_match`: Spawns arena, camera, lights, and combatants from `MatchConfig`
//! 2. Systems run each frame:
//!    - `update_play_match`: Handle ESC key to exit
//!    - `acquire_targets`: Find nearest enemy for each combatant
//!    - `move_to_target`: Move combatants towards targets if out of range
//!    - `combat_auto_attack`: Process attacks when in range, based on attack speed
//!    - `check_match_end`: Detect when match is over, transition to Results
//!    - `render_health_bars`: Draw 2D health bars over 3D combatants
//! 3. `cleanup_play_match`: Despawn all entities when exiting

// Submodules
pub mod abilities;
pub mod ability_config;
pub mod movement_config;
pub mod equipment;
pub mod components;
pub mod camera;
pub mod projectiles;
pub mod rendering;
pub mod auras;
pub mod effects;
pub mod match_flow;
pub mod ai_profile;
pub mod team_plan;
pub mod team_solve;
pub mod arena_bounds;
pub mod map_geometry;
pub mod map_config;
pub mod traps;
pub mod totems;
pub mod combat_ai;
pub mod combat_core;
pub mod shadow_sight;
pub mod systems;
pub mod utils;
pub mod class_ai;
pub mod constants;
pub mod decision_trace;
pub mod selection;

// Re-exports
pub use abilities::*;
pub use ability_config::*;
pub use movement_config::*;
pub use map_config::*;
pub use components::*;
pub use camera::*;
pub use projectiles::*;
pub use rendering::*;
pub use auras::*;
pub use match_flow::*;
pub use combat_ai::*;
pub use combat_core::*;
pub use shadow_sight::*;
pub use utils::*;
pub use constants::*;
pub use effects::*;
pub use traps::*;
pub use totems::*;
pub use class_ai::pet_ai::pet_ai_system;
pub use selection::{
    pick_selected_combatant, sync_selection_ring, follow_selection_ring,
    reset_selection_on_exit, Selection,
};

use bevy::prelude::*;
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::CascadeShadowConfigBuilder;
use bevy::math::Affine2;
use bevy::image::{ImageSampler, ImageSamplerDescriptor, ImageAddressMode};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::render_asset::RenderAssetUsages;
use super::match_config::{self, MatchConfig};
use super::GameState;
use crate::combat::log::{CombatLog, CombatLogEventType};
use equipment::{ItemDefinitions, DefaultLoadouts, ItemSlot, ItemId, resolve_loadout, enforce_two_hand_conflicts, format_loadout};

// ============================================================================
// Helper Functions
// ============================================================================

/// Largest half-extent of the arena the in-match camera and sun were tuned
/// against — the historical 76x46 octagon (`ArenaBounds::default()`).
const REFERENCE_HALF_EXTENT: f32 = 36.5;

/// How far to pull the camera back (and stretch the shadow cascade) for a map
/// bigger than the one those numbers were tuned on.
///
/// Never below 1.0: a smaller map keeps the tuned framing rather than shoving the
/// camera into the floor. Pure, so the framing is testable without a window.
pub(crate) fn arena_view_scale(bounds: &arena_bounds::ArenaBounds) -> f32 {
    (bounds.half_extents().max_element() / REFERENCE_HALF_EXTENT).max(1.0)
}

/// Integer hash (Murmur-style finalizer) for deterministic per-texel noise.
fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Deterministic white noise in 0..1 for a texel coordinate.
fn texel_noise(x: u32, y: u32) -> f32 {
    let h = hash_u32(
        x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663),
    );
    h as f32 / u32::MAX as f32
}

/// Generates a seamless, tileable surface texture procedurally — no asset
/// files. A `base` color (sRGB component space) is baked in as the average so
/// the surface's overall tone is unchanged; the texture adds low-frequency
/// blotches (weathered patches), fine grain, and — for masonry — faint
/// horizontal courses. All noise sources are periodic across the image so it
/// tiles without visible seams: the blotches use integer-frequency sinusoids,
/// the grain is independent per-texel, and the courses divide the image into a
/// whole number of bands (trivially seamless under Repeat wrapping).
///
/// - `blotch_amp` / `grain_amp`: multiplicative variation strength.
/// - `courses`: number of horizontal stone courses across the image height
///   (`0` disables — used by the floor).
pub(crate) fn create_surface_texture(base: [f32; 3], blotch_amp: f32, grain_amp: f32, courses: u32) -> Image {
    const SIZE: u32 = 512;
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f32 / SIZE as f32;
            let v = y as f32 / SIZE as f32;
            let tau = std::f32::consts::TAU;

            // Low-frequency blotches — integer wavenumbers keep it periodic.
            let blotch = 0.50 * (tau * u).sin() * (tau * v).cos()
                + 0.30 * (tau * 2.0 * u + 1.3).sin() * (tau * 2.0 * v + 0.7).cos()
                + 0.20 * (tau * 3.0 * u + 2.1).sin() * (tau * 1.0 * v + 2.4).cos();

            let grain = texel_noise(x, y) - 0.5;

            let mut variation = 1.0 + blotch_amp * blotch + grain_amp * grain;

            // Darken mortar lines between horizontal stone courses.
            if courses > 0 {
                let cv = v * courses as f32;
                let dist_to_line = (cv - cv.round()).abs(); // 0 at a course boundary
                let line = (1.0 - (dist_to_line / 0.06)).clamp(0.0, 1.0); // ramp near line
                variation *= 1.0 - 0.35 * line;
            }

            let variation = variation.clamp(0.6, 1.3);
            let idx = ((y * SIZE + x) * 4) as usize;
            for c in 0..3 {
                data[idx + c] = ((base[c] * variation).clamp(0.0, 1.0) * 255.0) as u8;
            }
            data[idx + 3] = 255;
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Repeat wrapping so the mesh can tile the texture across the surface.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// Arc tessellation for the arena floor and walls. Curved shapes get this many
/// segments around a full revolution; straight-sided shapes ignore it.
///
/// 64 keeps a ~120yd bowl's wall visibly smooth: the bowl outline it produces has
/// ~134 edges (two 63-point arcs plus the two gate mouths) at ~3yd chords, so a
/// wall follows the curve closely. Wall meshes and materials are deduplicated by
/// length in `spawn_arena_environment`, which is what keeps that edge count from
/// turning into 134 distinct assets.
pub(crate) const WALL_ARC_SEGMENTS: usize = 64;

/// Creates a flat floor mesh for an arbitrary arena outline (world-space XZ
/// vertices in counter-clockwise order, from `ArenaBounds::outline`).
///
/// `uv_scale` maps world units to texture space (UV = world_pos * uv_scale),
/// giving square, uniformly-tiled texels regardless of the floor's aspect
/// ratio. Smaller values = the texture repeats more often.
///
/// Triangulated as a fan from the origin. That is valid for every arena shape
/// because they are all **star-shaped about the origin** — the segment from the
/// centre to any boundary point stays inside the arena. This holds for the
/// cut-corner octagon (convex) and, less obviously, for the bowl-with-alcoves:
/// a gate corridor is centred on z=0 and narrower than the bowl, so the ray from
/// the origin to an alcove corner runs down the corridor rather than crossing a
/// wall. A fan would be wrong for a shape with an off-axis recess, so if one is
/// ever added this needs a real triangulator.
pub(crate) fn create_arena_floor_mesh(outline: &[Vec2], uv_scale: f32) -> Mesh {
    let mut positions: Vec<[f32; 3]> = outline.iter().map(|p| [p.x, 0.0, p.y]).collect();
    let center_idx = positions.len() as u32;
    positions.push([0.0, 0.0, 0.0]);

    let n = outline.len() as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(outline.len() * 3);
    for i in 0..n {
        // Wound to face +Y (up), matching the shading normal. A centroid fan over
        // a counter-clockwise XZ outline winds downward if taken naively; the
        // floor material sets `cull_mode: None` so this was not visible, but the
        // mesh should be correct regardless of material.
        indices.extend_from_slice(&[center_idx, (i + 1) % n, i]);
    }

    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    // World-space tiling so texels stay square and uniform regardless of the
    // floor's aspect ratio (Repeat sampler handles wrap).
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .map(|v| [v[0] * uv_scale, v[2] * uv_scale])
        .collect();

    Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::render::render_asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(bevy::render::mesh::Indices::U32(indices))
}

/// Build the mesh for a vertical prism over the XZ polygon `verts` (world-space,
/// edge order), spanning `y ∈ [base_y, base_y + height]`: one quad per side plus
/// a flat top cap.
///
/// `verts` comes from `map_geometry::prism_vertices_world`, the same helper the
/// collision and line-of-sight predicates use, so the rendered pillar is exactly
/// the volume the sim blocks — no second formula to keep in sync.
///
/// Side UVs run `u ∈ [0,1]` around the perimeter (proportional to edge length, so
/// the texture doesn't stretch on uneven polygons) and `v ∈ [0,1]` up the height,
/// matching the convention `spawn_arena_environment` expects: it scales them into
/// square texels via the material's `uv_transform`.
pub(crate) fn create_prism_mesh(verts: &[Vec2], base_y: f32, height: f32) -> Mesh {
    let n = verts.len();
    let top = base_y + height;

    // Cumulative perimeter fraction at each vertex, for non-stretching side UVs.
    let mut cumulative = Vec::with_capacity(n + 1);
    cumulative.push(0.0_f32);
    for i in 0..n {
        let seg = verts[i].distance(verts[(i + 1) % n]);
        cumulative.push(cumulative[i] + seg);
    }
    let perimeter = cumulative[n].max(1e-6);

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 4 + n + 1);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(n * 4 + n + 1);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(n * 4 + n + 1);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6 + n * 3);

    // Side quads. Each gets its own 4 vertices so the outward face normal is flat
    // (shared vertices would smooth the octagon into a cylinder).
    for i in 0..n {
        let a = verts[i];
        let b = verts[(i + 1) % n];
        let edge = b - a;
        // Outward normal of edge a→b for counter-clockwise XZ winding.
        let normal = Vec2::new(edge.y, -edge.x).normalize_or(Vec2::X);
        let nrm = [normal.x, 0.0, normal.y];
        let (u0, u1) = (cumulative[i] / perimeter, cumulative[i + 1] / perimeter);

        let base = positions.len() as u32;
        positions.push([a.x, base_y, a.y]);
        positions.push([b.x, base_y, b.y]);
        positions.push([b.x, top, b.y]);
        positions.push([a.x, top, a.y]);
        for _ in 0..4 {
            normals.push(nrm);
        }
        uvs.push([u0, 0.0]);
        uvs.push([u1, 0.0]);
        uvs.push([u1, 1.0]);
        uvs.push([u0, 1.0]);
        // Reverse winding (0,2,1 / 0,3,2 rather than 0,1,2 / 0,2,3): the outline
        // runs counter-clockwise in the XZ plane, and with Bevy's default
        // FrontFace::Ccw the naive order produces INWARD-facing front faces, so
        // every outward face gets backface-culled and the pillar renders as just
        // its far interior wall. The winding must agree with the shading normal
        // above, which is the outward edge normal.
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }

    // Top cap: triangle fan from the centroid. Planar UVs scaled by the outline's
    // extent so the cap grain roughly matches the sides.
    let centroid = verts.iter().copied().fold(Vec2::ZERO, |acc, v| acc + v) / n as f32;
    let extent = verts
        .iter()
        .map(|v| v.distance(centroid))
        .fold(1e-6_f32, f32::max);
    let center_idx = positions.len() as u32;
    positions.push([centroid.x, top, centroid.y]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push([0.5, 0.5]);
    for (i, v) in verts.iter().enumerate() {
        positions.push([v.x, top, v.y]);
        normals.push([0.0, 1.0, 0.0]);
        let rel = (*v - centroid) / (2.0 * extent);
        uvs.push([rel.x + 0.5, rel.y + 0.5]);
        let cur = center_idx + 1 + i as u32;
        let next = center_idx + 1 + ((i as u32 + 1) % n as u32);
        // Reversed for the same reason as the side quads — a centroid fan over a
        // counter-clockwise XZ outline winds DOWNWARD, against the +Y shading
        // normal, so an un-flipped cap is invisible from above.
        indices.extend_from_slice(&[center_idx, next, cur]);
    }

    Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::render::render_asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(bevy::render::mesh::Indices::U32(indices))
}

/// Vibrant per-class colors used for 3D combatant meshes. Deliberately
/// distinct from `CharacterClass::color()`, the muted UI palette.
pub(crate) fn class_mesh_color(class: match_config::CharacterClass) -> Color {
    match class {
        match_config::CharacterClass::Warrior => Color::srgb(0.9, 0.6, 0.3), // Orange/brown
        match_config::CharacterClass::Mage => Color::srgb(0.3, 0.6, 1.0),    // Bright blue
        match_config::CharacterClass::Rogue => Color::srgb(1.0, 0.9, 0.2),   // Bright yellow
        match_config::CharacterClass::Priest => Color::srgb(0.95, 0.95, 0.95), // White
        match_config::CharacterClass::Warlock => Color::srgb(0.58, 0.41, 0.93), // Purple
        match_config::CharacterClass::Paladin => Color::srgb(0.96, 0.55, 0.73), // Pink (WoW Paladin)
        match_config::CharacterClass::Hunter => Color::srgb(0.67, 0.83, 0.45), // Green (WoW Hunter)
        match_config::CharacterClass::Shaman => Color::srgb(0.0, 0.44, 0.87), // Blue (WoW Shaman)
    }
}

/// Spawns the warm directional "sun" light with 2-cascade shadows grounded to
/// the ~76-unit arena. Returns the entity so the caller can tag it with its
/// own scene marker (PlayMatch vs main-menu backdrop).
pub(crate) fn spawn_arena_sun(commands: &mut Commands, view_scale: f32) -> Entity {
    commands
        .spawn((
            DirectionalLight {
                illuminance: 25000.0,
                color: Color::srgb(1.0, 0.95, 0.85), // Warm golden sunlight
                shadows_enabled: true,
                ..default()
            },
            CascadeShadowConfigBuilder {
                num_cascades: 2,
                // Scaled with the map: 120 was sized for the ~76yd arena, so on
                // the 120yd bowl the far half of the floor fell outside the
                // shadow cascade and units there lost the contact shadow that
                // anchors them to the ground.
                maximum_distance: 120.0 * view_scale,
                ..default()
            }
            .build(),
            Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id()
}

/// Spawns the arena environment geometry: the octagonal dirt floor, the eight
/// chamfered stadium walls, and one cosmetic mesh per obstacle in `obstacles`
/// (the active map's line-of-sight volumes — empty for open maps). The obstacle
/// meshes are purely visual: geometry truth lives in the `ActiveMapGeometry`
/// resource, and these meshes are placed to match it. Returns all spawned
/// entities so the caller can tag them with its own scene marker (PlayMatch vs
/// main-menu backdrop).
pub(crate) fn spawn_arena_environment(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bounds: &arena_bounds::ArenaBounds,
    obstacles: &[map_geometry::ObstacleVolume],
) -> Vec<Entity> {
    let mut entities = Vec::with_capacity(16);

    // Floor and walls are both built from ONE outline, so a curved wall can never
    // drift off the floor edge. The outline is derived from the map's gameplay
    // bounds (offset outward by the wall+buffer inset), which is why the octagon
    // maps still land on exactly the old 38 x 23 / corner-cut-10 shape.
    let outline = bounds.outline(WALL_ARC_SEGMENTS);

    // UV scale tiles the procedural dirt texture ~every 12 world units (square
    // texels), giving the floor grain/variation without an external asset.
    let octagon_mesh = create_arena_floor_mesh(&outline, 1.0 / 12.0);
    // Sandy dirt: blotches + grain, no courses.
    let floor_texture = images.add(create_surface_texture([0.79, 0.66, 0.46], 0.12, 0.06, 0));

    entities.push(
        commands
            .spawn((
                Mesh3d(meshes.add(octagon_mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    // Sandy tone is baked into the texture, so the tint stays white to
                    // avoid double-darkening the baked color.
                    base_color: Color::WHITE,
                    base_color_texture: Some(floor_texture),
                    perceptual_roughness: 0.95, // Matte dirt/sand texture
                    cull_mode: None, // Render both sides
                    ..default()
                })),
            ))
            .id(),
    );

    // Rectangular arena walls with chamfered corners (simplified stadium shape)
    let wall_height = 4.0;
    let wall_thickness = 1.0;

    // Procedural weathered-stone texture for the walls (#8b7355 baked in),
    // with faint horizontal courses so it reads as stacked masonry.
    let wall_texture = images.add(create_surface_texture([0.54, 0.45, 0.33], 0.10, 0.05, 6));

    // One wall segment per outline edge, so any arena shape (straight-sided
    // octagon or tessellated curve) gets walls that exactly follow its floor.
    //
    // A Bevy `Cuboid` is centred at its origin with its length along local +X, so
    // each segment sits at the edge midpoint, rotated about Y to align +X with the
    // edge direction. `atan2(-dz, dx)` is that angle. For the octagon this
    // reproduces the previous eight hand-placed walls: the resulting box for a
    // corner edge differs from the old `PI/4` by exactly 180 degrees, which is the
    // same solid (a cuboid is symmetric under a half turn about its own axis).
    //
    // Meshes AND materials are shared per rounded length: a tessellated curve's
    // arc segments are all the same length, so Nagrand's ~130-edge outline
    // allocates a handful of assets instead of 130 identical cuboids and 130
    // identical materials. That matters twice — every match setup, and every map
    // switch in the Configure Match preview, which rebuilds this scene.
    let mut wall_materials: std::collections::BTreeMap<u32, Handle<StandardMaterial>> =
        std::collections::BTreeMap::new();
    let mut wall_meshes: std::collections::BTreeMap<u32, Handle<Mesh>> =
        std::collections::BTreeMap::new();
    let wall_tile = 6.0;

    for i in 0..outline.len() {
        let a = outline[i];
        let b = outline[(i + 1) % outline.len()];
        let edge = b - a;
        let length = edge.length();
        if length <= 1e-3 {
            continue; // degenerate edge (coincident outline points)
        }
        let mid = (a + b) * 0.5;
        let yaw = (-edge.y).atan2(edge.x);

        // Overlap adjacent segments slightly so a tessellated curve shows no
        // hairline gaps at the joints where the boxes meet at an angle.
        let seg_length = length + wall_thickness * 0.5;

        // Key on tenths of a world unit: identical-length segments share one mesh
        // and one material, and the uv_transform stays correct for each distinct
        // length.
        let key = (seg_length * 10.0).round() as u32;
        let material = wall_materials
            .entry(key)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::WHITE, // tone baked into the texture
                    base_color_texture: Some(wall_texture.clone()),
                    perceptual_roughness: 0.9,
                    uv_transform: Affine2::from_scale(Vec2::new(
                        seg_length / wall_tile,
                        wall_height / wall_tile,
                    )),
                    ..default()
                })
            })
            .clone();
        let mesh = wall_meshes
            .entry(key)
            .or_insert_with(|| meshes.add(Cuboid::new(seg_length, wall_height, wall_thickness)))
            .clone();

        entities.push(
            commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_xyz(mid.x, wall_height / 2.0, mid.y)
                        .with_rotation(Quat::from_rotation_y(yaw)),
                ))
                .id(),
        );
    }


    // Cosmetic obstacle meshes for the active map's line-of-sight volumes
    // (pillars, platforms). Solid stone architecture — reuses the wall texture
    // and roughness so they read as the same masonry family. Positioned to
    // match the analytic volumes exactly: Bevy's Cylinder/Cuboid meshes are
    // centered at their origin, so a volume whose base sits at `base_y` is
    // translated up by half its height. Opaque (no AlphaMode::Add) — these are
    // architecture, not effects.
    for volume in obstacles {
        let (mesh, translation, uv_extent): (Mesh, Vec3, Vec2) = match *volume {
            map_geometry::ObstacleVolume::Cylinder {
                center_xz,
                radius,
                base_y,
                height,
            } => (
                Cylinder::new(radius, height).into(),
                Vec3::new(center_xz.x, base_y + height / 2.0, center_xz.y),
                // Wrap the texture around the circumference and up the side.
                Vec2::new(2.0 * std::f32::consts::PI * radius, height),
            ),
            map_geometry::ObstacleVolume::Aabb { min, max } => {
                let size = max - min;
                (
                    Cuboid::new(size.x, size.y, size.z).into(),
                    (min + max) / 2.0,
                    Vec2::new(size.x, size.y),
                )
            }
            map_geometry::ObstacleVolume::Prism {
                center_xz,
                circumradius,
                sides,
                rotation,
                base_y,
                height,
            } => {
                // Built at world coordinates (not centered on the origin like the
                // Bevy primitives), so the transform is identity — this keeps the
                // mesh vertices literally equal to the collision outline.
                let verts =
                    map_geometry::prism_vertices_world(center_xz, circumradius, sides, rotation);
                let perimeter: f32 = (0..verts.len())
                    .map(|i| verts[i].distance(verts[(i + 1) % verts.len()]))
                    .sum();
                (
                    create_prism_mesh(&verts, base_y, height),
                    Vec3::ZERO,
                    Vec2::new(perimeter, height),
                )
            }
        };

        let obstacle_material = materials.add(StandardMaterial {
            base_color: Color::WHITE, // tone baked into the shared wall texture
            base_color_texture: Some(wall_texture.clone()),
            perceptual_roughness: 0.9,
            uv_transform: Affine2::from_scale(Vec2::new(
                uv_extent.x / wall_tile,
                uv_extent.y / wall_tile,
            )),
            ..default()
        });

        entities.push(
            commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(obstacle_material),
                    Transform::from_translation(translation),
                ))
                .id(),
        );
    }

    entities
}

// ============================================================================
// Setup & Cleanup Systems
// ============================================================================

/// Setup system: Spawns the 3D arena, camera, lighting, and combatants.
/// 
/// This runs once when entering the PlayMatch state.
/// Reads the `MatchConfig` resource to determine team compositions.
pub fn setup_play_match(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut combat_log: ResMut<CombatLog>,
    config: Res<MatchConfig>,
    game_settings: Res<crate::settings::GameSettings>,
    item_defs: Res<ItemDefinitions>,
    default_loadouts: Res<DefaultLoadouts>,
    map_geometry: Res<MapGeometryConfig>,
    // A `--replay` launch pre-inserts these so a recorded seed reproduces
    // exactly. Present => honour them; absent => fresh defaults as normal.
    existing_rng: Option<Res<GameRng>>,
    existing_profile: Option<Res<ai_profile::AiProfile>>,
) {
    info!("Setting up Play Match scene with config: {:?}", *config);

    // Clear combat log for new match
    combat_log.clear();
    combat_log.log(CombatLogEventType::MatchEvent, "Match started!".to_string());

    // Initialize combat panel view (for tabbed Combat Log / Timeline UI)
    commands.insert_resource(CombatPanelView::default());

    // Initialize spell icons resources (for ability timeline)
    commands.insert_resource(SpellIcons::default());
    commands.insert_resource(SpellIconHandles::default());

    // Resolved BEFORE the camera so the camera can be framed to the map. The
    // resource is inserted further down; this is just the lookup.
    let active_map_geometry = map_geometry.active_for(config.map);
    // How much bigger this map is than the arena the camera was tuned for.
    // Nagrand is a ~120yd bowl whose spawn rooms sit at |x| = 64.7, so a camera
    // fixed at the historical (0, 40, 50) framed an empty floor while both teams
    // walked in off-screen for the whole 10s countdown. The preview camera in
    // configure_match_ui was made map-aware when the bowl landed; this one was
    // missed.
    let view_scale = arena_view_scale(&active_map_geometry.bounds);

    // Spawn 3D camera with isometric-ish view.
    // HDR + tonemapping + bloom let the pre-scaled emissive effects (shields,
    // heal columns, traps, drain beams — all authored at 2-4x) actually glow
    // instead of clipping to flat white.
    commands.spawn((
        Camera3d::default(),
        Camera {
            hdr: true,
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Transform::from_xyz(0.0, 40.0 * view_scale, 50.0 * view_scale)
            .looking_at(Vec3::ZERO, Vec3::Y),
        ArenaCamera,
        PlayMatchEntity,
    ));

    // Add directional light (sun-like) - warm golden sunlight.
    // Shadows grounded to the ~76-unit arena via a 2-cascade config so units
    // cast contact shadows that anchor them to the floor.
    let sun = spawn_arena_sun(&mut commands, view_scale);
    commands.entity(sun).insert(PlayMatchEntity);

    // Add ambient light for overall scene brightness - warm atmospheric glow.
    // Kept low so the directional light + shadows carry the contrast and the
    // emissive effects pop under bloom (was 400.0, which flattened everything).
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.9, 0.85, 0.7), // Warm peachy ambient light
        brightness: 250.0,
        affects_lightmapped_meshes: true,
    });

    // Deep cool background so the warm sandy arena reads against a cohesive
    // backdrop instead of Bevy's default flat gray.
    commands.insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.09)));
    
    // Initialize simulation speed control
    commands.insert_resource(SimulationSpeed { multiplier: 1.0 });
    
    // Initialize camera controller
    commands.insert_resource(CameraController::default());
    
    // Initialize match countdown (10 seconds before gates open)
    commands.insert_resource(MatchCountdown::default());

    // Initialize arena dampening (time-ramped heal/absorb reduction)
    commands.insert_resource(ArenaDampening::default());

    // Derive the obstacle geometry for the selected map (line-of-sight). The
    // same volumes drive both the gameplay resource and the cosmetic obstacle
    // meshes spawned by `spawn_arena_environment` below, so they cannot drift.
    commands.insert_resource(active_map_geometry.clone());
    // AI profile — inserted in BOTH modes (dual-registration rule). Defaults to
    // Legacy, so experimental behaviour is never on by accident.
    // Graphical mode has no profile selector yet, so it runs Legacy. When the
    // TeamPlan work is playable this should read from GameSettings.
    // Do not clobber a profile a replay launch already chose.
    let profile = existing_profile.map(|p| *p).unwrap_or_default();
    info!("AI profile: {:?}", profile);
    commands.insert_resource(profile);
    commands.insert_resource(team_plan::TeamPlans::default());

    // Initialize Shadow Sight state (for stealth stalemate breaking)
    commands.insert_resource(ShadowSightState::default());

    // Initialize random number generator (non-deterministic for graphical mode)
    // Same for the RNG: a replay pre-seeds it, so overwriting here would make
    // the recorded seed meaningless — the whole point is bit-exact reproduction.
    let rng = match existing_rng {
        Some(r) if r.seed.is_some() => GameRng::from_seed(r.seed.unwrap()),
        _ => GameRng::default(),
    };
    info!("Match seed: {:?}", rng.seed);
    commands.insert_resource(rng);

    // Initialize display settings from game settings
    commands.insert_resource(DisplaySettings {
        show_aura_icons: game_settings.show_aura_icons,
        show_combat_panel: game_settings.show_combat_panel,
    });

    // Spawn arena environment (octagonal floor + chamfered stadium walls)
    // via the shared helper, tagging everything for PlayMatch cleanup.
    for entity in spawn_arena_environment(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &active_map_geometry.bounds,
        &active_map_geometry.volumes,
    ) {
        commands.entity(entity).insert(PlayMatchEntity);
    }

    // Count class occurrences per team to apply darkening to duplicates
    use std::collections::HashMap;
    let mut team1_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();
    let mut team2_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();

    // Spawn Team 1 combatants (left side of arena, in the starting room).
    //
    // Per-map, from the active map's bounds: teams must start OUTBOARD of the
    // cover so closing to engage carries them past it. On the octagon maps this
    // resolves to the historical ±35; on Nagrand it puts them inside the gate
    // alcoves, ~65yd out, so the walk in crosses the pillar line.
    let spawn_x = active_map_geometry.bounds.team_spawn_x();
    let team1_spawn_x = -spawn_x;
    for (i, character_opt) in config.team1.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team1_class_counts.get(character).unwrap_or(&0);
            *team1_class_counts.entry(*character).or_insert(0) += 1;

            // Register combatant with combat log for timeline display
            combat_log.register_combatant(combatant_id(1, i as u8, *character));

            // Get rogue opener preference for this slot
            let rogue_opener = config.team1_rogue_openers.get(i).copied().unwrap_or_default();
            let rogue_poison = config.team1_rogue_poisons.get(i).copied().unwrap_or_default();

            // Get warlock curse preferences for this slot (empty vec if none configured)
            let warlock_curse_prefs = config.team1_warlock_curse_prefs.get(i).cloned().unwrap_or_default();

            // Get class-specific strategic option preferences
            let warrior_shout = config.team1_warrior_shouts.get(i).copied().unwrap_or_default();
            let mage_armor = config.team1_mage_armors.get(i).copied().unwrap_or_default();
            let paladin_aura = config.team1_paladin_auras.get(i).copied().unwrap_or_default();

            // Resolve equipment loadout (defaults + overrides), enforcing 2H constraints
            let equipment_overrides = config.team1_equipment.get(i).cloned().unwrap_or_default();
            let mut loadout = resolve_loadout(*character, &default_loadouts, &equipment_overrides);
            enforce_two_hand_conflicts(&mut loadout, &item_defs);

            let position = Vec3::new(team1_spawn_x, 1.0, (i as f32 - 1.0) * 3.0);
            let (entity, combatant) = spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                1,
                i as u8, // slot index
                *character,
                position,
                count,
                rogue_opener,
                rogue_poison,
                warlock_curse_prefs,
                warrior_shout,
                mage_armor,
                paladin_aura,
                &loadout,
                &item_defs,
            );

            // Log equipment loadout
            combat_log.log(
                CombatLogEventType::MatchEvent,
                format!("[EQUIPMENT] {}: {}", combatant_id(1, i as u8, *character), format_loadout(&loadout, &item_defs)),
            );

            // Spawn Felhunter pet for Warlocks
            if *character == match_config::CharacterClass::Warlock {
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    PetType::Felhunter,
                );
            }

            // Spawn pet for Hunters (based on configured pet type)
            if *character == match_config::CharacterClass::Hunter {
                let pet_type_pref = config.team1_hunter_pet_types.get(i).copied().unwrap_or_default();
                let pet_type = match pet_type_pref {
                    match_config::HunterPetType::Spider => PetType::Spider,
                    match_config::HunterPetType::Boar => PetType::Boar,
                    match_config::HunterPetType::Bird => PetType::Bird,
                };
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    pet_type,
                );
            }
        } else {
            warn!("Team 1 slot {} is empty — skipping spawn", i);
        }
    }

    // Spawn Team 2 combatants (right side of arena, in the starting room).
    let team2_spawn_x = spawn_x;
    for (i, character_opt) in config.team2.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team2_class_counts.get(character).unwrap_or(&0);
            *team2_class_counts.entry(*character).or_insert(0) += 1;

            // Register combatant with combat log for timeline display
            combat_log.register_combatant(combatant_id(2, i as u8, *character));

            // Get rogue opener preference for this slot
            let rogue_opener = config.team2_rogue_openers.get(i).copied().unwrap_or_default();
            let rogue_poison = config.team2_rogue_poisons.get(i).copied().unwrap_or_default();

            // Get warlock curse preferences for this slot (empty vec if none configured)
            let warlock_curse_prefs = config.team2_warlock_curse_prefs.get(i).cloned().unwrap_or_default();

            // Get class-specific strategic option preferences
            let warrior_shout = config.team2_warrior_shouts.get(i).copied().unwrap_or_default();
            let mage_armor = config.team2_mage_armors.get(i).copied().unwrap_or_default();
            let paladin_aura = config.team2_paladin_auras.get(i).copied().unwrap_or_default();

            // Resolve equipment loadout (defaults + overrides), enforcing 2H constraints
            let equipment_overrides = config.team2_equipment.get(i).cloned().unwrap_or_default();
            let mut loadout = resolve_loadout(*character, &default_loadouts, &equipment_overrides);
            enforce_two_hand_conflicts(&mut loadout, &item_defs);

            let position = Vec3::new(team2_spawn_x, 1.0, (i as f32 - 1.0) * 3.0);
            let (entity, combatant) = spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                2,
                i as u8, // slot index
                *character,
                position,
                count,
                rogue_opener,
                rogue_poison,
                warlock_curse_prefs,
                warrior_shout,
                mage_armor,
                paladin_aura,
                &loadout,
                &item_defs,
            );

            // Log equipment loadout
            combat_log.log(
                CombatLogEventType::MatchEvent,
                format!("[EQUIPMENT] {}: {}", combatant_id(2, i as u8, *character), format_loadout(&loadout, &item_defs)),
            );

            // Spawn Felhunter pet for Warlocks
            if *character == match_config::CharacterClass::Warlock {
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    PetType::Felhunter,
                );
            }

            // Spawn pet for Hunters (based on configured pet type)
            if *character == match_config::CharacterClass::Hunter {
                let pet_type_pref = config.team2_hunter_pet_types.get(i).copied().unwrap_or_default();
                let pet_type = match pet_type_pref {
                    match_config::HunterPetType::Spider => PetType::Spider,
                    match_config::HunterPetType::Boar => PetType::Boar,
                    match_config::HunterPetType::Bird => PetType::Bird,
                };
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    pet_type,
                );
            }
        } else {
            warn!("Team 2 slot {} is empty — skipping spawn", i);
        }
    }
    
    // Spawn starting gate bars for both teams
    let (gate_x, gate_half_width) = active_map_geometry.bounds.gate_plane();
    spawn_gate_bars(&mut commands, &mut meshes, &mut materials, gate_x, gate_half_width);
}

/// Spawn visual gate bars that lower when countdown ends
/// Spawn the cosmetic gate bars that seal each team in during the countdown.
///
/// `gate_x` is the `|x|` of the gate plane and `gate_half_width` its half-extent
/// in z, both from `ArenaBounds::gate_plane()` so the bars span the actual mouth
/// of the starting area on whatever map is loaded. Bars are evenly distributed
/// across the mouth rather than at a fixed spacing, so a wide gate is filled and
/// a narrow one is not overshot.
fn spawn_gate_bars(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    gate_x: f32,
    gate_half_width: f32,
) {
    let gate_height = 6.0;
    let bar_width = 0.5;
    let bar_depth = 0.5;
    let num_bars = 7; // Number of vertical bars per gate
    // Distribute across the mouth: `num_bars - 1` gaps spanning the full width.
    let spacing = (gate_half_width * 2.0) / (num_bars as f32 - 1.0);
    
    // Dark metal material for the bars
    let bar_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.2), // Dark gray/metal
        metallic: 0.8,
        perceptual_roughness: 0.3,
        ..default()
    });
    
    // Team 1 gate (-x side)
    for i in 0..num_bars {
        let z_offset = (i as f32 - (num_bars as f32 - 1.0) / 2.0) * spacing;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(bar_width, gate_height, bar_depth))),
            MeshMaterial3d(bar_material.clone()),
            Transform::from_xyz(-gate_x, gate_height / 2.0, z_offset),
            GateBar {
                team: 1,
                initial_height: gate_height,
            },
            PlayMatchEntity,
        ));
    }
    
    // Team 2 gate (+x side)
    for i in 0..num_bars {
        let z_offset = (i as f32 - (num_bars as f32 - 1.0) / 2.0) * spacing;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(bar_width, gate_height, bar_depth))),
            MeshMaterial3d(bar_material.clone()),
            Transform::from_xyz(gate_x, gate_height / 2.0, z_offset),
            GateBar {
                team: 2,
                initial_height: gate_height,
            },
            PlayMatchEntity,
        ));
    }
}

/// Deterministic per-entity walk-animation phase offset derived from the
/// spawn XZ position. Two units at the same Z separated in X get different
/// phases, so a 3v3 team that starts walking in lockstep does not bob in unison.
fn walk_phase_seed(xz: Vec2) -> f32 {
    (xz.x * 7.314 + xz.y * 11.927).rem_euclid(std::f32::consts::TAU)
}

/// Helper function to spawn a single combatant entity.
///
/// Creates a capsule mesh colored by class, with darker shades for duplicates.
/// The `duplicate_index` parameter determines how much to darken (0 = base color, 1+ = darkened).
fn spawn_combatant(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    team: u8,
    slot: u8,
    class: match_config::CharacterClass,
    position: Vec3,
    duplicate_index: usize,
    rogue_opener: match_config::RogueOpener,
    rogue_poison: match_config::RoguePoison,
    warlock_curse_prefs: Vec<match_config::WarlockCurse>,
    warrior_shout: match_config::WarriorShout,
    mage_armor: match_config::MageArmor,
    paladin_aura: match_config::PaladinAura,
    equipment_loadout: &std::collections::HashMap<ItemSlot, ItemId>,
    item_defs: &ItemDefinitions,
) -> (Entity, Combatant) {
    // Get vibrant class colors for 3D visibility
    let base_color = class_mesh_color(class);

    // Apply darkening for duplicate classes (0.65 multiplier per duplicate)
    let darken_factor = 0.65f32.powi(duplicate_index as i32);
    let combatant_color = Color::srgb(
        base_color.to_srgba().red * darken_factor,
        base_color.to_srgba().green * darken_factor,
        base_color.to_srgba().blue * darken_factor,
    );

    // Create combatant mesh (capsule represents the body)
    let mesh_handle = meshes.add(Capsule3d::new(0.5, 1.5));
    let material = materials.add(StandardMaterial {
        base_color: combatant_color,
        perceptual_roughness: 0.5, // More reflective for better color visibility
        metallic: 0.2, // Slight metallic sheen for color pop
        // Enable alpha mode for stealth transparency
        alpha_mode: bevy::prelude::AlphaMode::Blend,
        ..default()
    });

    let mut combatant = Combatant::new_with_curse_prefs(team, slot, class, rogue_opener, rogue_poison, warlock_curse_prefs);
    combatant.warrior_shout = warrior_shout;
    combatant.mage_armor = mage_armor;
    combatant.paladin_aura = paladin_aura;
    combatant.apply_equipment(equipment_loadout, item_defs);
    let combatant_clone = combatant.clone();
    let weapon_poison_buff = combatant.weapon_poison_self_buff();

    // The mesh hangs off a CHILD entity so graphical animation never writes this
    // entity's Transform — see `VisualBody`. The child sits at local y 0, so the
    // capsule renders exactly where the sim puts the combatant.
    let entity = commands.spawn((
        Transform::from_translation(position),
        Visibility::default(),
        combatant,
        DRTracker::default(),
        FloatingTextState {
            next_pattern_index: 0,
        },
        PlayMatchEntity,
        WalkAnim {
            phase: walk_phase_seed(position.xz()),
            previous_xz: position.xz(),
        },
    ))
    .with_child((
        Mesh3d(mesh_handle.clone()),
        MeshMaterial3d(material),
        OriginalMesh(mesh_handle),
        VisualBody { rest_y: 0.0 },
        Transform::default(),
    ))
    .id();
    if let Some(buff) = weapon_poison_buff {
        commands.entity(entity).insert(ActiveAuras { auras: vec![buff] });
    }

    (entity, combatant_clone)
}

/// Helper function to spawn a pet entity for a Warlock combatant.
fn spawn_pet(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    combat_log: &mut CombatLog,
    owner_entity: Entity,
    owner_combatant: &Combatant,
    owner_position: Vec3,
    pet_type: PetType,
) {
    let pet_slot = PET_SLOT_BASE + owner_combatant.slot;
    let pet_combatant = Combatant::new_pet(owner_combatant.team, pet_slot, pet_type, owner_combatant);
    // MUST match the headless spawn offsets in `headless/runner.rs` exactly, or a
    // seed stops reproducing between the two modes.
    //
    // The x offset MIRRORS BY TEAM so the pet spawns BEHIND its owner: team 1
    // starts at -x, team 2 at +x. This used to be a flat `-2.0` for both, which
    // put team 2's pet two yards toward the ENEMY instead of two yards back — a
    // four-yard positional difference from headless on the very first frame. The
    // Felhunter then reached the enemy sooner and shifted every subsequent event
    // ~0.7s earlier, growing to 3.8s by the end of the match. That is the bulk of
    // the graphical/headless seed divergence in `design-docs/2026-08-01-nagrand-
    // camp-handoff.md` §3.3 — it is a POSITIONAL difference, not the archetype /
    // RNG-draw-order effect that document hypothesised. (An RNG-order change
    // would alter damage VALUES; every value matched, only timings moved.)
    //
    // The y must match too: gameplay range checks use `Vec3::distance`, so height
    // is not free. 0.75 is headless's value, and headless is what every recorded
    // baseline runs.
    let pet_position = owner_position
        + Vec3::new(if owner_combatant.team == 1 { -2.0 } else { 2.0 }, 0.75, 1.5);

    let pet_color = pet_type.color();
    // Stocky capsule for quadruped (tilted horizontal by apply_pet_mesh_tilt system)
    let mesh_handle = meshes.add(Capsule3d::new(0.35, 0.6));
    let material = materials.add(StandardMaterial {
        base_color: pet_color,
        perceptual_roughness: 0.5,
        metallic: 0.2,
        alpha_mode: bevy::prelude::AlphaMode::Blend,
        ..default()
    });

    // Face toward arena center so tilt system has a valid initial facing
    let initial_facing = if owner_combatant.team == 1 {
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) // Face right (+X)
    } else {
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) // Face left (-X)
    };

    // The sim entity sits at headless's y (0.75) so a seed reproduces; the
    // capsule is tuned to render at 0.3, so the `VisualBody` child carries the
    // difference as a local offset. Splitting the two is what lets the gameplay
    // height and the rendered height disagree without either being wrong.
    const PET_MESH_Y: f32 = 0.3;
    commands.spawn((
        Transform::from_translation(pet_position).with_rotation(initial_facing),
        Visibility::default(),
        pet_combatant,
        DRTracker::default(),
        Pet {
            owner: owner_entity,
            pet_type,
        },
        FloatingTextState {
            next_pattern_index: 0,
        },
        PlayMatchEntity,
        WalkAnim {
            phase: walk_phase_seed(pet_position.xz()),
            previous_xz: pet_position.xz(),
        },
    ))
    .with_child((
        Mesh3d(mesh_handle.clone()),
        MeshMaterial3d(material),
        OriginalMesh(mesh_handle),
        VisualBody { rest_y: PET_MESH_Y - pet_position.y },
        Transform::from_xyz(0.0, PET_MESH_Y - pet_position.y, 0.0),
    ));

    // Register pet with combat log
    combat_log.register_combatant(pet_combatant_id(owner_combatant.team, owner_combatant.slot, pet_type));
}

/// Handle camera input for mode switching, zoom, rotation, and drag

/// Cleanup system: Despawns all Play Match entities when exiting the state.
pub fn cleanup_play_match(
    mut commands: Commands,
    query: Query<Entity, With<PlayMatchEntity>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
    
    // Remove resources
    commands.remove_resource::<AmbientLight>();
    commands.remove_resource::<SimulationSpeed>();
    commands.remove_resource::<MatchCountdown>();
    commands.remove_resource::<ArenaDampening>();
    commands.remove_resource::<ActiveMapGeometry>();
    commands.remove_resource::<ShadowSightState>();
    commands.remove_resource::<DisplaySettings>();
    // MUST be removed, not left for the next match: `setup_play_match` treats a
    // surviving `GameRng` (and `AiProfile`) as "a replay pre-seeded this match"
    // and honours it. `GameRng::default()` now RECORDS its seed, so leaving the
    // resource behind made every match after the first in a client session
    // silently re-run the first match's seed instead of picking a fresh one —
    // and a `--replay` under TeamPlan leaked that profile into the next normal
    // match. A real replay inserts both BEFORE the state is entered, so it is
    // unaffected by this exit-time cleanup.
    commands.remove_resource::<GameRng>();
    commands.remove_resource::<ai_profile::AiProfile>();
    commands.remove_resource::<team_plan::TeamPlans>();
    // Remove optional resources (may not exist if match didn't finish)
    commands.remove_resource::<VictoryCelebration>();
}

// ============================================================================
// Update & Input Systems
// ============================================================================

/// Countdown system: Manage pre-combat countdown and gate opening.
/// 
/// During countdown (10 seconds):
/// - Mana is restored to 100% every second (encourages pre-buffing)
/// - Combatants can cast buffs but cannot move or attack
/// - Countdown timer ticks down
/// 
/// When countdown reaches 0:
/// - Gates open (sets gates_opened flag)
/// - Combat begins normally

/// Render time control UI panel in the top-right corner.
/// 
/// Shows current speed and clickable buttons for speed control.
/// Handle player input during the match.
/// Currently only handles ESC key to return to main menu.
pub fn update_play_match(
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    use crate::keybindings::GameAction;
    
    if keybindings.action_just_pressed(GameAction::Back, &keyboard) {
        next_state.set(GameState::MainMenu);
    }
}

// ============================================================================
// Combat Systems (see submodules: combat_ai, combat_core, auras, projectiles)
// ============================================================================


// ============================================================================
// Mesh construction tests
// ============================================================================

#[cfg(test)]
mod mesh_tests {
    use super::*;
    use bevy::render::mesh::{Indices, VertexAttributeValues};

    /// Pull (positions, normals, indices) out of a mesh for winding checks.
    fn mesh_parts(mesh: &Mesh) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
        let VertexAttributeValues::Float32x3(pos) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions")
        else {
            panic!("positions must be Float32x3");
        };
        let VertexAttributeValues::Float32x3(nrm) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL).expect("normals")
        else {
            panic!("normals must be Float32x3");
        };
        let Some(Indices::U32(idx)) = mesh.indices() else {
            panic!("indices must be U32");
        };
        (pos.clone(), nrm.clone(), idx.clone())
    }

    /// Every triangle's geometric winding must agree with its vertex shading
    /// normals: `(v1-v0) x (v2-v0)` must point the same way as the normals.
    ///
    /// This is the check that catches inverted winding. Bevy culls back faces by
    /// default (`FrontFace::Ccw`), so a mesh whose winding disagrees with its
    /// normals renders as only its FAR interior surface — which is exactly how the
    /// Nagrand pillars first appeared ("half built and not filled in"). Shading
    /// normals alone look right in that state, so nothing else flags it.
    fn assert_winding_matches_normals(mesh: &Mesh, label: &str) {
        let (pos, nrm, idx) = mesh_parts(mesh);
        assert!(!idx.is_empty(), "{label}: mesh has no triangles");
        assert_eq!(idx.len() % 3, 0, "{label}: index count is not a multiple of 3");

        for tri in idx.chunks(3) {
            let v: Vec<Vec3> = tri
                .iter()
                .map(|&i| Vec3::from_array(pos[i as usize]))
                .collect();
            let geo = (v[1] - v[0]).cross(v[2] - v[0]);
            // Degenerate triangles carry no orientation; skip them.
            if geo.length_squared() < 1e-9 {
                continue;
            }
            let geo = geo.normalize();
            for &i in tri {
                let shading = Vec3::from_array(nrm[i as usize]);
                assert!(
                    geo.dot(shading) > 0.0,
                    "{label}: triangle {tri:?} winds against its shading normal \
                     (geometric {geo:?} vs shading {shading:?}) — it will be \
                     backface-culled and render invisible"
                );
            }
        }
    }

    /// A pillar's side faces must point AWAY from its centre, or the solid renders
    /// inside-out.
    #[test]
    fn prism_mesh_faces_outward() {
        let center = Vec2::new(-40.0, 20.0);
        let verts = map_geometry::prism_vertices_world(center, 6.0, 8, 22.5_f32.to_radians());
        let mesh = create_prism_mesh(&verts, 0.0, 5.0);
        assert_winding_matches_normals(&mesh, "octagonal pillar");

        // Spot-check outwardness directly: each side quad's shading normal must
        // have a positive component away from the pillar's centre.
        let (pos, nrm, _) = mesh_parts(&mesh);
        let mut side_faces = 0;
        for (p, n) in pos.iter().zip(nrm.iter()) {
            let n = Vec3::from_array(*n);
            if n.y.abs() > 0.5 {
                continue; // top cap
            }
            let radial = Vec2::new(p[0] - center.x, p[2] - center.y);
            if radial.length_squared() < 1e-6 {
                continue;
            }
            assert!(
                Vec2::new(n.x, n.z).dot(radial.normalize()) > 0.0,
                "pillar side normal {n:?} points inward at {p:?}"
            );
            side_faces += 1;
        }
        assert!(side_faces > 0, "expected side-face vertices to check");
    }

    /// The arena floor must face up, for both arena shapes.
    #[test]
    fn arena_floor_mesh_faces_up() {
        let bowl = arena_bounds::ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        for (label, bounds) in [
            ("octagon floor", arena_bounds::ArenaBounds::default()),
            ("bowl floor", bowl),
        ] {
            let outline = bounds.outline(WALL_ARC_SEGMENTS);
            let mesh = create_arena_floor_mesh(&outline, 1.0 / 12.0);
            assert_winding_matches_normals(&mesh, label);
        }
    }

    /// Guards the fan-triangulation vertex/index arithmetic: a fan over `n`
    /// outline points is `n + 1` vertices and `n` triangles.
    #[test]
    fn arena_floor_mesh_has_expected_topology() {
        let outline = arena_bounds::ArenaBounds::default().outline(WALL_ARC_SEGMENTS);
        let mesh = create_arena_floor_mesh(&outline, 1.0 / 12.0);
        let (pos, nrm, idx) = mesh_parts(&mesh);
        assert_eq!(pos.len(), outline.len() + 1, "fan is outline + centre vertex");
        assert_eq!(nrm.len(), pos.len(), "one normal per vertex");
        assert_eq!(idx.len(), outline.len() * 3, "one triangle per outline edge");
        for p in &pos {
            assert!(
                p.iter().all(|c| c.is_finite()),
                "floor vertex {p:?} is not finite"
            );
        }
    }
}

#[cfg(test)]
mod view_scale_tests {
    use super::*;
    use arena_bounds::ArenaBounds;

    /// The tuned arena keeps its tuned framing.
    #[test]
    fn the_reference_arena_is_unscaled() {
        assert_eq!(arena_view_scale(&ArenaBounds::default()), 1.0);
    }

    /// Nagrand's spawn rooms sit at |x| = 64.72. The camera must pull back far
    /// enough to hold them, or both teams walk in off-screen — which is exactly
    /// what the fixed (0, 40, 50) framing did.
    #[test]
    fn the_bowl_pulls_the_camera_back_past_its_spawns() {
        let bowl = ArenaBounds::Bowl {
            semi_x: 59.72,
            semi_z: 59.72,
            alcove_depth: 10.0,
            alcove_half_width: 8.0,
        };
        let scale = arena_view_scale(&bowl);
        assert!(scale > 1.5, "expected a real pull-back, got {scale}");
        let framed_depth = 50.0 * scale;
        assert!(
            framed_depth > bowl.team_spawn_x(),
            "camera at z={framed_depth:.1} cannot hold spawns at |x|={:.1}",
            bowl.team_spawn_x()
        );
    }

    /// A hypothetical small map must not drag the camera closer than the tuned
    /// framing — clamping at 1.0 is deliberate.
    #[test]
    fn a_smaller_map_is_never_scaled_below_one() {
        let tiny = ArenaBounds::Octagon { half_x: 10.0, half_z: 8.0, corner_sum: 14.0 };
        assert_eq!(arena_view_scale(&tiny), 1.0);
    }
}
