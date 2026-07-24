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
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::render::camera::RenderTarget;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::RenderLayers;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;
use super::{GameState, match_config::{self, MatchConfig, ArenaMap}};
use super::view_combatant_ui::ViewCombatantState;
use super::play_match::map_config::MapGeometryConfig;
use super::play_match::map_geometry::ObstacleVolume;
use super::play_match::{spawn_arena_environment, ARENA_FLOOR_HALF_X, ARENA_FLOOR_HALF_Z, ARENA_FLOOR_CORNER_CUT};

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
        (match_config::CharacterClass::Paladin, "icons/classes/paladin.png"),
        (match_config::CharacterClass::Hunter, "icons/classes/hunter.png"),
        (match_config::CharacterClass::Shaman, "icons/classes/shaman.png"),
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

/// A user action produced by the Configure Match screen that requires ECS
/// side effects (state transitions, resource inserts). Everything that only
/// mutates `MatchConfig` / `CharacterPickerState` is handled inside the pure
/// draw function; only these bubble out to the Bevy wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigureMatchAction {
    /// Return to the main menu.
    Back,
    /// Begin the match with the current config.
    StartMatch,
    /// Open the View Combatant screen for a filled slot.
    ViewCombatant {
        class: match_config::CharacterClass,
        team: u8,
        slot: usize,
    },
}

/// Main UI system for the Configure Match screen (Bevy wrapper).
///
/// Grabs the egui context and resources, drives the pure
/// [`draw_configure_match`] renderer, and applies the returned
/// [`ConfigureMatchAction`] via ECS.
pub fn configure_match_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<MatchConfig>,
    mut next_state: ResMut<NextState<GameState>>,
    mut picker_state: Option<ResMut<CharacterPickerState>>,
    mut commands: Commands,
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    class_icons: Res<ClassIcons>,
    map_geometry: Res<MapGeometryConfig>,
    map_preview: Option<Res<MapPreview>>,
) {
    use crate::keybindings::GameAction;

    // Initialize picker state if it doesn't exist
    if picker_state.is_none() {
        commands.insert_resource(CharacterPickerState::default());
    }

    // Use try_ctx_mut to avoid panic when context isn't ready during state transitions
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

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

    // The pure renderer needs a live picker state; if the resource was only
    // just queued this frame, fall back to a scratch one so the screen still
    // draws (the resource lands next frame).
    let mut scratch_picker = CharacterPickerState::default();
    let picker: &mut CharacterPickerState = match picker_state {
        Some(ref mut p) => p,
        None => &mut scratch_picker,
    };

    // Show the live 3D preview texture once it's registered and rendering the
    // currently-selected map; otherwise the pure renderer falls back to the
    // vector schematic (which is also what the offscreen egui harness sees).
    let preview_texture = map_preview
        .as_ref()
        .filter(|p| p.rendered_map == config.map)
        .map(|p| p.texture_id);

    match draw_configure_match(ctx, &mut config, picker, &class_icons, &map_geometry, preview_texture) {
        Some(ConfigureMatchAction::Back) => {
            next_state.set(GameState::MainMenu);
        }
        Some(ConfigureMatchAction::StartMatch) => {
            info!("Starting match with config: {:?}", *config);
            next_state.set(GameState::PlayMatch);
        }
        Some(ConfigureMatchAction::ViewCombatant { class, team, slot }) => {
            commands.insert_resource(ViewCombatantState { class, team, slot });
            next_state.set(GameState::ViewCombatant);
        }
        None => {}
    }
}

