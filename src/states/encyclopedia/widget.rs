//! The encyclopedia's [`Topic`]-aware widgets — tiles, rows, chips and page
//! furniture — built on the shared kit, [`crate::ui::widgets`].
//!
//! The linked icon itself ([`link`], [`link_with`], [`tooltip`],
//! [`paint_icon`]) and its hint lines ([`click_hint`],
//! [`secondary_click_chrome_hint`]) live in the kit,
//! [`crate::ui::widgets::link`]: any screen may show something with a page,
//! and hovering it ALWAYS shows that entity's tooltip while clicking it ALWAYS
//! navigates there. They are re-exported here so existing callers are
//! unchanged. Every icon the encyclopedia draws goes through it, so there is no
//! way to add a picture of something without also making it hoverable and
//! reachable.
//!
//! What stays here is what knows about a [`Topic`]: the tiles, the kit-grid
//! icon, the class chip and the detail-page header. Everything they paint is a
//! `ui::theme` token, and the generic pieces (list rows, panels, stat rows)
//! are the kit's own.
//!
//! The helpers are written against plain `egui::Ui` and an [`EncyclopediaData`]
//! reference — no Bevy, no encyclopedia-only state — so another screen can
//! adopt them without dragging the encyclopedia in.
//!
//! Each widget returns `Some(topic)` on the frame it is clicked; callers bubble
//! that up as a navigation action.

use crate::ui::fonts::body_family;
use crate::ui::theme::{color, icon, radius, size, space, stroke, text};
use crate::ui::widgets::{self as kit, ListIcon, ListRow};
use bevy_egui::egui;

use super::{EncyclopediaData, Topic};

pub use crate::ui::widgets::link::{
    click_hint, link, link_with, paint_icon, secondary_click_chrome_hint, tooltip,
};

/// Width of one cell of a kit grid ([`icon_link`]): room for a two-line
/// caption under an [`icon::LG`] icon.
const KIT_CELL_WIDTH: f32 = 86.0;

/// Width reserved at a tile's right-hand end for its badge, so a long name
/// never runs under it.
const TILE_BADGE_WIDTH: f32 = 46.0;

/// The widest a prose block grows: sentences, so a measure the eye can track.
const PROSE_MAX_WIDTH: f32 = 560.0;

/// The widest a stat block grows: two columns of short facts.
const STAT_BLOCK_MAX_WIDTH: f32 = 430.0;

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
    let mut job =
        egui::text::LayoutJob::simple(text, egui::FontId::proportional(size), color, width);
    job.wrap.max_rows = max_rows;
    job.wrap.overflow_character = Some('…');
    ui.fonts(|f| f.layout_job(job))
}

/// Compact icon with a wrapped caption underneath — the kit-grid form.
///
/// Every cell is the SAME height whether its caption runs to one line or two,
/// so a kit grid lays out as a lattice instead of a ragged row of differently
/// tall tiles. Captions wrap to two lines and then ellipsize; the tooltip
/// always has the full name.
pub fn icon_link(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    let galley = fitted(
        ui,
        topic.name(data),
        text::SMALL,
        color::TEXT,
        KIT_CELL_WIDTH - space::SM,
        2,
    );
    // Reserve two caption lines unconditionally — the lattice invariant.
    let two_lines = ui.fonts(|f| f.row_height(&egui::FontId::proportional(text::SMALL))) * 2.0;
    let height = space::XS + icon::LG + space::XS + two_lines + space::SM;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(KIT_CELL_WIDTH, height), egui::Sense::click());

    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, radius::MD, color::RAISED);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + space::XS + icon::LG / 2.0),
        egui::Vec2::splat(icon::LG),
    );
    paint_icon(&painter, icon_rect, topic, data);
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            icon_rect.bottom() + space::XS,
        ),
        galley,
        color::TEXT,
    );

    link(response, topic, data)
}

