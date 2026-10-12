//! The button, in six variants.
//!
//! * **Primary** — the forward action. Gold fill, dark label, the tallest
//!   control on the screen; ONE per screen (decision D5).
//! * **Secondary** — the default.
//! * **Ghost** — inline and low-weight: no fill, no edge.
//! * **Danger** — removes or resets something.
//! * **Icon** — square, one glyph (`×`, `+`, `−`).
//! * **Hero** — the Hero template's menu (the main menu): Primary-sized, a
//!   translucent fill the scene shows through, and a gold ring outside the
//!   button under the pointer. Every Hero button is at least
//!   [`size::HERO_BUTTON_WIDTH`] wide, so a menu of them lines up as a column.
//!
//! Disabled is a state of every variant, not a variant of its own: a disabled
//! button stays where it was, dimmed, so nothing reflows under the cursor.

use bevy_egui::egui;

use super::{allocate, galley, mark, Look};
use crate::ui::theme::{color, radius, size, space, stroke, text};

/// Which of the six buttons. See the module docs for when each is right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Ghost,
    Danger,
    Icon,
    Hero,
}

/// What a button paints in one state.
struct Paint {
    fill: egui::Color32,
    edge: egui::Color32,
    label: egui::Color32,
}

impl Variant {
    fn height(self, compact: bool) -> f32 {
        if compact {
            return size::NAV;
        }
        match self {
            Variant::Primary | Variant::Hero => size::BUTTON_PRIMARY,
            Variant::Icon => size::BUTTON_ICON,
            Variant::Secondary | Variant::Ghost | Variant::Danger => size::BUTTON,
        }
    }

    fn text_size(self, compact: bool) -> f32 {
        match self {
            _ if compact => text::SMALL,
            Variant::Primary => text::H3,
            Variant::Hero => text::H2,
            _ => text::BODY,
        }
    }

    /// Horizontal padding either side of the label.
    fn pad(self, compact: bool) -> f32 {
        match self {
            _ if compact => space::SM,
            Variant::Primary | Variant::Hero => space::XL,
            Variant::Icon => space::XXS,
            Variant::Secondary | Variant::Ghost | Variant::Danger => space::LG,
        }
    }

    fn paint(self, look: Look) -> Paint {
        let p = |fill, edge, label| Paint { fill, edge, label };
        let none = egui::Color32::TRANSPARENT;
        if look == Look::Disabled {
            return match self {
                Variant::Ghost => p(none, none, color::TEXT_DIM),
                _ => p(color::PANEL, color::LINE, color::TEXT_DIM),
            };
        }
        match (self, look) {
            (Variant::Primary, Look::Hovered) => {
                p(color::ACCENT_RAISED, color::ACCENT_RAISED, color::ON_ACCENT)
            }
            (Variant::Primary, _) => p(color::ACCENT, color::ACCENT, color::ON_ACCENT),

            (Variant::Hero, Look::Idle) => p(color::HERO_FILL, color::LINE, color::TEXT),
            (Variant::Hero, _) => p(color::HERO_FILL, color::LINE, color::ACCENT_RAISED),

            (Variant::Ghost, Look::Idle) => p(none, none, color::TEXT_MUTED),
            (Variant::Ghost, _) => p(none, none, color::ACCENT),

            (Variant::Danger, Look::Idle) => p(color::PANEL, color::DANGER_LINE, color::DANGER),
            (Variant::Danger, _) => p(color::RAISED, color::DANGER, color::DANGER),

            (Variant::Secondary | Variant::Icon, Look::Idle) => {
                p(color::PANEL, color::LINE, color::TEXT)
            }
            (Variant::Secondary | Variant::Icon, Look::Pressed) => {
                p(color::RAISED, color::ACCENT, color::ACCENT)
            }
            (Variant::Secondary | Variant::Icon, _) => {
                p(color::RAISED, color::LINE_STRONG, color::ACCENT)
            }
        }
    }
}

/// A button, configured. [`button`] is the one-line form for the common case.
#[must_use = "call `.show(ui)` to draw the button"]
pub struct Button<'a> {
    label: &'a str,
    variant: Variant,
    enabled: bool,
    min_width: f32,
    preview: Option<Look>,
    driver_id: Option<&'a str>,
    compact: bool,
}

impl<'a> Button<'a> {
    pub fn new(label: &'a str, variant: Variant) -> Self {
        Self {
            label,
            variant,
            enabled: true,
            min_width: 0.0,
            preview: None,
            driver_id: None,
            compact: false,
        }
    }

    /// The inline size: [`size::NAV`] tall with a [`text::SMALL`] label, for
    /// a button that sits in a row of chips or a toolbar rather than leading
    /// a form (the screen header's navigation, a transport bar).
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Draw it disabled when `enabled` is false: dimmed, in place, unclickable.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// At least this wide (a column of buttons that should line up).
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }

    /// Draw in `look` whatever the pointer does — the gallery's seam.
    pub fn preview(mut self, look: Look) -> Self {
        self.preview = Some(look);
        self
    }

    /// The `--ui-script` id this button registers under.
    pub fn driver_id(mut self, id: &'a str) -> Self {
        self.driver_id = Some(id);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let v = self.variant;
        let (height, text_size, pad) = (
            v.height(self.compact),
            v.text_size(self.compact),
            v.pad(self.compact),
        );
        // Every word button speaks in capitals; an Icon button's glyph is
        // drawn as given.
        let label = match v {
            Variant::Icon => self.label.to_owned(),
            _ => self.label.to_uppercase(),
        };
        let measured = galley(ui, label.clone(), text_size, color::TEXT);
        let min_width = match v {
            Variant::Hero => self.min_width.max(size::HERO_BUTTON_WIDTH),
            _ => self.min_width,
        };
        let width = match v {
            Variant::Icon => height,
            _ => (measured.size().x + 2.0 * pad).max(min_width),
        };

        let (rect, response, look) =
            allocate(ui, egui::vec2(width, height), self.enabled, self.preview);
        mark(ui, rect, self.enabled, self.driver_id);

        let paint = v.paint(look);
        if v == Variant::Hero && look.is_lit() {
            // The Hero ring sits OUTSIDE the button, so it is painted before
            // the button's own clipped painter takes over.
            ui.painter().rect_stroke(
                rect.expand(space::XXS),
                radius::MD,
                egui::Stroke::new(stroke::MARK, color::ACCENT),
                egui::StrokeKind::Outside,
            );
        }
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            radius::MD,
            paint.fill,
            egui::Stroke::new(stroke::HAIRLINE, paint.edge),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(text_size),
            paint.label,
        );
        response
    }
}

/// A button: `button(ui, "Start match", Variant::Primary).clicked()`.
pub fn button(ui: &mut egui::Ui, label: &str, variant: Variant) -> egui::Response {
    Button::new(label, variant).show(ui)
}
