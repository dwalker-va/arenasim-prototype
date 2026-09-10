//! The encyclopedia's identity model.
//!
//! [`Topic`] is the address of every entity the encyclopedia can show a page
//! for. It is deliberately wider than the content that exists today: the
//! addresses for classes, abilities and auras are here so navigation, the
//! search registry and the linked-icon widget can be built against the whole
//! space now, and a later content card only has to fill in a page renderer.
//!
//! Everything a `Topic` knows about itself is derived from the game's own data
//! sources — `items.ron`, `CharacterClass`, the ability and aura registries.
//! Nothing here is hand-authored per entity, so entity N+1 needs no code.

use bevy_egui::egui;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::components::AuraType;
use crate::states::play_match::equipment::ItemId;
use crate::states::play_match::AbilityType;

use super::EncyclopediaData;

/// A top-level section of the encyclopedia — one tab, one index page, one
/// group in the search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Classes,
    Abilities,
    Auras,
    Items,
}

impl Section {
    /// Tabs in display order. This is the single source of truth for the tab
    /// row, the search-result grouping order, and the registry's sort order.
    pub fn all() -> &'static [Section] {
        &[Section::Classes, Section::Abilities, Section::Auras, Section::Items]
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Classes => "CLASSES",
            Section::Abilities => "ABILITIES",
            Section::Auras => "BUFFS & DEBUFFS",
            Section::Items => "ITEMS",
        }
    }

    /// Title-case label for the search-result group headers.
    pub fn group_label(self) -> &'static str {
        match self {
            Section::Classes => "Classes",
            Section::Abilities => "Abilities",
            Section::Auras => "Buffs & Debuffs",
            Section::Items => "Items",
        }
    }

    /// What a section that has no content yet tells the reader.
    pub fn pending_note(self) -> &'static str {
        match self {
            Section::Classes => "Class pages arrive with the Classes section.",
            Section::Abilities => "Ability pages arrive with the Abilities section.",
            Section::Auras => "Buff and debuff pages arrive with the aura catalog.",
            Section::Items => "",
        }
    }

    /// Display order index, used to sort the registry and group results.
    pub fn order(self) -> usize {
        Section::all().iter().position(|s| *s == self).unwrap_or(usize::MAX)
    }
}

/// The address of one encyclopedia entity.
///
/// Extensible by design: a new content domain becomes a new variant plus a
/// `Section`, and the navigation stack, breadcrumbs, search registry and
/// linked-icon widget all keep working unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Topic {
    Class(CharacterClass),
    Ability(AbilityType),
    Aura(AuraType),
    Item(ItemId),
}

impl Topic {
    /// Which tab this topic lives under.
    pub fn section(self) -> Section {
        match self {
            Topic::Class(_) => Section::Classes,
            Topic::Ability(_) => Section::Abilities,
            Topic::Aura(_) => Section::Auras,
            Topic::Item(_) => Section::Items,
        }
    }

    /// The entity's display name, resolved from its own data source.
    pub fn name(self, data: &EncyclopediaData) -> String {
        match self {
            Topic::Class(class) => class.name().to_string(),
            Topic::Item(id) => data
                .items
                .get(&id)
                .map(|item| item.name.clone())
                .unwrap_or_else(|| spaced_debug(&id)),
            // Abilities and auras carry their display names in `abilities.ron`
            // and (for auras) nowhere yet. Until those sections land, the
            // variant name spaced out is the honest fallback — still derived,
            // still zero-marginal-cost.
            Topic::Ability(ability) => spaced_debug(&ability),
            Topic::Aura(aura) => spaced_debug(&aura),
        }
    }

    /// One-line subtitle shown under the name on a detail header and beside a
    /// search hit.
    pub fn subtitle(self, data: &EncyclopediaData) -> String {
        match self {
            Topic::Class(class) => class.description().to_string(),
            Topic::Item(id) => match data.items.get(&id) {
                Some(item) => super::items::item_subtitle(item),
                None => String::new(),
            },
            Topic::Ability(_) | Topic::Aura(_) => String::new(),
        }
    }

    /// The egui texture for this entity's icon, when its icon resource is
    /// loaded. `None` renders as a neutral placeholder tile.
    pub fn icon(self, data: &EncyclopediaData) -> Option<egui::TextureId> {
        match self {
            Topic::Class(class) => data
                .class_icons
                .and_then(|icons| icons.textures.get(&class).copied()),
            Topic::Item(id) => data
                .item_icons
                .and_then(|icons| icons.textures.get(&id).copied()),
            // Ability and aura icon resources are keyed by ability NAME, which
            // needs the ability registry the Abilities section brings with it.
            Topic::Ability(_) | Topic::Aura(_) => None,
        }
    }

    /// Accent color for the entity's name. Items and classes use the gold /
    /// class-color conventions the rest of the UI already follows.
    pub fn accent(self) -> egui::Color32 {
        match self {
            Topic::Class(class) => {
                let c = class.color().to_srgba();
                egui::Color32::from_rgb(
                    (c.red * 255.0) as u8,
                    (c.green * 255.0) as u8,
                    (c.blue * 255.0) as u8,
                )
            }
            _ => super::GOLD,
        }
    }
}

/// Split a `Debug`-printed enum variant at its camel-case boundaries:
/// `MovementSpeedSlow` -> `Movement Speed Slow`.
fn spaced_debug<T: std::fmt::Debug>(value: &T) -> String {
    let raw = format!("{:?}", value);
    let mut out = String::with_capacity(raw.len() + 4);
    let mut prev_lower = false;
    for ch in raw.chars() {
        if ch.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = ch.is_lowercase() || ch.is_numeric();
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_order_matches_the_tab_row() {
        assert_eq!(Section::Classes.order(), 0);
        assert_eq!(Section::Items.order(), 3);
    }

    #[test]
    fn topics_route_to_their_section() {
        assert_eq!(Topic::Item(ItemId::WandOfTheInvoker).section(), Section::Items);
        assert_eq!(Topic::Class(CharacterClass::Mage).section(), Section::Classes);
        assert_eq!(Topic::Aura(AuraType::Stun).section(), Section::Auras);
    }

    #[test]
    fn camel_case_variants_are_spaced_out() {
        assert_eq!(spaced_debug(&AuraType::MovementSpeedSlow), "Movement Speed Slow");
        assert_eq!(spaced_debug(&AuraType::Stun), "Stun");
    }
}
