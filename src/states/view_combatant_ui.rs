//! View Combatant UI - Character Details Screen
//!
//! This module displays detailed information about a combatant:
//! - Base stats (health, resource, attack/spell power, attack/move speed)
//! - List of abilities with icons
//! - Equipment loadout editor (view/change gear per slot)
//!
//! Accessed by clicking a filled character slot in Configure Match.
//!
//! The Document template: the shared screen header, then one scrolling column
//! of kit panels. The class's pre-match choice (opener, shout, armor, aura,
//! pet, curses) is a row of kit choice tiles, and the item picker is a kit
//! modal.
//!
//! ## Reference lives in the encyclopedia
//!
//! This screen EDITS a loadout; it does not document the game. Its reference
//! surfaces — the class header, the kit list, the equipment picker — are
//! click-throughs into the encyclopedia's pages for the same entity, and their
//! hover tooltips say only enough to choose by. Nothing here re-renders prose
//! the encyclopedia owns.
//!
//! Where primary click is already the EDIT — equipping an item — the reference
//! affordance is the SECONDARY click, announced in that panel's own chrome
//! rather than inside a borrowed tooltip. The pre-match choices are radio
//! selects: their tiles hover to the same slim summary the kit rows show, and
//! the kit rows are where every one of those abilities links on.

use super::configure_match_ui::ClassIcons;
use super::play_match::ability_config::AbilityDefinitions;
use super::play_match::components::{class_base_stats, ClassBaseStats, PetType, ResourceType};
use super::play_match::equipment::{
    enforce_two_hand_conflicts, find_one_handed_mainhand, resolve_equipped_loadout,
    resolve_loadout, DefaultLoadouts, HeldSlot, ItemConfig, ItemDefinitions, ItemId, ItemSlot,
    Loadout,
};
use super::play_match::rendering::{icon_load_settled, item_aura_icons, GENERIC_AURA_ICONS};
use super::play_match::AbilityType;
use super::{
    match_config::{
        CharacterClass, HunterPetType, MageArmor, MatchConfig, PaladinAura, RogueOpener,
        RoguePoison, WarlockCurse, WarriorShout,
    },
    GameState,
};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;
// Item presentation lives in the encyclopedia's Items section — the loadout
// editor renders the same tooltip and stat line so the two never drift.
use super::encyclopedia::items::{format_item_stats, render_item_tooltip};
// This screen is a loadout EDITOR, not a reference work: every fact about an
// ability, item or class it shows is one the encyclopedia already owns, so it
// links there instead of reprinting it. `widget::link_with` carries the shared
// affordance (pointing hand, hover tooltip, "click to open"), and
// `EncyclopediaState::open_at` makes the target page the root of a fresh stack
// so the first Back comes straight back here.
use super::chrome::ScreenNav;
use super::encyclopedia::{
    abilities as encyclopedia_abilities, widget, EncyclopediaData, EncyclopediaState, Topic,
};
use crate::ui::driver as ui_driver;
use crate::ui::theme::{class_color, color, data, icon, size, space, text};
use crate::ui::widgets::panel::margin_xy;
use crate::ui::widgets::{
    class_icon, empty_state, icon_tile, list_row, panel, panel_header, screen_header,
    section_label, stat_row, Button, ChoiceTile, ListIcon, ListRow, Modal, Nav, NavAction, Variant,
};

/// Tracks which equipment slot has its picker open (if any)
#[derive(Default)]
pub struct EquipmentPickerState {
    open_slot: Option<ItemSlot>,
}

/// Resource to track which combatant is being viewed.
/// Inserted when navigating from Configure Match to this screen.
#[derive(Resource)]
pub struct ViewCombatantState {
    /// The class being viewed
    pub class: CharacterClass,
    /// Which team the combatant is on (1 or 2)
    pub team: u8,
    /// Which slot in the team (0-2)
    pub slot: usize,
}

/// Resource storing loaded ability icon textures for the view combatant screen.
///
/// Also carries the GENERIC AURA icons, under their `GENERIC_AURA_ICONS` keys.
/// They share one map because they share one keyspace: the encyclopedia asks
/// `auras::icon_key` for an aura's icon and gets back either an ability's
/// display name or a generic key, exactly as the in-match `SpellIcons` map
/// already works. Without them, an aura no ability applies — Shadow Sight — had
/// no key to look up and rendered the placeholder tile.
#[derive(Resource, Default)]
pub struct AbilityIcons {
    /// Map of ability name (or generic aura key) to egui texture ID
    pub textures: HashMap<String, egui::TextureId>,
    /// Whether icons have been loaded
    pub loaded: bool,
}

/// Resource storing the Bevy image handles for ability icons.
#[derive(Resource, Default)]
pub struct AbilityIconHandles {
    pub handles: Vec<(String, Handle<Image>)>,
}

/// Resource storing loaded item icon textures for the view combatant screen.
#[derive(Resource, Default)]
pub struct ItemIcons {
    pub textures: HashMap<ItemId, egui::TextureId>,
    pub loaded: bool,
}

/// Resource storing the Bevy image handles for item icons.
#[derive(Resource, Default)]
pub struct ItemIconHandles {
    pub handles: Vec<(ItemId, Handle<Image>)>,
}

/// Resource storing loaded hunter pet family icon textures.
#[derive(Resource, Default)]
pub struct HunterPetIcons {
    pub textures: HashMap<HunterPetType, egui::TextureId>,
    pub loaded: bool,
}

/// Resource storing the Bevy image handles for hunter pet icons.
#[derive(Resource, Default)]
pub struct HunterPetIconHandles {
    pub handles: Vec<(HunterPetType, Handle<Image>)>,
}

/// Equipment stat contributions for the stats panel
#[derive(Default)]
struct EquipmentBonuses {
    health: f32,
    mana: f32,
    mana_regen: f32,
    attack_power: f32,
    spell_power: f32,
    crit_chance: f32,
    move_speed: f32,
    armor: f32,
    fire_resistance: f32,
    frost_resistance: f32,
    shadow_resistance: f32,
    arcane_resistance: f32,
    nature_resistance: f32,
    holy_resistance: f32,
    /// The live weapon socket's weapon speed, in seconds per swing — the
    /// value `apply_equipment` gives the combatant. None means no weapon in
    /// that socket, and so no auto-attack.
    weapon_speed: Option<f32>,
}

impl EquipmentBonuses {
    fn from_loadout(loadout: &Loadout, items: &ItemDefinitions, class: CharacterClass) -> Self {
        let mut bonuses = Self::default();
        // Same socket the sim replaces stats from, so this screen cannot
        // report a weapon speed the combatant does not actually swing at.
        let primary_weapon_slot = class.weapon_slot();
        for (slot, item_id) in loadout {
            if let Some(item) = items.get(item_id) {
                bonuses.health += item.max_health;
                bonuses.mana += item.max_mana;
                bonuses.mana_regen += item.mana_regen;
                bonuses.attack_power += item.attack_power;
                bonuses.spell_power += item.spell_power;
                bonuses.crit_chance += item.crit_chance;
                bonuses.move_speed += item.movement_speed;
                bonuses.armor += item.armor;
                bonuses.fire_resistance += item.fire_resistance;
                bonuses.frost_resistance += item.frost_resistance;
                bonuses.shadow_resistance += item.shadow_resistance;
                bonuses.arcane_resistance += item.arcane_resistance;
                bonuses.nature_resistance += item.nature_resistance;
                bonuses.holy_resistance += item.holy_resistance;
                // The live socket's weapon sets the swing speed
                if *slot == primary_weapon_slot && item.is_weapon && item.weapon_speed > 0.0 {
                    bonuses.weapon_speed = Some(item.weapon_speed);
                }
            }
        }
        bonuses
    }
}

