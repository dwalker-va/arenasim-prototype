use bevy::prelude::*;
use crate::states::play_match::components::*;
use super::traps::{trap_type_rgb, trap_type_emissive};

// ==============================================================================
// Slow Zone Visual (spawned on SlowZone entity via Added<SlowZone>)
// ==============================================================================

/// Spawn flat cyan disc on newly created slow zones.
pub fn spawn_slow_zone_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_zones: Query<(Entity, &SlowZone), (Added<SlowZone>, Without<Mesh3d>)>,
) {
    for (zone_entity, zone) in new_zones.iter() {
        let mesh = meshes.add(Cylinder::new(zone.radius, 0.03));
        let (r, g, b) = trap_type_rgb(TrapType::Frost); // Slow zones are always Frost Trap
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(r, g, b, 0.2),
            emissive: trap_type_emissive(TrapType::Frost),
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(zone_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

/// Update slow zone visuals: gentle alpha pulse, fade out in last 2 seconds.
pub fn update_slow_zone_visuals(
    time: Res<Time>,
    zones: Query<(&SlowZone, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();

    for (zone, material_handle) in zones.iter() {
        let Some(material) = materials.get_mut(&material_handle.0) else {
            continue;
        };

        // Gentle sine pulse (period ~2s)
        let base_alpha = 0.15 + 0.05 * (t * std::f32::consts::PI).sin();

        // Fade out in last 2 seconds
        let alpha = if zone.duration_remaining < 2.0 {
            base_alpha * (zone.duration_remaining / 2.0).max(0.0)
        } else {
            base_alpha
        };

        let (r, g, b) = trap_type_rgb(TrapType::Frost);
        material.base_color = Color::srgba(r, g, b, alpha);
    }
}

