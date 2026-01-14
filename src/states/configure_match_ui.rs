//! Configure Match UI - Team Setup and Map Selection
//!
//! This module handles the match configuration screen where players:
//! - Select team sizes (1-3 combatants per team)
//! - Choose character classes for each team slot
//! - Select the arena map
//! - Start the match when ready
//!
//! ## UI Structure
//! - **Three-column layout**: Team 1 | Arena/Map | Team 2
//! - **Character Picker Modal**: Popup for selecting classes
//! - **Dynamic validation**: Start button only enabled when all slots filled
//!
//! ## Interaction Flow
//! 1. User adjusts team sizes with +/- buttons
//! 2. Clicks empty character slots to open picker modal
//! 3. Selects class from modal, slot updates
//! 4. Cycles through maps with arrow buttons
//! 5. Clicks "START MATCH" when all slots filled

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;
use super::{GameState, match_config::{self, MatchConfig}};

/// Resource storing loaded class icon textures for egui rendering.
/// Maps CharacterClass to egui TextureId for efficient icon display.
#[derive(Resource, Default)]
pub struct ClassIcons {
    /// Map of class to egui texture ID
    pub textures: HashMap<match_config::CharacterClass, egui::TextureId>,
    /// Whether icons have been loaded
    pub loaded: bool,
}

/// Resource storing the Bevy image handles for class icons.
/// These are kept alive to prevent the assets from being unloaded.
#[derive(Resource, Default)]
pub struct ClassIconHandles {
    pub handles: Vec<Handle<Image>>,
}

/// System to load class icons and register them with egui.
/// This runs during ConfigureMatch state update and only loads once.
pub fn load_class_icons(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    mut class_icons: ResMut<ClassIcons>,
    mut icon_handles: ResMut<ClassIconHandles>,
    images: Res<Assets<Image>>,
) {
    // Only load once
    if class_icons.loaded {
        return;
    }

    // Check if all images are loaded
    let class_paths = [
        (match_config::CharacterClass::Warrior, "icons/classes/warrior.png"),
        (match_config::CharacterClass::Mage, "icons/classes/mage.png"),
        (match_config::CharacterClass::Rogue, "icons/classes/rogue.png"),
        (match_config::CharacterClass::Priest, "icons/classes/priest.png"),
        (match_config::CharacterClass::Warlock, "icons/classes/warlock.png"),
    ];

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        for (_, path) in &class_paths {
            let handle: Handle<Image> = asset_server.load(*path);
            icon_handles.handles.push(handle);
        }
        return; // Wait for next frame to check if loaded
    }

    // Check if all images are loaded
    let all_loaded = icon_handles.handles.iter().all(|h| images.contains(h));
    if !all_loaded {
        return; // Wait for images to load
    }

    // Register textures with egui
    for (i, (class, _)) in class_paths.iter().enumerate() {
        let handle = icon_handles.handles[i].clone();
        let texture_id = contexts.add_image(handle);
        class_icons.textures.insert(*class, texture_id);
    }

    class_icons.loaded = true;
    info!("Class icons loaded and registered with egui");
}

/// State for the character picker modal.
/// Tracks which slot is being edited when the modal is open.
#[derive(Resource, Default)]
pub struct CharacterPickerState {
    /// Whether the modal is currently visible
    pub active: bool,
    /// Team being edited (1 or 2)
    pub team: u8,
    /// Slot index being edited (0-2)
    pub slot: usize,
}

