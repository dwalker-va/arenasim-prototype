//! The component kit: every piece of UI chrome a screen draws.
//!
//! A screen composes these rather than drawing a one-off, so two screens that
//! show the same thing show it the same way. Each component:
//!
//! * reads only [`crate::ui::theme`] tokens — `tests/ui_token_audit.rs` holds
//!   this module to zero literals;
//! * paints its own fill, stroke and text, so it looks the same whatever
//!   global `Style` a screen left behind;
//! * takes an optional `ui_driver` id ([`crate::ui::driver::mark`]), so a
//!   `--ui-script` can find it without the screen instrumenting it by hand;
//! * has a row in `tests/ui_gallery_snapshot.rs`, in every state it can be
//!   drawn in. A new component joins the gallery in the same commit.
//!
//! The design and the bench it was signed off against are
//! `docs/design/proposals/2026-10-11-ui-design-system.md` §2.3 and
//! `docs/design/benches/2026-10-11-ui-design-system-bench.html`.

pub mod bars;
pub mod button;
pub mod checkbox;
pub mod choice;
pub mod empty_state;
pub mod header;
pub mod link;
pub mod list_row;
pub mod modal;
pub mod panel;
pub mod stat_rows;
pub mod tabs;
pub mod tier_frame;
pub mod tooltip;

pub use bars::{class_icon, icon_tile, resource_bar};
pub use button::{button, Button, Variant};
pub use checkbox::{checkbox, Checkbox};
pub use choice::ChoiceTile;
pub use empty_state::empty_state;
pub use header::{nav_cluster, screen_header, Header, Nav, NavAction};
pub use link::{click_hint, link, link_with, paint_icon, secondary_click_chrome_hint};
pub use list_row::{list_row, ListIcon, ListRow};
pub use modal::{Modal, ModalResponse};
pub use panel::{divider, panel, panel_header, section_label};
pub use stat_rows::{stat_row, stat_rows};
pub use tabs::{chip, tabs, Chip, Tab};
pub use tier_frame::{tier_badge, tier_frame, tier_icon};
pub use tooltip::{hint_line, tooltip_frame, Tooltip};

use bevy_egui::egui;

/// The interaction state a component is drawn in.
///
/// Normally read off the pointer ([`Look::of`]). A component's `preview`
/// builder pins it instead, which is how the gallery pictures every state on
/// one page; a screen has no reason to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Idle,
    Hovered,
    Pressed,
    Disabled,
}

impl Look {
    /// The state `response` is in now, or `Disabled` when `enabled` is false.
    pub fn of(response: &egui::Response, enabled: bool) -> Self {
        if !enabled {
            Look::Disabled
        } else if response.is_pointer_button_down_on() {
            Look::Pressed
        } else if response.hovered() {
            Look::Hovered
        } else {
            Look::Idle
        }
    }

    /// The pointer is over it, pressed or not: the "lit" states.
    pub fn is_lit(self) -> bool {
        matches!(self, Look::Hovered | Look::Pressed)
    }
}

/// Allocate a clickable rect for a painted component and resolve the state to
/// draw it in. Disabled components sense hover only, so they cannot click,
/// and wear the not-allowed cursor.
pub(crate) fn allocate(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    enabled: bool,
    preview: Option<Look>,
) -> (egui::Rect, egui::Response, Look) {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response.on_hover_cursor(egui::CursorIcon::NotAllowed)
    };
    let look = match preview {
        Some(_) if !enabled => Look::Disabled,
        Some(look) => look,
        None => Look::of(&response, enabled),
    };
    (rect, response, look)
}

/// Register `rect` with the `--ui-script` driver under `id`, when there is one.
pub(crate) fn mark(ui: &egui::Ui, rect: egui::Rect, enabled: bool, id: Option<&str>) {
    if let Some(id) = id {
        crate::ui::driver::mark(ui, rect, enabled, format_args!("{id}"));
    }
}

/// One line of text laid out at `size` in `color`, unwrapped.
pub(crate) fn galley(
    ui: &egui::Ui,
    text: impl Into<String>,
    size: f32,
    color: egui::Color32,
) -> std::sync::Arc<egui::Galley> {
    ui.painter()
        .layout_no_wrap(text.into(), egui::FontId::proportional(size), color)
}

/// Text that must fit `width`: one line, ending in an ellipsis rather than
/// spilling past the component it belongs to.
pub(crate) fn fitted(
    ui: &egui::Ui,
    text: impl Into<String>,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.into(), font, color, width);
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    ui.fonts(|f| f.layout_job(job))
}

#[cfg(test)]
mod tests {
    //! The click contract every painted component shares, driven through
    //! `egui_kittest` with raw pointer events one frame at a time (the
    //! components are painted rects, so there is no label to query).

    use std::cell::Cell;

    use egui_kittest::Harness;

    use super::*;

    /// Draw one button, press and release it, and report whether it said
    /// `clicked()` on any frame.
    fn click_button(enabled: bool) -> bool {
        let rect = Cell::new(egui::Rect::NOTHING);
        let clicked = Cell::new(false);
        let mut harness = Harness::new_ui(|ui| {
            crate::ui::install(ui.ctx());
            let response = Button::new("Go", Variant::Primary)
                .enabled(enabled)
                .show(ui);
            rect.set(response.rect);
            if response.clicked() {
                clicked.set(true);
            }
        });
        harness.step();
        let pos = rect.get().center();
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(pos));
        harness.step();
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
        }
        harness.step();
        clicked.get()
    }

    /// The positive control: the same event path clicks an enabled button.
    #[test]
    fn an_enabled_button_clicks() {
        assert!(click_button(true));
    }

    /// Disabled is a state of every variant: drawn in place, never clickable.
    #[test]
    fn a_disabled_button_does_not_click() {
        assert!(!click_button(false));
    }
}
