use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Spell Impact Visual Effects Systems
// ==============================================================================

/// Spawn visual meshes for newly created spell impact effects.
pub fn spawn_spell_impact_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_effects: Query<(Entity, &SpellImpactEffect), (Added<SpellImpactEffect>, Without<Mesh3d>)>,
) {
    for (effect_entity, effect) in new_effects.iter() {
        // Create a sphere mesh
        let mesh = meshes.add(Sphere::new(effect.initial_scale));

        // Purple/shadow color with emissive glow and transparency
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.5, 0.2, 0.8, 0.8), // Purple with alpha
            emissive: LinearRgba::rgb(0.8, 0.3, 1.5), // Bright purple/magenta glow
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        // Add visual mesh to the effect entity at the target's position
        // Use try_insert to safely handle cases where the entity was despawned
        // between when the query ran and when commands are applied
        commands.entity(effect_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(effect.position + Vec3::new(0.0, 1.0, 0.0)), // Centered at chest height
        ));
    }
}

/// Update spell impact effects: fade and scale them over time.
pub fn update_spell_impact_effects(
    time: Res<Time>,
    mut effects: Query<(&mut SpellImpactEffect, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (mut effect, mut transform, material_handle) in effects.iter_mut() {
        effect.lifetime -= dt;

        if effect.lifetime <= 0.0 {
            continue; // Will be cleaned up by cleanup system
        }

        // Calculate progress (1.0 = just spawned, 0.0 = expired)
        let progress = effect.lifetime / effect.initial_lifetime;

        // Scale: expand from initial to final
        let current_scale = effect.initial_scale + (effect.final_scale - effect.initial_scale) * (1.0 - progress);
        transform.scale = Vec3::splat(current_scale);

        // Fade out: alpha goes from 1.0 to 0.0
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let alpha = progress * 0.8; // Max alpha 0.8 for translucency
            material.base_color = Color::srgba(0.5, 0.2, 0.8, alpha);
            material.alpha_mode = AlphaMode::Blend;
        }
    }
}

/// Cleanup expired spell impact effects.
pub fn cleanup_expired_spell_impacts(
    mut commands: Commands,
    effects: Query<(Entity, &SpellImpactEffect)>,
) {
    for (entity, effect) in effects.iter() {
        if effect.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