/// Render the entire Configure Match screen into `ctx`, returning any action
/// that needs ECS side effects (see [`ConfigureMatchAction`]).
///
/// This is deliberately free of Bevy ECS system params (it takes plain
/// references) so it can be driven directly by an egui harness — see
/// `tests/configure_match_snapshot.rs`, which renders it offscreen with
/// `egui_kittest` for a fast, human-free visual-iteration loop. All state that
/// only mutates the config or the picker modal is applied in place here; only
/// state transitions bubble out through the return value.
pub fn draw_configure_match(
    ctx: &egui::Context,
    config: &mut MatchConfig,
    picker: &mut CharacterPickerState,
    class_icons: &ClassIcons,
    map_geometry: &MapGeometryConfig,
    preview_texture: Option<egui::TextureId>,
) -> Option<ConfigureMatchAction> {
    // Configure dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    ctx.set_style(style);

    let mut action: Option<ConfigureMatchAction> = None;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin {
                    left: 15,
                    right: 15,
                    top: 20,
                    bottom: 20,
                })
        )
        .show(ctx, |ui| {
            ui.add_space(10.0);

            // Back button - custom-painted to match the rest of the (painted)
            // screen instead of egui's default chrome.
            let back_rect = egui::Rect::from_min_size(
                egui::pos2(20.0, 18.0),
                egui::vec2(96.0, 38.0)
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(back_rect), |ui| {
                let back = egui::Button::new(
                    egui::RichText::new("‹  BACK")
                        .size(18.0)
                        .color(egui::Color32::from_rgb(200, 205, 220)),
                )
                .fill(egui::Color32::from_rgb(38, 40, 54))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 74, 92)))
                .corner_radius(6.0)
                .min_size(egui::vec2(96.0, 38.0));
                if ui.add(back).clicked() {
                    action = Some(ConfigureMatchAction::Back);
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

            ui.add_space(24.0);

            // Main content area with 3 panels
            // Calculate widths to prevent overflow
            let screen_width = ctx.screen_rect().width();
            let margins_and_spacing = 30.0 + 40.0; // Margins + column spacing
            let content_width = screen_width - margins_and_spacing;
            let col_width = content_width / 3.0;

            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 20.0;
                let panel_width = col_width - 34.0; // account for the framed inner margin + stroke

                // Team 1 column (blue side)
                team_column_frame(ui, 1, col_width, |ui| {
                    if let Some(a) = render_team_panel(ui, config, 1, picker, panel_width, class_icons) {
                        action = Some(a);
                    }
                });

                // Map column (neutral center) — leads with the VS badge so it
                // sits between the two team headers.
                center_column_frame(ui, col_width, |ui| {
                    render_map_panel(ui, config, panel_width, map_geometry, preview_texture);
                });

                // Team 2 column (red side)
                team_column_frame(ui, 2, col_width, |ui| {
                    if let Some(a) = render_team_panel(ui, config, 2, picker, panel_width, class_icons) {
                        action = Some(a);
                    }
                });
            });

            // Push the action bar to the bottom of the panel so the primary
            // CTA reads as a docked footer instead of floating in dead space.
            let footer_height = 96.0;
            let remaining = ui.available_height();
            if remaining > footer_height {
                ui.add_space(remaining - footer_height);
            } else {
                ui.add_space(22.0);
            }

            // Bottom action bar: a full-width separator, then the primary
            // Start button so it reads as a committed footer action rather
            // than floating in dead space.
            let sep_color = egui::Color32::from_rgb(48, 50, 64);
            let sep_y = ui.cursor().top();
            let sep_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left(), sep_y),
                egui::pos2(ui.max_rect().right(), sep_y + 1.0),
            );
            ui.painter().rect_filled(sep_rect, 0.0, sep_color);
            ui.add_space(20.0);

            ui.vertical_centered(|ui| {
                let is_valid = config.is_valid();
                let button_text = if is_valid {
                    "START MATCH"
                } else {
                    "SELECT CHARACTERS TO CONTINUE"
                };

                let (fill, text_color, stroke) = if is_valid {
                    (
                        egui::Color32::from_rgb(46, 110, 66),
                        egui::Color32::from_rgb(235, 248, 235),
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(96, 176, 116)),
                    )
                } else {
                    (
                        egui::Color32::from_rgb(38, 40, 52),
                        egui::Color32::from_rgb(120, 120, 132),
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 58, 72)),
                    )
                };

                let button = egui::Button::new(
                    egui::RichText::new(button_text).size(24.0).strong().color(text_color),
                )
                .fill(fill)
                .stroke(stroke)
                .corner_radius(8.0)
                .min_size(egui::vec2(300.0, 54.0));

                if ui.add_enabled(is_valid, button).clicked() {
                    action = Some(ConfigureMatchAction::StartMatch);
                }
            });

            ui.add_space(10.0);
        });

    // Character picker modal - shown when active
    if picker.active {
        render_character_picker_modal(ctx, config, picker, class_icons);
    }

    action
}