/// Wide grid tile: icon, name, one subtitle line, and an optional right-aligned
/// badge (item level, resource, …). The items and classes grids are built from
/// these. A tile is a panel — fill and edge even at rest — because a grid of
/// them is the page's content, not a list to pick from.
pub fn tile(
    ui: &mut egui::Ui,
    topic: Topic,
    subtitle: &str,
    badge: Option<&str>,
    width: f32,
    data: &EncyclopediaData,
) -> Option<Topic> {
    let height = icon::LG + 2.0 * space::SM;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let painter = ui.painter_at(rect);

    let (fill, edge) = if response.hovered() {
        (color::RAISED, color::LINE_STRONG)
    } else {
        (color::PANEL, color::LINE)
    };
    painter.rect(
        rect,
        radius::MD,
        fill,
        egui::Stroke::new(stroke::HAIRLINE, edge),
        egui::StrokeKind::Inside,
    );

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + space::SM, rect.center().y - icon::LG / 2.0),
        egui::Vec2::splat(icon::LG),
    );
    paint_icon(&painter, icon_rect, topic, data);

    let badge_w = badge.map(|_| TILE_BADGE_WIDTH).unwrap_or(0.0);
    let text_left = icon_rect.right() + space::SM;
    let text_w = (rect.right() - space::SM - badge_w - text_left).max(space::XL);

    let name = fitted(
        ui,
        topic.name(data),
        text::BODY,
        topic.accent(data),
        text_w,
        2,
    );
    let sub = fitted(
        ui,
        subtitle.to_string(),
        text::SMALL,
        color::TEXT_MUTED,
        text_w,
        1,
    );
    let block_h = name.size().y + sub.size().y;
    let mut y = rect.center().y - block_h / 2.0;
    painter.galley(egui::pos2(text_left, y), name.clone(), topic.accent(data));
    y += name.size().y;
    painter.galley(egui::pos2(text_left, y), sub, color::TEXT_MUTED);

    if let Some(badge) = badge {
        painter.text(
            egui::pos2(rect.right() - space::SM, rect.bottom() - space::XS),
            egui::Align2::RIGHT_BOTTOM,
            badge,
            egui::FontId::proportional(text::SMALL),
            color::TEXT_DIM,
        );
    }

    link(response, topic, data)
}

/// Full-width linked row: icon, name, right-aligned trailing note. Search
/// results and "applied by" lists use this — the kit's [`kit::list_row`],
/// wearing the linked-icon contract.
pub fn row(
    ui: &mut egui::Ui,
    topic: Topic,
    trailing: &str,
    width: f32,
    data: &EncyclopediaData,
) -> Option<Topic> {
    let name = topic.name(data);
    let mut list_row = ListRow::new(&name).width(width).icon(ListIcon {
        texture: topic.icon(data),
        edge: color::LINE,
    });
    if !trailing.is_empty() {
        list_row = list_row.meta(trailing);
    }
    let response = kit::list_row(ui, list_row);
    link(response, topic, data)
}

/// Small pill with an icon and a name — used for the class chips that say who
/// may use an item, and for an ability's owning class.
pub fn chip(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    let height = icon::SM + 2.0 * space::XS;
    let galley = ui.painter().layout_no_wrap(
        topic.name(data),
        egui::FontId::proportional(text::BODY),
        topic.accent(data),
    );
    let width = space::XS + icon::SM + space::SM + galley.size().x + space::MD;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let painter = ui.painter_at(rect);

    let (fill, edge) = if response.hovered() {
        (color::RAISED, color::LINE_STRONG)
    } else {
        (egui::Color32::TRANSPARENT, color::LINE)
    };
    painter.rect(
        rect,
        height / 2.0,
        fill,
        egui::Stroke::new(stroke::HAIRLINE, edge),
        egui::StrokeKind::Inside,
    );

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + space::XS, rect.center().y - icon::SM / 2.0),
        egui::Vec2::splat(icon::SM),
    );
    paint_icon(&painter, icon_rect, topic, data);
    painter.galley(
        egui::pos2(
            icon_rect.right() + space::SM,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        topic.accent(data),
    );

    link(response, topic, data)
}