/// System to load ability icons for the view combatant screen.
pub fn load_ability_icons(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    mut ability_icons: ResMut<AbilityIcons>,
    mut icon_handles: ResMut<AbilityIconHandles>,
    images: Res<Assets<Image>>,
    ability_definitions: Res<AbilityDefinitions>,
    item_definitions: Res<ItemDefinitions>,
) {
    // Only load once
    if ability_icons.loaded {
        return;
    }

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        // Load ability icons from data-driven definitions
        for (_ability_type, config) in ability_definitions.iter() {
            if !config.icon.is_empty() {
                let handle: Handle<Image> = asset_server.load(&config.icon);
                icon_handles.handles.push((config.name.clone(), handle));
            }
        }
        // The aura art that belongs to no ability, keyed the same way the
        // actor frames key it, so the encyclopedia and the buff bar can never
        // draw different icons for one aura.
        for (key, path) in GENERIC_AURA_ICONS {
            let handle: Handle<Image> = asset_server.load(*path);
            icon_handles.handles.push((key.to_string(), handle));
        }
        // An item-applied aura (a proc buff) draws its item, under the same
        // key the buff bar uses.
        for (key, path) in item_aura_icons(&item_definitions) {
            let handle: Handle<Image> = asset_server.load(path);
            icon_handles.handles.push((key, handle));
        }
        return; // Wait for next frame to check if loaded
    }

    // Wait while any handle is still resolving — but a FAILED load (a missing
    // or undecodable file) counts as resolved, exactly as in the in-match
    // loader, so one bad icon cannot hold back every other icon on the
    // encyclopedia and View Combatant screens.
    let still_loading = icon_handles
        .handles
        .iter()
        .any(|(_, h)| !icon_load_settled(&asset_server.load_state(h.id())));
    if still_loading {
        return; // Wait for images to finish loading or fail
    }

    // Register textures with egui, skipping any that failed to load
    for (ability_name, handle) in &icon_handles.handles {
        if !images.contains(handle) {
            warn!(
                "Ability icon for '{}' failed to load; rendering without it",
                ability_name
            );
            continue;
        }
        let texture_id = contexts.add_image(handle.clone());
        ability_icons
            .textures
            .insert(ability_name.clone(), texture_id);
    }

    ability_icons.loaded = true;
    info!("Ability icons loaded for view combatant screen");
}

/// System to load item icons and register them with egui.
/// Follows the same 3-phase pattern as load_ability_icons.
pub fn load_item_icons(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    mut item_icons: ResMut<ItemIcons>,
    mut icon_handles: ResMut<ItemIconHandles>,
    images: Res<Assets<Image>>,
    item_definitions: Res<ItemDefinitions>,
) {
    if item_icons.loaded {
        return;
    }

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        for (item_id, item) in item_definitions.iter() {
            if !item.icon.is_empty() {
                let handle: Handle<Image> = asset_server.load(item.icon.as_str());
                icon_handles.handles.push((*item_id, handle));
            }
        }
        return;
    }

    // Check if all images are loaded
    let all_loaded = icon_handles.handles.iter().all(|(_, h)| images.contains(h));
    if !all_loaded {
        return;
    }

    // Register textures with egui
    for (item_id, handle) in &icon_handles.handles {
        let texture_id = contexts.add_image(handle.clone());
        item_icons.textures.insert(*item_id, texture_id);
    }

    item_icons.loaded = true;
    info!(
        "Item icons loaded for view combatant screen ({} icons)",
        item_icons.textures.len()
    );
}

/// System to load hunter pet family icons.
/// Follows the same 3-phase pattern as load_ability_icons.
pub fn load_hunter_pet_icons(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    mut pet_icons: ResMut<HunterPetIcons>,
    mut icon_handles: ResMut<HunterPetIconHandles>,
    images: Res<Assets<Image>>,
) {
    if pet_icons.loaded {
        return;
    }

    if icon_handles.handles.is_empty() {
        let pets = [
            (
                HunterPetType::Spider,
                "icons/abilities/ability_hunter_pet_spider.jpg",
            ),
            (
                HunterPetType::Boar,
                "icons/abilities/ability_hunter_pet_boar.jpg",
            ),
            (
                HunterPetType::Bird,
                "icons/abilities/ability_hunter_pet_owl.jpg",
            ),
        ];
        for (pet, path) in pets {
            let handle: Handle<Image> = asset_server.load(path);
            icon_handles.handles.push((pet, handle));
        }
        return;
    }

    let all_loaded = icon_handles.handles.iter().all(|(_, h)| images.contains(h));
    if !all_loaded {
        return;
    }

    for (pet, handle) in &icon_handles.handles {
        let texture_id = contexts.add_image(handle.clone());
        pet_icons.textures.insert(*pet, texture_id);
    }

    pet_icons.loaded = true;
    info!("Hunter pet icons loaded for view combatant screen");
}

/// A user action produced by the View Combatant screen that needs ECS side
/// effects (a state transition, a resource removal). Every edit that only
/// mutates `MatchConfig` or the equipment picker is applied inside the pure
/// draw function; only these bubble out to the Bevy wrapper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewCombatantAction {
    /// The header exit was clicked: leave for wherever opened the screen
    /// (Configure Match), dropping `ViewCombatantState`.
    Exit,
    /// A reference surface was clicked: open the encyclopedia on this topic,
    /// returning here. `ViewCombatantState` is deliberately LEFT IN PLACE — it
    /// is what makes Back come home to this same team and slot, mid-pick
    /// picker and all.
    OpenEncyclopedia(Topic),
}

/// Everything [`draw_view_combatant`] reads but never writes: the config
/// definitions and the lazily-loaded icon maps. Plain references, so an egui
/// harness can build one without a Bevy app.
pub struct ViewCombatantData<'a> {
    pub abilities: &'a AbilityDefinitions,
    pub items: &'a ItemDefinitions,
    pub default_loadouts: &'a DefaultLoadouts,
    pub class_icons: &'a ClassIcons,
    pub ability_icons: Option<&'a AbilityIcons>,
    pub item_icons: Option<&'a ItemIcons>,
    pub pet_icons: Option<&'a HunterPetIcons>,
}

/// Main UI system for the View Combatant screen (Bevy wrapper).
///
/// Grabs the egui context and resources, drives the pure
/// [`draw_view_combatant`] renderer, and applies the returned
/// [`ViewCombatantAction`] via ECS.
pub fn view_combatant_ui(
    mut contexts: EguiContexts,
    mut nav: ScreenNav,
    view_state: Option<Res<ViewCombatantState>>,
    mut commands: Commands,
    class_icons: Res<ClassIcons>,
    ability_icons: Option<Res<AbilityIcons>>,
    item_icons: Option<Res<ItemIcons>>,
    pet_icons: Option<Res<HunterPetIcons>>,
    ability_definitions: Res<AbilityDefinitions>,
    mut match_config: ResMut<MatchConfig>,
    item_definitions: Res<ItemDefinitions>,
    default_loadouts: Res<DefaultLoadouts>,
    mut picker_state: Local<EquipmentPickerState>,
    mut encyclopedia: ResMut<EncyclopediaState>,
) {
    // Esc closes the item picker first, then takes the header's exit. It
    // needs no egui context, so it answers on any frame.
    let screen_nav = nav.nav(false).expect("View Combatant has an exit");
    let escape = nav.escape(false, || picker_state.open_slot.take().is_some());
    if let Some(NavAction::Exit(to)) = escape {
        commands.remove_resource::<ViewCombatantState>();
        nav.leave(to);
        return;
    }

    // Use try_ctx_mut to avoid panic when context isn't ready
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    let data = ViewCombatantData {
        abilities: &ability_definitions,
        items: &item_definitions,
        default_loadouts: &default_loadouts,
        class_icons: &class_icons,
        ability_icons: ability_icons.as_deref(),
        item_icons: item_icons.as_deref(),
        pet_icons: pet_icons.as_deref(),
    };

    match draw_view_combatant(
        ctx,
        view_state.as_deref(),
        &mut match_config,
        &mut picker_state,
        &data,
        screen_nav,
    ) {
        Some(ViewCombatantAction::Exit) => {
            commands.remove_resource::<ViewCombatantState>();
            nav.leave(screen_nav.destination());
        }
        Some(ViewCombatantAction::OpenEncyclopedia(topic)) => {
            encyclopedia.open_at(topic);
            nav.open(GameState::Encyclopedia);
        }
        None => {}
    }
}

/// The screen's title, in the shared header.
const TITLE: &str = "View Combatant";

/// The widest the sheet grows: the Document template's body measure, wide
/// enough for two panels side by side.
const CONTENT_MAX_W: f32 = 820.0;

/// The item picker's `--ui-script` and egui id; its close `×` registers as
/// `item-picker:close`.
const ITEM_PICKER_ID: &str = "item-picker";

/// The tallest the item picker's list grows, as a fraction of the screen.
const PICKER_MAX_HEIGHT: f32 = 0.6;

