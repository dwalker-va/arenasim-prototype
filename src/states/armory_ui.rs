//! Armory UI - Browse all equipment in the game
//!
//! Player-facing showcase screen accessed from the main menu. Renders every
//! item from `ItemDefinitions` as a uniform-frame tile in a wrapping grid,
//! with a chip-bar of filters (Slot, Armor Type, Item Level range, Name search)
//! and hover-tooltips for stat details. Read-only — no equipping.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashSet;

use super::GameState;
use super::play_match::equipment::{ArmorType, ItemConfig, ItemDefinitions, ItemId, ItemSlot};
use super::view_combatant_ui::{render_item_tooltip, ItemIcons};

// ============================================================================
// THEME CONSTANTS
// ============================================================================

const BG_COLOR: egui::Color32 = egui::Color32::from_rgb(20, 20, 30);
const TITLE_GOLD: egui::Color32 = egui::Color32::from_rgb(230, 204, 153);
const BUTTON_TEXT: egui::Color32 = egui::Color32::from_rgb(230, 217, 191);
const MUTED_TEXT: egui::Color32 = egui::Color32::from_rgb(102, 102, 102);
const TILE_FRAME: egui::Color32 = egui::Color32::from_rgb(60, 60, 80);
const TILE_BG: egui::Color32 = egui::Color32::from_rgb(30, 30, 42);
const BADGE_BG: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 180);

/// Default upper bound for the item_level filter. No current item exceeds ~75,
/// so 100 leaves comfortable headroom without forcing the user to slide a max.
const DEFAULT_ITEM_LEVEL_MAX: u32 = 100;

const TILE_SIZE: f32 = 76.0;
const ICON_SIZE: f32 = 64.0;
const TILE_PADDING: f32 = 6.0;

// ============================================================================
// FILTER STATE
// ============================================================================

/// Filter state for the armory screen. Persists across MainMenu↔Armory
/// transitions within a session; resets across game launches.
#[derive(Resource)]
pub struct ArmoryFilters {
    pub selected_slots: HashSet<ItemSlot>,
    pub selected_armor_types: HashSet<ArmorType>,
    pub item_level_min: u32,
    pub item_level_max: u32,
    pub name_search: String,
}

impl Default for ArmoryFilters {
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

impl ArmoryFilters {
    /// Whether the given item passes all active filters.
    /// AND across axes, OR within each axis.
    /// Compute the search needle once for a batch of matches.
    /// Returns `None` when there is no active name search.
    pub fn name_needle(&self) -> Option<String> {
        let trimmed = self.name_search.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_lowercase()) }
    }

    /// Whether the given item passes all active filters.
    /// `needle` is the pre-lowercased result of `name_needle()`.
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

// ============================================================================
// FILTER CHIP GROUPS
// ============================================================================

/// Logical slot kinds presented in the chip-bar. `Ring1`/`Ring2` and
/// `Trinket1`/`Trinket2` collapse to single chips because users see them as
/// one slot kind.
const SLOT_CHIPS: &[(&str, &[ItemSlot])] = &[
    ("Head", &[ItemSlot::Head]),
    ("Neck", &[ItemSlot::Neck]),
    ("Shoulders", &[ItemSlot::Shoulders]),
    ("Back", &[ItemSlot::Back]),
    ("Chest", &[ItemSlot::Chest]),
    ("Wrists", &[ItemSlot::Wrists]),
    ("Hands", &[ItemSlot::Hands]),
    ("Waist", &[ItemSlot::Waist]),
    ("Legs", &[ItemSlot::Legs]),
    ("Feet", &[ItemSlot::Feet]),
    ("Ring", &[ItemSlot::Ring1, ItemSlot::Ring2]),
    ("Trinket", &[ItemSlot::Trinket1, ItemSlot::Trinket2]),
    ("Main Hand", &[ItemSlot::MainHand]),
    ("Off Hand", &[ItemSlot::OffHand]),
    ("Ranged", &[ItemSlot::Ranged]),
];

const ARMOR_TYPE_CHIPS: &[(&str, ArmorType)] = &[
    ("Plate", ArmorType::Plate),
    ("Mail", ArmorType::Mail),
    ("Leather", ArmorType::Leather),
    ("Cloth", ArmorType::Cloth),
    ("None", ArmorType::None),
];

