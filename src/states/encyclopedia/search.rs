//! The search registry.
//!
//! Every searchable thing in the encyclopedia is a [`SearchEntry`] contributed
//! by the section that owns its data. [`build_registry`] is the one place that
//! knows which sections exist; a new section adds one call here and its whole
//! corpus becomes searchable.
//!
//! No entry is ever hand-written. Item N+1 becomes searchable the moment it is
//! added to `items.ron` — which is the zero-marginal-cost rule the encyclopedia
//! is built on.

use bevy_egui::egui;

use crate::states::play_match::equipment::ItemDefinitions;

use super::{EncyclopediaData, Section, Topic, DIM, GOLD, MUTED};

/// Cap on rendered hits. A bare "a" matches most of the corpus and drawing all
/// of it is pure cost; the count line says when the list was cut.
const MAX_RESULTS: usize = 80;

/// One searchable entity.
pub struct SearchEntry {
    pub topic: Topic,
    /// Lowercased name, precomputed so matching does no per-keystroke allocation.
    pub needle: String,
    /// Display name.
    pub name: String,
    /// Muted right-hand note (slot, school, polarity, …).
    pub sub: String,
}

impl SearchEntry {
    pub fn new(topic: Topic, name: String, sub: String) -> Self {
        Self { needle: name.to_lowercase(), topic, name, sub }
    }
}

/// Build the whole registry from the game's data sources.
///
/// Sections contribute in tab order, and each section's entries are sorted by
/// name, so results are stable across runs no matter how the underlying maps
/// iterate.
pub fn build_registry(items: &ItemDefinitions) -> Vec<SearchEntry> {
    let mut entries = Vec::new();
    // Classes / abilities / auras join here as their sections land; each is one
    // call that reads its own registry.
    super::items::search_entries(items, &mut entries);
    entries.sort_by(|a, b| {
        a.topic
            .section()
            .order()
            .cmp(&b.topic.section().order())
            .then_with(|| a.name.cmp(&b.name))
    });
    entries
}

/// Rank a registry entry against a lowercased query. Lower is better; `None`
/// means no match. Prefix hits outrank interior hits so typing "wand" surfaces
/// "Wand of the Invoker" above "Gnarled Wand of Ruin".
fn rank(entry: &SearchEntry, needle: &str) -> Option<usize> {
    entry.needle.find(needle)
}

/// Render the grouped result list. Returns the topic whose row was clicked.
pub fn render_results(
    ui: &mut egui::Ui,
    needle: &str,
    registry: &[SearchEntry],
    data: &EncyclopediaData,
) -> Option<Topic> {
    let mut hits: Vec<(usize, &SearchEntry)> = registry
        .iter()
        .filter_map(|entry| rank(entry, needle).map(|r| (r, entry)))
        .collect();
    // Stable within a section: rank first, then the registry's own name order.
    hits.sort_by_key(|(r, entry)| (entry.topic.section().order(), *r));

    if hits.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(format!("No matches for “{}”", needle))
                    .size(15.0)
                    .color(MUTED)
                    .italics(),
            );
        });
        return None;
    }

    let total = hits.len();
    let truncated = total > MAX_RESULTS;
    hits.truncate(MAX_RESULTS);

    ui.label(
        egui::RichText::new(if truncated {
            format!("{} matches — showing the first {}", total, MAX_RESULTS)
        } else if total == 1 {
            "1 match".to_string()
        } else {
            format!("{} matches", total)
        })
        .size(12.5)
        .color(DIM),
    );
    ui.add_space(8.0);

    let width = ui.available_width().min(560.0);
    let mut clicked = None;
    for section in Section::all() {
        let group: Vec<&(usize, &SearchEntry)> = hits
            .iter()
            .filter(|(_, entry)| entry.topic.section() == *section)
            .collect();
        if group.is_empty() {
            continue;
        }
        ui.label(egui::RichText::new(section.group_label()).size(12.0).color(GOLD));
        ui.add_space(2.0);
        for (_, entry) in group {
            if let Some(topic) = super::widget::row(ui, entry.topic, &entry.sub, width, data) {
                clicked = Some(topic);
            }
        }
        ui.add_space(10.0);
    }

    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::equipment::load_item_definitions;

    #[test]
    fn every_item_is_searchable_without_a_hand_authored_entry() {
        let items = load_item_definitions().expect("items.ron must load");
        let registry = build_registry(&items);
        assert_eq!(registry.len(), items.item_count());
    }

    #[test]
    fn registry_order_is_deterministic() {
        let items = load_item_definitions().expect("items.ron must load");
        let a = build_registry(&items);
        let b = build_registry(&items);
        let names_a: Vec<&str> = a.iter().map(|e| e.name.as_str()).collect();
        let names_b: Vec<&str> = b.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names_a, names_b);
        assert!(names_a.windows(2).all(|w| w[0] <= w[1]), "items sort by name");
    }

    #[test]
    fn matching_is_case_insensitive_substring() {
        let items = load_item_definitions().expect("items.ron must load");
        let registry = build_registry(&items);
        let hits = registry.iter().filter(|e| rank(e, "wand").is_some()).count();
        assert!(hits > 0, "expected at least one item whose name contains 'wand'");
    }
}
