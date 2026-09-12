//! The Items section — the encyclopedia's first content corpus.
//!
//! This subsumes the standalone Armory screen: the chip-bar filters, the
//! wrapping tile grid and the item tooltip all came from there. What is new is
//! the per-item DETAIL page, which the Armory never had — the grid tiles are
//! now links, not just hover targets.
//!
//! Everything is derived from `items.ron` through [`ItemDefinitions`]: the
//! filter chips, the grid, the search registry and the "usable by" chips all
//! come from item data plus the existing `can_equip` rule, so a new item needs
//! no code here at all.

use bevy_egui::egui;
use std::collections::HashSet;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::equipment::{
    can_equip, ArmorType, ItemConfig, ItemDefinitions, ItemId, ItemSlotType, WeaponType,
};

use super::search::SearchEntry;
use super::widget;
use super::{EncyclopediaData, Topic, DIM, MUTED, TEXT};

/// Default upper bound for the item-level filter. No current item exceeds ~75,
/// so 100 leaves headroom without forcing the user to slide a max.
const DEFAULT_ITEM_LEVEL_MAX: u32 = 100;

/// Width of a grid tile. Tiles wrap; this is the only layout knob.
const TILE_WIDTH: f32 = 224.0;

// ============================================================================
// FILTERS
// ============================================================================

/// Chip-bar filter state for the Items section (ported from the retired
/// Armory screen). Persists for the session; resets across launches.
pub struct ItemFilters {
    pub selected_slots: HashSet<ItemSlotType>,
    pub selected_armor_types: HashSet<ArmorType>,
    pub item_level_min: u32,
    pub item_level_max: u32,
    pub name_search: String,
}

impl Default for ItemFilters {
    fn default() -> Self {
        Self {
            selected_slots: HashSet::new(),
            selected_armor_types: HashSet::new(),
            item_level_min: 0,
            item_level_max: DEFAULT_ITEM_LEVEL_MAX,
            name_search: String::new(),
        }
    }
}

impl ItemFilters {
    /// Compute the search needle once for a batch of matches.
    /// Returns `None` when there is no active name filter.
    pub fn name_needle(&self) -> Option<String> {
        let trimmed = self.name_search.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_lowercase()) }
    }

    /// Whether the given item passes all active filters.
    /// `needle` is the pre-lowercased result of [`Self::name_needle`].
    /// AND across axes, OR within each axis.
    pub fn matches(&self, item: &ItemConfig, needle: Option<&str>) -> bool {
        if !self.selected_slots.is_empty() && !self.selected_slots.contains(&item.slot) {
            return false;
        }
        if !self.selected_armor_types.is_empty()
            && !self.selected_armor_types.contains(&item.armor_type)
        {
            return false;
        }
        if item.item_level < self.item_level_min || item.item_level > self.item_level_max {
            return false;
        }
        if let Some(needle) = needle {
            if !item.name.to_lowercase().contains(needle) {
                return false;
            }
        }
        true
    }
}

const ARMOR_TYPE_CHIPS: &[(&str, ArmorType)] = &[
    ("Plate", ArmorType::Plate),
    ("Mail", ArmorType::Mail),
    ("Leather", ArmorType::Leather),
    ("Cloth", ArmorType::Cloth),
    ("None", ArmorType::None),
];

/// Canonical ordering index for slot kinds. Lower values sort first.
/// Mirrors `ItemSlotType::all()` ordering.
fn slot_order(slot: ItemSlotType) -> usize {
    ItemSlotType::all().iter().position(|s| *s == slot).unwrap_or(usize::MAX)
}

// ============================================================================
// SEARCH REGISTRY
// ============================================================================

/// Contribute every item to the search registry. Zero hand-authored entries:
/// this is a straight walk of `items.ron`.
pub fn search_entries(items: &ItemDefinitions, out: &mut Vec<SearchEntry>) {
    for (id, item) in items.iter() {
        out.push(SearchEntry::new(
            Topic::Item(*id),
            item.name.clone(),
            item_subtitle(item),
        ));
    }
}

/// `Chest · Plate` — the one-line identity of an item, shared by tiles, search
/// rows and the detail header.
pub fn item_subtitle(item: &ItemConfig) -> String {
    let mut parts = vec![item.slot.name().to_string()];
    if item.armor_type != ArmorType::None {
        parts.push(format!("{:?}", item.armor_type));
    } else if item.weapon_type != WeaponType::None {
        parts.push(format!("{:?}", item.weapon_type));
    }
    parts.join(" · ")
}

