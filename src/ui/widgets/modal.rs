//! The modal: a picker or a confirm, over a dimmed screen.
//!
//! A [`color::PANEL`] card with a [`color::LINE_STRONG`] edge and
//! [`radius::LG`] corners, centred over a [`color::SCRIM`]. Its header is the
//! title and a close `×`; an optional footer holds Cancel / Confirm. It closes
//! — [`ModalResponse::close`] — on the `×`, on Esc, and on a click outside it.
//! The screen decides what closing means and stops drawing it.
//!
//! Built on egui's own [`egui::Modal`], which blocks input to the screen
//! behind it and consumes Esc only when it is the topmost modal, so a modal's
//! Esc never also reaches the screen's own Esc ladder.

use bevy_egui::egui;

use super::button::{Button, Variant};
use super::panel::{divider, margin, margin_xy};
use crate::ui::theme::{color, radius, size, space, stroke, text};

/// What a modal reported this frame.
pub struct ModalResponse<T> {
    /// What the body closure returned.
    pub inner: T,
    /// The user asked to close it: the `×`, Esc, or a click outside.
    pub close: bool,
}

type Footer<'a> = Box<dyn FnOnce(&mut egui::Ui) + 'a>;

/// A modal, configured.
#[must_use = "call `.show(ctx, body)` to draw the modal"]
pub struct Modal<'a> {
    id: egui::Id,
    title: &'a str,
    width: f32,
    footer: Option<Footer<'a>>,
}

impl<'a> Modal<'a> {
    /// `id_salt` must be unique among the modals a screen can show at once.
    pub fn new(id_salt: impl std::hash::Hash, title: &'a str) -> Self {
        Self {
            id: egui::Id::new(id_salt),
            title,
            width: size::MODAL_MAX_WIDTH,
            footer: None,
        }
    }

    /// The card's width; [`size::MODAL_MAX_WIDTH`] by default.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// A footer row under the body, laid out right to left: put the
    /// confirming action first.
    pub fn footer(mut self, footer: impl FnOnce(&mut egui::Ui) + 'a) -> Self {
        self.footer = Some(Box::new(footer));
        self
    }

    fn frame() -> egui::Frame {
        egui::Frame::new()
            .fill(color::PANEL)
            .stroke(egui::Stroke::new(stroke::HAIRLINE, color::LINE_STRONG))
            .corner_radius(radius::LG)
    }

    /// Show the modal over the whole screen.
    pub fn show<T>(
        self,
        ctx: &egui::Context,
        body: impl FnOnce(&mut egui::Ui) -> T,
    ) -> ModalResponse<T> {
        let id = self.id;
        let response = egui::Modal::new(id)
            .backdrop_color(color::SCRIM)
            .frame(Self::frame())
            .show(ctx, |ui| self.card(ui, body));
        let close = response.should_close();
        let (inner, closed_by_card) = response.inner;
        ModalResponse {
            inner,
            close: close || closed_by_card,
        }
    }

    /// Draw the card alone, in `ui`, with no scrim and no input capture. The
    /// gallery pictures the modal this way; a screen wants [`Modal::show`].
    pub fn show_card<T>(
        self,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> T,
    ) -> ModalResponse<T> {
        let (inner, close) = Self::frame().show(ui, |ui| self.card(ui, body)).inner;
        ModalResponse { inner, close }
    }

    fn card<T>(self, ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> T) -> (T, bool) {
        ui.set_width(self.width.min(ui.available_width()));
        let mut close = false;

        egui::Frame::new()
            .inner_margin(margin_xy(space::MD, space::SM))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(self.title)
                            .size(text::H3)
                            .color(color::ACCENT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = Button::new("\u{d7}", Variant::Icon)
                            .driver_id("modal:close")
                            .show(ui)
                            .clicked();
                    });
                });
            });
        divider(ui);
        let inner = egui::Frame::new()
            .inner_margin(margin(space::SM))
            .show(ui, body)
            .inner;
        if let Some(footer) = self.footer {
            divider(ui);
            egui::Frame::new()
                .inner_margin(margin(space::SM))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = space::SM;
                            footer(ui);
                        });
                    });
                });
        }
        (inner, close)
    }
}
