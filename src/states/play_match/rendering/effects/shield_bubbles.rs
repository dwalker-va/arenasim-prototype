use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::components::*;

// ==============================================================================
// Shield Bubble Visual Effects
// ==============================================================================

/// System to spawn and despawn shield bubble visual effects based on Absorb auras.
/// Creates a translucent sphere around combatants with active absorb shields.
pub fn update_shield_bubbles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    combatants: Query<(Entity, &Transform, Option<&ActiveAuras>), With<Combatant>>,
    existing_bubbles: Query<(Entity, &ShieldBubble)>,
) {
    use std::collections::HashSet;

    // Track which combatants currently have shield bubbles
    let mut combatants_with_bubbles: HashSet<Entity> = HashSet::new();
    for (_, bubble) in existing_bubbles.iter() {
        combatants_with_bubbles.insert(bubble.combatant);
    }

    // Track which combatants need bubbles: (entity, position, spell_school, is_immunity)
    let mut combatants_needing_bubbles: Vec<(Entity, Vec3, SpellSchool, bool)> = Vec::new();
    let mut combatants_with_shield: HashSet<Entity> = HashSet::new();

    for (entity, transform, auras) in combatants.iter() {
        if let Some(auras) = auras {
            // Check for DamageImmunity auras (Divine Shield) — takes priority over absorb
            let has_immunity = auras.auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity);
            if has_immunity {
                combatants_with_shield.insert(entity);
                if !combatants_with_bubbles.contains(&entity) {
                    combatants_needing_bubbles.push((entity, transform.translation, SpellSchool::Holy, true));
                }
                continue; // Don't also spawn absorb bubble
            }

            // Check for Absorb auras
            for aura in &auras.auras {
                if aura.effect_type == AuraType::Absorb && aura.magnitude > 0.0 {
                    combatants_with_shield.insert(entity);

                    // Determine spell school based on ability name
                    let spell_school = if aura.ability_name.contains("Ice Barrier") {
                        SpellSchool::Frost
                    } else {
                        SpellSchool::Holy // Power Word: Shield
                    };

                    // If combatant doesn't have a bubble yet, spawn one
                    if !combatants_with_bubbles.contains(&entity) {
                        combatants_needing_bubbles.push((entity, transform.translation, spell_school, false));
                    }
                    break; // Only need one absorb aura to spawn bubble
                }
            }
        }
    }

    // Spawn bubbles for combatants that need them
    for (combatant_entity, position, spell_school, is_immunity) in combatants_needing_bubbles {
        // Color based on spell school and immunity status
        // Emissive uses LinearRgba with pre-scaled values (2x for glow effect)
        let (base_color, emissive) = if is_immunity {
            // Divine Shield: bright gold, more opaque and glowing
            (
                Color::srgba(1.0, 0.85, 0.3, 0.4),
                LinearRgba::new(3.0, 2.5, 0.8, 1.0),
            )
        } else {
            match spell_school {
                SpellSchool::Frost => (
                    Color::srgba(0.4, 0.7, 1.0, 0.25), // Light blue, translucent
                    LinearRgba::new(0.4, 1.0, 2.0, 1.0), // Blue glow (2x scaled)
                ),
                SpellSchool::Holy => (
                    Color::srgba(1.0, 0.95, 0.7, 0.25), // Golden/white, translucent
                    LinearRgba::new(2.0, 1.8, 1.0, 1.0), // Golden glow (2x scaled)
                ),
                _ => (
                    Color::srgba(0.8, 0.8, 0.8, 0.25), // Default grey
                    LinearRgba::new(1.0, 1.0, 1.0, 1.0),
                ),
            }
        };

        // Use unit sphere stretched into egg shape to encompass combatant
        let mesh = meshes.add(Sphere::new(1.0));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            // Use additive blending to avoid depth sorting flicker
            alpha_mode: AlphaMode::Add,
            // Disable depth writes so bubble doesn't interfere with other objects
            depth_bias: 0.0,
            ..default()
        });

        // Stretch sphere into tall narrow ellipsoid like WoW's shield bubble
        // Combatant transform is at capsule center (~y=1.0), so no Y offset needed
        // Divine Shield bubble is 1.3x larger than absorb shields
        let scale_factor = if is_immunity { 1.3 } else { 1.0 };
        let transform = Transform::from_translation(position)
            .with_scale(Vec3::new(0.9 * scale_factor, 1.4 * scale_factor, 0.9 * scale_factor));

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            ShieldBubble {
                combatant: combatant_entity,
                spell_school,
                is_immunity,
            },
            PlayMatchEntity,
        ));
    }

    // Despawn bubbles for combatants without shield auras
    for (bubble_entity, bubble) in existing_bubbles.iter() {
        if !combatants_with_shield.contains(&bubble.combatant) {
            commands.entity(bubble_entity).despawn();
        }
    }
}

/// System to update shield bubble positions to follow their combatants.
/// Immunity bubbles (Divine Shield) get a gentle pulse animation.
pub fn follow_shield_bubbles(
    time: Res<Time>,
    combatants: Query<&Transform, With<Combatant>>,
    mut bubbles: Query<(&ShieldBubble, &mut Transform), Without<Combatant>>,
) {
    for (bubble, mut bubble_transform) in bubbles.iter_mut() {
        if let Ok(combatant_transform) = combatants.get(bubble.combatant) {
            // Combatant transform is at capsule center, so use directly
            bubble_transform.translation = combatant_transform.translation;

            // Immunity bubbles pulse gently (scale oscillation)
            if bubble.is_immunity {
                let pulse = 1.0 + 0.05 * (time.elapsed_secs() * 3.0).sin();
                let base = 1.3; // Immunity base scale factor
                bubble_transform.scale = Vec3::new(
                    0.9 * base * pulse,
                    1.4 * base * pulse,
                    0.9 * base * pulse,
                );
            }
        }
    }
}

