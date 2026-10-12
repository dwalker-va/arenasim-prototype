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
            stat_row(ui, key.as_ref(), value.as_ref(), color::TEXT);
        }
    });
}

/// One [`stat_rows`] row on its own, its value in `value_color` — a data
/// colour when the value carries meaning (a stat equipment raised). Returns
/// the value's response, so a caller can explain the number on hover.
///
/// Rows drawn this way take the block's spacing from the caller; wrap a run
/// of them in a `ui.scope` with `item_spacing.y` at [`space::XS`], as
/// [`stat_rows`] does.
pub fn stat_row(
    ui: &mut egui::Ui,
    key: &str,
    value: &str,
    value_color: egui::Color32,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(key)
                .size(text::SMALL)
                .color(color::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .size(text::SMALL)
                    .color(value_color),
            )
        })
        .inner
    })
    .inner
}
