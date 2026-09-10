//! In-game encyclopedia — the browsable reference for everything in the game.
//!
//! This module is the navigation FRAMEWORK first and a content surface second.
//! Every later content card (classes, abilities, buffs & debuffs) plugs into
//! the same four invariants:
//!
//! 1. **[`Topic`]** addresses every encyclopedia entity. Navigation state is a
//!    stack of [`View`]s (a section tab plus an optional topic).
//! 2. **Hierarchical navigation** — section tabs, a breadcrumb trail, a Back
//!    button that pops the stack. `Esc` clears an active search, otherwise pops
//!    the stack; at the root it leaves for the main menu.
//! 3. **Search** over an extensible [registry](search::build_registry) that
//!    each section populates from its own data source. Nothing is hand-authored:
//!    item N+1 appears in search the moment it exists in `items.ron`.
//! 4. **The linked-icon widget** ([`widget`]) — every icon the encyclopedia
//!    draws shows that entity's tooltip on hover and navigates to its page on
//!    click. Tooltips reuse the game's existing text builders, so there is no
//!    second source of truth.
//!
//! ## Pure draw
//!
//! [`draw_encyclopedia`] is free of Bevy ECS types, so `tests/encyclopedia_snapshot.rs`
//! renders it offscreen through `egui_kittest` for a sub-second visual-iteration
//! loop (see CLAUDE.md, "Iterate on an egui screen fast"). [`encyclopedia_ui`]
//! is the thin Bevy wrapper that feeds it resources and applies the returned
//! [`EncyclopediaAction`].
//!
//! ## Sections
//!
//! Items is the first populated section — it subsumes the old standalone Armory
//! screen (grid, chip-bar filters and tooltips came from there) and adds the
//! per-item detail pages the Armory never had. The other three tabs render a
//! placeholder until their content cards land.

pub mod items;
pub mod search;
pub mod topic;
pub mod widget;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::configure_match_ui::ClassIcons;
use super::play_match::equipment::ItemDefinitions;
use super::view_combatant_ui::ItemIcons;
use super::GameState;

pub use items::ItemFilters;
pub use search::{build_registry, SearchEntry};
pub use topic::{Section, Topic};

// ============================================================================
// THEME
// ============================================================================
// The game's existing gold-on-near-black egui palette, extended with the two
// panel tones and the link blue the blessed encyclopedia mockup uses to
// separate chrome from content.

pub(crate) const BG: egui::Color32 = egui::Color32::from_rgb(20, 20, 30);
pub(crate) const PANEL: egui::Color32 = egui::Color32::from_rgb(30, 30, 42);
pub(crate) const PANEL_HI: egui::Color32 = egui::Color32::from_rgb(37, 37, 56);
pub(crate) const LINE: egui::Color32 = egui::Color32::from_rgb(60, 60, 80);
pub(crate) const LINE_HI: egui::Color32 = egui::Color32::from_rgb(86, 86, 122);
pub(crate) const GOLD: egui::Color32 = egui::Color32::from_rgb(230, 204, 153);
pub(crate) const TEXT: egui::Color32 = egui::Color32::from_rgb(230, 217, 191);
pub(crate) const MUTED: egui::Color32 = egui::Color32::from_rgb(142, 142, 166);
pub(crate) const DIM: egui::Color32 = egui::Color32::from_rgb(102, 102, 126);
pub(crate) const LINK: egui::Color32 = egui::Color32::from_rgb(127, 178, 229);

// ============================================================================
// NAVIGATION STATE
// ============================================================================

/// One entry on the navigation stack: a section tab, plus the topic whose
/// detail page is open within it (`None` = the section's index page).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub section: Section,
    pub topic: Option<Topic>,
}

impl View {
    pub fn index(section: Section) -> Self {
        Self { section, topic: None }
    }

    pub fn topic(topic: Topic) -> Self {
        Self { section: topic.section(), topic: Some(topic) }
    }
}