/// Render the entire View Combatant screen into `ctx`, returning any action
/// that needs ECS side effects (see [`ViewCombatantAction`]).
///
/// The Document template: the shared [`screen_header`], then a scrolling body
/// at [`CONTENT_MAX_W`] — the class header, the class's pre-match choice,
/// stats beside the kit, and the equipment sheet — with the item picker as a kit
/// [`Modal`] over it.
///
/// Deliberately free of Bevy ECS system params (it takes plain references) so
/// it can be driven directly by an egui harness — see
/// `tests/view_combatant_snapshot.rs`. Loadout and strategic-option edits are
/// applied to `match_config` in place, and the equipment picker's open socket
/// lives in `picker_state`; only navigation bubbles out. `view_state` is
/// `None` when the screen was entered without a combatant, which draws an
/// empty state under the same header.
pub fn draw_view_combatant(
    ctx: &egui::Context,
    view_state: Option<&ViewCombatantState>,
    match_config: &mut MatchConfig,
    picker_state: &mut EquipmentPickerState,
    data: &ViewCombatantData,
    nav: Nav,
) -> Option<ViewCombatantAction> {
    let mut exit = false;
    // Set by whichever reference surface was clicked this frame, and applied
    // once after the frame is drawn — so navigation is one decision at the end
    // rather than a state transition fired from inside a draw closure.
    let mut open_topic: Option<Topic> = None;

    // Everything the shared encyclopedia widgets need to draw a link and its
    // tooltip. This screen already held every one of these resources.
    let encyclopedia_data = EncyclopediaData {
        items: data.items,
        abilities: data.abilities,
        item_icons: data.item_icons,
        class_icons: Some(data.class_icons),
        ability_icons: data.ability_icons,
    };

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(margin_xy(space::XL, space::LG)),
        )
        .show(ctx, |ui| {
            if screen_header(ui, TITLE, nav, |_| {}).is_some() {
                exit = true;
            }
            ui.add_space(space::LG);

            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(CONTENT_MAX_W);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            match view_state {
                                Some(view_state) => {
                                    open_topic = draw_sheet(
                                        ui,
                                        view_state,
                                        match_config,
                                        picker_state,
                                        data,
                                        &encyclopedia_data,
                                    );
                                }
                                None => {
                                    empty_state(
                                        ui,
                                        "view-combatant:empty",
                                        "No combatant selected",
                                        "Open a filled slot from Match Setup to view its combatant.",
                                        None,
                                    );
                                }
                            }
                        });
                    });
                });
        });

    // The picker, over the whole screen. Drawn after the sheet so its modal
    // layer is the topmost.
    if let (Some(view_state), Some(open_slot)) = (view_state, picker_state.open_slot) {
        let class = view_state.class;
        let overrides = equipment_overrides(match_config, view_state);
        let resolved =
            resolve_equipped_loadout(class, data.default_loadouts, &overrides, data.items);
        // Anything already worn in the sibling socket is absent — items are
        // unique-equipped, so it is not selectable here.
        let valid_items = data
            .items
            .selectable_items_for_slot(open_slot, class, &resolved);
        let picked = draw_item_picker(
            ctx,
            ITEM_PICKER_ID,
            open_slot,
            &valid_items,
            resolved.get(&open_slot),
            data.item_icons,
        );
        if let Some(id) = picked.read {
            open_topic = open_topic.or(Some(Topic::Item(id)));
        }
        if let Some(item_id) = picked.equip {
            set_equipment_override(
                match_config,
                view_state,
                open_slot,
                item_id,
                data.items,
                data.default_loadouts,
                class,
            );
            picker_state.open_slot = None;
        } else if picked.close {
            picker_state.open_slot = None;
        }
    }

    if exit {
        return Some(ViewCombatantAction::Exit);
    }
    open_topic.map(ViewCombatantAction::OpenEncyclopedia)
}

/// The viewed combatant's equipment overrides (empty if its slot is missing).
fn equipment_overrides(match_config: &MatchConfig, view_state: &ViewCombatantState) -> Loadout {
    let team = if view_state.team == 1 {
        &match_config.team1_equipment
    } else {
        &match_config.team2_equipment
    };
    team.get(view_state.slot).cloned().unwrap_or_default()
}

/// This combatant's value in a per-slot `MatchConfig` table, or its default.
fn slot_value<T: Copy + Default>(team1: &[T], team2: &[T], view_state: &ViewCombatantState) -> T {
    let team = if view_state.team == 1 { team1 } else { team2 };
    team.get(view_state.slot).copied().unwrap_or_default()
}

/// The sheet itself: everything under the header for one combatant. Returns
/// the topic a reference surface asked to open this frame.
fn draw_sheet(
    ui: &mut egui::Ui,
    view_state: &ViewCombatantState,
    match_config: &mut MatchConfig,
    picker_state: &mut EquipmentPickerState,
    data: &ViewCombatantData,
    encyclopedia_data: &EncyclopediaData,
) -> Option<Topic> {
    let mut open_topic: Option<Topic> = None;
    let class = view_state.class;
    // Base stats and the class kit are both DERIVED: `class_base_stats` is the
    // same table `Combatant::new` builds from, and the ability list comes from
    // the `class` attribution on each `abilities.ron` entry. Neither can drift
    // from the sim or silently drop ability N+1.
    let stats = class_base_stats(class);
    let abilities = data.abilities.own_abilities_for_class(class);

    // Compute equipment bonuses for the stats panel
    let equip_overrides = equipment_overrides(match_config, view_state);
    let resolved_loadout =
        resolve_equipped_loadout(class, data.default_loadouts, &equip_overrides, data.items);
    let equip_bonuses = EquipmentBonuses::from_loadout(&resolved_loadout, data.items, class);

    open_topic = open_topic.or(render_class_header(ui, class, data, encyclopedia_data));

    // The class's pre-match choice, if it has one, straight under the class
    // header: it is the decision this screen exists for, so it sits above the
    // fold of the default 1280x720 window whatever the length of the kit.
    let ability_icon = |key: &str| {
        data.ability_icons
            .and_then(|icons| icons.textures.get(key).copied())
    };
    let pick_section = |ui: &mut egui::Ui| ui.add_space(space::LG);
    match class {
        CharacterClass::Rogue => {
            pick_section(ui);
            let current = slot_value(
                &match_config.team1_rogue_openers,
                &match_config.team2_rogue_openers,
                view_state,
            );
            let options = [
                (RogueOpener::Ambush, ability_icon("Ambush")),
                (RogueOpener::CheapShot, ability_icon("Cheap Shot")),
            ];
            if let Some(opener) =
                strategic_option_panel(ui, "Stealth Opener", &options, current, encyclopedia_data)
            {
                set_slot_value(
                    &mut match_config.team1_rogue_openers,
                    &mut match_config.team2_rogue_openers,
                    view_state,
                    opener,
                );
            }

            // The weapon poison, offered from `RoguePoison::ALL` so poison N+1
            // needs no edit here. Its icon is the poison's own ability art.
            pick_section(ui);
            let current = slot_value(
                &match_config.team1_rogue_poisons,
                &match_config.team2_rogue_poisons,
                view_state,
            );
            let options: Vec<(RoguePoison, Option<egui::TextureId>)> = RoguePoison::ALL
                .into_iter()
                .map(|poison| (poison, ability_icon(poison.name())))
                .collect();
            if let Some(poison) =
                strategic_option_panel(ui, "Weapon Poison", &options, current, encyclopedia_data)
            {
                set_slot_value(
                    &mut match_config.team1_rogue_poisons,
                    &mut match_config.team2_rogue_poisons,
                    view_state,
                    poison,
                );
            }
        }
        CharacterClass::Warrior => {
            pick_section(ui);
            let current = slot_value(
                &match_config.team1_warrior_shouts,
                &match_config.team2_warrior_shouts,
                view_state,
            );
            let options = [
                (WarriorShout::BattleShout, ability_icon("Battle Shout")),
                (
                    WarriorShout::DemoralizingShout,
                    ability_icon("Demoralizing Shout"),
                ),
                (
                    WarriorShout::CommandingShout,
                    ability_icon("Commanding Shout"),
                ),
            ];
            if let Some(shout) =
                strategic_option_panel(ui, "Battle Shout", &options, current, encyclopedia_data)
            {
                set_slot_value(
                    &mut match_config.team1_warrior_shouts,
                    &mut match_config.team2_warrior_shouts,
                    view_state,
                    shout,
                );
            }
        }
        CharacterClass::Mage => {
            pick_section(ui);
            let current = slot_value(
                &match_config.team1_mage_armors,
                &match_config.team2_mage_armors,
                view_state,
            );
            let options = [
                (MageArmor::FrostArmor, ability_icon("Frost Armor")),
                (MageArmor::MageArmor, ability_icon("Mage Armor")),
                (MageArmor::MoltenArmor, ability_icon("Molten Armor")),
            ];
            if let Some(armor) =
                strategic_option_panel(ui, "Mage Armor", &options, current, encyclopedia_data)
            {
                set_slot_value(
                    &mut match_config.team1_mage_armors,
                    &mut match_config.team2_mage_armors,
                    view_state,
                    armor,
                );
            }
        }
        CharacterClass::Paladin => {
            pick_section(ui);
            let current = slot_value(
                &match_config.team1_paladin_auras,
                &match_config.team2_paladin_auras,
                view_state,
            );
            let options = [
                (PaladinAura::DevotionAura, ability_icon("Devotion Aura")),
                (
                    PaladinAura::ShadowResistanceAura,
                    ability_icon("Shadow Resistance Aura"),
                ),
                (
                    PaladinAura::ConcentrationAura,
                    ability_icon("Concentration Aura"),
                ),
            ];
            if let Some(aura) =
                strategic_option_panel(ui, "Paladin Aura", &options, current, encyclopedia_data)
            {
                set_slot_value(
                    &mut match_config.team1_paladin_auras,
                    &mut match_config.team2_paladin_auras,
                    view_state,
                    aura,
                );
            }
        }
        CharacterClass::Hunter => {
            pick_section(ui);
            render_hunter_pet_panel(
                ui,
                view_state,
                match_config,
                data.pet_icons,
                encyclopedia_data,
            );
        }
        CharacterClass::Warlock => {
            pick_section(ui);
            render_warlock_curse_panel(
                ui,
                view_state,
                match_config,
                data.ability_icons,
                data.class_icons,
                encyclopedia_data,
            );
        }
        CharacterClass::Priest | CharacterClass::Shaman | CharacterClass::Druid => {}
    }

    // The pet whose abilities belong in THIS combatant's kit. View Combatant is
    // a loadout editor, so it shows the pet that is actually configured rather
    // than every pet the class could bring. The Pet Type panel above is where
    // that choice is made, and this is read after it, so a new pick shows in
    // the kit on the frame it is clicked. The encyclopedia, which documents the
    // whole class, uses `pet_abilities_for_class` instead.
    let active_pet: Option<PetType> = match class {
        CharacterClass::Warlock => Some(PetType::Felhunter),
        CharacterClass::Hunter => Some(
            match slot_value(
                &match_config.team1_hunter_pet_types,
                &match_config.team2_hunter_pet_types,
                view_state,
            ) {
                HunterPetType::Spider => PetType::Spider,
                HunterPetType::Boar => PetType::Boar,
                HunterPetType::Bird => PetType::Bird,
            },
        ),
        CharacterClass::Warrior
        | CharacterClass::Mage
        | CharacterClass::Rogue
        | CharacterClass::Priest
        | CharacterClass::Paladin
        | CharacterClass::Shaman
        | CharacterClass::Druid => None,
    };

    ui.add_space(space::LG);
    // Stats beside the kit, the two panels held to one height.
    let row_height_id = ui.id().with("vc:stats-kit-height");
    let held: f32 = ui
        .ctx()
        .data(|d| d.get_temp(row_height_id))
        .unwrap_or_default();
    let mut natural = [0.0_f32; 2];
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = space::LG;
        ui.columns(2, |columns| {
            natural[0] = render_stats_panel(&mut columns[0], &stats, &equip_bonuses, held);
            let (kit_height, clicked) = render_abilities_panel(
                &mut columns[1],
                &abilities,
                active_pet,
                held,
                encyclopedia_data,
            );
            natural[1] = kit_height;
            open_topic = open_topic.or(clicked);
        });
    });
    let tallest = natural[0].max(natural[1]);
    if tallest != held {
        ui.ctx().data_mut(|d| d.insert_temp(row_height_id, tallest));
        ui.ctx().request_repaint();
    }

    ui.add_space(space::LG);
    open_topic = open_topic.or(render_equipment_panel(
        ui,
        view_state,
        match_config,
        data.items,
        picker_state,
        &resolved_loadout,
        &equip_overrides,
        data.item_icons,
    ));

    open_topic
}

