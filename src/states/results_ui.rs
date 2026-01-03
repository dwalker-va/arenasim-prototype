//! Results Scene UI - Post-Match Statistics
//!
//! This module displays match results after a battle concludes, showing:
//! - Winner announcement (or draw)
//! - Side-by-side team statistics
//! - Per-combatant breakdown (class, survival status, damage stats)
//! - Return to main menu button
//!
//! ## Data Source
//! Reads the `MatchResults` resource inserted by the combat system
//! when a match ends. This resource contains the winner and detailed
//! stats for each combatant on both teams.
//!
//! ## UI Structure
//! ```text
//! ┌─────────────────────────────────────┐
//! │         MATCH RESULTS               │
//! │         [TEAM X WINS!]              │
//! │                                     │
//! │  ┌─────────────┐  ┌─────────────┐  │
//! │  │  TEAM 1     │  │  TEAM 2     │  │
//! │  │  Stats...   │  │  Stats...   │  │
//! │  └─────────────┘  └─────────────┘  │
//! │                                     │
//! │           [DONE]                    │
//! └─────────────────────────────────────┘
//! ```

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use super::{GameState, play_match::{MatchResults, CombatantStats}};

/// Main UI system for the Results screen.
/// 
/// Displays:
/// - Title and winner announcement
/// - Two-column layout with team stats
/// - Done button to return to main menu
/// 
/// Cleans up the `MatchResults` resource when exiting.
pub fn results_ui(
    mut contexts: EguiContexts,
    results: Option<Res<MatchResults>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    let ctx = contexts.ctx_mut();
    
    // Configure dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    ctx.set_style(style);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin::same(20.0))
        )
        .show(ctx, |ui| {
            ui.add_space(20.0);

            // Title
            ui.vertical_centered(|ui| {
                ui.heading(
                    egui::RichText::new("MATCH RESULTS")
                        .size(48.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );
            });

            ui.add_space(30.0);

            // Check if results exist
            let Some(results) = results else {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No match results available")
                            .size(24.0)
                            .color(egui::Color32::from_rgb(200, 100, 100)),
                    );
                });
                return;
            };

            // Winner announcement
            ui.vertical_centered(|ui| {
                let (winner_text, winner_color) = get_winner_display(results.winner);
                
                ui.heading(
                    egui::RichText::new(winner_text)
                        .size(36.0)
                        .color(winner_color),
                );
            });

            ui.add_space(40.0);

            // Stats tables side-by-side - centered
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    let available_width = ui.available_width();
                    let spacing = 30.0;
                    let table_width = (available_width - spacing) / 2.0;

                    // Team 1 Stats
                    ui.vertical(|ui| {
                        ui.set_max_width(table_width);
                        render_team_stats(
                            ui,
                            "TEAM 1",
                            &results.team1_combatants,
                            egui::Color32::from_rgb(51, 102, 204)
                        );
                    });

                    ui.add_space(spacing);

                    // Team 2 Stats
                    ui.vertical(|ui| {
                        ui.set_max_width(table_width);
                        render_team_stats(
                            ui,
                            "TEAM 2",
                            &results.team2_combatants,
                            egui::Color32::from_rgb(204, 51, 51)
                        );
                    });
                });
            });

            ui.add_space(40.0);

            // Done button - returns to main menu and cleans up results
            ui.vertical_centered(|ui| {
                let button = egui::Button::new(
                    egui::RichText::new("DONE")
                        .size(24.0)
                        .color(egui::Color32::from_rgb(230, 242, 230)),
                )
                .min_size(egui::vec2(200.0, 50.0));

                if ui.add(button).clicked() {
                    commands.remove_resource::<MatchResults>();
                    next_state.set(GameState::MainMenu);
                }
            });

            ui.add_space(20.0);
        });
}

