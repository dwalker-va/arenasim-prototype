//! Results Scene UI - Post-Match Statistics
//!
//! Displays match results after a battle concludes:
//! - Compact winner banner (victor color, match duration)
//! - Two aligned, face-off team panels (loser panel dimmed)
//! - Per-combatant rows with class icon, aligned stat columns, a relative
//!   damage mini-bar, survival tag, and a click-to-expand ability breakdown
//! - Team Σ TOTAL subtotal row
//! - Return-to-menu button
//!
//! ## Data Source
//! Reads the `MatchResults` resource inserted at match end (winner, duration,
//! per-combatant `CombatantStats`) plus the `CombatLog` for per-ability
//! damage/healing, killing blows, and CC time. Class icons come from the
//! shared `ClassIcons` egui-texture resource loaded in ConfigureMatch.
//!
//! ## UI Structure
//! ```text
//! ┌────────────────────────────────────────────────────┐
//! │  ★ TEAM 1 VICTORY                       ⏱ 1:47       │
//! ├──────────────────────────┬─────────────────────────┤
//! │ TEAM 1         ★ WINNER   │ TEAM 2       (defeated)  │
//! │ CLASS    DMG HEAL TKN  K  │ CLASS   DMG HEAL TKN  K  │
//! │ ▌🛡Warrior 8.4k  –  3.1k 1 │ ▌❄Mage  4.2k  – 9.0k  0  │
//! │   ▓▓▓▓▓▓▓▓▓▓▓▓▓     ALIVE │   ▓▓▓▓▓             DEAD │
//! │ Σ TOTAL  9.6k 6.8k 5.5k 1 │ Σ TOTAL 5.1k 5.1k15.2k 0 │
//! └──────────────────────────┴─────────────────────────┘
//! ```

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use super::{GameState, play_match::{MatchResults, CombatantStats}};
use super::configure_match_ui::ClassIcons;
use super::match_config::CharacterClass;
use crate::combat::log::CombatLog;

// --- Layout constants (fixed widths keep numeric columns aligned across the
//     header, every combatant row, and the Σ TOTAL row) ---
const W_NAME: f32 = 116.0; // accent stripe + icon + class name
const W_DMG: f32 = 52.0;
const W_HEAL: f32 = 52.0;
const W_TKN: f32 = 52.0;
const W_K: f32 = 26.0;
const ROW_HEIGHT: f32 = 22.0;
/// Gap between the right-aligned stat columns.
const STAT_GAP: f32 = 12.0;

// --- Palette ---
const BG: egui::Color32 = egui::Color32::from_rgb(20, 20, 30);
const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(28, 28, 40);
const HEADER_GREY: egui::Color32 = egui::Color32::from_rgb(140, 140, 158);
const DIVIDER: egui::Color32 = egui::Color32::from_rgb(70, 70, 88);
const C_DMG: egui::Color32 = egui::Color32::from_rgb(255, 150, 100);
const C_HEAL: egui::Color32 = egui::Color32::from_rgb(110, 215, 130);
const C_TKN: egui::Color32 = egui::Color32::from_rgb(230, 110, 110);
const C_KILL: egui::Color32 = egui::Color32::from_rgb(255, 205, 90);
const C_ALIVE: egui::Color32 = egui::Color32::from_rgb(120, 200, 120);
const C_DEAD: egui::Color32 = egui::Color32::from_rgb(205, 110, 110);

/// Dim factor applied to a defeated team's panel so the victor reads as dominant.
const DIM_LOSER: f32 = 0.55;

/// Max width of the whole results block; centered, so it doesn't stretch
/// edge-to-edge on a wide window.
const CONTENT_MAX_W: f32 = 1080.0;

/// Main UI system for the Results screen.
///
/// Thin Bevy wrapper: grabs the egui context + resources and delegates the
/// actual drawing to [`draw_results_screen`] (which is pure egui, so it can be
/// snapshot-tested offscreen). Applies the DONE action on click.
pub fn results_ui(
    mut contexts: EguiContexts,
    results: Option<Res<MatchResults>>,
    combat_log: Res<CombatLog>,
    class_icons: Res<ClassIcons>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    let done = draw_results_screen(ctx, results.as_deref(), &combat_log, &class_icons);

    if done {
        commands.remove_resource::<MatchResults>();
        next_state.set(GameState::MainMenu);
    }
}

