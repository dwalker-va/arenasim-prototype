//! The Classes section — one page per playable class.
//!
//! Every number on a class page is the number the simulation uses:
//! [`class_base_stats`] is the same block `Combatant::new` spawns from, and the
//! kit grid is [`AbilityDefinitions::abilities_for_class`], derived from the
//! `class` field on each entry in `abilities.ron`. Neither is a list this
//! module maintains, so class N+1 and ability N+1 both appear here for free.
//!
//! (The screen this replaces kept its own copies of both, and both had drifted
//! — the stats by up to 120 health, the kit by five whole abilities.)

use bevy_egui::egui;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::components::{class_base_stats, ClassBaseStats};

use super::search::SearchEntry;
use super::widget;
use super::{EncyclopediaData, Topic};

/// Width of a class tile on the index. Wider than an item tile because a class
/// description is a sentence, not a slot name.
const TILE_WIDTH: f32 = 300.0;

// ============================================================================
// SEARCH REGISTRY
// ============================================================================

/// Contribute every class to the search registry.
pub fn search_entries(out: &mut Vec<SearchEntry>) {
    for class in CharacterClass::all() {
        out.push(SearchEntry::new(
            Topic::Class(*class),
            class.name().to_string(),
            subtitle(*class),
        ));
    }
}

/// `Mana · 250 health` — the one-line identity of a class, shared by search
/// rows and the tooltip.
pub fn subtitle(class: CharacterClass) -> String {
    let stats = class_base_stats(class);
    format!("{} · {:.0} health", stats.resource_type.name(), stats.max_health)
}

// ============================================================================
// TOOLTIP
// ============================================================================

/// The class tooltip: name, resource/health line, and the class description.
pub fn tooltip(ui: &mut egui::Ui, class: CharacterClass, data: &EncyclopediaData) {
    ui.label(
        egui::RichText::new(class.name())
            .size(14.0)
            .color(Topic::Class(class).accent(data))
            .strong(),
    );
    ui.label(egui::RichText::new(subtitle(class)).size(12.0).color(super::DIM));
    ui.label(egui::RichText::new(class.description()).size(12.0).color(super::MUTED));
}

// ============================================================================
// INDEX PAGE
// ============================================================================

/// The eight class tiles. Returns the class whose tile was clicked.
pub fn render_index(ui: &mut egui::Ui, data: &EncyclopediaData) -> Option<Topic> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        for class in CharacterClass::all() {
            let stats = class_base_stats(*class);
            if let Some(topic) = widget::tile(
                ui,
                Topic::Class(*class),
                class.description(),
                Some(stats.resource_type.name()),
                TILE_WIDTH,
                data,
            ) {
                clicked = Some(topic);
            }
        }
    });
    clicked
}

// ============================================================================
// DETAIL PAGE
// ============================================================================

/// One class's page: identity header, the real base-stat block, the full kit,
/// and a subsection per pet. Returns the ability (or pet ability) clicked.
pub fn render_detail(
    ui: &mut egui::Ui,
    class: CharacterClass,
    data: &EncyclopediaData,
) -> Option<Topic> {
    let topic = Topic::Class(class);
    widget::detail_header(ui, topic, class.description(), data);

    ui.add_space(14.0);
    let stats = class_base_stats(class);
    widget::stat_rows(ui, "encyclopedia_class_stats", &stat_rows(&stats));

    let mut clicked = None;

    // The class's own abilities, in the kit order AS-30 derives from
    // `abilities.ron` (the `AbilityType` declaration order). Deliberately NOT
    // re-sorted here: the class page, the abilities index and View Combatant
    // all show one order, and that order has exactly one definition.
    widget::section_heading(ui, "ABILITIES");
    let own = data.abilities.own_abilities_for_class(class);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for ability in &own {
            if let Some(topic) = widget::icon_link(ui, Topic::Ability(*ability), data) {
                clicked = Some(topic);
            }
        }
    });

    // Pet abilities live under a subsection named for the pet that casts them —
    // the grouping `pet_abilities_for_class` already returns.
    for (pet, abilities) in data.abilities.pet_abilities_for_class(class) {
        widget::sub_heading(ui, &format!("PET — {}", pet.name().to_uppercase()));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for ability in &abilities {
                if let Some(topic) = widget::icon_link(ui, Topic::Ability(*ability), data) {
                    clicked = Some(topic);
                }
            }
        });
    }

    clicked
}