/// Write this combatant's value into a per-slot `MatchConfig` table. A slot
/// the table does not reach is left alone.
fn set_slot_value<T>(team1: &mut [T], team2: &mut [T], view_state: &ViewCombatantState, value: T) {
    let team = if view_state.team == 1 { team1 } else { team2 };
    if let Some(v) = team.get_mut(view_state.slot) {
        *v = value;
    }
}

/// The class header: a [`panel`] edged in the class colour, holding the
/// class's icon, its name and its one-line description. Icon and name are
/// one affordance split in two — either opens the class's encyclopedia page.
fn render_class_header(
    ui: &mut egui::Ui,
    class: CharacterClass,
    data: &ViewCombatantData,
    encyclopedia_data: &EncyclopediaData,
) -> Option<Topic> {
    let tint = class_color(class);
    let mut clicked = None;
    panel(Some(tint)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::MD;
            let (rect, response) =
                ui.allocate_exact_size(egui::Vec2::splat(icon::HERO), egui::Sense::click());
            icon_tile(
                ui.painter(),
                rect,
                data.class_icons.textures.get(&class).copied(),
                tint,
            );
            clicked = clicked.or(widget::link(
                response,
                Topic::Class(class),
                encyclopedia_data,
            ));

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::XXS;
                let name = ui.add(
                    egui::Label::new(
                        egui::RichText::new(class.name().to_uppercase())
                            .size(text::H2)
                            .color(tint),
                    )
                    .sense(egui::Sense::click()),
                );
                clicked = clicked.or(widget::link(name, Topic::Class(class), encyclopedia_data));
                widget::prose(ui, class.description(), text::BODY, color::TEXT_MUTED);
            });
        });
    });
    clicked
}

/// The colour a stat's value is drawn in: [`data::ITEM_BONUS`] when the worn
/// gear raised it, [`color::DANGER`] were it ever to lower it.
fn bonus_color(bonus: f32) -> egui::Color32 {
    if bonus > 0.0 {
        data::ITEM_BONUS
    } else if bonus < 0.0 {
        color::DANGER
    } else {
        color::TEXT
    }
}

/// One stats row whose value equipment changed: drawn in [`bonus_color`],
/// and explained on hover.
fn bonus_row(ui: &mut egui::Ui, key: &str, value: &str, bonus: f32, explain: impl Fn() -> String) {
    let response = stat_row(ui, key, value, bonus_color(bonus));
    if bonus != 0.0 {
        response.on_hover_text(explain());
    }
}

/// Render the Stats panel with effective totals (base + equipment). A stat
/// the worn gear raised is green, and its hover gives the breakdown. Held to
/// at least `min_height`; returns the height its content needs.
fn render_stats_panel(
    ui: &mut egui::Ui,
    stats: &ClassBaseStats,
    equip: &EquipmentBonuses,
    min_height: f32,
) -> f32 {
    panel(None)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Held before anything is drawn: egui grows a minimum height
            // from the cursor, not from the panel's top.
            ui.set_min_height(min_height);
            // Measured in a child of its own, so the height held does not
            // feed back into the next frame's measure.
            let natural = ui
                .vertical(|ui| {
                    panel_header(ui, "Stats", color::ACCENT, |_| {});

                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = space::XS;

                        let health = stats.max_health as i32;
                        let health_bonus = equip.health as i32;
                        bonus_row(
                            ui,
                            "Health",
                            &(health + health_bonus).to_string(),
                            health_bonus as f32,
                            || format!("{health} + {health_bonus} from equipment"),
                        );

                        // Resource: only mana takes a bonus from gear.
                        let mana_bonus = if stats.resource_type == ResourceType::Mana {
                            equip.mana as i32
                        } else {
                            0
                        };
                        let resource = stats.max_resource as i32;
                        bonus_row(
                            ui,
                            "Resource",
                            &format!("{} {}", stats.resource_type.name(), resource + mana_bonus),
                            mana_bonus as f32,
                            || format!("{resource} + {mana_bonus} from equipment"),
                        );

                        let ap = stats.attack_power as i32;
                        let ap_bonus = equip.attack_power as i32;
                        bonus_row(
                            ui,
                            "Attack Power",
                            &(ap + ap_bonus).to_string(),
                            ap_bonus as f32,
                            || format!("{ap} + {ap_bonus} from equipment"),
                        );
                        let sp = stats.spell_power as i32;
                        let sp_bonus = equip.spell_power as i32;
                        bonus_row(
                            ui,
                            "Spell Power",
                            &(sp + sp_bonus).to_string(),
                            sp_bonus as f32,
                            || format!("{sp} + {sp_bonus} from equipment"),
                        );

                        // Crit chance: every class has a non-zero base (Rogue 10% down
                        // to Priest 4%), so this row always shows.
                        bonus_row(
                            ui,
                            "Crit Chance",
                            &format!("{:.1}%", (stats.crit_chance + equip.crit_chance) * 100.0),
                            equip.crit_chance,
                            || {
                                format!(
                                    "{:.1}% base + {:.1}% from equipment",
                                    stats.crit_chance * 100.0,
                                    equip.crit_chance * 100.0
                                )
                            },
                        );

                        // Mana regen (only shown when equipment provides it)
                        if equip.mana_regen > 0.0 {
                            bonus_row(
                                ui,
                                "Mana Regen",
                                &format!("+{:.1} MP5", equip.mana_regen),
                                equip.mana_regen,
                                || format!("{:.1} from equipment", equip.mana_regen),
                            );
                        }

                        // Weapon speed: seconds per swing, straight from the weapon in
                        // the live socket — there is no class speed to compare it to.
                        let speed = match equip.weapon_speed {
                            Some(speed) => format!("{speed:.2}s"),
                            None => "No weapon".to_string(),
                        };
                        stat_row(ui, "Weapon Speed", &speed, color::TEXT);

                        bonus_row(
                            ui,
                            "Move Speed",
                            &format!("{:.1}/s", stats.movement_speed + equip.move_speed),
                            equip.move_speed,
                            || {
                                format!(
                                    "{:.1} base + {:.1} from equipment",
                                    stats.movement_speed, equip.move_speed
                                )
                            },
                        );

                        // Armor (only shown when equipment provides it, since base is 0)
                        if equip.armor > 0.0 {
                            let effective_armor = stats.armor + equip.armor;
                            let reduction_pct =
                                effective_armor / (effective_armor + 5500.0) * 100.0;
                            bonus_row(
                                ui,
                                "Armor",
                                &format!("{effective_armor:.0}"),
                                equip.armor,
                                || {
                                    format!(
                                "{:.0} from equipment ({reduction_pct:.1}% physical reduction)",
                                equip.armor
                            )
                                },
                            );
                        }

                        // Spell resistances (only non-zero values)
                        let resistances: &[(&str, f32)] = &[
                            ("Fire Resist", equip.fire_resistance),
                            ("Frost Resist", equip.frost_resistance),
                            ("Shadow Resist", equip.shadow_resistance),
                            ("Arcane Resist", equip.arcane_resistance),
                            ("Nature Resist", equip.nature_resistance),
                            ("Holy Resist", equip.holy_resistance),
                        ];
                        for &(label, value) in resistances {
                            if value > 0.0 {
                                let reduction_pct = value / (value * 5.0 / 3.0 + 300.0) * 100.0;
                                bonus_row(ui, label, &format!("{value:.0}"), value, || {
                                    format!(
                                "{value:.0} from equipment ({reduction_pct:.1}% damage reduction)"
                            )
                                });
                            }
                        }
                    });
                })
                .response
                .rect
                .height();
            natural
        })
        .inner
}

