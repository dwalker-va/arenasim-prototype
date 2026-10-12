//! In-game encyclopedia — the browsable reference for everything in the game.
//!
//! This module is the navigation FRAMEWORK first and a content surface second.
//! Every later content card (classes, abilities, buffs & debuffs) plugs into
//! the same four invariants:
//!
//! 1. **[`Topic`]** addresses every encyclopedia entity. Navigation state is a
//!    stack of [`View`]s (a section tab plus an optional topic).
//! 2. **Hierarchical navigation** — section tabs, a breadcrumb trail, and a
//!    browser-style cluster of TWO always-visible chrome buttons at the
//!    top-left, ahead of the title: *Back* is a literal history pointer that
//!    pops the stack one level (disabled, not hidden, at the root), and *Home*
//!    leaves the screen for the context that opened it, naming that
//!    destination. `Esc` walks the same ladder as Back (clear search, pop,
//!    then exit at the root).
//! 3. **Search** over an extensible [registry](search::build_registry) that
//!    each section populates from its own data source. Nothing is hand-authored:
//!    item N+1 appears in search the moment it exists in `items.ron`.
//!
//!    Results TAKE OVER the content area rather than floating in a dropdown
//!    over it, which is a deliberate divergence from the blessed HTML mockup
//!    — endorsed by the user on review. It is a choice, not a limitation: egui
//!    can float overlays (`egui::Area`, `popup_below_widget`). The takeover
//!    buys simpler `Esc` semantics (one ladder, not two) and a state the
//!    snapshot tests can render. Please do not "fix" it back to a dropdown.
//!
//!    Because the whole screen relayouts as the query changes, the search
//!    field is pinned to an ABSOLUTE egui `Id` — egui tracks keyboard focus by
//!    `Id` and derives an unpinned one from the widget's position among its
//!    siblings, so anything added or removed near the field would otherwise
//!    take its focus away mid-type. Keep the `.id(..)` if you touch that row.
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
//! All four sections are populated. Items subsumes the old standalone Armory
//! screen (grid, chip-bar filters and tooltips came from there) and adds the
//! per-item detail pages the Armory never had; classes and abilities are
//! derived wholly from `class_base_stats` and `abilities.ron`.
//!
//! Buffs & Debuffs is the catalog of NAMED auras — Rend and Corruption are two
//! entries, not one "Damage over Time" — with `AuraType` demoted to a mechanic
//! badge and the basis for sibling cross-links. See [`auras`] for why.

pub mod abilities;
pub mod auras;
pub mod classes;
pub mod items;
pub mod search;
pub mod topic;
pub mod widget;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::chrome::ScreenNav;
use super::configure_match_ui::ClassIcons;
use super::play_match::ability_config::AbilityDefinitions;
use super::play_match::equipment::ItemDefinitions;
use super::view_combatant_ui::{AbilityIcons, ItemIcons};
use super::GameState;
use crate::ui::driver as ui_driver;
use crate::ui::widgets::NavAction;

pub use abilities::AbilityFilters;
pub use auras::{AuraArt, AuraId, AuraSource, EngineAura};
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
        Self {
            section,
            topic: None,
        }
    }

    pub fn topic(topic: Topic) -> Self {
        Self {
            section: topic.section(),
            topic: Some(topic),
        }
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
    /// Leave the encyclopedia for whatever opened it (its `return_to` in
    /// [`crate::states::chrome::ScreenContext`]).
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
    /// Abilities-section chip-bar filters (owning class, spell school).
    pub ability_filters: AbilityFilters,
    /// Search registry, built once from the data sources. Empty until
    /// [`Self::rebuild_registry`] runs.
    pub registry: Vec<SearchEntry>,
}

/// The view a fresh visit starts on.
///
/// Classes is the landing tab, matching the blessed mockup: it is the
/// shallowest way into everything else, because a class page links on to every
/// ability that class has.
fn root_view() -> View {
    View::index(Section::Classes)
}

impl Default for EncyclopediaState {
    fn default() -> Self {
        Self {
            stack: vec![root_view()],
            search: String::new(),
            item_filters: ItemFilters::default(),
            ability_filters: AbilityFilters::default(),
            registry: Vec::new(),
        }
    }
}

impl EncyclopediaState {
    /// Land DIRECTLY on `topic` — the deep-link entry point every linked icon
    /// outside the encyclopedia uses, just before it opens the screen
    /// (`ScreenNav::open`, which records where Home and `Esc` return to).
    ///
    /// The topic becomes the ROOT of the navigation stack rather than a page
    /// pushed onto a section index. A deep link is entered from somewhere else,
    /// so "up one level" from it is that somewhere: `Esc` and the named Home
    /// button both put the reader straight back there, with no section index
    /// they never asked for sitting in between. Navigating ONWARD from the
    /// topic still stacks normally, so Back walks back down to it.
    pub fn open_at(&mut self, topic: Topic) {
        self.search.clear();
        self.stack.clear();
        self.stack.push(View::topic(topic));
    }