// ============================================================================
// INDEX PAGE
// ============================================================================

/// The filtered item grid. Returns the item whose tile was clicked.
pub fn render_index(
    ui: &mut egui::Ui,
    filters: &mut ItemFilters,
    data: &EncyclopediaData,
) -> Option<Topic> {
    let total = data.items.item_count();

    let needle = filters.name_needle();
    let mut filtered: Vec<(&ItemId, &ItemConfig)> = data
        .items
        .iter()
        .filter(|(_, item)| filters.matches(item, needle.as_deref()))
        .collect();
    filtered.sort_unstable_by(|(_, a), (_, b)| {
        slot_order(a.slot)
            .cmp(&slot_order(b.slot))
            .then(b.item_level.cmp(&a.item_level))
            .then(a.name.as_str().cmp(b.name.as_str()))
    });

    render_chip_bar(ui, filters, total, filtered.len());
    ui.add_space(10.0);

    if filtered.is_empty() {
        ui.add_space(50.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No items match these filters.")
                    .size(15.0)
                    .color(MUTED),
            );
            ui.add_space(10.0);
            if ui.button(egui::RichText::new("Clear filters").size(13.0).color(TEXT)).clicked() {
                *filters = ItemFilters::default();
            }
        });
        return None;
    }

    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        for (id, item) in &filtered {
            let badge = format!("ilvl {}", item.item_level);
            if let Some(topic) = widget::tile(
                ui,
                Topic::Item(**id),
                &item_subtitle(item),
                Some(&badge),
                TILE_WIDTH,
                data,
            ) {
                clicked = Some(topic);
            }
        }
    });

    clicked
}

/// Two-row filter chip-bar.
/// Row 1: slot chips. Row 2: armor type + iLvl range + name filter + count + clear.
fn render_chip_bar(ui: &mut egui::Ui, filters: &mut ItemFilters, total: usize, visible: usize) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("SLOT").size(12.0).color(DIM));
        // One chip per slot KIND, straight off `ItemSlotType::all()` — rings and
        // trinkets are one kind each, so there is nothing to collapse here.
        for slot_type in ItemSlotType::all() {
            let active = filters.selected_slots.contains(slot_type);
            if ui.selectable_label(active, slot_type.name()).clicked() {
                if active {
                    filters.selected_slots.remove(slot_type);
                } else {
                    filters.selected_slots.insert(*slot_type);
                }
            }
        }
    });

    ui.add_space(4.0);

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("TYPE").size(12.0).color(DIM));
        for (label, armor_type) in ARMOR_TYPE_CHIPS {
            let active = filters.selected_armor_types.contains(armor_type);
            if ui.selectable_label(active, *label).clicked() {
                if active {
                    filters.selected_armor_types.remove(armor_type);
                } else {
                    filters.selected_armor_types.insert(*armor_type);
                }
            }
        }

        ui.separator();

        ui.label(egui::RichText::new("iLvl").size(12.0).color(DIM));
        let min_response = ui.add(
            egui::DragValue::new(&mut filters.item_level_min)
                .range(0..=DEFAULT_ITEM_LEVEL_MAX)
                .speed(1.0),
        );
        ui.label("–");
        let max_response = ui.add(
            egui::DragValue::new(&mut filters.item_level_max)
                .range(0..=DEFAULT_ITEM_LEVEL_MAX)
                .speed(1.0),
        );
        // Keep min ≤ max by yielding to whichever side the user just moved, so
        // the active drag never "bounces" the other way.
        if filters.item_level_min > filters.item_level_max {
            if min_response.changed() {
                filters.item_level_max = filters.item_level_min;
            } else if max_response.changed() {
                filters.item_level_min = filters.item_level_max;
            }
        }

        ui.separator();

        ui.add(
            egui::TextEdit::singleline(&mut filters.name_search)
                .hint_text("Filter by name…")
                .desired_width(130.0),
        );

        ui.separator();

        ui.label(
            egui::RichText::new(format!("{} / {} items", visible, total))
                .size(12.5)
                .color(DIM),
        );

        if ui.button(egui::RichText::new("Clear").size(12.5).color(TEXT)).clicked() {
            *filters = ItemFilters::default();
        }
    });
}

// ============================================================================
// DETAIL PAGE
// ============================================================================

