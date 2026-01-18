//! HUD Rendering Systems
//!
//! Health bars, resource bars, cast bars, and time controls.

use bevy::prelude::*;
use bevy::time::Real;
use bevy_egui::{egui, EguiContexts};
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;

// ==============================================================================
// Low HP Highlighting Constants
// ==============================================================================

/// HP percentage threshold below which combatants get highlighted
const LOW_HP_THRESHOLD: f32 = 0.35;
/// Base glow intensity for low HP highlight (0.0-1.0)
const LOW_HP_GLOW_BASE: f32 = 0.3;
/// Pulse amplitude for low HP highlight
const LOW_HP_GLOW_PULSE: f32 = 0.7;
/// Pulse speed (cycles per second)
const LOW_HP_PULSE_SPEED: f32 = 2.0;

// ==============================================================================
// Time Controls
// ==============================================================================

/// Render time controls panel with pause/speed buttons and keyboard shortcuts.
///
/// Displays in top-right corner with semi-transparent background.
pub fn render_time_controls(
    mut contexts: EguiContexts,
    mut sim_speed: ResMut<SimulationSpeed>,
    mut time: ResMut<Time<Virtual>>,
) {
    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    // Position in top-right corner
    let screen_width = ctx.screen_rect().width();
    let panel_width = 180.0;

    egui::Window::new("Time Controls")
        .fixed_pos(egui::pos2(screen_width - panel_width - 10.0, 10.0))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style())
            .fill(egui::Color32::from_black_alpha(200)) // Semi-transparent
            .stroke(egui::Stroke::NONE)) // Remove border
        .show(ctx, |ui| {
            ui.set_width(panel_width);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Speed:")
                        .size(14.0)
                        .color(egui::Color32::from_rgb(200, 200, 200))
                );

                let speed_text = if sim_speed.is_paused() {
                    "PAUSED"
                } else {
                    match sim_speed.multiplier {
                        x if (x - 0.5).abs() < 0.01 => "0.5x",
                        x if (x - 1.0).abs() < 0.01 => "1x",
                        x if (x - 2.0).abs() < 0.01 => "2x",
                        x if (x - 3.0).abs() < 0.01 => "3x",
                        _ => "??",
                    }
                };

                ui.label(
                    egui::RichText::new(speed_text)
                        .size(14.0)
                        .color(if sim_speed.is_paused() {
                            egui::Color32::from_rgb(255, 100, 100)
                        } else {
                            egui::Color32::from_rgb(100, 255, 100)
                        })
                        .strong()
                );
            });

            ui.add_space(5.0);

            ui.horizontal(|ui| {
                // Pause button
                let pause_btn = egui::Button::new(
                    egui::RichText::new(if sim_speed.is_paused() { "▶" } else { "⏸" })
                        .size(16.0)
                ).min_size(egui::vec2(35.0, 30.0));

                if ui.add(pause_btn).clicked() {
                    if sim_speed.is_paused() {
                        sim_speed.multiplier = 1.0;
                    } else {
                        sim_speed.multiplier = 0.0;
                    }
                    time.set_relative_speed(sim_speed.multiplier);
                }

                // Speed buttons
                for &speed in &[0.5, 1.0, 2.0, 3.0] {
                    let is_active = !sim_speed.is_paused() && (sim_speed.multiplier - speed).abs() < 0.01;
                    let label = if speed == 0.5 { "½x" } else { &format!("{}x", speed as u8) };

                    let btn = egui::Button::new(
                        egui::RichText::new(label).size(12.0)
                    )
                    .min_size(egui::vec2(32.0, 30.0))
                    .fill(if is_active {
                        egui::Color32::from_rgb(60, 80, 120)
                    } else {
                        egui::Color32::from_rgb(40, 40, 50)
                    });

                    if ui.add(btn).clicked() {
                        sim_speed.multiplier = speed;
                        time.set_relative_speed(sim_speed.multiplier);
                    }
                }
            });

            ui.add_space(3.0);

            // Keyboard shortcuts hint
            ui.label(
                egui::RichText::new("Space=Pause 1-4=Speed")
                    .size(10.0)
                    .color(egui::Color32::from_rgb(120, 120, 120))
            );
        });
}

// ==============================================================================
// Health Bars
// ==============================================================================

