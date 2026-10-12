//! Combat Log and Timeline Rendering
//!
//! The tabbed combat panel showing combat log events and ability timeline.

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::configure_match_ui::ClassIcons;
use crate::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use crate::states::play_match::components::{CombatPanelView, DisplaySettings, SpellIcons};
use crate::ui::theme::{self, class_color, color, data, hud, school_color};
use crate::ui::widgets::{icon_tile, Tooltip};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

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
    class_icons: Res<ClassIcons>,
    abilities: Res<AbilityDefinitions>,
    display_settings: Res<DisplaySettings>,
) {
    // Hidden by default: the log/timeline is a diagnostic tool, not part of
    // the default spectator presentation. Toggle with L (or the Log checkbox).
    if !display_settings.show_combat_panel {
        return;
    }

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    // Combat panel on the left side - semi-transparent to reduce obstruction
    egui::SidePanel::left("combat_panel")
        .default_width(320.0)
        .max_width(450.0)
        .min_width(280.0)
        .resizable(true)
        .show_separator_line(false)
        .frame(
            egui::Frame::side_top_panel(&ctx.style())
                .fill(hud::HUD_PANEL)
                .stroke(egui::Stroke::NONE),
        )
        .show(ctx, |ui| {
            // Tab bar
            ui.horizontal(|ui| {
                // Combat Log tab
                let log_selected = *panel_view == CombatPanelView::CombatLog;
                if ui
                    .selectable_label(
                        log_selected,
                        egui::RichText::new("Combat Log")
                            .size(theme::text::BODY)
                            .color(if log_selected {
                                hud::TAB_ACTIVE
                            } else {
                                hud::TAB_IDLE
                            }),
                    )
                    .clicked()
                {
                    *panel_view = CombatPanelView::CombatLog;
                }

                ui.add_space(hud::space::TAB);

                // Timeline tab
                let timeline_selected = *panel_view == CombatPanelView::Timeline;
                if ui
                    .selectable_label(
                        timeline_selected,
                        egui::RichText::new("Timeline")
                            .size(theme::text::BODY)
                            .color(if timeline_selected {
                                hud::TAB_ACTIVE
                            } else {
                                hud::TAB_IDLE
                            }),
                    )
                    .clicked()
                {
                    *panel_view = CombatPanelView::Timeline;
                }
            });

            ui.add_space(hud::space::RULE);
            ui.separator();
            ui.add_space(hud::space::RULE);

            // Render the selected view
            match *panel_view {
                CombatPanelView::CombatLog => render_combat_log_content(ui, &combat_log),
                CombatPanelView::Timeline => render_timeline_content(
                    ui,
                    &combat_log,
                    &TimelineIcons {
                        spells: &spell_icons,
                        classes: &class_icons,
                        abilities: &abilities,
                    },
                ),
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
                    CombatLogEventType::Damage => hud::LOG_DAMAGE,
                    CombatLogEventType::Healing => hud::LOG_HEALING,
                    CombatLogEventType::Buff => hud::LOG_BUFF,
                    CombatLogEventType::Death => hud::LOG_DEATH,
                    CombatLogEventType::MatchEvent => hud::LOG_MATCH_EVENT,
                    _ => hud::LOG_OTHER,
                };

                let timestamp_str = format!("[{:>5.1}s]", entry.timestamp);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&timestamp_str)
                            .size(hud::text::CAPTION)
                            .color(hud::LOG_TIMESTAMP),
                    );
                    ui.label(
                        egui::RichText::new(&entry.message)
                            .size(theme::text::SMALL)
                            .color(color),
                    );
                });
            }
        });
}

/// What the timeline draws its icons from.
struct TimelineIcons<'a> {
    spells: &'a SpellIcons,
    classes: &'a ClassIcons,
    abilities: &'a AbilityDefinitions,
}

