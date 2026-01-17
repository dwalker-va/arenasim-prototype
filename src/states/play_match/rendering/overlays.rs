//! Screen Overlay Systems
//!
//! Full-screen overlays for countdown and victory celebration.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::states::play_match::components::{MatchCountdown, VictoryCelebration};
use super::draw_text_with_outline;

// ==============================================================================
// Countdown Overlay
// ==============================================================================

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
        2.0,
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
        2.0,
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
        1.5,
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
        2.0,
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
            1.5,
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
        2.0,
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
        2.0,
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
            1.5,
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
        1.5,
    );
}

// ==============================================================================
// Victory Celebration Overlay
// ==============================================================================

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
        egui::Align2::CENTER_CENTER,
        3.0,
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
            egui::Align2::CENTER_CENTER,
            2.0,
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
        egui::Align2::CENTER_CENTER,
        1.5,
    );
}
