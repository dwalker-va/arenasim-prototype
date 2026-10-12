//! The screen header, and the one exit model every screen shares.
//!
//! `screen_header` draws, left to right: the navigation cluster, the screen's
//! title at [`text::TITLE`] in [`color::ACCENT`], a trailing slot the screen
//! fills (a match duration, a search toggle), and a [`color::LINE`] rule
//! under the lot. Every screen gets this one header, so its exit sits in the
//! same place everywhere.
//!
//! **Every exit names its destination** — no label is hard-coded, and none
//! says only "Back" without a way out beside it (decision D2):
//!
//! * [`Nav::Exit`] — a flat screen with a fixed way out: `◀ MATCH SETUP`.
//! * [`Nav::Return`] — a flat screen whose way out is whoever OPENED it (its
//!   `return_to`, recorded by the opener's `open_from`): Results returns to
//!   the main menu today and to a run hub tomorrow, naming whichever it is
//!   (decision D4).
//! * [`Nav::Stacked`] — a screen with history (the encyclopedia): `◀ BACK`,
//!   disabled — never hidden — at the root, beside `🏠 <return_to>`.
//!
//! Esc takes the same path as the visible exit: [`Nav::on_escape`]. A screen's
//! own rungs come first (close a modal, clear a search), then the nav's.
//!
//! The cluster's `--ui-script` ids are fixed, `nav:back` and `nav:exit`: a
//! screen has exactly one header (`GameState::chrome` gives it one exit), so
//! they cannot collide. Components a screen can draw more than once — a modal,
//! an empty state — take their id from the caller instead.

use bevy_egui::egui;

use super::button::{Button, Variant};
use super::panel::divider;
use super::{galley, Look};
use crate::states::GameState;
use crate::ui::theme::{color, size, space, text};

/// The Back glyph, U+25C0: a geometric shape, not an emoji, so it renders in
/// the client face like any other character.
const BACK_GLYPH: &str = "\u{25c0}";

/// The Home glyph, U+1F3E0. Not a colour emoji here: the stack falls through
/// to egui's MONOCHROME NotoEmoji, so it tints like text (see the encyclopedia's
/// `HOME_GLYPH` for why this codepoint and not U+2302).
const HOME_GLYPH: &str = "\u{1f3e0}";

/// Where a screen's header exit goes, and how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// One exit to a fixed destination, drawn `◀ <DESTINATION>`.
    Exit(GameState),
    /// One exit to the screen that opened this one, drawn
    /// `◀ <RETURN_TO>`. The caller passes the screen's recorded `return_to`.
    Return(GameState),
    /// History: `◀ BACK` (disabled at the root) plus `🏠 <RETURN_TO>`.
    Stacked {
        can_back: bool,
        return_to: GameState,
    },
}

/// What the user asked the header for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    /// One level up the screen's own history.
    Back,
    /// Leave the screen for this one.
    Exit(GameState),
}

impl Nav {
    /// Where the way out goes.
    pub fn destination(self) -> GameState {
        match self {
            Nav::Exit(to) | Nav::Return(to) => to,
            Nav::Stacked { return_to, .. } => return_to,
        }
    }

    /// The exit button's label: the glyph and the destination's name.
    pub fn exit_label(self) -> String {
        let glyph = match self {
            Nav::Exit(_) | Nav::Return(_) => BACK_GLYPH,
            Nav::Stacked { .. } => HOME_GLYPH,
        };
        format!(
            "{glyph} {}",
            self.destination().screen_name().to_uppercase()
        )
    }

    /// What Esc does once the screen's own rungs (an open modal, a search)
    /// are spent: the same as the visible controls. Back while there is
    /// history, then out.
    pub fn on_escape(self) -> NavAction {
        match self {
            Nav::Stacked { can_back: true, .. } => NavAction::Back,
            _ => NavAction::Exit(self.destination()),
        }
    }
}

/// The header: `screen_header(ui, "Results", Nav::Return(opener), |ui| ...)`.
/// Returns the navigation the user clicked, if any; the screen applies it.
///
/// `trailing` fills the right-hand end of the title row, right to left.
pub fn screen_header(
    ui: &mut egui::Ui,
    title: &str,
    nav: Nav,
    trailing: impl FnOnce(&mut egui::Ui),
) -> Option<NavAction> {
    Header::new(title, nav).show(ui, trailing)
}

/// The navigation cluster alone, without the title row or the rule: what a
/// screen whose layout has not yet moved to [`screen_header`] draws in its own
/// header, so its exit already names its destination and registers as
/// `nav:exit`.
pub fn nav_cluster(ui: &mut egui::Ui, nav: Nav) -> Option<NavAction> {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        Header::new("", nav).nav_cluster(ui)
    })
    .inner
}

