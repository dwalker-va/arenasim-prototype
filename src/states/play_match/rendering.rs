//! Rendering Systems
//!
//! All UI and visual effect rendering for the Play Match state.
//! Includes:
//! - UI overlays (time controls, combat log, health bars, countdown, victory celebration)
//! - Floating combat text
//! - Spell impact visual effects
//! - Speech bubbles for ability callouts

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::components::*;
use super::abilities::AbilityType;

// ==============================================================================
// UI Rendering Systems
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

/// Render the countdown timer during the pre-combat phase.
/// 
/// Displays a large centered countdown timer showing remaining seconds until gates open.
/// Also shows "Prepare for battle!" message to indicate pre-buffing phase.
pub fn render_countdown(
    mut contexts: EguiContexts,
    countdown: Res<MatchCountdown>,
) {
    // Only show countdown if gates haven't opened yet
    if countdown.gates_opened {
        return;
    }

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };
    
    let screen_rect = ctx.screen_rect();
    let center = screen_rect.center();
    
    // Helper function to draw text with outline
    let draw_text_with_outline = |painter: &egui::Painter, pos: egui::Pos2, text: &str, font_id: egui::FontId, color: egui::Color32| {
        // Draw black outline (8 directions)
        for (dx, dy) in [
            (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0),  // Cardinal
            (-1.5, -1.5), (1.5, -1.5), (-1.5, 1.5), (1.5, 1.5), // Diagonal
        ] {
            painter.text(
                egui::pos2(pos.x + dx, pos.y + dy),
                egui::Align2::CENTER_CENTER,
                text,
                font_id.clone(),
                egui::Color32::BLACK,
            );
        }
        
        // Draw main text
        painter.text(pos, egui::Align2::CENTER_CENTER, text, font_id, color);
    };
    
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("countdown_overlay"),
    ));
    
    // Large countdown number
    let seconds_remaining = countdown.time_remaining.ceil() as i32;
    let countdown_pos = egui::pos2(center.x, center.y - 50.0);
    draw_text_with_outline(
        &painter,
        countdown_pos,
        &format!("{}", seconds_remaining),
        egui::FontId::proportional(120.0),
        egui::Color32::from_rgb(255, 215, 0), // Gold color
    );
    
    // "Prepare for battle!" message
    let message_pos = egui::pos2(center.x, center.y + 30.0);
    draw_text_with_outline(
        &painter,
        message_pos,
        "Prepare for battle!",
        egui::FontId::proportional(32.0),
        egui::Color32::from_rgb(230, 230, 230),
    );
    
    // Hint about buffing
    let hint_pos = egui::pos2(center.x, center.y + 65.0);
    draw_text_with_outline(
        &painter,
        hint_pos,
        "Apply buffs to your team!",
        egui::FontId::proportional(18.0),
        egui::Color32::from_rgb(180, 180, 180),
    );
}

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
    combatants: Query<(&Combatant, &Transform, Option<&CastingState>, Option<&ActiveAuras>)>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

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
                        let stealth_text = "STEALTH";
                        let stealth_font = egui::FontId::monospace(9.0);
                        
                        // Create galley for measuring size
                        let stealth_galley = ui.fonts(|f| f.layout_no_wrap(
                            stealth_text.to_string(),
                            stealth_font.clone(),
                            egui::Color32::from_rgb(180, 120, 230), // Brighter purple
                        ));
                        let stealth_center_pos = egui::pos2(
                            bar_pos.x + (bar_width - stealth_galley.size().x) / 2.0,
                            bar_pos.y + status_offset,
                        );
                        
                        // Draw black outline/stroke for visibility
                        for dx in [-1.0, 0.0, 1.0] {
                            for dy in [-1.0, 0.0, 1.0] {
                                if dx != 0.0 || dy != 0.0 {
                                    let outline_galley = ui.fonts(|f| f.layout_no_wrap(
                                        stealth_text.to_string(),
                                        stealth_font.clone(),
                                        egui::Color32::BLACK,
                                    ));
                                    let outline_pos = egui::pos2(
                                        stealth_center_pos.x + dx,
                                        stealth_center_pos.y + dy,
                                    );
                                    ui.painter().galley(outline_pos, outline_galley, egui::Color32::BLACK);
                                }
                            }
                        }
                        
                        // Draw main text on top
                        ui.painter().galley(stealth_center_pos, stealth_galley, egui::Color32::from_rgb(180, 120, 230));
                        status_offset -= 10.0; // Move next label up
                    }
                    
                    // Status effect indicators (if has auras)
                    if let Some(auras) = active_auras {
                        // STUN indicator with duration countdown
                        if let Some(stun_aura) = auras.auras.iter().find(|a| a.effect_type == AuraType::Stun) {
                            let stun_text = format!("STUN {:.1}s", stun_aura.duration);
                            let stun_font = egui::FontId::monospace(9.0);
                            
                            // Create galley for measuring size
                            let stun_galley = ui.fonts(|f| f.layout_no_wrap(
                                stun_text.clone(),
                                stun_font.clone(),
                                egui::Color32::from_rgb(255, 100, 100),
                            ));
                            let stun_center_pos = egui::pos2(
                                bar_pos.x + (bar_width - stun_galley.size().x) / 2.0,
                                bar_pos.y + status_offset,
                            );
                            
                            // Draw black outline/stroke for visibility
                            for dx in [-1.0, 0.0, 1.0] {
                                for dy in [-1.0, 0.0, 1.0] {
                                    if dx != 0.0 || dy != 0.0 {
                                        let outline_galley = ui.fonts(|f| f.layout_no_wrap(
                                            stun_text.clone(),
                                            stun_font.clone(),
                                            egui::Color32::BLACK,
                                        ));
                                        let outline_pos = egui::pos2(
                                            stun_center_pos.x + dx,
                                            stun_center_pos.y + dy,
                                        );
                                        ui.painter().galley(outline_pos, outline_galley, egui::Color32::BLACK);
                                    }
                                }
                            }
                            
                            // Draw main text on top
                            ui.painter().galley(stun_center_pos, stun_galley, egui::Color32::from_rgb(255, 100, 100));
                            status_offset -= 10.0; // Move next label up
                        }
                        
                        // ROOT indicator with duration countdown
                        if let Some(root_aura) = auras.auras.iter().find(|a| a.effect_type == AuraType::Root) {
                            let root_text = format!("ROOT {:.1}s", root_aura.duration);
                            let root_font = egui::FontId::monospace(9.0);
                            
                            // Create galley for measuring size
                            let root_galley = ui.fonts(|f| f.layout_no_wrap(
                                root_text.clone(),
                                root_font.clone(),
                                egui::Color32::from_rgb(100, 200, 255), // Brighter ice blue
                            ));
                            let root_center_pos = egui::pos2(
                                bar_pos.x + (bar_width - root_galley.size().x) / 2.0,
                                bar_pos.y + status_offset,
                            );
                            
                            // Draw black outline/stroke for visibility
                            for dx in [-1.0, 0.0, 1.0] {
                                for dy in [-1.0, 0.0, 1.0] {
                                    if dx != 0.0 || dy != 0.0 {
                                        let outline_galley = ui.fonts(|f| f.layout_no_wrap(
                                            root_text.clone(),
                                            root_font.clone(),
                                            egui::Color32::BLACK,
                                        ));
                                        let outline_pos = egui::pos2(
                                            root_center_pos.x + dx,
                                            root_center_pos.y + dy,
                                        );
                                        ui.painter().galley(outline_pos, outline_galley, egui::Color32::BLACK);
                                    }
                                }
                            }
                            
                            // Draw main text on top
                            ui.painter().galley(root_center_pos, root_galley, egui::Color32::from_rgb(100, 200, 255));
                            status_offset -= 10.0; // Move next label up
                        }

                        // FEAR indicator with duration countdown
                        if let Some(fear_aura) = auras.auras.iter().find(|a| a.effect_type == AuraType::Fear) {
                            let fear_text = format!("FEAR {:.1}s", fear_aura.duration);
                            let fear_font = egui::FontId::monospace(9.0);

                            // Create galley for measuring size
                            let fear_galley = ui.fonts(|f| f.layout_no_wrap(
                                fear_text.clone(),
                                fear_font.clone(),
                                egui::Color32::from_rgb(148, 103, 189), // Purple for fear
                            ));
                            let fear_center_pos = egui::pos2(
                                bar_pos.x + (bar_width - fear_galley.size().x) / 2.0,
                                bar_pos.y + status_offset,
                            );

                            // Draw black outline/stroke for visibility
                            for dx in [-1.0, 0.0, 1.0] {
                                for dy in [-1.0, 0.0, 1.0] {
                                    if dx != 0.0 || dy != 0.0 {
                                        let outline_galley = ui.fonts(|f| f.layout_no_wrap(
                                            fear_text.clone(),
                                            fear_font.clone(),
                                            egui::Color32::BLACK,
                                        ));
                                        let outline_pos = egui::pos2(
                                            fear_center_pos.x + dx,
                                            fear_center_pos.y + dy,
                                        );
                                        ui.painter().galley(outline_pos, outline_galley, egui::Color32::BLACK);
                                    }
                                }
                            }

                            // Draw main text on top
                            ui.painter().galley(fear_center_pos, fear_galley, egui::Color32::from_rgb(148, 103, 189));
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

                    // Health bar border
                    ui.painter().rect_stroke(
                        egui::Rect::from_min_size(bar_pos, egui::vec2(bar_width, bar_height)),
                        2.0,
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 200, 200)),
                    );
                    
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
                        let ability_def = casting.ability.definition();
                        
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
                                ability_def.name,
                                egui::FontId::proportional(10.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }
            }
        });
}