/// Wrap a team column in a team-tinted, team-bordered frame so the blue side
/// and red side read as distinct at a glance (peripheral-vision identity).
fn team_column_frame(
    ui: &mut egui::Ui,
    team: u8,
    col_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let (fill, stroke) = if team == 1 {
        (
            egui::Color32::from_rgb(26, 32, 48),
            egui::Color32::from_rgb(51, 102, 204),
        )
    } else {
        (
            egui::Color32::from_rgb(46, 28, 32),
            egui::Color32::from_rgb(204, 51, 51),
        )
    };

    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(2.0, stroke.gamma_multiply(0.85)))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(col_width - 28.0);
                add_contents(ui);
            });
        });
}

/// Wrap the center (map) column in a neutral framed panel to match the two
/// team columns' visual weight.
fn center_column_frame(
    ui: &mut egui::Ui,
    col_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(24, 24, 34))
        .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(58, 58, 74)))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(col_width - 28.0);
                add_contents(ui);
            });
        });
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

            // The class currently assigned to the slot being edited, so it can
            // be marked in the list.
            let current_class = if picker.team == 1 {
                config.team1.get(picker.slot).copied().flatten()
            } else {
                config.team2.get(picker.slot).copied().flatten()
            };

            // Scroll so the 8-class list never overflows a short window.
            egui::ScrollArea::vertical()
                .max_height(560.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
            for class in match_config::CharacterClass::all() {
                let is_current = current_class == Some(*class);
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

                // Background with hover effect; the current selection reads
                // brighter with a full-strength class-colored border.
                let bg_color = if is_current {
                    egui::Color32::from_rgb(58, 70, 84)
                } else if response.hovered() {
                    egui::Color32::from_rgb(64, 77, 89)
                } else {
                    egui::Color32::from_rgb(51, 51, 64)
                };

                ui.painter().rect_filled(rect, 8.0, bg_color);
                ui.painter().rect_stroke(
                    rect,
                    8.0,
                    egui::Stroke::new(
                        if is_current { 2.5 } else { 2.0 },
                        if is_current { color32 } else { color32.gamma_multiply(0.5) },
                    ),
                    egui::StrokeKind::Outside,
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
                    ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32), egui::StrokeKind::Outside);
                } else {
                    // Fallback: colored rectangle placeholder
                    ui.painter().rect_filled(icon_rect, 6.0, color32.gamma_multiply(0.3));
                    ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32), egui::StrokeKind::Outside);
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

                // "Selected" marker on the currently-assigned class.
                if is_current {
                    ui.painter().text(
                        egui::pos2(rect.right() - 16.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        "SELECTED",
                        egui::FontId::proportional(13.0),
                        color32,
                    );
                }

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
                });

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
    picker: &mut CharacterPickerState,
    max_width: f32,
    class_icons: &ClassIcons,
) -> Option<ConfigureMatchAction> {
    let mut action: Option<ConfigureMatchAction> = None;

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

        if let Some(a) = render_character_slot(ui, config, team, slot, character, is_active, team_color, picker, max_width, class_icons) {
            action = Some(a);
        }

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
        
        // Show enemy characters as kill target options. Selected = filled
        // team-color chip with light text; unselected = outlined chip so the
        // whole group reads as toggles, not disabled labels.
        for slot in 0..enemy_team_size {
            if let Some(Some(enemy_class)) = enemy_slots.get(slot) {
                let is_selected = current_kill_target == Some(slot);

                let button_text = format!("{}. {}", slot + 1, enemy_class.name());
                let (fill, text_color, stroke) = if is_selected {
                    (
                        team_color.gamma_multiply(0.9),
                        egui::Color32::from_rgb(240, 244, 250),
                        egui::Stroke::new(1.5, team_color),
                    )
                } else {
                    (
                        egui::Color32::from_rgb(40, 42, 54),
                        egui::Color32::from_rgb(180, 184, 196),
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(66, 70, 88)),
                    )
                };

                let button = egui::Button::new(
                    egui::RichText::new(button_text)
                        .size(14.0)
                        .color(text_color)
                )
                .fill(fill)
                .stroke(stroke)
                .corner_radius(5.0)
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

    action
}

/// Render a single character slot.
///
/// Display varies based on state:
/// - **Active + Filled**: Shows class icon and name (click to view details)
/// - **Active + Empty**: Shows "Click to select" prompt (click to open picker)
/// - **Inactive**: Shows grayed-out dash
fn render_character_slot(
    ui: &mut egui::Ui,
    _config: &mut MatchConfig,
    team: u8,
    slot: usize,
    character: Option<match_config::CharacterClass>,
    is_active: bool,
    team_color: egui::Color32,
    picker: &mut CharacterPickerState,
    max_width: f32,
    class_icons: &ClassIcons,
) -> Option<ConfigureMatchAction> {
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
        egui::Stroke::new(2.0, team_color.gamma_multiply(border_alpha)),
        egui::StrokeKind::Outside,
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
            ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32), egui::StrokeKind::Outside);
        } else {
            // Fallback: colored rectangle placeholder
            ui.painter().rect_filled(icon_rect, 6.0, color32.gamma_multiply(0.3));
            ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32), egui::StrokeKind::Outside);
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

        // Add X button in top-right corner to change selection
        let btn_size = 20.0;
        let btn_margin = 8.0;
        let btn_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - btn_size - btn_margin, rect.top() + btn_margin),
            egui::vec2(btn_size, btn_size),
        );

        // Check if mouse is over the X button
        let btn_hovered = ui.rect_contains_pointer(btn_rect);
        let btn_color = if btn_hovered {
            egui::Color32::from_rgb(200, 80, 80)
        } else {
            egui::Color32::from_rgb(120, 120, 120)
        };

        // Draw X button background
        ui.painter().rect_filled(
            btn_rect,
            4.0,
            if btn_hovered {
                egui::Color32::from_rgb(60, 40, 40)
            } else {
                egui::Color32::from_rgb(45, 45, 55)
            },
        );

        // Draw X
        ui.painter().text(
            btn_rect.center(),
            egui::Align2::CENTER_CENTER,
            "X",
            egui::FontId::proportional(12.0),
            btn_color,
        );

        // Card-hover affordance: signal that clicking the card (anywhere but
        // the corner button) opens the combatant detail screen. Without this,
        // the click-to-view action is invisible.
        if response.hovered() && !btn_hovered {
            ui.painter().text(
                egui::pos2(rect.right() - 12.0, rect.bottom() - 10.0),
                egui::Align2::RIGHT_BOTTOM,
                "View ›",
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgb(170, 176, 190),
            );
        }

        // Handle X button click - open picker to change selection
        if btn_hovered && response.clicked() {
            picker.active = true;
            picker.team = team;
            picker.slot = slot;
            return None; // Don't navigate to View Combatant
        }
    } else if is_active {
        // Empty active slot - inviting prompt with an add glyph.
        ui.painter().text(
            content_rect.center(),
            egui::Align2::CENTER_CENTER,
            "+  Add character",
            egui::FontId::proportional(18.0),
            if response.hovered() {
                egui::Color32::from_rgb(180, 186, 200)
            } else {
                egui::Color32::from_rgb(128, 128, 128)
            },
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

    // Handle click on active slots
    if is_active && response.clicked() {
        if let Some(class) = character {
            // Filled slot - navigate to view combatant screen
            return Some(ConfigureMatchAction::ViewCombatant { class, team, slot });
        } else {
            // Empty slot - open picker modal
            picker.active = true;
            picker.team = team;
            picker.slot = slot;
        }
    }

    None
}

/// Render the map selection panel (center column).
///
/// Shows, top to bottom:
/// - A prominent VS badge (aligned with the two team headers — this is the
///   visual divider between the blue and red sides)
/// - Arena title
/// - A top-down schematic preview drawn from the map's real obstacle geometry
/// - Map navigation controls (◀ name ▶)
/// - Map description
fn render_map_panel(
    ui: &mut egui::Ui,
    config: &mut MatchConfig,
    max_width: f32,
    map_geometry: &MapGeometryConfig,
    preview_texture: Option<egui::TextureId>,
) {
    ui.vertical_centered(|ui| {
        // VS badge — the emotional beat, sitting between the team headers.
        ui.heading(
            egui::RichText::new("VS")
                .size(48.0)
                .strong()
                .color(egui::Color32::from_rgb(230, 204, 153)),
        );

        ui.add_space(14.0);

        ui.label(
            egui::RichText::new("ARENA")
                .size(16.0)
                .color(egui::Color32::from_rgb(180, 168, 140)),
        );

        ui.add_space(12.0);

        // Map preview — real top-down geometry for the selected map.
        // Arena floor aspect ratio (x:z) keeps the schematic true to scale.
        let world_w = ARENA_FLOOR_HALF_X * 2.0;
        let world_h = ARENA_FLOOR_HALF_Z * 2.0;
        let preview_width = (max_width * 0.92).min(230.0);
        let preview_height = preview_width * (world_h / world_w);

        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(preview_width, preview_height),
            egui::Sense::hover(),
        );
        // Backing panel behind the preview.
        ui.painter().rect_filled(rect, 8.0, egui::Color32::from_rgb(16, 16, 24));
        ui.painter().rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(70, 74, 92)),
            egui::StrokeKind::Outside,
        );
        if let Some(tex) = preview_texture {
            // Live 3D render of the arena (fixed-camera offscreen pass).
            ui.painter().image(
                tex,
                rect.shrink(3.0),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            // Fallback: vector top-down schematic (also what the egui harness
            // renders, since Bevy render-to-texture isn't available there).
            draw_map_preview(ui.painter(), rect.shrink(10.0), config.map, map_geometry);
        }

        ui.add_space(16.0);

        // Map selection controls. Laid out in a fixed-width row equal to the
        // preview width, with exact widget sizes and zero item spacing, so the
        // row centers under the preview exactly (the enclosing
        // `vertical_centered` centers the whole allocation). The previous
        // manual-padding approach drifted because egui's default item spacing
        // and button padding aren't accounted for in the width math.
        let button_width = 32.0;
        let ctrl_h = 30.0;
        let label_width = preview_width - button_width * 2.0;
        let mut nav: i32 = 0;

        ui.allocate_ui_with_layout(
            egui::vec2(preview_width, ctrl_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                if ui.add_sized([button_width, ctrl_h], egui::Button::new("◀")).clicked() {
                    nav = -1;
                }

                ui.allocate_ui_with_layout(
                    egui::vec2(label_width, ctrl_h),
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        ui.label(
                            egui::RichText::new(config.map.name())
                                .size(16.0)
                                .strong()
                                .color(egui::Color32::from_rgb(220, 224, 235)),
                        );
                    },
                );

                if ui.add_sized([button_width, ctrl_h], egui::Button::new("▶")).clicked() {
                    nav = 1;
                }
            },
        );

        if nav != 0 {
            let maps = match_config::ArenaMap::all();
            let current_idx = maps.iter().position(|m| *m == config.map).unwrap_or(0);
            let new_idx = (current_idx as i32 + nav).rem_euclid(maps.len() as i32) as usize;
            config.map = maps[new_idx];
        }

        ui.add_space(12.0);

        // Map description
        ui.label(
            egui::RichText::new(config.map.description())
                .size(12.0)
                .color(egui::Color32::from_rgb(153, 153, 153)),
        );
    });
}

