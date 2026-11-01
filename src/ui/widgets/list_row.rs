//! The list row: one clickable entry in a list of things to pick from.
//!
//! Icon, name, an optional sub-line, and right-aligned meta. Three states
//! every list shares — the class picker, the item picker, the sandbox lists,
//! the encyclopedia's search results:
//!
//! * idle — no fill, no edge, so a list reads as text;
//! * hovered — [`color::RAISED`] with a [`color::LINE_STRONG`] edge;
//! * selected — [`color::SELECTED`] with an [`color::ACCENT`] edge, whatever
//!   the pointer does.
//!
//! A row is one fixed height by default, its name and sub-line each cut to
//! one line with an ellipsis. A row whose text IS the content — the item
//! picker's stat line and proc sentence — opts into [`ListRow::wrap`] and
//! grows to fit instead.

use bevy_egui::egui;

use super::tier_frame::tier_icon;
use super::{allocate, fitted, mark, Look};
use crate::states::play_match::equipment::ItemTier;
use crate::ui::fonts::body_family;
use crate::ui::theme::{color, icon, radius, space, stroke, text};

/// A row's icon: a texture when there is one, else the shared fallback tile,
/// edged in `edge` (a class colour, or [`color::LINE`]). An item's icon also
/// names its `tier`, and wears that tier's frame ([`super::tier_frame`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListIcon {
    pub texture: Option<egui::TextureId>,
    pub edge: egui::Color32,
    pub tier: Option<ItemTier>,
}

/// A list row, configured. Draw it with [`list_row`].
#[derive(Debug, Clone)]
pub struct ListRow<'a> {
    name: &'a str,
    name_color: egui::Color32,
    sub: Option<&'a str>,
    meta: Option<&'a str>,
    icon: Option<ListIcon>,
    selected: bool,
    enabled: bool,
    wrap: bool,
    width: Option<f32>,
    preview: Option<Look>,
    driver_id: Option<&'a str>,
}

impl<'a> ListRow<'a> {
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            name_color: color::TEXT,
            sub: None,
            meta: None,
            icon: None,
            selected: false,
            enabled: true,
            wrap: false,
            width: None,
            preview: None,
            driver_id: None,
        }
    }

    /// The name's colour (a class colour, a school colour). [`color::TEXT`]
    /// by default.
    pub fn name_color(mut self, c: egui::Color32) -> Self {
        self.name_color = c;
        self
    }

    /// A second, quieter line under the name. A row with one is taller and
    /// its icon larger.
    pub fn subtitle(mut self, sub: &'a str) -> Self {
        self.sub = Some(sub);
        self
    }

    /// A short note at the right-hand end (a resource, a cooldown).
    pub fn meta(mut self, meta: &'a str) -> Self {
        self.meta = Some(meta);
        self
    }

    pub fn icon(mut self, icon: ListIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Wrap the name and the sub-line onto as many lines as they need, and
    /// grow the row to fit, instead of cutting each to one line. For a row
    /// whose words are what the reader is choosing by. The name and icon keep
    /// their one-line sizes, sub-line or not.
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    /// This wide, rather than the whole available width.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Draw in `look` whatever the pointer does — the gallery's seam.
    pub fn preview(mut self, look: Look) -> Self {
        self.preview = Some(look);
        self
    }

    /// The `--ui-script` id this row registers under.
    pub fn driver_id(mut self, id: &'a str) -> Self {
        self.driver_id = Some(id);
        self
    }
}

