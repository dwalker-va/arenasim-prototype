//! Results Scene UI - Post-Match Statistics
//!
//! The Report template (design doc §2.4): the shared screen header — its exit,
//! the title, the AI profile that played and the match duration in the
//! trailing slot — then a `DISPLAY`
//! victory banner, then the data panels:
//! - Two aligned, face-off team panels (loser panel dimmed)
//! - Per-combatant rows with a linked class icon, aligned stat columns, a
//!   relative damage mini-bar, survival tag, and a click-to-expand ability
//!   breakdown whose bars link on to each ability
//! - Team Σ TOTAL subtotal row
//!
//! Every colour, size and gap is a `ui::theme` token. The stat columns keep
//! their own hues — damage, healing, taken, kills — from the theme's DATA
//! layer, where hue carries meaning.
//!
//! ## Data Source
//! Reads the `MatchResults` resource inserted at match end (winner, duration,
//! per-combatant `CombatantStats`) plus the `CombatLog` for per-ability
//! damage/healing, killing blows, and CC time. Icons and tooltip text come from
//! the encyclopedia's read-only `EncyclopediaData` bundle.
//!
//! ## The exit
//! One exit, in the header: `Nav::Return`, back to whatever started the match
//! (`ScreenNav::open_match` records it — the main menu today, a run hub
//! tomorrow), labelled with that screen's name. Esc takes the same path. The
//! header is drawn with or without results, so the empty screen has the same
//! way out.
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
//! rests on `MatchResults` being discarded only by the exit, and is pinned by
//! `tests/results_encyclopedia_round_trip.rs`.
//!
//! ## UI Structure
//! ```text
//! ┌────────────────────────────────────────────────────┐
//! │ [◀ MAIN MENU]  MATCH RESULTS    AI: TeamPlan ⏱ 1:47 │
//! │────────────────────────────────────────────────────│
//! │  ★ TEAM 1 VICTORY                                  │
//! ├──────────────────────────┬─────────────────────────┤
//! │ TEAM 1         ★ WINNER   │ TEAM 2         DEFEATED  │
//! │ CLASS    DMG HEAL TKN  K  │ CLASS   DMG HEAL TKN  K  │
//! │ ▌🛡Warrior 8.4k  –  3.1k 1 │ ▌❄Mage  4.2k  – 9.0k  0  │
//! │   ▓▓▓▓▓▓▓▓▓▓▓▓▓     ALIVE │   ▓▓▓▓▓             DEAD │
//! │ Σ TOTAL  9.6k 6.8k 5.5k 1 │ Σ TOTAL 5.1k 5.1k15.2k 0 │
//! └──────────────────────────┴─────────────────────────┘
//! ```

use super::chrome::ScreenNav;
use super::configure_match_ui::ClassIcons;
use super::encyclopedia::{widget, EncyclopediaData, EncyclopediaState, Topic};
use super::match_config::CharacterClass;
use super::play_match::ability_config::AbilityDefinitions;
use super::play_match::equipment::ItemDefinitions;
use super::play_match::AbilityType;
use super::view_combatant_ui::{AbilityIcons, ItemIcons};
use super::{
    play_match::{CombatantStats, MatchResults},
    GameState,
};
use crate::combat::log::CombatLog;
use crate::ui::theme::{color, data, icon, radius, size, space, stroke, text};
use crate::ui::widgets::panel::margin_xy;
use crate::ui::widgets::{divider, empty_state, panel, panel_header, screen_header, Nav};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

/// The screen's title, in its header.
const TITLE: &str = "Match Results";