/// Main UI system for the Configure Match screen.
/// 
/// Renders:
/// - Header with back button and title
/// - Three-column layout (Team 1, Map, Team 2)
/// - Start Match button (enabled when config is valid)
/// - Character picker modal (when active)
pub fn configure_match_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<MatchConfig>,
    mut next_state: ResMut<NextState<GameState>>,
    mut picker_state: Option<ResMut<CharacterPickerState>>,
    mut commands: Commands,
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    class_icons: Res<ClassIcons>,
) {
    use crate::keybindings::GameAction;

    // Initialize picker state if it doesn't exist
    if picker_state.is_none() {
        commands.insert_resource(CharacterPickerState::default());
    }

    let ctx = contexts.ctx_mut();

    // Configure dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    ctx.set_style(style);

    // Handle Back key - close modal if open, otherwise return to main menu
    if keybindings.action_just_pressed(GameAction::Back, &keyboard) {
        if let Some(ref mut picker) = picker_state {
            if picker.active {
                picker.active = false;
            } else {
                next_state.set(GameState::MainMenu);
            }
        } else {
            next_state.set(GameState::MainMenu);
        }
    }

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin {
                    left: 15.0,
                    right: 15.0,
                    top: 20.0,
                    bottom: 20.0,
                })
        )
        .show(ctx, |ui| {
            ui.add_space(10.0);
            
            // Back button - positioned in top-left
            let back_rect = egui::Rect::from_min_size(
                egui::pos2(20.0, 20.0),
                egui::vec2(80.0, 36.0)
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(back_rect), |ui| {
                if ui.button(egui::RichText::new("← BACK").size(20.0)).clicked() {
                    next_state.set(GameState::MainMenu);
                }
            });
            
            // Title - centered relative to full width
            ui.vertical_centered(|ui| {
                ui.heading(
                    egui::RichText::new("CONFIGURE MATCH")
                        .size(42.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );
            });

            ui.add_space(30.0);

            // Main content area with 3 panels
            // Calculate widths to prevent overflow
            let screen_width = ctx.screen_rect().width();
            let margins_and_spacing = 30.0 + 40.0; // Margins + column spacing
            let content_width = screen_width - margins_and_spacing;
            let col_width = content_width / 3.0;
            
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 20.0;
                let panel_width = col_width - 10.0; // Account for borders/padding
                
                // Team 1 column
                ui.vertical(|ui| {
                    ui.set_width(col_width);
                    ui.add_space(5.0);
                    render_team_panel(ui, &mut config, 1, &mut picker_state, panel_width, &class_icons);
                });

                // Map column
                ui.vertical(|ui| {
                    ui.set_width(col_width);
                    ui.add_space(5.0);
                    render_map_panel(ui, &mut config, panel_width);
                });

                // Team 2 column
                ui.vertical(|ui| {
                    ui.set_width(col_width);
                    ui.add_space(5.0);
                    render_team_panel(ui, &mut config, 2, &mut picker_state, panel_width, &class_icons);
                });
            });

            ui.add_space(30.0);

            // Start Match button - centered, only enabled when valid
            ui.vertical_centered(|ui| {
                let is_valid = config.is_valid();
                let button_text = if is_valid {
                    "START MATCH"
                } else {
                    "SELECT CHARACTERS"
                };
                
                let button = egui::Button::new(
                    egui::RichText::new(button_text)
                        .size(24.0)
                        .color(if is_valid {
                            egui::Color32::from_rgb(230, 242, 230)
                        } else {
                            egui::Color32::from_rgb(102, 102, 102)
                        }),
                )
                .min_size(egui::vec2(250.0, 50.0));

                if ui.add_enabled(is_valid, button).clicked() {
                    info!("Starting match with config: {:?}", *config);
                    next_state.set(GameState::PlayMatch);
                }
            });

            ui.add_space(20.0);
        });

    // Character picker modal - shown when active
    if let Some(ref mut picker) = picker_state {
        if picker.active {
            render_character_picker_modal(ctx, &mut config, picker, &class_icons);
        }
    }
}