/// The ability a timeline entry names, looked up in `abilities.ron`.
///
/// The combat log records a cast by the ability's display name. A few casts
/// log a variant of it — Holy Shock as "Holy Shock (Heal)" and
/// "Holy Shock (Damage)" — so a trailing parenthesised variant is dropped
/// before the lookup. `None` for a name no ability carries.
pub fn timeline_ability<'a>(
    logged: &str,
    abilities: &'a AbilityDefinitions,
) -> Option<&'a AbilityConfig> {
    let name = match logged.rfind(" (") {
        Some(i) if logged.ends_with(')') => &logged[..i],
        _ => logged,
    };
    abilities
        .iter()
        .map(|(_, config)| config)
        .find(|config| config.name == name)
}

/// The icon a timeline entry draws: its ability's `icon` in `abilities.ron`.
///
/// The in-match icon loader (`load_spell_icons`) registers every ability's
/// `icon` under the ability's name ([`super::ability_icon_paths`]), which is
/// the key the timeline reads the texture back by, so a new ability's casts
/// draw its icon with no code change.
pub fn timeline_icon_path<'a>(logged: &str, abilities: &'a AbilityDefinitions) -> Option<&'a str> {
    timeline_ability(logged, abilities)
        .map(|config| config.icon.as_str())
        .filter(|icon| !icon.is_empty())
}

