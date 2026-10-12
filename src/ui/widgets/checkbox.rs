//! The checkbox: one on/off setting, a square box beside its label.
//!
//! The box is SQUARE — [`radius::SM`] corners — so it never reads as a radio
//! button. egui's stock checkbox takes its corners from the installed widget
//! style, whose [`radius::MD`] (right for a button) rounds a 16pt box into a
//! circle; that is why a screen draws this one instead.
//!
//! * idle — [`color::PANEL`] box, [`color::LINE`] edge;
//! * hovered — [`color::RAISED`] box, [`color::LINE_STRONG`] edge;
//! * checked — an [`color::ACCENT`] box with an [`color::ON_ACCENT`] tick,
//!   the chip's "on" language;
//! * disabled — drawn in place, dimmed, unclickable, checked or not.
//!
//! The label and the box are one target: clicking either toggles it.

use bevy_egui::egui;

use super::{allocate, galley, mark, Look};
use crate::ui::theme::{color, radius, size, space, stroke, text};

/// A checkbox, configured. [`checkbox`] is the one-line form.
#[must_use = "call `.show(ui)` to draw the checkbox"]
pub struct Checkbox<'a> {
    checked: &'a mut bool,
    label: &'a str,
    enabled: bool,
    preview: Option<Look>,
    driver_id: Option<&'a str>,
}

impl<'a> Checkbox<'a> {
    pub fn new(checked: &'a mut bool, label: &'a str) -> Self {
        Self {
            checked,
            label,
            enabled: true,
            preview: None,
            driver_id: None,
        }
    }

    /// Draw it disabled when `enabled` is false: dimmed, in place, unclickable.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw in `look` whatever the pointer does — the gallery's seam.
    pub fn preview(mut self, look: Look) -> Self {
        self.preview = Some(look);
        self
    }

    /// The `--ui-script` id this checkbox registers under.
    pub fn driver_id(mut self, id: &'a str) -> Self {
        self.driver_id = Some(id);
        self
    }

    /// Draw it. A click flips the bool and marks the response `changed()`.
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let label_w = if self.label.is_empty() {
            0.0
        } else {
            space::SM + galley(ui, self.label, text::BODY, color::TEXT).size().x
        };
        let (rect, mut response, look) = allocate(
            ui,
            egui::vec2(size::CHECKBOX + label_w, size::CHIP),
            self.enabled,
            self.preview,
        );
        mark(ui, rect, self.enabled, self.driver_id);

        if response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }
        let on = *self.checked;

        let none = egui::Color32::TRANSPARENT;
        let (fill, edge, tick, label) = match look {
            Look::Disabled => (color::PANEL, color::LINE, color::TEXT_DIM, color::TEXT_DIM),
            _ if on => (color::ACCENT, color::ACCENT, color::ON_ACCENT, color::TEXT),
            Look::Idle => (color::PANEL, color::LINE, none, color::TEXT),
            Look::Hovered | Look::Pressed => {
                (color::RAISED, color::LINE_STRONG, none, color::ACCENT)
            }
        };

        let painter = ui.painter_at(rect);
        let check = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - size::CHECKBOX / 2.0),
            egui::Vec2::splat(size::CHECKBOX),
        );
        painter.rect(
            check,
            radius::SM,
            fill,
            egui::Stroke::new(stroke::HAIRLINE, edge),
            egui::StrokeKind::Inside,
        );
        if on {
            let r = check.shrink(space::XS);
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(r.left(), r.center().y),
                    egui::pos2(r.center().x, r.bottom()),
                    egui::pos2(r.right(), r.top()),
                ],
                egui::Stroke::new(stroke::MARK, tick),
            ));
        }
        if !self.label.is_empty() {
            painter.text(
                egui::pos2(check.right() + space::SM, rect.center().y),
                egui::Align2::LEFT_CENTER,
                self.label,
                egui::FontId::proportional(text::BODY),
                label,
            );
        }
        response
    }
}

/// A checkbox: `if checkbox(ui, &mut looping, "Loop").changed() { ... }`.
pub fn checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> egui::Response {
    Checkbox::new(checked, label).show(ui)
}