// --- Column widths (fixed, so numeric columns align across the header, every
//     combatant row, and the Σ TOTAL row). Layout of this one table, not
//     tokens: no other screen draws these columns. ---
const W_NAME: f32 = 116.0; // accent stripe + icon + class name
const W_DMG: f32 = 52.0;
const W_HEAL: f32 = 52.0;
const W_TKN: f32 = 52.0;
const W_K: f32 = 26.0;
const ROW_HEIGHT: f32 = 22.0;
/// The width the survival tag ("ALIVE" / "DEAD") keeps beside the mini-bar.
const W_STATUS: f32 = 46.0;
/// The widest an ability-breakdown bar grows.
const W_ABILITY_BAR: f32 = 260.0;

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
    /// (see `MatchResults::pet_damage_links`), so a prefix strip is the fallback.
    /// The WHOLE label is tried first, because a real ability name must beat a
    /// prefix-stripped guess: "Power Word: Shield" and "Power Word: Fortitude"
    /// carry that separator inside their own names, and stripping first resolved
    /// them to "Shield" — which names nothing, so the bar rendered unlinked with
    /// no error. The strip cuts at the FIRST separator, the exact inverse of the
    /// `"{pet_name}: {ability}"` the label is built from, so a pet casting such
    /// an ability resolves too.
    ///
    /// Labels that name no ability at all — auto attacks, wands — resolve to
    /// `None` and simply stay unlinked.
    fn ability_topic(&self, label: &str) -> Option<Topic> {
        let stripped = label.split_once(": ").map(|(_, name)| name);
        self.by_name
            .get(label)
            .or_else(|| stripped.and_then(|bare| self.by_name.get(bare)))
            .copied()
            .map(Topic::Ability)
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
    /// The exit — discard the results and leave for whatever started the
    /// match (the main menu today). Esc asks for the same.
    Exit,
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
    mut nav: ScreenNav,
    mut commands: Commands,
) {
    // Esc takes the exit's own path, so it discards the results too. It needs
    // no egui context, so it answers on any frame.
    if nav.escape(false, || false).is_some() {
        apply_results_action(
            Some(ResultsAction::Exit),
            &mut encyclopedia,
            &mut nav,
            &mut commands,
        );
        return;
    }
    let screen_nav = nav.nav(false).expect("Results has an exit");

    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    let data = EncyclopediaData {
        items: &item_definitions,
        abilities: &ability_definitions,
        item_icons: item_icons.as_deref(),
        class_icons: Some(&class_icons),
        ability_icons: ability_icons.as_deref(),
    };

    let action = draw_results_screen(ctx, results.as_deref(), &combat_log, &data, screen_nav);
    apply_results_action(action, &mut encyclopedia, &mut nav, &mut commands);
}