/// The header, configured. [`screen_header`] is the one-line form.
#[must_use = "call `.show(ui, trailing)` to draw the header"]
pub struct Header<'a> {
    title: &'a str,
    nav: Nav,
    preview: Option<Look>,
}

impl<'a> Header<'a> {
    pub fn new(title: &'a str, nav: Nav) -> Self {
        Self {
            title,
            nav,
            preview: None,
        }
    }

    /// Draw the navigation buttons in `look` whatever the pointer does — the
    /// gallery's seam. A disabled Back stays disabled.
    pub fn preview(mut self, look: Look) -> Self {
        self.preview = Some(look);
        self
    }

    pub fn show(
        self,
        ui: &mut egui::Ui,
        trailing: impl FnOnce(&mut egui::Ui),
    ) -> Option<NavAction> {
        let mut action = None;
        let title = self.title.to_uppercase();

        // The row is allocated at the title's own line height and centred on
        // it, so the cluster and the title share one vertical centre (a plain
        // `horizontal` gives the row only `interact_size.y`, which the title
        // overflows).
        let row_height = galley(ui, title.clone(), text::TITLE, color::ACCENT)
            .size()
            .y
            .max(size::NAV);
        ui.allocate_ui(egui::vec2(ui.available_width(), row_height), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                action = self.nav_cluster(ui);
                ui.add_space(space::LG);
                ui.label(
                    egui::RichText::new(title)
                        .size(text::TITLE)
                        .color(color::ACCENT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
            });
        });
        ui.add_space(space::MD);
        divider(ui);
        action
    }

    fn nav_button<'b>(&self, label: &'b str) -> Button<'b> {
        let button = Button::new(label, Variant::Secondary).compact();
        match self.preview {
            Some(look) => button.preview(look),
            None => button,
        }
    }

    fn nav_cluster(&self, ui: &mut egui::Ui) -> Option<NavAction> {
        let mut action = None;
        if let Nav::Stacked { can_back, .. } = self.nav {
            let back_label = format!("{BACK_GLYPH} BACK");
            // Registered even when disabled, so a script can pin "Back is dead
            // at the root" rather than only its absence.
            let mut back = self
                .nav_button(&back_label)
                .enabled(can_back)
                .driver_id("nav:back")
                .show(ui);
            if !can_back {
                back = back.on_hover_text("Nothing to go back to");
            }
            if back.clicked() {
                action = Some(NavAction::Back);
            }
        }
        let exit_label = self.nav.exit_label();
        let exit = self.nav_button(&exit_label).driver_id("nav:exit").show(ui);
        // What the exit SAYS, for a script to read back: the label is the
        // claim that it names its destination.
        crate::ui::driver::note(
            ui,
            format_args!(
                "nav:exit to={:?} label={exit_label}",
                self.nav.destination()
            ),
        );
        if exit.clicked() {
            action = Some(NavAction::Exit(self.nav.destination()));
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every label names its destination, from the destination's own
    /// `screen_name` — never a hard-coded MAIN MENU.
    #[test]
    fn every_exit_names_its_destination() {
        assert_eq!(
            Nav::Exit(GameState::ConfigureMatch).exit_label(),
            "\u{25c0} MATCH SETUP"
        );
        assert_eq!(
            Nav::Return(GameState::MainMenu).exit_label(),
            "\u{25c0} MAIN MENU"
        );
        assert_eq!(
            Nav::Return(GameState::Encyclopedia).exit_label(),
            "\u{25c0} ENCYCLOPEDIA"
        );
        assert_eq!(
            Nav::Stacked {
                can_back: false,
                return_to: GameState::Results
            }
            .exit_label(),
            "\u{1f3e0} RESULTS"
        );
    }

    /// Esc walks the same ladder as the visible controls: Back while there is
    /// history, then out to the destination the exit button names.
    #[test]
    fn escape_takes_the_same_path_as_the_buttons() {
        let deep = Nav::Stacked {
            can_back: true,
            return_to: GameState::ViewCombatant,
        };
        let root = Nav::Stacked {
            can_back: false,
            return_to: GameState::ViewCombatant,
        };
        assert_eq!(deep.on_escape(), NavAction::Back);
        assert_eq!(root.on_escape(), NavAction::Exit(GameState::ViewCombatant));
        assert_eq!(
            Nav::Return(GameState::MainMenu).on_escape(),
            NavAction::Exit(GameState::MainMenu)
        );
        assert_eq!(
            Nav::Exit(GameState::Options).on_escape(),
            NavAction::Exit(GameState::Options)
        );
    }
}