/// Render the entire Results screen into `ctx`. Returns `true` if the DONE
/// button was clicked this frame.
///
/// This is deliberately free of Bevy ECS types (takes plain references) so it
/// can be driven directly by an egui harness — see
/// `tests/results_screen_snapshot.rs`, which renders it offscreen with
/// `egui_kittest` for a fast, human-free visual-iteration loop.
pub fn draw_results_screen(
    ctx: &egui::Context,
    results: Option<&MatchResults>,
    combat_log: &CombatLog,
    class_icons: &ClassIcons,
) -> bool {
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = BG;
    style.visuals.panel_fill = BG;
    ctx.set_style(style);

    let mut done = false;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(BG)
                .inner_margin(egui::Margin::same(24)),
        )
        .show(ctx, |ui| {
          ui.vertical_centered(|ui| {
            ui.set_max_width(CONTENT_MAX_W);

            let Some(results) = results else {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.heading(
                        egui::RichText::new("No match results available")
                            .size(28.0)
                            .color(C_DEAD),
                    );
                });
                return;
            };

            render_banner(ui, results.winner, results.duration_secs);
            ui.add_space(24.0);

            // Bar scaling shared across both teams so lengths are comparable.
            let max_damage = results
                .team1_combatants
                .iter()
                .chain(results.team2_combatants.iter())
                .map(|s| s.damage_dealt)
                .fold(0.0_f32, f32::max)
                .max(1.0);

            // Two face-off panels. `columns` gives each panel its own
            // top-down layout (a plain `horizontal` wrapper would make the
            // panel interiors inherit a left-to-right layout and collapse
            // every row onto one line).
            ui.columns(2, |columns| {
                render_team_panel(
                    &mut columns[0], "TEAM 1", 1, &results.team1_combatants, combat_log,
                    class_icons, egui::Color32::from_rgb(90, 140, 230),
                    results.winner, max_damage,
                );
                render_team_panel(
                    &mut columns[1], "TEAM 2", 2, &results.team2_combatants, combat_log,
                    class_icons, egui::Color32::from_rgb(230, 90, 90),
                    results.winner, max_damage,
                );
            });

            ui.add_space(28.0);

            ui.vertical_centered(|ui| {
                let button = egui::Button::new(
                    egui::RichText::new("DONE")
                        .size(22.0)
                        .color(egui::Color32::from_rgb(230, 242, 230)),
                )
                .min_size(egui::vec2(200.0, 48.0));

                if ui.add(button).clicked() {
                    done = true;
                }
            });
          });
        });

    done
}

/// Render the top winner banner: victory line (in winner color) + match duration.
fn render_banner(ui: &mut egui::Ui, winner: Option<u8>, duration_secs: f32) {
    let (text, color) = match winner {
        None => ("DRAW".to_string(), egui::Color32::from_rgb(210, 200, 120)),
        Some(1) => ("TEAM 1 VICTORY".to_string(), egui::Color32::from_rgb(110, 160, 255)),
        Some(2) => ("TEAM 2 VICTORY".to_string(), egui::Color32::from_rgb(255, 110, 110)),
        Some(_) => ("MATCH COMPLETE".to_string(), HEADER_GREY),
    };
    let star = if winner.is_some() { "★ " } else { "" };

    egui::Frame::none()
        .fill(egui::Color32::from_rgb(26, 26, 38))
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(20, 14))
        .stroke(egui::Stroke::new(2.0, color))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new(format!("{star}{text}"))
                        .size(40.0)
                        .color(color),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("⏱ {}", fmt_duration(duration_secs)))
                            .size(22.0)
                            .color(HEADER_GREY),
                    );
                });
            });
        });
}

/// Render one team's face-off panel.
#[allow(clippy::too_many_arguments)]
fn render_team_panel(
    ui: &mut egui::Ui,
    title: &str,
    team: u8,
    combatants: &[CombatantStats],
    combat_log: &CombatLog,
    class_icons: &ClassIcons,
    team_color: egui::Color32,
    winner: Option<u8>,
    max_damage: f32,
) {
    let is_winner = winner == Some(team);
    let is_loser = winner.is_some() && !is_winner;
    let dimf = if is_loser { DIM_LOSER } else { 1.0 };

    let stroke = if is_winner {
        egui::Stroke::new(2.0, team_color)
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 55, 70))
    };

    egui::Frame::none()
        .fill(PANEL_BG)
        .rounding(6.0)
        .inner_margin(egui::Margin::same(14))
        .stroke(stroke)
        .show(ui, |ui| {
            // Stretch the panel to fill its column (cells are narrow).
            ui.set_min_width(ui.available_width());

            // Title row: team name + winner/defeated tag.
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(20.0)
                        .color(dim(team_color, dimf))
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_winner {
                        tag(ui, "★ WINNER", egui::Color32::from_rgb(60, 90, 50), C_KILL);
                    } else if is_loser {
                        ui.label(
                            egui::RichText::new("(defeated)")
                                .size(13.0)
                                .italics()
                                .color(dim(HEADER_GREY, dimf)),
                        );
                    }
                });
            });

            ui.add_space(10.0);

            // Column header row (stats right-aligned to the panel edge).
            ui.horizontal(|ui| {
                header_name_cell(ui, "CLASS", dimf);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = STAT_GAP;
                    header_num_cell(ui, W_K, "K", dimf);
                    header_num_cell(ui, W_TKN, "TKN", dimf);
                    header_num_cell(ui, W_HEAL, "HEAL", dimf);
                    header_num_cell(ui, W_DMG, "DMG", dimf);
                });
            });
            ui.add_space(4.0);

            // Combatant rows.
            for stats in combatants {
                combatant_block(ui, stats, team, combat_log, class_icons, max_damage, dimf);
            }

            // Σ TOTAL row.
            total_row(ui, combatants, combat_log, team, dimf);
        });
}

