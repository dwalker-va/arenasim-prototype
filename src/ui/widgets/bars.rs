//! The resource bar, and the icon with its one fallback.

use bevy_egui::egui;

use crate::states::match_config::CharacterClass;
use crate::ui::theme::{class_color, color, radius, size, stroke};

/// The whole of a texture, as UV coordinates.
const FULL_UV: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// A horizontal bar, `width` long and [`size::BAR`] thick: `fraction` (clamped
/// to 0..=1) of it filled with `fill` over the chrome
/// [`color::TRACK`]. A menu bar: the HUD draws its own.
pub fn resource_bar(
    ui: &mut egui::Ui,
    fraction: f32,
    fill: egui::Color32,
    width: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, size::BAR), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, radius::SM, color::TRACK);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let mut filled = rect;
        filled.set_width(rect.width() * fraction);
        painter.rect_filled(filled, radius::SM, fill);
    }
    response
}

/// Paint an icon into `rect`: the texture when there is one, otherwise the
/// one fallback every surface shares — a [`color::RAISED`] tile with a
/// hairline `edge` (a class colour, or [`color::LINE`] for no meaning).
pub fn icon_tile(
    painter: &egui::Painter,
    rect: egui::Rect,
    texture: Option<egui::TextureId>,
    edge: egui::Color32,
) {
    match texture {
        Some(texture) => {
            painter.image(texture, rect, FULL_UV, color::UNTINTED);
        }
        None => {
            painter.rect_filled(rect, radius::SM, color::RAISED);
            painter.rect_stroke(
                rect,
                radius::SM,
                egui::Stroke::new(stroke::HAIRLINE, edge),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// A class's icon at `edge_len` (an [`crate::ui::theme::icon`] step), falling
/// back to a tile edged in the class's colour while its art is not loaded.
pub fn class_icon(
    ui: &mut egui::Ui,
    class: CharacterClass,
    texture: Option<egui::TextureId>,
    edge_len: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(edge_len), egui::Sense::hover());
    icon_tile(ui.painter(), rect, texture, class_color(class));
    response
}
