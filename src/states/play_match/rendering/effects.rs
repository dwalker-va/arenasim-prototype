//! Visual Effects Systems
//!
//! Floating combat text, spell impact effects, speech bubbles, shield bubbles, and dispel bursts.

use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_egui::{egui, EguiContexts};
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::play_match::banter::vocab;
use crate::states::play_match::components::*;
use crate::states::play_match::map_config::ActiveMapGeometry;
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

    let Ok((camera, camera_transform)) = camera_query.single() else {
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
            commands.entity(entity).despawn();
        }
    }
}

// ==============================================================================
// Speech Bubble Systems
// ==============================================================================

/// Whether a speech bubble should be drawn at all, given the gate state.
///
/// Ability bubbles are suppressed while the gates are closed: the countdown is
/// the buff rotation, and a Mage yelling "Frost Armor!" at an empty starting
/// room steps on the banter that plays there. Banter always renders. The filter
/// lives here rather than at the ~12 `spawn_speech_bubble` call sites because
/// this renderer is graphical-only — no sim file is touched, so headless stays
/// byte-identical by construction.
pub fn bubble_visible(kind: BubbleKind, gates_opened: bool) -> bool {
    match kind {
        BubbleKind::Ability => gates_opened,
        BubbleKind::Banter => true,
    }
}