/// Render the Abilities panel: the class's kit, then the configured pet's.
///
/// Every row is a click-through to the ability's encyclopedia page. Held to at
/// least `min_height`; returns the height its content needs, and the topic
/// whose row was clicked this frame.
fn render_abilities_panel(
    ui: &mut egui::Ui,
    abilities: &[AbilityType],
    active_pet: Option<PetType>,
    min_height: f32,
    data: &EncyclopediaData,
) -> (f32, Option<Topic>) {
    panel(None)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Held before anything is drawn, as in the stats panel.
            ui.set_min_height(min_height);
            let mut clicked = None;
            let natural = ui
                .vertical(|ui| {
                    panel_header(ui, "Abilities", color::ACCENT, |_| {});

                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = space::XXS;
                        for ability in abilities {
                            clicked = clicked.or(render_ability_row(ui, *ability, data));
                        }

                        // Pet subsection, labelled by the pet that casts them.
                        if let Some(pet) = active_pet {
                            let pet_abilities = data.abilities.abilities_for_pet(pet);
                            if !pet_abilities.is_empty() {
                                ui.add_space(space::SM);
                                section_label(ui, &format!("{} (pet)", pet.name()));
                                for ability in pet_abilities {
                                    clicked = clicked.or(render_ability_row(ui, ability, data));
                                }
                            }
                        }
                    });
                })
                .response
                .rect
                .height();
            (natural, clicked)
        })
        .inner
}

/// Render one ability row: a kit [`list_row`] with the ability's icon and
/// name, a one-line hover summary, and a click through to the ability's
/// encyclopedia page.
///
/// The display name comes from the loaded `AbilityConfig`, so `abilities.ron`
/// is the only place an ability is named — and the hover text comes from the
/// encyclopedia's own builders, so this screen holds no prose of its own.
/// Returns the topic on the frame the row is clicked.
fn render_ability_row(
    ui: &mut egui::Ui,
    ability: AbilityType,
    data: &EncyclopediaData,
) -> Option<Topic> {
    let topic = Topic::Ability(ability);
    let name = topic.name(data);
    let response = list_row(
        ui,
        ListRow::new(&name).icon(ListIcon {
            texture: topic.icon(data),
            edge: color::LINE,
            tier: None,
        }),
    );
    ui_driver::mark(ui, response.rect, true, format_args!("kit:{ability:?}"));

    // The SLIM tooltip: name, stat strip, one sentence. The long-form version
    // is the encyclopedia's page, one click away.
    widget::link_with(response, topic, |ui| {
        encyclopedia_abilities::slim_tooltip(ui, ability, data)
    })
}

/// Equipment slot groups for the panel layout
const ARMOR_SLOTS: &[ItemSlot] = &[
    ItemSlot::Head,
    ItemSlot::Shoulders,
    ItemSlot::Chest,
    ItemSlot::Wrists,
    ItemSlot::Hands,
    ItemSlot::Waist,
    ItemSlot::Legs,
    ItemSlot::Feet,
];
const ACCESSORY_SLOTS: &[ItemSlot] = &[
    ItemSlot::Neck,
    ItemSlot::Back,
    ItemSlot::Ring1,
    ItemSlot::Ring2,
    ItemSlot::Trinket1,
    ItemSlot::Trinket2,
];
const WEAPON_SLOTS: &[ItemSlot] = &[ItemSlot::MainHand, ItemSlot::OffHand, ItemSlot::Ranged];

/// Render the equipment sheet — armor down the left, accessories and weapons
/// down the right — with the whole-set restore in its header.
///
/// LEFT-click is the editor: a socket row opens the picker. RIGHT-click is the
/// reference: it opens that item's encyclopedia page, from the worn row and
/// from the picker alike, so "what is this actually?" never costs you the
/// pick you were making. Returns the topic to open, if any.
fn render_equipment_panel(
    ui: &mut egui::Ui,
    view_state: &ViewCombatantState,
    match_config: &mut MatchConfig,
    items: &ItemDefinitions,
    picker_state: &mut EquipmentPickerState,
    resolved: &Loadout,
    overrides: &Loadout,
    item_icons: Option<&ItemIcons>,
) -> Option<Topic> {
    let mut clicked_slot: Option<ItemSlot> = None;
    let mut restore_clicked = false;
    let mut open_topic: Option<Topic> = None;

    // The live override map, as one greppable line. `Loadout` is a BTreeMap,
    // so the order is a property of the type rather than of a hash seed.
    //
    // Passed as a `Display` wrapper, NOT as a pre-joined `String`:
    // `format_args!` defers FORMATTING, not evaluation of its arguments, so a
    // `.collect().join()` in the argument position would run on every frame
    // the panel draws even with the driver off. `OverrideMap` writes straight
    // into the formatter, which `note` only reaches after its armed check.
    ui_driver::note(ui, format_args!("overrides {{{}}}", OverrideMap(overrides)));

    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        panel_header(ui, "Equipment", color::ACCENT, |ui| {
            // One whole-set restore rather than a per-slot reset. Clearing
            // every override lands on the RON default exactly as a fresh
            // match would, and that default is validated unique-equipped at
            // load — so unlike a single-slot reset, a restore can never
            // collide with a sibling socket's explicit pick and hand the
            // conflict to the resolver. Registered whether or not it is
            // enabled, so `assert-enabled equip:restore false` can pin the
            // disabled-with-no-overrides state.
            let restore = Button::new("\u{21a9} Restore defaults", Variant::Secondary)
                .compact()
                .enabled(!overrides.is_empty())
                .driver_id("equip:restore")
                .show(ui)
                .on_hover_text("Clear every equipment override for this combatant");
            restore_clicked = restore.clicked();
        });
        // The item tooltips stay as they are (AS-65 decision 4), so the
        // panel's own chrome is the only place right-click can be announced.
        widget::secondary_click_chrome_hint(ui);
        ui.add_space(space::SM);

        let columns: [&[(&str, &[ItemSlot])]; 2] = [
            &[("Armor", ARMOR_SLOTS)],
            &[("Accessories", ACCESSORY_SLOTS), ("Weapons", WEAPON_SLOTS)],
        ];
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = space::LG;
            ui.columns(2, |cols| {
                for (col, groups) in cols.iter_mut().zip(columns) {
                    col.spacing_mut().item_spacing.y = space::XXS;
                    for (i, (group_name, slots)) in groups.iter().enumerate() {
                        if i > 0 {
                            col.add_space(space::MD);
                        }
                        section_label(col, group_name);
                        for slot in *slots {
                            let row = render_equipment_row(
                                col, *slot, items, resolved, overrides, item_icons,
                            );
                            if row.clicked() {
                                clicked_slot = Some(*slot);
                            }
                            if row.secondary_clicked() {
                                if let Some(id) = resolved.get(slot) {
                                    open_topic = open_topic.or(Some(Topic::Item(*id)));
                                }
                            }
                        }
                    }
                }
            });
        });
    });

    if restore_clicked {
        if let Some(equip_map) = equipment_overrides_mut(match_config, view_state) {
            restore_default_equipment(equip_map);
        }
    }

    // Open the picker if a socket was clicked; it draws over the whole screen
    // once the sheet is done.
    if let Some(slot) = clicked_slot {
        picker_state.open_slot = Some(slot);
    }

    open_topic
}