/// One combatant: stat row + relative damage mini-bar + expandable breakdown.
fn combatant_block(
    ui: &mut egui::Ui,
    stats: &CombatantStats,
    team: u8,
    combat_log: &CombatLog,
    class_icons: &ClassIcons,
    max_damage: f32,
    dimf: f32,
) {
    let cid = combatant_id(team, stats);
    let class_color = dim(class_color32(stats.class), dimf);
    let kills = combat_log.killing_blows(&cid);

    // Stat row (name left, stats right-aligned to the panel edge).
    ui.horizontal(|ui| {
        name_cell(ui, class_icons.textures.get(&stats.class).copied(), stats.class.name(), class_color);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = STAT_GAP;
            num_cell(ui, W_K, kills.to_string(), dim(C_KILL, dimf), false);
            num_cell(ui, W_TKN, fmt_k(stats.damage_taken), dim(C_TKN, dimf), false);
            num_cell(ui, W_HEAL, fmt_opt(stats.healing_done), dim(C_HEAL, dimf), false);
            num_cell(ui, W_DMG, fmt_k(stats.damage_dealt), dim(C_DMG, dimf), false);
        });
    });

    // Relative damage mini-bar + survival tag.
    let frac = (stats.damage_dealt / max_damage).clamp(0.0, 1.0);
    let (status_text, status_color) = if stats.survived {
        ("ALIVE", C_ALIVE)
    } else {
        ("DEAD", C_DEAD)
    };
    ui.horizontal(|ui| {
        let tag_w = 46.0;
        let bar_w = (ui.available_width() - tag_w).max(20.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, 7.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(38, 38, 50));
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac, 7.0));
        painter.rect_filled(fill, 2.0, dim(C_DMG, dimf * 0.9));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(status_text)
                    .size(9.0)
                    .strong()
                    .color(dim(status_color, dimf)),
            );
        });
    });

    // Expandable ability breakdown.
    egui::CollapsingHeader::new(
        egui::RichText::new("Ability breakdown")
            .size(10.0)
            .color(dim(egui::Color32::from_rgb(150, 150, 170), dimf)),
    )
    .id_salt(&cid)
    .show(ui, |ui| {
        render_ability_details(ui, &cid, combat_log, dimf);
    });

    ui.add_space(8.0);
}

/// Σ TOTAL subtotal row for a team (divider above, bold values).
fn total_row(
    ui: &mut egui::Ui,
    combatants: &[CombatantStats],
    combat_log: &CombatLog,
    team: u8,
    dimf: f32,
) {
    let dmg: f32 = combatants.iter().map(|s| s.damage_dealt).sum();
    let heal: f32 = combatants.iter().map(|s| s.healing_done).sum();
    let tkn: f32 = combatants.iter().map(|s| s.damage_taken).sum();
    let kills: u32 = combatants
        .iter()
        .map(|s| combat_log.killing_blows(&combatant_id(team, s)))
        .sum();

    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, dim(DIVIDER, dimf));
    ui.add_space(5.0);

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(W_NAME, ROW_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("Σ TOTAL")
                        .size(13.0)
                        .strong()
                        .color(dim(egui::Color32::from_rgb(205, 205, 215), dimf)),
                );
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = STAT_GAP;
            num_cell(ui, W_K, kills.to_string(), dim(C_KILL, dimf), true);
            num_cell(ui, W_TKN, fmt_k(tkn), dim(C_TKN, dimf), true);
            num_cell(ui, W_HEAL, fmt_opt(heal), dim(C_HEAL, dimf), true);
            num_cell(ui, W_DMG, fmt_k(dmg), dim(C_DMG, dimf), true);
        });
    });
}

