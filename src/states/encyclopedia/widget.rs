//! The linked-icon widget — the encyclopedia's core invariant.
//!
//! One shared helper renders any [`Topic`] as an icon (plus an optional label).
//! Hovering it ALWAYS shows that entity's tooltip; clicking it ALWAYS navigates
//! to that entity's page. Every icon the encyclopedia draws goes through here,
//! so there is no way to add a picture of something without also making it
//! hoverable and reachable.
//!
//! Tooltip text is never written here: each arm of [`tooltip`] delegates to the
//! game's existing builder for that entity kind, so the encyclopedia can never
//! become a second source of truth that drifts from the tooltips shown
//! elsewhere in the client.
//!
//! The helpers are written against plain `egui::Ui` and an [`EncyclopediaData`]
//! reference — no Bevy, no encyclopedia-only state — so another screen can
//! adopt them without dragging the encyclopedia in.
//!
//! Each widget returns `Some(topic)` on the frame it is clicked; callers bubble
//! that up as a navigation action.

use bevy_egui::egui;

use super::{EncyclopediaData, Topic, DIM, GOLD, LINE, LINE_HI, MUTED, PANEL, PANEL_HI, TEXT};

/// Side length of the icon on a compact linked icon.
pub const ICON_SMALL: f32 = 24.0;
/// Side length of the icon on a grid tile.
pub const ICON_TILE: f32 = 36.0;
/// Side length of the icon in a detail-page header.
pub const ICON_HEADER: f32 = 56.0;

/// Lay out text that must fit a fixed box: wraps to at most `max_rows` and
/// ends in an ellipsis rather than spilling past the tile it belongs to.
fn fitted(
    ui: &egui::Ui,
    text: String,
    size: f32,
    color: egui::Color32,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text,
        egui::FontId::proportional(size),
        color,
        width,
    );
    job.wrap.max_rows = max_rows;
    job.wrap.overflow_character = Some('…');
    ui.fonts(|f| f.layout_job(job))
}

/// Paint an entity's icon into `rect`, falling back to a neutral placeholder
/// tile when its icon resource has not been loaded (or does not exist yet).
pub fn paint_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    topic: Topic,
    data: &EncyclopediaData,
) {
    match topic.icon(data) {
        Some(texture) => {
            painter.image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(rect, 3.0, PANEL_HI);
            painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
        }
    }
}

/// The entity's tooltip. Delegates to the existing builder for each kind — the
/// encyclopedia adds only the "click to open" affordance line, which [`link`]
/// appends via [`click_hint`] so a caller-supplied body gets it too.
pub fn tooltip(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) {
    ui.set_max_width(320.0);
    match topic {
        Topic::Item(id) => match data.items.get(&id) {
            // The same builder the equipment picker and in-match UI use.
            Some(item) => super::items::render_item_tooltip(ui, item),
            None => {
                ui.label(egui::RichText::new(topic.name(data)).size(14.0).color(GOLD).strong());
            }
        },
        Topic::Class(class) => super::classes::tooltip(ui, class, data),
        // The shared `ability_text` generator — the same prose View Combatant
        // shows for the same ability.
        Topic::Ability(ability) => super::abilities::tooltip(ui, ability, data),
        // The same generated text the aura's own page shows — one builder, one
        // source of truth.
        Topic::Aura(id) => super::auras::render_tooltip(ui, id, data),
    }
}

/// The affordance line every linked tooltip ends on — the promise that the
/// thing under the cursor has a page. Written once so a caller that supplies
/// its OWN tooltip body still makes the same promise in the same words.
pub fn click_hint(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Click to open in the encyclopedia")
            .size(11.0)
            .color(DIM)
            .italics(),
    );
}

/// Attach the hover tooltip + click-to-navigate contract to an already-drawn
/// response. Every widget in this module funnels through it, and so should any
/// bespoke one a caller draws itself.
pub fn link(response: egui::Response, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    link_with(response, topic, |ui| tooltip(ui, topic, data))
}

/// [`link`] with a caller-supplied tooltip BODY.
///
/// The contract — pointing-hand cursor, a hover tooltip, the shared
/// "click to open" hint, and `Some(topic)` on the clicked frame — is identical;
/// only the text inside differs. This is the seam for a surface whose job is
/// not to be the article: View Combatant is a loadout editor, so its ability
/// rows show a one-line gist and link on to the full page rather than
/// reprinting it in a tooltip.
pub fn link_with(
    response: egui::Response,
    topic: Topic,
    body: impl FnOnce(&mut egui::Ui),
) -> Option<Topic> {
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_ui(|ui| {
            body(ui);
            click_hint(ui);
        });
    response.clicked().then_some(topic)
}

