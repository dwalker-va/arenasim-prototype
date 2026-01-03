//! Game state management
//!
//! Defines the core game states and transitions between them.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

pub mod match_config;

pub use match_config::MatchConfig;

/// The core game states representing the main screens/modes of the game.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Main menu - entry point, navigate to other states
    #[default]
    MainMenu,
    /// Options menu - video/audio settings
    Options,
    /// Match configuration - team setup, map selection
    ConfigureMatch,
    /// Active match - the autobattle simulation
    PlayMatch,
    /// Post-match results - statistics and breakdown
    Results,
}

/// Plugin for managing game states and transitions
pub struct StatesPlugin;

impl Plugin for StatesPlugin {
    fn build(&self, app: &mut App) {
        app
            // Initialize match config resource
            .init_resource::<MatchConfig>()
            // Main menu systems (now using egui)
            .add_systems(
                Update,
                main_menu_ui.run_if(in_state(GameState::MainMenu)),
            )
            // Options menu systems (now using egui)
            .add_systems(
                Update,
                options_ui.run_if(in_state(GameState::Options)),
            )
            // Configure match systems (now using egui)
            .add_systems(
                Update,
                configure_match_ui.run_if(in_state(GameState::ConfigureMatch)),
            )
            // Play match systems
            .add_systems(OnEnter(GameState::PlayMatch), setup_play_match)
            .add_systems(OnExit(GameState::PlayMatch), cleanup_play_match)
            // Results systems
            .add_systems(OnEnter(GameState::Results), setup_results)
            .add_systems(OnExit(GameState::Results), cleanup_results);
    }
}

/// Marker component for play match entities
#[derive(Component)]
pub struct PlayMatchEntity;

/// Marker component for results entities
#[derive(Component)]
pub struct ResultsEntity;

// ============================================================================
// Main Menu (egui)
// ============================================================================

fn main_menu_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit_events: EventWriter<AppExit>,
) {
    let ctx = contexts.ctx_mut();
    
    // Configure style for a dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    ctx.set_style(style);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(egui::Color32::from_rgb(20, 20, 30)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(150.0);

                // Title
                ui.heading(
                    egui::RichText::new("ARENASIM")
                        .size(72.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );

                ui.add_space(10.0);

                // Subtitle
                ui.label(
                    egui::RichText::new("Arena Combat Autobattler")
                        .size(24.0)
                        .color(egui::Color32::from_rgb(153, 140, 128)),
                );

                ui.add_space(60.0);

                // Menu buttons
                let button_size = egui::vec2(280.0, 60.0);

                if ui
                    .add_sized(
                        button_size,
                        egui::Button::new(
                            egui::RichText::new("MATCH")
                                .size(28.0)
                                .color(egui::Color32::from_rgb(230, 217, 191)),
                        ),
                    )
                    .clicked()
                {
                    info!("Match button pressed - transitioning to ConfigureMatch");
                    next_state.set(GameState::ConfigureMatch);
                }

                ui.add_space(10.0);

                if ui
                    .add_sized(
                        button_size,
                        egui::Button::new(
                            egui::RichText::new("OPTIONS")
                                .size(28.0)
                                .color(egui::Color32::from_rgb(230, 217, 191)),
                        ),
                    )
                    .clicked()
                {
                    info!("Options button pressed - transitioning to Options");
                    next_state.set(GameState::Options);
                }

                ui.add_space(10.0);

                if ui
                    .add_sized(
                        button_size,
                        egui::Button::new(
                            egui::RichText::new("EXIT")
                                .size(28.0)
                                .color(egui::Color32::from_rgb(230, 217, 191)),
                        ),
                    )
                    .clicked()
                {
                    info!("Exit button pressed - quitting application");
                    exit_events.send(AppExit::Success);
                }
            });

            // Version text in bottom right
            ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("v0.1.0 - Prototype")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(102, 102, 102)),
                    );
                });
            });
        });
}

