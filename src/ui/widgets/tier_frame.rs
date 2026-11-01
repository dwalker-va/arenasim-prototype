//! The tier frame: the edge an item's icon wears to say which tier the item
//! belongs to — the starting kit, a Gauntlet reward or a Champion reward.
//!
//! One frame, drawn the same wherever an item's icon is: the encyclopedia's
//! linked icon ([`super::link::paint_icon`]) for its tiles, item pages and kit
//! grids; a [`super::ListIcon`] carrying a tier for search hits and the
//! equipment rows and picker; and [`tier_badge`], the framed swatch and tier
//! name an item tooltip leads with, which is the frame's legend.
//!
//! The colour is the tier's token ([`tier_color`]); the frame is drawn INSIDE
//! the icon's rect, over its art, so an icon keeps its footprint and a clipped
//! painter never cuts the frame off.

use bevy_egui::egui;

use super::bars::icon_tile;
use crate::states::play_match::equipment::ItemTier;
use crate::ui::theme::{color, icon, radius, space, stroke, text, tier_color};

/// Stroke `tier`'s frame inside `rect`, over whatever icon is already there.
pub fn tier_frame(painter: &egui::Painter, rect: egui::Rect, tier: ItemTier) {
    painter.rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        egui::Stroke::new(stroke::TIER_FRAME, tier_color(tier)),
        egui::StrokeKind::Inside,
    );
}

/// An item's icon ([`icon_tile`]) with its tier frame, when it has a tier.
/// With none it is exactly [`icon_tile`].
pub fn tier_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    texture: Option<egui::TextureId>,
    edge: egui::Color32,
    tier: Option<ItemTier>,
) {
    icon_tile(painter, rect, texture, edge);
    if let Some(tier) = tier {
        tier_frame(painter, rect, tier);
    }
}

/// A small framed swatch and the tier's name: the line an item tooltip leads
/// with, so the frame's colour is always one hover from being named.
pub fn tier_badge(ui: &mut egui::Ui, tier: ItemTier) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(icon::XS), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, radius::SM, color::RAISED);
        tier_frame(painter, rect, tier);
        ui.label(
            egui::RichText::new(tier.name())
                .size(text::SMALL)
                .color(color::TEXT_MUTED),
        );
    })
    .response
}

/// The tier frames a draw paints: every rect stroked at the frame's width, by
/// colour. Shared by the tests that pin each surface's wiring.
#[cfg(test)]
pub(crate) fn painted_frames(mut draw: impl FnMut(&mut egui::Ui)) -> Vec<egui::Color32> {
    fn collect(shape: &egui::Shape, out: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            egui::Shape::Rect(rect) if rect.stroke.width == stroke::TIER_FRAME => {
                out.push(rect.stroke.color)
            }
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    crate::ui::install(&ctx);
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| draw(ui));
    });
    let mut out = Vec::new();
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::data;
    use crate::ui::widgets::{list_row, ListIcon, ListRow};

    /// Each tier strokes its own token, and only that one.
    #[test]
    fn a_tier_frame_strokes_its_tier_colour() {
        for (tier, token) in [
            (ItemTier::StartingKit, data::TIER_STARTING_KIT),
            (ItemTier::Gauntlet, data::TIER_GAUNTLET),
            (ItemTier::Champion, data::TIER_CHAMPION),
        ] {
            let frames = painted_frames(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(icon::LG), egui::Sense::hover());
                tier_icon(ui.painter(), rect, None, color::LINE, Some(tier));
            });
            assert_eq!(frames, vec![token], "{tier:?}");
        }
        // The control: no tier, no frame.
        let frames = painted_frames(|ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::Vec2::splat(icon::LG), egui::Sense::hover());
            tier_icon(ui.painter(), rect, None, color::LINE, None);
        });
        assert!(frames.is_empty());
    }

    /// The tooltip's badge wears the frame it names.
    #[test]
    fn the_badge_wears_its_tier_frame() {
        let frames = painted_frames(|ui| {
            tier_badge(ui, ItemTier::Champion);
        });
        assert_eq!(frames, vec![data::TIER_CHAMPION]);
    }

    /// A list row draws its icon's tier frame — the search hits, the
    /// equipment sheet and the picker — and a disabled row gives it up.
    #[test]
    fn a_list_row_draws_its_tier_frame_unless_disabled() {
        let row = |enabled: bool| {
            painted_frames(|ui| {
                let _ = list_row(
                    ui,
                    ListRow::new("Item")
                        .icon(ListIcon {
                            texture: None,
                            edge: color::LINE,
                            tier: Some(ItemTier::Gauntlet),
                        })
                        .enabled(enabled),
                );
            })
        };
        assert_eq!(row(true), vec![data::TIER_GAUNTLET]);
        assert!(row(false).is_empty());
    }
}