/// The line a panel whose widgets open a page on SECONDARY click puts in its
/// own chrome so right-click is discoverable at all.
///
/// The equipment surfaces are the ones that need it: left-click equips, so the
/// reference affordance there is the right-click, and the item tooltips are the
/// encyclopedia's own — not this hint's to annotate. Written once, like
/// [`click_hint`], so the equipment panel and its picker make the same promise
/// in the same words.
pub fn secondary_click_chrome_hint(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("right-click opens the encyclopedia")
            .size(11.0)
            .color(DIM)
            .italics(),
    );
}

/// Compact icon with a wrapped caption underneath — the kit-grid form.
///
/// Every cell is the SAME height whether its caption runs to one line or two,
/// so a kit grid lays out as a lattice instead of a ragged row of differently
/// tall tiles. Captions wrap to two lines and then ellipsize; the tooltip
/// always has the full name.
pub fn icon_link(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    const W: f32 = 86.0;
    const CAPTION_SIZE: f32 = 11.5;
    let galley = fitted(ui, topic.name(data), CAPTION_SIZE, TEXT, W - 6.0, 2);
    // Reserve two caption lines unconditionally — the lattice invariant.
    let two_lines = ui.fonts(|f| f.row_height(&egui::FontId::proportional(CAPTION_SIZE))) * 2.0;
    let height = ICON_TILE + 6.0 + two_lines + 8.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(W, height), egui::Sense::click());

    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 5.0, PANEL_HI);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 4.0 + ICON_TILE / 2.0),
        egui::vec2(ICON_TILE, ICON_TILE),
    );
    paint_icon(&painter, icon_rect, topic, data);
    painter.galley(
        egui::pos2(rect.center().x - galley.size().x / 2.0, icon_rect.bottom() + 6.0),
        galley,
        TEXT,
    );

    link(response, topic, data)
}

/// Wide grid tile: icon, name, one subtitle line, and an optional right-aligned
/// badge (item level, cooldown, …). The items grid is built from these.
pub fn tile(
    ui: &mut egui::Ui,
    topic: Topic,
    subtitle: &str,
    badge: Option<&str>,
    width: f32,
    data: &EncyclopediaData,
) -> Option<Topic> {
    const H: f32 = 52.0;
    const PAD: f32 = 8.0;

    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, H), egui::Sense::click());
    let painter = ui.painter_at(rect);

    let fill = if response.hovered() { PANEL_HI } else { PANEL };
    let frame = if response.hovered() { LINE_HI } else { LINE };
    painter.rect_filled(rect, 5.0, fill);
    painter.rect_stroke(rect, 5.0, egui::Stroke::new(1.0, frame), egui::StrokeKind::Inside);

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + PAD, rect.center().y - ICON_TILE / 2.0),
        egui::vec2(ICON_TILE, ICON_TILE),
    );
    paint_icon(&painter, icon_rect, topic, data);

    // Reserve the badge's width so a long name never runs under it.
    let badge_w = badge.map(|_| 46.0).unwrap_or(0.0);
    let text_left = icon_rect.right() + PAD;
    let text_w = (rect.right() - PAD - badge_w - text_left).max(20.0);

    let name = fitted(ui, topic.name(data), 13.5, topic.accent(data), text_w, 2);
    let sub = fitted(ui, subtitle.to_string(), 11.5, MUTED, text_w, 1);
    let block_h = name.size().y + sub.size().y;
    let mut y = rect.center().y - block_h / 2.0;
    painter.galley(egui::pos2(text_left, y), name.clone(), topic.accent(data));
    y += name.size().y;
    painter.galley(egui::pos2(text_left, y), sub, MUTED);

    if let Some(badge) = badge {
        painter.text(
            egui::pos2(rect.right() - PAD, rect.bottom() - PAD / 2.0),
            egui::Align2::RIGHT_BOTTOM,
            badge,
            egui::FontId::proportional(11.0),
            DIM,
        );
    }

    link(response, topic, data)
}

