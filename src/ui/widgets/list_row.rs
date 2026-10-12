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

use bevy_egui::egui;

use super::bars::icon_tile;
use super::{allocate, fitted, mark, Look};
use crate::ui::fonts::body_family;
use crate::ui::theme::{color, icon, radius, space, stroke, text};

/// A row's icon: a texture when there is one, else the shared fallback tile,
/// edged in `edge` (a class colour, or [`color::LINE`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListIcon {
    pub texture: Option<egui::TextureId>,
    pub edge: egui::Color32,
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
    let two_line = row.sub.is_some();
    let icon_len = if two_line { icon::LG } else { icon::MD };
    let name_size = if two_line { text::H3 } else { text::BODY };
    let height = icon_len + 2.0 * space::SM;
    let width = row.width.unwrap_or_else(|| ui.available_width());

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

    let mut text_left = rect.left() + space::SM;
    if let Some(row_icon) = row.icon {
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(text_left, rect.center().y - icon_len / 2.0),
            egui::Vec2::splat(icon_len),
        );
        // A disabled row gives up its colour, the icon's edge included.
        let edge = if look == Look::Disabled {
            color::LINE
        } else {
            row_icon.edge
        };
        icon_tile(&painter, icon_rect, row_icon.texture, edge);
        text_left = icon_rect.right() + space::SM;
    }

    let (name_color, sub_color, meta_color) = if look == Look::Disabled {
        (color::TEXT_DIM, color::TEXT_DIM, color::TEXT_DIM)
    } else {
        (row.name_color, color::TEXT_MUTED, color::TEXT_DIM)
    };

    // The meta keeps its natural width; the name and sub-line take the rest.
    let meta = row.meta.map(|m| {
        painter.layout_no_wrap(
            m.to_owned(),
            egui::FontId::proportional(text::SMALL),
            meta_color,
        )
    });
    let meta_w = meta.as_ref().map_or(0.0, |g| g.size().x + space::SM);
    let text_w = (rect.right() - space::SM - meta_w - text_left).max(space::XL);

    let name = fitted(
        ui,
        row.name,
        egui::FontId::proportional(name_size),
        name_color,
        text_w,
    );
    let sub = row.sub.map(|s| {
        fitted(
            ui,
            s,
            egui::FontId::new(text::SMALL, body_family(ui.ctx())),
            sub_color,
            text_w,
        )
    });
    let block_h = name.size().y + sub.as_ref().map_or(0.0, |g| g.size().y);
    let mut y = rect.center().y - block_h / 2.0;
    let name_h = name.size().y;
    painter.galley(egui::pos2(text_left, y), name, name_color);
    y += name_h;
    if let Some(sub) = sub {
        painter.galley(egui::pos2(text_left, y), sub, sub_color);
    }
    if let Some(meta) = meta {
        painter.galley(
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