/// Draw a top-down schematic of `map` into `rect`, using the map's real
/// obstacle geometry (cylinders → circles, boxes → rectangles) scaled to fit.
///
/// World coordinates: x ∈ [-`ARENA_FLOOR_HALF_X`, +], z ∈ [-`ARENA_FLOOR_HALF_Z`, +].
/// World x maps to the schematic's horizontal axis and world z to its vertical
/// axis, preserving aspect. Pure painter work so it renders identically in the
/// egui harness and the real client.
fn draw_map_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    map: match_config::ArenaMap,
    map_geometry: &MapGeometryConfig,
) {
    let world_w = ARENA_FLOOR_HALF_X * 2.0;
    let world_h = ARENA_FLOOR_HALF_Z * 2.0;
    let scale = (rect.width() / world_w).min(rect.height() / world_h);
    let center = rect.center();

    // World (x, z) -> screen point.
    let to_screen = |x: f32, z: f32| egui::pos2(center.x + x * scale, center.y + z * scale);

    // Arena floor: the cut-corner octagon (matches the 3D floor mesh).
    let hx = ARENA_FLOOR_HALF_X;
    let hz = ARENA_FLOOR_HALF_Z;
    let cut = ARENA_FLOOR_CORNER_CUT;
    let floor: Vec<egui::Pos2> = vec![
        to_screen(-hx + cut, -hz),
        to_screen(hx - cut, -hz),
        to_screen(hx, -hz + cut),
        to_screen(hx, hz - cut),
        to_screen(hx - cut, hz),
        to_screen(-hx + cut, hz),
        to_screen(-hx, hz - cut),
        to_screen(-hx, -hz + cut),
    ];
    painter.add(egui::Shape::convex_polygon(
        floor,
        egui::Color32::from_rgb(34, 36, 48),
        egui::Stroke::new(1.5, egui::Color32::from_rgb(90, 96, 120)),
    ));

    // Obstacles for the selected map.
    let obstacle_fill = egui::Color32::from_rgb(96, 102, 128);
    let obstacle_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(150, 156, 180));
    let active = map_geometry.active_for(map);
    for volume in &active.volumes {
        match volume {
            ObstacleVolume::Cylinder { center_xz, radius, .. } => {
                painter.circle(
                    to_screen(center_xz.x, center_xz.y),
                    radius * scale,
                    obstacle_fill,
                    obstacle_stroke,
                );
            }
            ObstacleVolume::Aabb { min, max } => {
                let r = egui::Rect::from_two_pos(
                    to_screen(min.x, min.z),
                    to_screen(max.x, max.z),
                );
                painter.rect(r, 2.0, obstacle_fill, obstacle_stroke, egui::StrokeKind::Inside);
            }
        }
    }
}

