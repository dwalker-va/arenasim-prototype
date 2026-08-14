use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Death Coil Burst (target-centered horror impact)
// ==============================================================================

/// Vivid skull-green for the Death Coil impact (base, emissive). Emissive is
/// pushed well above 1.0 so the flash blooms — Death Coil's whole problem is
/// being too subtle, so this errs bright.
fn death_coil_burst_colors() -> (Color, LinearRgba) {
    (
        Color::srgba(0.25, 0.95, 0.4, 0.7),
        LinearRgba::new(0.5, 2.6, 0.9, 1.0),
    )
}

/// Spawn the visual mesh for new Death Coil bursts (graphical-only).
pub fn spawn_death_coil_burst(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &DeathCoilBurst), (Added<DeathCoilBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let Ok(target_transform) = transforms.get(burst.target) else {
            continue;
        };

        let (base_color, emissive) = death_coil_burst_colors();

        let mesh = meshes.add(Sphere::new(0.6));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.2;

        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update Death Coil bursts: a hot flash that punches outward then fades.
pub fn update_death_coil_bursts(
    time: Res<Time>,
    mut bursts: Query<(&mut DeathCoilBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<DeathCoilBurst>>,
) {
    for (mut burst, mut burst_transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= time.delta_secs();

        // Stay pinned on the (possibly fleeing) target.
        if let Ok(target_transform) = transforms.get(burst.target) {
            burst_transform.translation = target_transform.translation + Vec3::Y * 1.2;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired).
        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);
        let grow = 1.0 - progress; // 0 → 1 over life

        // Punch from a tight bright core out to ~2.8yd.
        let scale = 0.5 + grow * 2.8;
        burst_transform.scale = Vec3::splat(scale);

        // Hold the flash bright early, then fade fast (progress^1.5 ≈ ease-out).
        let (base_color, emissive) = death_coil_burst_colors();
        let fade = progress * progress.sqrt();
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = base_color.with_alpha(base_color.alpha() * fade);
            material.emissive = LinearRgba::new(
                emissive.red * fade,
                emissive.green * fade,
                emissive.blue * fade,
                1.0,
            );
        }
    }
}

/// Cleanup expired Death Coil bursts.
pub fn cleanup_expired_death_coil_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &DeathCoilBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

