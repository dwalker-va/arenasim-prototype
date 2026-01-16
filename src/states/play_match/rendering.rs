//! Rendering Systems
//!
//! All UI and visual effect rendering for the Play Match state.
//! Includes:
//! - UI overlays (time controls, combat log, health bars, countdown, victory celebration)
//! - Floating combat text
//! - Spell impact visual effects
//! - Speech bubbles for ability callouts

use bevy::prelude::*;
use bevy::time::Real;
use bevy_egui::{egui, EguiContexts};
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::components::*;

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
// Spell Icon Loading
// ==============================================================================

/// Maps ability names to their icon file paths
fn get_ability_icon_path(ability: &str) -> Option<&'static str> {
    match ability {
        "Frostbolt" => Some("icons/abilities/spell_frost_frostbolt02.jpg"),
        "Frost Nova" => Some("icons/abilities/spell_frost_frostnova.jpg"),
        "Flash Heal" => Some("icons/abilities/spell_holy_flashheal.jpg"),
        "Mind Blast" => Some("icons/abilities/spell_shadow_unholyfrenzy.jpg"),
        "Power Word: Fortitude" => Some("icons/abilities/spell_holy_wordfortitude.jpg"),
        "Charge" => Some("icons/abilities/ability_warrior_charge.jpg"),
        "Rend" => Some("icons/abilities/ability_gouge.jpg"),
        "Mortal Strike" => Some("icons/abilities/ability_warrior_savageblow.jpg"),
        "Heroic Strike" => Some("icons/abilities/ability_rogue_ambush.jpg"),
        "Ambush" => Some("icons/abilities/ability_rogue_ambush.jpg"),
        "Sinister Strike" => Some("icons/abilities/spell_shadow_ritualofsacrifice.jpg"),
        "Kidney Shot" => Some("icons/abilities/ability_rogue_kidneyshot.jpg"),
        "Corruption" => Some("icons/abilities/spell_shadow_abominationexplosion.jpg"),
        "Shadowbolt" => Some("icons/abilities/spell_shadow_shadowbolt.jpg"),
        "Fear" => Some("icons/abilities/spell_shadow_possession.jpg"),
        "Pummel" => Some("icons/abilities/inv_gauntlets_04.jpg"),
        "Kick" => Some("icons/abilities/ability_kick.jpg"),
        "Arcane Intellect" => Some("icons/abilities/spell_holy_magicalsentry.jpg"),
        "Battle Shout" => Some("icons/abilities/ability_warrior_battleshout.jpg"),
        _ => None,
    }
}

/// All abilities that have icons
const SPELL_ICON_ABILITIES: &[&str] = &[
    "Frostbolt", "Frost Nova", "Flash Heal", "Mind Blast", "Power Word: Fortitude",
    "Charge", "Rend", "Mortal Strike", "Heroic Strike", "Ambush",
    "Sinister Strike", "Kidney Shot", "Corruption", "Shadowbolt", "Fear",
    "Pummel", "Kick", "Arcane Intellect", "Battle Shout",
];