/// Apply the screen's action to the world.
///
/// Split out of [`results_ui`] so a test can drive it with a real `World` and
/// prove the property the encyclopedia round trip depends on: **only the exit
/// path discards [`MatchResults`]** (the exit button and Esc both take it). Nothing else in the app removes that
/// resource, and neither the Results nor the Encyclopedia state has an
/// `OnEnter`/`OnExit` hook, so a reader who steps into the encyclopedia and
/// walks back comes home to the same numbers.
pub fn apply_results_action(
    action: Option<ResultsAction>,
    encyclopedia: &mut EncyclopediaState,
    nav: &mut ScreenNav,
    commands: &mut Commands,
) {
    match action {
        None => {}
        Some(ResultsAction::Exit) => {
            commands.remove_resource::<MatchResults>();
            let to = nav
                .return_to(GameState::Results)
                .expect("Results has an exit");
            nav.leave(to);
        }
        Some(ResultsAction::OpenTopic(topic)) => {
            // The results are LEFT IN PLACE: the encyclopedia is an
            // informational detour, and opening it records where to come
            // back to.
            encyclopedia.open_at(topic);
            nav.open(GameState::Encyclopedia);
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
    nav: Nav,
) -> Option<ResultsAction> {
    let mut links = Links::new(data);
    let mut exit = false;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(margin_xy(space::XL, space::LG)),
        )
        .show(ctx, |ui| {
            // The shared header, drawn whether or not there are results: the
            // empty screen needs the same way out. Its exit is the screen's
            // only one; Esc takes the same path (in `results_ui`).
            let duration = results.map(|r| r.duration_secs);
            let profiles = results.map(|r| r.ai_profiles);
            if screen_header(ui, TITLE, nav, |ui| {
                if let Some(secs) = duration {
                    ui.label(
                        egui::RichText::new(format!("⏱ {}", fmt_duration(secs)))
                            .size(text::H3)
                            .color(color::TEXT_MUTED),
                    );
                }
                // Which AI played: the client plays `TeamPlan`, a replay what
                // its config says, so a report can always be read for it.
                if let Some(profiles) = profiles {
                    let label = profiles.trace_label();
                    ui.add_space(space::LG);
                    ui.label(
                        egui::RichText::new(format!("AI: {label}"))
                            .size(text::H3)
                            .color(color::TEXT_MUTED),
                    );
                    crate::ui::driver::note(ui, format_args!("results:ai_profile={label}"));
                }
            })
            .is_some()
            {
                exit = true;
            }
            ui.add_space(space::LG);

            ui.vertical_centered(|ui| {
                ui.set_max_width(CONTENT_MAX_W);

                let Some(results) = results else {
                    empty_state(
                        ui,
                        "results:empty",
                        "No match results available",
                        "There is no finished match to report on.",
                        None,
                    );
                    return;
                };

                render_banner(ui, results.winner);
                ui.add_space(space::LG);

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
                        &mut columns[0],
                        "Team 1",
                        1,
                        &results.team1_combatants,
                        combat_log,
                        &mut links,
                        data::TEAM_1,
                        results.winner,
                        max_damage,
                        &results.pet_damage_links,
                    );
                    render_team_panel(
                        &mut columns[1],
                        "Team 2",
                        2,
                        &results.team2_combatants,
                        combat_log,
                        &mut links,
                        data::TEAM_2,
                        results.winner,
                        max_damage,
                        &results.pet_damage_links,
                    );
                });
            });
        });

    // A click on a linked icon wins over the exit: they cannot both happen in one
    // frame, and answering the reader's most specific gesture is the safe order.
    match (links.clicked, exit) {
        (Some(topic), _) => Some(ResultsAction::OpenTopic(topic)),
        (None, true) => Some(ResultsAction::Exit),
        (None, false) => None,
    }
}

