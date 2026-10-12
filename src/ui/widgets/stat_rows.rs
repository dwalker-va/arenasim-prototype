//! Key/value rows: a stat block, a detail sheet.

use bevy_egui::egui;

use crate::ui::theme::{color, space, text};

/// One row per `(key, value)`: the key at [`text::SMALL`] in
/// [`color::TEXT_MUTED`] on the left, the value in [`color::TEXT`]
/// right-aligned, so numbers line up down the column. Unframed — put it in a
/// [`super::panel`].
pub fn stat_rows<K, V>(ui: &mut egui::Ui, rows: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        for (key, value) in rows {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(key.as_ref())
                        .size(text::SMALL)
                        .color(color::TEXT_MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(value.as_ref())
                            .size(text::SMALL)
                            .color(color::TEXT),
                    );
                });
            });
        }
    });
}