/// Draw `row`. `list_row(ui, ListRow::new("Warrior").sub("Melee").selected(on))`.
pub fn list_row(ui: &mut egui::Ui, row: ListRow<'_>) -> egui::Response {
    // A wrapped row is a text row: its name and icon keep the one-line
    // sizes whatever runs under them, so a list mixing rows with a sub-line
    // and rows without reads as one list.
    let two_line = row.sub.is_some() && !row.wrap;
    let icon_len = if two_line { icon::LG } else { icon::MD };
    let name_size = if two_line { text::H3 } else { text::BODY };
    let width = row.width.unwrap_or_else(|| ui.available_width());

    // The text is laid out before the row is allocated: a wrapped row's
    // height is its text's. Laid out in the idle colours and recoloured at
    // paint time, since the state is only known once the row has a response.
    let mut text_left = space::SM;
    if row.icon.is_some() {
        text_left += icon_len + space::SM;
    }
    // The meta keeps its natural width; the name and sub-line take the rest.
    let meta = row.meta.map(|m| {
        ui.painter().layout_no_wrap(
            m.to_owned(),
            egui::FontId::proportional(text::SMALL),
            color::TEXT_DIM,
        )
    });
    let meta_w = meta.as_ref().map_or(0.0, |g| g.size().x + space::SM);
    let text_w = (width - space::SM - meta_w - text_left).max(space::XL);
    let lay_out = |s: &str, font: egui::FontId| {
        if row.wrap {
            ui.fonts(|f| f.layout(s.to_owned(), font, color::TEXT, text_w))
        } else {
            fitted(ui, s, font, color::TEXT, text_w)
        }
    };
    let name = lay_out(row.name, egui::FontId::proportional(name_size));
    let sub = row
        .sub
        .map(|s| lay_out(s, egui::FontId::new(text::SMALL, body_family(ui.ctx()))));
    let block_h = name.size().y + sub.as_ref().map_or(0.0, |g| g.size().y);
    let height = if row.wrap {
        icon_len.max(block_h)
    } else {
        icon_len
    } + 2.0 * space::SM;

    let (rect, response, look) = allocate(ui, egui::vec2(width, height), row.enabled, row.preview);
    mark(ui, rect, row.enabled, row.driver_id);
    let painter = ui.painter_at(rect);

    let (fill, edge) = if row.selected && look != Look::Disabled {
        (color::SELECTED, color::ACCENT)
    } else if look.is_lit() {
        (color::RAISED, color::LINE_STRONG)
    } else {
        (egui::Color32::TRANSPARENT, egui::Color32::TRANSPARENT)
    };
    painter.rect(
        rect,
        radius::SM,
        fill,
        egui::Stroke::new(stroke::HAIRLINE, edge),
        egui::StrokeKind::Inside,
    );

    if let Some(row_icon) = row.icon {
        // A tall wrapped row keeps its icon level with the first line.
        let icon_top = if row.wrap && block_h > icon_len {
            rect.top() + space::SM
        } else {
            rect.center().y - icon_len / 2.0
        };
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + space::SM, icon_top),
            egui::Vec2::splat(icon_len),
        );
        // A disabled row gives up its colour, the icon's edge and tier frame
        // included.
        let (edge, tier) = if look == Look::Disabled {
            (color::LINE, None)
        } else {
            (row_icon.edge, row_icon.tier)
        };
        tier_icon(&painter, icon_rect, row_icon.texture, edge, tier);
    }

    let (name_color, sub_color, meta_color) = if look == Look::Disabled {
        (color::TEXT_DIM, color::TEXT_DIM, color::TEXT_DIM)
    } else {
        (row.name_color, color::TEXT_MUTED, color::TEXT_DIM)
    };

    let text_x = rect.left() + text_left;
    let mut y = rect.center().y - block_h / 2.0;
    let name_h = name.size().y;
    painter.galley_with_override_text_color(egui::pos2(text_x, y), name, name_color);
    y += name_h;
    if let Some(sub) = sub {
        painter.galley_with_override_text_color(egui::pos2(text_x, y), sub, sub_color);
    }
    if let Some(meta) = meta {
        painter.galley_with_override_text_color(
            egui::pos2(
                rect.right() - space::SM - meta.size().x,
                rect.center().y - meta.size().y / 2.0,
            ),
            meta,
            meta_color,
        );
    }

    response
}
