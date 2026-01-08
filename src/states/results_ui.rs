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
use crate::combat::log::CombatLog;

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
    combat_log: Res<CombatLog>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    let Some(ctx) = contexts.try_ctx_mut() else { return; };
    
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
                            1,
                            &results.team1_combatants,
                            &combat_log,
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
                            2,
                            &results.team2_combatants,
                            &combat_log,
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
/// - Expandable combatant rows with stats and ability breakdown
fn render_team_stats(
    ui: &mut egui::Ui,
    title: &str,
    team: u8,
    combatants: &[CombatantStats],
    combat_log: &CombatLog,
    color: egui::Color32,
) {
    ui.group(|ui| {
        ui.set_min_width(380.0);

        // Team title
        ui.heading(egui::RichText::new(title).size(20.0).color(color));
        ui.add_space(10.0);

        // Combatant rows
        for stats in combatants {
            render_expandable_combatant_row(ui, stats, team, combat_log);
            ui.add_space(4.0);
        }
    });
}

/// Generate combatant ID for looking up combat log data.
fn get_combatant_id(team: u8, stats: &CombatantStats) -> String {
    format!("Team {} {}", team, stats.class.name())
}

/// Render a stat pill (small colored box with label and value).
fn render_stat_pill(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(10.0).color(egui::Color32::from_rgb(150, 150, 150)));
        ui.label(egui::RichText::new(value).size(12.0).color(color).strong());
    });
}

/// Render an expandable combatant card with stats and ability breakdown.
fn render_expandable_combatant_row(ui: &mut egui::Ui, stats: &CombatantStats, team: u8, combat_log: &CombatLog) {
    let combatant_id = get_combatant_id(team, stats);

    // Get class color
    let class_color = stats.class.color();
    let egui_class_color = egui::Color32::from_rgb(
        (class_color.to_srgba().red * 255.0) as u8,
        (class_color.to_srgba().green * 255.0) as u8,
        (class_color.to_srgba().blue * 255.0) as u8,
    );

    // Get combat log stats
    let kills = combat_log.killing_blows(&combatant_id);
    let cc_time = combat_log.cc_done_seconds(&combatant_id);

    // Card frame
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(30, 30, 40))
        .rounding(4.0)
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            // Header row: Class name + Status badge
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(stats.class.name()).size(16.0).color(egui_class_color).strong());
                ui.add_space(10.0);

                // Status badge
                let (status_text, status_bg) = if stats.survived {
                    ("ALIVE", egui::Color32::from_rgb(40, 80, 40))
                } else {
                    ("DEAD", egui::Color32::from_rgb(80, 40, 40))
                };
                egui::Frame::none()
                    .fill(status_bg)
                    .rounding(3.0)
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(status_text).size(10.0).color(egui::Color32::WHITE));
                    });
            });

            ui.add_space(8.0);

            // Stats row
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                render_stat_pill(ui, "DMG", &format!("{:.0}", stats.damage_dealt), egui::Color32::from_rgb(255, 150, 100));
                render_stat_pill(ui, "TAKEN", &format!("{:.0}", stats.damage_taken), egui::Color32::from_rgb(255, 100, 100));
                render_stat_pill(ui, "HEAL", &format!("{:.0}", stats.healing_done), egui::Color32::from_rgb(100, 255, 100));
                render_stat_pill(ui, "KILLS", &format!("{}", kills), egui::Color32::from_rgb(255, 215, 0));
                if cc_time > 0.0 {
                    render_stat_pill(ui, "CC", &format!("{:.1}s", cc_time), egui::Color32::from_rgb(180, 100, 255));
                }
            });

            ui.add_space(6.0);

            // Expandable details section
            egui::CollapsingHeader::new(
                egui::RichText::new("Ability Details").size(11.0).color(egui::Color32::from_rgb(150, 150, 170))
            )
            .id_salt(&combatant_id)
            .show(ui, |ui| {
                ui.add_space(4.0);

                // Damage breakdown with bars
                let damage_by_ability = combat_log.damage_by_ability(&combatant_id);
                if !damage_by_ability.is_empty() {
                    ui.label(egui::RichText::new("Damage").size(10.0).color(egui::Color32::from_rgb(255, 180, 100)));
                    ui.add_space(2.0);

                    let mut damage_vec: Vec<_> = damage_by_ability.iter().collect();
                    damage_vec.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let total_damage: f32 = damage_vec.iter().map(|(_, &d)| d).sum();

                    for (rank, (ability, &damage)) in damage_vec.iter().take(5).enumerate() {
                        render_ability_bar(
                            ui,
                            rank + 1,
                            ability,
                            damage,
                            total_damage,
                            egui::Color32::from_rgb(180, 100, 40),
                            egui::Color32::from_rgb(255, 180, 100),
                        );
                    }
                }

                // Healing breakdown with bars
                let healing_by_ability = combat_log.healing_by_ability(&combatant_id);
                if !healing_by_ability.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Healing").size(10.0).color(egui::Color32::from_rgb(100, 255, 100)));
                    ui.add_space(2.0);

                    let mut healing_vec: Vec<_> = healing_by_ability.iter().collect();
                    healing_vec.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let total_healing: f32 = healing_vec.iter().map(|(_, &h)| h).sum();

                    for (rank, (ability, &healing)) in healing_vec.iter().take(5).enumerate() {
                        render_ability_bar(
                            ui,
                            rank + 1,
                            ability,
                            healing,
                            total_healing,
                            egui::Color32::from_rgb(40, 130, 40),
                            egui::Color32::from_rgb(100, 255, 100),
                        );
                    }
                }

                // CC received
                let cc_received = combat_log.cc_received_seconds(&combatant_id);
                if cc_received > 0.0 {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("CC Received: {:.1}s", cc_received))
                            .size(10.0)
                            .color(egui::Color32::from_rgb(255, 100, 100))
                    );
                }
            });
        });
}

/// Render a single ability bar with background fill showing relative contribution.
fn render_ability_bar(
    ui: &mut egui::Ui,
    rank: usize,
    ability: &str,
    amount: f32,
    total: f32,
    bar_color: egui::Color32,
    text_color: egui::Color32,
) {
    let percentage = if total > 0.0 { amount / total } else { 0.0 };
    let bar_width = ui.available_width().min(250.0);

    // Create a frame for the bar row
    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(bar_width, 18.0),
        egui::Sense::hover()
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        // Background (dark)
        painter.rect_filled(
            rect,
            2.0,
            egui::Color32::from_rgb(30, 30, 40)
        );

        // Filled portion (colored bar)
        let filled_rect = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * percentage, rect.height())
        );
        painter.rect_filled(
            filled_rect,
            2.0,
            bar_color.linear_multiply(0.6)
        );

        // Border
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 70))
        );

        // Rank number
        let rank_text = format!("{}.", rank);
        painter.text(
            rect.min + egui::vec2(4.0, 9.0),
            egui::Align2::LEFT_CENTER,
            &rank_text,
            egui::FontId::proportional(10.0),
            egui::Color32::from_rgb(150, 150, 150)
        );

        // Ability name
        painter.text(
            rect.min + egui::vec2(20.0, 9.0),
            egui::Align2::LEFT_CENTER,
            ability,
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(220, 220, 220)
        );

        // Amount and percentage on right
        let amount_text = format!("{:.0} ({:.1}%)", amount, percentage * 100.0);
        painter.text(
            rect.max - egui::vec2(4.0, 9.0),
            egui::Align2::RIGHT_CENTER,
            &amount_text,
            egui::FontId::proportional(10.0),
            text_color
        );
    }
}


