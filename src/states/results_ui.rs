//! Results Scene UI - Post-Match Statistics
//!
//! Displays match results after a battle concludes:
//! - Compact winner banner (victor color, match duration)
//! - Two aligned, face-off team panels (loser panel dimmed)
//! - Per-combatant rows with a linked class icon, aligned stat columns, a
//!   relative damage mini-bar, survival tag, and a click-to-expand ability
//!   breakdown whose bars link on to each ability
//! - Team Σ TOTAL subtotal row
//! - Return-to-menu button
//!
//! ## Data Source
//! Reads the `MatchResults` resource inserted at match end (winner, duration,
//! per-combatant `CombatantStats`) plus the `CombatLog` for per-ability
//! damage/healing, killing blows, and CC time. Icons and tooltip text come from
//! the encyclopedia's read-only `EncyclopediaData` bundle.
//!
//! ## Linked icons
//! Every class cell and every named ability in a breakdown is a LINK: hovering
//! shows that entity's tooltip, clicking opens its encyclopedia page with a
//! working way back here. Both halves come from `encyclopedia::widget`, so this
//! screen writes no tooltip prose of its own and cannot drift from the text the
//! encyclopedia and View Combatant show for the same thing.
//!
//! Navigation is RETURNED as a [`ResultsAction`], never applied inside the draw.
//! That is what keeps [`draw_results_screen`] a pure function of its inputs —
//! and that purity is what makes the offscreen snapshot loop in
//! `tests/results_screen_snapshot.rs` possible (CLAUDE.md, "Iterate on an egui
//! screen fast"). Reaching for `EguiContexts` or a `ResMut` in here would cost
//! the screen that loop; put it in [`results_ui`] instead.
//!
//! The round trip — step into the encyclopedia, come back to the same numbers —
//! rests on `MatchResults` being discarded only by DONE, and is pinned by
//! `tests/results_encyclopedia_round_trip.rs`.
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
use super::encyclopedia::{widget, EncyclopediaData, EncyclopediaState, Topic};
use super::match_config::CharacterClass;
use super::play_match::ability_config::AbilityDefinitions;
use super::play_match::equipment::ItemDefinitions;
use super::play_match::AbilityType;
use super::view_combatant_ui::{AbilityIcons, ItemIcons};
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
/// Near-white text drawn on top of the colored ability-breakdown bars (kept
/// independent of the bar fill color so it stays legible on a full bar).
const BAR_TEXT: egui::Color32 = egui::Color32::from_rgb(244, 244, 248);

/// Dim factor applied to a defeated team's panel so the victor reads as dominant.
const DIM_LOSER: f32 = 0.72;

/// Max width of the whole results block; centered, so it doesn't stretch
/// edge-to-edge on a wide window.
const CONTENT_MAX_W: f32 = 1080.0;

/// The encyclopedia link layer, bundled so the render helpers take one extra
/// parameter instead of four.
///
/// It owns no text of its own: hover copy comes from [`widget::link`], which
/// delegates to the same builders the encyclopedia and View Combatant use. The
/// only thing this struct adds is the name→topic resolution the combat log
/// forces, because the log records an ability by its DISPLAY NAME.
struct Links<'a> {
    data: &'a EncyclopediaData<'a>,
    /// Display name → ability, built once per frame from `abilities.ron`.
    by_name: std::collections::HashMap<&'a str, AbilityType>,
    /// The topic the reader clicked this frame, if any.
    clicked: Option<Topic>,
}

impl<'a> Links<'a> {
    fn new(data: &'a EncyclopediaData<'a>) -> Self {
        Self {
            data,
            by_name: data
                .abilities
                .iter()
                .map(|(ability, config)| (config.name.as_str(), *ability))
                .collect(),
            clicked: None,
        }
    }

    /// Attach the hover tooltip + click-to-navigate contract to an already-drawn
    /// response, recording a click for the caller to return.
    fn link(&mut self, response: egui::Response, topic: Topic) {
        if let Some(topic) = widget::link(response, topic, self.data) {
            self.clicked = Some(topic);
        }
    }

    /// Resolve a combat-log ability label to its encyclopedia topic.
    ///
    /// Pet damage is folded into its owner's breakdown under `"<Pet>: <ability>"`
    /// (see `MatchResults::pet_damage_links`), so the prefix is stripped before
    /// the lookup. Labels that name no ability at all — auto attacks, wands —
    /// resolve to `None` and simply stay unlinked.
    fn ability_topic(&self, label: &str) -> Option<Topic> {
        let bare = label.rsplit_once(": ").map_or(label, |(_, name)| name);
        self.by_name.get(bare).copied().map(Topic::Ability)
    }
}

