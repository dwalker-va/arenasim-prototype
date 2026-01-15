//! Game state management
//!
//! Defines the core game states and transitions between them.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

pub mod match_config;
pub mod configure_match_ui;
pub mod play_match;
pub mod results_ui;

pub use match_config::MatchConfig;

/// The core game states representing the main screens/modes of the game.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Main menu - entry point, navigate to other states
    #[default]
    MainMenu,
    /// Options menu - video/audio settings
    Options,
    /// Keybindings menu - control remapping
    Keybindings,
    /// Match configuration - team setup, map selection
    ConfigureMatch,
    /// Active match - the autobattle simulation
    PlayMatch,
    /// Post-match results - statistics and breakdown
    Results,
}

/// System sets for PlayMatch state to ensure proper execution order.
/// ResourcesAndAuras must run before CombatAndMovement so that timer
/// decrements (like kiting_timer) are visible to movement systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
enum PlayMatchSystems {
    /// First phase: resource regeneration, DoT ticks, aura updates
    ResourcesAndAuras,
    /// Second phase: targeting, abilities, casting, projectiles, movement
    CombatAndMovement,
}

/// Plugin for managing game states and transitions
pub struct StatesPlugin;

impl Plugin for StatesPlugin {
    fn build(&self, app: &mut App) {
        app
            // Initialize match config resource
            .init_resource::<MatchConfig>()
            // Initialize class icon resources
            .init_resource::<configure_match_ui::ClassIcons>()
            .init_resource::<configure_match_ui::ClassIconHandles>()
            // Configure PlayMatch system sets ordering
            // ResourcesAndAuras must run before CombatAndMovement so that
            // kiting_timer decrements are visible to move_to_target
            .configure_sets(
                Update,
                PlayMatchSystems::CombatAndMovement.after(PlayMatchSystems::ResourcesAndAuras),
            )
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
            .add_systems(
                Update,
                keybindings_ui.run_if(in_state(GameState::Keybindings)),
            )
            // Configure match systems (defined in configure_match_ui module)
            .add_systems(
                Update,
                (
                    configure_match_ui::load_class_icons,
                    configure_match_ui::configure_match_ui,
                )
                    .chain()
                    .run_if(in_state(GameState::ConfigureMatch)),
            )
            // Play match systems (defined in play_match module)
            .add_systems(OnEnter(GameState::PlayMatch), play_match::setup_play_match)
            // Phase 1: Resources and Auras - includes kiting_timer decrement
            .add_systems(
                Update,
                (
                    play_match::handle_time_controls,
                    play_match::handle_camera_input,
                    play_match::update_camera_position,
                    play_match::update_countdown,
                    play_match::animate_gate_bars,
                    play_match::update_play_match,
                    play_match::regenerate_resources,
                    play_match::track_shadow_sight_timer,  // Track combat time and spawn Shadow Sight orbs
                    play_match::process_dot_ticks,  // Process DoT ticks BEFORE updating auras (so final tick fires on expiration)
                    play_match::update_auras,
                    play_match::apply_pending_auras,
                )
                    .chain()
                    .in_set(PlayMatchSystems::ResourcesAndAuras)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // Apply deferred commands (e.g., inserting ActiveAuras) before processing them
            // This runs after ResourcesAndAuras, before CombatAndMovement
            .add_systems(
                Update,
                apply_deferred
                    .after(PlayMatchSystems::ResourcesAndAuras)
                    .before(PlayMatchSystems::CombatAndMovement)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // Phase 2: Combat and Movement - uses kiting_timer to decide movement
            .add_systems(
                Update,
                (
                    play_match::process_aura_breaks,
                    play_match::acquire_targets,
                    play_match::check_orb_pickups,  // Check for Shadow Sight orb pickups
                    play_match::decide_abilities,
                    apply_deferred,  // Flush commands so CastingState is visible
                    play_match::check_interrupts,  // Check for interrupts after CastingState is visible
                    play_match::process_interrupts,
                    play_match::process_casting,
                    play_match::spawn_projectile_visuals,
                    play_match::move_projectiles,
                    play_match::process_projectile_hits,
                    play_match::move_to_target,
                )
                    .chain()
                    .in_set(PlayMatchSystems::CombatAndMovement)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // Combat resolution, death, and effects
            .add_systems(
                Update,
                (
                    play_match::update_stealth_visuals,
                    play_match::combat_auto_attack,
                    play_match::check_match_end,
                    play_match::trigger_death_animation,
                    play_match::animate_death,
                    play_match::update_victory_celebration,
                    play_match::update_floating_combat_text,
                    play_match::update_speech_bubbles,
                    play_match::cleanup_expired_floating_text,
                    play_match::spawn_spell_impact_visuals,
                    play_match::update_spell_impact_effects,
                    play_match::cleanup_expired_spell_impacts,
                    play_match::animate_shadow_sight_orbs,  // Pulsing orb animation
                )
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // UI rendering systems
            .add_systems(
                Update,
                (
                    play_match::render_time_controls,
                    play_match::render_camera_controls,
                    play_match::render_countdown,
                    play_match::render_victory_celebration,
                    play_match::render_health_bars,
                    play_match::render_floating_combat_text,
                    play_match::render_speech_bubbles,
                    play_match::render_combat_panel,
                    play_match::load_spell_icons,
                )
                    .run_if(in_state(GameState::PlayMatch)),
            )
            .add_systems(OnExit(GameState::PlayMatch), play_match::cleanup_play_match)
            // Results systems (defined in results_ui module)
            .add_systems(
                Update,
                results_ui::results_ui.run_if(in_state(GameState::Results)),
            );
    }
}

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
                    egui::RichText::new("OPTIONS")
                        .size(42.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );
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
                        
                        ui.add_space(20.0);
                        
                        // Controls / Keybindings button
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);
                            
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Controls")
                                        .size(24.0)
                                        .color(egui::Color32::from_rgb(230, 204, 153)),
                                );
                                
                                ui.add_space(20.0);
                                
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("Configure Keybindings")
                                            .size(18.0)
                                    )
                                    .min_size(egui::vec2(200.0, 36.0))
                                ).clicked() {
                                    next_state.set(GameState::Keybindings);
                                }
                            });
                            
                            ui.add_space(5.0);
                            
                            ui.label(
                                egui::RichText::new("Customize keyboard controls")
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

/// Resource to track keybinding that is currently being rebound
#[derive(Resource, Default)]
struct RebindingState {
    action: Option<crate::keybindings::GameAction>,
    is_primary: bool,
}

/// Keybindings configuration UI
fn keybindings_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut settings: ResMut<crate::settings::GameSettings>,
    mut rebinding_state: Local<Option<RebindingState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut keys_just_pressed: Local<Vec<KeyCode>>,
) {
    use crate::keybindings::{GameAction, Keybindings};
    
    // Initialize rebinding state if needed
    if rebinding_state.is_none() {
        *rebinding_state = Some(RebindingState::default());
    }
    
    // Collect all keys just pressed this frame (for rebinding)
    keys_just_pressed.clear();
    for key in keyboard.get_just_pressed() {
        keys_just_pressed.push(*key);
    }
    
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
            
            // Back button - positioned in top-left
            let back_rect = egui::Rect::from_min_size(
                egui::pos2(20.0, 20.0),
                egui::vec2(80.0, 36.0)
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(back_rect), |ui| {
                if ui.button(egui::RichText::new("← BACK").size(20.0)).clicked() {
                    next_state.set(GameState::Options);
                }
            });
            
            // Title - centered relative to full width
            ui.vertical_centered(|ui| {
                ui.heading(
                    egui::RichText::new("KEYBINDINGS")
                        .size(42.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );
            });

            ui.add_space(30.0);
            
            // Reset to defaults button
            ui.vertical_centered(|ui| {
                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("Reset to Defaults")
                            .size(16.0)
                    )
                    .min_size(egui::vec2(180.0, 32.0))
                ).clicked() {
                    settings.keybindings.reset_to_defaults();
                }
            });

            ui.add_space(20.0);

            // Center the keybindings panel
            ui.vertical_centered(|ui| {
                // Create a fixed-width panel for keybindings
                ui.allocate_ui_with_layout(
                    egui::vec2(800.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        // Group actions by category
                        let mut actions_by_category: std::collections::HashMap<&str, Vec<GameAction>> = 
                            std::collections::HashMap::new();
                        
                        for action in GameAction::all() {
                            actions_by_category
                                .entry(action.category())
                                .or_insert_with(Vec::new)
                                .push(action);
                        }
                        
                        // Render each category
                        let categories = vec!["Navigation", "Camera", "Simulation"];
                        for category in categories {
                            if let Some(actions) = actions_by_category.get(category) {
                                ui.group(|ui| {
                                    ui.set_min_width(780.0);
                                    ui.add_space(10.0);
                                    
                                    ui.label(
                                        egui::RichText::new(category)
                                            .size(28.0)
                                            .color(egui::Color32::from_rgb(230, 204, 153)),
                                    );
                                    
                                    ui.add_space(10.0);
                                    
                                    // Render each action in this category
                                    for action in actions {
                                        let rebinding = rebinding_state.as_ref()
                                            .and_then(|rs| rs.action)
                                            .map_or(false, |a| a == *action);
                                        
                                        ui.horizontal(|ui| {
                                            // Action name
                                            ui.label(
                                                egui::RichText::new(action.description())
                                                    .size(18.0)
                                                    .color(egui::Color32::from_rgb(200, 200, 200))
                                            );
                                            
                                            ui.add_space(20.0);
                                            
                                            // Spacer to push buttons to the right
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                // Secondary key button
                                                let binding = settings.keybindings.get(*action);
                                                let secondary_text = binding
                                                    .and_then(|b| b.secondary)
                                                    .map(|k| Keybindings::key_name(k).to_string())
                                                    .unwrap_or_else(|| "-".to_string());
                                                
                                                let secondary_rebinding = rebinding && 
                                                    !rebinding_state.as_ref().unwrap().is_primary;
                                                
                                                let secondary_button = egui::Button::new(
                                                    egui::RichText::new(if secondary_rebinding {
                                                        "Press key..."
                                                    } else {
                                                        &secondary_text
                                                    })
                                                    .size(16.0)
                                                    .color(if secondary_rebinding {
                                                        egui::Color32::from_rgb(255, 200, 100)
                                                    } else {
                                                        egui::Color32::from_rgb(180, 180, 180)
                                                    })
                                                )
                                                .min_size(egui::vec2(120.0, 32.0))
                                                .fill(if secondary_rebinding {
                                                    egui::Color32::from_rgb(80, 60, 40)
                                                } else {
                                                    egui::Color32::from_rgb(40, 40, 50)
                                                });
                                                
                                                if ui.add(secondary_button).clicked() {
                                                    if let Some(rs) = rebinding_state.as_mut() {
                                                        rs.action = Some(*action);
                                                        rs.is_primary = false;
                                                    }
                                                }
                                                
                                                ui.add_space(10.0);
                                                
                                                // Primary key button
                                                let primary_text = binding
                                                    .map(|b| Keybindings::key_name(b.primary).to_string())
                                                    .unwrap_or_else(|| "Unbound".to_string());
                                                
                                                let primary_rebinding = rebinding && 
                                                    rebinding_state.as_ref().unwrap().is_primary;
                                                
                                                let primary_button = egui::Button::new(
                                                    egui::RichText::new(if primary_rebinding {
                                                        "Press key..."
                                                    } else {
                                                        &primary_text
                                                    })
                                                    .size(16.0)
                                                    .color(if primary_rebinding {
                                                        egui::Color32::from_rgb(255, 200, 100)
                                                    } else {
                                                        egui::Color32::from_rgb(255, 255, 255)
                                                    })
                                                )
                                                .min_size(egui::vec2(120.0, 32.0))
                                                .fill(if primary_rebinding {
                                                    egui::Color32::from_rgb(80, 60, 40)
                                                } else {
                                                    egui::Color32::from_rgb(60, 60, 80)
                                                });
                                                
                                                if ui.add(primary_button).clicked() {
                                                    if let Some(rs) = rebinding_state.as_mut() {
                                                        rs.action = Some(*action);
                                                        rs.is_primary = true;
                                                    }
                                                }
                                            });
                                        });
                                        
                                        ui.add_space(8.0);
                                    }
                                    
                                    ui.add_space(10.0);
                                });
                                
                                ui.add_space(20.0);
                            }
                        }
                    }
                );
            });
        });
    
    // Handle key press for rebinding
    if let Some(ref mut rs) = rebinding_state.as_mut() {
        if let Some(action) = rs.action {
            if !keys_just_pressed.is_empty() {
                let new_key = keys_just_pressed[0];
                
                // Check for conflicts
                if let Some(conflicting_action) = settings.keybindings.is_key_bound(new_key, Some(action)) {
                    info!("Key {:?} is already bound to {:?}", new_key, conflicting_action);
                    // For now, just warn. In a full implementation, you'd show a conflict dialog
                }
                
                // Update the binding
                if let Some(mut binding) = settings.keybindings.get(action).cloned() {
                    if rs.is_primary {
                        binding.primary = new_key;
                    } else {
                        binding.secondary = Some(new_key);
                    }
                    settings.keybindings.set(action, binding);
                }
                
                // Clear rebinding state
                rs.action = None;
            }
        }
    }
}

// ============================================================================
// Configure Match UI
// ============================================================================
// All Configure Match logic has been moved to src/states/configure_match_ui.rs
// See that module for team setup, character selection, and map controls.

// ============================================================================
// Play Match - 3D Combat Arena
// ============================================================================
// All Play Match logic has been moved to src/states/play_match.rs
// See that module for combat systems, combatant components, and match flow.

// ============================================================================
// Results Scene UI
// ============================================================================
// All Results screen logic has been moved to src/states/results_ui.rs
// See that module for match results display and statistics tables.
