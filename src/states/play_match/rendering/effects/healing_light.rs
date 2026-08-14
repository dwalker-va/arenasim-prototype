use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;
use crate::states::match_config::CharacterClass;

// ==============================================================================
// Healing Light Column Systems
// ==============================================================================

/// Returns (base_color, emissive) for healing light based on healer class.
/// Priest heals are white-gold (brighter), Paladin heals are golden (warmer).
fn healing_light_colors(class: CharacterClass) -> (Color, LinearRgba) {
    match class {
        CharacterClass::Priest => (
            // White-gold: brighter, less yellow
            Color::srgba(1.0, 1.0, 0.9, 0.35),
            LinearRgba::new(2.8, 2.8, 2.4, 1.0),
        ),
        CharacterClass::Paladin => (
            // Golden: warmer, more yellow
            Color::srgba(1.0, 0.9, 0.6, 0.35),
            LinearRgba::new(2.5, 2.0, 1.0, 1.0),
        ),
        CharacterClass::Shaman => (
            // Elemental blue (Shaman class color): cool water/wave heal
            Color::srgba(0.3, 0.6, 1.0, 0.35),
            LinearRgba::new(1.2, 2.0, 3.0, 1.0),
        ),
        _ => (
            // Fallback golden
            Color::srgba(1.0, 0.95, 0.7, 0.35),
            LinearRgba::new(2.5, 2.2, 1.2, 1.0),
        ),
    }
}

/// Spawn visual mesh for newly created healing light columns.
pub fn spawn_healing_light_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_columns: Query<(Entity, &HealingLightColumn), (Added<HealingLightColumn>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (column_entity, column) in new_columns.iter() {
        let Ok(target_transform) = transforms.get(column.target) else {
            continue;
        };

        let (base_color, emissive) = healing_light_colors(column.healer_class);

        let mesh = meshes.add(Cylinder::new(0.7, 3.5));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.0;

        commands.entity(column_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update healing light columns: follow target and fade over time.
pub fn update_healing_light_columns(
    time: Res<Time>,
    mut columns: Query<(&mut HealingLightColumn, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<HealingLightColumn>>,
) {
    let dt = time.delta_secs();

    for (mut column, mut column_transform, material_handle) in columns.iter_mut() {
        column.lifetime -= dt;

        // Update position to follow target (if target still exists)
        if let Ok(target_transform) = transforms.get(column.target) {
            column_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        // Fade based on remaining lifetime
        let progress = (column.lifetime / column.initial_lifetime).max(0.0);
        let (base_color, emissive) = healing_light_colors(column.healer_class);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            // Scale alpha by progress for fade
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

/// Cleanup expired healing light columns.
pub fn cleanup_expired_healing_lights(
    mut commands: Commands,
    columns: Query<(Entity, &HealingLightColumn)>,
) {
    for (entity, column) in columns.iter() {
        if column.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

