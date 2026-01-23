//! Visual Effects Systems
//!
//! Floating combat text, spell impact effects, speech bubbles, and shield bubbles.

use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy_egui::{egui, EguiContexts};
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::components::*;

// ==============================================================================
// Floating Combat Text Systems
// ==============================================================================

/// Update floating combat text - make it float upward and fade over time.
///
/// Each FCT floats upward at a constant speed and decreases its lifetime.
/// Expired FCT is not removed here (see `cleanup_expired_floating_text`).
pub fn update_floating_combat_text(
    time: Res<Time>,
    mut floating_texts: Query<&mut FloatingCombatText>,
) {
    let dt = time.delta_secs();

    for mut fct in floating_texts.iter_mut() {
        // Float upward
        fct.vertical_offset += 1.5 * dt; // Rise at 1.5 units/sec
        fct.world_position.y += 1.5 * dt;

        // Decrease lifetime
        fct.lifetime -= dt;
    }
}

/// Render floating combat text as 2D overlay.
///
/// Projects 3D world positions to 2D screen space and renders damage numbers.
/// Text fades out as lifetime decreases (alpha based on remaining lifetime).
pub fn render_floating_combat_text(
    mut contexts: EguiContexts,
    floating_texts: Query<&FloatingCombatText>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

    egui::Area::new(egui::Id::new("floating_combat_text"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            for fct in floating_texts.iter() {
                // Only render if still alive
                if fct.lifetime <= 0.0 {
                    continue;
                }

                // Project 3D position to 2D screen space
                if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, fct.world_position) {
                    // Calculate alpha based on remaining lifetime
                    // Fade out in the last 0.5 seconds
                    let alpha = if fct.lifetime < 0.5 {
                        (fct.lifetime / 0.5 * 255.0) as u8
                    } else {
                        255
                    };

                    // Apply alpha to color
                    let color_with_alpha = egui::Color32::from_rgba_unmultiplied(
                        fct.color.r(),
                        fct.color.g(),
                        fct.color.b(),
                        alpha,
                    );
                    let outline_color = egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha);

                    // Check if this is absorbed text - render number and label separately
                    if let Some(number_str) = fct.text.strip_suffix(" absorbed") {
                        // Render number at 24pt
                        let number_font = egui::FontId::proportional(24.0);
                        let label_font = egui::FontId::proportional(14.0);

                        // Calculate positions - number centered, label to the right
                        let number_galley = ui.painter().layout_no_wrap(number_str.to_string(), number_font.clone(), color_with_alpha);
                        let label_galley = ui.painter().layout_no_wrap("absorbed".to_string(), label_font.clone(), color_with_alpha);
                        let total_width = number_galley.size().x + 4.0 + label_galley.size().x;
                        let number_x = screen_pos.x - total_width / 2.0 + number_galley.size().x / 2.0;
                        let label_x = number_x + number_galley.size().x / 2.0 + 4.0 + label_galley.size().x / 2.0;

                        // Draw number outline
                        for (dx, dy) in [
                            (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0),
                            (-1.5, -1.5), (1.5, -1.5), (-1.5, 1.5), (1.5, 1.5),
                        ] {
                            ui.painter().text(
                                egui::pos2(number_x + dx, screen_pos.y + dy),
                                egui::Align2::CENTER_CENTER,
                                number_str,
                                number_font.clone(),
                                outline_color,
                            );
                        }
                        // Draw number
                        ui.painter().text(
                            egui::pos2(number_x, screen_pos.y),
                            egui::Align2::CENTER_CENTER,
                            number_str,
                            number_font,
                            color_with_alpha,
                        );

                        // Draw label outline (smaller offset for smaller text)
                        for (dx, dy) in [
                            (-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5),
                            (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0),
                        ] {
                            ui.painter().text(
                                egui::pos2(label_x + dx, screen_pos.y + 2.0 + dy),
                                egui::Align2::CENTER_CENTER,
                                "absorbed",
                                label_font.clone(),
                                outline_color,
                            );
                        }
                        // Draw label (slightly lower to align with number baseline)
                        ui.painter().text(
                            egui::pos2(label_x, screen_pos.y + 2.0),
                            egui::Align2::CENTER_CENTER,
                            "absorbed",
                            label_font,
                            color_with_alpha,
                        );
                    } else {
                        // Regular text - render normally at 24pt
                        let font_id = egui::FontId::proportional(24.0);

                        // Draw thick black outline (8 directions for smooth outline)
                        for (dx, dy) in [
                            (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0),
                            (-1.5, -1.5), (1.5, -1.5), (-1.5, 1.5), (1.5, 1.5),
                        ] {
                            ui.painter().text(
                                egui::pos2(screen_pos.x + dx, screen_pos.y + dy),
                                egui::Align2::CENTER_CENTER,
                                &fct.text,
                                font_id.clone(),
                                outline_color,
                            );
                        }

                        // Draw main text
                        ui.painter().text(
                            egui::pos2(screen_pos.x, screen_pos.y),
                            egui::Align2::CENTER_CENTER,
                            &fct.text,
                            font_id,
                            color_with_alpha,
                        );
                    }
                }
            }
        });
}

