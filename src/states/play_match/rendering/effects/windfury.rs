use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use crate::states::play_match::components::*;

// ==============================================================================
// Windfury Tornado Visual (spawned on a WindfuryTornado entity via Added<>)
// ==============================================================================

const WINDFURY_TURNS: f32 = 4.5;
const WINDFURY_HEIGHT: f32 = 3.0;
const WINDFURY_WIDTH: f32 = 0.35;
// Wide enough at the base to ENCIRCLE the ally (capsule radius ≈ 0.5), flaring
// wider toward the top — the character stands inside the vortex.
const WINDFURY_BOTTOM_RADIUS: f32 = 0.9;
const WINDFURY_TOP_RADIUS: f32 = 1.7;
const WINDFURY_SEGMENTS: usize = 96;
const WINDFURY_SPIN_RATE: f32 = 14.0; // fast — a tornado, not a gentle coil
/// Drop the funnel base to the ally's actual feet. Combatants sit at y≈1.0
/// (capsule center) with the capsule bottom ≈1.25 below that, so a 1.25 offset
/// puts the base right at the feet and the funnel rises up around the body.
const WINDFURY_FEET_OFFSET: f32 = 1.25;
// Storm-grey dust funnel, NOT white — `Blend` (not `Add`) so it can actually
// read dark, keeping it distinct from the bright additive shield bubbles
// (Power Word: Shield / Ice Barrier). Like the dispel ribbon, the helix coils
// are vertically separated, so coplanar Z-fighting risk is low.
const WINDFURY_RGB: (f32, f32, f32) = (0.34, 0.36, 0.40);
const WINDFURY_ALPHA: f32 = 0.62;
const WINDFURY_EMISSIVE: (f32, f32, f32) = (0.10, 0.12, 0.16); // faint, cool — not a glow

/// A funnel-shaped helix: identical to the dispel ribbon but the orbit radius
/// widens with height (narrow at the feet, broad at the top) so the band reads
/// as a swirling tornado rather than a uniform coil.
fn build_tornado_mesh(
    turns: f32,
    height: f32,
    width: f32,
    bottom_radius: f32,
    top_radius: f32,
    segments: usize,
) -> Mesh {
    use std::f32::consts::TAU;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(2 * (segments + 1));

    for i in 0..=segments {
        let t = i as f32 / segments as f32; // 0 (feet) .. 1 (top)
        let angle = t * turns * TAU;
        let y = t * height;
        let radius = bottom_radius + (top_radius - bottom_radius) * t; // funnel
        let (sin_a, cos_a) = angle.sin_cos();

        let center = Vec3::new(cos_a * radius, y, sin_a * radius);
        let radial = Vec3::new(cos_a, 0.0, sin_a);
        // Band widens a touch toward the top, like a flaring vortex.
        let half = radial * (width * (0.6 + 0.4 * t) * 0.5);

        let left = center - half;
        let right = center + half;
        positions.push([left.x, left.y, left.z]);
        positions.push([right.x, right.y, right.z]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([0.0, t]);
        uvs.push([1.0, t]);
    }

    let mut indices: Vec<u32> = Vec::with_capacity(6 * segments);
    for i in 0..segments {
        let bl = (2 * i) as u32;
        let br = (2 * i + 1) as u32;
        let tl = (2 * (i + 1)) as u32;
        let tr = (2 * (i + 1) + 1) as u32;
        indices.extend_from_slice(&[bl, br, tr, bl, tr, tl]);
    }

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Attach the funnel mesh when a `WindfuryTornado` marker is spawned (at the
/// proc site, in core). Graphical-only — registered solely in `states/mod.rs`.
pub fn spawn_windfury_tornado_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_tornados: Query<(Entity, &WindfuryTornado), (Added<WindfuryTornado>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (entity, tornado) in new_tornados.iter() {
        let Ok(target_transform) = transforms.get(tornado.target) else {
            // Target despawned in the same frame — drop the orphan effect.
            commands.entity(entity).despawn();
            continue;
        };

        let mesh = meshes.add(build_tornado_mesh(
            WINDFURY_TURNS,
            WINDFURY_HEIGHT,
            WINDFURY_WIDTH,
            WINDFURY_BOTTOM_RADIUS,
            WINDFURY_TOP_RADIUS,
            WINDFURY_SEGMENTS,
        ));
        // Dark storm-grey dust funnel — blended (not additive) so it stays dark.
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(WINDFURY_RGB.0, WINDFURY_RGB.1, WINDFURY_RGB.2, WINDFURY_ALPHA),
            emissive: LinearRgba::new(WINDFURY_EMISSIVE.0, WINDFURY_EMISSIVE.1, WINDFURY_EMISSIVE.2, 1.0),
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });

        let position = target_transform.translation - Vec3::Y * WINDFURY_FEET_OFFSET;
        commands.entity(entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Spin the funnel fast, follow the ally's feet, and fade out over its lifetime.
pub fn update_windfury_tornados(
    time: Res<Time>,
    mut tornados: Query<(&mut WindfuryTornado, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<WindfuryTornado>>,
) {
    for (mut tornado, mut tornado_transform, material_handle) in tornados.iter_mut() {
        tornado.lifetime -= time.delta_secs();
        tornado.spin += time.delta_secs() * WINDFURY_SPIN_RATE;

        let progress = (tornado.lifetime / tornado.initial_lifetime).max(0.0);

        // Follow the ally (freeze in place if they died mid-effect, like the
        // dispel ribbon / healing column).
        if let Ok(target_transform) = transforms.get(tornado.target) {
            tornado_transform.translation =
                target_transform.translation - Vec3::Y * WINDFURY_FEET_OFFSET;
        }
        tornado_transform.rotation = Quat::from_rotation_y(tornado.spin);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color =
                Color::srgba(WINDFURY_RGB.0, WINDFURY_RGB.1, WINDFURY_RGB.2, WINDFURY_ALPHA * progress);
            material.emissive = LinearRgba::new(
                WINDFURY_EMISSIVE.0 * progress,
                WINDFURY_EMISSIVE.1 * progress,
                WINDFURY_EMISSIVE.2 * progress,
                1.0,
            );
        }
    }
}

/// Despawn expired Windfury funnels.
pub fn cleanup_expired_windfury_tornados(
    mut commands: Commands,
    tornados: Query<(Entity, &WindfuryTornado)>,
) {
    for (entity, tornado) in tornados.iter() {
        if tornado.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