// ============================================================================
// Live arena preview (render-to-texture)
//
// A dedicated offscreen camera renders the real arena meshes (floor + walls +
// the selected map's obstacles) to an image from a fixed 3/4 top-down angle;
// the image is registered with egui and drawn into the preview pane. The
// scene lives on its own `RenderLayers` so it never leaks into any other
// camera, and it is torn down when the state exits. This is graphical-only —
// the egui snapshot harness has no Bevy renderer, so it keeps the vector
// schematic fallback in `draw_map_preview`.
// ============================================================================

/// Isolated render layer for the preview scene (camera + light + meshes),
/// so it is invisible to every window-facing camera and vice versa.
const PREVIEW_LAYER: usize = 3;
/// Offscreen render-target size. Aspect matches the arena floor (x:z) so the
/// arena fills the frame without letterboxing.
const PREVIEW_TEX_W: u32 = 640;
const PREVIEW_TEX_H: u32 = 388;

/// Marks every entity in the offscreen preview scene (camera, light, meshes)
/// for teardown on state exit.
#[derive(Component)]
pub struct PreviewSceneEntity;

/// Marks just the arena-environment meshes, which are rebuilt when the
/// selected map changes (the camera and light persist across map switches).
#[derive(Component)]
pub struct PreviewEnvEntity;