/// Cleanup expired floating combat text.
///
/// Despawns FCT entities when their lifetime reaches zero.
pub fn cleanup_expired_floating_text(
    mut commands: Commands,
    floating_texts: Query<(Entity, &FloatingCombatText)>,
) {
    for (entity, fct) in floating_texts.iter() {
        if fct.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

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
            commands.entity(entity).despawn_recursive();
        }
    }
}

// ==============================================================================
// Speech Bubble Systems
// ==============================================================================

/// Render speech bubbles above combatants' heads
pub fn render_speech_bubbles(
    mut contexts: EguiContexts,
    speech_bubbles: Query<&SpeechBubble>,
    combatants: Query<&Transform, With<Combatant>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    for bubble in speech_bubbles.iter() {
        // Get owner's position
        let Ok(owner_transform) = combatants.get(bubble.owner) else {
            continue;
        };

        // Position above the combatant's head
        let bubble_world_pos = owner_transform.translation + Vec3::new(0.0, 4.0, 0.0);

        // Project to screen space
        let Ok(screen_pos) = camera.world_to_viewport(camera_transform, bubble_world_pos) else {
            continue;
        };

        // Measure text to make bubble fit snugly
        let font_id = egui::FontId::proportional(14.0);
        let galley = ctx.fonts(|f| f.layout_no_wrap(bubble.text.clone(), font_id.clone(), egui::Color32::BLACK));

        // Tight padding around text
        let padding = egui::vec2(12.0, 6.0);
        let bubble_size = galley.size() + padding * 2.0;
        let bubble_pos = egui::pos2(
            screen_pos.x - bubble_size.x / 2.0,
            screen_pos.y - bubble_size.y / 2.0,
        );

        let rect = egui::Rect::from_min_size(bubble_pos, bubble_size);

        // Paint speech bubble background
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(format!("speech_bubble_{:?}", bubble.owner)),
        ));

        // White rounded rectangle background
        painter.rect_filled(
            rect,
            egui::Rounding::same(6.0),
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240),
        );

        // Black border
        painter.rect_stroke(
            rect,
            egui::Rounding::same(6.0),
            egui::Stroke::new(2.0, egui::Color32::BLACK),
        );

        // Draw text
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &bubble.text,
            egui::FontId::proportional(14.0),
            egui::Color32::BLACK,
        );
    }
}