/// One item's page: identity header, weapon block, full stat block, and the
/// classes that may equip it (each a link to that class's page).
pub fn render_detail(ui: &mut egui::Ui, id: ItemId, data: &EncyclopediaData) -> Option<Topic> {
    let topic = Topic::Item(id);
    let Some(item) = data.items.get(&id) else {
        ui.label(egui::RichText::new("Unknown item").size(16.0).color(MUTED));
        return None;
    };

    widget::detail_header(ui, topic, &detail_subtitle(item), data);

    ui.add_space(14.0);
    widget::stat_rows(ui, "encyclopedia_item_stats", &item_stat_rows(item));

    let usable: Vec<CharacterClass> = CharacterClass::all()
        .iter()
        .copied()
        .filter(|class| can_equip(*class, item))
        .collect();

    let mut clicked = None;
    if !usable.is_empty() {
        widget::section_heading(ui, "USABLE BY");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for class in usable {
                if let Some(topic) = widget::chip(ui, Topic::Class(class), data) {
                    clicked = Some(topic);
                }
            }
        });
    }

    clicked
}

/// `Item level 63 · Chest · Plate` (plus `Two-handed` where it applies).
fn detail_subtitle(item: &ItemConfig) -> String {
    let mut parts = Vec::new();
    if item.item_level > 0 {
        parts.push(format!("Item level {}", item.item_level));
    }
    parts.push(item_subtitle(item));
    if item.two_handed {
        parts.push("Two-handed".to_string());
    }
    parts.join(" · ")
}

/// Key/value rows for the detail page's stat block. Derived field-by-field from
/// `ItemConfig`, so a new stat needs one line here and nothing per item.
fn item_stat_rows(item: &ItemConfig) -> Vec<(String, String)> {
    let mut rows = Vec::new();

    if item.is_weapon && (item.attack_damage_min > 0.0 || item.attack_damage_max > 0.0) {
        rows.push((
            "Weapon damage".to_string(),
            format!("{:.0} – {:.0}", item.attack_damage_min, item.attack_damage_max),
        ));
        if item.attack_speed > 0.0 {
            rows.push(("Attack speed".to_string(), format!("{:.1}/s", item.attack_speed)));
            let mid = (item.attack_damage_min + item.attack_damage_max) / 2.0;
            rows.push(("Damage per second".to_string(), format!("{:.1}", mid * item.attack_speed)));
        }
    }

    let mut push = |label: &str, value: f32, fmt: fn(f32) -> String| {
        if value != 0.0 {
            rows.push((label.to_string(), fmt(value)));
        }
    };

    push("Armor", item.armor, |v| format!("{:.0}", v));
    push("Health", item.max_health, |v| format!("+{:.0}", v));
    push("Mana", item.max_mana, |v| format!("+{:.0}", v));
    push("Mana regen", item.mana_regen, |v| format!("+{:.1} MP5", v));
    push("Attack power", item.attack_power, |v| format!("+{:.0}", v));
    push("Spell power", item.spell_power, |v| format!("+{:.0}", v));
    push("Crit chance", item.crit_chance, |v| format!("+{:.1}%", v * 100.0));
    push("Movement speed", item.movement_speed, |v| format!("+{:.0}%", v * 100.0));
    push("Fire resistance", item.fire_resistance, |v| format!("+{:.0}", v));
    push("Frost resistance", item.frost_resistance, |v| format!("+{:.0}", v));
    push("Shadow resistance", item.shadow_resistance, |v| format!("+{:.0}", v));
    push("Arcane resistance", item.arcane_resistance, |v| format!("+{:.0}", v));
    push("Nature resistance", item.nature_resistance, |v| format!("+{:.0}", v));
    push("Holy resistance", item.holy_resistance, |v| format!("+{:.0}", v));

    rows
}

// ============================================================================
// ITEM TOOLTIP (shared with the loadout editor)
// ============================================================================

