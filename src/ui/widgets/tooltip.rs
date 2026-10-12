//! The tooltip: one builder for every hover card.
//!
//! A title at [`text::H3`], an optional meta line at [`text::SMALL`] in
//! [`color::TEXT_MUTED`], an optional body in the prose family, and an
//! optional capitalised hint at [`text::MICRO`] in [`color::TEXT_DIM`] ("click
//! to open in the encyclopedia"), never wider than
//! [`size::TOOLTIP_MAX_WIDTH`].
//!
//! [`Tooltip::attach`] puts it on a response. egui draws a hover card inside
//! its own popup frame, so the card's fill and edge are the installed style's;
//! [`tooltip_frame`] is that frame as the kit defines it ([`color::BG`] fill, a
//! [`color::LINE_STRONG`] edge, [`radius::MD`] corners), for the gallery and
//! for the shared style to match.

use bevy_egui::egui;

use super::panel::margin_xy;
use crate::ui::fonts::body_family;
use crate::ui::theme::{color, radius, size, space, stroke, text};

/// The hover card's frame.
pub fn tooltip_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(color::BG)
        .stroke(egui::Stroke::new(stroke::HAIRLINE, color::LINE_STRONG))
        .corner_radius(radius::MD)
        .inner_margin(margin_xy(space::MD, space::SM))
}

/// A tooltip's contents, configured.
#[derive(Debug, Clone)]
pub struct Tooltip<'a> {
    title: &'a str,
    title_color: egui::Color32,
    meta: Option<&'a str>,
    body: Option<&'a str>,
    hint: Option<&'a str>,
}

impl<'a> Tooltip<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            title_color: color::TEXT,
            meta: None,
            body: None,
            hint: None,
        }
    }

    /// The title's colour (the entity's class or school). [`color::TEXT`] by
    /// default.
    pub fn title_color(mut self, c: egui::Color32) -> Self {
        self.title_color = c;
        self
    }

    /// One short line of facts under the title: `Energy · 275 health`.
    pub fn meta(mut self, meta: &'a str) -> Self {
        self.meta = Some(meta);
        self
    }

    /// The prose.
    pub fn body(mut self, body: &'a str) -> Self {
        self.body = Some(body);
        self
    }

    /// What clicking does, said last and quietest.
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Draw the contents into `ui` — the inside of a hover card.
    pub fn ui(self, ui: &mut egui::Ui) {
        ui.set_max_width(size::TOOLTIP_MAX_WIDTH);
        ui.label(
            egui::RichText::new(self.title)
                .size(text::H3)
                .color(self.title_color),
        );
        if let Some(meta) = self.meta {
            ui.label(
                egui::RichText::new(meta)
                    .size(text::SMALL)
                    .color(color::TEXT_MUTED),
            );
        }
        if let Some(body) = self.body {
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(body)
                    .size(text::SMALL)
                    .family(body_family(ui.ctx()))
                    .color(color::TEXT),
            );
        }
        if let Some(hint) = self.hint {
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(hint.to_uppercase())
                    .size(text::MICRO)
                    .color(color::TEXT_DIM),
            );
        }
    }

    /// Show this tooltip while `response` is hovered.
    pub fn attach(self, response: egui::Response) -> egui::Response {
        response.on_hover_ui(|ui| self.ui(ui))
    }
}