/// Full-width linked row: icon, name, right-aligned trailing note. Search
/// results and "applied by" lists use this.
pub fn row(
    ui: &mut egui::Ui,
    topic: Topic,
    trailing: &str,
    width: f32,
    data: &EncyclopediaData,
) -> Option<Topic> {
    const H: f32 = 30.0;
    const PAD: f32 = 8.0;

    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, H), egui::Sense::click());
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 4.0, PANEL_HI);
    }

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + PAD, rect.center().y - ICON_SMALL / 2.0),
        egui::vec2(ICON_SMALL, ICON_SMALL),
    );
    paint_icon(&painter, icon_rect, topic, data);

    // The trailing note keeps its natural width; the name takes what is left.
    let trail = (!trailing.is_empty())
        .then(|| ui.painter().layout_no_wrap(trailing.to_string(), egui::FontId::proportional(11.5), DIM));
    let trail_w = trail.as_ref().map(|g| g.size().x + PAD).unwrap_or(0.0);

    let name_left = icon_rect.right() + PAD;
    let name = fitted(
        ui,
        topic.name(data),
        13.5,
        TEXT,
        (rect.right() - PAD - trail_w - name_left).max(20.0),
        1,
    );
    painter.galley(
        egui::pos2(name_left, rect.center().y - name.size().y / 2.0),
        name,
        TEXT,
    );
    if let Some(trail) = trail {
        painter.galley(
            egui::pos2(rect.right() - PAD - trail.size().x, rect.center().y - trail.size().y / 2.0),
            trail,
            DIM,
        );
    }

    link(response, topic, data)
}

/// Small pill with an icon and a name — used for the class chips that say who
/// may use an item.
pub fn chip(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    const H: f32 = 28.0;
    const ICON: f32 = 20.0;
    const PAD: f32 = 9.0;

    let galley = ui.painter().layout_no_wrap(
        topic.name(data),
        egui::FontId::proportional(13.0),
        topic.accent(data),
    );
    let width = 4.0 + ICON + 6.0 + galley.size().x + PAD;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, H), egui::Sense::click());
    let painter = ui.painter_at(rect);

    let frame = if response.hovered() { LINE_HI } else { LINE };
    if response.hovered() {
        painter.rect_filled(rect, H / 2.0, PANEL_HI);
    }
    painter.rect_stroke(
        rect,
        H / 2.0,
        egui::Stroke::new(1.0, frame),
        egui::StrokeKind::Inside,
    );

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 4.0, rect.center().y - ICON / 2.0),
        egui::vec2(ICON, ICON),
    );
    paint_icon(&painter, icon_rect, topic, data);
    painter.galley(
        egui::pos2(icon_rect.right() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        topic.accent(data),
    );

    link(response, topic, data)
}

// ============================================================================
// PAGE FURNITURE
// ============================================================================

/// Icon + name + subtitle block at the top of a detail page.
pub fn detail_header(
    ui: &mut egui::Ui,
    topic: Topic,
    subtitle: &str,
    data: &EncyclopediaData,
) {
    ui.horizontal(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ICON_HEADER, ICON_HEADER), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        paint_icon(&painter, rect, topic, data);

        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(topic.name(data))
                    .size(24.0)
                    .color(topic.accent(data)),
            );
            if !subtitle.is_empty() {
                ui.label(egui::RichText::new(subtitle).size(13.5).color(MUTED));
            }
        });
    });
}

/// Uppercase section heading inside a page.
pub fn section_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(16.0);
    ui.label(egui::RichText::new(text).size(15.0).color(GOLD));
    ui.add_space(6.0);
}

/// Quieter heading for a subdivision INSIDE a section — the per-pet groups on
/// a class page. Muted and smaller than [`section_heading`] so the hierarchy
/// reads at a glance: gold headings are sections, grey ones are their parts.
pub fn sub_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(egui::RichText::new(text).size(12.5).color(MUTED));
    ui.add_space(4.0);
}

/// Panelled block of generated prose — an ability's mechanics text. Wider than
/// the stat block beside it because it is sentences, not numbers.
pub fn prose_block(ui: &mut egui::Ui, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let width = ui.available_width().min(560.0);
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, LINE))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(width - 32.0);
            ui.label(egui::RichText::new(text).size(13.5).color(TEXT));
        });
}