// ============================================================================
// Options Menu (egui)
// ============================================================================

fn options_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut settings: ResMut<crate::settings::GameSettings>,
    pending_restart: Res<crate::settings::PendingSettingsRestart>,
) {
    let ctx = contexts.ctx_mut();
    
    // Configure style for a dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    ctx.set_style(style);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin {
                    left: 20.0,
                    right: 20.0,
                    top: 20.0,
                    bottom: 20.0,
                })
        )
        .show(ctx, |ui| {
            ui.add_space(10.0);
            
            // Header with perfectly centered title
            ui.horizontal(|ui| {
                // Left column: Back button (fixed width)
                ui.allocate_ui_with_layout(
                    egui::vec2(100.0, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        if ui.button(egui::RichText::new("← BACK").size(20.0)).clicked() {
                            next_state.set(GameState::MainMenu);
                        }
                    }
                );
                
                // Middle column: Title (takes remaining space, centered)
                ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                    ui.heading(
                        egui::RichText::new("OPTIONS")
                            .size(42.0)
                            .color(egui::Color32::from_rgb(230, 204, 153)),
                    );
                });
                
                // Right column: Empty spacer (same width as left to balance)
                ui.allocate_space(egui::vec2(100.0, 40.0));
            });

            ui.add_space(60.0);

            // Center the options panel
            ui.vertical_centered(|ui| {
                // Create a fixed-width panel for options
                ui.allocate_ui_with_layout(
                    egui::vec2(600.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        // Window Mode Setting
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);
                            
                            ui.label(
                                egui::RichText::new("Window Mode")
                                    .size(24.0)
                                    .color(egui::Color32::from_rgb(230, 204, 153)),
                            );
                            
                            ui.add_space(5.0);
                            
                            ui.label(
                                egui::RichText::new("(Requires restart)")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );
                            
                            ui.add_space(10.0);
                            
                            ui.horizontal(|ui| {
                                for mode in crate::settings::WindowModeOption::all() {
                                    let is_selected = settings.window_mode == mode;
                                    let button = egui::Button::new(
                                        egui::RichText::new(mode.as_str())
                                            .size(18.0)
                                            .color(if is_selected {
                                                egui::Color32::from_rgb(255, 255, 255)
                                            } else {
                                                egui::Color32::from_rgb(180, 180, 180)
                                            })
                                    )
                                    .min_size(egui::vec2(280.0, 40.0))
                                    .fill(if is_selected {
                                        egui::Color32::from_rgb(60, 60, 80)
                                    } else {
                                        egui::Color32::from_rgb(40, 40, 50)
                                    });

                                    if ui.add(button).clicked() {
                                        settings.window_mode = mode;
                                    }
                                }
                            });
                            
                            ui.add_space(10.0);
                        });

                        ui.add_space(20.0);

                        // Resolution Setting
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);
                            
                            ui.label(
                                egui::RichText::new("Resolution")
                                    .size(24.0)
                                    .color(egui::Color32::from_rgb(230, 204, 153)),
                            );
                            
                            ui.add_space(5.0);
                            
                            ui.label(
                                egui::RichText::new("(Requires restart • Only applies in Windowed mode)")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );
                            
                            ui.add_space(10.0);
                            
                            ui.horizontal(|ui| {
                                for resolution in crate::settings::ResolutionOption::all() {
                                    let is_selected = settings.resolution == resolution;
                                    let button = egui::Button::new(
                                        egui::RichText::new(resolution.as_str())
                                            .size(18.0)
                                            .color(if is_selected {
                                                egui::Color32::from_rgb(255, 255, 255)
                                            } else {
                                                egui::Color32::from_rgb(180, 180, 180)
                                            })
                                    )
                                    .min_size(egui::vec2(180.0, 40.0))
                                    .fill(if is_selected {
                                        egui::Color32::from_rgb(60, 60, 80)
                                    } else {
                                        egui::Color32::from_rgb(40, 40, 50)
                                    });

                                    if ui.add(button).clicked() {
                                        settings.resolution = resolution;
                                    }
                                }
                            });
                            
                            ui.add_space(10.0);
                        });

                        ui.add_space(20.0);

                        // VSync Setting
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);
                            
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("VSync")
                                        .size(24.0)
                                        .color(egui::Color32::from_rgb(230, 204, 153)),
                                );
                                
                                ui.add_space(20.0);
                                
                                // Toggle switch
                                let vsync_label = if settings.vsync { "On" } else { "Off" };
                                if ui.add(
                                    egui::widgets::Checkbox::new(
                                        &mut settings.vsync,
                                        egui::RichText::new(vsync_label)
                                            .size(18.0)
                                    )
                                ).changed() {
                                    info!("VSync toggled to: {}", settings.vsync);
                                }
                            });
                            
                            ui.add_space(5.0);
                            
                            ui.label(
                                egui::RichText::new("Prevents screen tearing but may reduce performance • Applied immediately")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );
                            
                            ui.add_space(10.0);
                        });

                        // Restart notification
                        if pending_restart.restart_required {
                            ui.add_space(30.0);
                            
                            ui.group(|ui| {
                                ui.set_min_width(580.0);
                                ui.add_space(10.0);
                                
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("⚠")
                                            .size(24.0)
                                            .color(egui::Color32::from_rgb(230, 170, 80)),
                                    );
                                    
                                    ui.add_space(10.0);
                                    
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new("Restart Required")
                                                .size(20.0)
                                                .color(egui::Color32::from_rgb(230, 170, 80)),
                                        );
                                        ui.label(
                                            egui::RichText::new("Settings will be applied when you restart the application")
                                                .size(14.0)
                                                .color(egui::Color32::from_rgb(180, 180, 180)),
                                        );
                                    });
                                });
                                
                                ui.add_space(10.0);
                            });
                        }
                    }
                );
            });
        });
}