/// System to load spell icons and register them with egui.
/// This runs during PlayMatch state update and only loads once.
pub fn load_spell_icons(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    mut spell_icons: ResMut<SpellIcons>,
    mut icon_handles: ResMut<SpellIconHandles>,
    images: Res<Assets<Image>>,
) {
    // Only load once
    if spell_icons.loaded {
        return;
    }

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        for ability in SPELL_ICON_ABILITIES {
            if let Some(path) = get_ability_icon_path(ability) {
                let handle: Handle<Image> = asset_server.load(path);
                icon_handles.handles.push((ability.to_string(), handle));
            }
        }
        return; // Wait for next frame to check if loaded
    }

    // Check if all images are loaded
    let all_loaded = icon_handles.handles.iter().all(|(_, h)| images.contains(h));
    if !all_loaded {
        return; // Wait for images to load
    }

    // Register textures with egui
    for (ability_name, handle) in &icon_handles.handles {
        let texture_id = contexts.add_image(handle.clone());
        spell_icons.textures.insert(ability_name.clone(), texture_id);
    }

    spell_icons.loaded = true;
    info!("Spell icons loaded and registered with egui ({} icons)", spell_icons.textures.len());
}

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
/// Also shows "Prepare for battle!" message and a match preview overlay showing team compositions.
pub fn render_countdown(
    mut contexts: EguiContexts,
    countdown: Res<MatchCountdown>,
    match_config: Res<crate::states::match_config::MatchConfig>,
    class_icons: Res<crate::states::configure_match_ui::ClassIcons>,
) {
    // Only show countdown if gates haven't opened yet
    if countdown.gates_opened {
        return;
    }

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    let screen_rect = ctx.screen_rect();
    let screen_center = screen_rect.center();

    // Shift the entire overlay upward to visually center it
    // (content extends further below center than above)
    let center = egui::pos2(screen_center.x, screen_center.y - 60.0);

    // Helper function to draw text with outline
    let draw_text_with_outline = |painter: &egui::Painter, pos: egui::Pos2, text: &str, font_id: egui::FontId, color: egui::Color32, align: egui::Align2| {
        // Draw black outline (8 directions)
        for (dx, dy) in [
            (-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0),  // Cardinal
            (-1.5, -1.5), (1.5, -1.5), (-1.5, 1.5), (1.5, 1.5), // Diagonal
        ] {
            painter.text(
                egui::pos2(pos.x + dx, pos.y + dy),
                align,
                text,
                font_id.clone(),
                egui::Color32::BLACK,
            );
        }

        // Draw main text
        painter.text(pos, align, text, font_id, color);
    };

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("countdown_overlay"),
    ));

    // Large countdown number
    let seconds_remaining = countdown.time_remaining.ceil() as i32;
    let countdown_pos = egui::pos2(center.x, center.y - 80.0);
    draw_text_with_outline(
        &painter,
        countdown_pos,
        &format!("{}", seconds_remaining),
        egui::FontId::proportional(120.0),
        egui::Color32::from_rgb(255, 215, 0), // Gold color
        egui::Align2::CENTER_CENTER,
    );

    // "Prepare for battle!" message
    let message_pos = egui::pos2(center.x, center.y);
    draw_text_with_outline(
        &painter,
        message_pos,
        "Prepare for battle!",
        egui::FontId::proportional(32.0),
        egui::Color32::from_rgb(230, 230, 230),
        egui::Align2::CENTER_CENTER,
    );

    // Hint about buffing
    let hint_pos = egui::pos2(center.x, center.y + 35.0);
    draw_text_with_outline(
        &painter,
        hint_pos,
        "Apply buffs to your team!",
        egui::FontId::proportional(18.0),
        egui::Color32::from_rgb(180, 180, 180),
        egui::Align2::CENTER_CENTER,
    );

    // =========================================================================
    // Match Preview Overlay - Shows team compositions with icons
    // =========================================================================

    let preview_y = center.y + 100.0;
    let team_spacing = 200.0; // Distance from center to each team
    let icon_size = 40.0;
    let icon_spacing = 50.0; // Vertical spacing between icons

    // Team 1 (left side, blue)
    let team1_x = center.x - team_spacing;
    draw_text_with_outline(
        &painter,
        egui::pos2(team1_x, preview_y),
        "Team 1",
        egui::FontId::proportional(24.0),
        egui::Color32::from_rgb(100, 150, 255), // Blue
        egui::Align2::CENTER_CENTER,
    );

    // Team 1 class icons with names
    let team1_classes: Vec<_> = match_config.team1.iter()
        .filter_map(|c| c.as_ref())
        .collect();
    for (i, class) in team1_classes.iter().enumerate() {
        let y_offset = preview_y + 45.0 + (i as f32 * icon_spacing);

        // Draw icon
        if let Some(&texture_id) = class_icons.textures.get(*class) {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(team1_x - 50.0, y_offset),
                egui::vec2(icon_size, icon_size),
            );
            // Icon border
            painter.rect_stroke(
                icon_rect.expand(2.0),
                4.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 150, 255)),
            );
            painter.image(
                texture_id,
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        // Draw class name next to icon
        draw_text_with_outline(
            &painter,
            egui::pos2(team1_x + 10.0, y_offset),
            class.name(),
            egui::FontId::proportional(18.0),
            egui::Color32::from_rgb(200, 200, 200),
            egui::Align2::LEFT_CENTER,
        );
    }

    // "VS" in center
    draw_text_with_outline(
        &painter,
        egui::pos2(center.x, preview_y + 40.0),
        "VS",
        egui::FontId::proportional(36.0),
        egui::Color32::from_rgb(255, 215, 0), // Gold
        egui::Align2::CENTER_CENTER,
    );

    // Team 2 (right side, red)
    let team2_x = center.x + team_spacing;
    draw_text_with_outline(
        &painter,
        egui::pos2(team2_x, preview_y),
        "Team 2",
        egui::FontId::proportional(24.0),
        egui::Color32::from_rgb(255, 100, 100), // Red
        egui::Align2::CENTER_CENTER,
    );

    // Team 2 class icons with names
    let team2_classes: Vec<_> = match_config.team2.iter()
        .filter_map(|c| c.as_ref())
        .collect();
    for (i, class) in team2_classes.iter().enumerate() {
        let y_offset = preview_y + 45.0 + (i as f32 * icon_spacing);

        // Draw class name (on the left side for team 2)
        draw_text_with_outline(
            &painter,
            egui::pos2(team2_x - 10.0, y_offset),
            class.name(),
            egui::FontId::proportional(18.0),
            egui::Color32::from_rgb(200, 200, 200),
            egui::Align2::RIGHT_CENTER,
        );

        // Draw icon (on the right side for team 2)
        if let Some(&texture_id) = class_icons.textures.get(*class) {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(team2_x + 50.0, y_offset),
                egui::vec2(icon_size, icon_size),
            );
            // Icon border
            painter.rect_stroke(
                icon_rect.expand(2.0),
                4.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 100, 100)),
            );
            painter.image(
                texture_id,
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    // Calculate bottom of team lists for map name positioning
    let max_team_size = team1_classes.len().max(team2_classes.len());
    let map_y = preview_y + 60.0 + (max_team_size as f32 * icon_spacing);
    draw_text_with_outline(
        &painter,
        egui::pos2(center.x, map_y),
        &format!("Arena: {}", match_config.map.name()),
        egui::FontId::proportional(16.0),
        egui::Color32::from_rgb(150, 150, 150),
        egui::Align2::CENTER_CENTER,
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

                    // Absorb shield visualization (light blue extension after health)
                    if let Some(auras) = active_auras {
                        let absorb_amount: f32 = auras.auras.iter()
                            .filter(|a| a.effect_type == AuraType::Absorb)
                            .map(|a| a.magnitude)
                            .sum();

                        if absorb_amount > 0.0 {
                            // Scale absorb relative to max_health for consistent visualization
                            // Cap at 50% of bar width to prevent overflow
                            let absorb_percent = (absorb_amount / combatant.max_health).min(0.5);
                            let absorb_bar_width = bar_width * absorb_percent;

                            // Start where health bar ends
                            let health_bar_width = bar_width * health_percent;
                            let absorb_start_x = bar_pos.x + health_bar_width;

                            // Light blue color for shields
                            let shield_color = egui::Color32::from_rgb(100, 180, 255);

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

// ==============================================================================
// Timeline Constants
// ==============================================================================

const TIMELINE_PIXELS_PER_SECOND: f32 = 30.0;
const TIMELINE_TIME_COLUMN_WIDTH: f32 = 35.0;
const TIMELINE_ICON_SIZE: f32 = 28.0;
const TIMELINE_TIME_TICK_INTERVAL: f32 = 5.0;
/// Top padding so icons at t=0 aren't cut off
const TIMELINE_TOP_PADDING: f32 = 18.0;
/// Minimum vertical spacing between icons to avoid overlap
const TIMELINE_MIN_ICON_SPACING: f32 = 32.0;

/// Render the combat panel with tabbed view (Combat Log or Timeline).
///
/// Displays on the left side of the screen with:
/// - Tabbed interface to switch between Combat Log and Timeline views
/// - Combat Log: scrollable list of combat events, color-coded by type
/// - Timeline: columnar visualization of ability casts per combatant
pub fn render_combat_panel(
    mut contexts: EguiContexts,
    combat_log: Res<CombatLog>,
    mut panel_view: ResMut<CombatPanelView>,
    spell_icons: Res<SpellIcons>,
) {
    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    // Combat panel on the left side - semi-transparent to reduce obstruction
    egui::SidePanel::left("combat_panel")
        .default_width(320.0)
        .max_width(450.0)
        .min_width(280.0)
        .resizable(true)
        .show_separator_line(false)
        .frame(egui::Frame::side_top_panel(&ctx.style())
            .fill(egui::Color32::from_black_alpha(180))
            .stroke(egui::Stroke::NONE))
        .show(ctx, |ui| {
            // Tab bar
            ui.horizontal(|ui| {
                // Combat Log tab
                let log_selected = *panel_view == CombatPanelView::CombatLog;
                if ui.selectable_label(
                    log_selected,
                    egui::RichText::new("Combat Log")
                        .size(14.0)
                        .color(if log_selected {
                            egui::Color32::from_rgb(230, 204, 153)
                        } else {
                            egui::Color32::from_rgb(150, 150, 150)
                        })
                ).clicked() {
                    *panel_view = CombatPanelView::CombatLog;
                }

                ui.add_space(10.0);

                // Timeline tab
                let timeline_selected = *panel_view == CombatPanelView::Timeline;
                if ui.selectable_label(
                    timeline_selected,
                    egui::RichText::new("Timeline")
                        .size(14.0)
                        .color(if timeline_selected {
                            egui::Color32::from_rgb(230, 204, 153)
                        } else {
                            egui::Color32::from_rgb(150, 150, 150)
                        })
                ).clicked() {
                    *panel_view = CombatPanelView::Timeline;
                }
            });

            ui.add_space(3.0);
            ui.separator();
            ui.add_space(3.0);

            // Render the selected view
            match *panel_view {
                CombatPanelView::CombatLog => render_combat_log_content(ui, &combat_log),
                CombatPanelView::Timeline => render_timeline_content(ui, &combat_log, &spell_icons),
            }
        });
}

/// Render the combat log content (used by the tabbed panel).
fn render_combat_log_content(ui: &mut egui::Ui, combat_log: &CombatLog) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in &combat_log.entries {
                // Color based on event type
                let color = match entry.event_type {
                    CombatLogEventType::Damage => egui::Color32::from_rgb(255, 180, 180),
                    CombatLogEventType::Healing => egui::Color32::from_rgb(180, 255, 180),
                    CombatLogEventType::Buff => egui::Color32::from_rgb(180, 220, 255),
                    CombatLogEventType::Death => egui::Color32::from_rgb(200, 100, 100),
                    CombatLogEventType::MatchEvent => egui::Color32::from_rgb(200, 200, 100),
                    _ => egui::Color32::from_rgb(200, 200, 200),
                };

                let timestamp_str = format!("[{:>5.1}s]", entry.timestamp);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&timestamp_str)
                            .size(11.0)
                            .color(egui::Color32::from_rgb(150, 150, 150))
                    );
                    ui.label(
                        egui::RichText::new(&entry.message)
                            .size(12.0)
                            .color(color)
                    );
                });
            }
        });
}

