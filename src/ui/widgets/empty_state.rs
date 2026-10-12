//! The empty state: what a list shows when there is nothing in it.
//!
//! An outlined box — no fill, so it does not read as content — with a title,
//! one sentence saying why it is empty, and optionally the one action that
//! fixes it ("Clear filters"). Never a red error line: an empty list is not a
//! failure.

use bevy_egui::egui;

use super::button::{Button, Variant};
use super::panel::margin;
use crate::ui::fonts::body_family;
use crate::ui::theme::{color, radius, space, stroke, text};

/// Draw the empty state across the available width. Returns true on the frame
/// `action`'s button is clicked.
pub fn empty_state(ui: &mut egui::Ui, title: &str, body: &str, action: Option<&str>) -> bool {
    let mut clicked = false;
    egui::Frame::new()
        .stroke(egui::Stroke::new(stroke::HAIRLINE, color::LINE))
        .corner_radius(radius::MD)
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
                    clicked = Button::new(action, Variant::Secondary)
                        .driver_id("empty:action")
                        .show(ui)
                        .clicked();
                }
            });
        });
    clicked
}
