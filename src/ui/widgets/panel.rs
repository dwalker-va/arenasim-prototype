//! Panels: the one framed surface, its header, and the rules between things.

use bevy_egui::egui;

use crate::ui::theme::{color, radius, space, stroke, text};

/// A margin of `points` on every side. Margins are whole points in egui.
pub(crate) fn margin(points: f32) -> egui::Margin {
    egui::Margin::same(points as i8)
}

/// A margin of `x` left and right, `y` top and bottom.
pub(crate) fn margin_xy(x: f32, y: f32) -> egui::Margin {
    egui::Margin::symmetric(x as i8, y as i8)
}

/// The panel frame: [`color::PANEL`] fill, a hairline [`color::LINE`] edge —
/// or `accent` (a team colour, the winner) — [`radius::MD`] corners and an
/// [`space::MD`] margin. `panel(None).show(ui, |ui| ...)`.
pub fn panel(accent: Option<egui::Color32>) -> egui::Frame {
    egui::Frame::new()
        .fill(color::PANEL)
        .stroke(egui::Stroke::new(
            stroke::HAIRLINE,
            accent.unwrap_or(color::LINE),
        ))
        .corner_radius(radius::MD)
        .inner_margin(margin(space::MD))
}

/// A panel's title row: the title at [`text::H2`] in `title_color`, and
/// `trailing` (a count, a chip, `+`/`−`) right-aligned on the same line.
pub fn panel_header(
    ui: &mut egui::Ui,
    title: &str,
    title_color: egui::Color32,
    trailing: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .size(text::H2)
                .color(title_color),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
    });
    ui.add_space(space::SM);
}

/// A small capitalised caption over a group of controls ("BUTTONS",
/// "FILTER BY SCHOOL").
pub fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label.to_uppercase())
            .size(text::SMALL)
            .color(color::TEXT_MUTED),
    );
    ui.add_space(space::XS);
}

/// A hairline [`color::LINE`] rule across the available width.
pub fn divider(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, stroke::HAIRLINE), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(stroke::HAIRLINE, color::LINE),
    );
}