/// Key/value rows for the class stat block, read field-by-field off
/// [`ClassBaseStats`]. Zero-valued optional stats are skipped, so a pure caster
/// shows no attack power and a pure melee no spell power.
fn stat_rows(stats: &ClassBaseStats) -> Vec<(String, String)> {
    let resource = stats.resource_type.name();
    let mut rows = vec![
        ("Health".to_string(), format!("{:.0}", stats.max_health)),
        (resource.to_string(), format!("{:.0}", stats.max_resource)),
    ];
    if stats.resource_regen != 0.0 {
        rows.push((format!("{} regen", resource), format!("+{:.0}/sec", stats.resource_regen)));
    }
    rows.push((
        "Attack damage".to_string(),
        format!("{:.0} per swing · {:.1}/sec", stats.attack_damage, stats.attack_speed),
    ));
    if stats.attack_power != 0.0 {
        rows.push(("Attack power".to_string(), format!("{:.0}", stats.attack_power)));
    }
    if stats.spell_power != 0.0 {
        rows.push(("Spell power".to_string(), format!("{:.0}", stats.spell_power)));
    }
    rows.push(("Crit chance".to_string(), format!("{:.0}%", stats.crit_chance * 100.0)));
    rows.push(("Movement speed".to_string(), format!("{:.1} yd/sec", stats.movement_speed)));
    if stats.armor != 0.0 {
        rows.push(("Armor".to_string(), format!("{:.0}", stats.armor)));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::ability_config::load_ability_definitions;

    #[test]
    fn every_class_is_searchable_and_has_a_subtitle() {
        let mut entries = Vec::new();
        search_entries(&mut entries);
        assert_eq!(entries.len(), CharacterClass::all().len());
        for entry in &entries {
            assert!(!entry.name.is_empty() && !entry.sub.is_empty());
        }
    }

    /// The kit grid is derived, so no class can render an empty ABILITIES
    /// section — the failure mode a hand-maintained list produces silently.
    #[test]
    fn every_class_page_shows_a_non_empty_kit() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        for class in CharacterClass::all() {
            assert!(
                !abilities.own_abilities_for_class(*class).is_empty(),
                "{:?} has no abilities to show",
                class
            );
        }
    }

    /// The stat block is the sim's block: reading a page can never report a
    /// number the simulation does not use.
    #[test]
    fn stat_rows_report_the_simulations_own_numbers() {
        for class in CharacterClass::all() {
            let stats = class_base_stats(*class);
            let rows = stat_rows(&stats);
            let health = rows.iter().find(|(k, _)| k == "Health").expect("health row");
            assert_eq!(health.1, format!("{:.0}", stats.max_health));
            // A class shows exactly one of the two power stats today, and never
            // a zeroed one.
            assert!(!rows.iter().any(|(_, v)| v == "0"));
        }
    }

    /// Only the Warlock and the Hunter field pets, and both pet groups must
    /// carry a label — an unlabelled subsection would be a bare icon row.
    #[test]
    fn pet_subsections_are_labelled_by_their_pet() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        let with_pets: Vec<CharacterClass> = CharacterClass::all()
            .iter()
            .copied()
            .filter(|c| !abilities.pet_abilities_for_class(*c).is_empty())
            .collect();
        assert_eq!(with_pets, vec![CharacterClass::Warlock, CharacterClass::Hunter]);
        for class in with_pets {
            for (pet, list) in abilities.pet_abilities_for_class(class) {
                assert!(!pet.name().is_empty());
                assert!(!list.is_empty());
            }
        }
    }
}