/// Render the timeline content (columnar ability visualization).
fn render_timeline_content(ui: &mut egui::Ui, combat_log: &CombatLog, spell_icons: &SpellIcons) {
    // Get all combatants and sort: Team 1 first, then Team 2
    let mut combatants = combat_log.all_combatants();
    combatants.sort_by(|a, b| {
        let team_a = if a.starts_with("Team 1") { 1 } else { 2 };
        let team_b = if b.starts_with("Team 1") { 1 } else { 2 };
        team_a.cmp(&team_b).then(a.cmp(b))
    });

    // Show all combatants from the start (not just those with casts)
    // This prevents layout shifts as abilities are used
    if combatants.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(
                egui::RichText::new("Waiting for match to start...")
                    .size(14.0)
                    .color(egui::Color32::from_rgb(120, 120, 120))
                    .italics()
            );
        });
        return;
    }

    let num_combatants = combatants.len();
    let current_time = combat_log.match_time;
    // Add top padding so icons at t=0 are visible
    let timeline_height = TIMELINE_TOP_PADDING + (current_time * TIMELINE_PIXELS_PER_SECOND).max(200.0);

    // Calculate dynamic column width to fill available space
    let available_width = ui.available_width() - 15.0; // Reserve space for scrollbar
    let combatant_column_width = if num_combatants > 0 {
        ((available_width - TIMELINE_TIME_COLUMN_WIDTH) / num_combatants as f32).max(60.0)
    } else {
        60.0
    };
    let total_width = TIMELINE_TIME_COLUMN_WIDTH + (num_combatants as f32 * combatant_column_width);

    // Fixed header row with combatant names
    ui.horizontal(|ui| {
        // Remove default spacing so headers align with painted columns
        ui.spacing_mut().item_spacing.x = 0.0;

        // Time column header (empty)
        ui.allocate_space(egui::vec2(TIMELINE_TIME_COLUMN_WIDTH, 24.0));

        // Combatant column headers
        for combatant_id in &combatants {
            let short_name = shorten_combatant_name(combatant_id);
            let team_color = if combatant_id.starts_with("Team 1") {
                egui::Color32::from_rgb(100, 150, 255) // Blue
            } else {
                egui::Color32::from_rgb(255, 100, 100) // Red
            };

            ui.allocate_ui_with_layout(
                egui::vec2(combatant_column_width, 24.0),
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(
                        egui::RichText::new(short_name)
                            .size(12.0)
                            .color(team_color)
                            .strong()
                    );
                }
            );
        }
    });

    ui.add_space(2.0);
    ui.separator();
    ui.add_space(2.0);

    // Scrollable timeline content with always-visible scrollbar
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .animated(false) // Disable scroll animation
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            // Allocate the full timeline space
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(total_width, timeline_height),
                egui::Sense::hover()
            );

            let painter = ui.painter_at(rect);

            // Draw vertical column separator lines
            for i in 0..=num_combatants {
                let x = rect.min.x + TIMELINE_TIME_COLUMN_WIDTH + (i as f32 * combatant_column_width);
                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                    egui::Stroke::new(1.0, egui::Color32::from_white_alpha(30))
                );
            }

            // Draw horizontal time tick lines and labels
            let mut t = 0.0;
            while t <= current_time {
                let y = rect.min.y + TIMELINE_TOP_PADDING + t * TIMELINE_PIXELS_PER_SECOND;

                // Horizontal line across all columns
                painter.line_segment(
                    [egui::pos2(rect.min.x + TIMELINE_TIME_COLUMN_WIDTH, y), egui::pos2(rect.max.x, y)],
                    egui::Stroke::new(1.0, egui::Color32::from_white_alpha(20))
                );

                // Time label on left
                painter.text(
                    egui::pos2(rect.min.x + 5.0, y + 2.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}s", t as u32),
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_rgb(120, 120, 120)
                );

                t += TIMELINE_TIME_TICK_INTERVAL;
            }

            // Draw ability icons for each combatant
            let mut hovered_ability: Option<(String, f32, bool)> = None; // (ability_name, timestamp, interrupted)

            for (col_idx, combatant_id) in combatants.iter().enumerate() {
                let casts = combat_log.ability_casts_for(combatant_id);
                let col_center_x = rect.min.x + TIMELINE_TIME_COLUMN_WIDTH
                    + (col_idx as f32 * combatant_column_width)
                    + (combatant_column_width / 2.0);

                // First pass: calculate base y positions and detect overlaps
                // We'll push overlapping icons down to avoid collision
                let mut icon_positions: Vec<(f32, f32, &str, bool)> = Vec::new(); // (timestamp, adjusted_y, ability_name, interrupted)

                for (timestamp, ability_name, interrupted) in &casts {
                    let base_y = rect.min.y + TIMELINE_TOP_PADDING + timestamp * TIMELINE_PIXELS_PER_SECOND;

                    // Check if this icon would overlap with any previous icon in this column
                    let mut adjusted_y = base_y;
                    for &(_, prev_y, _, _) in &icon_positions {
                        // If icons are too close, push this one down
                        if (adjusted_y - prev_y).abs() < TIMELINE_MIN_ICON_SPACING {
                            adjusted_y = prev_y + TIMELINE_MIN_ICON_SPACING;
                        }
                    }

                    icon_positions.push((*timestamp, adjusted_y, ability_name, *interrupted));
                }

                // Second pass: draw icons at adjusted positions
                for (timestamp, y, ability_name, interrupted) in icon_positions {
                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(col_center_x, y),
                        egui::vec2(TIMELINE_ICON_SIZE, TIMELINE_ICON_SIZE)
                    );

                    // Try to use spell icon if available
                    if let Some(texture_id) = spell_icons.textures.get(ability_name) {
                        // Draw spell icon image with rounded corners (via clipping)
                        // First draw border - red if interrupted, white otherwise
                        let border_color = if interrupted {
                            egui::Color32::from_rgb(255, 60, 60) // Bright red for interrupted
                        } else {
                            egui::Color32::from_white_alpha(100)
                        };
                        painter.rect_stroke(
                            icon_rect.expand(1.0),
                            3.0,
                            egui::Stroke::new(if interrupted { 2.0 } else { 1.0 }, border_color)
                        );
                        // Draw the spell icon (tinted red if interrupted)
                        let icon_tint = if interrupted {
                            egui::Color32::from_rgb(255, 150, 150) // Red tint
                        } else {
                            egui::Color32::WHITE
                        };
                        painter.image(
                            *texture_id,
                            icon_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            icon_tint
                        );
                    } else {
                        // Fallback: colored rectangle with abbreviation
                        let icon_color = if interrupted {
                            egui::Color32::from_rgb(180, 60, 60) // Dark red for interrupted
                        } else {
                            get_ability_icon_color(ability_name)
                        };
                        painter.rect_filled(icon_rect, 3.0, icon_color);

                        let abbrev = get_ability_abbreviation(ability_name);
                        painter.text(
                            icon_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            abbrev,
                            egui::FontId::proportional(9.0),
                            egui::Color32::WHITE
                        );
                    }

                    // Check hover for tooltip
                    if let Some(hover_pos) = response.hover_pos() {
                        if icon_rect.contains(hover_pos) {
                            hovered_ability = Some((ability_name.to_string(), timestamp, interrupted));
                        }
                    }
                }
            }

            // Show tooltip for hovered ability using foreground layer (so it can overflow panel bounds)
            if let Some((ability_name, timestamp, interrupted)) = hovered_ability {
                if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                    // Format: "{time}s {ability}" with time in yellow
                    let time_text = format!("{:.1}s", timestamp);
                    let ability_text = if interrupted {
                        format!(" {} (Interrupted)", ability_name)
                    } else {
                        format!(" {}", ability_name)
                    };
                    let full_text = format!("{}{}", time_text, ability_text);
                    let tooltip_pos = egui::pos2(hover_pos.x + 15.0, hover_pos.y - 10.0);

                    // Use foreground layer painter so tooltip can overflow panel bounds
                    let foreground_painter = ui.ctx().layer_painter(
                        egui::LayerId::new(egui::Order::Foreground, egui::Id::new("timeline_tooltip"))
                    );

                    let font = egui::FontId::proportional(12.0);
                    let time_color = egui::Color32::from_rgb(255, 215, 0); // Gold
                    let ability_color = egui::Color32::WHITE;

                    // Calculate total size for background
                    let full_galley = foreground_painter.layout_no_wrap(
                        full_text,
                        font.clone(),
                        egui::Color32::WHITE
                    );
                    let bg_rect = egui::Rect::from_min_size(
                        tooltip_pos,
                        full_galley.size() + egui::vec2(8.0, 4.0)
                    );
                    foreground_painter.rect_filled(bg_rect, 3.0, egui::Color32::from_black_alpha(220));

                    // Draw time in yellow
                    let time_galley = foreground_painter.layout_no_wrap(
                        time_text,
                        font.clone(),
                        time_color
                    );
                    let time_width = time_galley.size().x;
                    foreground_painter.galley(tooltip_pos + egui::vec2(4.0, 2.0), time_galley, time_color);

                    // Draw ability name in white (after time)
                    foreground_painter.text(
                        tooltip_pos + egui::vec2(4.0 + time_width, 2.0),
                        egui::Align2::LEFT_TOP,
                        ability_text,
                        font,
                        ability_color
                    );
                }
            }
        });
}