    /// Leave the encyclopedia: reset the transient view state, so the next
    /// visit starts fresh.
    ///
    /// Every exit path funnels through here, so re-entering always lands on a
    /// section index rather than on whatever page — or stale search results —
    /// the last visit ended on. (Round 1 cleared the search on the `Esc` path
    /// only, so exiting by button and re-entering showed stale results.)
    ///
    /// The stack is RESET to [`root_view`] rather than truncated to its own
    /// first entry, because [`Self::open_at`] makes a deep-linked topic the
    /// root: truncating would strand that page as the landing view of every
    /// later visit.
    pub fn leave(&mut self) {
        self.search.clear();
        self.stack.clear();
        self.stack.push(root_view());
    }

    /// The view currently on screen.
    pub fn current(&self) -> View {
        *self
            .stack
            .last()
            .expect("encyclopedia nav stack is never empty")
    }

    /// Whether Back has anywhere to go inside the encyclopedia.
    pub fn can_go_back(&self) -> bool {
        self.stack.len() > 1
    }

    /// Apply a navigation action. Returns `true` when the encyclopedia should
    /// be left for the context it was opened from, which is not necessarily
    /// the main menu.
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

    /// The encyclopedia's own rung on the Back key ladder: clear an active
    /// search. Returns `true` when there was one to clear, which takes the
    /// key; past it, `Esc` does what the header does (`chrome::escape_ladder`)
    /// — pop the stack, and only leave from the root.
    ///
    /// The always-visible Home button is the shortcut PAST this ladder, not a
    /// replacement for it: `Esc` still unwinds one level at a time, so no depth
    /// traps the user and no depth exits unexpectedly.
    pub fn clear_search(&mut self) -> bool {
        if self.search.trim().is_empty() {
            return false;
        }
        self.search.clear();
        true
    }

    /// Follow what the Back key asked for: one level up, or out. Returns
    /// `true` when the encyclopedia should be left.
    pub fn escape(&mut self, action: NavAction) -> bool {
        match action {
            NavAction::Back => self.apply(EncyclopediaAction::Back),
            NavAction::Exit(_) => true,
        }
    }

    /// Rebuild the search registry from the live data sources.
    pub fn rebuild_registry(&mut self, items: &ItemDefinitions, abilities: &AbilityDefinitions) {
        self.registry = build_registry(items, abilities);
    }
}

/// Read-only view of the game data the encyclopedia renders from.
///
/// Bundled into one struct so [`draw_encyclopedia`] takes plain references and
/// later sections can add fields without re-threading every call site.
pub struct EncyclopediaData<'a> {
    pub items: &'a ItemDefinitions,
    pub abilities: &'a AbilityDefinitions,
    pub item_icons: Option<&'a ItemIcons>,
    pub class_icons: Option<&'a ClassIcons>,
    /// Keyed by ability NAME. Auras borrow the icon of the ability that applies
    /// them, the same convention the in-match buff bar uses.
    pub ability_icons: Option<&'a AbilityIcons>,
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
    mut nav: ScreenNav,
    item_definitions: Res<ItemDefinitions>,
    ability_definitions: Res<AbilityDefinitions>,
    item_icons: Option<Res<ItemIcons>>,
    class_icons: Option<Res<ClassIcons>>,
    ability_icons: Option<Res<AbilityIcons>>,
) {
    // Esc first: it needs no egui context, so it answers on any frame.
    let return_to = nav
        .return_to(GameState::Encyclopedia)
        .expect("the encyclopedia has an exit");
    let can_back = state.can_go_back();
    if let Some(action) = nav.escape(can_back, || state.clear_search()) {
        if state.escape(action) {
            state.leave();
            nav.leave(return_to);
            return;
        }
    }

    // try_ctx_mut: the context dies with the primary window, and ctx_mut
    // panics on that final frame.
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    // The registry is derived purely from the data sources, so building it once
    // per session is enough — nothing hand-authored, nothing to invalidate.
    if state.registry.is_empty() {
        state.rebuild_registry(&item_definitions, &ability_definitions);
    }

    let data = EncyclopediaData {
        items: &item_definitions,
        abilities: &ability_definitions,
        item_icons: item_icons.as_deref(),
        class_icons: class_icons.as_deref(),
        ability_icons: ability_icons.as_deref(),
    };

    if let Some(action) = draw_encyclopedia(ctx, &mut state, &data, return_to) {
        if state.apply(action) {
            state.leave();
            nav.leave(return_to);
        }
    }
}

