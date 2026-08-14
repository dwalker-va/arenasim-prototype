use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
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

