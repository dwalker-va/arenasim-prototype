use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use crate::states::play_match::components::*;
use crate::states::match_config::CharacterClass;
use super::dispel_burst::dispel_burst_colors;

// ==============================================================================
// Dispel Ribbon Visual Effects
// ==============================================================================
// A twisting ribbon that spirals up off the dispelled combatant's head — the
// distinct "you got cleansed" indicator (see DispelRibbon). Graphical only;
// registered in states/mod.rs, never in headless systems.rs.

/// Number of turns the ribbon helix coils through over its baked length.
const RIBBON_TURNS: f32 = 2.5;
/// Baked vertical span (yards) of the helix geometry itself.
const RIBBON_HEIGHT: f32 = 1.4;
/// Ribbon band width (yards). Thin so it reads as a defined ribbon, not a blob.
const RIBBON_WIDTH: f32 = 0.22;
/// Horizontal coil radius (yards). Must be > 0 or the ribbon degenerates to a
/// twisted vertical column instead of a laterally-spiraling ribbon.
const RIBBON_RADIUS: f32 = 0.35;
/// Number of strip segments along the helix (mesh resolution).
const RIBBON_SEGMENTS: usize = 48;
/// Anchor height (yards) above the target's origin — the head, taller than the
/// sphere burst's chest-height `Vec3::Y * 1.0`.
const RIBBON_HEAD_OFFSET: f32 = 1.9;
/// Additional upward travel (yards) accrued over the ribbon's lifetime, so it
/// visibly lifts off the head (the motion is a primary distinctiveness lever).
const RIBBON_RISE_DISTANCE: f32 = 0.5;
/// Spin rate (radians/sec) of the ribbon's slow Y-axis rotation.
const RIBBON_SPIN_RATE: f32 = 3.0;

/// Color for the dispel ribbon: the same class hue as the burst, but near-opaque
/// with a moderated emissive so it reads as a *solid* ribbon (a colored surface
/// with a sheen) rather than the wispy additive glow of the sphere bursts. Kept
/// separate from `dispel_burst_colors` so the burst (Concussive Shot / Master's
/// Call) is unaffected.
fn dispel_ribbon_colors(class: CharacterClass) -> (Color, LinearRgba) {
    let (base, emissive) = dispel_burst_colors(class);
    (
        // Near-opaque surface so AlphaMode::Blend gives it physical presence.
        base.with_alpha(0.95),
        // Trim the emissive so it's a colored sheen + light bloom, not a pure glow.
        LinearRgba::new(emissive.red * 0.6, emissive.green * 0.6, emissive.blue * 0.6, 1.0),
    )
}

