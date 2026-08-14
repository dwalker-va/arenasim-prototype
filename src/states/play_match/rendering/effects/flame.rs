use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Flame Particle Visual Effects (Immolate)
// ==============================================================================

/// Update flame particles: move upward, shrink, and despawn when expired.
pub fn update_flame_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut FlameParticle, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (entity, mut particle, mut transform) in particles.iter_mut() {
        particle.lifetime -= dt;

        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Move in velocity direction (primarily upward)
        transform.translation += particle.velocity * dt;

        // Shrink as lifetime decreases
        let life_ratio = (particle.lifetime / particle.initial_lifetime).max(0.1);
        transform.scale = Vec3::splat(life_ratio);
    }
}

/// Spawn visual meshes for newly created flame particles.
/// Creates small glowing orange/red spheres.
pub fn spawn_flame_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_particles: Query<(Entity, &FlameParticle), (Added<FlameParticle>, Without<Mesh3d>)>,
) {
    for (entity, _particle) in new_particles.iter() {
        // Create a small sphere mesh for the flame particle
        let mesh = meshes.add(Sphere::new(0.15));

        // Fire colors - orange base with bright emissive glow
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.4, 0.1, 0.9),
            emissive: LinearRgba::rgb(2.0, 0.8, 0.1),  // Bright orange glow
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        // Add visual mesh to the particle entity
        commands.entity(entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