/// The stat lines of an item, one string per stat.
///
/// Lives here rather than in the loadout editor because the encyclopedia is
/// where item presentation is defined; the editor imports it so both surfaces
/// read from one source.
pub fn item_stat_parts(item: &ItemConfig) -> Vec<String> {
    let mut parts = Vec::new();

    if item.is_weapon {
        if item.attack_damage_min > 0.0 || item.attack_damage_max > 0.0 {
            parts.push(format!("{:.0}-{:.0} Damage", item.attack_damage_min, item.attack_damage_max));
        }
        if item.attack_speed > 0.0 {
            parts.push(format!("{:.1} Speed", item.attack_speed));
        }
    }

    if item.max_health != 0.0 { parts.push(format!("+{:.0} HP", item.max_health)); }
    if item.max_mana != 0.0 { parts.push(format!("+{:.0} Mana", item.max_mana)); }
    if item.mana_regen != 0.0 { parts.push(format!("+{:.1} MP5", item.mana_regen)); }
    if item.attack_power != 0.0 { parts.push(format!("+{:.0} AP", item.attack_power)); }
    if item.spell_power != 0.0 { parts.push(format!("+{:.0} SP", item.spell_power)); }
    if item.crit_chance != 0.0 { parts.push(format!("+{:.1}% Crit", item.crit_chance * 100.0)); }
    if item.movement_speed != 0.0 { parts.push(format!("+{:.0}% Speed", item.movement_speed * 100.0)); }
    if item.armor != 0.0 { parts.push(format!("{:.0} Armor", item.armor)); }
    if item.fire_resistance != 0.0 { parts.push(format!("+{:.0} Fire Resist", item.fire_resistance)); }
    if item.frost_resistance != 0.0 { parts.push(format!("+{:.0} Frost Resist", item.frost_resistance)); }
    if item.shadow_resistance != 0.0 { parts.push(format!("+{:.0} Shadow Resist", item.shadow_resistance)); }
    if item.arcane_resistance != 0.0 { parts.push(format!("+{:.0} Arcane Resist", item.arcane_resistance)); }
    if item.nature_resistance != 0.0 { parts.push(format!("+{:.0} Nature Resist", item.nature_resistance)); }
    if item.holy_resistance != 0.0 { parts.push(format!("+{:.0} Holy Resist", item.holy_resistance)); }

    parts
}

/// Format stat bonuses as a comma-separated string for inline display.
pub fn format_item_stats(item: &ItemConfig) -> String {
    item_stat_parts(item).join(", ")
}

/// Render a tooltip showing an item's full stat breakdown.
pub fn render_item_tooltip(ui: &mut egui::Ui, item: &ItemConfig) {
    ui.label(
        egui::RichText::new(&item.name)
            .size(14.0)
            .color(egui::Color32::from_rgb(255, 215, 0))
            .strong(),
    );

    if item.item_level > 0 {
        ui.label(
            egui::RichText::new(format!("Item Level {}", item.item_level))
                .size(12.0)
                .color(egui::Color32::from_rgb(170, 170, 170)),
        );
    }

    if item.armor_type != ArmorType::None {
        ui.label(
            egui::RichText::new(format!("{:?}", item.armor_type))
                .size(12.0)
                .color(egui::Color32::from_rgb(170, 170, 170)),
        );
    }

    let stat_parts = item_stat_parts(item);
    if !stat_parts.is_empty() {
        ui.add_space(4.0);
        for part in &stat_parts {
            ui.label(
                egui::RichText::new(part)
                    .size(12.0)
                    .color(egui::Color32::from_rgb(100, 255, 100)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::equipment::load_item_definitions;

    #[test]
    fn every_item_has_a_subtitle_and_is_usable_by_someone() {
        let items = load_item_definitions().expect("items.ron must load");
        for (id, item) in items.iter() {
            assert!(
                !item_subtitle(item).is_empty(),
                "{:?} produced an empty subtitle",
                id
            );
            assert!(
                CharacterClass::all().iter().any(|c| can_equip(*c, item)),
                "{:?} is equippable by no class — it would show an empty page",
                id
            );
        }
    }

    #[test]
    fn stat_rows_skip_zeroed_fields() {
        let items = load_item_definitions().expect("items.ron must load");
        for (_, item) in items.iter() {
            for (label, value) in item_stat_rows(item) {
                assert!(!label.is_empty() && !value.is_empty());
                assert!(!value.contains("+0 "), "zeroed stat leaked into the block");
            }
        }
    }

    #[test]
    fn filters_and_across_axes() {
        let items = load_item_definitions().expect("items.ron must load");
        let mut filters = ItemFilters::default();
        let all = items.iter().filter(|(_, i)| filters.matches(i, None)).count();
        assert_eq!(all, items.item_count(), "a default filter hides nothing");

        filters.selected_armor_types.insert(ArmorType::Plate);
        let plate = items.iter().filter(|(_, i)| filters.matches(i, None)).count();
        assert!(plate > 0 && plate < all);

        filters.selected_slots.insert(ItemSlotType::Head);
        let plate_heads = items.iter().filter(|(_, i)| filters.matches(i, None)).count();
        assert!(plate_heads <= plate, "adding an axis can only narrow the set");
    }
}
