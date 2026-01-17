//! Combat Log and Timeline Rendering
//!
//! The tabbed combat panel showing combat log events and ability timeline.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::components::{CombatPanelView, SpellIcons};

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

// ==============================================================================
// Combat Panel
// ==============================================================================

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

// ==============================================================================
// Helper Functions
// ==============================================================================

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