/// Render speech bubbles above combatants' heads
pub fn render_speech_bubbles(
    mut contexts: EguiContexts,
    speech_bubbles: Query<&SpeechBubble>,
    combatants: Query<&Transform, With<Combatant>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    countdown: Res<MatchCountdown>,
    class_icons: Res<crate::states::configure_match_ui::ClassIcons>,
    spell_icons: Res<SpellIcons>,
    emoji_icons: Res<super::emoji::EmojiIcons>,
) {
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    for bubble in speech_bubbles.iter() {
        // Ability bubbles stay silent until the gates open (banter always shows)
        if !bubble_visible(bubble.kind, countdown.gates_opened) {
            continue;
        }

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

        // A line is a sequence of glyph runs and icons, so measure the pieces
        // and lay them out on one row rather than measuring a single string.
        let font_id = egui::FontId::proportional(BUBBLE_TEXT_SIZE);
        let spans = vocab::parse(&bubble.text);
        let measured: Vec<(vocab::Span, f32)> = spans
            .into_iter()
            .map(|span| {
                let width = match &span {
                    vocab::Span::Text(text) => ctx
                        .fonts(|f| {
                            f.layout_no_wrap(text.clone(), font_id.clone(), egui::Color32::BLACK)
                        })
                        .size()
                        .x,
                    // Icons are square and sized to the line's cap height so
                    // they sit on the same visual baseline as the glyphs.
                    _ => BUBBLE_ICON,
                };
                (span, width)
            })
            .collect();
        let content_w: f32 = measured.iter().map(|(_, w)| *w).sum();
        let content_h = BUBBLE_ICON.max(BUBBLE_TEXT_SIZE);

        // Tight padding around content
        let padding = egui::vec2(12.0, 6.0);
        let bubble_size = egui::vec2(content_w, content_h) + padding * 2.0;
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
            egui::CornerRadius::same(6),
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240),
        );

        // Black border
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(6),
            egui::Stroke::new(2.0, egui::Color32::BLACK),
            egui::StrokeKind::Outside,
        );

        // Lay the spans out left to right, vertically centred.
        let mut x = rect.min.x + padding.x;
        let mid_y = rect.center().y;
        for (span, width) in &measured {
            match span {
                vocab::Span::Text(text) => {
                    painter.text(
                        egui::pos2(x, mid_y),
                        egui::Align2::LEFT_CENTER,
                        text,
                        font_id.clone(),
                        egui::Color32::BLACK,
                    );
                }
                vocab::Span::Class(class, team) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    // Team ownership is carried by a BORDER, not a tint.
                    //
                    // Tinting was tried first and multiplies the portrait's own
                    // colours, which muddies the silhouette exactly when the
                    // reader needs to identify a class at a glance in a bubble
                    // that is on screen for a few seconds. A frame around the
                    // art says the same thing and costs the art nothing.
                    match class_icons.textures.get(class) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, egui::Color32::WHITE);
                        }
                        // No texture: a dark plate carrying the class initial,
                        // so the line still reads if art is missing.
                        None => {
                            painter.rect_filled(icon_rect, 3.0, egui::Color32::from_gray(40));
                            painter.text(
                                icon_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &class.name()[0..1],
                                egui::FontId::proportional(13.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                    // Inside, so the frame overlays the art's edge rather than
                    // bleeding into the glyph beside it.
                    painter.rect_stroke(
                        icon_rect,
                        3.0,
                        egui::Stroke::new(2.0, team_tint(*team)),
                        egui::StrokeKind::Inside,
                    );
                }
                vocab::Span::Ability(name) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    match spell_icons.textures.get(name) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, egui::Color32::WHITE);
                        }
                        // A named ability with no loaded icon is a content
                        // mistake; show a placeholder rather than a gap so it
                        // is noticed.
                        None => {
                            painter.rect_stroke(
                                icon_rect,
                                3.0,
                                egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
                vocab::Span::Emoji(name) => {
                    let icon_rect = icon_rect_at(x, mid_y);
                    match emoji_icons.textures.get(name) {
                        Some(texture) => {
                            painter.image(*texture, icon_rect, UV_FULL, egui::Color32::WHITE);
                        }
                        // Named art that is not on disk. A dashed-looking
                        // outline rather than nothing, so a typo'd name is
                        // visible in the bubble instead of a silent gap.
                        None => {
                            painter.rect_stroke(
                                icon_rect,
                                3.0,
                                egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
                vocab::Span::Unknown => {
                    painter.rect_stroke(
                        icon_rect_at(x, mid_y),
                        3.0,
                        egui::Stroke::new(1.0, egui::Color32::RED),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            x += width;
        }
    }
}

/// Glyph size inside a speech bubble.
const BUBBLE_TEXT_SIZE: f32 = 18.0;
/// Icon edge length inside a speech bubble, matched to the glyph size so
/// portraits and symbols sit on one visual line.
const BUBBLE_ICON: f32 = 20.0;
const UV_FULL: egui::Rect =
    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// A square icon slot at `x`, vertically centred on `mid_y`.
fn icon_rect_at(x: f32, mid_y: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(x, mid_y - BUBBLE_ICON / 2.0),
        egui::vec2(BUBBLE_ICON, BUBBLE_ICON),
    )
}

/// The colour of the frame around a team's class portraits.
///
/// Same blue/red the combat-log timeline already uses for team headers, so a
/// portrait in a bubble reads as the same team a reader has seen elsewhere.
fn team_tint(team: u8) -> egui::Color32 {
    if team == 1 {
        egui::Color32::from_rgb(100, 150, 255)
    } else {
        egui::Color32::from_rgb(255, 100, 100)
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
            commands.entity(entity).despawn();
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

// ==============================================================================
// Polymorph Visual Effect System
// ==============================================================================

/// Half-extents of the sheep torso, in the [`VisualBody`]'s local space. These
/// reproduce the footprint of the cuboid placeholder the sheep replaced
/// (0.8 x 0.6 x 1.0), so the transformed unit occupies the same volume it did.
const SHEEP_TORSO_HALF: Vec3 = Vec3::new(0.40, 0.30, 0.50);
/// Leg length; also the torso's clearance above the floor.
const SHEEP_LEG_LEN: f32 = 0.30;
const SHEEP_HEAD_RADIUS: f32 = 0.22;
/// Wool: off-white and fully rough, so the sheep reads as fleece rather than as
/// a lit surface next to the glossy class capsules.
const SHEEP_WOOL_COLOR: Color = Color::srgb(0.93, 0.91, 0.85);
/// Face, ears and legs — the bare-skin parts.
const SHEEP_SKIN_COLOR: Color = Color::srgb(0.30, 0.26, 0.26);

/// System that swaps combatant meshes when polymorphed.
///
/// The victim's body becomes a sheep: the [`VisualBody`]'s own mesh is swapped
/// to the wool torso, and the head, ears, legs and tail ride along as
/// [`SheepPart`] children of it (so they inherit the walk bob and despawn with
/// the unit for free). Weapon sockets are hidden separately, by
/// `animate_weapon_swings`.
pub fn update_polymorph_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // The aura state lives on the sim entity; the mesh lives on its `VisualBody`
    // child, so this joins across the hierarchy. `ActiveAuras` is OPTIONAL
    // because `update_auras` removes the component outright once the last aura
    // expires — required for the component, not the vec, to signal the end.
    combatants: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&PolymorphedVisual>,
        &Children,
    )>,
    mut bodies: Query<(
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &OriginalMesh,
        &VisualBody,
    )>,
    parts: Query<(Entity, &SheepPart)>,
) {
    for (entity, combatant, transform, auras, polymorphed_marker, children) in combatants.iter() {
        // A killing blow leaves the aura ON the corpse — `update_auras` skips
        // dead combatants entirely — so death has to count as an exit path here
        // or the loser stays a sheep for the rest of the match.
        let is_polymorphed = combatant.is_alive()
            && auras.is_some_and(|a| {
                a.auras.iter().any(|au| au.effect_type == AuraType::Polymorph)
            });

        if is_polymorphed && polymorphed_marker.is_none() {
            // Just transformed.
            let wool = materials.add(StandardMaterial {
                base_color: SHEEP_WOOL_COLOR,
                perceptual_roughness: 1.0,
                ..default()
            });
            let skin = materials.add(StandardMaterial {
                base_color: SHEEP_SKIN_COLOR,
                perceptual_roughness: 0.9,
                ..default()
            });
            let mut displaced_material = None;
            for child in children.iter() {
                let Ok((mut mesh3d, mut material, _, body)) = bodies.get_mut(child) else {
                    continue;
                };
                // Floor height in the body's local space. Derived rather than
                // hardcoded because pets render their body at an offset from
                // the sim entity's `y` (see `VisualBody::rest_y`).
                let ground_y = -(transform.translation.y + body.rest_y);
                let torso_y = ground_y + SHEEP_LEG_LEN + SHEEP_TORSO_HALF.y;

                // The torso is the body's OWN mesh, whose transform belongs to
                // the walk bob and the death sink, so its offset and squash are
                // baked into the mesh instead of applied as a scale.
                *mesh3d = Mesh3d(meshes.add(
                    Sphere::new(1.0)
                        .mesh()
                        .uv(24, 12)
                        .scaled_by(SHEEP_TORSO_HALF)
                        .translated_by(Vec3::Y * torso_y),
                ));
                displaced_material = Some(material.0.clone());
                *material = MeshMaterial3d(wool.clone());

                spawn_sheep_parts(
                    &mut commands,
                    &mut meshes,
                    &wool,
                    &skin,
                    child,
                    entity,
                    ground_y,
                    torso_y,
                );
            }
            // Without a body child there is nothing to restore; leaving the
            // marker off retries next frame rather than stranding the unit.
            if let Some(original_material) = displaced_material {
                commands
                    .entity(entity)
                    .try_insert(PolymorphedVisual { original_material });
            }
        } else if let (false, Some(marker)) = (is_polymorphed, polymorphed_marker) {
            // Just restored — by expiry, damage break, dispel or death.
            for child in children.iter() {
                if let Ok((mut mesh3d, mut material, original_mesh, _)) = bodies.get_mut(child) {
                    *mesh3d = Mesh3d(original_mesh.0.clone());
                    *material = MeshMaterial3d(marker.original_material.clone());
                }
            }
            // Owner-scoped: a global sweep would strip a second sheep that is
            // still polymorphed.
            for (part_entity, part) in parts.iter() {
                if part.owner == entity {
                    commands.entity(part_entity).despawn();
                }
            }
            commands.entity(entity).remove::<PolymorphedVisual>();
        }
    }
}

/// Spawn the sheep's non-torso primitives as children of `body`, tagged for
/// `owner` so restore despawns exactly this unit's set.
///
/// `ground_y` and `torso_y` are local heights in the body's space: the floor and
/// the torso's centre.
#[allow(clippy::too_many_arguments)]
fn spawn_sheep_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    wool: &Handle<StandardMaterial>,
    skin: &Handle<StandardMaterial>,
    body: Entity,
    owner: Entity,
    ground_y: f32,
    torso_y: f32,
) {
    // One unit sphere, posed per part by the child transforms.
    let ball = meshes.add(Sphere::new(1.0).mesh().uv(16, 10));
    let leg = meshes.add(Cylinder::new(0.06, SHEEP_LEG_LEN));

    let head_y = torso_y + 0.14;
    let head_z = SHEEP_TORSO_HALF.z * 0.85;
    let mut part = |mesh: Handle<Mesh>, material: Handle<StandardMaterial>, transform: Transform| {
        let child = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                SheepPart { owner },
            ))
            .id();
        commands.entity(body).add_child(child);
    };

    // Head, with a darker muzzle poking out of the fleece.
    part(
        ball.clone(),
        wool.clone(),
        Transform::from_xyz(0.0, head_y, head_z).with_scale(Vec3::splat(SHEEP_HEAD_RADIUS)),
    );
    part(
        ball.clone(),
        skin.clone(),
        Transform::from_xyz(0.0, head_y - 0.06, head_z + 0.16)
            .with_scale(Vec3::new(0.11, 0.09, 0.13)),
    );
    // Ears: flat lozenges angled up and out, one per side.
    for side in [-1.0f32, 1.0] {
        part(
            ball.clone(),
            skin.clone(),
            Transform::from_xyz(side * 0.19, head_y + 0.08, head_z - 0.06)
                .with_rotation(Quat::from_rotation_z(side * 0.5))
                .with_scale(Vec3::new(0.14, 0.04, 0.08)),
        );
    }
    // Legs at the corners of the torso footprint; the cylinder's origin is its
    // middle, so it stands on `ground_y` at half a leg up.
    for (x, z) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        part(
            leg.clone(),
            skin.clone(),
            Transform::from_xyz(
                x * SHEEP_TORSO_HALF.x * 0.6,
                ground_y + SHEEP_LEG_LEN * 0.5,
                z * SHEEP_TORSO_HALF.z * 0.6,
            ),
        );
    }
    // Tail.
    part(
        ball,
        wool.clone(),
        Transform::from_xyz(0.0, torso_y + 0.12, -SHEEP_TORSO_HALF.z - 0.02)
            .with_scale(Vec3::new(0.09, 0.11, 0.09)),
    );
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
        CharacterClass::Hunter => (
            // Hunter gold (for Concussive Shot impact and Master's Call)
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

// ==============================================================================
// Dispel Ribbon Visual Effects
// ==============================================================================
// A twisting ribbon that spirals up off the dispelled combatant's head — the
// distinct "you got cleansed" indicator (see DispelRibbon). Graphical only;
// registered in states/mod.rs, never in headless systems.rs.

/// Number of turns the ribbon helix coils through over its baked length.
const RIBBON_TURNS: f32 = 2.5;
/// Baked vertical span (yards) of the helix geometry itself.
const RIBBON_HEIGHT: f32 = 1.4;
/// Ribbon band width (yards). Thin so it reads as a defined ribbon, not a blob.
const RIBBON_WIDTH: f32 = 0.22;
/// Horizontal coil radius (yards). Must be > 0 or the ribbon degenerates to a
/// twisted vertical column instead of a laterally-spiraling ribbon.
const RIBBON_RADIUS: f32 = 0.35;
/// Number of strip segments along the helix (mesh resolution).
const RIBBON_SEGMENTS: usize = 48;
/// Anchor height (yards) above the target's origin — the head, taller than the
/// sphere burst's chest-height `Vec3::Y * 1.0`.
const RIBBON_HEAD_OFFSET: f32 = 1.9;
/// Additional upward travel (yards) accrued over the ribbon's lifetime, so it
/// visibly lifts off the head (the motion is a primary distinctiveness lever).
const RIBBON_RISE_DISTANCE: f32 = 0.5;
/// Spin rate (radians/sec) of the ribbon's slow Y-axis rotation.
const RIBBON_SPIN_RATE: f32 = 3.0;

/// Color for the dispel ribbon: the same class hue as the burst, but near-opaque
/// with a moderated emissive so it reads as a *solid* ribbon (a colored surface
/// with a sheen) rather than the wispy additive glow of the sphere bursts. Kept
/// separate from `dispel_burst_colors` so the burst (Concussive Shot / Master's
/// Call) is unaffected.
fn dispel_ribbon_colors(class: CharacterClass) -> (Color, LinearRgba) {
    let (base, emissive) = dispel_burst_colors(class);
    (
        // Near-opaque surface so AlphaMode::Blend gives it physical presence.
        base.with_alpha(0.95),
        // Trim the emissive so it's a colored sheen + light bloom, not a pure glow.
        LinearRgba::new(emissive.red * 0.6, emissive.green * 0.6, emissive.blue * 0.6, 1.0),
    )
}

/// Build the twisting-ribbon mesh: a flat strip of quads whose centerline follows
/// a helix coiling upward. Modeled on `create_arena_floor_mesh` (the codebase's raw-vertex
/// mesh precedent). `radius` must be > 0 so the band coils laterally rather than
/// twisting in place. Returns a `TriangleList` with POSITION / NORMAL / UV_0 and
/// U32 indices; render it double-sided (`cull_mode: None`) since the band is thin.
fn build_dispel_ribbon_mesh(
    turns: f32,
    height: f32,
    width: f32,
    radius: f32,
    segments: usize,
) -> Mesh {
    use std::f32::consts::TAU;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(2 * (segments + 1));

    for i in 0..=segments {
        let t = i as f32 / segments as f32; // 0..1 along the strip
        let angle = t * turns * TAU;
        let y = t * height;
        let (sin_a, cos_a) = angle.sin_cos();

        // Centerline orbits the vertical axis at `radius`.
        let center = Vec3::new(cos_a * radius, y, sin_a * radius);
        // Width is offset along the horizontal radial direction, so the band
        // reads as a coiling ramp rather than a vertical wall.
        let radial = Vec3::new(cos_a, 0.0, sin_a);
        let half = radial * (width * 0.5);

        let left = center - half;
        let right = center + half;
        positions.push([left.x, left.y, left.z]);
        positions.push([right.x, right.y, right.z]);

        // Emissive + additive blending makes lighting negligible; an up-facing
        // normal is a fine, stable approximation for the ramp band.
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);

        uvs.push([0.0, t]);
        uvs.push([1.0, t]);
    }

    // Two triangles per segment over the 4 edge vertices of segments i and i+1.
    let mut indices: Vec<u32> = Vec::with_capacity(6 * segments);
    for i in 0..segments {
        let bl = (2 * i) as u32; // bottom-left
        let br = (2 * i + 1) as u32; // bottom-right
        let tl = (2 * (i + 1)) as u32; // top-left
        let tr = (2 * (i + 1) + 1) as u32; // top-right
        indices.extend_from_slice(&[bl, br, tr, bl, tr, tl]);
    }

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Spawn visual mesh for new dispel ribbons.
pub fn spawn_dispel_ribbon_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_ribbons: Query<(Entity, &DispelRibbon), (Added<DispelRibbon>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (ribbon_entity, ribbon) in new_ribbons.iter() {
        let Ok(target_transform) = transforms.get(ribbon.target) else {
            continue;
        };

        let (base_color, emissive) = dispel_ribbon_colors(ribbon.caster_class);

        let mesh = meshes.add(build_dispel_ribbon_mesh(
            RIBBON_TURNS,
            RIBBON_HEIGHT,
            RIBBON_WIDTH,
            RIBBON_RADIUS,
            RIBBON_SEGMENTS,
        ));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            // Blend (not Add) so the ribbon reads as a solid surface rather than a
            // glow. The helix coils are vertically separated, so the thin band
            // doesn't self-overlap coplanarly — Z-fighting risk is low.
            alpha_mode: AlphaMode::Blend,
            // Thin ribbon: render both faces so it never vanishes from behind.
            cull_mode: None,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * RIBBON_HEAD_OFFSET;

        commands.entity(ribbon_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update dispel ribbons: follow the target, rise off the head, spin, and fade.
pub fn update_dispel_ribbons(
    time: Res<Time>,
    mut ribbons: Query<(&mut DispelRibbon, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<DispelRibbon>>,
) {
    for (mut ribbon, mut ribbon_transform, material_handle) in ribbons.iter_mut() {
        ribbon.lifetime -= time.delta_secs();
        ribbon.spin += time.delta_secs() * RIBBON_SPIN_RATE;

        // Progress: 1.0 (just spawned) → 0.0 (expired)
        let progress = (ribbon.lifetime / ribbon.initial_lifetime).max(0.0);

        // Follow the target's head, plus a rise that grows as the ribbon ages so
        // it visibly lifts off the head over its lifetime. If the target is gone
        // (died mid-ribbon), freeze at the last anchored position and keep fading
        // — matches DispelBurst / HealingLightColumn.
        if let Ok(target_transform) = transforms.get(ribbon.target) {
            let rise = (1.0 - progress) * RIBBON_RISE_DISTANCE;
            ribbon_transform.translation =
                target_transform.translation + Vec3::Y * (RIBBON_HEAD_OFFSET + rise);
        }
        ribbon_transform.rotation = Quat::from_rotation_y(ribbon.spin);

        // Fade out: scale alpha + emissive by progress (recompute canonical color).
        let (base_color, emissive) = dispel_ribbon_colors(ribbon.caster_class);
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

/// Cleanup expired dispel ribbons.
pub fn cleanup_expired_dispel_ribbons(
    mut commands: Commands,
    ribbons: Query<(Entity, &DispelRibbon)>,
) {
    for (entity, ribbon) in ribbons.iter() {
        if ribbon.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ==============================================================================
// Windfury Tornado Visual (spawned on a WindfuryTornado entity via Added<>)
// ==============================================================================

const WINDFURY_TURNS: f32 = 4.5;
const WINDFURY_HEIGHT: f32 = 3.0;
const WINDFURY_WIDTH: f32 = 0.35;
// Wide enough at the base to ENCIRCLE the ally (capsule radius ≈ 0.5), flaring
// wider toward the top — the character stands inside the vortex.
const WINDFURY_BOTTOM_RADIUS: f32 = 0.9;
const WINDFURY_TOP_RADIUS: f32 = 1.7;
const WINDFURY_SEGMENTS: usize = 96;
const WINDFURY_SPIN_RATE: f32 = 14.0; // fast — a tornado, not a gentle coil
/// Drop the funnel base to the ally's actual feet. Combatants sit at y≈1.0
/// (capsule center) with the capsule bottom ≈1.25 below that, so a 1.25 offset
/// puts the base right at the feet and the funnel rises up around the body.
const WINDFURY_FEET_OFFSET: f32 = 1.25;
// Storm-grey dust funnel, NOT white — `Blend` (not `Add`) so it can actually
// read dark, keeping it distinct from the bright additive shield bubbles
// (Power Word: Shield / Ice Barrier). Like the dispel ribbon, the helix coils
// are vertically separated, so coplanar Z-fighting risk is low.
const WINDFURY_RGB: (f32, f32, f32) = (0.34, 0.36, 0.40);
const WINDFURY_ALPHA: f32 = 0.62;
const WINDFURY_EMISSIVE: (f32, f32, f32) = (0.10, 0.12, 0.16); // faint, cool — not a glow

/// A funnel-shaped helix: identical to the dispel ribbon but the orbit radius
/// widens with height (narrow at the feet, broad at the top) so the band reads
/// as a swirling tornado rather than a uniform coil.
fn build_tornado_mesh(
    turns: f32,
    height: f32,
    width: f32,
    bottom_radius: f32,
    top_radius: f32,
    segments: usize,
) -> Mesh {
    use std::f32::consts::TAU;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(2 * (segments + 1));

    for i in 0..=segments {
        let t = i as f32 / segments as f32; // 0 (feet) .. 1 (top)
        let angle = t * turns * TAU;
        let y = t * height;
        let radius = bottom_radius + (top_radius - bottom_radius) * t; // funnel
        let (sin_a, cos_a) = angle.sin_cos();

        let center = Vec3::new(cos_a * radius, y, sin_a * radius);
        let radial = Vec3::new(cos_a, 0.0, sin_a);
        // Band widens a touch toward the top, like a flaring vortex.
        let half = radial * (width * (0.6 + 0.4 * t) * 0.5);

        let left = center - half;
        let right = center + half;
        positions.push([left.x, left.y, left.z]);
        positions.push([right.x, right.y, right.z]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([0.0, t]);
        uvs.push([1.0, t]);
    }

    let mut indices: Vec<u32> = Vec::with_capacity(6 * segments);
    for i in 0..segments {
        let bl = (2 * i) as u32;
        let br = (2 * i + 1) as u32;
        let tl = (2 * (i + 1)) as u32;
        let tr = (2 * (i + 1) + 1) as u32;
        indices.extend_from_slice(&[bl, br, tr, bl, tr, tl]);
    }

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Attach the funnel mesh when a `WindfuryTornado` marker is spawned (at the
/// proc site, in core). Graphical-only — registered solely in `states/mod.rs`.
pub fn spawn_windfury_tornado_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_tornados: Query<(Entity, &WindfuryTornado), (Added<WindfuryTornado>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (entity, tornado) in new_tornados.iter() {
        let Ok(target_transform) = transforms.get(tornado.target) else {
            // Target despawned in the same frame — drop the orphan effect.
            commands.entity(entity).despawn();
            continue;
        };

        let mesh = meshes.add(build_tornado_mesh(
            WINDFURY_TURNS,
            WINDFURY_HEIGHT,
            WINDFURY_WIDTH,
            WINDFURY_BOTTOM_RADIUS,
            WINDFURY_TOP_RADIUS,
            WINDFURY_SEGMENTS,
        ));
        // Dark storm-grey dust funnel — blended (not additive) so it stays dark.
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(WINDFURY_RGB.0, WINDFURY_RGB.1, WINDFURY_RGB.2, WINDFURY_ALPHA),
            emissive: LinearRgba::new(WINDFURY_EMISSIVE.0, WINDFURY_EMISSIVE.1, WINDFURY_EMISSIVE.2, 1.0),
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });

        let position = target_transform.translation - Vec3::Y * WINDFURY_FEET_OFFSET;
        commands.entity(entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Spin the funnel fast, follow the ally's feet, and fade out over its lifetime.
pub fn update_windfury_tornados(
    time: Res<Time>,
    mut tornados: Query<(&mut WindfuryTornado, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<WindfuryTornado>>,
) {
    for (mut tornado, mut tornado_transform, material_handle) in tornados.iter_mut() {
        tornado.lifetime -= time.delta_secs();
        tornado.spin += time.delta_secs() * WINDFURY_SPIN_RATE;

        let progress = (tornado.lifetime / tornado.initial_lifetime).max(0.0);

        // Follow the ally (freeze in place if they died mid-effect, like the
        // dispel ribbon / healing column).
        if let Ok(target_transform) = transforms.get(tornado.target) {
            tornado_transform.translation =
                target_transform.translation - Vec3::Y * WINDFURY_FEET_OFFSET;
        }
        tornado_transform.rotation = Quat::from_rotation_y(tornado.spin);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color =
                Color::srgba(WINDFURY_RGB.0, WINDFURY_RGB.1, WINDFURY_RGB.2, WINDFURY_ALPHA * progress);
            material.emissive = LinearRgba::new(
                WINDFURY_EMISSIVE.0 * progress,
                WINDFURY_EMISSIVE.1 * progress,
                WINDFURY_EMISSIVE.2 * progress,
                1.0,
            );
        }
    }
}

/// Despawn expired Windfury funnels.
pub fn cleanup_expired_windfury_tornados(
    mut commands: Commands,
    tornados: Query<(Entity, &WindfuryTornado)>,
) {
    for (entity, tornado) in tornados.iter() {
        if tornado.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod dispel_ribbon_mesh_tests {
    use super::*;
    use std::f32::consts::TAU;

    fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v.clone(),
            _ => panic!("expected Float32x3 positions"),
        }
    }

    #[test]
    fn vertex_and_index_counts_match_segments() {
        let segments = 32;
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, 0.35, 0.35, segments);
        assert_eq!(positions(&mesh).len(), 2 * (segments + 1));
        let index_count = match mesh.indices().unwrap() {
            Indices::U32(v) => v.len(),
            Indices::U16(v) => v.len(),
        };
        assert_eq!(index_count, 6 * segments);
    }

    #[test]
    fn vertices_span_the_full_rise() {
        let height = 1.4;
        let mesh = build_dispel_ribbon_mesh(2.5, height, 0.35, 0.35, 48);
        let ys: Vec<f32> = positions(&mesh).iter().map(|p| p[1]).collect();
        let min_y = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_y = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(min_y.abs() < 1e-4, "min Y should be ~0, got {min_y}");
        assert!((max_y - height).abs() < 1e-4, "max Y should be ~height, got {max_y}");
    }

    #[test]
    fn centerline_covers_requested_turns() {
        // First and last segment centerline angles should span turns * TAU.
        let turns = 2.5;
        let segments = 48;
        let mesh = build_dispel_ribbon_mesh(turns, 1.4, 0.35, 0.35, segments);
        let pos = positions(&mesh);
        // Centerline at each segment = midpoint of its two edge vertices, in XZ.
        let first = pos[0];
        let last = pos[pos.len() - 1];
        let first_angle = first[2].atan2(first[0]);
        // Sanity: the first centerline angle is near 0 (cos≈1).
        assert!(first_angle.abs() < 0.3, "first angle near 0, got {first_angle}");
        // The helix advances monotonically: the y of the last vertex >> first.
        assert!(last[1] > first[1]);
        // Number of full turns is encoded in height/turns geometry; assert the
        // angular range by walking centerline angle deltas.
        let mut total = 0.0_f32;
        let mut prev = first_angle;
        for i in 1..=segments {
            let v = pos[2 * i];
            let a = v[2].atan2(v[0]);
            let mut d = a - prev;
            while d > std::f32::consts::PI {
                d -= TAU;
            }
            while d < -std::f32::consts::PI {
                d += TAU;
            }
            total += d;
            prev = a;
        }
        assert!(
            (total.abs() - turns * TAU).abs() < 0.2,
            "unwrapped angular span {:.3} should be ~{:.3}",
            total.abs(),
            turns * TAU
        );
    }

    #[test]
    fn radius_produces_lateral_coil() {
        let radius = 0.35;
        let width = 0.35;
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, width, radius, 48);
        let max_horiz = positions(&mesh)
            .iter()
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(0.0_f32, f32::max);
        // A real lateral coil reaches out to ~radius + width/2, not ~0.
        assert!(
            (max_horiz - (radius + width * 0.5)).abs() < 0.05,
            "max horizontal extent {max_horiz} should be ~{}",
            radius + width * 0.5
        );
    }

    #[test]
    fn attributes_present_equal_length_and_indices_valid() {
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, 0.35, 0.35, 24);
        let vcount = positions(&mesh).len();
        for attr in [Mesh::ATTRIBUTE_NORMAL, Mesh::ATTRIBUTE_UV_0] {
            let len = match mesh.attribute(attr).unwrap() {
                bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v.len(),
                bevy::render::mesh::VertexAttributeValues::Float32x2(v) => v.len(),
                _ => panic!("unexpected attribute kind"),
            };
            assert_eq!(len, vcount, "attribute length should equal vertex count");
        }
        match mesh.indices().unwrap() {
            Indices::U32(v) => assert!(v.iter().all(|&i| (i as usize) < vcount)),
            Indices::U16(v) => assert!(v.iter().all(|&i| (i as usize) < vcount)),
        }
    }
}

// ==============================================================================
// Psychic Scream Burst (self-centered AoE fear)
// ==============================================================================

/// Shadow-violet color for the Psychic Scream burst (base, emissive).
fn scream_burst_colors() -> (Color, LinearRgba) {
    (
        Color::srgba(0.45, 0.12, 0.6, 0.55),
        LinearRgba::new(0.7, 0.2, 0.9, 1.0),
    )
}

/// Spawn the visual mesh for new Psychic Scream bursts (graphical-only).
pub fn spawn_scream_burst(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &ScreamBurst), (Added<ScreamBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let Ok(caster_transform) = transforms.get(burst.caster) else {
            continue;
        };

        let (base_color, emissive) = scream_burst_colors();

        let mesh = meshes.add(Sphere::new(1.0));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = caster_transform.translation + Vec3::Y * 1.0;

        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update Psychic Scream bursts: expand outward toward the AoE radius and fade.
pub fn update_scream_bursts(
    time: Res<Time>,
    mut bursts: Query<(&mut ScreamBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<ScreamBurst>>,
) {
    for (mut burst, mut burst_transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= time.delta_secs();

        // Keep the burst centered on the caster.
        if let Ok(caster_transform) = transforms.get(burst.caster) {
            burst_transform.translation = caster_transform.translation + Vec3::Y * 1.0;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired).
        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);

        // Expand a ring out toward the ~8yd scream radius (1.0 → 8.0).
        let scale = 1.0 + (1.0 - progress) * 7.0;
        burst_transform.scale = Vec3::splat(scale);

        // Fade out as it expands.
        let (base_color, emissive) = scream_burst_colors();
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

/// Cleanup expired Psychic Scream bursts.
pub fn cleanup_expired_scream_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &ScreamBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ==============================================================================
// Berserker Rage Mask (TBC-style black angry mask + red glow at the head)
// ==============================================================================

/// Head height for the Berserker Rage mask/glow (above the model, below FCT).
const BERSERK_MASK_HEIGHT: f32 = 2.6;

/// Hot rage red-orange for the Berserker Rage glow (base, emissive). Emissive
/// pushed above 1.0 so the glow blooms behind the flat black mask.
fn berserk_glow_colors() -> (Color, LinearRgba) {
    (
        Color::srgba(1.0, 0.35, 0.1, 0.7),
        LinearRgba::new(4.0, 0.9, 0.2, 1.0),
    )
}

/// Spawn the visuals for new Berserker Rage masks (graphical-only): the flat
/// black glyph quad on the marker entity, plus a separate additive glow sphere.
///
/// The mask uses `AlphaMode::Mask` (alpha cutout), NOT the usual `Add` — a
/// flat BLACK shape is invisible under additive blending (black contributes
/// zero light), and a hard cutout has no blend-sorting, so it also avoids the
/// Z-fighting flicker that made `Add` the house default. The glow behind it is
/// standard additive.
pub fn spawn_berserk_mask_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    new_masks: Query<(Entity, &BerserkMask), (Added<BerserkMask>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (mask_entity, mask) in new_masks.iter() {
        let Ok(caster_transform) = transforms.get(mask.caster) else {
            continue;
        };
        let head = caster_transform.translation + Vec3::Y * BERSERK_MASK_HEIGHT;

        // The black glyph quad (billboarded each frame by the update system).
        let glyph: Handle<Image> = asset_server.load("textures/effects/berserk_mask.png");
        let mask_mesh = meshes.add(Rectangle::new(1.3, 1.3));
        let mask_material = materials.add(StandardMaterial {
            base_color: Color::WHITE, // texture supplies the flat black + alpha
            base_color_texture: Some(glyph),
            unlit: true,              // flat black regardless of arena lighting
            alpha_mode: AlphaMode::Mask(0.5),
            ..default()
        });
        commands.entity(mask_entity).try_insert((
            Mesh3d(mask_mesh),
            MeshMaterial3d(mask_material),
            Transform::from_translation(head).with_scale(Vec3::splat(0.3)),
        ));

        // The emissive red-orange glow behind it.
        let (base_color, emissive) = berserk_glow_colors();
        let glow_mesh = meshes.add(Sphere::new(0.55));
        let glow_material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        commands.spawn((
            BerserkGlow {
                caster: mask.caster,
                lifetime: mask.lifetime,
                initial_lifetime: mask.initial_lifetime,
            },
            Mesh3d(glow_mesh),
            MeshMaterial3d(glow_material),
            Transform::from_translation(head).with_scale(Vec3::splat(0.3)),
            PlayMatchEntity,
        ));
    }
}

/// Update Berserker Rage masks and glows: follow the caster's head, billboard
/// the mask toward the camera, pop-overshoot in, hold, then collapse/fade.
pub fn update_berserk_masks(
    time: Res<Time>,
    mut masks: Query<(&mut BerserkMask, &mut Transform), (Without<BerserkGlow>, Without<Camera3d>)>,
    mut glows: Query<
        (&mut BerserkGlow, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<BerserkMask>, Without<Camera3d>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, (Without<BerserkMask>, Without<BerserkGlow>, Without<Camera3d>)>,
    camera: Query<&Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next();

    for (mut mask, mut mask_transform) in masks.iter_mut() {
        mask.lifetime -= dt;
        let elapsed = mask.initial_lifetime - mask.lifetime;

        let mut head = mask_transform.translation;
        if let Ok(caster_transform) = transforms.get(mask.caster) {
            head = caster_transform.translation + Vec3::Y * BERSERK_MASK_HEIGHT;
        }

        if let Some(cam_transform) = cam {
            // Billboard: face the camera plane, and sit slightly in front of
            // the glow along the view axis so the black glyph always wins.
            mask_transform.rotation = cam_transform.rotation;
            let toward_cam = (cam_transform.translation - head).normalize_or_zero();
            mask_transform.translation = head + toward_cam * 0.35;
        } else {
            mask_transform.translation = head;
        }

        // Pop in with overshoot (0-0.15s), settle (0.15-0.3s), hold, then
        // collapse over the final 0.35s — scale-collapse instead of an alpha
        // fade because Mask-mode alpha fades pop out binarily at the cutoff.
        let scale = if elapsed < 0.15 {
            0.3 + (elapsed / 0.15) * (1.15 - 0.3)
        } else if elapsed < 0.3 {
            1.15 - ((elapsed - 0.15) / 0.15) * 0.15
        } else if mask.lifetime < 0.35 {
            (mask.lifetime / 0.35).max(0.0)
        } else {
            1.0
        };
        mask_transform.scale = Vec3::splat(scale);
    }

    for (mut glow, mut glow_transform, material_handle) in glows.iter_mut() {
        glow.lifetime -= dt;
        let elapsed = glow.initial_lifetime - glow.lifetime;
        let progress = (glow.lifetime / glow.initial_lifetime).max(0.0);

        if let Ok(caster_transform) = transforms.get(glow.caster) {
            glow_transform.translation = caster_transform.translation + Vec3::Y * BERSERK_MASK_HEIGHT;
        }

        // Quick flare-in, then an angry pulse while the mask holds.
        let grow = (elapsed / 0.15).min(1.0);
        let pulse = 1.0 + 0.12 * (elapsed * 18.0).sin();
        glow_transform.scale = Vec3::splat(grow * pulse);

        // Fade the emissive out over the effect's life.
        let (base_color, emissive) = berserk_glow_colors();
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

/// Cleanup expired Berserker Rage masks and glows.
pub fn cleanup_expired_berserk_masks(
    mut commands: Commands,
    masks: Query<(Entity, &BerserkMask)>,
    glows: Query<(Entity, &BerserkGlow)>,
) {
    for (entity, mask) in masks.iter() {
        if mask.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    for (entity, glow) in glows.iter() {
        if glow.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

// ==============================================================================
// Death Coil Burst (target-centered horror impact)
// ==============================================================================

/// Vivid skull-green for the Death Coil impact (base, emissive). Emissive is
/// pushed well above 1.0 so the flash blooms — Death Coil's whole problem is
/// being too subtle, so this errs bright.
fn death_coil_burst_colors() -> (Color, LinearRgba) {
    (
        Color::srgba(0.25, 0.95, 0.4, 0.7),
        LinearRgba::new(0.5, 2.6, 0.9, 1.0),
    )
}

/// Spawn the visual mesh for new Death Coil bursts (graphical-only).
pub fn spawn_death_coil_burst(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &DeathCoilBurst), (Added<DeathCoilBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let Ok(target_transform) = transforms.get(burst.target) else {
            continue;
        };

        let (base_color, emissive) = death_coil_burst_colors();

        let mesh = meshes.add(Sphere::new(0.6));
        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.2;

        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update Death Coil bursts: a hot flash that punches outward then fades.
pub fn update_death_coil_bursts(
    time: Res<Time>,
    mut bursts: Query<(&mut DeathCoilBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<DeathCoilBurst>>,
) {
    for (mut burst, mut burst_transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= time.delta_secs();

        // Stay pinned on the (possibly fleeing) target.
        if let Ok(target_transform) = transforms.get(burst.target) {
            burst_transform.translation = target_transform.translation + Vec3::Y * 1.2;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired).
        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);
        let grow = 1.0 - progress; // 0 → 1 over life

        // Punch from a tight bright core out to ~2.8yd.
        let scale = 0.5 + grow * 2.8;
        burst_transform.scale = Vec3::splat(scale);

        // Hold the flash bright early, then fade fast (progress^1.5 ≈ ease-out).
        let (base_color, emissive) = death_coil_burst_colors();
        let fade = progress * progress.sqrt();
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = base_color.with_alpha(base_color.alpha() * fade);
            material.emissive = LinearRgba::new(
                emissive.red * fade,
                emissive.green * fade,
                emissive.blue * fade,
                1.0,
            );
        }
    }
}

/// Cleanup expired Death Coil bursts.
pub fn cleanup_expired_death_coil_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &DeathCoilBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
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
    pets: Query<&Children, With<Pet>>,
    mut bodies: Query<&mut Transform, With<VisualBody>>,
) {
    let tilt = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    for children in pets.iter() {
        for child in children.iter() {
            let Ok(mut body_transform) = bodies.get_mut(child) else {
                continue;
            };
            // The parent already carries the Y-facing the sim wrote, and the
            // child's rotation composes on top of it, so the tilt is now a plain
            // local rotation instead of a decompose-and-reapply. Assigning it
            // unconditionally preserves the old behaviour of overriding a dying
            // pet's fall rotation on the next frame.
            body_transform.rotation = tilt;
        }
    }
}

// ==============================================================================
// Trap Visual Helpers
// ==============================================================================

/// Base RGB color for a trap type. Frost = cyan, Freezing = deep blue.
fn trap_type_rgb(trap_type: TrapType) -> (f32, f32, f32) {
    match trap_type {
        TrapType::Frost => (0.3, 0.8, 1.0),
        TrapType::Freezing => (0.3, 0.55, 1.0),
    }
}

/// Emissive glow for a trap type.
fn trap_type_emissive(trap_type: TrapType) -> LinearRgba {
    match trap_type {
        TrapType::Frost => LinearRgba::new(0.4, 1.2, 2.0, 1.0),
        TrapType::Freezing => LinearRgba::new(0.6, 1.2, 2.8, 1.0),
    }
}

// ==============================================================================
// Trap Ground Circle Visual (spawned on Trap entity via Added<Trap>)
// ==============================================================================

/// Spawn flat cylinder mesh on newly created traps to visualize their position.
/// Color depends on trap type: Frost = cyan, Freezing = ice-white.
pub fn spawn_trap_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_traps: Query<(Entity, &Trap), (Added<Trap>, Without<Mesh3d>)>,
) {
    for (trap_entity, trap) in new_traps.iter() {
        let mesh = meshes.add(Cylinder::new(2.0, 0.05));

        let (r, g, b) = trap_type_rgb(trap.trap_type);
        let base_color = Color::srgba(r, g, b, 0.15); // Dim while arming
        let emissive = trap_type_emissive(trap.trap_type);

        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(trap_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

/// Update trap ground circles: dim pulse while arming, bright shimmer when armed.
pub fn update_trap_visuals(
    time: Res<Time>,
    traps: Query<(&Trap, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();

    for (trap, material_handle) in traps.iter() {
        let Some(material) = materials.get_mut(&material_handle.0) else {
            continue;
        };

        let (r, g, b) = trap_type_rgb(trap.trap_type);
        let emissive_base = trap_type_emissive(trap.trap_type);

        if trap.arm_timer > 0.0 {
            // Arming: low alpha with slow sine pulse
            let pulse = 0.1 + 0.05 * (t * 2.0).sin();
            material.base_color = Color::srgba(r, g, b, pulse);
            // Dim emissive while arming
            material.emissive = LinearRgba::new(
                emissive_base.red * 0.3,
                emissive_base.green * 0.3,
                emissive_base.blue * 0.3,
                1.0,
            );
        } else {
            // Armed: full brightness with subtle shimmer
            let shimmer = 0.35 + 0.05 * (t * 4.0).sin();
            material.base_color = Color::srgba(r, g, b, shimmer);
            material.emissive = emissive_base;
        }
    }
}

// ==============================================================================
// Trap Burst Visual (expanding sphere on trigger)
// ==============================================================================

/// Spawn visual mesh for trap burst effects.
pub fn spawn_trap_burst_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &TrapBurst), (Added<TrapBurst>, Without<Mesh3d>)>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let mesh = meshes.add(Sphere::new(0.6));

        let (r, g, b) = trap_type_rgb(burst.trap_type);
        let base_color = Color::srgba(r, g, b, 0.6);
        // Burst uses brighter emissive than ground circle
        let emissive = match burst.trap_type {
            TrapType::Frost => LinearRgba::new(0.6, 1.5, 2.5, 1.0),
            TrapType::Freezing => LinearRgba::new(0.8, 1.5, 3.5, 1.0),
        };

        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

/// Update and cleanup trap bursts: expand scale and fade, despawn when expired.
pub fn update_and_cleanup_trap_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut bursts: Query<(Entity, &mut TrapBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (entity, mut burst, mut transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= dt;

        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired)
        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);

        // Scale up: 1.0 → 4.0
        let scale = 1.0 + (1.0 - progress) * 3.0;
        transform.scale = Vec3::splat(scale);

        // Fade out
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let alpha = 0.6 * progress;
            let (r, g, b) = trap_type_rgb(burst.trap_type);
            material.base_color = Color::srgba(r, g, b, alpha);
        }
    }
}

// ==============================================================================
// Trap Launch Arc Visual (in-flight sphere while trap travels to landing position)
// ==============================================================================

/// Spawn glowing sphere mesh on newly created trap launch projectiles.
pub fn spawn_trap_launch_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_projectiles: Query<(Entity, &TrapLaunchProjectile), (Added<TrapLaunchProjectile>, Without<Mesh3d>)>,
) {
    for (entity, proj) in new_projectiles.iter() {
        let mesh = meshes.add(Sphere::new(0.3));

        let (r, g, b) = trap_type_rgb(proj.trap_type);
        let emissive = trap_type_emissive(proj.trap_type);

        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(r, g, b, 0.8),
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

// ==============================================================================
// Ice Block Visual (Freezing Trap cuboid)
// ==============================================================================

/// Spawn translucent ice cuboid around Freezing Trap targets.
pub fn spawn_ice_block_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_blocks: Query<(Entity, &IceBlockVisual), (Added<IceBlockVisual>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (block_entity, block) in new_blocks.iter() {
        let Ok(target_transform) = transforms.get(block.target) else {
            continue;
        };

        let mesh = meshes.add(Cuboid::new(1.5, 2.3, 1.5));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.3, 0.6, 1.0, 0.45),
            emissive: LinearRgba::new(0.5, 1.0, 2.8, 1.0),
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        commands.entity(block_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(target_transform.translation),
        ));
    }
}

/// Update ice block positions to follow their frozen targets.
pub fn update_ice_blocks(
    mut ice_blocks: Query<(&IceBlockVisual, &mut Transform), Without<Combatant>>,
    combatants: Query<&Transform, With<Combatant>>,
) {
    for (block, mut block_transform) in ice_blocks.iter_mut() {
        if let Ok(target_transform) = combatants.get(block.target) {
            block_transform.translation = target_transform.translation;
        }
    }
}

/// Cleanup ice blocks when the Incapacitate aura breaks or target dies.
pub fn cleanup_ice_blocks(
    mut commands: Commands,
    time: Res<Time>,
    mut ice_blocks: Query<(Entity, &mut IceBlockVisual)>,
    combatants: Query<(&Combatant, Option<&ActiveAuras>)>,
) {
    let dt = time.delta_secs();
    for (block_entity, mut block) in ice_blocks.iter_mut() {
        // Grace period: skip cleanup check to let apply_pending_auras process the aura
        if block.grace_timer > 0.0 {
            block.grace_timer -= dt;
            continue;
        }
        let should_despawn = match combatants.get(block.target) {
            Ok((combatant, auras)) => {
                // Despawn if target died
                if !combatant.is_alive() {
                    true
                } else {
                    // Despawn if target no longer has Incapacitate aura
                    auras.map_or(true, |a| {
                        !a.auras.iter().any(|aura| aura.effect_type == AuraType::Incapacitate)
                    })
                }
            }
            Err(_) => true, // Target entity gone
        };

        if should_despawn {
            commands.entity(block_entity).despawn();
        }
    }
}

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

// ==============================================================================
// Disengage Trail Visual
// ==============================================================================

/// Spawn wind streak trail when a combatant starts Disengaging.
pub fn spawn_disengage_trail(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_disengages: Query<(Entity, &Transform, &DisengagingState), Added<DisengagingState>>,
) {
    for (_entity, transform, disengage) in new_disengages.iter() {
        // Elongated cylinder at the Hunter's start position
        let mesh = meshes.add(Cylinder::new(0.3, 3.0));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.85, 0.9, 1.0, 0.4),
            emissive: LinearRgba::new(1.5, 1.7, 2.0, 1.0),
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        // Orient cylinder along the disengage direction
        // Cylinder points up (Y axis), so rotate from Y to direction
        let direction = disengage.direction.normalize_or_zero();
        let rotation = if direction != Vec3::ZERO {
            Quat::from_rotation_arc(Vec3::Y, direction)
        } else {
            Quat::IDENTITY
        };

        let trail_pos = transform.translation + Vec3::Y * 0.5;

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(trail_pos).with_rotation(rotation),
            DisengageTrail {
                lifetime: 0.5,
                initial_lifetime: 0.5,
            },
            PlayMatchEntity,
        ));
    }
}

/// Update and cleanup disengage trails: fade alpha and despawn when expired.
pub fn update_and_cleanup_disengage_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut trails: Query<(Entity, &mut DisengageTrail, &MeshMaterial3d<StandardMaterial>)>,
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
            material.base_color = Color::srgba(0.85, 0.9, 1.0, 0.4 * progress);
            material.emissive = LinearRgba::new(
                1.5 * progress,
                1.7 * progress,
                2.0 * progress,
                1.0,
            );
        }
    }
}

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

// ==============================================================================
// Unstable Affliction DoT Glow
// ==============================================================================
//
// Spawn/update/cleanup three-system pattern. The glow is a deep-violet sphere
// that pulses at ~0.5 Hz around afflicted combatants. Distinct from Corruption
// (faster green tendrils) so stacked Corruption + UA reads independently.
//
// Per project memory: AlphaMode::Add, Res<Time>, try_insert, Without<T>.

/// Spawn the UA glow mesh when a `UnstableAfflictionGlow` component is added.
pub fn spawn_ua_glow_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_glows: Query<(Entity, &UnstableAfflictionGlow), (Added<UnstableAfflictionGlow>, Without<Mesh3d>)>,
    transforms: Query<&Transform, Without<UnstableAfflictionGlow>>,
) {
    for (glow_entity, glow) in new_glows.iter() {
        let Ok(target_transform) = transforms.get(glow.target) else {
            continue;
        };

        let mesh = meshes.add(Sphere::new(0.55));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.35, 0.05, 0.55, 0.30),
            emissive: LinearRgba::new(0.55, 0.10, 0.85, 1.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.0;
        commands.entity(glow_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update the UA glow: follow target, pulse opacity at ~0.5 Hz.
pub fn update_ua_glow(
    time: Res<Time>,
    mut glows: Query<(&mut UnstableAfflictionGlow, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<UnstableAfflictionGlow>>,
) {
    let dt = time.delta_secs();
    for (mut glow, mut glow_transform, material_handle) in glows.iter_mut() {
        glow.phase += dt;

        if let Ok(target_transform) = transforms.get(glow.target) {
            glow_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        // 0.5 Hz pulse — period 2.0s, oscillates between 0.20 and 0.55 alpha.
        let pulse = (glow.phase * std::f32::consts::TAU * 0.5).sin() * 0.5 + 0.5; // [0,1]
        let alpha = 0.20 + 0.35 * pulse;
        let intensity = 0.55 + 0.45 * pulse;

        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.35, 0.05, 0.55, alpha);
            material.emissive = LinearRgba::new(0.55 * intensity, 0.10 * intensity, 0.85 * intensity, 1.0);
        }
    }
}

/// Despawn the UA glow when its target loses the UA aura (or dies).
pub fn cleanup_ua_glow(
    mut commands: Commands,
    glows: Query<(Entity, &UnstableAfflictionGlow)>,
    targets: Query<&ActiveAuras>,
) {
    for (glow_entity, glow) in glows.iter() {
        let still_afflicted = targets
            .get(glow.target)
            .map(|auras| {
                auras.auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageOverTime
                        && a.ability_name == "Unstable Affliction"
                })
            })
            .unwrap_or(false);

        if !still_afflicted {
            commands.entity(glow_entity).despawn();
        }
    }
}

// ==============================================================================
// Backlash Burst (UA dispel impact on dispeller)
// ==============================================================================
//
// Distinct from DispelBurst: dark-violet shadow color, ~2x scale, snappier
// 0.3s lifetime. Fired on the dispeller the frame backlash damage lands.

/// Spawn the BacklashBurst mesh when the component is added.
pub fn spawn_backlash_burst_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &BacklashBurst), (Added<BacklashBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform, Without<BacklashBurst>>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let Ok(target_transform) = transforms.get(burst.target) else {
            continue;
        };

        let mesh = meshes.add(Sphere::new(0.6));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.20, 0.0, 0.35, 0.85),
            emissive: LinearRgba::new(1.6, 0.20, 2.0, 1.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
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

/// Update BacklashBurst: expand quickly (1x -> 2.5x) and fade in 0.3s.
pub fn update_backlash_bursts(
    time: Res<Time>,
    mut bursts: Query<(&mut BacklashBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<BacklashBurst>>,
) {
    let dt = time.delta_secs();
    for (mut burst, mut burst_transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= dt;

        if let Ok(target_transform) = transforms.get(burst.target) {
            burst_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);
        let scale = 1.0 + (1.0 - progress) * 1.5; // 1.0 -> 2.5
        burst_transform.scale = Vec3::splat(scale);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.20, 0.0, 0.35, 0.85 * progress);
            material.emissive = LinearRgba::new(
                1.6 * progress,
                0.20 * progress,
                2.0 * progress,
                1.0,
            );
        }
    }
}

/// Despawn BacklashBurst entities once their lifetime expires.
pub fn cleanup_expired_backlash_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &BacklashBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Detect targets that have an Unstable Affliction aura but no `UnstableAfflictionGlow`
/// visual yet, and spawn the glow. Cleanup is handled by `cleanup_ua_glow` once the
/// UA aura is no longer present.
pub fn spawn_ua_glow_for_afflicted(
    mut commands: Commands,
    afflicted: Query<(Entity, &ActiveAuras)>,
    existing_glows: Query<&UnstableAfflictionGlow>,
) {
    use std::collections::HashSet;
    let already_glowing: HashSet<Entity> = existing_glows.iter().map(|g| g.target).collect();

    for (entity, auras) in afflicted.iter() {
        let has_ua = auras.auras.iter().any(|a| {
            a.effect_type == AuraType::DamageOverTime
                && a.ability_name == "Unstable Affliction"
        });
        if has_ua && !already_glowing.contains(&entity) {
            commands.spawn((
                UnstableAfflictionGlow { target: entity, phase: 0.0 },
                PlayMatchEntity,
            ));
        }
    }
}

// ==============================================================================
// DoT Drip Indicators (poison / bleed)
// ==============================================================================
//
// Generic affliction indicator: a `DotDripEmitter` per (target, kind) spawns
// small falling drops — green for poisons, red for bleeds. Color is game
// language shared across abilities; new afflictions are one row in
// `drip_kind_for_aura`. Drips mirror the FlameParticle idiom (velocity +
// lifetime + shrink), emitters follow the detector/cleanup convention.

/// Map an aura to the affliction family it should drip as, or None for DoTs
/// with their own identity (UA glow) or no body visual (Corruption, CoA).
/// Keys on the exact RON `name:` string, same as the class-AI dedup checks.
fn drip_kind_for_aura(aura: &Aura) -> Option<DripKind> {
    if aura.effect_type != AuraType::DamageOverTime {
        return None;
    }
    match aura.ability_name.as_str() {
        "Serpent Sting" => Some(DripKind::Poison), // future rogue poisons join here
        "Rend" => Some(DripKind::Bleed),           // future Rupture/Garrote join here
        _ => None,
    }
}

/// Cheap deterministic jitter in [0,1) from a seed — visual-only, so it does
/// not touch the sim's seeded GameRng.
fn drip_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// Detect combatants carrying a mapped DoT and ensure one emitter per
/// (target, kind). Emitter removal is handled by `update_drip_emitters`
/// once the mapped aura is gone.
pub fn spawn_drip_emitters_for_afflicted(
    mut commands: Commands,
    afflicted: Query<(Entity, &ActiveAuras)>,
    existing: Query<&DotDripEmitter>,
) {
    use std::collections::HashSet;
    let already: HashSet<(Entity, DripKind)> =
        existing.iter().map(|e| (e.target, e.kind)).collect();

    for (entity, auras) in afflicted.iter() {
        let mut kinds: HashSet<DripKind> = HashSet::new();
        for aura in auras.auras.iter() {
            if let Some(kind) = drip_kind_for_aura(aura) {
                kinds.insert(kind);
            }
        }
        for kind in kinds {
            if !already.contains(&(entity, kind)) {
                commands.spawn((
                    DotDripEmitter {
                        target: entity,
                        kind,
                        spawn_accumulator: 0.0,
                        drips_spawned: 0,
                    },
                    PlayMatchEntity,
                ));
            }
        }
    }
}

/// Tick emitters: despawn when the mapped aura is gone, otherwise spawn a
/// drip every `DRIP_INTERVAL` at a jittered point around the target's torso.
pub fn update_drip_emitters(
    mut commands: Commands,
    time: Res<Time>,
    mut emitters: Query<(Entity, &mut DotDripEmitter)>,
    targets: Query<(&ActiveAuras, &Transform)>,
) {
    const DRIP_INTERVAL: f32 = 0.45;
    let dt = time.delta_secs();

    for (emitter_entity, mut emitter) in emitters.iter_mut() {
        let Ok((auras, target_transform)) = targets.get(emitter.target) else {
            commands.entity(emitter_entity).despawn();
            continue;
        };
        let still_afflicted = auras
            .auras
            .iter()
            .any(|a| drip_kind_for_aura(a) == Some(emitter.kind));
        if !still_afflicted {
            commands.entity(emitter_entity).despawn();
            continue;
        }

        emitter.spawn_accumulator += dt;
        while emitter.spawn_accumulator >= DRIP_INTERVAL {
            emitter.spawn_accumulator -= DRIP_INTERVAL;
            let seed = emitter.target.index().wrapping_add(emitter.drips_spawned.wrapping_mul(3));
            emitter.drips_spawned = emitter.drips_spawned.wrapping_add(1);

            // Jittered spawn point around the torso (body capsule radius 0.5).
            let angle = drip_jitter(seed) * std::f32::consts::TAU;
            let radius = 0.35 + 0.20 * drip_jitter(seed + 1);
            let height = 0.6 + 0.5 * drip_jitter(seed + 2);
            let offset = Vec3::new(angle.cos() * radius, height, angle.sin() * radius);

            commands.spawn((
                DotDrip {
                    kind: emitter.kind,
                    velocity: Vec3::new(
                        angle.cos() * 0.25,
                        -2.2,
                        angle.sin() * 0.25,
                    ),
                    lifetime: 0.7,
                    initial_lifetime: 0.7,
                },
                Transform::from_translation(target_transform.translation + offset),
                PlayMatchEntity,
            ));
        }
    }
}

/// Spawn visual meshes for newly created drips: small glowing drops,
/// green for poison, red for bleed.
pub fn spawn_drip_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_drips: Query<(Entity, &DotDrip), (Added<DotDrip>, Without<Mesh3d>)>,
) {
    for (drip_entity, drip) in new_drips.iter() {
        // Opaque, not additive: drops are liquid, not glow. Additive blending
        // can only add light, so it can never produce a deep saturated color;
        // opaque unlit gives true deep green/red and depth-sorts like any
        // solid geometry (the documented Z-fighting pitfall is Blend-specific).
        // A whisper of emissive keeps drops readable in arena shadow.
        let (base, emissive) = match drip.kind {
            DripKind::Poison => (
                Color::srgb(0.04, 0.45, 0.07),
                LinearRgba::new(0.02, 0.35, 0.04, 1.0),
            ),
            DripKind::Bleed => (
                Color::srgb(0.50, 0.03, 0.03),
                LinearRgba::new(0.40, 0.02, 0.02, 1.0),
            ),
        };

        let mesh = meshes.add(Sphere::new(0.13));
        let material = materials.add(StandardMaterial {
            base_color: base,
            emissive,
            unlit: true,
            ..default()
        });

        commands.entity(drip_entity).try_insert((Mesh3d(mesh), MeshMaterial3d(material)));
    }
}

/// Animate drips: fall along velocity, shrink over lifetime, despawn at zero.
pub fn update_drips(
    mut commands: Commands,
    time: Res<Time>,
    mut drips: Query<(Entity, &mut DotDrip, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (entity, mut drip, mut transform) in drips.iter_mut() {
        drip.lifetime -= dt;

        if drip.lifetime <= 0.0 || transform.translation.y <= 0.02 {
            commands.entity(entity).despawn();
            continue;
        }

        transform.translation += drip.velocity * dt;

        let life_ratio = (drip.lifetime / drip.initial_lifetime).max(0.1);
        transform.scale = Vec3::splat(life_ratio);
    }
}

// Silence visibility note: an earlier iteration spawned a dedicated "Silenced"
// floating combat text on apply, but that diverged from how Stun / Fear / Polymorph
// surface to viewers. Those CC types use the `[CC]` combat-log entry plus the
// HUD aura icon (rendered by `render_aura_icons`) and skip floating text entirely.
// Silence now follows the same pattern — the existing CC log line
// "[CC] Unstable Affliction on Team X (5.0s, DR: ...)" plus the aura icon over
// the silenced combatant covers the visibility need without bespoke FCT.

/// Peak height of the walking bob above `ground_y`, in arena units.
/// Capsule height is ~2.5, so 0.10 reads as a subtle walk rather than a hop.
const WALK_BOB_AMPLITUDE: f32 = 0.10;

/// Arena units of horizontal travel per full bob cycle.
/// At base movement speed this lands near a natural walking cadence.
const WALK_STEP_LENGTH: f32 = 1.5;

/// Per-frame horizontal travel below this counts as "not moving" — the unit
/// is held flat at `ground_y` instead of accumulating phase.
const WALK_IDLE_EPSILON: f32 = 0.001;

/// Maximum phase advance per frame. Caps the cadence during Charge so the bob
/// reads as a fast walk instead of strobing when the warrior covers a large
/// XZ delta in a single frame.
const WALK_MAX_PHASE_STEP: f32 = std::f32::consts::PI;

/// Drive the walking bob on combatant and pet capsules.
///
/// Reads each unit's post-movement XZ, advances phase by the horizontal
/// distance traveled this frame, and writes the bob to that unit's
/// [`VisualBody`] child as `local.y = rest_y + sin(phase) * amplitude`. Idle
/// units (and any unit whose `Combatant::is_alive()` returns true but whose XZ
/// delta is below `WALK_IDLE_EPSILON`) snap to `rest_y` so they stand still.
///
/// **The bob must never touch the parent's `Transform`.** Gameplay range checks
/// use `Vec3::distance`, so a ±0.10 bob on the sim entity perturbed real range
/// checks — that is why a seed stopped reproducing between the client and
/// headless. See [`VisualBody`].
///
/// `Without<DeathAnimation>` and `Without<Celebrating>` cede the Y axis to
/// `animate_death` (corpse sink) and `update_victory_celebration` (winner
/// bounce). All three now write the same child's local Y and run in the same
/// post-`CombatResolution` window, so excluding their drivers is still the
/// cleanest way to avoid the last-writer-wins race.
///
/// Graphical-mode only — registered in `StatesPlugin::build()`, never in
/// `add_core_combat_systems`. Visual-only; touches no gameplay state.
pub fn update_walk_animation(
    time: Res<Time>,
    mut movers: Query<
        (&Transform, &mut WalkAnim, &Combatant, &Children),
        (Without<DeathAnimation>, Without<Celebrating>, Without<VisualBody>),
    >,
    mut bodies: Query<(&mut Transform, &VisualBody)>,
) {
    for (transform, mut walk, combatant, children) in movers.iter_mut() {
        // Read the sim entity's XZ, but write only the child's local Y.
        let current_xz = transform.translation.xz();
        let distance = (current_xz - walk.previous_xz).length();
        walk.previous_xz = current_xz;

        // Idle is TIME-based, not frame-based: the sim moves units only on
        // FixedUpdate ticks, so at render rates above the tick rate every
        // other frame sees zero movement. Snapping to rest on those frames
        // strobed the bob (and every attached weapon) at frame rate.
        if distance >= WALK_IDLE_EPSILON {
            // Coming out of a real stop, restart the cycle at its zero
            // crossing so the first bobbing frame matches the rest height.
            if walk.idle_time > 0.1 {
                walk.phase = 0.0;
            }
            walk.idle_time = 0.0;
        } else {
            walk.idle_time += time.delta_secs();
        }
        let idle = !combatant.is_alive() || walk.idle_time > 0.1;
        if !idle {
            let step =
                (distance / WALK_STEP_LENGTH * std::f32::consts::TAU).min(WALK_MAX_PHASE_STEP);
            walk.phase = (walk.phase + step).rem_euclid(std::f32::consts::TAU);
        }

        // Settling into idle EASES down to rest instead of snapping — the
        // walk can stop at any bob height, and a one-frame drop reads as a
        // pop (more so with weapons riding the body).
        let settle_step = 0.6 * time.delta_secs();
        for child in children.iter() {
            let Ok((mut body_transform, body)) = bodies.get_mut(child) else {
                continue;
            };
            if idle {
                let err = body.rest_y - body_transform.translation.y;
                body_transform.translation.y += err.clamp(-settle_step, settle_step);
            } else {
                body_transform.translation.y =
                    body.rest_y + walk.phase.sin() * WALK_BOB_AMPLITUDE;
            }
        }
    }
}

// ==============================================================================
// Totem Visuals (Shaman, graphical-only)
// ==============================================================================

/// Build a flat ground disc of `radius` centered at `center` (world XZ), clipped
/// to `bounds` so it never spills past the arena walls. Vertices are in LOCAL
/// space (offsets from `center`) lying in the XZ plane at y=0, so the mesh can be
/// parented to an entity sitting at `center`. Reusable by any ground decal that
/// must stay inside the arena.
///
/// Clipping is a per-direction march against [`ArenaBounds::contains`], which is
/// shape-agnostic: this used to be eight hard-coded octagon half-planes, which
/// silently collapsed the disc to zero radius on Nagrand's bowl (any totem outside
/// the retired 76×46 rectangle failed every plane test at once). The disc now
/// stops at the walkable edge rather than exactly at the wall — a `WALL_OFFSET`
/// (1.5yd) inset, and the only bound that holds for every shape.
fn arena_clipped_disc_mesh(bounds: &ArenaBounds, center: Vec2, radius: f32) -> Mesh {
    const SEGMENTS: usize = 96;
    /// Radial march step. Fine enough that the clip reads as a clean edge on a
    /// decal this faint.
    const STEP: f32 = 0.2;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(SEGMENTS + 1);
    let mut indices: Vec<u32> = Vec::with_capacity(SEGMENTS * 3);
    positions.push([0.0, 0.0, 0.0]); // fan center (index 0)
    for i in 0..SEGMENTS {
        let a = (i as f32) / (SEGMENTS as f32) * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        // March outward until the point leaves the arena, capped at `radius`.
        let mut t = 0.0_f32;
        while t + STEP <= radius {
            let probe = center + dir * (t + STEP);
            if !bounds.contains(Vec3::new(probe.x, 1.0, probe.y)) {
                break;
            }
            t += STEP;
        }
        let p = dir * t;
        positions.push([p.x, 0.0, p.y]); // dir.y maps to world/local Z
    }
    for i in 0..SEGMENTS {
        indices.push(0);
        indices.push(1 + i as u32);
        indices.push(1 + ((i + 1) % SEGMENTS) as u32);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs = vec![[0.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Attach meshes to newly spawned totems. Headless mode spawns the bare `Totem`
/// gameplay entity; this graphical-only system gives it a SOLID, clearly-
/// non-player silhouette — a chunky carved post topped with a glowing element
/// orb — plus a very subtle ground disc (clipped to the arena walls) marking the
/// buff radius. Every mesh is a child entity, so the totem's gameplay `Transform`
/// is never touched and the meshes clean up with the totem via recursive
/// despawn. Registered ONLY in `StatesPlugin::build` — never in
/// `add_core_combat_systems`.
pub fn spawn_totem_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Per-map arena shape, for clipping the buff-radius decal to the real walls.
    // `Option` so a scene without the resource simply skips the map-aware clip.
    map_geometry: Option<Res<ActiveMapGeometry>>,
    new_totems: Query<(Entity, &Totem, &Transform), (Added<Totem>, Without<Children>)>,
) {
    let bounds = map_geometry
        .as_ref()
        .map(|g| g.bounds)
        .unwrap_or_default();
    for (totem_entity, totem, transform) in new_totems.iter() {
        let color = totem.element.color();
        let s = color.to_srgba();

        // Solid carved post — short and blocky, distinct from the tall rounded
        // player capsules.
        let post_mesh = meshes.add(Cuboid::new(0.6, 1.3, 0.6));
        let post_mat = materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::new(s.red * 0.5, s.green * 0.5, s.blue * 0.5, 1.0),
            perceptual_roughness: 0.75,
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });

        // Floating element orb on top — reads instantly as a magic totem.
        let orb_mesh = meshes.add(Sphere::new(0.34));
        let orb_mat = materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::new(s.red * 2.5, s.green * 2.5, s.blue * 2.5, 1.0),
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });

        // Very subtle ground disc marking the buff radius, clipped to the active
        // map's walkable region so it never spills past the walls. `Add` blend per
        // the project's ground-indicator convention to avoid z-fighting flicker.
        let disc_mesh = meshes.add(arena_clipped_disc_mesh(
            &bounds,
            transform.translation.xz(),
            totem.radius,
        ));
        let disc_mat = materials.add(StandardMaterial {
            base_color: color.with_alpha(0.08),
            emissive: LinearRgba::new(s.red * 0.18, s.green * 0.18, s.blue * 0.18, 1.0),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        });

        // Child entities anchored to the totem's ground position (y = 0).
        // The core-spawned Totem entity has only Transform/Totem (no visibility
        // components). Give it `Visibility` (which pulls in InheritedVisibility +
        // ViewVisibility) so the mesh children inherit a valid visibility chain —
        // otherwise Bevy logs B0004 for every totem.
        commands
            .entity(totem_entity)
            .insert(Visibility::default())
            .with_children(|parent| {
            // post: base rests on the ground (Cuboid is centered, half-height 0.65)
            parent.spawn((
                Mesh3d(post_mesh),
                MeshMaterial3d(post_mat),
                Transform::from_xyz(0.0, 0.65, 0.0),
            ));
            // orb: floats just above the post top (post spans y 0.0..1.3)
            parent.spawn((
                Mesh3d(orb_mesh),
                MeshMaterial3d(orb_mat),
                Transform::from_xyz(0.0, 1.65, 0.0),
            ));
            // radius disc: a hair above the floor so it doesn't z-fight it
            parent.spawn((
                Mesh3d(disc_mesh),
                MeshMaterial3d(disc_mat),
                Transform::from_xyz(0.0, 0.03, 0.0),
            ));
        });
    }
}

/// Shrink a totem (post + orb + radius disc together, via the child hierarchy)
/// over its final 1.2 seconds as a clean expiry tell. Totems stay fully SOLID
/// otherwise — no alpha fade. Mutates only `Transform.scale`, which gameplay
/// ignores (the pulse system keys off `Totem.radius` and translation), so this
/// remains purely cosmetic and graphical-only.
pub fn update_totem_visuals(mut totems: Query<(&Totem, &mut Transform)>) {
    for (totem, mut transform) in totems.iter_mut() {
        let scale = if totem.duration_remaining < 1.2 {
            (totem.duration_remaining / 1.2).clamp(0.0, 1.0).max(0.001)
        } else {
            1.0
        };
        if (transform.scale.x - scale).abs() > f32::EPSILON {
            transform.scale = Vec3::splat(scale);
        }
    }
}

// ==============================================================================
// Auto-Attack Weapon Swings (graphical-only)
// ==============================================================================
//
// The sim spawns one bare `AutoAttackSwing` marker per LANDED auto-attack
// (combat_core/auto_attack.rs, apply loop). `consume_swing_signals` (FixedUpdate,
// so a signal can never be missed when FixedUpdate ticks multiple times per
// rendered frame) transfers each marker into `WeaponSocket` state and spawns the
// cosmetic arrow for bow shots. `animate_weapon_swings` (Update, once per
// rendered frame) writes the socket transforms: an anticipatory windup read
// live from the attack timer, a release stroke synced to the landed hit, and
// aim yaw toward the target. Registered ONLY in `StatesPlugin::build` — never
// in `systems.rs` — so headless never runs any of this.

/// Seconds of the release stroke (windup -> impact sweep).
const SWING_RELEASE_SECS: f32 = 0.12;
/// Seconds held at full extension so the impact registers before easing back.
const SWING_IMPACT_HOLD_SECS: f32 = 0.05;
/// Seconds of follow-through easing back to rest after the impact hold.
const SWING_FOLLOW_SECS: f32 = 0.25;
/// Fraction of the attack interval spent winding up, clamped to sane bounds
/// so fast daggers still telegraph and slow 2H axes don't hover forever.
const SWING_WINDUP_FRACTION: f32 = 0.30;
const SWING_WINDUP_MIN_SECS: f32 = 0.15;
const SWING_WINDUP_MAX_SECS: f32 = 0.60;
/// Cosmetic arrow flight speed (yd/s) and hard despawn backstop.
const COSMETIC_ARROW_SPEED: f32 = 45.0;
const COSMETIC_ARROW_TTL: f32 = 1.5;

/// Normalized swing parameter in `[-1, 1]`.
///
/// * `s < 0` — windup: eases 0 -> -1 over the anticipation window as
///   `timer` approaches `interval`, holding at -1 while an overdue attack
///   waits (out of range / no LoS).
/// * `s > 0` — release: sweeps from `release_from` (the windup depth at the
///   moment the hit landed, `<= 0`) THROUGH to full extension at 1 over
///   `SWING_RELEASE_SECS` — the pull-back powers the strike instead of being
///   discarded — holds at 1 for `SWING_IMPACT_HOLD_SECS` so the impact
///   registers, then decays to 0 over `SWING_FOLLOW_SECS`.
/// * `s == 0` — at rest.
///
/// Pure so the timing behavior is unit-testable without Bevy (see tests at the
/// bottom of this file). A live `release_t` always wins over windup: the hit
/// already landed, so the stroke plays regardless of what the timer says.
fn swing_param(
    timer: f32,
    interval: f32,
    windup_window: f32,
    release_t: Option<f32>,
    release_from: f32,
) -> f32 {
    if let Some(t) = release_t {
        let from = release_from.clamp(-1.0, 0.0);
        if t < SWING_RELEASE_SECS {
            let p = (t / SWING_RELEASE_SECS).clamp(0.0, 1.0);
            // Ease-in: the stroke accelerates into the hit.
            let p = p * p;
            return from + (1.0 - from) * p;
        }
        if t < SWING_RELEASE_SECS + SWING_IMPACT_HOLD_SECS {
            return 1.0;
        }
        let f = (t - SWING_RELEASE_SECS - SWING_IMPACT_HOLD_SECS) / SWING_FOLLOW_SECS;
        return (1.0 - f).clamp(0.0, 1.0);
    }
    if !(interval > 0.0) || !(windup_window > 0.0) {
        return 0.0; // degenerate attack speed — hold at rest, never NaN
    }
    let windup_start = interval - windup_window;
    if timer >= windup_start {
        let w = ((timer - windup_start) / windup_window).clamp(0.0, 1.0);
        // Ease-in so the raise reads as a deliberate telegraph, not a twitch.
        return -(w * w);
    }
    0.0
}

/// Per-kind pose offset for a swing parameter: local rotation + translation
/// applied on top of the socket's rest mount. Melee kinds arc around local X
/// (raise back on windup, chop through on release); daggers add a forward jab;
/// the bow draws back instead of arcing.
fn swing_pose(kind: WeaponKind, s: f32) -> Transform {
    let pitch = match kind {
        WeaponKind::Bow => {
            // Draw: slight tilt + pull toward the body. Release: tiny forward snap.
            let pull = if s < 0.0 { -s } else { 0.0 };
            let snap = if s > 0.0 { s } else { 0.0 };
            return Transform::from_translation(Vec3::new(0.0, 0.0, -0.18 * pull + 0.08 * snap))
                .with_rotation(Quat::from_rotation_z(0.15 * pull));
        }
        WeaponKind::Dagger => {
            // A stab, not an arc: pull back along the aim axis on windup,
            // lunge hard forward on release, with only a whisper of pitch.
            let pull = if s < 0.0 { -s } else { 0.0 };
            let thrust = if s > 0.0 { s } else { 0.0 };
            return Transform::from_translation(Vec3::new(
                0.0,
                0.0,
                -0.4 * pull + 0.85 * thrust,
            ))
            .with_rotation(Quat::from_rotation_x(0.2 * pull - 0.1 * thrust));
        }
        WeaponKind::Shield => 0.0, // static (plan R9)
        // TwoHandAxe / Mace: big readable arc, raised back past vertical on
        // windup and chopped forward-down through the target on release. In
        // the socket frame, POSITIVE X-rotation pitches forward — windup is
        // negative (s < 0 keeps the product negative), release positive. The
        // mount's own 0.75 forward lean adds to these totals.
        _ => {
            if s < 0.0 {
                0.9 * s
            } else {
                1.4 * s
            }
        }
    };
    Transform::from_rotation(Quat::from_rotation_x(pitch))
}

/// FixedUpdate (graphical-only): consume the sim's landed-attack markers.
/// Main-hand sockets of the attacker begin their release stroke aimed at the
/// hit target; a Bow main hand additionally looses a cosmetic arrow. Attackers
/// with no sockets (pets, wand casters, un-animated classes) no-op — the
/// marker is simply despawned.
pub fn consume_swing_signals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    signals: Query<(Entity, &AutoAttackSwing)>,
    mut sockets: Query<&mut WeaponSocket>,
    positions: Query<&Transform, With<Combatant>>,
) {
    for (signal_entity, signal) in signals.iter() {
        let target_pos = positions.get(signal.target).map(|t| t.translation).ok();
        // Dual-wield alternation: the sim has ONE attack timer, so each landed
        // auto swings whichever dagger is flagged as next, then hands the flag
        // to its twin. Single-weapon classes keep the flag on the main hand
        // permanently.
        let has_off_dagger = sockets.iter().any(|s| {
            s.owner == signal.attacker && s.hand == WeaponHand::Off && s.kind == WeaponKind::Dagger
        });
        for mut socket in sockets.iter_mut() {
            if socket.owner != signal.attacker {
                continue;
            }
            if let Some(pos) = target_pos {
                socket.aim = pos; // both hands track the victim
            }
            if !socket.winds_up_next {
                if has_off_dagger && socket.kind == WeaponKind::Dagger {
                    socket.winds_up_next = true; // this twin swings the NEXT auto
                }
                continue;
            }
            socket.release_t = Some(0.0);
            if has_off_dagger && socket.kind == WeaponKind::Dagger {
                socket.winds_up_next = false; // twin takes over
            }
            // Cosmetic arrow: bow-kind main hand only. This single gate keeps
            // caster Wand Shots (ranged, no bow) and any future non-bow ranged
            // weapon from loosing arrows.
            if signal.ranged && socket.kind == WeaponKind::Bow {
                if let (Ok(from_tf), Some(to)) = (positions.get(signal.attacker), target_pos) {
                    let from = from_tf.translation + Vec3::Y * 1.1;
                    let dir = (to - from).normalize_or_zero();
                    commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(0.06, 0.06, 0.55))),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgb(0.85, 0.78, 0.55),
                            emissive: LinearRgba::new(0.25, 0.2, 0.1, 1.0),
                            unlit: false,
                            ..default()
                        })),
                        Transform::from_translation(from)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                        CosmeticArrow {
                            to: to + Vec3::Y * 1.0,
                            speed: COSMETIC_ARROW_SPEED,
                            ttl: COSMETIC_ARROW_TTL,
                        },
                        PlayMatchEntity,
                    ));
                }
            }
        }
        commands.entity(signal_entity).despawn();
    }
}

/// Update (graphical-only): write every weapon socket's local transform for
/// this rendered frame — aim yaw toward the current target composed with the
/// rest mount and the swing pose. Reads sim state (attack timer, attack speed,
/// auras, positions) and never writes any of it.
pub fn animate_weapon_swings(
    time: Res<Time>,
    mut sockets: Query<(&mut WeaponSocket, &mut Transform, &mut Visibility)>,
    owners: Query<
        (
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            Option<&CastingState>,
            Option<&ChannelingState>,
        ),
        Without<WeaponSocket>,
    >,
) {
    use crate::states::play_match::combat_core::effective_attack_interval;
    use crate::states::play_match::utils::is_incapacitated;
    use crate::states::play_match::{AUTO_SHOT_RANGE, HUNTER_DEAD_ZONE, MELEE_RANGE};

    let dt = time.delta_secs();
    for (mut socket, mut transform, mut visibility) in sockets.iter_mut() {
        let Ok((combatant, owner_tf, auras, casting, channeling)) = owners.get(socket.owner)
        else {
            continue;
        };

        // A polymorphed victim's body swaps to the sheep form — a sheep
        // gripping a full-size axe gives it away, so hide the sockets (the
        // glTF subtree inherits). Stealth does NOT hide: the weapons fade
        // with the body instead (`update_weapon_stealth_fade`).
        let polymorphed = auras.is_some_and(|a| {
            a.auras.iter().any(|au| au.effect_type == AuraType::Polymorph)
        });
        let wanted = if polymorphed {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != wanted {
            *visibility = wanted;
        }

        // Advance / expire the release stroke. `windup_s` is frozen during
        // the stroke — it is the sweep's starting pose — and zeroed at expiry
        // so the next windup ramps fresh.
        if let Some(t) = socket.release_t {
            let t = t + dt;
            if t >= SWING_RELEASE_SECS + SWING_IMPACT_HOLD_SECS + SWING_FOLLOW_SECS {
                socket.release_t = None;
                socket.windup_s = 0.0;
            } else {
                socket.release_t = Some(t);
            }
        }

        // Track the live target's position whenever one exists — weapons face
        // their target during the approach too, not just once in reach.
        let mut target_dist = f32::INFINITY;
        if combatant.is_alive() {
            if let Some(target) = combatant.target {
                if let Ok((target_combatant, target_tf, _, _, _)) = owners.get(target) {
                    if target_combatant.is_alive() {
                        socket.aim = target_tf.translation;
                        target_dist = owner_tf.translation.distance(target_tf.translation);
                    }
                }
            }
        }

        // Windup eligibility: cosmetic-grade approximation of "an attack is
        // coming" — the RELEASE never depends on this (it keys off the sim's
        // landed-hit marker), so a wrong guess here costs at most a windup
        // that eases back down. Mirrors the sim's own can't-swing gates: an
        // incapacitated / casting / channeling attacker's timer is frozen,
        // and a Hunter inside the dead zone can't loose the overdue shot —
        // telegraphing in those states reads as a stuck animation.
        let mut windup_window = 0.0;
        let mut interval = 0.0;
        if socket.winds_up_next
            && socket.release_t.is_none()
            && combatant.is_alive()
            && !combatant.stealthed
            && casting.is_none()
            && channeling.is_none()
            && !is_incapacitated(auras)
        {
            let (reach, min_reach) = if socket.kind == WeaponKind::Bow {
                (AUTO_SHOT_RANGE + 2.0, HUNTER_DEAD_ZONE)
            } else {
                (MELEE_RANGE + 1.5, 0.0)
            };
            if target_dist <= reach && target_dist >= min_reach {
                interval = effective_attack_interval(combatant, auras);
                windup_window = (interval * SWING_WINDUP_FRACTION)
                    .clamp(SWING_WINDUP_MIN_SECS, SWING_WINDUP_MAX_SECS);
            }
        }

        // Windup eases at a bounded rate; the raw parameter is discontinuous
        // during pursuit (overdue timer + the reach boundary flickering as
        // both units move), and rendering it raw strobes the pose. The
        // release stroke stays raw — its sharpness IS the hit — and sweeps
        // from the frozen windup depth through to full extension.
        let s_raw = swing_param(
            combatant.attack_timer,
            interval,
            windup_window,
            socket.release_t,
            socket.windup_s,
        );
        let s = if socket.release_t.is_some() {
            s_raw
        } else {
            let max_step = 6.0 * dt;
            socket.windup_s += (s_raw - socket.windup_s).clamp(-max_step, max_step);
            socket.windup_s
        };

        // Aim yaw: the weapon is RIGID to the body (its transform is local to
        // the hierarchy, so when `move_to_target` turns the parent's facing
        // the weapon turns with it — that rigidity is what reads as a solidly
        // held object while units move). On top of that, a LOCAL yaw angle
        // eases toward the target bearing at a bounded rate: moving units
        // keep the weapon glued to their frame with a gentle drift toward the
        // victim; stationary units converge to exact target facing. A release
        // stroke corrects faster so the hit still lands visually on-target.
        let owner_forward = owner_tf.rotation * Vec3::Z;
        let owner_yaw = owner_forward.x.atan2(owner_forward.z);

        // Absorb LARGE one-frame facing snaps (gate-open first move, a hard
        // target switch) into the local yaw: the weapon holds its world
        // bearing through the snap and eases to the new aim, instead of
        // whipping a quarter-turn with the body. Ordinary per-tick turning
        // stays rigid — only discrete jumps qualify.
        let owner_snap = (owner_yaw - socket.prev_owner_yaw + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        socket.prev_owner_yaw = owner_yaw;
        if owner_snap.abs() > 0.5 {
            socket.yaw_local = (socket.yaw_local - owner_snap + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
        }

        let aim_dir = socket.aim - owner_tf.translation;
        if aim_dir.xz().length_squared() > 1e-6 {
            let bearing = aim_dir.x.atan2(aim_dir.z);
            // Wrap-aware shortest-path delta from current local angle.
            let target_local = bearing - owner_yaw;
            let mut err = target_local - socket.yaw_local;
            err = (err + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let rate = if socket.release_t.is_some() { 20.0 } else { 6.0 };
            let max_step = rate * dt;
            socket.yaw_local += err.clamp(-max_step, max_step);
            // Keep the stored angle wrapped so it never accumulates turns.
            socket.yaw_local = (socket.yaw_local + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
        }

        // Pose composes in the SOCKET frame (left of `rest`), not the model
        // frame: the swing arc must rotate around the socket's horizontal
        // axis and the stab must translate straight along the aim axis,
        // regardless of the roll each model carries inside its mount.
        // Composed model-side, the axe's chop became a flat-faced sideways
        // slap and the dagger's lunge became a sideways drag.
        *transform = Transform::from_rotation(Quat::from_rotation_y(socket.yaw_local))
            * swing_pose(socket.kind, s)
            * socket.rest;
    }
}

/// Update (graphical-only): fly cosmetic arrows to their captured destination
/// and despawn on arrival (or on the TTL backstop — the damage already landed,
/// the arrow is pure theater).
pub fn update_cosmetic_arrows(
    mut commands: Commands,
    time: Res<Time>,
    mut arrows: Query<(Entity, &mut CosmeticArrow, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut arrow, mut transform) in arrows.iter_mut() {
        arrow.ttl -= dt;
        let to_target = arrow.to - transform.translation;
        let step = arrow.speed * dt;
        if arrow.ttl <= 0.0 || to_target.length() <= step {
            commands.entity(entity).despawn();
            continue;
        }
        let dir = to_target.normalize_or_zero();
        transform.translation += dir * step;
        transform.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
    }
}

#[cfg(test)]
mod swing_tests {
    use super::*;

    #[test]
    fn rest_before_windup_window() {
        assert_eq!(swing_param(0.5, 2.0, 0.5, None, 0.0), 0.0);
    }

    #[test]
    fn windup_ramps_monotonically_to_full() {
        let interval = 2.0;
        let window = 0.5;
        let mut last = 0.0;
        for i in 0..=10 {
            let timer = (interval - window) + window * (i as f32 / 10.0);
            let s = swing_param(timer, interval, window, None, 0.0);
            assert!(s <= last + 1e-6, "windup must be monotonically deepening");
            assert!((-1.0..=0.0).contains(&s));
            last = s;
        }
        assert!((last + 1.0).abs() < 1e-5, "full windup reaches -1");
    }

    #[test]
    fn overdue_attack_holds_full_windup() {
        // Timer past the interval (target out of range, attack pending):
        // the weapon holds at full draw instead of snapping back.
        let s = swing_param(3.7, 2.0, 0.5, None, 0.0);
        assert!((s + 1.0).abs() < 1e-5);
    }

    #[test]
    fn release_sweeps_through_from_windup_depth() {
        // The stroke starts AT the frozen windup pose and powers through to
        // full extension — the pull-back is spent, not discarded.
        let start = swing_param(0.0, 2.0, 0.5, Some(0.0), -1.0);
        assert!((start + 1.0).abs() < 1e-5, "stroke begins at the windup pose");
        let peak = swing_param(0.0, 2.0, 0.5, Some(SWING_RELEASE_SECS), -1.0);
        assert!((peak - 1.0).abs() < 1e-5, "stroke reaches full extension");
        // Monotonically rising through the sweep.
        let mut last = -1.0;
        for i in 0..=10 {
            let s = swing_param(0.0, 2.0, 0.5, Some(SWING_RELEASE_SECS * i as f32 / 10.0), -1.0);
            assert!(s >= last - 1e-6, "sweep must rise monotonically");
            last = s;
        }
    }

    #[test]
    fn impact_holds_then_returns_to_rest() {
        // Held at full extension through the impact window...
        let held = swing_param(
            0.0,
            2.0,
            0.5,
            Some(SWING_RELEASE_SECS + SWING_IMPACT_HOLD_SECS * 0.5),
            0.0,
        );
        assert!((held - 1.0).abs() < 1e-5);
        // ...then the follow-through decays back to 0.
        let done = swing_param(
            0.0,
            2.0,
            0.5,
            Some(SWING_RELEASE_SECS + SWING_IMPACT_HOLD_SECS + SWING_FOLLOW_SECS),
            0.0,
        );
        assert!(done.abs() < 1e-5);
    }

    #[test]
    fn release_wins_over_windup_state() {
        // A landed hit plays its stroke even if the timer says "mid-windup"
        // (no-warning release, plan R7 / AE2): with no prior windup the
        // stroke rises from rest regardless of the overdue timer.
        let s = swing_param(1.9, 2.0, 0.5, Some(SWING_RELEASE_SECS * 0.5), 0.0);
        assert!(s > 0.0);
    }

    #[test]
    fn degenerate_interval_is_guarded() {
        for bad in [0.0_f32, -1.0, f32::NAN] {
            let s = swing_param(1.0, bad, 0.5, None, 0.0);
            assert_eq!(s, 0.0, "degenerate interval must rest, not NaN");
        }
        // Interval change mid-windup stays in range (AE3 continuity).
        let s1 = swing_param(1.8, 2.0, 0.5, None, 0.0);
        let s2 = swing_param(1.8, 2.6, 0.6, None, 0.0); // slow applied mid-windup
        assert!((-1.0..=0.0).contains(&s1));
        assert!((-1.0..=0.0).contains(&s2));
        // An out-of-range release_from is clamped, never amplified.
        let s3 = swing_param(0.0, 2.0, 0.5, Some(0.0), -7.0);
        assert!((-1.0..=1.0).contains(&s3));
    }

    #[test]
    fn shield_pose_is_static() {
        let t = swing_pose(WeaponKind::Shield, -1.0);
        assert_eq!(t.translation, Vec3::ZERO);
        assert_eq!(t.rotation, Quat::IDENTITY);
    }

    #[test]
    fn dagger_pose_is_a_thrust_not_an_arc() {
        // Windup pulls back along the aim axis; release thrusts forward —
        // translation dominates and pitch stays a whisper.
        let windup = swing_pose(WeaponKind::Dagger, -1.0);
        assert!(windup.translation.z < -0.2, "windup pulls the dagger back");
        let release = swing_pose(WeaponKind::Dagger, 1.0);
        assert!(release.translation.z > 0.6, "release lunges the dagger forward");
        let (_, angle) = release.rotation.to_axis_angle();
        assert!(angle.abs() < 0.3, "a stab barely rotates");
    }
}

/// Update (graphical-only): fade weapon materials with their owner's stealth,
/// mirroring the body's 40%-alpha darkened tint (`update_stealth_visuals`).
///
/// glTF weapon materials are SHARED assets across every spawned instance of a
/// model, so the fade swaps each weapon-mesh descendant onto a per-instance
/// clone and remembers the original in [`OriginalWeaponMaterial`]; unstealth
/// restores the shared original exactly. The scene subtree spawns async, so
/// this keys off `Changed<Combatant>` (which fires every sim tick — timers
/// mutate) and converges the first frame the meshes exist; the
/// already-faded guard makes the steady state cheap.
pub fn update_weapon_stealth_fade(
    mut commands: Commands,
    combatants: Query<(Entity, &Combatant), Changed<Combatant>>,
    sockets: Query<(Entity, &WeaponSocket)>,
    children: Query<&Children>,
    mesh_mats: Query<&MeshMaterial3d<StandardMaterial>>,
    originals: Query<&OriginalWeaponMaterial>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (owner_entity, combatant) in combatants.iter() {
        for (socket_entity, socket) in sockets.iter() {
            if socket.owner != owner_entity {
                continue;
            }
            for desc in children.iter_descendants(socket_entity) {
                if combatant.stealthed {
                    if originals.get(desc).is_ok() {
                        continue; // already faded
                    }
                    let Ok(mat_handle) = mesh_mats.get(desc) else {
                        continue;
                    };
                    let Some(mat) = materials.get(&mat_handle.0) else {
                        continue;
                    };
                    let mut faded = mat.clone();
                    let c = faded.base_color.to_srgba();
                    faded.base_color =
                        Color::srgba(c.red * 0.6, c.green * 0.6, c.blue * 0.6, 0.4);
                    faded.alpha_mode = bevy::prelude::AlphaMode::Blend;
                    let original = mat_handle.0.clone();
                    let faded_handle = materials.add(faded);
                    commands.entity(desc).insert((
                        MeshMaterial3d(faded_handle),
                        OriginalWeaponMaterial(original),
                    ));
                } else if let Ok(original) = originals.get(desc) {
                    commands
                        .entity(desc)
                        .insert(MeshMaterial3d(original.0.clone()))
                        .remove::<OriginalWeaponMaterial>();
                }
            }
        }
    }
}

// ==============================================================================
// Casting Orb (gathering-orb casting animation)
// ==============================================================================

/// Full-size orb radius (world units) at cast completion; growth scales up to it.
const CASTING_ORB_FULL_SCALE: f32 = 0.35;
/// Height of the orb anchor above the caster's transform (the projectile
/// spawn height, so the completion flash sits where the bolt appears).
const CASTING_ORB_HEIGHT: f32 = 1.5;
/// Horizontal offset from the caster toward the cast target.
const CASTING_ORB_FORWARD: f32 = 0.6;
/// Sputter (interrupt/fizzle) duration — matches the HUD's 0.5s
/// interrupted-display window so both cues end together.
const CASTING_ORB_SPUTTER_SECS: f32 = 0.5;
/// Release-flash duration after a landed completion.
const CASTING_ORB_FLASH_SECS: f32 = 0.25;
/// Seconds between mote spawns while the orb is active.
const CASTING_ORB_MOTE_INTERVAL: f32 = 0.1;
/// Radius of the ring motes stream in from.
const CASTING_ORB_MOTE_RADIUS: f32 = 1.2;
/// Mote travel speed in progress-units per second (~0.4s to reach the orb).
const CASTING_ORB_MOTE_SPEED: f32 = 2.5;
/// Golden angle (radians) — deterministic angular spread for mote offsets
/// without touching any RNG.
const GOLDEN_ANGLE: f32 = 2.399_963;

/// Where the orb sits for a caster at `caster_pos` casting at `target_pos`:
/// chest/launch height, nudged horizontally toward the target.
fn casting_orb_anchor(caster_pos: Vec3, target_pos: Option<Vec3>) -> Vec3 {
    let base = caster_pos + Vec3::Y * CASTING_ORB_HEIGHT;
    let Some(target_pos) = target_pos else {
        return base;
    };
    let mut dir = target_pos - caster_pos;
    dir.y = 0.0;
    if dir.length_squared() < 0.0001 {
        return base;
    }
    base + dir.normalize() * CASTING_ORB_FORWARD
}

/// Spawn a casting orb when a combatant starts a hard cast or channel.
/// No ability filter (R3): anything with cast/channel state gets the orb.
/// Guards: one live (non-ending) orb per caster (drain-life duplicate-check
/// idiom, scoped to Growing/Holding so a back-to-back cast whose prior orb is
/// still in its Sputter/Flash ending doesn't get swallowed), and a cast
/// already flagged `interrupted` never spawns one — a same-frame interrupt's
/// `CastEnding` marker was already consumed, so a late orb would linger.
pub fn spawn_casting_orbs(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    abilities: Res<AbilityDefinitions>,
    new_casts: Query<(Entity, &CastingState), Added<CastingState>>,
    new_channels: Query<(Entity, &ChannelingState), Added<ChannelingState>>,
    existing_orbs: Query<&CastingOrb>,
    casters: Query<&Transform, With<Combatant>>,
) {
    let starts = new_casts
        .iter()
        .map(|(e, c)| {
            (
                e,
                c.ability,
                c.interrupted,
                CastingOrbPhase::Growing,
                Some(c.time_remaining),
            )
        })
        .chain(new_channels.iter().map(|(e, c)| {
            (e, c.ability, c.interrupted, CastingOrbPhase::Holding, None)
        }));

    for (caster_entity, ability, interrupted, phase, time_remaining) in starts {
        if interrupted {
            continue;
        }
        if existing_orbs.iter().any(|orb| {
            orb.caster == caster_entity
                && matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding)
        }) {
            continue;
        }

        let def = abilities.get_unchecked(&ability);
        let ([r, g, b], [er, eg, eb]) = def.cast_color();

        let mesh = meshes.add(Sphere::new(1.0));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            emissive: LinearRgba::rgb(er, eg, eb),
            // Opaque, not Add/Blend: an additive orb only brightens what's
            // behind it, so it read as ghostly and hard to distinguish in
            // play. A solid emissive sphere occludes the background and stays
            // unmistakable, and Opaque is depth-tested so the overlapping
            // orb + mote stack has no blend-order flicker at all (the concern
            // that ruled out Blend). Motes share this material and become
            // crisp solid sparks.
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });

        let initial_intensity = match phase {
            CastingOrbPhase::Holding => 1.0,
            _ => 0.0,
        };

        // cast_total tracks the LIVE cast time (incl. CastTimeIncrease auras
        // such as Curse of Tongues), not the base config value — see the
        // field doc comment. Unused in Holding, so 0.0 is fine there.
        let cast_total = match time_remaining {
            Some(remaining) => remaining.max(def.cast_time),
            None => def.cast_time,
        };

        // Real initial translation (not the world origin) so a mote spawned
        // before the first `update_casting_orbs` tick still streams toward
        // the caster instead of Vec3::ZERO.
        let initial_translation = casters
            .get(caster_entity)
            .map(|caster_transform| casting_orb_anchor(caster_transform.translation, None))
            .unwrap_or(Vec3::ZERO);

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            // Scale ~0 until the first update grows it; translation is real.
            Transform::from_translation(initial_translation).with_scale(Vec3::splat(0.001)),
            CastingOrb {
                caster: caster_entity,
                intensity: initial_intensity,
                phase,
                ending_remaining: 0.0,
                mote_spawn_timer: 0.0,
                mote_index: 0,
                cast_total,
            },
            PlayMatchEntity,
        ));
    }
}

/// Per-frame orb animation: follow the caster, grow with cast progress (hard
/// casts) or hold at full intensity (channels), and play the Sputter/Flash
/// ending animations. Time comes from `Res<Time>` accumulation — never gated
/// on per-frame sim movement (fixed-timestep strobe lesson).
pub fn update_casting_orbs(
    time: Res<Time>,
    abilities: Res<AbilityDefinitions>,
    mut orbs: Query<(&mut CastingOrb, &mut Transform)>,
    casters: Query<&Transform, (With<Combatant>, Without<CastingOrb>)>,
    cast_states: Query<&CastingState>,
    channel_states: Query<&ChannelingState>,
) {
    let dt = time.delta_secs();

    for (mut orb, mut orb_transform) in orbs.iter_mut() {
        let Ok(caster_transform) = casters.get(orb.caster) else {
            continue; // caster entity gone; cleanup handles despawn
        };

        match orb.phase {
            CastingOrbPhase::Growing => {
                let Ok(casting) = cast_states.get(orb.caster) else {
                    continue; // state gone; ending marker or cleanup resolves this
                };
                if !casting.interrupted {
                    let def = abilities.get_unchecked(&casting.ability);
                    let total = if orb.cast_total > 0.0 {
                        orb.cast_total
                    } else {
                        def.cast_time
                    };
                    if total > 0.0 {
                        orb.intensity =
                            (1.0 - casting.time_remaining / total).clamp(0.0, 1.0);
                    }
                }
                let target_pos = casting
                    .target
                    .and_then(|t| casters.get(t).ok())
                    .map(|t| t.translation);
                orb_transform.translation =
                    casting_orb_anchor(caster_transform.translation, target_pos);
                // Ease-in growth: quadratic reads as "gathering power".
                let eased = orb.intensity * orb.intensity;
                orb_transform.scale =
                    Vec3::splat((0.15 + 0.85 * eased) * CASTING_ORB_FULL_SCALE);
            }
            CastingOrbPhase::Holding => {
                let target_pos = channel_states
                    .get(orb.caster)
                    .ok()
                    .and_then(|c| casters.get(c.target).ok())
                    .map(|t| t.translation);
                orb.intensity = 1.0;
                orb_transform.translation =
                    casting_orb_anchor(caster_transform.translation, target_pos);
                orb_transform.scale = Vec3::splat(CASTING_ORB_FULL_SCALE);
            }
            CastingOrbPhase::Sputter => {
                orb.ending_remaining -= dt;
                let t = (orb.ending_remaining / CASTING_ORB_SPUTTER_SECS).clamp(0.0, 1.0);
                // Shrink from the captured intensity down to nothing, with a
                // slight sag — reads as the gathered power dissipating.
                let scale = (0.15 + 0.85 * orb.intensity * orb.intensity)
                    * CASTING_ORB_FULL_SCALE
                    * t;
                orb_transform.scale = Vec3::splat(scale.max(0.001));
                orb_transform.translation.y -= 0.4 * dt;
            }
            CastingOrbPhase::Flash => {
                orb.ending_remaining -= dt;
                let t = 1.0 - (orb.ending_remaining / CASTING_ORB_FLASH_SECS).clamp(0.0, 1.0);
                // Expanding pulse under additive blending reads as a release
                // flash at the projectile launch point.
                orb_transform.scale =
                    Vec3::splat(CASTING_ORB_FULL_SCALE * (1.0 + 1.5 * t));
            }
        }
    }
}

/// Stream motes toward active orbs on a fixed interval. Offsets use a
/// golden-angle sequence keyed on the orb's monotonic mote counter, so the
/// spread looks scattered while staying fully deterministic.
pub fn spawn_casting_orb_motes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    mut orbs: Query<(Entity, &mut CastingOrb, &Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();

    for (orb_entity, mut orb, orb_transform, orb_material) in orbs.iter_mut() {
        if !matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding) {
            continue;
        }

        orb.mote_spawn_timer -= dt;
        if orb.mote_spawn_timer > 0.0 {
            continue;
        }
        orb.mote_spawn_timer = CASTING_ORB_MOTE_INTERVAL;

        let angle = orb.mote_index as f32 * GOLDEN_ANGLE;
        // Vertical variation from the same counter — deterministic scatter.
        let y = ((orb.mote_index % 7) as f32 / 6.0 - 0.5) * 0.8;
        orb.mote_index = orb.mote_index.wrapping_add(1);
        let start_offset = Vec3::new(
            angle.cos() * CASTING_ORB_MOTE_RADIUS,
            y,
            angle.sin() * CASTING_ORB_MOTE_RADIUS,
        );

        let mesh = meshes.add(Sphere::new(0.06));

        commands.spawn((
            Mesh3d(mesh),
            // Reuse the orb's material: same resolved color, same Add mode,
            // and no per-mote material allocation.
            MeshMaterial3d(orb_material.0.clone()),
            Transform::from_translation(orb_transform.translation + start_offset),
            CastingOrbMote {
                orb: orb_entity,
                progress: 0.0,
                speed: CASTING_ORB_MOTE_SPEED,
                start_offset,
            },
            PlayMatchEntity,
        ));
    }
}