/// What the reader asked the Results screen to do this frame.
///
/// Navigation is RETURNED rather than applied inside the draw, so
/// [`draw_results_screen`] stays a pure function of its inputs — which is what
/// lets `tests/results_screen_snapshot.rs` render it offscreen. The Bevy
/// wrapper below is the only thing that touches the ECS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResultsAction {
    /// DONE — discard the results and go back to the main menu.
    Done,
    /// A linked icon was clicked: open this topic in the encyclopedia and come
    /// back here afterwards.
    OpenTopic(Topic),
}

/// Main UI system for the Results screen.
///
/// Thin Bevy wrapper: grabs the egui context + resources and delegates the
/// actual drawing to [`draw_results_screen`] (which is pure egui, so it can be
/// snapshot-tested offscreen). Applies whatever [`ResultsAction`] comes back.
pub fn results_ui(
    mut contexts: EguiContexts,
    results: Option<Res<MatchResults>>,
    combat_log: Res<CombatLog>,
    class_icons: Res<ClassIcons>,
    item_definitions: Res<ItemDefinitions>,
    ability_definitions: Res<AbilityDefinitions>,
    item_icons: Option<Res<ItemIcons>>,
    ability_icons: Option<Res<AbilityIcons>>,
    mut encyclopedia: ResMut<EncyclopediaState>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    let data = EncyclopediaData {
        items: &item_definitions,
        abilities: &ability_definitions,
        item_icons: item_icons.as_deref(),
        class_icons: Some(&class_icons),
        ability_icons: ability_icons.as_deref(),
    };

    let action = draw_results_screen(ctx, results.as_deref(), &combat_log, &data);
    apply_results_action(action, &mut encyclopedia, &mut next_state, &mut commands);
}

/// Apply the screen's action to the world.
///
/// Split out of [`results_ui`] so a test can drive it with a real `World` and
/// prove the property the encyclopedia round trip depends on: **only the DONE
/// path discards [`MatchResults`]**. Nothing else in the app removes that
/// resource, and neither the Results nor the Encyclopedia state has an
/// `OnEnter`/`OnExit` hook, so a reader who steps into the encyclopedia and
/// walks back comes home to the same numbers.
pub fn apply_results_action(
    action: Option<ResultsAction>,
    encyclopedia: &mut EncyclopediaState,
    next_state: &mut NextState<GameState>,
    commands: &mut Commands,
) {
    match action {
        None => {}
        Some(ResultsAction::Done) => {
            commands.remove_resource::<MatchResults>();
            next_state.set(GameState::MainMenu);
        }
        Some(ResultsAction::OpenTopic(topic)) => {
            // The results are LEFT IN PLACE: the encyclopedia is an
            // informational detour, and `open_at` records where to come back to.
            encyclopedia.open_at(topic, GameState::Results);
            next_state.set(GameState::Encyclopedia);
        }
    }
}

/// Render the entire Results screen into `ctx`, returning the action the reader
/// requested this frame (if any).
///
/// This is deliberately free of Bevy ECS types (takes plain references) so it
/// can be driven directly by an egui harness — see
/// `tests/results_screen_snapshot.rs`, which renders it offscreen with
/// `egui_kittest` for a fast, human-free visual-iteration loop. The linked-icon
/// widget it borrows from the encyclopedia is Bevy-free for the same reason, so
/// the two compose without dragging the ECS into the draw.
pub fn draw_results_screen(
    ctx: &egui::Context,
    results: Option<&MatchResults>,
    combat_log: &CombatLog,
    data: &EncyclopediaData,
) -> Option<ResultsAction> {
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = BG;
    style.visuals.panel_fill = BG;
    // Zero-delay tooltips, matching the encyclopedia: hovering a linked icon
    // must answer at once — that immediacy is the point of the widget.
    style.interaction.tooltip_delay = 0.0;
    ctx.set_style(style);

    let mut links = Links::new(data);
    let mut done = false;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
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
                    &mut links, egui::Color32::from_rgb(90, 140, 230),
                    results.winner, max_damage, &results.pet_damage_links,
                );
                render_team_panel(
                    &mut columns[1], "TEAM 2", 2, &results.team2_combatants, combat_log,
                    &mut links, egui::Color32::from_rgb(230, 90, 90),
                    results.winner, max_damage, &results.pet_damage_links,
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

    // A click on a linked icon wins over DONE: they cannot both happen in one
    // frame, and answering the reader's most specific gesture is the safe order.
    match (links.clicked, done) {
        (Some(topic), _) => Some(ResultsAction::OpenTopic(topic)),
        (None, true) => Some(ResultsAction::Done),
        (None, false) => None,
    }
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

    egui::Frame::new()
        .fill(egui::Color32::from_rgb(26, 26, 38))
        .corner_radius(8.0)
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
    links: &mut Links,
    team_color: egui::Color32,
    winner: Option<u8>,
    max_damage: f32,
    pet_links: &std::collections::HashMap<String, (String, String)>,
) {
    let is_winner = winner == Some(team);
    let is_loser = winner.is_some() && !is_winner;
    let dimf = if is_loser { DIM_LOSER } else { 1.0 };

    let stroke = if is_winner {
        egui::Stroke::new(2.0, team_color)
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 55, 70))
    };

    egui::Frame::new()
        .fill(PANEL_BG)
        .corner_radius(6.0)
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

            // Combatant rows. Every row carries the "#slot" suffix, matching the
            // combat-log ids unconditionally — so a reader cross-referencing the
            // saved report to this screen always finds the same label, and two
            // same-class teammates are never ambiguous.
            for stats in combatants {
                combatant_block(ui, stats, team, combat_log, links, max_damage, dimf, pet_links);
            }

            // Σ TOTAL row.
            total_row(ui, combatants, combat_log, team, dimf, pet_links);
        });
}