/// Build the twisting-ribbon mesh: a flat strip of quads whose centerline follows
/// a helix coiling upward. Modeled on `create_arena_floor_mesh` (the codebase's raw-vertex
/// mesh precedent). `radius` must be > 0 so the band coils laterally rather than
/// twisting in place. Returns a `TriangleList` with POSITION / NORMAL / UV_0 and
/// U32 indices; render it double-sided (`cull_mode: None`) since the band is thin.
fn build_dispel_ribbon_mesh(
    turns: f32,
    height: f32,
    width: f32,
    radius: f32,
    segments: usize,
) -> Mesh {
    use std::f32::consts::TAU;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(2 * (segments + 1));

    for i in 0..=segments {
        let t = i as f32 / segments as f32; // 0..1 along the strip
        let angle = t * turns * TAU;
        let y = t * height;
        let (sin_a, cos_a) = angle.sin_cos();

        // Centerline orbits the vertical axis at `radius`.
        let center = Vec3::new(cos_a * radius, y, sin_a * radius);
        // Width is offset along the horizontal radial direction, so the band
        // reads as a coiling ramp rather than a vertical wall.
        let radial = Vec3::new(cos_a, 0.0, sin_a);
        let half = radial * (width * 0.5);

        let left = center - half;
        let right = center + half;
        positions.push([left.x, left.y, left.z]);
        positions.push([right.x, right.y, right.z]);

        // Emissive + additive blending makes lighting negligible; an up-facing
        // normal is a fine, stable approximation for the ramp band.
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);

        uvs.push([0.0, t]);
        uvs.push([1.0, t]);
    }

    // Two triangles per segment over the 4 edge vertices of segments i and i+1.
    let mut indices: Vec<u32> = Vec::with_capacity(6 * segments);
    for i in 0..segments {
        let bl = (2 * i) as u32; // bottom-left
        let br = (2 * i + 1) as u32; // bottom-right
        let tl = (2 * (i + 1)) as u32; // top-left
        let tr = (2 * (i + 1) + 1) as u32; // top-right
        indices.extend_from_slice(&[bl, br, tr, bl, tr, tl]);
    }

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Spawn visual mesh for new dispel ribbons.
pub fn spawn_dispel_ribbon_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_ribbons: Query<(Entity, &DispelRibbon), (Added<DispelRibbon>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (ribbon_entity, ribbon) in new_ribbons.iter() {
        let Ok(target_transform) = transforms.get(ribbon.target) else {
            continue;
        };

        let (base_color, emissive) = dispel_ribbon_colors(ribbon.caster_class);

        let mesh = meshes.add(build_dispel_ribbon_mesh(
            RIBBON_TURNS,
            RIBBON_HEIGHT,
            RIBBON_WIDTH,
            RIBBON_RADIUS,
            RIBBON_SEGMENTS,
        ));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            // Blend (not Add) so the ribbon reads as a solid surface rather than a
            // glow. The helix coils are vertically separated, so the thin band
            // doesn't self-overlap coplanarly — Z-fighting risk is low.
            alpha_mode: AlphaMode::Blend,
            // Thin ribbon: render both faces so it never vanishes from behind.
            cull_mode: None,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * RIBBON_HEAD_OFFSET;

        commands.entity(ribbon_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update dispel ribbons: follow the target, rise off the head, spin, and fade.
pub fn update_dispel_ribbons(
    time: Res<Time>,
    mut ribbons: Query<(&mut DispelRibbon, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<DispelRibbon>>,
) {
    for (mut ribbon, mut ribbon_transform, material_handle) in ribbons.iter_mut() {
        ribbon.lifetime -= time.delta_secs();
        ribbon.spin += time.delta_secs() * RIBBON_SPIN_RATE;

        // Progress: 1.0 (just spawned) → 0.0 (expired)
        let progress = (ribbon.lifetime / ribbon.initial_lifetime).max(0.0);

        // Follow the target's head, plus a rise that grows as the ribbon ages so
        // it visibly lifts off the head over its lifetime. If the target is gone
        // (died mid-ribbon), freeze at the last anchored position and keep fading
        // — matches DispelBurst / HealingLightColumn.
        if let Ok(target_transform) = transforms.get(ribbon.target) {
            let rise = (1.0 - progress) * RIBBON_RISE_DISTANCE;
            ribbon_transform.translation =
                target_transform.translation + Vec3::Y * (RIBBON_HEAD_OFFSET + rise);
        }
        ribbon_transform.rotation = Quat::from_rotation_y(ribbon.spin);

        // Fade out: scale alpha + emissive by progress (recompute canonical color).
        let (base_color, emissive) = dispel_ribbon_colors(ribbon.caster_class);
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = base_color.with_alpha(base_color.alpha() * progress);
            material.emissive = LinearRgba::new(
                emissive.red * progress,
                emissive.green * progress,
                emissive.blue * progress,
                1.0,
            );
        }
    }
}

/// Cleanup expired dispel ribbons.
pub fn cleanup_expired_dispel_ribbons(
    mut commands: Commands,
    ribbons: Query<(Entity, &DispelRibbon)>,
) {
    for (entity, ribbon) in ribbons.iter() {
        if ribbon.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod dispel_ribbon_mesh_tests {
    use super::*;
    use std::f32::consts::TAU;

    fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v.clone(),
            _ => panic!("expected Float32x3 positions"),
        }
    }

    #[test]
    fn vertex_and_index_counts_match_segments() {
        let segments = 32;
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, 0.35, 0.35, segments);
        assert_eq!(positions(&mesh).len(), 2 * (segments + 1));
        let index_count = match mesh.indices().unwrap() {
            Indices::U32(v) => v.len(),
            Indices::U16(v) => v.len(),
        };
        assert_eq!(index_count, 6 * segments);
    }

    #[test]
    fn vertices_span_the_full_rise() {
        let height = 1.4;
        let mesh = build_dispel_ribbon_mesh(2.5, height, 0.35, 0.35, 48);
        let ys: Vec<f32> = positions(&mesh).iter().map(|p| p[1]).collect();
        let min_y = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_y = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(min_y.abs() < 1e-4, "min Y should be ~0, got {min_y}");
        assert!((max_y - height).abs() < 1e-4, "max Y should be ~height, got {max_y}");
    }

    #[test]
    fn centerline_covers_requested_turns() {
        // First and last segment centerline angles should span turns * TAU.
        let turns = 2.5;
        let segments = 48;
        let mesh = build_dispel_ribbon_mesh(turns, 1.4, 0.35, 0.35, segments);
        let pos = positions(&mesh);
        // Centerline at each segment = midpoint of its two edge vertices, in XZ.
        let first = pos[0];
        let last = pos[pos.len() - 1];
        let first_angle = first[2].atan2(first[0]);
        // Sanity: the first centerline angle is near 0 (cos≈1).
        assert!(first_angle.abs() < 0.3, "first angle near 0, got {first_angle}");
        // The helix advances monotonically: the y of the last vertex >> first.
        assert!(last[1] > first[1]);
        // Number of full turns is encoded in height/turns geometry; assert the
        // angular range by walking centerline angle deltas.
        let mut total = 0.0_f32;
        let mut prev = first_angle;
        for i in 1..=segments {
            let v = pos[2 * i];
            let a = v[2].atan2(v[0]);
            let mut d = a - prev;
            while d > std::f32::consts::PI {
                d -= TAU;
            }
            while d < -std::f32::consts::PI {
                d += TAU;
            }
            total += d;
            prev = a;
        }
        assert!(
            (total.abs() - turns * TAU).abs() < 0.2,
            "unwrapped angular span {:.3} should be ~{:.3}",
            total.abs(),
            turns * TAU
        );
    }

    #[test]
    fn radius_produces_lateral_coil() {
        let radius = 0.35;
        let width = 0.35;
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, width, radius, 48);
        let max_horiz = positions(&mesh)
            .iter()
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(0.0_f32, f32::max);
        // A real lateral coil reaches out to ~radius + width/2, not ~0.
        assert!(
            (max_horiz - (radius + width * 0.5)).abs() < 0.05,
            "max horizontal extent {max_horiz} should be ~{}",
            radius + width * 0.5
        );
    }

    #[test]
    fn attributes_present_equal_length_and_indices_valid() {
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, 0.35, 0.35, 24);
        let vcount = positions(&mesh).len();
        for attr in [Mesh::ATTRIBUTE_NORMAL, Mesh::ATTRIBUTE_UV_0] {
            let len = match mesh.attribute(attr).unwrap() {
                bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v.len(),
                bevy::render::mesh::VertexAttributeValues::Float32x2(v) => v.len(),
                _ => panic!("unexpected attribute kind"),
            };
            assert_eq!(len, vcount, "attribute length should equal vertex count");
        }
        match mesh.indices().unwrap() {
            Indices::U32(v) => assert!(v.iter().all(|&i| (i as usize) < vcount)),
            Indices::U16(v) => assert!(v.iter().all(|&i| (i as usize) < vcount)),
        }
    }
}