/// Render the timeline content (columnar ability visualization).
fn render_timeline_content(ui: &mut egui::Ui, combat_log: &CombatLog, icons: &TimelineIcons) {
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
            ui.add_space(hud::space::EMPTY);
            ui.label(
                egui::RichText::new("Waiting for match to start...")
                    .size(theme::text::BODY)
                    .color(hud::HINT)
                    .italics(),
            );
        });
        return;
    }

    let num_combatants = combatants.len();
    let current_time = combat_log.match_time;
    // Add top padding so icons at t=0 are visible
    let timeline_height =
        TIMELINE_TOP_PADDING + (current_time * TIMELINE_PIXELS_PER_SECOND).max(200.0);

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
                data::TEAM_1
            } else {
                data::TEAM_2
            };

            ui.allocate_ui_with_layout(
                egui::vec2(combatant_column_width, 24.0),
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(
                        egui::RichText::new(short_name)
                            .size(theme::text::SMALL)
                            .color(team_color)
                            .strong(),
                    );
                },
            );
        }
    });

    ui.add_space(theme::space::XXS);
    ui.separator();
    ui.add_space(theme::space::XXS);

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
                egui::Sense::hover(),
            );

            let painter = ui.painter_at(rect);

            // Draw vertical column separator lines
            for i in 0..=num_combatants {
                let x =
                    rect.min.x + TIMELINE_TIME_COLUMN_WIDTH + (i as f32 * combatant_column_width);
                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                    egui::Stroke::new(1.0_f32, hud::TIMELINE_COLUMN_RULE),
                );
            }

            // Draw horizontal time tick lines and labels
            let mut t = 0.0;
            while t <= current_time {
                let y = rect.min.y + TIMELINE_TOP_PADDING + t * TIMELINE_PIXELS_PER_SECOND;

                // Horizontal line across all columns
                painter.line_segment(
                    [
                        egui::pos2(rect.min.x + TIMELINE_TIME_COLUMN_WIDTH, y),
                        egui::pos2(rect.max.x, y),
                    ],
                    egui::Stroke::new(1.0_f32, hud::TIMELINE_TICK_RULE),
                );

                // Time label on left
                painter.text(
                    egui::pos2(rect.min.x + 5.0, y + 2.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}s", t as u32),
                    egui::FontId::proportional(theme::text::MICRO),
                    hud::HINT,
                );

                t += TIMELINE_TIME_TICK_INTERVAL;
            }

            // Draw ability icons for each combatant
            let mut hovered_ability: Option<(String, f32, bool)> = None; // (ability_name, timestamp, interrupted)

            for (col_idx, combatant_id) in combatants.iter().enumerate() {
                let casts = combat_log.ability_casts_for(combatant_id);
                let col_center_x = rect.min.x
                    + TIMELINE_TIME_COLUMN_WIDTH
                    + (col_idx as f32 * combatant_column_width)
                    + (combatant_column_width / 2.0);

                // First pass: calculate base y positions and detect overlaps
                // We'll push overlapping icons down to avoid collision
                let mut icon_positions: Vec<(f32, f32, &str, bool)> = Vec::new(); // (timestamp, adjusted_y, ability_name, interrupted)

                for (timestamp, ability_name, interrupted) in &casts {
                    let base_y =
                        rect.min.y + TIMELINE_TOP_PADDING + timestamp * TIMELINE_PIXELS_PER_SECOND;

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
                        egui::vec2(TIMELINE_ICON_SIZE, TIMELINE_ICON_SIZE),
                    );

                    let ability = timeline_ability(ability_name, icons.abilities);
                    // Keyed by the ability's name, as `load_spell_icons`
                    // registered its `icon` (see `timeline_icon_path`).
                    let texture =
                        ability.and_then(|config| icons.spells.textures.get(&config.name));

                    // Border - red if interrupted, white otherwise
                    let border_color = if interrupted {
                        hud::TIMELINE_INTERRUPTED_EDGE
                    } else {
                        hud::TIMELINE_ICON_EDGE
                    };
                    let border_width: f32 = if interrupted { 2.0 } else { 1.0 };
                    painter.rect_stroke(
                        icon_rect.expand(1.0),
                        theme::radius::SM,
                        egui::Stroke::new(border_width, border_color),
                        egui::StrokeKind::Outside,
                    );

                    if let Some(texture_id) = texture {
                        // Draw the spell icon (tinted red if interrupted)
                        let icon_tint = if interrupted {
                            hud::TIMELINE_INTERRUPTED_TINT
                        } else {
                            color::UNTINTED
                        };
                        painter.image(
                            *texture_id,
                            icon_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            icon_tint,
                        );
                    } else {
                        // The spell's art is not loaded yet: what the kit's
                        // `class_icon` draws for the ability's class — painted
                        // rather than laid out, so the pointer still reaches
                        // the timeline's own hover — or the neutral tile for a
                        // name no ability carries.
                        match ability {
                            Some(config) => icon_tile(
                                &painter,
                                icon_rect,
                                icons.classes.textures.get(&config.class).copied(),
                                class_color(config.class),
                            ),
                            None => icon_tile(&painter, icon_rect, None, color::LINE),
                        }
                    }

                    // Check hover for tooltip
                    if let Some(hover_pos) = response.hover_pos() {
                        if icon_rect.contains(hover_pos) {
                            hovered_ability =
                                Some((ability_name.to_string(), timestamp, interrupted));
                        }
                    }
                }
            }

            // The shared hover card, at the pointer: one widget covers the
            // whole timeline, so the card follows the icon under the pointer.
            if let Some((ability_name, timestamp, interrupted)) = hovered_ability {
                let title_color = timeline_ability(&ability_name, icons.abilities)
                    .map_or(color::TEXT, |config| school_color(config.spell_school));
                let meta = if interrupted {
                    format!("{:.1}s · Interrupted", timestamp)
                } else {
                    format!("{:.1}s", timestamp)
                };
                response.on_hover_ui_at_pointer(|ui| {
                    Tooltip::new(&ability_name)
                        .title_color(title_color)
                        .meta(&meta)
                        .ui(ui)
                });
            }
        });
}

// ==============================================================================
// Helper Functions
// ==============================================================================

/// Shorten combatant name for compact display (e.g., "Team 1 Mage" -> "T1 Mag")
fn shorten_combatant_name(name: &str) -> String {
    let shortened = name.replace("Team 1 ", "T1 ").replace("Team 2 ", "T2 ");

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