// ============================================================================
// Configure Match (egui)
// ============================================================================

/// State for the character picker modal
#[derive(Resource, Default)]
struct CharacterPickerState {
    active: bool,
    team: u8,
    slot: usize,
}

fn configure_match_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<MatchConfig>,
    mut next_state: ResMut<NextState<GameState>>,
    mut picker_state: Option<ResMut<CharacterPickerState>>,
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
) {
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

    // Handle ESC key
    if keyboard.just_pressed(KeyCode::Escape) {
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

                    // Back button - positioned absolutely in top-left
                    let back_response = ui.allocate_ui_at_rect(
                        egui::Rect::from_min_size(
                            egui::pos2(20.0, 20.0),
                            egui::vec2(100.0, 40.0)
                        ),
                        |ui| {
                            ui.horizontal(|ui| {
                                if ui.button(egui::RichText::new("← BACK").size(20.0)).clicked() {
                                    next_state.set(GameState::MainMenu);
                                }
                            });
                        }
                    );

                    // Title - centered relative to full viewport
                    ui.vertical_centered(|ui| {
                        ui.heading(
                            egui::RichText::new("CONFIGURE MATCH")
                                .size(42.0)
                                .color(egui::Color32::from_rgb(230, 204, 153)),
                        );
                    });

                    ui.add_space(30.0);

                    // Main content area with 3 panels - use egui's screen rect
                    // This accounts for egui's coordinate system and scale factor correctly
                    let screen_width = ctx.screen_rect().width();
                    
                    // Account for CentralPanel margins (15px left + 15px right = 30px total)
                    // and spacing between columns (20px * 2 = 40px)
                    let margins_and_spacing = 30.0 + 40.0;
                    let content_width = screen_width - margins_and_spacing;
                    let col_width = content_width / 3.0;
                    
                    ui.horizontal(|ui| {
                        // Add spacing between columns
                        ui.spacing_mut().item_spacing.x = 20.0;
                        
                        // Reduce content width slightly to prevent overflow from borders/padding
                        let panel_width = col_width - 10.0;
                        
                        // Team 1 column
                        ui.vertical(|ui| {
                            ui.set_width(col_width);
                            ui.add_space(5.0);
                            render_team_panel(ui, &mut config, 1, &mut picker_state, panel_width);
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
                            render_team_panel(ui, &mut config, 2, &mut picker_state, panel_width);
                        });
                    });

                    ui.add_space(30.0);

                    // Start Match button - centered
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

    // Character picker modal
    if let Some(ref mut picker) = picker_state {
        if picker.active {
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

                        // Make entire character option a clickable button
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 70.0),
                            egui::Sense::click()
                        );

                        // Background color with hover effect
                        let bg_color = if response.hovered() {
                            egui::Color32::from_rgb(64, 77, 89)
                        } else {
                            egui::Color32::from_rgb(51, 51, 64)
                        };

                        // Draw background
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
                        ui.painter().rect_filled(icon_rect, 6.0, color32.gamma_multiply(0.3));
                        ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32));

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

                        // Handle click
                        if response.clicked() {
                            // Assign character
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
    }
}