/// One combatant: stat row + relative damage mini-bar + expandable breakdown.
#[allow(clippy::too_many_arguments)]
fn combatant_block(
    ui: &mut egui::Ui,
    stats: &CombatantStats,
    team: u8,
    combat_log: &CombatLog,
    links: &mut Links,
    max_damage: f32,
    dimf: f32,
    pet_links: &std::collections::HashMap<String, (String, String)>,
) {
    let cid = stats_log_id(team, stats);
    let class_color = dim(class_color32(stats.class), dimf);
    let kills = combat_log.killing_blows_including_pets(&cid, pet_links);
    let row_label = format!("{} #{}", stats.class.name(), stats.slot + 1);

    // Stat row (name left, stats right-aligned to the panel edge).
    ui.horizontal(|ui| {
        // The class cell is a LINK: hovering it shows the class's own tooltip
        // (built by the shared builder, not restated here) and clicking it
        // opens that class in the encyclopedia.
        let icon = links
            .data
            .class_icons
            .and_then(|icons| icons.textures.get(&stats.class).copied());
        let cell = name_cell(ui, icon, &row_label, class_color);
        let hit = ui.interact(
            cell.rect,
            class_link_id(team, stats.slot, stats.class),
            egui::Sense::click(),
        );
        links.link(hit, Topic::Class(stats.class));
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
        render_ability_details(ui, &cid, combat_log, dimf, pet_links, links);
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
    pet_links: &std::collections::HashMap<String, (String, String)>,
) {
    let dmg: f32 = combatants.iter().map(|s| s.damage_dealt).sum();
    let heal: f32 = combatants.iter().map(|s| s.healing_done).sum();
    let tkn: f32 = combatants.iter().map(|s| s.damage_taken).sum();
    let kills: u32 = combatants
        .iter()
        .map(|s| combat_log.killing_blows_including_pets(&stats_log_id(team, s), pet_links))
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
fn render_ability_details(
    ui: &mut egui::Ui,
    cid: &str,
    combat_log: &CombatLog,
    dimf: f32,
    pet_links: &std::collections::HashMap<String, (String, String)>,
    links: &mut Links,
) {
    ui.add_space(2.0);

    let damage = combat_log.damage_by_ability_including_pets(cid, pet_links);
    if !damage.is_empty() {
        ui.label(egui::RichText::new("Damage").size(10.0).color(dim(C_DMG, dimf)));
        render_ability_bars(ui, &damage, dim(C_DMG, dimf), dim(BAR_TEXT, dimf), links);
    }

    let healing = combat_log.healing_by_ability(cid);
    if !healing.is_empty() {
        ui.add_space(5.0);
        ui.label(egui::RichText::new("Healing").size(10.0).color(dim(C_HEAL, dimf)));
        render_ability_bars(ui, &healing, dim(C_HEAL, dimf), dim(BAR_TEXT, dimf), links);
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
///
/// `text_color` is kept near-white (not `bar_color`) so the ability name and
/// amount stay legible on top of a full/near-full colored bar fill.
fn render_ability_bars(
    ui: &mut egui::Ui,
    by_ability: &std::collections::HashMap<String, f32>,
    bar_color: egui::Color32,
    text_color: egui::Color32,
    links: &mut Links,
) {
    /// Icon square inside a bar, and the gap either side of it. The slot is
    /// reserved on EVERY row — including the auto-attack lines that name no
    /// ability — so the labels stay on one left edge instead of stepping in
    /// and out with whatever happens to be linkable.
    const BAR_ICON: f32 = 12.0;
    const BAR_PAD: f32 = 4.0;

    // Biggest contribution first, ties broken by NAME. The source is a
    // `HashMap`, whose iteration order varies run to run, so amount alone left
    // two equal bars swapping places between renders of the same match.
    let mut entries: Vec<_> = by_ability.iter().collect();
    entries.sort_by(|a, b| {
        b.1.partial_cmp(a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    let total: f32 = entries.iter().map(|(_, &v)| v).sum();

    for (ability, &amount) in entries.iter().take(5) {
        let pct = if total > 0.0 { amount / total } else { 0.0 };
        let width = ui.available_width().min(260.0);
        let topic = links.ability_topic(ability);
        let sense = if topic.is_some() { egui::Sense::click() } else { egui::Sense::hover() };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 16.0), sense);
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(34, 34, 46));
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * pct, rect.height()));
        painter.rect_filled(fill, 2.0, bar_color.linear_multiply(0.5));
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + BAR_PAD + BAR_ICON / 2.0, rect.center().y),
            egui::vec2(BAR_ICON, BAR_ICON),
        );
        if let Some(topic) = topic {
            widget::paint_icon(painter, icon_rect, topic, links.data);
            if response.hovered() {
                painter.rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.0, text_color.gamma_multiply(0.4)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        painter.text(
            egui::pos2(icon_rect.right() + BAR_PAD, rect.center().y),
            egui::Align2::LEFT_CENTER,
            ability,
            egui::FontId::proportional(10.0),
            text_color,
        );
        painter.text(
            rect.right_center() - egui::vec2(6.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("{amount:.0} ({:.0}%)", pct * 100.0),
            egui::FontId::proportional(9.0),
            text_color,
        );
        if let Some(topic) = topic {
            links.link(response, topic);
        }
    }
}

// --- Cell helpers (fixed-width for column alignment) ---

/// Absolute egui id of a combatant row's class link.
///
/// PINNED rather than derived from sibling order (egui's default), for the same
/// reason the encyclopedia pins its search field: the id has to survive rows
/// being added or reordered, and the snapshot harness needs a stable handle to
/// drive a real hover over the widget.
pub fn class_link_id(team: u8, slot: u8, class: CharacterClass) -> egui::Id {
    egui::Id::new((
        "results_class_link",
        super::play_match::combatant_id(team, slot, class),
    ))
}

/// Class name cell: accent stripe + icon (or color fallback) + class name.
///
/// Returns the cell's response so the caller can hang the linked-icon contract
/// on it — the cell is drawn exactly as before, and the link is layered on top
/// via `ui.interact`, so a non-hovered frame is pixel-identical.
fn name_cell(
    ui: &mut egui::Ui,
    icon: Option<egui::TextureId>,
    name: &str,
    color: egui::Color32,
) -> egui::Response {
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
    )
    .response
}

/// Right-aligned numeric cell of an *exact* `width`. We reserve the rect and
/// paint the text into it (rather than `allocate_ui_with_layout`, which lets
/// the cell shrink to its content and so misaligns columns when value widths
/// differ — e.g. a "–" next to "12.3k").
fn num_cell(ui: &mut egui::Ui, width: f32, text: String, color: egui::Color32, strong: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::hover());
    let pos = rect.right_center();
    let font = egui::FontId::proportional(14.0);
    let painter = ui.painter();
    // Faux-bold for totals: a second pass nudged a fraction of a pixel.
    if strong {
        painter.text(pos + egui::vec2(0.6, 0.0), egui::Align2::RIGHT_CENTER, &text, font.clone(), color);
    }
    painter.text(pos, egui::Align2::RIGHT_CENTER, &text, font, color);
}

fn header_name_cell(ui: &mut egui::Ui, text: &str, dimf: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(W_NAME, 16.0), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(10.0),
        dim(HEADER_GREY, dimf),
    );
}

fn header_num_cell(ui: &mut egui::Ui, width: f32, text: &str, dimf: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), egui::Sense::hover());
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        text,
        egui::FontId::proportional(10.0),
        dim(HEADER_GREY, dimf),
    );
}

/// Small rounded pill tag (e.g. "★ WINNER").
fn tag(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).strong().color(fg));
        });
}

// --- Pure helpers ---

/// This combatant's combat-log id, from its team + slot + class. Delegates to
/// the canonical builder so it stays byte-for-byte identical to the ids the
/// combat log is written under (a divergent format here would silently break
/// the kill-count / ability-breakdown lookups). Named distinctly so it does not
/// shadow the imported `play_match::combatant_id`.
fn stats_log_id(team: u8, stats: &CombatantStats) -> String {
    super::play_match::combatant_id(team, stats.slot, stats.class)
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
/// Public: also used by the in-match team frames for icon fallbacks.
pub fn class_color32(class: CharacterClass) -> egui::Color32 {
    let c = class.color().to_srgba();
    egui::Color32::from_rgb(
        (c.red * 255.0) as u8,
        (c.green * 255.0) as u8,
        (c.blue * 255.0) as u8,
    )
}