/// Move motes along their lerp into the orb; despawn on arrival, or the
/// moment the parent orb is gone or ending (the sputter/flash owns the
/// screen at that point).
pub fn update_casting_orb_motes(
    mut commands: Commands,
    time: Res<Time>,
    mut motes: Query<(Entity, &mut CastingOrbMote, &mut Transform)>,
    orbs: Query<(&CastingOrb, &Transform), Without<CastingOrbMote>>,
) {
    let dt = time.delta_secs();

    for (mote_entity, mut mote, mut mote_transform) in motes.iter_mut() {
        let Ok((orb, orb_transform)) = orbs.get(mote.orb) else {
            commands.entity(mote_entity).despawn();
            continue;
        };
        if !matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding) {
            commands.entity(mote_entity).despawn();
            continue;
        }

        mote.progress += mote.speed * dt;
        if mote.progress >= 1.0 {
            commands.entity(mote_entity).despawn();
            continue;
        }

        // Lerp from the (orb-relative) start offset into the orb center, so a
        // moving caster carries the whole stream with it.
        let from = orb_transform.translation + mote.start_offset;
        mote_transform.translation = from.lerp(orb_transform.translation, mote.progress);
    }
}

/// Consume `CastEnding` markers spawned by core combat at cast/channel
/// resolution sites, transitioning the matching orb into its ending phase.
/// Runs in `FixedUpdate` after `CombatSystemPhase::CombatResolution` — the
/// `consume_swing_signals` placement — because FixedUpdate can tick multiple
/// times per rendered frame, and an Update-schedule consumer could miss a
/// marker whose cast started and ended inside one rendered frame.
pub fn consume_cast_ending_signals(
    mut commands: Commands,
    signals: Query<(Entity, &CastEnding)>,
    mut orbs: Query<&mut CastingOrb>,
) {
    for (signal_entity, ending) in signals.iter() {
        for mut orb in orbs.iter_mut() {
            if orb.caster != ending.caster {
                continue;
            }
            if !matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding) {
                continue; // already ending; nothing to do
            }
            match ending.kind {
                CastEndingKind::Landed => {
                    orb.phase = CastingOrbPhase::Flash;
                    orb.ending_remaining = CASTING_ORB_FLASH_SECS;
                }
                CastEndingKind::Fizzled | CastEndingKind::Interrupted => {
                    orb.phase = CastingOrbPhase::Sputter;
                    orb.ending_remaining = CASTING_ORB_SPUTTER_SECS;
                }
            }
            // At most one NON-ENDING orb per caster (spawn dedup guard) — done.
            break;
        }
        commands.entity(signal_entity).despawn();
    }
}

