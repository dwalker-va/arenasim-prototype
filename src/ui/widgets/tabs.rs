//! Tabs and chips: the two "on" languages, each with one job.
//!
//! * A **tab** picks which page of a screen is showing. Active is a
//!   [`color::RAISED`] fill with a [`color::LINE_STRONG`] edge and an
//!   [`color::ACCENT`] label.
//! * A **chip** is a filter or a toggle — several can be on at once. On is a
//!   gold pill with a dark label.
//!
//! Keeping the two apart is the point: a reader never has to wonder whether a
//! gold thing is a page or a filter.

use bevy_egui::egui;

use super::{allocate, galley, mark, Look};
use crate::ui::theme::{color, radius, size, space, stroke, text};

/// One tab, configured. [`tabs`] draws a row of them.
#[must_use = "call `.show(ui)` to draw the tab"]
pub struct Tab<'a> {
    label: &'a str,
    active: bool,
    preview: Option<Look>,
    driver_id: Option<&'a str>,
}

impl<'a> Tab<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            active: false,
            preview: None,
            driver_id: None,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Draw in `look` whatever the pointer does — the gallery's seam.
    pub fn preview(mut self, look: Look) -> Self {
        self.preview = Some(look);
        self
    }

    /// The `--ui-script` id this tab registers under.
    pub fn driver_id(mut self, id: &'a str) -> Self {
        self.driver_id = Some(id);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let label = self.label.to_uppercase();
        let width = galley(ui, label.clone(), text::BODY, color::TEXT).size().x + 2.0 * space::MD;
        let (rect, response, look) = allocate(ui, egui::vec2(width, size::TAB), true, self.preview);
        mark(ui, rect, true, self.driver_id);

        let painter = ui.painter_at(rect);
        let label_color = if self.active {
            painter.rect(
                rect,
                radius::SM,
                color::RAISED,
                egui::Stroke::new(stroke::HAIRLINE, color::LINE_STRONG),
                egui::StrokeKind::Inside,
            );
            color::ACCENT
        } else if look.is_lit() {
            color::ACCENT
        } else {
            color::TEXT_MUTED
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(text::BODY),
            label_color,
        );
        response
    }
}

/// A row of tabs with `labels[active]` on. Returns the index of the tab
/// clicked this frame, if any — the active one included, which a caller may
/// treat as "back to this page's index".
pub fn tabs(ui: &mut egui::Ui, labels: &[&str], active: usize) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        for (i, label) in labels.iter().enumerate() {
            if Tab::new(label).active(i == active).show(ui).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// One chip, configured. [`chip`] is the one-line form.
#[must_use = "call `.show(ui)` to draw the chip"]
pub struct Chip<'a> {
    label: &'a str,
    on: bool,
    label_color: egui::Color32,
    enabled: bool,
    preview: Option<Look>,
    driver_id: Option<&'a str>,
}

impl<'a> Chip<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            on: false,
            label_color: color::TEXT_MUTED,
            enabled: true,
            preview: None,
            driver_id: None,
        }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// The label's colour while off (a school chip wears its school). On, it
    /// is always [`color::ON_ACCENT`].
    pub fn label_color(mut self, c: egui::Color32) -> Self {
        self.label_color = c;
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

    /// The `--ui-script` id this chip registers under.
    pub fn driver_id(mut self, id: &'a str) -> Self {
        self.driver_id = Some(id);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let width = galley(ui, self.label, text::SMALL, color::TEXT).size().x + 2.0 * space::SM;
        let (rect, response, look) = allocate(
            ui,
            egui::vec2(width, size::CHIP),
            self.enabled,
            self.preview,
        );
        mark(ui, rect, self.enabled, self.driver_id);

        let none = egui::Color32::TRANSPARENT;
        let (fill, edge, label) = match look {
            Look::Disabled => (none, color::LINE, color::TEXT_DIM),
            _ if self.on => (color::ACCENT, color::ACCENT, color::ON_ACCENT),
            Look::Idle => (none, color::LINE, self.label_color),
            Look::Hovered | Look::Pressed => (color::RAISED, color::LINE_STRONG, color::ACCENT),
        };
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            size::CHIP / 2.0,
            fill,
            egui::Stroke::new(stroke::HAIRLINE, edge),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            self.label,
            egui::FontId::proportional(text::SMALL),
            label,
        );
        response
    }
}

/// A chip: `if chip(ui, "Frost", on).clicked() { on = !on }`.
pub fn chip(ui: &mut egui::Ui, label: &str, on: bool) -> egui::Response {
    Chip::new(label).on(on).show(ui)
}
