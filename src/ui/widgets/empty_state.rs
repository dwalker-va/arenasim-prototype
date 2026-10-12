//! The empty state: what a list shows when there is nothing in it.
//!
//! A dashed outline — no fill, so it does not read as content — with a title,
//! one sentence saying why it is empty, and optionally the one action that
//! fixes it ("Clear filters"). Never a red error line: an empty list is not a
//! failure.

use bevy_egui::egui;
use bevy_egui::egui::epaint::tessellator::path::rounded_rectangle;

use super::button::{Button, Variant};
use super::panel::margin;
use crate::ui::fonts::body_family;
use crate::ui::theme::{color, radius, space, stroke, text};

/// Draw the empty state across the available width. Returns true on the frame
/// `action`'s button is clicked.
///
/// `id` names it to `--ui-script`: the action button registers as
/// `<id>:action`, so two empty lists on one screen stay distinct.
pub fn empty_state(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    body: &str,
    action: Option<&str>,
) -> bool {
    let mut clicked = false;
    let shown = egui::Frame::new()
        .inner_margin(margin(space::XL))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(title).size(text::H3).color(color::TEXT));
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(body)
                        .size(text::SMALL)
                        .family(body_family(ui.ctx()))
                        .color(color::TEXT_MUTED),
                );
                if let Some(action) = action {
                    ui.add_space(space::MD);
                    let driver_id = format!("{id}:action");
                    clicked = Button::new(action, Variant::Secondary)
                        .driver_id(&driver_id)
                        .show(ui)
                        .clicked();
                }
            });
        });
    dashed_outline(ui, shown.response.rect);
    clicked
}

/// A hairline [`color::LINE`] dashed edge round `rect`, with [`radius::MD`]
/// corners. egui frames stroke solid only, so the outline is painted as a
/// dashed path round the rounded rectangle, closed back on its first point.
fn dashed_outline(ui: &egui::Ui, rect: egui::Rect) {
    let mut path = Vec::new();
    let inset = rect.shrink(stroke::HAIRLINE / 2.0);
    rounded_rectangle(
        &mut path,
        inset,
        egui::CornerRadius::same(radius::MD).into(),
    );
    if let Some(&first) = path.first() {
        path.push(first);
    }
    ui.painter().extend(egui::Shape::dashed_line(
        &path,
        egui::Stroke::new(stroke::HAIRLINE, color::LINE),
        stroke::DASH,
        stroke::DASH_GAP,
    ));
}