/// Despawn orbs whose ending animation finished, and silently vanish orbs
/// whose caster lost its cast/channel state with no `CastEnding` marker —
/// caster death, match end, and natural channel completion (all
/// by-design silent; the death/celebration animation owns those moments).
/// Runs in Update AFTER the FixedUpdate consumer, so a marker always wins
/// over the state-gone check within the same rendered frame.
pub fn cleanup_casting_orbs(
    mut commands: Commands,
    orbs: Query<(Entity, &CastingOrb)>,
    cast_states: Query<&CastingState>,
    channel_states: Query<&ChannelingState>,
) {
    for (orb_entity, orb) in orbs.iter() {
        match orb.phase {
            CastingOrbPhase::Sputter | CastingOrbPhase::Flash => {
                if orb.ending_remaining <= 0.0 {
                    commands.entity(orb_entity).despawn();
                }
            }
            CastingOrbPhase::Growing => {
                if cast_states.get(orb.caster).is_err() {
                    commands.entity(orb_entity).despawn();
                }
            }
            CastingOrbPhase::Holding => {
                if channel_states.get(orb.caster).is_err() {
                    commands.entity(orb_entity).despawn();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_bubbles_are_hidden_until_the_gates_open() {
        // The starting-room cleanup: no buff shouting during the countdown.
        assert!(!bubble_visible(BubbleKind::Ability, false));
        assert!(bubble_visible(BubbleKind::Ability, true));
    }

    #[test]
    fn banter_bubbles_render_in_both_gate_states() {
        // Banter's whole point is the pre-gate scene, and the mid-fight shout
        // has to survive the gates opening too.
        assert!(bubble_visible(BubbleKind::Banter, false));
        assert!(bubble_visible(BubbleKind::Banter, true));
    }
}