// ============================================================================
// PURE DRAW
// ============================================================================

/// Render the whole encyclopedia into `ctx`, returning the navigation action
/// the user requested this frame (if any). `return_to` is the screen that
/// opened it, which the Home button names.
///
/// Deliberately free of Bevy ECS types so `tests/encyclopedia_snapshot.rs` can
/// drive it offscreen through `egui_kittest`.
pub fn draw_encyclopedia(
    ctx: &egui::Context,
    state: &mut EncyclopediaState,
    data: &EncyclopediaData,
    return_to: GameState,
) -> Option<EncyclopediaAction> {
    // Split the borrows up front so the search box can be edited in the same
    // pass that reads the registry.
    let EncyclopediaState {
        stack,
        search,
        item_filters,
        ability_filters,
        registry,
    } = state;
    let current = *stack.last().expect("encyclopedia nav stack is never empty");
    let can_go_back = stack.len() > 1;
    let home_label = home_label(return_to);

    let mut action = None;

    egui::TopBottomPanel::top("encyclopedia_chrome")
        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
            left: 18,
            right: 18,
            top: 12,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            // --- Top bar: [Back][Home] title ---
            //
            // ONE navigation cluster, top-left, ahead of the title — the
            // browser toolbar the two buttons were already imitating. Back and
            // Home are SEPARATE, ALWAYS-VISIBLE affordances answering different
            // questions ("back one page" and "out of here"), and collapsing
            // them into one button (round 1 swapped the label the moment you
            // navigated anywhere) left a reader two pages deep with no
            // one-click way out. But they are ONE JOB — navigation — so they
            // sit together: Home used to float alone at the far right, which
            // split the row's two controls across the full screen width and
            // made neither read as the other's neighbour.
            //
            // Back is disabled rather than hidden at the root, exactly like a
            // browser with no history: the cluster must not reflow under the
            // cursor when the stack empties.
            //
            // The row is allocated at an EXPLICIT height — the title's own line
            // height — and laid out with `horizontal_centered`, so the cluster
            // and the title share one vertical centre. A plain `ui.horizontal`
            // gives the row only `interact_size.y` (18pt) of cross-axis space,
            // which the 30pt title overflows: the buttons ended up pinned near
            // the top of that band while the title sat 8px lower.
            let title_height = ui
                .painter()
                .layout_no_wrap(
                    TITLE.to_owned(),
                    egui::FontId::proportional(TITLE_SIZE),
                    GOLD,
                )
                .size()
                .y;
            let row_height = title_height.max(ui.spacing().interact_size.y);
            ui.allocate_ui(egui::vec2(ui.available_width(), row_height), |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = CLUSTER_GAP;

                    let back = ui
                        .add_enabled(
                            can_go_back,
                            chrome_button(BACK_LABEL, if can_go_back { TEXT } else { DIM }),
                        )
                        .on_disabled_hover_text("Nothing to go back to");
                    // Registered even when disabled, so a script can pin
                    // "Back is dead at the root" rather than only its absence.
                    ui_driver::mark(ui, back.rect, can_go_back, format_args!("enc:back"));
                    if back.clicked() {
                        action = Some(EncyclopediaAction::Back);
                    }

                    // Home NAMES its destination. Position no longer says where
                    // "out" goes now that the button sits in the cluster rather
                    // than alone at the far right, so the label has to — and
                    // the destination is whatever context opened the
                    // encyclopedia, not a fixed main menu.
                    let home = ui.add(chrome_button(&home_label, TEXT));
                    ui_driver::mark(ui, home.rect, true, format_args!("enc:home"));
                    if home.clicked() {
                        action = Some(EncyclopediaAction::Exit);
                    }

                    ui.add_space(CLUSTER_TITLE_GAP);
                    ui.label(egui::RichText::new(TITLE).size(TITLE_SIZE).color(GOLD));
                });
            });

            ui.add_space(10.0);

            // --- Search: a primary control, not an afterthought ---
            // Full-width and tall on its own row. Search is one of the four
            // framework invariants and the only way to reach an entity whose
            // section you do not know; it is sized to say so.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // The clear button is ALWAYS drawn — DISABLED while the box
                    // is empty, never removed. Two reasons, one of them a bug
                    // fix: egui numbers a widget's Id by its position in the
                    // sibling sequence, so a button that appears the instant the
                    // query becomes non-empty renumbers the field behind it and
                    // silently takes its keyboard focus with it (the field went
                    // dead mid-type). The reserved slot also stops the field
                    // jumping width on the first and last keystroke.
                    //
                    // "×" rather than a heavier glyph: egui's default font has
                    // no coverage for most dingbats and draws them as tofu.
                    let has_query = !search.trim().is_empty();
                    let clear = ui
                        .add_enabled(
                            has_query,
                            egui::Button::new(egui::RichText::new("×").size(19.0).color(MUTED))
                                .fill(PANEL)
                                .stroke(egui::Stroke::new(1.0_f32, LINE))
                                .min_size(egui::vec2(SEARCH_HEIGHT, SEARCH_HEIGHT)),
                        )
                        .on_hover_text("Clear search");
                    if clear.clicked() {
                        search.clear();
                    }
                    ui.add_space(6.0);
                    ui.add_sized(
                        [ui.available_width(), SEARCH_HEIGHT],
                        egui::TextEdit::singleline(search)
                            // An ABSOLUTE Id, not egui's positional default.
                            // The search field is the one widget on this screen
                            // whose focus must survive an arbitrary relayout of
                            // everything around it — the results take over the
                            // content area as the query changes — so it is
                            // pinned to an Id nothing else on the screen can
                            // move.
                            //
                            // `.id_salt` would ALSO have fixed the original
                            // focus bug: it resolves through
                            // `ui.make_persistent_id(salt)` = `Ui::id.with(salt)`,
                            // hashed with the CONTAINING Ui's id and not with
                            // the widget's position among its siblings. (An
                            // earlier revision of this comment claimed
                            // otherwise; it was wrong.) The absolute Id is
                            // still the better call, because a child Ui's own
                            // id DOES fold in its position in its parent — so a
                            // salted Id would stay hostage to anything
                            // conditional appearing ABOVE this row.
                            .id(egui::Id::new(SEARCH_FIELD_ID))
                            .hint_text(
                                egui::RichText::new("Search everything…")
                                    .size(16.0)
                                    .color(DIM),
                            )
                            .font(egui::FontId::proportional(16.0))
                            .margin(egui::Margin::symmetric(12, 8)),
                    );
                });
            });

            ui.add_space(10.0);

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
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let needle = search.trim().to_lowercase();
                    if !needle.is_empty() {
                        // Results TAKE OVER the content area. The blessed mockup
                        // floated them in a dropdown; this divergence is deliberate
                        // and user-endorsed (simpler Esc ladder, snapshot-testable
                        // state) — see the module docs. Not an egui limitation.
                        if let Some(topic) = search::render_results(ui, &needle, registry, data) {
                            action = Some(EncyclopediaAction::Navigate(View::topic(topic)));
                        }
                        return;
                    }

                    let nav = match current.topic {
                        Some(topic) => render_topic_page(ui, topic, data),
                        None => render_section_index(
                            ui,
                            current.section,
                            item_filters,
                            ability_filters,
                            data,
                        ),
                    };
                    if let Some(topic) = nav {
                        action = Some(EncyclopediaAction::Navigate(View::topic(topic)));
                    }
                });
        });

    action
}