/// Handle + egui texture id for the live arena preview render target, plus the
/// map currently rendered into it (so the UI only shows the texture once it
/// matches the selection, and the env is rebuilt when the selection changes).
#[derive(Resource)]
pub struct MapPreview {
    image: Handle<Image>,
    texture_id: egui::TextureId,
    rendered_map: ArenaMap,
}

/// Fixed 3/4 top-down camera framing the whole arena (76 × 46 world units).
fn preview_camera_transform() -> Transform {
    Transform::from_xyz(0.0, 52.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y)
}

/// Spawn the arena environment for `map` onto the preview render layer, tagged
/// for rebuild. Reuses the same mesh builder the real match uses.
fn spawn_preview_environment(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    map: ArenaMap,
    map_geometry: &MapGeometryConfig,
) {
    let active = map_geometry.active_for(map);
    for entity in spawn_arena_environment(commands, meshes, materials, images, &active.volumes) {
        commands
            .entity(entity)
            .insert((RenderLayers::layer(PREVIEW_LAYER), PreviewEnvEntity, PreviewSceneEntity));
    }
}

/// `OnEnter(ConfigureMatch)`: build the offscreen render target, camera, light,
/// and arena environment, then register the texture with egui.
pub fn setup_map_preview(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<MatchConfig>,
    map_geometry: Res<MapGeometryConfig>,
) {
    // Offscreen render-target image.
    let size = Extent3d {
        width: PREVIEW_TEX_W,
        height: PREVIEW_TEX_H,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image_handle = images.add(image);
    let texture_id = contexts.add_image(image_handle.clone());

    // Preview camera: renders only PREVIEW_LAYER into the image. `order` is
    // negative so it resolves before any window camera; its target is the
    // image, so it never composites to the window.
    commands.spawn((
        Camera3d::default(),
        Camera {
            target: RenderTarget::Image(image_handle.clone().into()),
            hdr: true,
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.05, 0.06, 0.09)),
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        preview_camera_transform(),
        RenderLayers::layer(PREVIEW_LAYER),
        PreviewSceneEntity,
    ));

    // Warm key light on the preview layer (the match sun is on layer 0).
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.85),
            illuminance: 12000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(25.0, 45.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(PREVIEW_LAYER),
        PreviewSceneEntity,
    ));

    // Ambient fill so the shadowed sides aren't crushed (removed on exit).
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.9, 0.85, 0.7),
        brightness: 300.0,
        affects_lightmapped_meshes: true,
    });

    spawn_preview_environment(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        config.map,
        &map_geometry,
    );

    commands.insert_resource(MapPreview {
        image: image_handle,
        texture_id,
        rendered_map: config.map,
    });
}