/// Shorten combatant name for compact display (e.g., "Team 1 Mage" -> "T1 Mag")
fn shorten_combatant_name(name: &str) -> String {
    let shortened = name
        .replace("Team 1 ", "T1 ")
        .replace("Team 2 ", "T2 ");

    // Further shorten class names
    shortened
        .replace("Warrior", "War")
        .replace("Priest", "Pri")
        .replace("Warlock", "Wlk")
        .replace("Paladin", "Pal")
        .replace("Hunter", "Hun")
        .replace("Shaman", "Sha")
        .replace("Druid", "Dru")
}

/// Get a short abbreviation for an ability name
fn get_ability_abbreviation(ability: &str) -> &'static str {
    match ability {
        "Frostbolt" => "FB",
        "Frost Nova" => "FN",
        "Flash Heal" => "FH",
        "Mind Blast" => "MB",
        "Power Word: Fortitude" => "PF",
        "Charge" => "CH",
        "Rend" => "RD",
        "Mortal Strike" => "MS",
        "Heroic Strike" => "HS",
        "Ambush" => "AM",
        "Sinister Strike" => "SS",
        "Kidney Shot" => "KS",
        "Corruption" => "CO",
        "Shadowbolt" => "SB",
        "Fear" => "FE",
        "Pummel" => "PM",
        "Kick" => "KI",
        _ => {
            // Return first 2 chars as fallback
            "??"
        }
    }
}