/// The victory banner: the result at [`text::DISPLAY`], in the winner's team
/// colour, on a panel edged in the same colour.
fn render_banner(ui: &mut egui::Ui, winner: Option<u8>) {
    let (label, tint) = match winner {
        None => ("DRAW", color::ACCENT),
        Some(1) => ("TEAM 1 VICTORY", data::TEAM_1),
        Some(2) => ("TEAM 2 VICTORY", data::TEAM_2),
        Some(_) => ("MATCH COMPLETE", color::TEXT_MUTED),
    };
    let star = if winner.is_some() { "★ " } else { "" };

    panel(Some(tint))
        .inner_margin(margin_xy(space::LG, space::MD))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{star}{label}"))
                        .size(text::DISPLAY)
                        .color(tint),
                );
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

    // The winner's panel is edged in its team colour; every other panel
    // takes the plain line.
    panel(is_winner.then_some(team_color)).show(ui, |ui| {
        // Stretch the panel to fill its column (cells are narrow).
        ui.set_min_width(ui.available_width());

        // Title row: team name + winner/defeated tag.
        panel_header(ui, title, dim(team_color, dimf), |ui| {
            if is_winner {
                ui.label(
                    egui::RichText::new("★ WINNER")
                        .size(text::SMALL)
                        .color(color::ACCENT),
                );
            } else if is_loser {
                ui.label(
                    egui::RichText::new("DEFEATED")
                        .size(text::SMALL)
                        .color(dim(color::TEXT_MUTED, dimf)),
                );
            }
        });

        // Column header row (stats right-aligned to the panel edge).
        ui.horizontal(|ui| {
            header_name_cell(ui, "CLASS", dimf);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::MD;
                header_num_cell(ui, W_K, "K", dimf);
                header_num_cell(ui, W_TKN, "TKN", dimf);
                header_num_cell(ui, W_HEAL, "HEAL", dimf);
                header_num_cell(ui, W_DMG, "DMG", dimf);
            });
        });
        ui.add_space(space::XS);

        // Combatant rows. Every row carries the "#slot" suffix, matching the
        // combat-log ids unconditionally — so a reader cross-referencing the
        // saved report to this screen always finds the same label, and two
        // same-class teammates are never ambiguous.
        for stats in combatants {
            combatant_block(
                ui, stats, team, combat_log, links, max_damage, dimf, pet_links,
            );
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
        // Same hover outline the ability bars wear: the two links sit on one
        // screen, so they announce themselves the same way rather than leaving
        // this one to the cursor shape alone.
        if hit.hovered() {
            paint_hover_stroke(ui.painter(), cell.rect, dim(color::TEXT, dimf));
        }
        links.link(hit, Topic::Class(stats.class));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::MD;
            num_cell(ui, W_K, kills.to_string(), dim(data::KILLS, dimf));
            num_cell(ui, W_TKN, fmt_k(stats.damage_taken), dim(data::TAKEN, dimf));
            num_cell(
                ui,
                W_HEAL,
                fmt_opt(stats.healing_done),
                dim(data::HEALING, dimf),
            );
            num_cell(
                ui,
                W_DMG,
                fmt_k(stats.damage_dealt),
                dim(data::DAMAGE, dimf),
            );
        });
    });

    // Relative damage mini-bar + survival tag.
    let frac = (stats.damage_dealt / max_damage).clamp(0.0, 1.0);
    let (status_text, status_color) = if stats.survived {
        ("ALIVE", data::ALIVE)
    } else {
        ("DEAD", data::DEAD)
    };
    ui.horizontal(|ui| {
        let bar_w = (ui.available_width() - W_STATUS).max(W_K);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, size::BAR), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, radius::SM, color::RAISED);
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac, size::BAR));
        painter.rect_filled(fill, radius::SM, dim(data::DAMAGE, dimf * 0.9));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(status_text)
                    .size(text::MICRO)
                    .color(dim(status_color, dimf)),
            );
        });
    });

    // Expandable ability breakdown.
    egui::CollapsingHeader::new(
        egui::RichText::new("Ability breakdown")
            .size(text::MICRO)
            .color(dim(color::TEXT_MUTED, dimf)),
    )
    .id_salt(&cid)
    .show(ui, |ui| {
        render_ability_details(ui, &cid, combat_log, dimf, pet_links, links);
    });

    ui.add_space(space::SM);
}

/// Σ TOTAL subtotal row for a team, under a rule.
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

    divider(ui);
    ui.add_space(space::XS);

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(W_NAME, ROW_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("Σ TOTAL")
                        .size(text::SMALL)
                        .color(dim(color::TEXT, dimf)),
                );
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::MD;
            num_cell(ui, W_K, kills.to_string(), dim(data::KILLS, dimf));
            num_cell(ui, W_TKN, fmt_k(tkn), dim(data::TAKEN, dimf));
            num_cell(ui, W_HEAL, fmt_opt(heal), dim(data::HEALING, dimf));
            num_cell(ui, W_DMG, fmt_k(dmg), dim(data::DAMAGE, dimf));
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
    ui.add_space(space::XXS);
    let bar_text = dim(color::TEXT, dimf);

    let damage = combat_log.damage_by_ability_including_pets(cid, pet_links);
    if !damage.is_empty() {
        ui.label(
            egui::RichText::new("Damage")
                .size(text::MICRO)
                .color(dim(data::DAMAGE, dimf)),
        );
        render_ability_bars(ui, &damage, dim(data::DAMAGE, dimf), bar_text, links);
    }

    let healing = combat_log.healing_by_ability(cid);
    if !healing.is_empty() {
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new("Healing")
                .size(text::MICRO)
                .color(dim(data::HEALING, dimf)),
        );
        render_ability_bars(ui, &healing, dim(data::HEALING, dimf), bar_text, links);
    }

    let cc_received = combat_log.cc_received_seconds(cid);
    if cc_received > 0.0 {
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new(format!("CC received: {cc_received:.1}s"))
                .size(text::MICRO)
                .color(dim(data::CC_RECEIVED, dimf)),
        );
    }
}