/// Render the character picker modal window.
///
/// Displays all available character classes with:
/// - Class icon and name
/// - Class description
/// - Hover effects
/// - Click to select
fn render_character_picker_modal(
    ctx: &egui::Context,
    config: &mut MatchConfig,
    picker: &mut CharacterPickerState,
    class_icons: &ClassIcons,
) {
    egui::Window::new(format!("Select Character - Team {} Slot {}", picker.team, picker.slot + 1))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(500.0);

            for class in match_config::CharacterClass::all() {
                let color = class.color();
                let color32 = egui::Color32::from_rgb(
                    (color.to_srgba().red * 255.0) as u8,
                    (color.to_srgba().green * 255.0) as u8,
                    (color.to_srgba().blue * 255.0) as u8,
                );

                // Make entire character option clickable
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 70.0),
                    egui::Sense::click()
                );

                // Background with hover effect
                let bg_color = if response.hovered() {
                    egui::Color32::from_rgb(64, 77, 89)
                } else {
                    egui::Color32::from_rgb(51, 51, 64)
                };

                ui.painter().rect_filled(rect, 8.0, bg_color);
                ui.painter().rect_stroke(
                    rect,
                    8.0,
                    egui::Stroke::new(2.0, color32.gamma_multiply(0.5))
                );

                // Draw content
                let content_rect = rect.shrink(12.0);
                let mut content_pos = content_rect.left_top();
                content_pos.x += 12.0;
                content_pos.y = content_rect.center().y;

                // Class icon
                let icon_size = 46.0;
                let icon_rect = egui::Rect::from_min_size(
                    egui::pos2(content_pos.x, content_pos.y - icon_size / 2.0),
                    egui::vec2(icon_size, icon_size),
                );

                // Draw the actual class icon if loaded, otherwise fall back to colored rectangle
                if let Some(&texture_id) = class_icons.textures.get(class) {
                    ui.painter().image(
                        texture_id,
                        icon_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    // Add border around the icon
                    ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32));
                } else {
                    // Fallback: colored rectangle placeholder
                    ui.painter().rect_filled(icon_rect, 6.0, color32.gamma_multiply(0.3));
                    ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32));
                }

                // Class text
                let text_pos = egui::pos2(content_pos.x + icon_size + 15.0, content_pos.y - 20.0);
                ui.painter().text(
                    text_pos,
                    egui::Align2::LEFT_TOP,
                    class.name(),
                    egui::FontId::proportional(20.0),
                    color32,
                );
                ui.painter().text(
                    egui::pos2(text_pos.x, text_pos.y + 24.0),
                    egui::Align2::LEFT_TOP,
                    class.description(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::from_rgb(153, 153, 153),
                );

                // Handle click - assign character to slot
                if response.clicked() {
                    if picker.team == 1 {
                        if picker.slot < config.team1.len() {
                            config.team1[picker.slot] = Some(*class);
                        }
                    } else {
                        if picker.slot < config.team2.len() {
                            config.team2[picker.slot] = Some(*class);
                        }
                    }
                    picker.active = false;
                }

                ui.add_space(12.0);
            }

            ui.add_space(10.0);

            if ui.button("Cancel").clicked() {
                picker.active = false;
            }
        });
}