/// One equipment socket: a kit [`list_row`] naming the worn item (green when
/// it is the player's pick rather than the class default, dim when the socket
/// is empty), the socket's name at its right-hand end, and the item's tooltip
/// on hover.
fn render_equipment_row(
    ui: &mut egui::Ui,
    slot: ItemSlot,
    items: &ItemDefinitions,
    resolved: &Loadout,
    overrides: &Loadout,
    item_icons: Option<&ItemIcons>,
) -> egui::Response {
    let item_id = resolved.get(&slot);
    let item = item_id.and_then(|id| items.get(id));
    let is_override = is_effective_override(slot, overrides, resolved);

    let (name, name_color) = match (item_id, item) {
        (Some(_), Some(item)) if is_override => (item.name.as_str(), data::ITEM_BONUS),
        (Some(_), Some(item)) => (item.name.as_str(), color::TEXT),
        (Some(_), None) => ("\u{2014} Unknown \u{2014}", color::TEXT_DIM),
        (None, _) => ("\u{2014} Empty \u{2014}", color::TEXT_DIM),
    };
    let texture = item_id.and_then(|id| item_icons.and_then(|icons| icons.textures.get(id)));
    let response = list_row(
        ui,
        ListRow::new(name)
            .name_color(name_color)
            .meta(slot.name())
            .icon(ListIcon {
                texture: texture.copied(),
                edge: color::LINE,
                tier: item.and_then(|item| item.tier()),
            }),
    );
    ui_driver::mark(ui, response.rect, true, format_args!("equip:{slot:?}"));
    // What the row ACTUALLY rendered — the fact the AS-64 repro turned on, and
    // one a screenshot-blind agent cannot otherwise read back. `is_override`
    // is the green colouring.
    match item_id {
        Some(id) => ui_driver::note(
            ui,
            format_args!("equip-row {slot:?} item={id:?} override={is_override}"),
        ),
        None => ui_driver::note(
            ui,
            format_args!("equip-row {slot:?} item=empty override={is_override}"),
        ),
    }

    match (item_id, item) {
        // The item tooltip is deliberately UNCHANGED (AS-65 decision 4): the
        // encyclopedia's item page is what right-click reaches, not a second
        // stat block here.
        (Some(id), Some(item)) => response.on_hover_ui(|ui| {
            ui_driver::note(ui, format_args!("tooltip:{:?}", Topic::Item(*id)));
            render_item_tooltip(ui, item);
        }),
        _ => response,
    }
}

/// What the player did in the equipment picker this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ItemPickerAction {
    /// Left-click: equip this item.
    pub equip: Option<ItemId>,
    /// Right-click: open this item's encyclopedia page instead.
    pub read: Option<ItemId>,
    /// The `×`, Esc or a click outside: close without a change.
    pub close: bool,
}

/// The text of one picker row: `Name  —  stats`, and — for an item carrying a
/// proc or an on-use — the sentence saying what it does, for the row's
/// quieter second line. That line is `ItemConfig::effect_description`, the
/// same sentence the item tooltip and the encyclopedia print, so the picker
/// cannot describe a trinket a third way. Rows without one are exactly one
/// line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemPickerRowText {
    pub name: String,
    pub effect_line: Option<String>,
}

/// The [`ItemPickerRowText`] for `item`.
pub fn item_picker_row_text(item: &ItemConfig) -> ItemPickerRowText {
    let stat_text = format_item_stats(item);
    let name = if stat_text.is_empty() {
        item.name.clone()
    } else {
        format!("{}  \u{2014}  {}", item.name, stat_text)
    };
    ItemPickerRowText {
        name,
        effect_line: item.effect_description(),
    }
}

/// The equipment picker: a kit [`Modal`] titled for `slot`, listing the items
/// that socket accepts. `id` is the modal's egui and `--ui-script` id (its
/// `×` registers as `<id>:close`), the caller's to choose. `valid_items` is
/// what the socket accepts; `current` is what it holds now (drawn selected).
///
/// Left-click equips; right-click reads the item's encyclopedia page and
/// leaves the picker open. The `×`, Esc and a click outside close it.
pub fn draw_item_picker(
    ctx: &egui::Context,
    id: &str,
    slot: ItemSlot,
    valid_items: &[(ItemId, &ItemConfig)],
    current: Option<&ItemId>,
    item_icons: Option<&ItemIcons>,
) -> ItemPickerAction {
    let title = format!("Select: {}", slot.name());
    let max_height = ctx.screen_rect().height() * PICKER_MAX_HEIGHT;
    let response = Modal::new(id, &title).show(ctx, |ui| {
        // The picker is its own layer, so it needs the hint of its own.
        widget::secondary_click_chrome_hint(ui);
        ui.add_space(space::XS);
        draw_item_picker_list(ui, valid_items, current, item_icons, max_height)
    });
    ItemPickerAction {
        close: response.close,
        ..response.inner
    }
}

/// The picker's scrolling list of selectable items, one wrapping kit
/// [`list_row`] each, at most `max_height` tall.
pub fn draw_item_picker_list(
    ui: &mut egui::Ui,
    valid_items: &[(ItemId, &ItemConfig)],
    current: Option<&ItemId>,
    item_icons: Option<&ItemIcons>,
    max_height: f32,
) -> ItemPickerAction {
    let mut action = ItemPickerAction::default();
    // `min_scrolled_height` too: a modal is laid out from the screen's centre,
    // so the space egui offers below it would otherwise clip the list to half
    // the screen even when every item fits.
    egui::ScrollArea::vertical()
        .max_height(max_height)
        .min_scrolled_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::XXS;
            for (item_id, item) in valid_items {
                let is_equipped = current == Some(item_id);
                let text = item_picker_row_text(item);
                let mut row = ListRow::new(&text.name)
                    .name_color(if is_equipped {
                        color::ACCENT
                    } else {
                        color::TEXT
                    })
                    .icon(ListIcon {
                        texture: item_icons.and_then(|icons| icons.textures.get(item_id).copied()),
                        edge: color::LINE,
                        tier: item.tier(),
                    })
                    .selected(is_equipped)
                    .wrap();
                if let Some(effect_line) = &text.effect_line {
                    row = row.subtitle(effect_line);
                }
                let response = list_row(ui, row);
                ui_driver::mark(ui, response.rect, true, format_args!("pick:{item_id:?}"));

                if response.clicked() {
                    action.equip = Some(*item_id);
                }
                // Right-click reads instead of equipping. The picker stays
                // open and this screen keeps its state, so the trip to the
                // item's page costs the player nothing: Back lands them right
                // back on this pick.
                if response.secondary_clicked() {
                    action.read = Some(*item_id);
                }
            }
        });
    action
}

/// The override map rendered lazily, for the UI driver's per-frame note.
///
/// The whole point is that nothing is built unless someone formats it. See the
/// call site in [`render_equipment_panel`] and the `format_args!` rule in
/// `ui::driver::registry`.
struct OverrideMap<'a>(&'a Loadout);

impl std::fmt::Display for OverrideMap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, (slot, id)) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{slot:?}={id:?}")?;
        }
        Ok(())
    }
}

/// Whether a slot row is drawn as overridden. True only when the override is
/// what is actually WORN: the constraint passes (`enforce_two_hand_conflicts`,
/// `enforce_unique_equipped`) run on the resolved loadout, not the override
/// map, so an override they strip — a duplicate ring, an off-hand under a
/// two-hander — is still in the map while the socket resolves empty. Colouring
/// off the resolved item, not the map key, means a row can never render
/// green-as-overridden while showing nothing.
fn is_effective_override(slot: ItemSlot, overrides: &Loadout, resolved: &Loadout) -> bool {
    overrides
        .get(&slot)
        .is_some_and(|chosen| resolved.get(&slot) == Some(chosen))
}

/// Clear every equipment override so the loadout resolves to the RON default
/// exactly as a fresh match would. Whole-set on purpose: the default is
/// validated unique-equipped at load, so restoring all sockets together lands
/// on a known-good state with nothing for the resolver to reconcile — a
/// single-socket reset could collide with a sibling's explicit pick and
/// silently drop it.
fn restore_default_equipment(overrides: &mut Loadout) {
    overrides.clear();
}

/// The viewed combatant's override map, if its slot exists.
fn equipment_overrides_mut<'a>(
    match_config: &'a mut MatchConfig,
    view_state: &ViewCombatantState,
) -> Option<&'a mut Loadout> {
    if view_state.team == 1 {
        match_config.team1_equipment.get_mut(view_state.slot)
    } else {
        match_config.team2_equipment.get_mut(view_state.slot)
    }
}

