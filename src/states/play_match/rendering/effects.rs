//! Visual Effects Systems
//!
//! Floating combat text, spell impact effects, speech bubbles, shield bubbles, and dispel bursts.

use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy_egui::{egui, EguiContexts};
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::components::*;
use crate::states::match_config::CharacterClass;

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
                        // Regular text - 32pt for crits, 24pt for normal
                        let font_size = if fct.is_crit { 32.0 } else { 24.0 };
                        let display_text = if fct.is_crit {
                            format!("{}!", fct.text)
                        } else {
                            fct.text.clone()
                        };
                        let font_id = egui::FontId::proportional(font_size);

                        // Draw thick black outline (8 directions for smooth outline)
                        for (dx, dy) in [
                            (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0),
                            (-1.5, -1.5), (1.5, -1.5), (-1.5, 1.5), (1.5, 1.5),
                        ] {
                            ui.painter().text(
                                egui::pos2(screen_pos.x + dx, screen_pos.y + dy),
                                egui::Align2::CENTER_CENTER,
                                &display_text,
                                font_id.clone(),
                                outline_color,
                            );
                        }

                        // Draw main text
                        ui.painter().text(
                            egui::pos2(screen_pos.x, screen_pos.y),
                            egui::Align2::CENTER_CENTER,
                            &display_text,
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
            commands.entity(bubble_entity).despawn_recursive();
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
            commands.entity(entity).despawn_recursive();
            continue;
        };

        // Increment progress
        particle.progress += particle.speed * dt;

        // Despawn when reached caster
        if particle.progress >= 1.0 {
            commands.entity(entity).despawn_recursive();
            continue;
        }

        // Get caster and target positions
        let Ok(caster_transform) = positions.get(beam.caster) else {
            commands.entity(entity).despawn_recursive();
            continue;
        };
        let Ok(target_transform) = positions.get(beam.target) else {
            commands.entity(entity).despawn_recursive();
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
                    commands.entity(particle_entity).despawn_recursive();
                }
            }

            // Despawn the beam itself
            commands.entity(beam_entity).despawn_recursive();
        }
    }
}

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
            commands.entity(entity).despawn_recursive();
        }
    }
}

// ==============================================================================
// Dispel Burst Visual Effects
// ==============================================================================

/// Returns (base_color, emissive) for dispel burst based on caster class.
fn dispel_burst_colors(class: CharacterClass) -> (Color, LinearRgba) {
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
            commands.entity(entity).despawn_recursive();
        }
    }
}

// ==============================================================================
// Pet Mesh Tilt (Quadruped Orientation)
// ==============================================================================

/// Reconstructs pet rotation as Y-facing * X-tilt so the capsule mesh lies
/// horizontal like a four-legged creature. Uses Euler decomposition to
/// extract the Y-facing angle regardless of whether the tilt is already
/// baked into the current rotation or the movement system just set a fresh
/// Y-only rotation this frame.
pub fn apply_pet_mesh_tilt(
    mut pets: Query<&mut Transform, With<Pet>>,
) {
    let tilt = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    for mut transform in pets.iter_mut() {
        // Euler YXZ decomposition correctly separates the Y-facing angle
        // from the X-tilt, whether the rotation is Y-only or Y*X_tilt.
        let (y_angle, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        transform.rotation = Quat::from_rotation_y(y_angle) * tilt;
    }
}