/// Render 2D health, resource, and cast bars above each living combatant's 3D position.
///
/// This system uses egui to draw bars in screen space, converting 3D world positions
/// to 2D screen coordinates. Displays:
/// - **Health bar** (always): Green/yellow/red based on HP percentage
/// - **Resource bar** (if applicable): Colored by resource type
///   - Mana (blue): Mages, Priests - regenerates slowly, starts full
///   - Energy (yellow): Rogues - regenerates rapidly, starts full
///   - Rage (red): Warriors - starts at 0, builds from attacks and taking damage
/// - **Cast bar** (when casting): Orange bar with spell name showing cast progress
pub fn render_health_bars(
    mut contexts: EguiContexts,
    abilities: Res<AbilityDefinitions>,
    combatants: Query<(&Combatant, &Transform, Option<&CastingState>, Option<&ActiveAuras>)>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    time: Res<Time<Real>>,
) {
    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

    // Calculate pulse intensity for low HP highlighting (uses real time so it works when paused)
    let pulse_phase = time.elapsed_secs() * LOW_HP_PULSE_SPEED * std::f32::consts::TAU;
    let pulse_intensity = LOW_HP_GLOW_BASE + LOW_HP_GLOW_PULSE * (0.5 + 0.5 * pulse_phase.sin());

    egui::Area::new(egui::Id::new("health_bars"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            for (combatant, transform, casting_state, active_auras) in combatants.iter() {
                if !combatant.is_alive() {
                    continue;
                }

                // Project 3D position to 2D screen space
                let health_bar_offset = Vec3::new(0.0, 2.5, 0.0); // Above head
                let world_pos = transform.translation + health_bar_offset;

                if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_pos) {
                    let health_percent = combatant.current_health / combatant.max_health;

                    // Health bar dimensions
                    let bar_width = 50.0;
                    let bar_height = 6.0;
                    let bar_spacing = 2.0; // Space between bars
                    let bar_pos = egui::pos2(
                        screen_pos.x - bar_width / 2.0,
                        screen_pos.y - bar_height / 2.0,
                    );

                    // Status indicators above health bar
                    let mut status_offset = -12.0; // Starting position above health bar

                    // STEALTH indicator (if stealthed)
                    if combatant.stealthed {
                        render_status_label(ui, &bar_pos, bar_width, &mut status_offset, "STEALTH", egui::Color32::from_rgb(180, 120, 230));
                    }

                    // Status effect indicators (if has auras)
                    if let Some(auras) = active_auras {
                        // STUN indicator with duration countdown
                        if let Some(stun_aura) = auras.auras.iter().find(|a| a.effect_type == AuraType::Stun) {
                            let stun_text = format!("STUN {:.1}s", stun_aura.duration);
                            render_status_label(ui, &bar_pos, bar_width, &mut status_offset, &stun_text, egui::Color32::from_rgb(255, 100, 100));
                        }

                        // ROOT indicator with duration countdown
                        if let Some(root_aura) = auras.auras.iter().find(|a| a.effect_type == AuraType::Root) {
                            let root_text = format!("ROOT {:.1}s", root_aura.duration);
                            render_status_label(ui, &bar_pos, bar_width, &mut status_offset, &root_text, egui::Color32::from_rgb(100, 200, 255));
                        }

                        // FEAR indicator with duration countdown
                        if let Some(fear_aura) = auras.auras.iter().find(|a| a.effect_type == AuraType::Fear) {
                            let fear_text = format!("FEAR {:.1}s", fear_aura.duration);
                            render_status_label(ui, &bar_pos, bar_width, &mut status_offset, &fear_text, egui::Color32::from_rgb(148, 103, 189));
                        }

                        // SHEEPED indicator with duration countdown (Polymorph)
                        if let Some(poly_aura) = auras.auras.iter().find(|a| a.effect_type == AuraType::Polymorph) {
                            let poly_text = format!("SHEEPED {:.1}s", poly_aura.duration);
                            render_status_label(ui, &bar_pos, bar_width, &mut status_offset, &poly_text, egui::Color32::from_rgb(255, 105, 180)); // Hot pink
                        }
                    }

                    // Health bar background (dark gray)
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(bar_pos, egui::vec2(bar_width, bar_height)),
                        2.0,
                        egui::Color32::from_rgb(30, 30, 30),
                    );

                    // Health bar fill (color based on health %)
                    let health_color = if health_percent > 0.5 {
                        egui::Color32::from_rgb(0, 200, 0) // Green
                    } else if health_percent > 0.25 {
                        egui::Color32::from_rgb(255, 200, 0) // Yellow
                    } else {
                        egui::Color32::from_rgb(200, 0, 0) // Red
                    };

                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            bar_pos,
                            egui::vec2(bar_width * health_percent, bar_height),
                        ),
                        2.0,
                        health_color,
                    );

                    // Absorb shield visualization (translucent white overlay from right)
                    if let Some(auras) = active_auras {
                        let absorb_amount: f32 = auras.auras.iter()
                            .filter(|a| a.effect_type == AuraType::Absorb)
                            .map(|a| a.magnitude)
                            .sum();

                        if absorb_amount > 0.0 {
                            // Scale absorb relative to max_health, cap at 100% of bar
                            let absorb_percent = (absorb_amount / combatant.max_health).min(1.0);
                            let absorb_bar_width = bar_width * absorb_percent;

                            // Draw from right edge, going left
                            let absorb_start_x = bar_pos.x + bar_width - absorb_bar_width;

                            // Translucent white overlay
                            let shield_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 100);

                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(absorb_start_x, bar_pos.y),
                                    egui::vec2(absorb_bar_width, bar_height),
                                ),
                                2.0,
                                shield_color,
                            );
                        }
                    }

                    // Health bar border (pulsing red if low HP)
                    let is_low_hp = health_percent < LOW_HP_THRESHOLD;
                    let border_color = if is_low_hp {
                        // Pulsing red border for low HP
                        let red_intensity = (200.0 + 55.0 * pulse_intensity) as u8;
                        egui::Color32::from_rgb(red_intensity, 50, 50)
                    } else {
                        egui::Color32::from_rgb(200, 200, 200)
                    };
                    let border_width = if is_low_hp { 2.0 } else { 1.0 };

                    ui.painter().rect_stroke(
                        egui::Rect::from_min_size(bar_pos, egui::vec2(bar_width, bar_height)),
                        2.0,
                        egui::Stroke::new(border_width, border_color),
                    );

                    // Low HP outer glow effect (pulsing red halo)
                    if is_low_hp {
                        let glow_alpha = (80.0 * pulse_intensity) as u8;
                        let glow_expand = 3.0 + 2.0 * pulse_intensity;
                        ui.painter().rect_stroke(
                            egui::Rect::from_min_size(
                                bar_pos - egui::vec2(glow_expand, glow_expand),
                                egui::vec2(bar_width + glow_expand * 2.0, bar_height + glow_expand * 2.0),
                            ),
                            4.0,
                            egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(255, 50, 50, glow_alpha)),
                        );
                    }

                    // Resource bar (mana/energy/rage)
                    let mut next_bar_y_offset = bar_height + bar_spacing;
                    if combatant.max_mana > 0.0 {
                        let resource_percent = combatant.current_mana / combatant.max_mana;
                        let resource_bar_pos = egui::pos2(
                            bar_pos.x,
                            bar_pos.y + next_bar_y_offset,
                        );
                        let resource_bar_height = 4.0; // Slightly smaller than health bar

                        // Determine resource color based on type
                        let (resource_color, border_color) = match combatant.resource_type {
                            ResourceType::Mana => (
                                egui::Color32::from_rgb(80, 150, 255),  // Blue
                                egui::Color32::from_rgb(150, 150, 200),
                            ),
                            ResourceType::Energy => (
                                egui::Color32::from_rgb(255, 255, 100), // Yellow
                                egui::Color32::from_rgb(200, 200, 150),
                            ),
                            ResourceType::Rage => (
                                egui::Color32::from_rgb(255, 80, 80),   // Red
                                egui::Color32::from_rgb(200, 150, 150),
                            ),
                        };

                        // Resource bar background
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(resource_bar_pos, egui::vec2(bar_width, resource_bar_height)),
                            2.0,
                            egui::Color32::from_rgb(20, 20, 30),
                        );

                        // Resource bar fill (colored by resource type)
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                resource_bar_pos,
                                egui::vec2(bar_width * resource_percent, resource_bar_height),
                            ),
                            2.0,
                            resource_color,
                        );

                        // Resource bar border
                        ui.painter().rect_stroke(
                            egui::Rect::from_min_size(resource_bar_pos, egui::vec2(bar_width, resource_bar_height)),
                            2.0,
                            egui::Stroke::new(1.0, border_color),
                        );

                        next_bar_y_offset += resource_bar_height + bar_spacing;
                    }

                    // Cast bar (only when actively casting)
                    if let Some(casting) = casting_state {
                        let ability_def = abilities.get_unchecked(&casting.ability);

                        let cast_bar_pos = egui::pos2(
                            bar_pos.x,
                            bar_pos.y + next_bar_y_offset,
                        );
                        let cast_bar_height = 8.0; // Slightly larger than other bars
                        let cast_bar_width = bar_width + 10.0; // Wider for better visibility

                        // Adjust x position to keep it centered
                        let cast_bar_pos = egui::pos2(
                            cast_bar_pos.x - 5.0,
                            cast_bar_pos.y,
                        );

                        // Interrupted casts show in RED
                        if casting.interrupted {
                            // Red background for interrupted
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                                2.0,
                                egui::Color32::from_rgb(150, 20, 20), // Dark red
                            );

                            // Red border
                            ui.painter().rect_stroke(
                                egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                                2.0,
                                egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 50, 50)),
                            );

                            // "INTERRUPTED" text in white
                            let text_pos = egui::pos2(
                                cast_bar_pos.x + cast_bar_width / 2.0,
                                cast_bar_pos.y + cast_bar_height / 2.0,
                            );
                            ui.painter().text(
                                text_pos,
                                egui::Align2::CENTER_CENTER,
                                "INTERRUPTED",
                                egui::FontId::proportional(10.0),
                                egui::Color32::WHITE,
                            );
                        } else {
                            // Normal cast bar
                            let cast_progress = 1.0 - (casting.time_remaining / ability_def.cast_time);

                            // Cast bar background (darker)
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                                2.0,
                                egui::Color32::from_rgb(15, 15, 20),
                            );

                            // Cast bar fill (orange/yellow, WoW-style)
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    cast_bar_pos,
                                    egui::vec2(cast_bar_width * cast_progress, cast_bar_height),
                                ),
                                2.0,
                                egui::Color32::from_rgb(255, 180, 50), // Orange
                            );

                            // Cast bar border
                            ui.painter().rect_stroke(
                                egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                                2.0,
                                egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 200, 100)),
                            );

                            // Spell name text (centered on cast bar)
                            let text_pos = egui::pos2(
                                cast_bar_pos.x + cast_bar_width / 2.0,
                                cast_bar_pos.y + cast_bar_height / 2.0,
                            );
                            ui.painter().text(
                                text_pos,
                                egui::Align2::CENTER_CENTER,
                                &ability_def.name,
                                egui::FontId::proportional(10.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }
            }
        });
}