/// Apply an equipment override for the viewed combatant.
/// Handles 2H/OH conflicts using shared helpers from equipment.rs.
fn set_equipment_override(
    match_config: &mut MatchConfig,
    view_state: &ViewCombatantState,
    slot: ItemSlot,
    id: ItemId,
    items: &ItemDefinitions,
    defaults: &DefaultLoadouts,
    class: CharacterClass,
) {
    if let Some(equip_map) = equipment_overrides_mut(match_config, view_state) {
        // Equipping an off-hand while a 2H is in main-hand → swap MH to 1H first
        if slot == ItemSlot::OffHand {
            let mut resolved = resolve_loadout(class, defaults, equip_map);
            enforce_two_hand_conflicts(&mut resolved, items);
            // Check if *after* enforcement the MH is still 2H (shouldn't be, but check the
            // pre-enforcement state to decide whether to swap)
            let pre_resolved = resolve_loadout(class, defaults, equip_map);
            let mh_is_2h = pre_resolved
                .get(&ItemSlot::MainHand)
                .and_then(|id| items.get(id))
                .is_some_and(|item| item.held() == Some(HeldSlot::TwoHand));
            if mh_is_2h {
                if let Some(replacement) = find_one_handed_mainhand(items, class) {
                    equip_map.insert(ItemSlot::MainHand, replacement);
                } else {
                    return; // No 1H exists — prevent the off-hand equip
                }
            }
        }

        equip_map.insert(slot, id);

        // Equipping a 2H main-hand → clear off-hand override
        // (enforce_two_hand_conflicts handles the default off-hand at resolve time)
        if slot == ItemSlot::MainHand {
            if let Some(new_item) = items.get(&id) {
                if new_item.held() == Some(HeldSlot::TwoHand) {
                    equip_map.remove(&ItemSlot::OffHand);
                }
            }
        }
    }
}

/// One option of a pre-match choice, as its [`ChoiceTile`] shows it.
struct ChoiceOption<'a> {
    label: &'a str,
    texture: Option<egui::TextureId>,
    /// The ability the option picks, when it is one: its tile hovers to that
    /// ability's slim summary and registers as `strategic:<Ability>`.
    ability: Option<AbilityType>,
}

/// One row of [`ChoiceTile`]s, `selected` the chosen one. Returns the index
/// of a tile clicked this frame, unless it was already chosen.
///
/// An option that is an ability hovers to the same slim summary the kit rows
/// show — one builder. Click is the pick and nothing else: the kit list is
/// where each of these abilities links on to its page. `enemy_slot` names a
/// row that is repeated per enemy (the Warlock's curses), so its tiles'
/// `--ui-script` ids stay distinct: `strategic:<Ability>:e<slot>`.
fn choice_row(
    ui: &mut egui::Ui,
    options: &[ChoiceOption],
    selected: usize,
    enemy_slot: Option<usize>,
    data: &EncyclopediaData,
) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        for (i, option) in options.iter().enumerate() {
            let is_selected = i == selected;
            let response = ChoiceTile::new(option.label)
                .texture(option.texture)
                .selected(is_selected)
                .show(ui);
            if response.clicked() && !is_selected {
                clicked = Some(i);
            }
            if let Some(ability) = option.ability {
                match enemy_slot {
                    Some(e) => ui_driver::mark(
                        ui,
                        response.rect,
                        true,
                        format_args!("strategic:{ability:?}:e{e}"),
                    ),
                    None => ui_driver::mark(
                        ui,
                        response.rect,
                        true,
                        format_args!("strategic:{ability:?}"),
                    ),
                }
                response.on_hover_ui(|ui| {
                    ui_driver::note(ui, format_args!("tooltip:{:?}", Topic::Ability(ability)));
                    encyclopedia_abilities::slim_tooltip(ui, ability, data)
                });
            } else {
                // Not an ability (a Hunter's pet): no tooltip, but the tile
                // still registers, so a script or a test can find it.
                ui_driver::mark(
                    ui,
                    response.rect,
                    true,
                    format_args!("choice:{}", option.label),
                );
            }
        }
    });
    clicked
}

/// A pre-match choice: a [`panel`] titled `title`, one row of tiles, and the
/// chosen option's description under them. Returns the index of a newly
/// chosen option.
fn choice_panel(
    ui: &mut egui::Ui,
    title: &str,
    options: &[ChoiceOption],
    selected: usize,
    description: &str,
    data: &EncyclopediaData,
) -> Option<usize> {
    panel(None)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            panel_header(ui, title, color::ACCENT, |_| {});
            let clicked = choice_row(ui, options, selected, None, data);
            ui.add_space(space::SM);
            widget::prose(ui, description, text::BODY, color::TEXT_MUTED);
            clicked
        })
        .inner
}

/// A pre-match choice whose options are abilities: the Rogue's opener, the
/// Warrior's shout, the Mage's armor, the Paladin's aura. `options` pairs
/// each value with its icon. Returns the value newly chosen this frame.
fn strategic_option_panel<T>(
    ui: &mut egui::Ui,
    title: &str,
    options: &[(T, Option<egui::TextureId>)],
    current: T,
    data: &EncyclopediaData,
) -> Option<T>
where
    T: Copy + PartialEq + HasNameDescription,
{
    let tiles: Vec<ChoiceOption> = options
        .iter()
        .map(|(value, texture)| ChoiceOption {
            label: value.name(),
            texture: *texture,
            ability: Some(value.ability()),
        })
        .collect();
    let selected = options
        .iter()
        .position(|(value, _)| *value == current)
        .unwrap_or_default();
    choice_panel(ui, title, &tiles, selected, current.description(), data).map(|i| options[i].0)
}

/// Trait for strategic option enums that have name() and description() methods,
/// and that know WHICH ABILITY they select — the last one is what lets the
/// generic panel hover an option to the encyclopedia's summary of it without
/// matching on its display name.
trait HasNameDescription {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn ability(&self) -> AbilityType;
}

impl HasNameDescription for RogueOpener {
    fn name(&self) -> &str {
        self.name()
    }
    fn description(&self) -> &str {
        self.description()
    }
    fn ability(&self) -> AbilityType {
        self.ability()
    }
}

impl HasNameDescription for RoguePoison {
    fn name(&self) -> &str {
        self.name()
    }
    fn description(&self) -> &str {
        self.description()
    }
    fn ability(&self) -> AbilityType {
        self.ability()
    }
}

impl HasNameDescription for WarriorShout {
    fn name(&self) -> &str {
        self.name()
    }
    fn description(&self) -> &str {
        self.description()
    }
    fn ability(&self) -> AbilityType {
        self.ability()
    }
}

impl HasNameDescription for MageArmor {
    fn name(&self) -> &str {
        self.name()
    }
    fn description(&self) -> &str {
        self.description()
    }
    fn ability(&self) -> AbilityType {
        self.ability()
    }
}

impl HasNameDescription for PaladinAura {
    fn name(&self) -> &str {
        self.name()
    }
    fn description(&self) -> &str {
        self.description()
    }
    fn ability(&self) -> AbilityType {
        self.ability()
    }
}

/// Render the Hunter Pet Type choice.
///
/// DELIBERATELY NOT a reference surface. Alone among the pre-match choices
/// its options are not abilities — Spider, Boar and Bird are pets, and the
/// encyclopedia has no pet topic to link to. The abilities that choice
/// actually buys (Web, Charge, Master's Call) DO get the contract: picking a
/// pet here swaps the pet subsection of the Abilities panel, whose rows are
/// already click-throughs.
fn render_hunter_pet_panel(
    ui: &mut egui::Ui,
    view_state: &ViewCombatantState,
    match_config: &mut MatchConfig,
    pet_icons: Option<&HunterPetIcons>,
    data: &EncyclopediaData,
) {
    let current = slot_value(
        &match_config.team1_hunter_pet_types,
        &match_config.team2_hunter_pet_types,
        view_state,
    );
    let pets = [
        HunterPetType::Spider,
        HunterPetType::Boar,
        HunterPetType::Bird,
    ];
    let tiles: Vec<ChoiceOption> = pets
        .iter()
        .map(|pet| ChoiceOption {
            label: pet.name(),
            texture: pet_icons.and_then(|icons| icons.textures.get(pet).copied()),
            ability: None,
        })
        .collect();
    let selected = pets.iter().position(|p| *p == current).unwrap_or_default();
    let picked = choice_panel(
        ui,
        "Pet Type",
        &tiles,
        selected,
        current.description(),
        data,
    );

    if let Some(i) = picked {
        let pet_types = if view_state.team == 1 {
            &mut match_config.team1_hunter_pet_types
        } else {
            &mut match_config.team2_hunter_pet_types
        };
        while pet_types.len() <= view_state.slot {
            pet_types.push(HunterPetType::default());
        }
        pet_types[view_state.slot] = pets[i];
    }
}