/// What the user asked the screen to do this frame. Navigation is returned
/// rather than applied inside the draw, so the pure function stays a pure
/// function of its inputs; the caller ([`encyclopedia_ui`], or a test) applies
/// it via [`EncyclopediaState::apply`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncyclopediaAction {
    /// Push a view onto the stack.
    Navigate(View),
    /// Pop the stack. At the root this means [`Self::Exit`].
    Back,
    /// Leave the encyclopedia for the main menu.
    Exit,
}

/// Everything the encyclopedia remembers between frames.
///
/// A `Resource` in the app, but plain data: the snapshot test constructs one
/// directly and drives [`draw_encyclopedia`] with it.
#[derive(Resource)]
pub struct EncyclopediaState {
    /// Navigation stack. Never empty — `stack[0]` is the root view.
    stack: Vec<View>,
    /// Global search query. Non-empty takes over the content area.
    pub search: String,
    /// Items-section chip-bar filters (ported from the retired Armory screen).
    pub item_filters: ItemFilters,
    /// Search registry, built once from the data sources. Empty until
    /// [`Self::rebuild_registry`] runs.
    pub registry: Vec<SearchEntry>,
}

impl Default for EncyclopediaState {
    fn default() -> Self {
        Self {
            // Items is the only populated section today, so it is the landing
            // tab. When the Classes section lands it becomes `Section::Classes`,
            // matching the blessed mockup's default.
            stack: vec![View::index(Section::Items)],
            search: String::new(),
            item_filters: ItemFilters::default(),
            registry: Vec::new(),
        }
    }
}

impl EncyclopediaState {
    /// The view currently on screen.
    pub fn current(&self) -> View {
        *self.stack.last().expect("encyclopedia nav stack is never empty")
    }

    /// Whether Back has anywhere to go inside the encyclopedia.
    pub fn can_go_back(&self) -> bool {
        self.stack.len() > 1
    }

    /// Apply a navigation action. Returns `true` when the encyclopedia should
    /// be left for the main menu.
    pub fn apply(&mut self, action: EncyclopediaAction) -> bool {
        match action {
            EncyclopediaAction::Navigate(view) => {
                // Re-entering the view you are already on is a no-op rather
                // than a stack entry, so Back never has to be pressed twice to
                // leave a page you never actually left.
                if self.current() != view {
                    self.stack.push(view);
                }
                self.search.clear();
                false
            }
            EncyclopediaAction::Back => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                    false
                } else {
                    true
                }
            }
            EncyclopediaAction::Exit => true,
        }
    }

    /// The Back key ladder: clear an active search first, then pop the stack,
    /// and only leave for the main menu from the root. Returns `true` on exit.
    pub fn back_key(&mut self) -> bool {
        if !self.search.trim().is_empty() {
            self.search.clear();
            return false;
        }
        self.apply(EncyclopediaAction::Back)
    }

    /// Rebuild the search registry from the live data sources.
    pub fn rebuild_registry(&mut self, items: &ItemDefinitions) {
        self.registry = build_registry(items);
    }
}

/// Read-only view of the game data the encyclopedia renders from.
///
/// Bundled into one struct so [`draw_encyclopedia`] takes plain references and
/// later sections can add fields without re-threading every call site.
pub struct EncyclopediaData<'a> {
    pub items: &'a ItemDefinitions,
    pub item_icons: Option<&'a ItemIcons>,
    pub class_icons: Option<&'a ClassIcons>,
}

// ============================================================================
// BEVY WRAPPER
// ============================================================================