/// The caption at the head of a filter-chip row (`CLASS`, `SLOT`): small and
/// dim, so the chips after it carry the row.
///
/// Drawn at a chip's height and centred on it, so the caption sits on the
/// chips' line rather than at the top of the row.
pub fn chip_row_label(ui: &mut egui::Ui, label: &str) {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(text::SMALL),
        color::TEXT_DIM,
    );
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(galley.size().x, size::CHIP),
        egui::Sense::hover(),
    );
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_DIM,
    );
}

// ============================================================================
// PAGE FURNITURE
// ============================================================================

/// Icon + name + subtitle block at the top of a detail page.
pub fn detail_header(ui: &mut egui::Ui, topic: Topic, subtitle: &str, data: &EncyclopediaData) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(icon::HERO), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        paint_icon(&painter, rect, topic, data);

        ui.add_space(space::SM);
        ui.vertical(|ui| {
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(topic.name(data))
                    .size(text::H2)
                    .color(topic.accent(data)),
            );
            if !subtitle.is_empty() {
                ui.label(
                    egui::RichText::new(subtitle)
                        .size(text::BODY)
                        .color(color::TEXT_MUTED),
                );
            }
        });
    });
}

/// Uppercase section heading inside a page.
pub fn section_heading(ui: &mut egui::Ui, heading: &str) {
    ui.add_space(space::LG);
    ui.label(
        egui::RichText::new(heading)
            .size(text::H3)
            .color(color::ACCENT),
    );
    ui.add_space(space::SM);
}

/// Quieter heading for a subdivision INSIDE a section — the per-pet groups on
/// a class page. Muted and smaller than [`section_heading`] so the hierarchy
/// reads at a glance: gold headings are sections, grey ones are their parts.
pub fn sub_heading(ui: &mut egui::Ui, heading: &str) {
    ui.add_space(space::MD);
    ui.label(
        egui::RichText::new(heading)
            .size(text::SMALL)
            .color(color::TEXT_MUTED),
    );
    ui.add_space(space::XS);
}

/// A run of prose in the prose family (decision D3): a description, a note, a
/// rule. Labels, names and numbers stay Bold; anything a reader reads as
/// sentences comes through here.
pub fn prose(ui: &mut egui::Ui, body: &str, size: f32, text_color: egui::Color32) {
    ui.label(
        egui::RichText::new(body)
            .size(size)
            .family(body_family(ui.ctx()))
            .color(text_color),
    );
}

/// Panelled block of generated prose — an ability's mechanics text. Wider than
/// the stat block beside it because it is sentences, not numbers.
pub fn prose_block(ui: &mut egui::Ui, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    let width = ui.available_width().min(PROSE_MAX_WIDTH);
    kit::panel(None).show(ui, |ui| {
        ui.set_width(width - 2.0 * space::MD);
        prose(ui, body, text::BODY, color::TEXT);
    });
}

/// Panelled two-column key/value block — the kit's [`kit::stat_rows`] in a
/// [`kit::panel`]. Values are right-aligned so numbers line up down the
/// column.
pub fn stat_rows(ui: &mut egui::Ui, rows: &[(String, String)]) {
    if rows.is_empty() {
        return;
    }
    let width = ui.available_width().min(STAT_BLOCK_MAX_WIDTH);
    kit::panel(None).show(ui, |ui| {
        ui.set_width(width - 2.0 * space::MD);
        kit::stat_rows(ui, rows.iter().map(|(k, v)| (k, v)));
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
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(pos));
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
            // The client's font stack, as every kittest harness installs it:
            // this probe allocates a fixed-size hit rect, but the fonts are
            // what a real `link` measures, so the harness renders what the
            // client renders.
            crate::ui::install(ui.ctx());
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