/// Render the Warlock Curse Preferences choice: one row of curse tiles per
/// enemy, under that enemy's class.
///
/// Every curse is an ability, so each tile hovers to that curse's slim
/// summary — the one the kit rows show. Click is the pick; the kit list links
/// each curse to its page.
fn render_warlock_curse_panel(
    ui: &mut egui::Ui,
    view_state: &ViewCombatantState,
    match_config: &mut MatchConfig,
    ability_icons: Option<&AbilityIcons>,
    class_icons: &ClassIcons,
    data: &EncyclopediaData,
) {
    // Clone enemy team composition to avoid borrow conflicts
    let enemy_team: Vec<Option<CharacterClass>> = if view_state.team == 1 {
        match_config.team2.clone()
    } else {
        match_config.team1.clone()
    };
    let current_prefs = if view_state.team == 1 {
        match_config
            .team1_warlock_curse_prefs
            .get(view_state.slot)
            .cloned()
            .unwrap_or_default()
    } else {
        match_config
            .team2_warlock_curse_prefs
            .get(view_state.slot)
            .cloned()
            .unwrap_or_default()
    };

    let curses = [
        (WarlockCurse::Agony, "Curse of Agony", "Agony"),
        (WarlockCurse::Weakness, "Curse of Weakness", "Weakness"),
        (WarlockCurse::Tongues, "Curse of Tongues", "Tongues"),
    ];
    let tiles: Vec<ChoiceOption> = curses
        .iter()
        .map(|(curse, icon_key, label)| ChoiceOption {
            label,
            texture: ability_icons.and_then(|icons| icons.textures.get(*icon_key).copied()),
            ability: Some(curse.ability()),
        })
        .collect();

    let mut changed_curse: Option<(usize, WarlockCurse)> = None;
    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        panel_header(ui, "Curse Preferences", color::ACCENT, |_| {});
        widget::prose(
            ui,
            "Select which curse to apply to each enemy target.",
            text::BODY,
            color::TEXT_MUTED,
        );

        ui.add_space(space::MD);
        for (enemy_slot, enemy_class) in enemy_team.iter().enumerate() {
            if enemy_slot > 0 {
                ui.add_space(space::SM);
            }
            let current = current_prefs.get(enemy_slot).copied().unwrap_or_default();
            let selected = curses
                .iter()
                .position(|(curse, _, _)| *curse == current)
                .unwrap_or_default();
            // One row per enemy: who it curses, one tile wide and level with
            // the tiles' middle, then the curses — so a full enemy team's rows
            // stay above the fold of a 1280x720 window.
            // Top-aligned, so the row is exactly one tile tall; the label
            // centres itself in a tile-tall box of its own.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let label_box = egui::vec2(size::CHOICE_TILE, ChoiceTile::height(ui));
                ui.allocate_ui_with_layout(
                    label_box,
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(label_box);
                        enemy_label(ui, enemy_slot, *enemy_class, class_icons);
                    },
                );
                if let Some(i) = choice_row(ui, &tiles, selected, Some(enemy_slot), data) {
                    changed_curse = Some((enemy_slot, curses[i].0));
                }
            });
        }
    });

    if let Some((enemy_slot, curse)) = changed_curse {
        match_config.set_curse_pref(view_state.team, view_state.slot, enemy_slot, curse);
    }
}

/// The enemy a row of curse tiles is for: its class icon and name, or its
/// slot when the slot is empty.
fn enemy_label(
    ui: &mut egui::Ui,
    enemy_slot: usize,
    enemy_class: Option<CharacterClass>,
    class_icons: &ClassIcons,
) {
    ui.spacing_mut().item_spacing.x = space::SM;
    match enemy_class {
        Some(class) => {
            let _ = class_icon(
                ui,
                class,
                class_icons.textures.get(&class).copied(),
                icon::SM,
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(class.name())
                        .size(text::BODY)
                        .color(class_color(class)),
                )
                .truncate(),
            );
        }
        None => {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("Enemy Target {}", enemy_slot + 1))
                        .size(text::BODY)
                        .color(color::TEXT_MUTED),
                )
                .truncate(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::play_match::equipment::{enforce_unique_equipped, LoadoutsConfig};
    use super::*;

    /// The Warrior's shipped ring defaults: Band of Accuria / Ring of Protection.
    fn warrior_defaults() -> DefaultLoadouts {
        let mut warrior = Loadout::new();
        warrior.insert(ItemSlot::Ring1, ItemId::BandOfAccuria);
        warrior.insert(ItemSlot::Ring2, ItemId::RingOfProtection);
        warrior.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        let mut loadouts = HashMap::new();
        loadouts.insert(CharacterClass::Warrior, warrior);
        DefaultLoadouts::new(LoadoutsConfig { loadouts })
    }

    fn resolve(defaults: &DefaultLoadouts, overrides: &Loadout) -> Loadout {
        // The two-hand pass needs item definitions; the ring cases here never
        // touch it, so this mirrors the resolve site minus that pass.
        let mut resolved = resolve_loadout(CharacterClass::Warrior, defaults, overrides);
        enforce_unique_equipped(&mut resolved);
        resolved
    }

    /// The equipment sheet's rows and the picker's rows draw each item in the
    /// tier frame the encyclopedia draws it in: one item of each tier through
    /// each, so a row that dropped or mis-keyed the tier fails.
    #[test]
    fn equipment_rows_and_the_picker_wear_the_tier_frame() {
        use crate::states::play_match::equipment::{load_item_definitions, ItemTier};
        use crate::ui::theme::tier_color;
        use crate::ui::widgets::tier_frame::painted_frames;

        let items = load_item_definitions().expect("items.ron must load");
        let picks: Vec<(ItemId, &ItemConfig)> = ItemTier::ALL
            .iter()
            .map(|tier| {
                let (id, item) = items
                    .iter()
                    .find(|(_, item)| item.tier() == Some(*tier))
                    .unwrap_or_else(|| panic!("items.ron ships no {tier:?} item"));
                (*id, item)
            })
            .collect();
        let want: Vec<egui::Color32> = ItemTier::ALL.iter().map(|t| tier_color(*t)).collect();

        let picker = painted_frames(|ui| {
            let _ = draw_item_picker_list(ui, &picks, None, None, f32::INFINITY);
        });
        assert_eq!(picker, want, "the picker");

        for ((id, item), colour) in picks.iter().zip(&want) {
            let mut worn = Loadout::new();
            worn.insert(item.sockets()[0], *id);
            let sheet = painted_frames(|ui| {
                let _ = render_equipment_row(
                    ui,
                    item.sockets()[0],
                    &items,
                    &worn,
                    &Loadout::new(),
                    None,
                );
            });
            assert_eq!(sheet, vec![*colour], "the sheet row for {id:?}");
        }
    }

    #[test]
    fn a_stripped_override_is_never_drawn_as_overridden() {
        // The AS-64 repro's end state: Ring2 explicitly holds Band of Accuria
        // while Ring1 resolves to its default, also Band of Accuria. The
        // resolver strips Ring2, so the socket is EMPTY — and the row must
        // say so, not draw green over nothing.
        let defaults = warrior_defaults();
        let mut overrides = Loadout::new();
        overrides.insert(ItemSlot::Ring2, ItemId::BandOfAccuria);

        let resolved = resolve(&defaults, &overrides);
        assert_eq!(
            resolved.get(&ItemSlot::Ring2),
            None,
            "precondition: resolver strips the duplicate"
        );
        assert!(!is_effective_override(
            ItemSlot::Ring2,
            &overrides,
            &resolved
        ));
    }

    #[test]
    fn an_override_that_is_worn_is_drawn_as_overridden() {
        let defaults = warrior_defaults();
        let mut overrides = Loadout::new();
        overrides.insert(ItemSlot::Ring1, ItemId::SignetOfFocus);

        let resolved = resolve(&defaults, &overrides);
        assert!(is_effective_override(
            ItemSlot::Ring1,
            &overrides,
            &resolved
        ));
        assert!(
            !is_effective_override(ItemSlot::Ring2, &overrides, &resolved),
            "a default is not an override"
        );
    }

    #[test]
    fn restore_defaults_lands_on_the_ron_default_exactly() {
        // The repro's first two clicks, then a restore: both rings must come
        // back as the default pair with no row left overridden — and there is
        // no collision to resolve because the default is unique by
        // construction.
        let defaults = warrior_defaults();
        let mut overrides = Loadout::new();
        overrides.insert(ItemSlot::Ring1, ItemId::SignetOfFocus);
        overrides.insert(ItemSlot::Ring2, ItemId::BandOfAccuria);
        overrides.insert(ItemSlot::MainHand, ItemId::FrostbiteBlade);

        restore_default_equipment(&mut overrides);

        assert!(overrides.is_empty());
        let resolved = resolve(&defaults, &overrides);
        assert_eq!(&resolved, defaults.get(CharacterClass::Warrior).unwrap());
        for slot in ItemSlot::all() {
            assert!(!is_effective_override(*slot, &overrides, &resolved));
        }
    }
}
