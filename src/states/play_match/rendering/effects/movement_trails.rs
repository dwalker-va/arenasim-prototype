use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Charge Trail Visual (Boar Charge)
// ==============================================================================

/// Spawn speed streak trail when a pet starts charging.
/// Uses `With<Pet>` filter to distinguish from Warrior charges.
pub fn spawn_charge_trail(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_charges: Query<(Entity, &Transform, &ChargingState), (Added<ChargingState>, With<Pet>)>,
    targets: Query<&Transform, Without<ChargingState>>,
) {
    for (_entity, transform, charging) in new_charges.iter() {
        // Determine direction from charger to target
        let direction = if let Ok(target_transform) = targets.get(charging.target) {
            (target_transform.translation - transform.translation).normalize_or_zero()
        } else {
            Vec3::Z
        };

        let mesh = meshes.add(Cylinder::new(0.25, 2.0));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.6, 0.5, 0.3, 0.35),
            emissive: LinearRgba::new(1.0, 0.8, 0.4, 1.0),
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        // Orient along charge direction
        let rotation = if direction != Vec3::ZERO {
            Quat::from_rotation_arc(Vec3::Y, direction)
        } else {
            Quat::IDENTITY
        };

        let trail_pos = transform.translation + Vec3::Y * 0.3;

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(trail_pos).with_rotation(rotation),
            ChargeTrail {
                lifetime: 0.3,
                initial_lifetime: 0.3,
            },
            PlayMatchEntity,
        ));
    }
}

/// Update and cleanup charge trails: fade and despawn when expired.
pub fn update_and_cleanup_charge_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut trails: Query<(Entity, &mut ChargeTrail, &MeshMaterial3d<StandardMaterial>)>,
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
            material.base_color = Color::srgba(0.6, 0.5, 0.3, 0.35 * progress);
            material.emissive = LinearRgba::new(
                1.0 * progress,
                0.8 * progress,
                0.4 * progress,
                1.0,
            );
        }
    }
}

