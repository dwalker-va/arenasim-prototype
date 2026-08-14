use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Drain Life Beam Visual Effects
// ==============================================================================

use crate::states::play_match::abilities::AbilityType;

/// Spawn Drain Life beams when a combatant starts channeling Drain Life.
/// Detects newly added ChannelingState components with DrainLife ability.
pub fn spawn_drain_life_beams(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_channels: Query<(Entity, &ChannelingState), Added<ChannelingState>>,
    existing_beams: Query<&DrainLifeBeam>,
) {
    for (caster_entity, channeling) in new_channels.iter() {
        // Only create beam for Drain Life
        if channeling.ability != AbilityType::DrainLife {
            continue;
        }

        // Check if beam already exists for this caster (avoid duplicates)
        let beam_exists = existing_beams.iter().any(|beam| beam.caster == caster_entity);
        if beam_exists {
            continue;
        }

        // Create cylinder mesh for the beam
        // Cylinder height is 1.0 by default, we'll scale it to match distance
        let mesh = meshes.add(Cylinder::new(0.15, 1.0));

        // Purple shadow color with bright emissive glow
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.7, 0.3, 0.9, 0.8),
            emissive: LinearRgba::rgb(3.0, 1.0, 4.0),
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        // Spawn the beam entity
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::default(),
            DrainLifeBeam {
                caster: caster_entity,
                target: channeling.target,
                particle_spawn_timer: 0.0,
            },
            PlayMatchEntity,
        ));
    }
}

/// Update Drain Life beam positions to connect caster and target.
/// Positions beam at midpoint, scales to match distance, rotates to point correctly.
pub fn update_drain_life_beams(
    mut beams: Query<(&DrainLifeBeam, &mut Transform)>,
    positions: Query<&Transform, (With<Combatant>, Without<DrainLifeBeam>)>,
) {
    for (beam, mut beam_transform) in beams.iter_mut() {
        // Get caster and target positions
        let Ok(caster_transform) = positions.get(beam.caster) else {
            continue;
        };
        let Ok(target_transform) = positions.get(beam.target) else {
            continue;
        };

        // Add Y offset for chest height (combatant transform is at ~1.0 already)
        let caster_pos = caster_transform.translation + Vec3::Y * 0.5;
        let target_pos = target_transform.translation + Vec3::Y * 0.5;

        // Calculate direction and distance
        let direction = target_pos - caster_pos;
        let distance = direction.length();

        if distance < 0.01 {
            continue; // Avoid division by zero
        }

        let normalized_dir = direction.normalize();

        // Position beam at midpoint
        beam_transform.translation = (caster_pos + target_pos) / 2.0;

        // Scale Y to match distance (cylinder default height is 1.0)
        beam_transform.scale = Vec3::new(1.0, distance, 1.0);

        // Rotate to point from caster to target
        // Cylinder points up (Y axis), so we rotate from Y to our direction
        beam_transform.rotation = Quat::from_rotation_arc(Vec3::Y, normalized_dir);
    }
}

/// Spawn particles along the Drain Life beam at regular intervals.
pub fn spawn_drain_particles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut beams: Query<(Entity, &mut DrainLifeBeam, &Transform)>,
    positions: Query<&Transform, (With<Combatant>, Without<DrainLifeBeam>)>,
) {
    let dt = time.delta_secs();

    for (beam_entity, mut beam, _beam_transform) in beams.iter_mut() {
        // Decrement spawn timer
        beam.particle_spawn_timer -= dt;

        if beam.particle_spawn_timer <= 0.0 {
            // Reset timer (~12-13 particles per second)
            beam.particle_spawn_timer = 0.08;

            // Get target position for initial particle placement
            let Ok(target_transform) = positions.get(beam.target) else {
                continue;
            };

            let particle_pos = target_transform.translation + Vec3::Y * 0.5;

            // Create sphere mesh for particle
            let mesh = meshes.add(Sphere::new(0.18));

            // Bright purple/magenta with strong emissive glow
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(0.9, 0.5, 1.0, 1.0),
                emissive: LinearRgba::rgb(4.0, 2.0, 5.0),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });

            // Spawn particle at target position (progress = 0.0)
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(particle_pos),
                DrainParticle {
                    progress: 0.0,
                    speed: 0.4, // ~2.5 second travel time
                    beam: beam_entity,
                },
                PlayMatchEntity,
            ));
        }
    }
}

/// Move Drain particles along the beam from target to caster.
pub fn update_drain_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut DrainParticle, &mut Transform)>,
    beams: Query<&DrainLifeBeam>,
    positions: Query<&Transform, (With<Combatant>, Without<DrainLifeBeam>, Without<DrainParticle>)>,
) {
    let dt = time.delta_secs();

    for (entity, mut particle, mut particle_transform) in particles.iter_mut() {
        // Get the beam this particle belongs to
        let Ok(beam) = beams.get(particle.beam) else {
            // Beam was despawned, remove particle
            commands.entity(entity).despawn();
            continue;
        };

        // Increment progress
        particle.progress += particle.speed * dt;

        // Despawn when reached caster
        if particle.progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Get caster and target positions
        let Ok(caster_transform) = positions.get(beam.caster) else {
            commands.entity(entity).despawn();
            continue;
        };
        let Ok(target_transform) = positions.get(beam.target) else {
            commands.entity(entity).despawn();
            continue;
        };

        // Calculate current position along beam (lerp from target to caster)
        let caster_pos = caster_transform.translation + Vec3::Y * 0.5;
        let target_pos = target_transform.translation + Vec3::Y * 0.5;

        // progress: 0.0 = at target, 1.0 = at caster
        particle_transform.translation = target_pos.lerp(caster_pos, particle.progress);
    }
}

/// Cleanup Drain Life beams when the channel ends or is interrupted.
pub fn cleanup_drain_life_beams(
    mut commands: Commands,
    beams: Query<(Entity, &DrainLifeBeam)>,
    channeling_query: Query<&ChannelingState>,
    particles: Query<(Entity, &DrainParticle)>,
) {
    for (beam_entity, beam) in beams.iter() {
        // Check if caster still has a Drain Life channel active
        let still_channeling = channeling_query
            .get(beam.caster)
            .map(|c| c.ability == AbilityType::DrainLife && !c.interrupted)
            .unwrap_or(false);

        if !still_channeling {
            // Despawn all particles belonging to this beam
            for (particle_entity, particle) in particles.iter() {
                if particle.beam == beam_entity {
                    commands.entity(particle_entity).despawn();
                }
            }

            // Despawn the beam itself
            commands.entity(beam_entity).despawn();
        }
    }
}