fn render_team_panel(
    ui: &mut egui::Ui,
    config: &mut MatchConfig,
    team: u8,
    picker_state: &mut Option<ResMut<CharacterPickerState>>,
    max_width: f32, // Explicit max width passed from parent
) {
    let team_color = if team == 1 {
        egui::Color32::from_rgb(51, 102, 204)
    } else {
        egui::Color32::from_rgb(204, 51, 51)
    };

    // Extract values we need before the frame closure
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

    // Use ALL available width in the column
    let content_width = max_width;
    
    // Header
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(format!("TEAM {}", team)).size(20.0).color(team_color));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Plus button
                    if ui.add(egui::Button::new("+").min_size(egui::vec2(25.0, 25.0))).clicked() && team_size < 3 {
                        if team == 1 {
                            config.set_team1_size(team_size + 1);
                        } else {
                            config.set_team2_size(team_size + 1);
                        }
                    }

                    ui.label(egui::RichText::new(format!("{}", team_size)).size(18.0));

                    // Minus button
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

    // Character slots
    for slot in 0..3 {
        let character = team_slots.get(slot).and_then(|c| *c);
        let is_active = slot < team_size;

        render_character_slot(ui, team, slot, character, is_active, team_color, picker_state, content_width);
        
        if slot < 2 {
            ui.add_space(12.0);
        }
    }
}

fn render_character_slot(
    ui: &mut egui::Ui,
    team: u8,
    slot: usize,
    character: Option<match_config::CharacterClass>,
    is_active: bool,
    team_color: egui::Color32,
    picker_state: &mut Option<ResMut<CharacterPickerState>>,
    max_width: f32, // Explicit max width passed from parent
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

    // Use the provided max_width for slots (already accounts for padding)
    let slot_width = max_width.max(50.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(slot_width, 60.0),
        if is_active { egui::Sense::click() } else { egui::Sense::hover() }
    );

    // Highlight on hover if active
    let visual_bg_color = if is_active && response.hovered() {
        bg_color.linear_multiply(1.2) // Lighten on hover
    } else {
        bg_color
    };

    // Draw the background frame
    ui.painter().rect_filled(rect, 8.0, visual_bg_color);
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(2.0, team_color.gamma_multiply(border_alpha))
    );

    // Render content on top of the allocated rect
    let content_rect = rect.shrink(12.0);
    let mut content_pos = content_rect.left_top();
    content_pos.x += 12.0; // Add left padding
    content_pos.y = content_rect.center().y; // Vertically center

    if let Some(class) = character {
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
        ui.painter().rect_filled(icon_rect, 6.0, color32.gamma_multiply(0.3));
        ui.painter().rect_stroke(icon_rect, 6.0, egui::Stroke::new(2.0, color32));

        // Class info text
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
        ui.painter().text(
            content_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Click to select character",
            egui::FontId::proportional(18.0),
            egui::Color32::from_rgb(128, 128, 128),
        );
    } else {
        ui.painter().text(
            content_rect.center(),
            egui::Align2::CENTER_CENTER,
            "—",
            egui::FontId::proportional(18.0),
            egui::Color32::from_rgb(77, 77, 77),
        );
    }

    // Handle click
    if is_active && response.clicked() {
        if let Some(ref mut picker) = picker_state {
            picker.active = true;
            picker.team = team;
            picker.slot = slot;
        }
    }
}