/// Render a team panel (Team 1 or Team 2).
///
/// Shows:
/// - Team header with size controls (+/-)
/// - Three character slots (active/inactive based on team size)
fn render_team_panel(
    ui: &mut egui::Ui,
    config: &mut MatchConfig,
    team: u8,
    picker_state: &mut Option<ResMut<CharacterPickerState>>,
    max_width: f32,
    class_icons: &ClassIcons,
) {
    let team_color = if team == 1 {
        egui::Color32::from_rgb(51, 102, 204)
    } else {
        egui::Color32::from_rgb(204, 51, 51)
    };

    // Get current team data
    let team_size = if team == 1 {
        config.team1_size
    } else {
        config.team2_size
    };

    let team_slots: Vec<Option<match_config::CharacterClass>> = if team == 1 {
        config.team1.clone()
    } else {
        config.team2.clone()
    };
    
    // Header with team name and size controls
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(format!("TEAM {}", team)).size(20.0).color(team_color));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Plus button - increase team size
            if ui.add(egui::Button::new("+").min_size(egui::vec2(25.0, 25.0))).clicked() && team_size < 3 {
                if team == 1 {
                    config.set_team1_size(team_size + 1);
                } else {
                    config.set_team2_size(team_size + 1);
                }
            }

            ui.label(egui::RichText::new(format!("{}", team_size)).size(18.0));

            // Minus button - decrease team size
            if ui.add(egui::Button::new("-").min_size(egui::vec2(25.0, 25.0))).clicked() && team_size > 1 {
                if team == 1 {
                    config.set_team1_size(team_size - 1);
                } else {
                    config.set_team2_size(team_size - 1);
                }
            }
        });
    });

    ui.add_space(20.0);

    // Character slots (always show 3, but some may be inactive)
    for slot in 0..3 {
        let character = team_slots.get(slot).and_then(|c| *c);
        let is_active = slot < team_size;

        render_character_slot(ui, team, slot, character, is_active, team_color, picker_state, max_width, class_icons);

        if slot < 2 {
            ui.add_space(12.0);
        }
    }
    
    ui.add_space(20.0);
    
    // Kill Target Selection
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Kill Target Priority").size(16.0).color(team_color));
        ui.add_space(8.0);
        
        // Get enemy team info
        let (enemy_team_size, enemy_slots) = if team == 1 {
            (config.team2_size, config.team2.clone())
        } else {
            (config.team1_size, config.team1.clone())
        };
        
        let current_kill_target = if team == 1 {
            config.team1_kill_target
        } else {
            config.team2_kill_target
        };
        
        // Show enemy characters as kill target options
        for slot in 0..enemy_team_size {
            if let Some(Some(enemy_class)) = enemy_slots.get(slot) {
                let is_selected = current_kill_target == Some(slot);
                
                let button_text = format!("{}. {}", slot + 1, enemy_class.name());
                let button_color = if is_selected {
                    team_color
                } else {
                    egui::Color32::from_rgb(102, 102, 102)
                };
                
                let button = egui::Button::new(
                    egui::RichText::new(button_text)
                        .size(14.0)
                        .color(button_color)
                )
                .min_size(egui::vec2(max_width, 30.0));
                
                if ui.add(button).clicked() {
                    // Toggle selection
                    if is_selected {
                        // Deselect
                        if team == 1 {
                            config.team1_kill_target = None;
                        } else {
                            config.team2_kill_target = None;
                        }
                    } else {
                        // Select this target
                        if team == 1 {
                            config.team1_kill_target = Some(slot);
                        } else {
                            config.team2_kill_target = Some(slot);
                        }
                    }
                }
                
                if slot < enemy_team_size - 1 {
                    ui.add_space(4.0);
                }
            }
        }
        
        if current_kill_target.is_none() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("No priority - team targets freely")
                    .size(12.0)
                    .color(egui::Color32::from_rgb(153, 153, 153))
            );
        }
    });

    ui.add_space(20.0);

    // CC Priority Section
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("CC Priority").size(16.0).color(team_color));
        ui.add_space(8.0);

        // Get enemy team info
        let (enemy_team_size, enemy_slots) = if team == 1 {
            (config.team2_size, config.team2.clone())
        } else {
            (config.team1_size, config.team1.clone())
        };

        // CC type colors
        let stun_color = egui::Color32::from_rgb(204, 51, 51);   // Red
        let sheep_color = egui::Color32::from_rgb(255, 153, 204); // Pink
        let fear_color = egui::Color32::from_rgb(148, 130, 201);  // Purple

        // Get current CC targets
        let (current_stun, current_sheep, current_fear) = if team == 1 {
            (config.team1_stun_target, config.team1_sheep_target, config.team1_fear_target)
        } else {
            (config.team2_stun_target, config.team2_sheep_target, config.team2_fear_target)
        };

        // Stun target row
        render_cc_row(ui, "Stun", stun_color, current_stun, enemy_team_size, &enemy_slots, |slot| {
            if team == 1 {
                if config.team1_stun_target == slot { config.team1_stun_target = None; }
                else { config.team1_stun_target = slot; }
            } else {
                if config.team2_stun_target == slot { config.team2_stun_target = None; }
                else { config.team2_stun_target = slot; }
            }
        });

        ui.add_space(4.0);

        // Sheep target row
        render_cc_row(ui, "Sheep", sheep_color, current_sheep, enemy_team_size, &enemy_slots, |slot| {
            if team == 1 {
                if config.team1_sheep_target == slot { config.team1_sheep_target = None; }
                else { config.team1_sheep_target = slot; }
            } else {
                if config.team2_sheep_target == slot { config.team2_sheep_target = None; }
                else { config.team2_sheep_target = slot; }
            }
        });

        ui.add_space(4.0);

        // Fear target row
        render_cc_row(ui, "Fear", fear_color, current_fear, enemy_team_size, &enemy_slots, |slot| {
            if team == 1 {
                if config.team1_fear_target == slot { config.team1_fear_target = None; }
                else { config.team1_fear_target = slot; }
            } else {
                if config.team2_fear_target == slot { config.team2_fear_target = None; }
                else { config.team2_fear_target = slot; }
            }
        });
    });
}

