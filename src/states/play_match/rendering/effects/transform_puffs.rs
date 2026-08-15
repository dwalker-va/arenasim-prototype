use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Transform Puff (polymorph apply / restore)
// ==============================================================================
// A pale cloud that puffs at the victim's torso on BOTH transform directions
// (see TransformPuff). Graphical only; registered in states/mod.rs, never in
// headless systems.rs.

/// Lifetime (seconds) of the puff. Short on purpose: polymorph breaks on ANY
/// damage, so an apply and its break can land within a second of each other and
/// must read as two pops rather than one smear.
const PUFF_LIFETIME: f32 = 0.45;
/// Radius (yards) of the puff's central lobe.
const PUFF_CENTER_RADIUS: f32 = 0.24;
/// Outer lobes as (offset from the puff's origin, radius) — staggered sizes and
/// heights so the cluster reads as billowing cloud rather than a sphere.
const PUFF_LOBES: [(Vec3, f32); 4] = [
    (Vec3::new(0.26, 0.06, 0.10), 0.19),
    (Vec3::new(-0.22, -0.04, 0.20), 0.16),
    (Vec3::new(0.06, 0.22, -0.24), 0.21),
    (Vec3::new(-0.12, 0.14, 0.26), 0.14),
];
/// Cluster scale at spawn and at expiry. The expansion is eased so the puff
/// pops open and then coasts, the shape of real smoke.
const PUFF_SCALE_START: f32 = 0.55;
const PUFF_SCALE_END: f32 = 2.1;
/// Upward drift (yards/sec) — cloud rises as it dissipates.
const PUFF_RISE_SPEED: f32 = 0.7;
/// Soft white with a warm tint, which separates from both arena floors.
const PUFF_COLOR: Color = Color::srgba(1.0, 0.97, 0.92, 0.55);
const PUFF_EMISSIVE: LinearRgba = LinearRgba::new(2.6, 2.5, 2.3, 1.0);

/// Spawn a puff at a transforming unit's torso. `torso_world_y` comes from the
/// unit's [`VisualBody`] (pets render their body off the sim entity's `y`); a
/// unit without one falls back to its logical position.
pub(crate) fn spawn_transform_puff(commands: &mut Commands, unit_pos: Vec3, torso_world_y: Option<f32>) {
    let position = Vec3::new(
        unit_pos.x,
        torso_world_y.unwrap_or(unit_pos.y),
        unit_pos.z,
    );
    commands.spawn((
        TransformPuff {
            position,
            lifetime: PUFF_LIFETIME,
            initial_lifetime: PUFF_LIFETIME,
        },
        PlayMatchEntity,
    ));
}

/// Attach the cloud cluster to newly spawned [`TransformPuff`] markers.
///
/// The lobes are children sharing the parent's mesh and material, so the update
/// system animates the whole cluster by driving the PARENT alone — and the
/// cleanup despawn takes the children with it.
pub fn spawn_transform_puff_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_puffs: Query<(Entity, &TransformPuff), (Added<TransformPuff>, Without<Mesh3d>)>,
) {
    for (puff_entity, puff) in new_puffs.iter() {
        let ball = meshes.add(Sphere::new(1.0).mesh().uv(12, 8));
        let material = materials.add(StandardMaterial {
            base_color: PUFF_COLOR,
            emissive: PUFF_EMISSIVE,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(puff_entity).try_insert((
            Mesh3d(ball.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(puff.position)
                .with_scale(Vec3::splat(PUFF_SCALE_START * PUFF_CENTER_RADIUS)),
        ));

        // Lobe offsets are in the puff's own units, so the parent's scale
        // expands the cluster outward as well as inflating each lobe. The
        // parent's scale already carries the centre radius, so each lobe
        // divides it back out to reach its own.
        for (offset, radius) in PUFF_LOBES {
            let lobe = commands
                .spawn((
                    Mesh3d(ball.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(offset / PUFF_CENTER_RADIUS)
                        .with_scale(Vec3::splat(radius / PUFF_CENTER_RADIUS)),
                ))
                .id();
            commands.entity(puff_entity).add_child(lobe);
        }
    }
}

/// Expand, rise and fade the puff cluster.
pub fn update_transform_puffs(
    time: Res<Time>,
    mut puffs: Query<(&mut TransformPuff, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (mut puff, mut transform, material_handle) in puffs.iter_mut() {
        puff.lifetime -= dt;

        // Progress: 1.0 (just spawned) → 0.0 (expired).
        let progress = (puff.lifetime / puff.initial_lifetime).clamp(0.0, 1.0);
        let elapsed = 1.0 - progress;

        // sqrt easing: most of the expansion lands in the first few frames.
        let scale = PUFF_SCALE_START + (PUFF_SCALE_END - PUFF_SCALE_START) * elapsed.sqrt();
        transform.scale = Vec3::splat(scale * PUFF_CENTER_RADIUS);
        transform.translation.y = puff.position.y + PUFF_RISE_SPEED * elapsed * puff.initial_lifetime;

        // Fade on the square so the cloud thins out early and leaves no
        // lingering haze over the restored unit.
        let fade = progress * progress;
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = PUFF_COLOR.with_alpha(PUFF_COLOR.alpha() * fade);
            material.emissive = LinearRgba::new(
                PUFF_EMISSIVE.red * fade,
                PUFF_EMISSIVE.green * fade,
                PUFF_EMISSIVE.blue * fade,
                1.0,
            );
        }
    }
}

/// Cleanup expired transform puffs. Despawn is recursive, so the lobes go too.
pub fn cleanup_expired_transform_puffs(
    mut commands: Commands,
    puffs: Query<(Entity, &TransformPuff)>,
) {
    for (entity, puff) in puffs.iter() {
        if puff.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

