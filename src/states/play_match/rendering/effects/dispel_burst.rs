use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;
use crate::states::match_config::CharacterClass;

// ==============================================================================
// Dispel Burst Visual Effects
// ==============================================================================

/// Returns (base_color, emissive) for dispel burst based on caster class.
pub(crate) fn dispel_burst_colors(class: CharacterClass) -> (Color, LinearRgba) {
    match class {
        CharacterClass::Priest => (
            // White/silver with slight blue tint
            Color::srgba(0.85, 0.85, 1.0, 0.5),
            LinearRgba::new(2.0, 2.0, 2.8, 1.0),
        ),
        CharacterClass::Paladin => (
            // Golden (matches Paladin healing color)
            Color::srgba(1.0, 0.9, 0.6, 0.5),
            LinearRgba::new(2.5, 2.0, 1.0, 1.0),
        ),
        CharacterClass::Hunter => (
            // Hunter gold (Master's Call)
            Color::srgba(1.0, 0.85, 0.3, 0.5),
            LinearRgba::new(2.0, 1.7, 0.6, 1.0),
        ),
        CharacterClass::Shaman => (
            // Elemental blue (Shaman class color): Purge burst reads as a wave
            Color::srgba(0.3, 0.6, 1.0, 0.5),
            LinearRgba::new(1.2, 2.0, 3.0, 1.0),
        ),
        _ => (
            Color::srgba(0.9, 0.9, 1.0, 0.5),
            LinearRgba::new(2.0, 2.0, 2.5, 1.0),
        ),
    }
}

/// Spawn visual mesh for new dispel bursts.
pub fn spawn_dispel_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &DispelBurst), (Added<DispelBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let Ok(target_transform) = transforms.get(burst.target) else {
            continue;
        };

        let (base_color, emissive) = dispel_burst_colors(burst.caster_class);

        let mesh = meshes.add(Sphere::new(0.3));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.0;

        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update dispel bursts: expand sphere and fade out.
pub fn update_dispel_bursts(
    time: Res<Time>,
    mut bursts: Query<(&mut DispelBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<DispelBurst>>,
) {
    for (mut burst, mut burst_transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= time.delta_secs();

        // Follow target position
        if let Ok(target_transform) = transforms.get(burst.target) {
            burst_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired)
        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);

        // Scale up as it expands (1.0 → 3.0)
        let scale = 1.0 + (1.0 - progress) * 2.0;
        burst_transform.scale = Vec3::splat(scale);

        // Fade out
        let (base_color, emissive) = dispel_burst_colors(burst.caster_class);
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

/// Cleanup expired dispel bursts.
pub fn cleanup_expired_dispel_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &DispelBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