/// Render the combat log in a scrollable panel.
/// 
/// Displays the most recent combat events in WoW-like fashion:
/// - Scrollable area on the left side of the screen
/// - Color-coded by event type (damage, healing, death)
/// - Auto-scrolls to show latest events
/// - Shows timestamp for each event
pub fn render_combat_log(
    mut contexts: EguiContexts,
    combat_log: Res<CombatLog>,
) {
    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    // Combat log panel on the left side - semi-transparent to reduce obstruction
    egui::SidePanel::left("combat_log_panel")
        .default_width(320.0)
        .max_width(400.0)
        .min_width(250.0)
        .resizable(true)
        .show_separator_line(false) // Hide the black resize separator line
        .frame(egui::Frame::side_top_panel(&ctx.style())
            .fill(egui::Color32::from_black_alpha(180)) // Semi-transparent background
            .stroke(egui::Stroke::NONE)) // Remove border
        .show(ctx, |ui| {
            ui.heading(
                egui::RichText::new("Combat Log")
                    .size(18.0)
                    .color(egui::Color32::from_rgb(230, 204, 153))
            );
            
            ui.add_space(3.0);
            ui.separator();
            ui.add_space(3.0);
            
            // Scrollable area for log entries
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    // Show all combat log entries
                    for entry in &combat_log.entries {
                        // Color based on event type
                        let color = match entry.event_type {
                            CombatLogEventType::Damage => egui::Color32::from_rgb(255, 180, 180), // Light red
                            CombatLogEventType::Healing => egui::Color32::from_rgb(180, 255, 180), // Light green
                            CombatLogEventType::Buff => egui::Color32::from_rgb(180, 220, 255), // Light blue/cyan
                            CombatLogEventType::Death => egui::Color32::from_rgb(200, 100, 100), // Dark red
                            CombatLogEventType::MatchEvent => egui::Color32::from_rgb(200, 200, 100), // Yellow
                            _ => egui::Color32::from_rgb(200, 200, 200), // Gray
                        };
                        
                        // Format timestamp
                        let timestamp_str = format!("[{:>5.1}s]", entry.timestamp);
                        
                        ui.horizontal(|ui| {
                            // Timestamp in gray
                            ui.label(
                                egui::RichText::new(&timestamp_str)
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150))
                            );
                            
                            // Event message in color
                            ui.label(
                                egui::RichText::new(&entry.message)
                                    .size(12.0)
                                    .color(color)
                            );
                        });
                    }
                });
        });
}

