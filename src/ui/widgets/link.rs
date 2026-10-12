//! The linked icon: the contract that anything with a page can be hovered and
//! clicked.
//!
//! [`link`] attaches it to an already-drawn response. Hovering ALWAYS shows
//! that entity's tooltip; clicking ALWAYS yields its [`Topic`], which the
//! caller turns into navigation to its encyclopedia page. Every icon the
//! encyclopedia draws, and every one Results and View Combatant borrow, goes
//! through here, so there is no way to add a picture of something without
//! also making it hoverable and reachable.
//!
//! Tooltip text is never written here: each arm of [`tooltip`] delegates to the
//! game's existing builder for that entity kind, so the client has one source
//! of truth per entity rather than one per screen.
//!
//! The encyclopedia's tiles, rows and chips are built on these; they live in
//! `states::encyclopedia::widget`, which re-exports this module's items so its
//! callers are unchanged.

use bevy_egui::egui;

use super::bars::icon_tile;
use crate::states::encyclopedia::{widget::click_hint, EncyclopediaData, Topic};
use crate::ui::theme::{color, size, text};

/// Paint an entity's icon into `rect`, falling back to the shared neutral tile
/// when its icon resource has not been loaded (or does not exist yet).
pub fn paint_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    topic: Topic,
    data: &EncyclopediaData,
) {
    icon_tile(painter, rect, topic.icon(data), color::LINE);
}

/// The entity's tooltip. Delegates to the existing builder for each kind —
/// [`link`] appends the "click to open" affordance line, so a caller-supplied
/// body gets it too.
pub fn tooltip(ui: &mut egui::Ui, topic: Topic, data: &EncyclopediaData) {
    ui.set_max_width(size::TOOLTIP_MAX_WIDTH);
    match topic {
        Topic::Item(id) => match data.items.get(&id) {
            // The same builder the equipment picker and in-match UI use.
            Some(item) => crate::states::encyclopedia::items::render_item_tooltip(ui, item),
            None => {
                ui.label(
                    egui::RichText::new(topic.name(data))
                        .size(text::BODY)
                        .color(color::ACCENT)
                        .strong(),
                );
            }
        },
        Topic::Class(class) => crate::states::encyclopedia::classes::tooltip(ui, class, data),
        // The shared `ability_text` generator — the same prose View Combatant
        // shows for the same ability.
        Topic::Ability(ability) => {
            crate::states::encyclopedia::abilities::tooltip(ui, ability, data)
        }
        // The same generated text the aura's own page shows — one builder, one
        // source of truth.
        Topic::Aura(id) => crate::states::encyclopedia::auras::render_tooltip(ui, id, data),
    }
}

/// Attach the hover tooltip + click-to-navigate contract to an already-drawn
/// response. Every linked widget funnels through it, and so should any
/// bespoke one a caller draws itself.
pub fn link(response: egui::Response, topic: Topic, data: &EncyclopediaData) -> Option<Topic> {
    link_with(response, topic, |ui| tooltip(ui, topic, data))
}

/// [`link`] with a caller-supplied tooltip BODY.
///
/// The contract — pointing-hand cursor, a hover tooltip, the shared
/// "click to open" hint, and `Some(topic)` on the clicked frame — is identical;
/// only the text inside differs. This is the seam for a surface whose job is
/// not to be the article: View Combatant is a loadout editor, so its ability
/// rows show a one-line gist and link on to the full page rather than
/// reprinting it in a tooltip.
pub fn link_with(
    response: egui::Response,
    topic: Topic,
    body: impl FnOnce(&mut egui::Ui),
) -> Option<Topic> {
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_ui(|ui| {
            // Every linked icon in the client funnels through here, so ONE
            // probe reports "this entity's tooltip body actually ran" for all
            // of them — the encyclopedia's own icons and View Combatant's kit
            // rows alike. Inert unless `--ui-script` armed the registry.
            crate::ui::driver::note(ui, format_args!("tooltip:{topic:?}"));
            body(ui);
            click_hint(ui);
        });
    response.clicked().then_some(topic)
}