/// Render the top-5 ability contribution bars for one breakdown map.
///
/// `text_color` is the body text colour (not `bar_color`) so the ability name and
/// amount stay legible on top of a full/near-full colored bar fill.
fn render_ability_bars(
    ui: &mut egui::Ui,
    by_ability: &std::collections::HashMap<String, f32>,
    bar_color: egui::Color32,
    text_color: egui::Color32,
    links: &mut Links,
) {
    // The icon square inside a bar, and the gap either side of it. The slot
    // is reserved on EVERY row — including the auto-attack lines that name no
    // ability — so the labels stay on one left edge instead of stepping in
    // and out with whatever happens to be linkable.
    let bar_icon = icon::XS;
    let bar_pad = space::XXS;
    let bar_height = bar_icon + 2.0 * bar_pad;

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
        let width = ui.available_width().min(W_ABILITY_BAR);
        let topic = links.ability_topic(ability);
        let sense = if topic.is_some() {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, bar_height), sense);
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let painter = ui.painter();
        painter.rect_filled(rect, radius::SM, color::RAISED);
        let fill =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * pct, rect.height()));
        painter.rect_filled(fill, radius::SM, bar_color.linear_multiply(0.5));
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + bar_pad + bar_icon / 2.0, rect.center().y),
            egui::Vec2::splat(bar_icon),
        );
        if let Some(topic) = topic {
            widget::paint_icon(painter, icon_rect, topic, links.data);
            if response.hovered() {
                paint_hover_stroke(painter, rect, text_color);
            }
        }
        painter.text(
            egui::pos2(icon_rect.right() + space::XS, rect.center().y),
            egui::Align2::LEFT_CENTER,
            ability,
            egui::FontId::proportional(text::MICRO),
            text_color,
        );
        painter.text(
            rect.right_center() - egui::vec2(space::SM, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("{amount:.0} ({:.0}%)", pct * 100.0),
            egui::FontId::proportional(text::MICRO),
            text_color,
        );
        if let Some(topic) = topic {
            links.link(response, topic);
        }
    }
}

/// The hairline outline a hovered LINK wears on this screen.
///
/// One recipe, used by both links the screen offers — the ability bars and the
/// class cell — so the pair cannot drift into announcing themselves
/// differently. Drawn on top of the already-painted widget, so a non-hovered
/// frame is pixel-identical.
fn paint_hover_stroke(painter: &egui::Painter, rect: egui::Rect, text_color: egui::Color32) {
    painter.rect_stroke(
        rect,
        radius::SM,
        egui::Stroke::new(stroke::HAIRLINE, text_color.gamma_multiply(0.4)),
        egui::StrokeKind::Inside,
    );
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
    tint: egui::Color32,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(W_NAME, ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            // Class-color accent stripe.
            let (stripe, _) = ui.allocate_exact_size(
                egui::vec2(space::XXS, icon::SM - space::XS),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(stripe, radius::SM, tint);
            // Icon, or a colored square fallback if not loaded.
            let (irect, _) =
                ui.allocate_exact_size(egui::Vec2::splat(icon::SM), egui::Sense::hover());
            if let Some(tex) = icon {
                ui.painter().image(
                    tex,
                    irect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    color::UNTINTED,
                );
            } else {
                ui.painter().rect_filled(irect, radius::SM, tint);
            }
            ui.label(egui::RichText::new(name).size(text::BODY).color(tint));
        },
    )
    .response
}

