//! The choice tile: one option of a pick-one, shown by its icon.
//!
//! An [`icon::XL`] icon over a caption of up to two lines, in a tile
//! [`size::CHOICE_TILE`] wide, so a row of them lays out as a lattice whatever
//! their names. A row of tiles is a radio select — a Rogue's opener, a Mage's
//! armor, a Hunter's pet — and the tile speaks the list row's states:
//!
//! * idle — a hairline [`color::LINE`] edge and no fill;
//! * hovered — [`color::RAISED`] with a [`color::LINE_STRONG`] edge;
//! * selected — [`color::SELECTED`] with an [`color::ACCENT`] edge and an
//!   accent caption, whatever the pointer does;
//! * disabled — dimmed, in place, unclickable.
//!
//! What a tile does on hover (a summary of the ability it picks) is the
//! caller's: attach it to the returned response.

use bevy_egui::egui;

use super::bars::icon_tile;
use super::{allocate, mark, Look};
use crate::ui::theme::{color, icon, radius, size, space, stroke, text};

/// The most lines a tile's caption runs to before it ends in an ellipsis.
const CAPTION_ROWS: usize = 2;

/// The caption's font.
fn caption_font() -> egui::FontId {
    egui::FontId::proportional(text::SMALL)
}

/// A choice tile, configured. Draw it with [`ChoiceTile::show`].
#[must_use = "call `.show(ui)` to draw the tile"]
pub struct ChoiceTile<'a> {
    label: &'a str,
    texture: Option<egui::TextureId>,
    selected: bool,
    enabled: bool,
    preview: Option<Look>,
    driver_id: Option<&'a str>,
}

impl<'a> ChoiceTile<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            texture: None,
            selected: false,
            enabled: true,
            preview: None,
            driver_id: None,
        }
    }

    /// The option's art. Without one the tile draws the shared fallback.
    pub fn texture(mut self, texture: Option<egui::TextureId>) -> Self {
        self.texture = texture;
        self
    }

    /// This is the option currently chosen.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw in `look` whatever the pointer does — the gallery's seam.
    pub fn preview(mut self, look: Look) -> Self {
        self.preview = Some(look);
        self
    }

    /// The `--ui-script` id this tile registers under.
    pub fn driver_id(mut self, id: &'a str) -> Self {
        self.driver_id = Some(id);
        self
    }

    /// How tall every tile is drawn in `ui`, whatever its label: the icon and
    /// two caption lines. For a caller lining something up beside a row of
    /// them.
    pub fn height(ui: &egui::Ui) -> f32 {
        // Two caption lines are reserved whatever the label: the lattice.
        let caption_h = ui.fonts(|f| f.row_height(&caption_font())) * CAPTION_ROWS as f32;
        space::SM + icon::XL + space::XS + caption_h + space::SM
    }

    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let caption_w = size::CHOICE_TILE - 2.0 * space::XS;
        let font = caption_font();
        let height = Self::height(ui);

        let (rect, response, look) = allocate(
            ui,
            egui::vec2(size::CHOICE_TILE, height),
            self.enabled,
            self.preview,
        );
        mark(ui, rect, self.enabled, self.driver_id);
        let painter = ui.painter_at(rect);

        let none = egui::Color32::TRANSPARENT;
        let (fill, edge, caption) = match look {
            Look::Disabled => (none, color::LINE, color::TEXT_DIM),
            _ if self.selected => (color::SELECTED, color::ACCENT, color::ACCENT),
            Look::Idle => (none, color::LINE, color::TEXT),
            Look::Hovered | Look::Pressed => (color::RAISED, color::LINE_STRONG, color::TEXT),
        };
        painter.rect(
            rect,
            radius::MD,
            fill,
            egui::Stroke::new(stroke::HAIRLINE, edge),
            egui::StrokeKind::Inside,
        );

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + space::SM + icon::XL / 2.0),
            egui::Vec2::splat(icon::XL),
        );
        icon_tile(&painter, icon_rect, self.texture, color::LINE);

        let mut job =
            egui::text::LayoutJob::simple(self.label.to_owned(), font, caption, caption_w);
        job.wrap.max_rows = CAPTION_ROWS;
        job.wrap.overflow_character = Some('\u{2026}');
        job.halign = egui::Align::Center;
        let galley = ui.fonts(|f| f.layout_job(job));
        painter.galley(
            egui::pos2(rect.center().x, icon_rect.bottom() + space::XS),
            galley,
            caption,
        );

        response
    }
}