/// Height of the search field and its clear button. Sized as a primary control.
const SEARCH_HEIGHT: f32 = 34.0;

/// The screen's title. Its laid-out height also sets the chrome row's height,
/// so the title and the navigation cluster share a vertical centre no matter
/// what size the title is given.
const TITLE: &str = "ENCYCLOPEDIA";
const TITLE_SIZE: f32 = 30.0;

/// Gap BETWEEN the two cluster buttons — tight, so they read as one control
/// group rather than two unrelated buttons that happen to be adjacent.
const CLUSTER_GAP: f32 = 6.0;

/// Gap between the cluster and the title. Wider than [`CLUSTER_GAP`], which is
/// what makes the pair a cluster: the eye groups by relative spacing.
const CLUSTER_TITLE_GAP: f32 = 18.0;

/// The Back button's label. The triangle is U+25C0, a GEOMETRIC SHAPE — not an
/// emoji. egui's font atlas is a single coverage channel, so a colour emoji can
/// never render as text here (it comes out as tofu or a flat silhouette);
/// glyphs on this screen must be plain font glyphs or image textures.
const BACK_LABEL: &str = "\u{25c0} BACK";

/// The Home button's glyph, U+1F3E0 HOUSE BUILDING.
///
/// It lives in the emoji block but it is NOT a colour emoji here: egui's
/// default font stack falls through to NotoEmoji-**Regular**, a MONOCHROME
/// outline font, so the house arrives as an ordinary single-channel glyph that
/// tints to the label's colour like any other character. That fallthrough is
/// the whole reason this codepoint and not U+2302 HOUSE — the "proper"
/// Miscellaneous Technical one, which nothing in the default stack covers and
/// which therefore draws as a tofu box (measured, `tests/encyclopedia_snapshot`).
/// A COLOUR emoji font would still be impossible, per [`BACK_LABEL`]; the
/// snapshot baselines are what pin the coverage.
const HOME_GLYPH: &str = "\u{1f3e0}";