// ============================================================================
// UI SYSTEM
// ============================================================================

/// Top-level armory UI system. Renders the dark-themed screen with header,
/// back button, chip-bar filters, and the item tile grid.
pub fn armory_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut filters: ResMut<ArmoryFilters>,
    item_defs: Res<ItemDefinitions>,
    item_icons: Option<Res<ItemIcons>>,
) {
    let ctx = contexts.ctx_mut();

    // Apply dark theme matching the main menu, with zero-delay tooltips so
    // hovering a tile shows item info immediately (matches ViewCombatant).
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = BG_COLOR;
    style.visuals.panel_fill = BG_COLOR;
    style.interaction.tooltip_delay = 0.0;
    ctx.set_style(style);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(BG_COLOR).inner_margin(egui::Margin::same(16.0)))
        .show(ctx, |ui| {
            render_header(ui, &mut next_state);
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            let icons_ready = item_icons
                .as_ref()
                .map(|icons| icons.loaded)
                .unwrap_or(false);

            if !icons_ready {
                render_loading(ui);
                return;
            }

            let total = item_defs.item_count();
            // Single filter pass shared by the chip-bar count and the grid.
            let needle = filters.name_needle();
            let needle_ref = needle.as_deref();
            let mut filtered: Vec<(&ItemId, &ItemConfig)> = item_defs
                .iter()
                .filter(|(_, item)| filters.matches(item, needle_ref))
                .collect();
            filtered.sort_unstable_by(|(_, a), (_, b)| {
                slot_order(a.slot).cmp(&slot_order(b.slot))
                    .then(b.item_level.cmp(&a.item_level))
                    .then(a.name.as_str().cmp(b.name.as_str()))
            });
            let visible = filtered.len();
            render_chip_bar(ui, &mut filters, total, visible);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            render_grid(ui, &filtered, &mut filters, item_icons.as_deref());
        });
}

// ============================================================================
// SUB-COMPONENTS
// ============================================================================

/// Renders the top header: "← Back" button on the left, "ARMORY" title centered.
fn render_header(ui: &mut egui::Ui, next_state: &mut NextState<GameState>) {
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("← Back")
                        .size(18.0)
                        .color(BUTTON_TEXT),
                )
                .frame(true),
            )
            .clicked()
        {
            info!("Armory back button pressed - returning to MainMenu");
            next_state.set(GameState::MainMenu);
        }

        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("ARMORY")
                    .size(48.0)
                    .color(TITLE_GOLD),
            );
        });
    });
}

/// Centered "Loading..." text while item icons are still being registered.
fn render_loading(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.label(
            egui::RichText::new("Loading...")
                .size(20.0)
                .color(MUTED_TEXT),
        );
    });
}

/// Renders the two-row filter chip-bar.
/// Row 1: SLOT chips. Row 2: TYPE chips + iLvl min/max + Search + count + Clear.
fn render_chip_bar(
    ui: &mut egui::Ui,
    filters: &mut ArmoryFilters,
    total: usize,
    visible: usize,
) {
    // Row 1 — Slot chips.
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("SLOT:").size(13.0).color(MUTED_TEXT));
        for (label, slots) in SLOT_CHIPS {
            // Use `all` rather than `any` so the chip's active state matches its
            // toggle semantics — paired slots (Ring1/Ring2, Trinket1/Trinket2)
            // stay in lockstep even if external state ever drifts.
            let active = slots.iter().all(|s| filters.selected_slots.contains(s));
            if ui.selectable_label(active, *label).clicked() {
                if active {
                    for s in *slots {
                        filters.selected_slots.remove(s);
                    }
                } else {
                    for s in *slots {
                        filters.selected_slots.insert(*s);
                    }
                }
            }
        }
    });

    ui.add_space(4.0);

    // Row 2 — Armor type / iLvl range / search / count / clear.
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("TYPE:").size(13.0).color(MUTED_TEXT));
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

        ui.label(egui::RichText::new("iLvl").size(13.0).color(MUTED_TEXT));
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
        // Keep min ≤ max by yielding to whichever side the user just moved,
        // so the active drag never "bounces" the other way.
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
                .hint_text("Search...")
                .desired_width(140.0),
        );

        ui.separator();

        ui.label(
            egui::RichText::new(format!("{} / {} items", visible, total))
                .size(13.0)
                .color(MUTED_TEXT),
        );

        if ui
            .button(
                egui::RichText::new("Clear filters")
                    .size(13.0)
                    .color(BUTTON_TEXT),
            )
            .clicked()
        {
            *filters = ArmoryFilters::default();
        }
    });
}