/// Get a color for an ability icon based on its type/school
fn get_ability_icon_color(ability: &str) -> egui::Color32 {
    match ability {
        // Frost (blue)
        "Frostbolt" | "Frost Nova" => egui::Color32::from_rgb(60, 120, 180),
        // Holy (yellow/gold)
        "Flash Heal" | "Power Word: Fortitude" => egui::Color32::from_rgb(200, 180, 80),
        // Shadow (purple)
        "Mind Blast" | "Shadowbolt" | "Corruption" | "Fear" => egui::Color32::from_rgb(120, 80, 160),
        // Physical (brown/orange)
        "Charge" | "Rend" | "Mortal Strike" | "Heroic Strike" | "Pummel" => egui::Color32::from_rgb(160, 100, 60),
        // Rogue (yellow)
        "Ambush" | "Sinister Strike" | "Kidney Shot" | "Kick" => egui::Color32::from_rgb(180, 160, 60),
        // Default
        _ => egui::Color32::from_rgb(100, 100, 100),
    }
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
        // Use get_entity to safely handle cases where the entity was despawned
        if let Some(mut entity_commands) = commands.get_entity(effect_entity) {
            entity_commands.insert((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(effect.position + Vec3::new(0.0, 1.0, 0.0)), // Centered at chest height
            ));
        }
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