/// Render a single character slot.
///
/// Display varies based on state:
/// - **Active + Filled**: Shows class icon and name
/// - **Active + Empty**: Shows "Click to select" prompt
/// - **Inactive**: Shows grayed-out dash
fn render_character_slot(
    ui: &mut egui::Ui,
    team: u8,
    slot: usize,
    character: Option<match_config::CharacterClass>,
    is_active: bool,
    team_color: egui::Color32,
    picker_state: &mut Option<ResMut<CharacterPickerState>>,
    max_width: f32,
    class_icons: &ClassIcons,
) {
    let bg_color = if is_active {
        if character.is_some() {
            egui::Color32::from_rgb(64, 77, 89)
        } else {
            egui::Color32::from_rgb(51, 51, 64)
        }
    } else {
        egui::Color32::from_rgb(26, 26, 31)
    };

    let border_alpha = if is_active { 1.0 } else { 0.3 };

    // Allocate space for the slot
    let slot_width = max_width.max(50.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(slot_width, 60.0),
        if is_active { egui::Sense::click() } else { egui::Sense::hover() }
    );

    // Hover effect for active slots
    let visual_bg_color = if is_active && response.hovered() {
        bg_color.linear_multiply(1.2)
    } else {
        bg_color
    };

    // Draw background and border
    ui.painter().rect_filled(rect, 8.0, visual_bg_color);
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(2.0, team_color.gamma_multiply(border_alpha))
    );

    // Draw content based on slot state
    let content_rect = rect.shrink(12.0);
    let mut content_pos = content_rect.left_top();
    content_pos.x += 12.0;
    content_pos.y = content_rect.center().y;

    if let Some(class) = character {
        // Filled slot - show class info
        let color = class.color();
        let color32 = egui::Color32::from_rgb(
            (color.to_srgba().red * 255.0) as u8,
            (color.to_srgba().green * 255.0) as u8,
            (color.to_srgba().blue * 255.0) as u8,
        );

        // Class icon
        let icon_size = 46.0;
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(content_pos.x, content_pos.y - icon_size / 2.0),
            egui::vec2(icon_size, icon_size),
        );

        // Draw the actual class icon if loaded, otherwise fall back to colored rectangle
        if let Some(&texture_id) = class_icons.textures.get(&class) {
            ui.painter().image(
                texture_id,
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            // Add border around the icon
            ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32));
        } else {
            // Fallback: colored rectangle placeholder
            ui.painter().rect_filled(icon_rect, 6.0, color32.gamma_multiply(0.3));
            ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32));
        }

        // Class text
        let text_pos = egui::pos2(content_pos.x + icon_size + 15.0, content_pos.y - 20.0);

        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_TOP,
            class.name(),
            egui::FontId::proportional(20.0),
            color32,
        );

        ui.painter().text(
            egui::pos2(text_pos.x, text_pos.y + 24.0),
            egui::Align2::LEFT_TOP,
            class.description(),
            egui::FontId::proportional(14.0),
            egui::Color32::from_rgb(153, 153, 153),
        );
    } else if is_active {
        // Empty active slot - show prompt
        ui.painter().text(
            content_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Click to select character",
            egui::FontId::proportional(18.0),
            egui::Color32::from_rgb(128, 128, 128),
        );
    } else {
        // Inactive slot - show dash
        ui.painter().text(
            content_rect.center(),
            egui::Align2::CENTER_CENTER,
            "—",
            egui::FontId::proportional(18.0),
            egui::Color32::from_rgb(77, 77, 77),
        );
    }

    // Handle click on active slots - open picker modal
    if is_active && response.clicked() {
        if let Some(ref mut picker) = picker_state {
            picker.active = true;
            picker.team = team;
            picker.slot = slot;
        }
    }
}