/// Encyclopedia screen system. Thin wrapper: grabs the egui context and the
/// data resources, delegates the drawing to [`draw_encyclopedia`], and applies
/// the action it returns.
pub fn encyclopedia_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<EncyclopediaState>,
    mut next_state: ResMut<NextState<GameState>>,
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    item_definitions: Res<ItemDefinitions>,
    item_icons: Option<Res<ItemIcons>>,
    class_icons: Option<Res<ClassIcons>>,
) {
    use crate::keybindings::GameAction;

    // try_ctx_mut: the context dies with the primary window, and ctx_mut
    // panics on that final frame.
    let Some(ctx) = contexts.try_ctx_mut() else { return };

    // The registry is derived purely from the data sources, so building it once
    // per session is enough — nothing hand-authored, nothing to invalidate.
    if state.registry.is_empty() {
        state.rebuild_registry(&item_definitions);
    }

    if keybindings.action_just_pressed(GameAction::Back, &keyboard) && state.back_key() {
        next_state.set(GameState::MainMenu);
        return;
    }

    let data = EncyclopediaData {
        items: &item_definitions,
        item_icons: item_icons.as_deref(),
        class_icons: class_icons.as_deref(),
    };

    if let Some(action) = draw_encyclopedia(ctx, &mut state, &data) {
        if state.apply(action) {
            next_state.set(GameState::MainMenu);
        }
    }
}

// ============================================================================
// PURE DRAW
// ============================================================================

/// Render the whole encyclopedia into `ctx`, returning the navigation action
/// the user requested this frame (if any).
///
/// Deliberately free of Bevy ECS types so `tests/encyclopedia_snapshot.rs` can
/// drive it offscreen through `egui_kittest`.
pub fn draw_encyclopedia(
    ctx: &egui::Context,
    state: &mut EncyclopediaState,
    data: &EncyclopediaData,
) -> Option<EncyclopediaAction> {
    let mut style = (*ctx.style()).clone();
    // Zero-delay tooltips: hovering a linked icon must show its tooltip at
    // once — that immediacy is the point of the widget.
    style.interaction.tooltip_delay = 0.0;
    apply_palette(&mut style.visuals);
    ctx.set_style(style);

    // Split the borrows up front so the search box can be edited in the same
    // pass that reads the registry.
    let EncyclopediaState { stack, search, item_filters, registry } = state;
    let current = *stack.last().expect("encyclopedia nav stack is never empty");
    let can_go_back = stack.len() > 1;

    let mut action = None;

    egui::TopBottomPanel::top("encyclopedia_chrome")
        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
            left: 18,
            right: 18,
            top: 12,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            // --- Top bar: Back, title, search ---
            ui.horizontal(|ui| {
                // At the root the button is the way out rather than a dead
                // control — the screen must never trap a mouse-only player
                // with Esc as its only exit.
                let label = if can_go_back { "◀ BACK" } else { "◀ MAIN MENU" };
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(label).size(14.0).color(TEXT))
                            .fill(PANEL)
                            .stroke(egui::Stroke::new(1.0, LINE)),
                    )
                    .clicked()
                {
                    action = Some(if can_go_back {
                        EncyclopediaAction::Back
                    } else {
                        EncyclopediaAction::Exit
                    });
                }
                ui.add_space(10.0);
                ui.label(egui::RichText::new("ENCYCLOPEDIA").size(30.0).color(GOLD));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // "×" rather than a heavier glyph: egui's default font has
                    // no coverage for most dingbats and draws them as tofu.
                    if !search.trim().is_empty()
                        && ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("×").size(16.0).color(MUTED),
                                )
                                .fill(PANEL)
                                .stroke(egui::Stroke::new(1.0, LINE)),
                            )
                            .clicked()
                    {
                        search.clear();
                    }
                    ui.add(
                        egui::TextEdit::singleline(search)
                            .hint_text("Search everything…")
                            .desired_width(280.0),
                    );
                });
            });

            ui.add_space(8.0);

            // --- Section tabs ---
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for section in Section::all() {
                    let on = current.section == *section && search.trim().is_empty();
                    if tab_button(ui, section.label(), on).clicked() {
                        search.clear();
                        action = Some(EncyclopediaAction::Navigate(View::index(*section)));
                    }
                }
            });

            ui.add_space(6.0);

            // --- Breadcrumbs ---
            ui.horizontal(|ui| {
                if let Some(nav) = breadcrumbs(ui, current, search, data) {
                    action = Some(nav);
                }
            });
            ui.add_space(6.0);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
            left: 18,
            right: 18,
            top: 14,
            bottom: 12,
        }))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let needle = search.trim().to_lowercase();
                if !needle.is_empty() {
                    if let Some(topic) = search::render_results(ui, &needle, registry, data) {
                        action = Some(EncyclopediaAction::Navigate(View::topic(topic)));
                    }
                    return;
                }

                let nav = match current.topic {
                    Some(topic) => render_topic_page(ui, topic, data),
                    None => render_section_index(ui, current.section, item_filters, data),
                };
                if let Some(topic) = nav {
                    action = Some(EncyclopediaAction::Navigate(View::topic(topic)));
                }
            });
        });

    action
}