/// `Update` while in ConfigureMatch: when the selected map changes, rebuild the
/// preview environment meshes for the new map.
pub fn update_map_preview(
    mut commands: Commands,
    config: Res<MatchConfig>,
    map_geometry: Res<MapGeometryConfig>,
    map_preview: Option<ResMut<MapPreview>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    env_entities: Query<Entity, With<PreviewEnvEntity>>,
) {
    let Some(mut preview) = map_preview else { return };
    if preview.rendered_map == config.map {
        return;
    }

    for entity in &env_entities {
        commands.entity(entity).despawn();
    }
    spawn_preview_environment(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        config.map,
        &map_geometry,
    );
    preview.rendered_map = config.map;
}

/// `OnExit(ConfigureMatch)`: tear the preview scene down and drop its resources.
pub fn cleanup_map_preview(
    mut commands: Commands,
    mut contexts: EguiContexts,
    map_preview: Option<Res<MapPreview>>,
    scene_entities: Query<Entity, With<PreviewSceneEntity>>,
) {
    for entity in &scene_entities {
        commands.entity(entity).despawn();
    }
    if let Some(preview) = map_preview {
        contexts.remove_image(&preview.image);
    }
    commands.remove_resource::<MapPreview>();
    commands.remove_resource::<AmbientLight>();
}