/// The search field's absolute egui `Id`. Stable by construction — see the
/// comment at its `TextEdit`.
const SEARCH_FIELD_ID: &str = "encyclopedia_search_field";

/// The Home button's label: the house glyph plus the NAME of the screen it
/// leaves to.
///
/// The destination is dynamic — the encyclopedia is an informational context
/// entered from somewhere, so "out" means the main menu, the match setup, the
/// results or the combatant sheet depending on who opened it. Naming it is
/// what makes the button safe to press: in the cluster, position no longer
/// hints at where it goes.
fn home_label(return_to: GameState) -> String {
    format!("{HOME_GLYPH} {}", return_to.screen_name().to_uppercase())
}

/// A chrome button in the screen's palette — the Back and Home affordances.
fn chrome_button(label: &str, color: egui::Color32) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(label.to_string())
            .size(14.0)
            .color(color),
    )
    .fill(PANEL)
    .stroke(egui::Stroke::new(1.0_f32, LINE))
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
        painter.rect_stroke(
            rect,
            5.0,
            egui::Stroke::new(1.0_f32, LINE),
            egui::StrokeKind::Inside,
        );
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
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        color,
    );

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
    ability_filters: &mut AbilityFilters,
    data: &EncyclopediaData,
) -> Option<Topic> {
    match section {
        Section::Classes => classes::render_index(ui, data),
        Section::Abilities => abilities::render_index(ui, ability_filters, data),
        Section::Items => items::render_index(ui, item_filters, data),
        Section::Auras => auras::render_index(ui, data),
    }
}