/// Push egui's stock widget colours onto the game's palette, so the built-in
/// widgets the screen still uses (text fields, drag values, buttons, filter
/// chips) read as part of the same screen instead of stock light-grey egui.
fn apply_palette(v: &mut egui::Visuals) {
    v.window_fill = BG;
    v.panel_fill = BG;
    // TextEdit / DragValue wells.
    v.extreme_bg_color = PANEL;
    // Selected filter chips: dark text on gold, like the mockup's active chip.
    v.selection.bg_fill = GOLD;
    v.selection.stroke = egui::Stroke::new(1.0, BG);

    let stroke = |c| egui::Stroke::new(1.0, c);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = stroke(LINE);
    v.widgets.noninteractive.fg_stroke = stroke(MUTED);
    v.widgets.inactive.bg_fill = PANEL;
    v.widgets.inactive.weak_bg_fill = PANEL;
    v.widgets.inactive.bg_stroke = stroke(LINE);
    v.widgets.inactive.fg_stroke = stroke(TEXT);
    v.widgets.hovered.bg_fill = PANEL_HI;
    v.widgets.hovered.weak_bg_fill = PANEL_HI;
    v.widgets.hovered.bg_stroke = stroke(LINE_HI);
    v.widgets.hovered.fg_stroke = stroke(GOLD);
    v.widgets.active.bg_fill = PANEL_HI;
    v.widgets.active.weak_bg_fill = PANEL_HI;
    v.widgets.active.bg_stroke = stroke(GOLD);
    v.widgets.active.fg_stroke = stroke(GOLD);
    v.widgets.open.bg_fill = PANEL_HI;
    v.widgets.open.weak_bg_fill = PANEL_HI;
    v.widgets.open.bg_stroke = stroke(LINE_HI);
    v.widgets.open.fg_stroke = stroke(TEXT);
}