/// Render victory celebration UI.
/// 
/// Displays after match ends, showing winner and countdown to results screen.
pub fn render_victory_celebration(
    mut contexts: EguiContexts,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Only render if celebration is active
    let Some(celebration) = celebration else {
        return;
    };

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };
    let screen_rect = ctx.screen_rect();
    let center = screen_rect.center();
    
    // Helper function to draw text with outline
    let draw_text_with_outline = |painter: &egui::Painter, pos: egui::Pos2, text: &str, font_id: egui::FontId, color: egui::Color32| {
        // Draw black outline (8 directions)
        for (dx, dy) in [
            (-3.0, 0.0), (3.0, 0.0), (0.0, -3.0), (0.0, 3.0),  // Cardinal (thicker for victory text)
            (-2.0, -2.0), (2.0, -2.0), (-2.0, 2.0), (2.0, 2.0), // Diagonal
        ] {
            painter.text(
                egui::pos2(pos.x + dx, pos.y + dy),
                egui::Align2::CENTER_CENTER,
                text,
                font_id.clone(),
                egui::Color32::BLACK,
            );
        }
        
        // Draw main text
        painter.text(pos, egui::Align2::CENTER_CENTER, text, font_id, color);
    };
    
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("victory_overlay"),
    ));
    
    // Victory text based on winner
    let (victory_text, victory_color) = match celebration.winner {
        Some(1) => ("TEAM 1 WINS!", egui::Color32::from_rgb(100, 150, 255)), // Blue
        Some(2) => ("TEAM 2 WINS!", egui::Color32::from_rgb(255, 100, 100)), // Red
        None => ("DRAW!", egui::Color32::from_rgb(200, 200, 100)),           // Yellow
        _ => ("MATCH OVER", egui::Color32::from_rgb(200, 200, 200)),        // Gray
    };
    
    // Large victory text
    let victory_pos = egui::pos2(center.x, center.y - 80.0);
    draw_text_with_outline(
        &painter,
        victory_pos,
        victory_text,
        egui::FontId::proportional(96.0),
        victory_color,
    );
    
    // Celebration message (only show "Victory!" if not a draw)
    if celebration.winner.is_some() {
        let celebration_pos = egui::pos2(center.x, center.y + 5.0);
        draw_text_with_outline(
            &painter,
            celebration_pos,
            "Victory!",
            egui::FontId::proportional(42.0),
            egui::Color32::from_rgb(255, 215, 0), // Gold
        );
    }
    
    // Countdown to results
    let seconds_remaining = celebration.time_remaining.ceil() as i32;
    let countdown_pos = egui::pos2(center.x, center.y + 50.0);
    draw_text_with_outline(
        &painter,
        countdown_pos,
        &format!("Results in {}...", seconds_remaining),
        egui::FontId::proportional(20.0),
        egui::Color32::from_rgb(180, 180, 180),
    );
}

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
                    
                    // Draw the damage number with thick outline for visibility
                    let font_id = egui::FontId::proportional(24.0);
                    
                    // Draw thick black outline (8 directions for smooth outline)
                    let outline_color = egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha);
                    for (dx, dy) in [
                        (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0),  // Cardinal
                        (-1.5, -1.5), (1.5, -1.5), (-1.5, 1.5), (1.5, 1.5), // Diagonal
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
        commands.entity(effect_entity).insert((
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