fn render_map_panel(ui: &mut egui::Ui, config: &mut MatchConfig, max_width: f32) {
    // Use ALL available width in the column
    let content_width = max_width;
    
    ui.vertical_centered(|ui| {
        ui.heading(
            egui::RichText::new("ARENA")
                .size(20.0)
                .color(egui::Color32::from_rgb(230, 204, 153)),
        );

        ui.add_space(20.0);

        // Map preview placeholder - centered
        let preview_width = (content_width * 0.8).min(180.0);
        let preview_height = preview_width * 0.75; // Maintain aspect ratio
        
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
        
        // Map selection controls - aligned to match preview width
        // Calculate layout: we want controls to span the same width as preview
        let button_width = 25.0; // Approximate button width
        let spacing = 8.0;
        let label_width = preview_width - (button_width * 2.0) - (spacing * 2.0);
        
        ui.horizontal(|ui| {
            // Center the control group to align with preview
            let available = ui.available_width();
            let controls_width = button_width + spacing + label_width + spacing + button_width;
            let padding = ((available - controls_width) / 2.0).max(0.0);
            
            ui.add_space(padding);
            
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
            
            // Fixed-width label container - text centered within
            ui.allocate_ui_with_layout(
                egui::vec2(label_width, ui.spacing().interact_size.y),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    ui.label(egui::RichText::new(config.map.name()).size(16.0));
                },
            );
            
            ui.add_space(spacing);

            if ui.button("▶").clicked() {
                let maps = match_config::ArenaMap::all();
                let current_idx = maps.iter().position(|m| *m == config.map).unwrap_or(0);
                let new_idx = (current_idx + 1) % maps.len();
                config.map = maps[new_idx];
            }
        });

        ui.add_space(12.0);

        ui.label(
            egui::RichText::new(config.map.description())
                .size(12.0)
                .color(egui::Color32::from_rgb(153, 153, 153)),
        );

        ui.add_space(30.0);
        
        // VS text
        ui.heading(
            egui::RichText::new("VS")
                .size(36.0)
                .color(egui::Color32::from_rgb(128, 115, 102)),
        );
    });
}

// ============================================================================
// Play Match (placeholder)
// ============================================================================

fn setup_play_match(mut commands: Commands) {
    info!("Entering PlayMatch state");

    // Spawn 2D camera for UI (temporary - will be 3D later)
    commands.spawn((Camera2d::default(), PlayMatchEntity));

    // Placeholder UI
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.08, 0.12)),
            PlayMatchEntity,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Match In Progress"),
                TextFont {
                    font_size: 48.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.8, 0.6)),
            ));

            parent.spawn((
                Text::new("Combat simulation coming soon..."),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.55, 0.5)),
                Node {
                    margin: UiRect::top(Val::Px(20.0)),
                    ..default()
                },
            ));

            parent.spawn((
                Text::new("Press ESC to return to menu"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.5, 0.5, 0.5)),
                Node {
                    margin: UiRect::top(Val::Px(40.0)),
                    ..default()
                },
            ));
        });
}

fn cleanup_play_match(mut commands: Commands, query: Query<Entity, With<PlayMatchEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

// ============================================================================
// Results (placeholder)
// ============================================================================

fn setup_results(mut _commands: Commands) {
    info!("Entering Results state");
}

fn cleanup_results(mut commands: Commands, query: Query<Entity, With<ResultsEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}