/// Get the winner announcement text and color.
/// 
/// Returns tuple of (text, color) based on match outcome:
/// - None: Draw (yellow)
/// - Some(1): Team 1 wins (blue)
/// - Some(2): Team 2 wins (red)
fn get_winner_display(winner: Option<u8>) -> (String, egui::Color32) {
    match winner {
        None => (
            "DRAW!".to_string(),
            egui::Color32::from_rgb(200, 200, 100), // Yellow
        ),
        Some(1) => (
            "TEAM 1 WINS!".to_string(),
            egui::Color32::from_rgb(100, 150, 255), // Blue
        ),
        Some(2) => (
            "TEAM 2 WINS!".to_string(),
            egui::Color32::from_rgb(255, 100, 100), // Red
        ),
        Some(_) => (
            "ERROR: Invalid winner".to_string(),
            egui::Color32::from_rgb(200, 100, 100),
        ),
    }
}

/// Render the stats table for one team.
/// 
/// Displays a grouped panel containing:
/// - Team name header
/// - Grid-based stats table with columns:
///   - Class (colored by class)
///   - Status (✓ survived / ✗ eliminated)
///   - Dmg Tkn - Damage Taken (red)
///   - Dmg Dlt - Damage Dealt (orange)
///   - Healing - Healing Done (green)
fn render_team_stats(
    ui: &mut egui::Ui,
    title: &str,
    combatants: &[CombatantStats],
    color: egui::Color32,
) {
    ui.group(|ui| {
        ui.set_min_height(250.0);
        
        // Team title
        ui.heading(egui::RichText::new(title).size(20.0).color(color));
        ui.add_space(10.0);

        // Stats table using egui::Grid for proper alignment
        let available = ui.available_width();
        egui::Grid::new(format!("{}_stats_grid", title))
            .striped(false)
            .spacing([15.0, 8.0]) // horizontal, vertical spacing
            .min_col_width(available * 0.16) // Each column gets ~16% of width (5 columns)
            .show(ui, |ui| {
                // Header row
                ui.label(egui::RichText::new("Class").size(13.0).strong());
                ui.label(egui::RichText::new("Status").size(13.0).strong());
                ui.label(egui::RichText::new("Dmg Tkn").size(13.0).strong());
                ui.label(egui::RichText::new("Dmg Dlt").size(13.0).strong());
                ui.label(egui::RichText::new("Healing").size(13.0).strong());
                ui.end_row();
                
                // Separator row
                ui.separator();
                ui.separator();
                ui.separator();
                ui.separator();
                ui.separator();
                ui.end_row();
                
                // Data rows - one per combatant
                for stats in combatants {
                    render_combatant_row(ui, stats);
                    ui.end_row();
                }
            });
    });
}

/// Render a single combatant's stats row.
/// 
/// Displays:
/// - Class name (colored, 15px)
/// - Survival status (✓ green or ✗ gray, 16px)
/// - Damage taken (red, 15px)
/// - Damage dealt (orange, 15px)
/// - Healing done (green, 15px)
fn render_combatant_row(ui: &mut egui::Ui, stats: &CombatantStats) {
    // Get class color for the name
    let class_color = stats.class.color();
    let egui_class_color = egui::Color32::from_rgb(
        (class_color.to_srgba().red * 255.0) as u8,
        (class_color.to_srgba().green * 255.0) as u8,
        (class_color.to_srgba().blue * 255.0) as u8,
    );
    
    // Class name (colored)
    ui.label(
        egui::RichText::new(stats.class.name())
            .size(15.0)
            .color(egui_class_color)
    );
    
    // Status (survived or eliminated)
    let (status_text, status_color) = if stats.survived {
        ("✓", egui::Color32::from_rgb(100, 255, 100)) // Green checkmark
    } else {
        ("✗", egui::Color32::from_rgb(150, 150, 150)) // Gray X
    };
    ui.label(egui::RichText::new(status_text).size(16.0).color(status_color));
    
    // Damage Taken (red)
    ui.label(
        egui::RichText::new(format!("{:.0}", stats.damage_taken))
            .size(15.0)
            .color(egui::Color32::from_rgb(255, 100, 100))
    );
    
    // Damage Dealt (orange)
    ui.label(
        egui::RichText::new(format!("{:.0}", stats.damage_dealt))
            .size(15.0)
            .color(egui::Color32::from_rgb(255, 150, 100))
    );
    
    // Healing Done (green)
    ui.label(
        egui::RichText::new(format!("{:.0}", stats.healing_done))
            .size(15.0)
            .color(egui::Color32::from_rgb(100, 255, 100))
    );
}

