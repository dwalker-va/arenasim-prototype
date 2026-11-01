//! The Talents section — every class's talent trees, and a page per talent.
//!
//! Everything here is derived from `talents.ron` through
//! [`TalentDefinitions`]: the index draws each class's trees with the kit's
//! talent tree (read-only, each node a link), and a talent's page says what
//! every rank does, where the talent sits and what it needs, in the prose
//! [`crate::states::talent_text`] generates for every surface. Talent N+1
//! gets its index node, its search entry and its page with no code here.

use bevy_egui::egui;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::talents::{TalentDefinitions, TalentEffect, TalentId, TreeId};
use crate::states::talent_panel;
use crate::states::talent_text;
use crate::ui::fonts::body_family;
use crate::ui::theme::{class_color, color, space, text};

use super::search::SearchEntry;
use super::widget;
use super::{EncyclopediaData, Topic};

/// A talent's key in the icon map (`AbilityIcons`), beside the ability
/// names and generic aura keys it shares the map with.
pub fn talent_icon_key(talent: TalentId) -> String {
    format!("talent:{}", talent.as_str())
}

/// A tree's key in the icon map.
pub fn tree_icon_key(class: CharacterClass, tree: TreeId) -> String {
    format!("talent-tree:{class:?}:{tree:?}")
}

/// Every talent art path the icon loader registers, under its key: each
/// node's and each tree's.
pub fn icon_paths(talents: &TalentDefinitions) -> Vec<(String, String)> {
    let mut paths: Vec<(String, String)> = talents
        .iter()
        .map(|(id, t)| (talent_icon_key(id), t.icon.clone()))
        .collect();
    for class in CharacterClass::all() {
        for tree in talents.trees(*class) {
            paths.push((tree_icon_key(*class, tree.id), tree.icon.clone()));
        }
    }
    paths
}

// ============================================================================
// SEARCH REGISTRY
// ============================================================================

/// Contribute every talent to the search registry.
pub fn search_entries(talents: &TalentDefinitions, out: &mut Vec<SearchEntry>) {
    for (id, talent) in talents.iter() {
        out.push(SearchEntry::new(
            Topic::Talent(id),
            talent.name.clone(),
            format!(
                "{} \u{b7} {}",
                talent.class.name(),
                talent_text::tree_name(talents, talent.class, talent.tree)
            ),
        ));
    }
}

/// `Warlock · Demonology, tier 1 · 2 ranks`: a talent's one-line identity.
pub fn subtitle(talent: TalentId, data: &EncyclopediaData) -> String {
    match data.talents.get(talent) {
        Some(t) => format!(
            "{} \u{b7} {}",
            t.class.name(),
            talent_text::placement(data.talents, t)
        ),
        None => String::new(),
    }
}

// ============================================================================
// TOOLTIP
// ============================================================================

/// The talent tooltip: name, placement, what each rank does, and what it
/// needs.
pub fn tooltip(ui: &mut egui::Ui, talent: TalentId, data: &EncyclopediaData) {
    let topic = Topic::Talent(talent);
    ui.label(
        egui::RichText::new(topic.name(data))
            .size(text::BODY)
            .color(color::ACCENT)
            .strong(),
    );
    let Some(config) = data.talents.get(talent) else {
        return;
    };
    ui.label(
        egui::RichText::new(subtitle(talent, data))
            .size(text::SMALL)
            .color(color::TEXT_MUTED),
    );
    ui.add_space(space::XXS);
    for line in talent_text::rank_lines(config, data.abilities) {
        ui.label(
            egui::RichText::new(line)
                .family(body_family(ui.ctx()))
                .size(text::SMALL)
                .color(color::TEXT),
        );
    }
    for line in talent_text::requirement_lines(data.talents, config) {
        ui.label(
            egui::RichText::new(line)
                .size(text::SMALL)
                .color(color::TEXT_DIM),
        );
    }
}

// ============================================================================
// INDEX PAGE
// ============================================================================

/// Every class's trees, read-only. Returns the talent clicked.
pub fn render_index(ui: &mut egui::Ui, data: &EncyclopediaData) -> Option<Topic> {
    let mut clicked = None;
    widget::prose(
        ui,
        &format!(
            "A build spends up to {} points across a class's trees. {}",
            crate::states::play_match::talents::MAX_TALENT_POINTS,
            "A tier opens once the tiers above it hold enough points, and the tree with the most points names the spec."
        ),
        text::BODY,
        color::TEXT_MUTED,
    );
    let mut without = Vec::new();
    for class in CharacterClass::all() {
        if data.talents.trees(*class).is_empty() {
            without.push(class.name());
            continue;
        }
        ui.add_space(space::LG);
        ui.label(
            egui::RichText::new(class.name().to_uppercase())
                .size(text::H3)
                .color(class_color(*class)),
        );
        ui.add_space(space::SM);
        let salt = format!("enc-talents-{class:?}");
        if let Some(topic) = talent_panel::read_trees(ui, *class, data, &salt, None) {
            clicked = Some(topic);
        }
    }
    if !without.is_empty() {
        ui.add_space(space::LG);
        ui.label(
            egui::RichText::new(format!("No talent trees yet: {}", without.join(", ")))
                .size(text::SMALL)
                .color(color::TEXT_DIM),
        );
    }
    clicked
}