/// Update speech bubble lifetimes and remove expired ones
pub fn update_speech_bubbles(
    time: Res<Time>,
    mut commands: Commands,
    mut bubbles: Query<(Entity, &mut SpeechBubble)>,
) {
    let dt = time.delta_secs();

    for (entity, mut bubble) in bubbles.iter_mut() {
        bubble.lifetime -= dt;

        if bubble.lifetime <= 0.0 {
            commands.entity(entity).despawn_recursive();
        }
    }
}

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

    // Track which combatants need bubbles
    let mut combatants_needing_bubbles: Vec<(Entity, Vec3, SpellSchool)> = Vec::new();
    let mut combatants_with_absorb: HashSet<Entity> = HashSet::new();

    for (entity, transform, auras) in combatants.iter() {
        if let Some(auras) = auras {
            // Check for Absorb auras
            for aura in &auras.auras {
                if aura.effect_type == AuraType::Absorb && aura.magnitude > 0.0 {
                    combatants_with_absorb.insert(entity);

                    // Determine spell school based on ability name
                    let spell_school = if aura.ability_name.contains("Ice Barrier") {
                        SpellSchool::Frost
                    } else {
                        SpellSchool::Holy // Power Word: Shield
                    };

                    // If combatant doesn't have a bubble yet, spawn one
                    if !combatants_with_bubbles.contains(&entity) {
                        combatants_needing_bubbles.push((entity, transform.translation, spell_school));
                    }
                    break; // Only need one absorb aura to spawn bubble
                }
            }
        }
    }

    // Spawn bubbles for combatants that need them
    for (combatant_entity, position, spell_school) in combatants_needing_bubbles {
        // Color based on spell school
        // Emissive uses LinearRgba with pre-scaled values (2x for glow effect)
        let (base_color, emissive) = match spell_school {
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
        // Scale large enough to fully encompass the combatant capsule without intersection
        let transform = Transform::from_translation(position)
            .with_scale(Vec3::new(0.9, 1.4, 0.9));

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            ShieldBubble {
                combatant: combatant_entity,
                spell_school,
            },
            PlayMatchEntity,
        ));
    }

    // Despawn bubbles for combatants without absorb auras
    for (bubble_entity, bubble) in existing_bubbles.iter() {
        if !combatants_with_absorb.contains(&bubble.combatant) {
            commands.entity(bubble_entity).despawn_recursive();
        }
    }
}

/// System to update shield bubble positions to follow their combatants.
pub fn follow_shield_bubbles(
    combatants: Query<&Transform, With<Combatant>>,
    mut bubbles: Query<(&ShieldBubble, &mut Transform), Without<Combatant>>,
) {
    for (bubble, mut bubble_transform) in bubbles.iter_mut() {
        if let Ok(combatant_transform) = combatants.get(bubble.combatant) {
            // Combatant transform is at capsule center, so use directly
            bubble_transform.translation = combatant_transform.translation;
        }
    }
}

// ==============================================================================
// Polymorph Visual Effect System
// ==============================================================================

/// System that swaps combatant meshes when polymorphed.
/// Polymorphed combatants are rendered as a cuboid to show the "transfiguration" effect.
pub fn update_polymorph_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut combatants: Query<(
        Entity,
        &ActiveAuras,
        &OriginalMesh,
        &mut Mesh3d,
        Option<&PolymorphedVisual>,
    ), With<Combatant>>,
) {
    for (entity, auras, original_mesh, mut mesh3d, polymorphed_marker) in combatants.iter_mut() {
        let is_polymorphed = auras.auras.iter().any(|a| a.effect_type == AuraType::Polymorph);

        if is_polymorphed && polymorphed_marker.is_none() {
            // Combatant just got polymorphed - swap to cuboid (sheep/pig box shape)
            // Using a squat cuboid to represent the transformed creature
            let poly_mesh = meshes.add(Cuboid::new(0.8, 0.6, 1.0));
            *mesh3d = Mesh3d(poly_mesh);
            commands.entity(entity).insert(PolymorphedVisual);
        } else if !is_polymorphed && polymorphed_marker.is_some() {
            // Polymorph ended - restore original capsule mesh
            *mesh3d = Mesh3d(original_mesh.0.clone());
            commands.entity(entity).remove::<PolymorphedVisual>();
        }
    }
}

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
            commands.entity(entity).despawn_recursive();
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
