use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::play_match::components::*;
use crate::states::play_match::map_config::ActiveMapGeometry;

// ==============================================================================
// Totem Visuals (Shaman, graphical-only)
// ==============================================================================

/// Build a flat ground disc of `radius` centered at `center` (world XZ), clipped
/// to `bounds` so it never spills past the arena walls. Vertices are in LOCAL
/// space (offsets from `center`) lying in the XZ plane at y=0, so the mesh can be
/// parented to an entity sitting at `center`. Reusable by any ground decal that
/// must stay inside the arena.
///
/// Clipping is a per-direction march against [`ArenaBounds::contains`], which is
/// shape-agnostic: this used to be eight hard-coded octagon half-planes, which
/// silently collapsed the disc to zero radius on Nagrand's bowl (any totem outside
/// the retired 76×46 rectangle failed every plane test at once). The disc now
/// stops at the walkable edge rather than exactly at the wall — a `WALL_OFFSET`
/// (1.5yd) inset, and the only bound that holds for every shape.
fn arena_clipped_disc_mesh(bounds: &ArenaBounds, center: Vec2, radius: f32) -> Mesh {
    const SEGMENTS: usize = 96;
    /// Radial march step. Fine enough that the clip reads as a clean edge on a
    /// decal this faint.
    const STEP: f32 = 0.2;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(SEGMENTS + 1);
    let mut indices: Vec<u32> = Vec::with_capacity(SEGMENTS * 3);
    positions.push([0.0, 0.0, 0.0]); // fan center (index 0)
    for i in 0..SEGMENTS {
        let a = (i as f32) / (SEGMENTS as f32) * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        // March outward until the point leaves the arena, capped at `radius`.
        let mut t = 0.0_f32;
        while t + STEP <= radius {
            let probe = center + dir * (t + STEP);
            if !bounds.contains(Vec3::new(probe.x, 1.0, probe.y)) {
                break;
            }
            t += STEP;
        }
        let p = dir * t;
        positions.push([p.x, 0.0, p.y]); // dir.y maps to world/local Z
    }
    for i in 0..SEGMENTS {
        indices.push(0);
        indices.push(1 + i as u32);
        indices.push(1 + ((i + 1) % SEGMENTS) as u32);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs = vec![[0.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Attach meshes to newly spawned totems. Headless mode spawns the bare `Totem`
/// gameplay entity; this graphical-only system gives it a SOLID, clearly-
/// non-player silhouette — a chunky carved post topped with a glowing element
/// orb — plus a very subtle ground disc (clipped to the arena walls) marking the
/// buff radius. Every mesh is a child entity, so the totem's gameplay `Transform`
/// is never touched and the meshes clean up with the totem via recursive
/// despawn. Registered ONLY in `StatesPlugin::build` — never in
/// `add_core_combat_systems`.
pub fn spawn_totem_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Per-map arena shape, for clipping the buff-radius decal to the real walls.
    // `Option` so a scene without the resource simply skips the map-aware clip.
    map_geometry: Option<Res<ActiveMapGeometry>>,
    new_totems: Query<(Entity, &Totem, &Transform), (Added<Totem>, Without<Children>)>,
) {
    let bounds = map_geometry
        .as_ref()
        .map(|g| g.bounds)
        .unwrap_or_default();
    for (totem_entity, totem, transform) in new_totems.iter() {
        let color = totem.element.color();
        let s = color.to_srgba();

        // Solid carved post — short and blocky, distinct from the tall rounded
        // player capsules.
        let post_mesh = meshes.add(Cuboid::new(0.6, 1.3, 0.6));
        let post_mat = materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::new(s.red * 0.5, s.green * 0.5, s.blue * 0.5, 1.0),
            perceptual_roughness: 0.75,
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });

        // Floating element orb on top — reads instantly as a magic totem.
        let orb_mesh = meshes.add(Sphere::new(0.34));
        let orb_mat = materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::new(s.red * 2.5, s.green * 2.5, s.blue * 2.5, 1.0),
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });

        // Very subtle ground disc marking the buff radius, clipped to the active
        // map's walkable region so it never spills past the walls. `Add` blend per
        // the project's ground-indicator convention to avoid z-fighting flicker.
        let disc_mesh = meshes.add(arena_clipped_disc_mesh(
            &bounds,
            transform.translation.xz(),
            totem.radius,
        ));
        let disc_mat = materials.add(StandardMaterial {
            base_color: color.with_alpha(0.08),
            emissive: LinearRgba::new(s.red * 0.18, s.green * 0.18, s.blue * 0.18, 1.0),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        });

        // Child entities anchored to the totem's ground position (y = 0).
        // The core-spawned Totem entity has only Transform/Totem (no visibility
        // components). Give it `Visibility` (which pulls in InheritedVisibility +
        // ViewVisibility) so the mesh children inherit a valid visibility chain —
        // otherwise Bevy logs B0004 for every totem.
        commands
            .entity(totem_entity)
            .insert(Visibility::default())
            .with_children(|parent| {
            // post: base rests on the ground (Cuboid is centered, half-height 0.65)
            parent.spawn((
                Mesh3d(post_mesh),
                MeshMaterial3d(post_mat),
                Transform::from_xyz(0.0, 0.65, 0.0),
            ));
            // orb: floats just above the post top (post spans y 0.0..1.3)
            parent.spawn((
                Mesh3d(orb_mesh),
                MeshMaterial3d(orb_mat),
                Transform::from_xyz(0.0, 1.65, 0.0),
            ));
            // radius disc: a hair above the floor so it doesn't z-fight it
            parent.spawn((
                Mesh3d(disc_mesh),
                MeshMaterial3d(disc_mat),
                Transform::from_xyz(0.0, 0.03, 0.0),
            ));
        });
    }
}

/// Shrink a totem (post + orb + radius disc together, via the child hierarchy)
/// over its final 1.2 seconds as a clean expiry tell. Totems stay fully SOLID
/// otherwise — no alpha fade. Mutates only `Transform.scale`, which gameplay
/// ignores (the pulse system keys off `Totem.radius` and translation), so this
/// remains purely cosmetic and graphical-only.
pub fn update_totem_visuals(mut totems: Query<(&Totem, &mut Transform)>) {
    for (totem, mut transform) in totems.iter_mut() {
        let scale = if totem.duration_remaining < 1.2 {
            (totem.duration_remaining / 1.2).clamp(0.0, 1.0).max(0.001)
        } else {
            1.0
        };
        if (transform.scale.x - scale).abs() > f32::EPSILON {
            transform.scale = Vec3::splat(scale);
        }
    }
}