// ============================================================================
// DETAIL PAGE
// ============================================================================

/// One talent's page: its identity, its owning class, what every rank does,
/// what it needs and what it grants, and its place in its class's trees.
pub fn render_detail(
    ui: &mut egui::Ui,
    talent: TalentId,
    data: &EncyclopediaData,
) -> Option<Topic> {
    let topic = Topic::Talent(talent);
    let Some(config) = data.talents.get(talent) else {
        ui.label(
            egui::RichText::new("Unknown talent")
                .size(text::H3)
                .color(color::TEXT_MUTED),
        );
        return None;
    };
    widget::detail_header(
        ui,
        topic,
        &talent_text::placement(data.talents, config),
        data,
    );

    let mut clicked = None;
    ui.add_space(space::MD);
    ui.horizontal(|ui| {
        if let Some(topic) = widget::chip(ui, Topic::Class(config.class), data) {
            clicked = Some(topic);
        }
    });

    ui.add_space(space::MD);
    widget::prose_block(
        ui,
        &talent_text::rank_lines(config, data.abilities).join("\n"),
    );

    let requirements = talent_text::requirement_lines(data.talents, config);
    if !requirements.is_empty() {
        widget::section_heading(ui, "REQUIRES");
        for line in requirements {
            widget::prose(ui, &line, text::BODY, color::TEXT_MUTED);
        }
        if let Some((required, _)) = config.requires {
            ui.add_space(space::SM);
            ui.horizontal_wrapped(|ui| {
                if let Some(topic) = widget::icon_link(ui, Topic::Talent(required), data) {
                    clicked = Some(topic);
                }
            });
        }
    }

    let grants: Vec<_> = config
        .per_rank
        .iter()
        .filter_map(|effect| match effect {
            TalentEffect::Grant(ability) => Some(*ability),
            _ => None,
        })
        .collect();
    if !grants.is_empty() {
        widget::section_heading(ui, "GRANTS");
        ui.horizontal_wrapped(|ui| {
            for ability in grants {
                if let Some(topic) = widget::icon_link(ui, Topic::Ability(ability), data) {
                    clicked = Some(topic);
                }
            }
        });
    }

    let leads_to: Vec<TalentId> = data
        .talents
        .talents_for_class(config.class)
        .filter(|(_, t)| t.requires.is_some_and(|(needed, _)| needed == talent))
        .map(|(id, _)| id)
        .collect();
    if !leads_to.is_empty() {
        widget::section_heading(ui, "LEADS TO");
        ui.horizontal_wrapped(|ui| {
            for id in leads_to {
                if let Some(topic) = widget::icon_link(ui, Topic::Talent(id), data) {
                    clicked = Some(topic);
                }
            }
        });
    }

    widget::section_heading(
        ui,
        &format!("{} TALENTS", config.class.name().to_uppercase()),
    );
    let salt = format!("enc-talent-{talent:?}");
    if let Some(topic) = talent_panel::read_trees(ui, config.class, data, &salt, Some(talent)) {
        clicked = Some(topic);
    }

    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_talent_is_searchable_under_its_class() {
        let talents = TalentDefinitions::shipped();
        let mut entries = Vec::new();
        search_entries(talents, &mut entries);
        let searched: Vec<Topic> = entries.iter().map(|e| e.topic).collect();
        let expected: Vec<Topic> = talents.iter().map(|(id, _)| Topic::Talent(id)).collect();
        assert_eq!(searched, expected);
        for entry in &entries {
            assert!(!entry.name.is_empty() && !entry.sub.is_empty());
        }
    }

    #[test]
    fn every_talent_and_tree_has_an_icon_path_under_its_own_key() {
        let talents = TalentDefinitions::shipped();
        let paths = icon_paths(talents);
        for (id, t) in talents.iter() {
            assert!(paths.contains(&(talent_icon_key(id), t.icon.clone())));
        }
        let keys: std::collections::BTreeSet<&String> = paths.iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), paths.len(), "two icons share a key");
    }
}