/// Helper to render a status label above the health bar with outline
fn render_status_label(
    ui: &mut egui::Ui,
    bar_pos: &egui::Pos2,
    bar_width: f32,
    status_offset: &mut f32,
    text: &str,
    color: egui::Color32,
) {
    let font = egui::FontId::monospace(9.0);

    // Create galley for measuring size
    let galley = ui.fonts(|f| f.layout_no_wrap(
        text.to_string(),
        font.clone(),
        color,
    ));
    let center_pos = egui::pos2(
        bar_pos.x + (bar_width - galley.size().x) / 2.0,
        bar_pos.y + *status_offset,
    );

    // Draw black outline/stroke for visibility
    for dx in [-1.0, 0.0, 1.0] {
        for dy in [-1.0, 0.0, 1.0] {
            if dx != 0.0 || dy != 0.0 {
                let outline_galley = ui.fonts(|f| f.layout_no_wrap(
                    text.to_string(),
                    font.clone(),
                    egui::Color32::BLACK,
                ));
                let outline_pos = egui::pos2(
                    center_pos.x + dx,
                    center_pos.y + dy,
                );
                ui.painter().galley(outline_pos, outline_galley, egui::Color32::BLACK);
            }
        }
    }

    // Draw main text on top
    ui.painter().galley(center_pos, galley, color);
    *status_offset -= 10.0; // Move next label up
}