/// Panelled two-column key/value block. Values are right-aligned so numbers
/// line up down the column.
pub fn stat_rows(ui: &mut egui::Ui, id: &str, rows: &[(String, String)]) {
    if rows.is_empty() {
        return;
    }
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, LINE))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_max_width(430.0);
            egui::Grid::new(id)
                .num_columns(2)
                .min_col_width(150.0)
                .spacing(egui::vec2(22.0, 5.0))
                .show(ui, |ui| {
                    for (key, value) in rows {
                        ui.label(egui::RichText::new(key).size(13.0).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(value).size(13.5).color(TEXT));
                        });
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    //! The click contract, driven through `egui_kittest` with raw pointer
    //! events. The widgets are painted rects with no accessibility label, so
    //! there is nothing for kittest's `get_by_label` to find — the harness
    //! feeds `PointerMoved` / `PointerButton` events one frame at a time, the
    //! way a real pointer delivers them, and reads back what the widget
    //! returned on the release frame.

    use std::cell::Cell;

    use egui_kittest::Harness;

    use super::*;
    use crate::states::play_match::ability_config::load_ability_definitions;
    use crate::states::play_match::equipment::load_item_definitions;
    use crate::states::play_match::AbilityType;

    const TOPIC: Topic = Topic::Ability(AbilityType::Frostbolt);

    /// What one clickable rect wearing [`link`] reported: its screen rect
    /// (allocated by egui, so the test clicks where the widget actually is) and
    /// the widget's return value on the most recent frame.
    struct Probe {
        rect: Cell<egui::Rect>,
        out: Cell<Option<Topic>>,
    }

    /// Press and release `button` on the probe's rect, one frame per event,
    /// and return what the widget yielded on the release frame and on the idle
    /// frame after it.
    fn press_and_release(
        harness: &mut Harness<'_>,
        probe: &Probe,
        button: egui::PointerButton,
    ) -> (Option<Topic>, Option<Topic>) {
        // One frame so the rect is allocated and its position known.
        harness.step();
        let pos = probe.rect.get().center();
        let modifiers = egui::Modifiers::NONE;
        harness.input_mut().events.push(egui::Event::PointerMoved(pos));
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers,
        });
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers,
        });
        harness.step();
        let on_release = probe.out.get();
        harness.step();
        (on_release, probe.out.get())
    }

    /// Run [`link`] on a clickable rect under kittest and click it with `button`.
    fn click_link(button: egui::PointerButton) -> (Option<Topic>, Option<Topic>) {
        let items = load_item_definitions().expect("items.ron must load");
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        let data = EncyclopediaData {
            items: &items,
            abilities: &abilities,
            item_icons: None,
            class_icons: None,
            ability_icons: None,
        };
        let probe = Probe {
            rect: Cell::new(egui::Rect::NOTHING),
            out: Cell::new(None),
        };
        let mut harness = Harness::new_ui(|ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(60.0, 30.0), egui::Sense::click());
            probe.rect.set(rect);
            probe.out.set(link(response, TOPIC, &data));
        });
        press_and_release(&mut harness, &probe, button)
    }

    /// A primary click navigates: the widget yields its topic on the frame the
    /// button is released, and only on that frame — a latched `Some` would
    /// re-navigate every frame.
    #[test]
    fn link_yields_its_topic_on_the_frame_a_primary_click_releases() {
        let (on_release, frame_after) = click_link(egui::PointerButton::Primary);
        assert_eq!(on_release, Some(TOPIC));
        assert_eq!(frame_after, None, "the click must be a one-frame pulse");
    }

    /// A secondary click must NOT navigate. This is the assertion any caller
    /// that gives the other button its own meaning rests on — a picker that
    /// selects on left-click and opens the page on right-click — and it is a
    /// property of egui rather than of this module: egui 0.31
    /// `Response::clicked()` (`response.rs:153-155`) is
    /// `FAKE_PRIMARY_CLICKED || clicked_by(PointerButton::Primary)`, and
    /// `clicked_by` (`response.rs:166-168`) gates the widget's `CLICKED` flag —
    /// which egui sets on ANY button's release over the pressed widget — on
    /// `pointer.button_clicked(button)`. So the secondary release reaches the
    /// widget, and `clicked()` still says no. The primary test above is the
    /// positive control proving the same event path does register clicks.
    #[test]
    fn link_ignores_a_secondary_click() {
        let (on_release, frame_after) = click_link(egui::PointerButton::Secondary);
        assert_eq!(on_release, None, "a secondary click must not navigate");
        assert_eq!(frame_after, None);
    }
}
