use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Psychic Scream Burst (self-centered AoE fear)
// ==============================================================================

/// Shadow-violet color for the Psychic Scream burst (base, emissive).
fn scream_burst_colors() -> (Color, LinearRgba) {
    (
        Color::srgba(0.45, 0.12, 0.6, 0.55),
        LinearRgba::new(0.7, 0.2, 0.9, 1.0),
    )
}

/// Spawn the visual mesh for new Psychic Scream bursts (graphical-only).
pub fn spawn_scream_burst(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &ScreamBurst), (Added<ScreamBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let Ok(caster_transform) = transforms.get(burst.caster) else {
            continue;
        };

        let (base_color, emissive) = scream_burst_colors();

        let mesh = meshes.add(Sphere::new(1.0));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = caster_transform.translation + Vec3::Y * 1.0;

        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update Psychic Scream bursts: expand outward toward the AoE radius and fade.
pub fn update_scream_bursts(
    time: Res<Time>,
    mut bursts: Query<(&mut ScreamBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<ScreamBurst>>,
) {
    for (mut burst, mut burst_transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= time.delta_secs();

        // Keep the burst centered on the caster.
        if let Ok(caster_transform) = transforms.get(burst.caster) {
            burst_transform.translation = caster_transform.translation + Vec3::Y * 1.0;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired).
        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);

        // Expand a ring out toward the ~8yd scream radius (1.0 → 8.0).
        let scale = 1.0 + (1.0 - progress) * 7.0;
        burst_transform.scale = Vec3::splat(scale);

        // Fade out as it expands.
        let (base_color, emissive) = scream_burst_colors();
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

/// Cleanup expired Psychic Scream bursts.
pub fn cleanup_expired_scream_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &ScreamBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