/// A topic's detail page. Returns the topic to open, if a linked icon on the
/// page was clicked.
fn render_topic_page(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    match topic {
        Topic::Class(class) => classes::render_detail(ui, class, data),
        Topic::Ability(ability) => abilities::render_detail(ui, ability, data),
        Topic::Item(id) => items::render_detail(ui, id, data),
        Topic::Aura(id) => auras::render_detail(ui, id, data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::chrome::{escape_ladder, ScreenContext};
    use crate::states::match_config::CharacterClass;
    use crate::states::play_match::equipment::ItemId;
    use crate::states::play_match::AbilityType;
    use crate::ui::widgets::Nav;

    /// One press of the Back key, exactly as the wrapper runs it: the search
    /// rung, then the ladder the header's nav gives. True when it leaves.
    fn back_key(state: &mut EncyclopediaState) -> bool {
        let nav = Nav::Stacked {
            can_back: state.can_go_back(),
            return_to: GameState::MainMenu,
        };
        match escape_ladder(Some(nav), || state.clear_search()) {
            Some(action) => state.escape(action),
            None => false,
        }
    }

    #[test]
    fn back_pops_the_stack_and_exits_only_at_the_root() {
        let mut state = EncyclopediaState::default();
        assert!(!state.can_go_back());

        assert!(
            !state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
                ItemId::WandOfTheInvoker
            ))))
        );
        assert!(state.can_go_back());
        assert_eq!(state.current().section, Section::Items);

        // Cross-section link from an item page: the class page is pushed and
        // the tab follows the topic.
        assert!(
            !state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Class(
                CharacterClass::Mage
            ))))
        );
        assert_eq!(state.current().section, Section::Classes);

        assert!(!state.apply(EncyclopediaAction::Back));
        assert_eq!(
            state.current(),
            View::topic(Topic::Item(ItemId::WandOfTheInvoker))
        );
        assert!(!state.apply(EncyclopediaAction::Back));
        assert!(!state.can_go_back());

        // At the root, Back leaves the screen.
        assert!(state.apply(EncyclopediaAction::Back));
    }

    #[test]
    fn back_key_clears_an_active_search_before_it_pops() {
        let mut state = EncyclopediaState {
            search: "bulwark".to_string(),
            ..Default::default()
        };

        assert!(!back_key(&mut state));
        assert!(state.search.is_empty());
        // Only the second press reaches the (root) stack and exits.
        assert!(back_key(&mut state));
    }

    #[test]
    fn navigating_to_the_current_view_does_not_grow_the_stack() {
        let mut state = EncyclopediaState::default();
        state.apply(EncyclopediaAction::Navigate(View::index(Section::Classes)));
        assert!(!state.can_go_back());
    }

    #[test]
    fn leaving_clears_the_search_and_the_stack_so_re_entry_is_fresh() {
        // Round 1 cleared the search on the Esc path only, so exiting by
        // button with results on screen and coming back landed on stale ones.
        let mut state = EncyclopediaState::default();
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
            ItemId::WandOfTheInvoker,
        ))));
        state.search = "bulwark".to_string();
        assert!(state.can_go_back());

        assert!(state.apply(EncyclopediaAction::Exit));
        state.leave();

        assert!(state.search.is_empty());
        assert!(!state.can_go_back());
        assert_eq!(state.current(), View::index(Section::Classes));
    }

    #[test]
    fn the_home_button_returns_to_the_calling_context() {
        let return_to = |context: &ScreenContext| {
            context
                .return_to(GameState::Encyclopedia)
                .expect("the encyclopedia has an exit")
        };
        let mut context = ScreenContext::default();
        // Nothing opened it: the declared fallback.
        assert_eq!(home_label(return_to(&context)), "\u{1f3e0} MAIN MENU");

        // Whoever opens it is where both exit paths go.
        context.open_from(GameState::Encyclopedia, GameState::ConfigureMatch);
        assert_eq!(home_label(return_to(&context)), "\u{1f3e0} MATCH SETUP");

        let mut state = EncyclopediaState::default();
        // ...the button path,
        assert!(state.apply(EncyclopediaAction::Exit));
        // ...and the Esc-at-root path.
        assert!(back_key(&mut state));
    }

    /// Every return context that exists gets a Home label that names it. The
    /// label is the ONLY thing telling the reader where "out" goes now that the
    /// button sits in the top-left cluster instead of alone at the far right,
    /// so a context whose name renders badly here is a real defect — hence the
    /// exhaustive list rather than a spot check.
    #[test]
    fn the_home_label_names_every_return_context() {
        let expected = [
            (GameState::MainMenu, "\u{1f3e0} MAIN MENU"),
            (GameState::ConfigureMatch, "\u{1f3e0} MATCH SETUP"),
            (GameState::Results, "\u{1f3e0} RESULTS"),
            (GameState::ViewCombatant, "\u{1f3e0} COMBATANT"),
        ];
        for (from, label) in expected {
            let mut context = ScreenContext::default();
            context.open_from(GameState::Encyclopedia, from);
            assert_eq!(
                home_label(context.return_to(GameState::Encyclopedia).unwrap()),
                label
            );
        }
    }

    /// The cluster's glyphs are pinned to EXACT codepoints, because whether one
    /// renders at all is a property of egui's default font stack and not of
    /// anything in this repo. U+25C0 comes from Ubuntu-Light and U+1F3E0 from
    /// MONOCHROME NotoEmoji; the more obvious U+2302 HOUSE is covered by nothing
    /// in the stack and draws a tofu box (measured — it was the first attempt).
    ///
    /// That failure is invisible to every behavioural test, so this assertion
    /// exists to make a glyph swap a deliberate act that re-renders the
    /// snapshot baselines, which are the thing that actually shows coverage.
    ///
    /// A VARIATION SELECTOR-16 is banned outright: it asks for colour
    /// presentation, and egui's atlas is a single coverage channel that can
    /// never deliver it whatever font is loaded.
    #[test]
    fn the_cluster_glyphs_are_the_codepoints_egui_can_actually_draw() {
        assert_eq!(BACK_LABEL, "\u{25c0} BACK");
        assert_eq!(HOME_GLYPH, "\u{1f3e0}");

        for label in [BACK_LABEL.to_string(), home_label(GameState::MainMenu)] {
            assert!(
                !label.contains('\u{fe0f}'),
                "{label:?} asks for colour presentation, which egui cannot render"
            );
        }
    }

    /// Back is a literal history pointer, and the cluster shows it DISABLED —
    /// never hidden — when there is no history, so the row cannot reflow under
    /// the cursor. [`EncyclopediaState::can_go_back`] is that enabled flag; the
    /// draw feeds it straight to `add_enabled`, and the snapshot baselines pin
    /// the rendering.
    #[test]
    fn back_is_disabled_rather_than_absent_when_there_is_no_history() {
        let mut state = EncyclopediaState::default();
        assert!(!state.can_go_back(), "a fresh root has no history");

        // A deep link is a FRESH stack, so its landing page has none either.
        state.open_at(Topic::Class(CharacterClass::Mage));
        assert!(!state.can_go_back());

        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
            ItemId::WandOfTheInvoker,
        ))));
        assert!(state.can_go_back());

        // ...and popping back to the landing page disables it again.
        assert!(!state.apply(EncyclopediaAction::Back));
        assert!(!state.can_go_back());
    }

    /// `Esc` and the Back BUTTON must not diverge: the button is the same
    /// action the key sends, so at every depth they agree — pop, pop, then
    /// leave at the root. The one rung `Esc` has that the button does not is
    /// clearing an active search first (pinned separately above).
    #[test]
    fn esc_and_the_back_button_walk_the_same_ladder() {
        let route = || {
            let mut state = EncyclopediaState::default();
            state.open_at(Topic::Class(CharacterClass::Mage));
            state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
                ItemId::WandOfTheInvoker,
            ))));
            state
        };

        let mut by_key = route();
        let mut by_button = route();
        for step in 0..2 {
            assert_eq!(
                back_key(&mut by_key),
                by_button.apply(EncyclopediaAction::Back),
                "step {step}: the key and the button disagreed about leaving"
            );
            assert_eq!(by_key.current(), by_button.current(), "step {step}");
            assert_eq!(by_key.can_go_back(), by_button.can_go_back(), "step {step}");
        }
        by_key.leave();
        by_button.leave();
    }

    /// A deep link lands ON the topic and unwinds straight back to the screen
    /// that opened it — no section index the reader never asked for in
    /// between — and leaving does not strand that topic as the landing view of
    /// the next visit.
    #[test]
    fn a_deep_link_opens_on_the_topic_and_unwinds_to_its_caller() {
        let mut state = EncyclopediaState {
            search: "stale".to_string(),
            ..Default::default()
        };

        state.open_at(Topic::Class(CharacterClass::Mage));
        assert_eq!(
            state.current(),
            View::topic(Topic::Class(CharacterClass::Mage))
        );
        assert!(state.search.is_empty());

        // Onward navigation still stacks, so Back walks back down to the
        // landing topic rather than leaving from the first page.
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
            ItemId::WandOfTheInvoker,
        ))));
        assert!(state.can_go_back());
        assert!(!state.apply(EncyclopediaAction::Back));
        assert_eq!(
            state.current(),
            View::topic(Topic::Class(CharacterClass::Mage))
        );

        // At the landing topic, Back IS the way out — to the caller.
        assert!(back_key(&mut state));
        state.leave();

        // ...and the next visit starts fresh at the root, not on the Mage page.
        assert_eq!(state.current(), View::index(Section::Classes));
        assert!(!state.can_go_back());
    }

    /// The deep-link contract other screens are built on: open the
    /// encyclopedia ON a page, and the FIRST Back — button or `Esc` — hands the
    /// player back to the screen they linked from, not to a section index they
    /// never visited.
    #[test]
    fn a_deep_link_returns_to_its_caller_on_the_first_back() {
        let mut state = EncyclopediaState::default();
        state.open_at(Topic::Ability(AbilityType::Corruption));

        assert_eq!(
            state.current(),
            View::topic(Topic::Ability(AbilityType::Corruption))
        );
        // The tab follows the topic, so the chrome reads as the Abilities section.
        assert_eq!(state.current().section, Section::Abilities);
        assert!(
            !state.can_go_back(),
            "the linked page is the ROOT of the stack — there is nowhere above it"
        );

        assert!(
            back_key(&mut state),
            "Esc at the root leaves the encyclopedia"
        );
        state.leave();
    }

    /// Browsing ONWARD from a deep-linked page still unwinds the route the
    /// reader took, and only then leaves.
    #[test]
    fn browsing_on_from_a_deep_link_unwinds_before_it_leaves() {
        let mut state = EncyclopediaState::default();
        state.open_at(Topic::Ability(AbilityType::Corruption));
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Class(
            CharacterClass::Warlock,
        ))));

        assert!(
            !back_key(&mut state),
            "the first Back pops to the ability page"
        );
        assert_eq!(
            state.current(),
            View::topic(Topic::Ability(AbilityType::Corruption))
        );
        assert!(back_key(&mut state));
        state.leave();
    }

    /// A deep-linked page must not become the LANDING view for the next visit
    /// from somewhere else. `leave` resets the stack to [`root_view`] rather
    /// than truncating it, which is what makes that true once a link can set
    /// the root.
    #[test]
    fn a_deep_link_does_not_poison_the_next_visit() {
        let mut state = EncyclopediaState::default();
        state.open_at(Topic::Item(ItemId::WandOfTheInvoker));
        state.leave();

        assert_eq!(state.current(), View::index(Section::Classes));
        assert!(!state.can_go_back());
    }

    #[test]
    fn back_and_exit_are_independent_at_every_depth() {
        // Two pages deep, Back unwinds one level while Exit leaves outright —
        // neither affordance is ever the other's fallback.
        let mut state = EncyclopediaState::default();
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
            ItemId::WandOfTheInvoker,
        ))));
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Class(
            CharacterClass::Mage,
        ))));
        assert!(state.can_go_back());

        assert!(!state.apply(EncyclopediaAction::Back));
        assert!(state.can_go_back());
        assert!(state.apply(EncyclopediaAction::Exit));
    }

    /// Typing must NEVER drop the search field's keyboard focus — not on the
    /// keystroke that first fills the box, not on the one that empties it, and
    /// not when the result count changes underneath.
    ///
    /// egui derives a widget's `Id` from its position in the sibling sequence
    /// unless one is given explicitly, and focus is tracked by `Id`. The clear
    /// button appearing the moment the query became non-empty renumbered the
    /// field behind it, so the field went dead mid-type. The field now carries
    /// an absolute `Id` and the clear button's slot is always reserved.
    ///
    /// Driven through a real headless `egui::Context`: the keystrokes are input
    /// events and the text is edited by the widget itself, so this exercises the
    /// same focus bookkeeping the client does rather than asserting on a field.
    #[test]
    fn typing_never_drops_focus_from_the_search_field() {
        let items = crate::states::play_match::equipment::load_item_definitions()
            .expect("items.ron must load");
        let abilities = crate::states::play_match::ability_config::load_ability_definitions()
            .expect("abilities.ron must load");
        let mut state = EncyclopediaState::default();
        state.rebuild_registry(&items, &abilities);

        let ctx = egui::Context::default();
        let field = egui::Id::new(SEARCH_FIELD_ID);

        let frame = |state: &mut EncyclopediaState, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                let data = EncyclopediaData {
                    items: &items,
                    abilities: &abilities,
                    item_icons: None,
                    class_icons: None,
                    ability_icons: None,
                };
                let _ = draw_encyclopedia(ctx, state, &data, GameState::MainMenu);
            });
        };

        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };

        // Lay the screen out once, then focus the field by CLICKING it, the way
        // a player does — so what follows tests the real focus path rather than
        // a hand-planted memory entry.
        frame(&mut state, vec![]);
        let click_at = ctx
            .read_response(field)
            .expect("the search field is drawn every frame")
            .rect
            .center();
        let button = |pressed| egui::Event::PointerButton {
            pos: click_at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        // Hover, press and release each need their own frame: egui hit-tests
        // against the previous frame's pointer position and widget rects, and
        // only promotes a press to a click for a widget that saw the button
        // held down during its own frame.
        frame(&mut state, vec![egui::Event::PointerMoved(click_at)]);
        frame(&mut state, vec![button(true)]);
        frame(&mut state, vec![button(false)]);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(field),
            "clicking the search field did not focus it"
        );

        // Type a query with results. The FIRST keystroke is the one that used
        // to reveal the clear button and renumber the row.
        for ch in ["g", "u"] {
            frame(&mut state, vec![egui::Event::Text(ch.to_string())]);
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(field),
                "focus lost while typing '{ch}'"
            );
        }
        // The load-bearing assertion: against the old code the 'u' never
        // arrived, because the 'g' had already cost the field its focus.
        assert_eq!(state.search, "gu");

        // ...and delete it all again, crossing back to empty.
        for _ in 0..2 {
            frame(&mut state, vec![key(egui::Key::Backspace)]);
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(field),
                "focus lost on backspace"
            );
        }
        assert!(state.search.is_empty());

        // The field is still live: typing resumes without a re-click.
        frame(&mut state, vec![egui::Event::Text("w".to_string())]);
        assert_eq!(state.search, "w");
        assert_eq!(ctx.memory(|m| m.focused()), Some(field));
    }

    /// The cross-link loop the Classes and Abilities sections exist to make:
    /// a class page links to an ability, whose page links back to the class.
    /// The tab follows the topic in both directions, and Back unwinds the
    /// route the reader actually took rather than snapping to a section index.
    #[test]
    fn class_and_ability_pages_link_to_each_other_both_ways() {
        let mut state = EncyclopediaState::default();
        assert_eq!(state.current(), View::index(Section::Classes));

        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Class(
            CharacterClass::Warlock,
        ))));
        // ...out along the kit grid,
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Ability(
            AbilityType::Corruption,
        ))));
        assert_eq!(state.current().section, Section::Abilities);
        // ...and back along the owning-class chip, which is a forward
        // navigation to a page already on the stack, not a pop.
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Class(
            CharacterClass::Warlock,
        ))));
        assert_eq!(state.current().section, Section::Classes);

        state.apply(EncyclopediaAction::Back);
        assert_eq!(
            state.current(),
            View::topic(Topic::Ability(AbilityType::Corruption))
        );
    }

    #[test]
    fn navigating_clears_an_active_search() {
        let mut state = EncyclopediaState {
            search: "wand".to_string(),
            ..Default::default()
        };
        state.apply(EncyclopediaAction::Navigate(View::topic(Topic::Item(
            ItemId::WandOfTheInvoker,
        ))));
        assert!(state.search.is_empty());
    }
}