/// Renders the scrollable wrapping grid of item tiles. Items are sorted by
/// (slot_order, item_level desc, name). When no items match, shows the empty
/// state with an inline "Clear filters" button.
fn render_grid(
    ui: &mut egui::Ui,
    items: &[(&ItemId, &ItemConfig)],
    filters: &mut ArmoryFilters,
    item_icons: Option<&ItemIcons>,
) {

    if items.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(
                egui::RichText::new("No items match these filters.")
                    .size(16.0)
                    .color(MUTED_TEXT),
            );
            ui.add_space(12.0);
            if ui
                .button(
                    egui::RichText::new("Clear filters")
                        .size(14.0)
                        .color(BUTTON_TEXT),
                )
                .clicked()
            {
                *filters = ArmoryFilters::default();
            }
        });
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for (id, item) in items {
                    let response = tile_ui(ui, id, item, item_icons);
                    let item_for_tooltip = *item;
                    response.on_hover_ui(|ui| render_item_tooltip(ui, item_for_tooltip));
                }
            });
        });
}

/// Renders a single item tile: uniform frame, item icon centered, item-level
/// badge at the bottom-right corner. Returns the response so callers can
/// attach hover tooltips.
fn tile_ui(
    ui: &mut egui::Ui,
    item_id: &ItemId,
    item: &ItemConfig,
    item_icons: Option<&ItemIcons>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(TILE_SIZE, TILE_SIZE),
        egui::Sense::hover(),
    );

    let painter = ui.painter();

    // Tile background and frame.
    painter.rect_filled(rect, 4.0, TILE_BG);
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, TILE_FRAME));

    // Centered icon.
    let icon_rect = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2(ICON_SIZE, ICON_SIZE),
    );
    if let Some(icons) = item_icons {
        if let Some(&texture_id) = icons.textures.get(item_id) {
            painter.image(
                texture_id,
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            // Icon not loaded for this specific item — show a subtle placeholder.
            painter.rect_filled(icon_rect, 2.0, TILE_FRAME);
        }
    } else {
        painter.rect_filled(icon_rect, 2.0, TILE_FRAME);
    }

    // Item-level badge at bottom-right.
    let badge_text = item.item_level.to_string();
    let badge_size = egui::vec2(22.0, 14.0);
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.max.x - badge_size.x - TILE_PADDING / 2.0,
            rect.max.y - badge_size.y - TILE_PADDING / 2.0,
        ),
        badge_size,
    );
    painter.rect_filled(badge_rect, 2.0, BADGE_BG);
    painter.text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        badge_text,
        egui::FontId::proportional(11.0),
        TITLE_GOLD,
    );

    response
}

// ============================================================================
// HELPERS
// ============================================================================

/// Canonical ordering index for slots. Lower values sort first.
/// Mirrors `ItemSlot::all()` ordering.
fn slot_order(slot: ItemSlot) -> usize {
    match slot {
        ItemSlot::Head      =>  0,
        ItemSlot::Neck      =>  1,
        ItemSlot::Shoulders =>  2,
        ItemSlot::Back      =>  3,
        ItemSlot::Chest     =>  4,
        ItemSlot::Wrists    =>  5,
        ItemSlot::Hands     =>  6,
        ItemSlot::Waist     =>  7,
        ItemSlot::Legs      =>  8,
        ItemSlot::Feet      =>  9,
        ItemSlot::Ring1     => 10,
        ItemSlot::Ring2     => 11,
        ItemSlot::Trinket1  => 12,
        ItemSlot::Trinket2  => 13,
        ItemSlot::MainHand  => 14,
        ItemSlot::OffHand   => 15,
        ItemSlot::Ranged    => 16,
    }
}