/// Per-ability damage/healing bars + CC received, shown inside the expander.
fn render_ability_details(ui: &mut egui::Ui, cid: &str, combat_log: &CombatLog, dimf: f32) {
    ui.add_space(2.0);

    let damage = combat_log.damage_by_ability(cid);
    if !damage.is_empty() {
        ui.label(egui::RichText::new("Damage").size(10.0).color(dim(C_DMG, dimf)));
        render_ability_bars(ui, &damage, dim(C_DMG, dimf));
    }

    let healing = combat_log.healing_by_ability(cid);
    if !healing.is_empty() {
        ui.add_space(5.0);
        ui.label(egui::RichText::new("Healing").size(10.0).color(dim(C_HEAL, dimf)));
        render_ability_bars(ui, &healing, dim(C_HEAL, dimf));
    }

    let cc_received = combat_log.cc_received_seconds(cid);
    if cc_received > 0.0 {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!("CC received: {cc_received:.1}s"))
                .size(10.0)
                .color(dim(egui::Color32::from_rgb(180, 100, 255), dimf)),
        );
    }
}

/// Render the top-5 ability contribution bars for one breakdown map.
fn render_ability_bars(
    ui: &mut egui::Ui,
    by_ability: &std::collections::HashMap<String, f32>,
    bar_color: egui::Color32,
) {
    let mut entries: Vec<_> = by_ability.iter().collect();
    entries.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    let total: f32 = entries.iter().map(|(_, &v)| v).sum();

    for (ability, &amount) in entries.iter().take(5) {
        let pct = if total > 0.0 { amount / total } else { 0.0 };
        let width = ui.available_width().min(260.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(34, 34, 46));
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * pct, rect.height()));
        painter.rect_filled(fill, 2.0, bar_color.linear_multiply(0.5));
        painter.text(
            rect.left_center() + egui::vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            ability,
            egui::FontId::proportional(10.0),
            egui::Color32::from_rgb(215, 215, 220),
        );
        painter.text(
            rect.right_center() - egui::vec2(6.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("{amount:.0} ({:.0}%)", pct * 100.0),
            egui::FontId::proportional(9.0),
            bar_color,
        );
    }
}

// --- Cell helpers (fixed-width for column alignment) ---

/// Class name cell: accent stripe + icon (or color fallback) + class name.
fn name_cell(ui: &mut egui::Ui, icon: Option<egui::TextureId>, name: &str, color: egui::Color32) {
    ui.allocate_ui_with_layout(
        egui::vec2(W_NAME, ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            // Class-color accent stripe.
            let (stripe, _) =
                ui.allocate_exact_size(egui::vec2(3.0, ROW_HEIGHT - 6.0), egui::Sense::hover());
            ui.painter().rect_filled(stripe, 1.0, color);
            // Icon, or a colored square fallback if not loaded.
            let (irect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            if let Some(tex) = icon {
                ui.painter().image(
                    tex,
                    irect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                ui.painter().rect_filled(irect, 3.0, color);
            }
            ui.label(egui::RichText::new(name).size(14.0).strong().color(color));
        },
    );
}

/// Right-aligned fixed-width numeric cell.
fn num_cell(ui: &mut egui::Ui, width: f32, text: String, color: egui::Color32, strong: bool) {
    let mut rt = egui::RichText::new(text).size(14.0).color(color);
    if strong {
        rt = rt.strong();
    }
    ui.allocate_ui_with_layout(
        egui::vec2(width, ROW_HEIGHT),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(rt);
        },
    );
}

fn header_name_cell(ui: &mut egui::Ui, text: &str, dimf: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(W_NAME, 16.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(egui::RichText::new(text).size(10.0).color(dim(HEADER_GREY, dimf)));
        },
    );
}

fn header_num_cell(ui: &mut egui::Ui, width: f32, text: &str, dimf: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 16.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(egui::RichText::new(text).size(10.0).color(dim(HEADER_GREY, dimf)));
        },
    );
}

/// Small rounded pill tag (e.g. "★ WINNER").
fn tag(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(3.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).strong().color(fg));
        });
}

// --- Pure helpers ---

fn combatant_id(team: u8, stats: &CombatantStats) -> String {
    format!("Team {} {}", team, stats.class.name())
}

/// `8400.0 -> "8.4k"`, values under 1000 stay exact.
fn fmt_k(v: f32) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{v:.0}")
    }
}

/// Like `fmt_k`, but renders zero as an em dash (for non-healers' HEAL column).
fn fmt_opt(v: f32) -> String {
    if v > 0.0 {
        fmt_k(v)
    } else {
        "–".to_string()
    }
}

/// Seconds -> `M:SS`.
fn fmt_duration(secs: f32) -> String {
    let total = secs.max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Multiply an RGB color toward black by `f` (1.0 = unchanged, <1 = dimmer).
fn dim(c: egui::Color32, f: f32) -> egui::Color32 {
    egui::Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}

/// Convert a class's bevy `Color` to an egui `Color32`.
fn class_color32(class: CharacterClass) -> egui::Color32 {
    let c = class.color().to_srgba();
    egui::Color32::from_rgb(
        (c.red * 255.0) as u8,
        (c.green * 255.0) as u8,
        (c.blue * 255.0) as u8,
    )
}