/// Right-aligned numeric cell of an *exact* `width`. We reserve the rect and
/// paint the text into it (rather than `allocate_ui_with_layout`, which lets
/// the cell shrink to its content and so misaligns columns when value widths
/// differ — e.g. a "–" next to "12.3k").
fn num_cell(ui: &mut egui::Ui, width: f32, value: String, tint: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::hover());
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        &value,
        egui::FontId::proportional(text::BODY),
        tint,
    );
}

/// The column header row's height.
const HEADER_ROW_HEIGHT: f32 = 16.0;

fn header_name_cell(ui: &mut egui::Ui, label: &str, dimf: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(W_NAME, HEADER_ROW_HEIGHT), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(text::MICRO),
        dim(color::TEXT_MUTED, dimf),
    );
}

fn header_num_cell(ui: &mut egui::Ui, width: f32, label: &str, dimf: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, HEADER_ROW_HEIGHT), egui::Sense::hover());
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        label,
        egui::FontId::proportional(text::MICRO),
        dim(color::TEXT_MUTED, dimf),
    );
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
    crate::ui::theme::darken(c, f)
}

/// A class's colour as egui draws it. Defined in `ui::theme`; kept under this
/// name for its existing callers (the in-match team frames among them).
pub fn class_color32(class: CharacterClass) -> egui::Color32 {
    crate::ui::theme::class_color(class)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::ability_config::load_ability_definitions;
    use crate::states::play_match::equipment::load_item_definitions;

    /// Run `f` with a [`Links`] built over the REAL ability table, so the
    /// name -> topic map under test is the one the screen actually resolves
    /// against (two shipped abilities — both Priest — carry a `": "` inside
    /// their own display name).
    fn with_links(f: impl FnOnce(&Links)) {
        let items = load_item_definitions().expect("items.ron must load");
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        let data = EncyclopediaData {
            items: &items,
            abilities: &abilities,
            item_icons: None,
            class_icons: None,
            ability_icons: None,
        };
        f(&Links::new(&data));
    }

    /// An ability whose OWN NAME contains `": "` resolves to itself.
    ///
    /// Stripping a pet prefix unconditionally turned "Power Word: Shield" into
    /// "Shield", which names nothing, so the bar rendered silently unlinked.
    #[test]
    fn colon_bearing_ability_name_links_to_itself() {
        with_links(|links| {
            assert_eq!(
                links.ability_topic("Power Word: Shield"),
                Some(Topic::Ability(AbilityType::PowerWordShield)),
            );
            assert_eq!(
                links.ability_topic("Power Word: Fortitude"),
                Some(Topic::Ability(AbilityType::PowerWordFortitude)),
            );
        });
    }

    /// A genuine pet-prefixed label still resolves to the ability behind the
    /// prefix — preferring the full label must not cost the prefix strip.
    #[test]
    fn pet_prefixed_label_strips_its_prefix() {
        with_links(|links| {
            assert_eq!(
                links.ability_topic("Felhunter: Devour Magic"),
                Some(Topic::Ability(AbilityType::DevourMagic)),
            );
        });
    }

    /// Both at once: a pet casting a colon-bearing ability. The prefix is the
    /// FIRST segment (the label is built as `"{pet}: {ability}"`), so the strip
    /// has to cut there rather than at the last colon.
    #[test]
    fn pet_prefixed_colon_bearing_ability_links() {
        with_links(|links| {
            assert_eq!(
                links.ability_topic("Felhunter: Power Word: Shield"),
                Some(Topic::Ability(AbilityType::PowerWordShield)),
            );
        });
    }

    /// A label that names no ability — auto attacks, wands — stays unlinked.
    #[test]
    fn unnamed_ability_labels_stay_unlinked() {
        with_links(|links| {
            assert_eq!(links.ability_topic("Auto Attack"), None);
            assert_eq!(links.ability_topic("Wand Shot"), None);
            assert_eq!(links.ability_topic("Felhunter: Auto Attack"), None);
        });
    }
}