/// Render the map selection panel.
/// 
/// Shows:
/// - Arena title
/// - Map preview placeholder
/// - Map navigation controls (◀ name ▶)
/// - Map description
/// - VS text separator
fn render_map_panel(ui: &mut egui::Ui, config: &mut MatchConfig, max_width: f32) {
    ui.vertical_centered(|ui| {
        ui.heading(
            egui::RichText::new("ARENA")
                .size(20.0)
                .color(egui::Color32::from_rgb(230, 204, 153)),
        );

        ui.add_space(20.0);

        // Map preview placeholder
        let preview_width = (max_width * 0.8).min(180.0);
        let preview_height = preview_width * 0.75;
        
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(preview_width, preview_height),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 8.0, egui::Color32::from_rgb(38, 38, 46));
        ui.painter().rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(77, 77, 77)),
        );

        ui.add_space(20.0);
        
        // Map selection controls - centered to match preview width
        let button_width = 25.0;
        let spacing = 8.0;
        let label_width = preview_width - (button_width * 2.0) - (spacing * 2.0);
        
        ui.horizontal(|ui| {
            let available = ui.available_width();
            let controls_width = button_width + spacing + label_width + spacing + button_width;
            let padding = ((available - controls_width) / 2.0).max(0.0);
            
            ui.add_space(padding);
            
            // Previous map button
            if ui.button("◀").clicked() {
                let maps = match_config::ArenaMap::all();
                let current_idx = maps.iter().position(|m| *m == config.map).unwrap_or(0);
                let new_idx = if current_idx == 0 {
                    maps.len() - 1
                } else {
                    current_idx - 1
                };
                config.map = maps[new_idx];
            }

            ui.add_space(spacing);
            
            // Map name label (fixed width)
            ui.allocate_ui_with_layout(
                egui::vec2(label_width, ui.spacing().interact_size.y),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    ui.label(egui::RichText::new(config.map.name()).size(16.0));
                },
            );
            
            ui.add_space(spacing);

            // Next map button
            if ui.button("▶").clicked() {
                let maps = match_config::ArenaMap::all();
                let current_idx = maps.iter().position(|m| *m == config.map).unwrap_or(0);
                let new_idx = (current_idx + 1) % maps.len();
                config.map = maps[new_idx];
            }
        });

        ui.add_space(12.0);

        // Map description
        ui.label(
            egui::RichText::new(config.map.description())
                .size(12.0)
                .color(egui::Color32::from_rgb(153, 153, 153)),
        );

        ui.add_space(30.0);
        
        // VS separator
        ui.heading(
            egui::RichText::new("VS")
                .size(36.0)
                .color(egui::Color32::from_rgb(128, 115, 102)),
        );
    });
}

/// Render a CC target selection row.
///
/// Shows a row with CC type label and buttons for each enemy slot.
/// Format: "Stun: [1] [2] [3]" where selected slot is highlighted.
fn render_cc_row<F>(
    ui: &mut egui::Ui,
    cc_name: &str,
    cc_color: egui::Color32,
    current_target: Option<usize>,
    enemy_team_size: usize,
    enemy_slots: &[Option<match_config::CharacterClass>],
    mut on_click: F,
) where
    F: FnMut(Option<usize>),
{
    ui.horizontal(|ui| {
        // CC type label
        ui.label(
            egui::RichText::new(format!("{}:", cc_name))
                .size(12.0)
                .color(cc_color),
        );

        ui.add_space(4.0);

        // None button
        let none_selected = current_target.is_none();
        let none_color = if none_selected {
            cc_color
        } else {
            egui::Color32::from_rgb(77, 77, 77)
        };

        let none_button = egui::Button::new(
            egui::RichText::new("None")
                .size(11.0)
                .color(none_color)
        )
        .min_size(egui::vec2(36.0, 20.0));

        if ui.add(none_button).clicked() && !none_selected {
            on_click(None);
        }

        // Slot buttons
        for slot in 0..enemy_team_size {
            if let Some(Some(enemy_class)) = enemy_slots.get(slot) {
                let is_selected = current_target == Some(slot);

                // Get class color for the button
                let class_color = enemy_class.color();
                let class_color32 = egui::Color32::from_rgb(
                    (class_color.to_srgba().red * 255.0) as u8,
                    (class_color.to_srgba().green * 255.0) as u8,
                    (class_color.to_srgba().blue * 255.0) as u8,
                );

                let button_color = if is_selected {
                    cc_color
                } else {
                    class_color32.gamma_multiply(0.6)
                };

                // Show slot number with class initial
                let class_initial = enemy_class.name().chars().next().unwrap_or('?');
                let button_text = format!("{}{}", slot + 1, class_initial);

                let button = egui::Button::new(
                    egui::RichText::new(button_text)
                        .size(11.0)
                        .color(button_color)
                )
                .min_size(egui::vec2(28.0, 20.0));

                if ui.add(button).clicked() {
                    on_click(Some(slot));
                }
            }
        }
    });
}

