use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Disengage Trail Visual
// ==============================================================================

/// Spawn wind streak trail when a combatant starts Disengaging.
pub fn spawn_disengage_trail(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_disengages: Query<(Entity, &Transform, &DisengagingState), Added<DisengagingState>>,
) {
    for (_entity, transform, disengage) in new_disengages.iter() {
        // Elongated cylinder at the Hunter's start position
        let mesh = meshes.add(Cylinder::new(0.3, 3.0));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.85, 0.9, 1.0, 0.4),
            emissive: LinearRgba::new(1.5, 1.7, 2.0, 1.0),
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        // Orient cylinder along the disengage direction
        // Cylinder points up (Y axis), so rotate from Y to direction
        let direction = disengage.direction.normalize_or_zero();
        let rotation = if direction != Vec3::ZERO {
            Quat::from_rotation_arc(Vec3::Y, direction)
        } else {
            Quat::IDENTITY
        };

        let trail_pos = transform.translation + Vec3::Y * 0.5;

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(trail_pos).with_rotation(rotation),
            DisengageTrail {
                lifetime: 0.5,
                initial_lifetime: 0.5,
            },
            PlayMatchEntity,
        ));
    }
}

/// Update and cleanup disengage trails: fade alpha and despawn when expired.
pub fn update_and_cleanup_disengage_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut trails: Query<(Entity, &mut DisengageTrail, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (entity, mut trail, material_handle) in trails.iter_mut() {
        trail.lifetime -= dt;

        if trail.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        let progress = (trail.lifetime / trail.initial_lifetime).max(0.0);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.85, 0.9, 1.0, 0.4 * progress);
            material.emissive = LinearRgba::new(
                1.5 * progress,
                1.7 * progress,
                2.0 * progress,
                1.0,
            );
        }
    }
}