/// A section tab. Painted rather than a `selectable_label` so the tab row reads
/// as chrome and cannot be confused with the filter chips below it.
fn tab_button(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let font = egui::FontId::proportional(14.5);
    let width = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), MUTED)
        .size()
        .x;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width + 28.0, 32.0), egui::Sense::click());
    let painter = ui.painter_at(rect);

    if active {
        painter.rect_filled(rect, 5.0, PANEL);
        painter.rect_stroke(rect, 5.0, egui::Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
    } else if response.hovered() {
        painter.rect_filled(rect, 5.0, PANEL);
    }
    let color = if active {
        GOLD
    } else if response.hovered() {
        TEXT
    } else {
        MUTED
    };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The `Encyclopedia › Section › Entity` trail. Every crumb but the last is a
/// link back up the hierarchy.
fn breadcrumbs(
    ui: &mut egui::Ui,
    current: View,
    search: &str,
    data: &EncyclopediaData,
) -> Option<EncyclopediaAction> {
    let mut action = None;
    let searching = !search.trim().is_empty();

    let root_is_here = !searching && current.topic.is_none();
    if crumb(ui, "Encyclopedia", root_is_here).clicked() && !root_is_here {
        action = Some(EncyclopediaAction::Navigate(View::index(current.section)));
    }

    if searching {
        sep(ui);
        crumb(ui, &format!("Search “{}”", search.trim()), true);
        return action;
    }

    sep(ui);
    let section_is_here = current.topic.is_none();
    if crumb(ui, current.section.label(), section_is_here).clicked() && !section_is_here {
        action = Some(EncyclopediaAction::Navigate(View::index(current.section)));
    }

    if let Some(topic) = current.topic {
        sep(ui);
        crumb(ui, &topic.name(data), true);
    }

    action
}

fn crumb(ui: &mut egui::Ui, text: &str, here: bool) -> egui::Response {
    let color = if here { TEXT } else { LINK };
    let label = egui::Label::new(egui::RichText::new(text).size(13.5).color(color));
    if here {
        ui.add(label)
    } else {
        ui.add(label.sense(egui::Sense::click()))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

fn sep(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("›").size(13.5).color(DIM));
}

/// A section's index page. Returns the topic to open, if one was clicked.
fn render_section_index(
    ui: &mut egui::Ui,
    section: Section,
    item_filters: &mut ItemFilters,
    data: &EncyclopediaData,
) -> Option<Topic> {
    match section {
        Section::Items => items::render_index(ui, item_filters, data),
        other => {
            render_pending_section(ui, other);
            None
        }
    }
}

/// A topic's detail page. Returns the topic to open, if a linked icon on the
/// page was clicked.
fn render_topic_page(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    match topic {
        Topic::Item(id) => items::render_detail(ui, id, data),
        other => {
            render_pending_topic(ui, other, data);
            None
        }
    }
}

/// Placeholder for a section whose content card has not landed yet. The tab and
/// its addresses exist so navigation, breadcrumbs and cross-links can be built
/// against them now.
fn render_pending_section(ui: &mut egui::Ui, section: Section) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(section.label()).size(20.0).color(GOLD));
        ui.add_space(8.0);
        ui.label(egui::RichText::new(section.pending_note()).size(14.0).color(MUTED));
    });
}

fn render_pending_topic(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) {
    widget::detail_header(ui, topic, &topic.subtitle(data), data);
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new(topic.section().pending_note())
            .size(14.0)
            .color(MUTED),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;
    use crate::states::play_match::equipment::ItemId;

    #[test]
    fn back_pops_the_stack_and_exits_only_at_the_root() {
        let mut state = EncyclopediaState::default();
        assert!(!state.can_go_back());

        assert!(!state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
            ItemId::WandOfTheInvoker
        )))));
        assert!(state.can_go_back());
        assert_eq!(state.current().section, Section::Items);

        // Cross-section link from an item page: the class page is pushed and
        // the tab follows the topic.
        assert!(!state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Class(
            CharacterClass::Mage
        )))));
        assert_eq!(state.current().section, Section::Classes);

        assert!(!state.apply(EncyclopediaAction::Back));
        assert_eq!(state.current(), View::topic(Topic::Item(ItemId::WandOfTheInvoker)));
        assert!(!state.apply(EncyclopediaAction::Back));
        assert!(!state.can_go_back());

        // At the root, Back leaves the screen.
        assert!(state.apply(EncyclopediaAction::Back));
    }

    #[test]
    fn back_key_clears_an_active_search_before_it_pops() {
        let mut state = EncyclopediaState::default();
        state.search = "bulwark".to_string();

        assert!(!state.back_key());
        assert!(state.search.is_empty());
        // Only the second press reaches the (root) stack and exits.
        assert!(state.back_key());
    }

    #[test]
    fn navigating_to_the_current_view_does_not_grow_the_stack() {
        let mut state = EncyclopediaState::default();
        state.apply(EncyclopediaAction::Navigate(View::index(Section::Items)));
        assert!(!state.can_go_back());
    }

    #[test]
    fn navigating_clears_an_active_search() {
        let mut state = EncyclopediaState::default();
        state.search = "wand".to_string();
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
            ItemId::WandOfTheInvoker,
        ))));
        assert!(state.search.is_empty());
    }
}
