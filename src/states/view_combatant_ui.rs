//! View Combatant UI - Character Details Screen
//!
//! This module displays detailed information about a combatant:
//! - Base stats (health, resource, attack/spell power, attack/move speed)
//! - List of abilities with icons
//! - Equipment loadout editor (view/change gear per slot)
//!
//! Accessed by clicking a filled character slot in Configure Match.
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
//! rather than inside a borrowed tooltip. The strategic-option panels are
//! radio selects: their icons hover to the same slim summary the kit rows
//! show, and the kit rows are where every one of those abilities links on.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;
use super::{GameState, match_config::{CharacterClass, HunterPetType, MatchConfig, MageArmor, PaladinAura, RogueOpener, WarriorShout, WarlockCurse}};
use super::configure_match_ui::ClassIcons;
use super::play_match::AbilityType;
use super::play_match::ability_config::AbilityDefinitions;
use super::play_match::components::{ClassBaseStats, PetType, ResourceType, class_base_stats};
use super::play_match::equipment::{ItemSlot, ItemId, Loadout, ItemDefinitions, DefaultLoadouts, resolve_loadout, resolve_equipped_loadout, enforce_two_hand_conflicts, find_one_handed_mainhand};
// Item presentation lives in the encyclopedia's Items section — the loadout
// editor renders the same tooltip and stat line so the two never drift.
use super::encyclopedia::items::{format_item_stats, render_item_tooltip};
// This screen is a loadout EDITOR, not a reference work: every fact about an
// ability, item or class it shows is one the encyclopedia already owns, so it
// links there instead of reprinting it. `widget::link_with` carries the shared
// affordance (pointing hand, hover tooltip, "click to open"), and
// `EncyclopediaState::open_at` makes the target page the root of a fresh stack
// so the first Back comes straight back here.
use super::encyclopedia::{abilities as encyclopedia_abilities, widget, EncyclopediaData, EncyclopediaState, Topic};

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
#[derive(Resource, Default)]
pub struct AbilityIcons {
    /// Map of ability name to egui texture ID
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
    /// If a primary weapon is equipped, its attack speed replaces the base.
    /// None means no weapon replacement (use base attack speed).
    weapon_attack_speed: Option<f32>,
}

impl EquipmentBonuses {
    fn from_loadout(loadout: &Loadout, items: &ItemDefinitions, class: CharacterClass) -> Self {
        let mut bonuses = Self::default();
        // Determine which weapon slot is primary (melee classes use MainHand, ranged use Ranged)
        let primary_weapon_slot = if class.is_melee() { ItemSlot::MainHand } else { ItemSlot::Ranged };
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
                // Track weapon attack speed replacement for primary slot
                if *slot == primary_weapon_slot && item.is_weapon && item.attack_speed > 0.0 {
                    bonuses.weapon_attack_speed = Some(item.attack_speed);
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
        return; // Wait for next frame to check if loaded
    }

    // Check if all images are loaded
    let all_loaded = icon_handles.handles.iter().all(|(_, h)| images.contains(h));
    if !all_loaded {
        return; // Wait for images to load
    }

    // Register textures with egui
    for (ability_name, handle) in &icon_handles.handles {
        let texture_id = contexts.add_image(handle.clone());
        ability_icons.textures.insert(ability_name.clone(), texture_id);
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
    info!("Item icons loaded for view combatant screen ({} icons)", item_icons.textures.len());
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
            (HunterPetType::Spider, "icons/abilities/ability_hunter_pet_spider.jpg"),
            (HunterPetType::Boar, "icons/abilities/ability_hunter_pet_boar.jpg"),
            (HunterPetType::Bird, "icons/abilities/ability_hunter_pet_owl.jpg"),
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

/// Main UI system for the View Combatant screen.
pub fn view_combatant_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    view_state: Option<Res<ViewCombatantState>>,
    mut commands: Commands,
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
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
    use crate::keybindings::GameAction;

    // Use try_ctx_mut to avoid panic when context isn't ready
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    // Configure dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    style.interaction.tooltip_delay = 0.0;
    ctx.set_style(style);

    // Handle Back key
    if keybindings.action_just_pressed(GameAction::Back, &keyboard) {
        if view_state.is_some() {
            commands.remove_resource::<ViewCombatantState>();
        }
        next_state.set(GameState::ConfigureMatch);
        return;
    }

    // Get the view state or show error
    let Some(view_state) = view_state else {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::from_rgb(20, 20, 30)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.label(
                        egui::RichText::new("No combatant selected")
                            .size(24.0)
                            .color(egui::Color32::from_rgb(200, 100, 100)),
                    );
                });
            });
        return;
    };

    let class = view_state.class;
    // Base stats and the class kit are both DERIVED: `class_base_stats` is the
    // same table `Combatant::new` builds from, and the ability list comes from
    // the `class` attribution on each `abilities.ron` entry. Neither can drift
    // from the sim or silently drop ability N+1.
    let stats = class_base_stats(class);
    let abilities = ability_definitions.own_abilities_for_class(class);
    // The pet whose abilities belong in THIS combatant's kit. View Combatant is
    // a loadout editor, so it shows the pet that is actually configured (the Pet
    // Type panel below is where that choice is made) rather than every pet the
    // class could bring. The encyclopedia, which documents the whole class, uses
    // `pet_abilities_for_class` instead.
    let active_pet: Option<PetType> = match class {
        CharacterClass::Warlock => Some(PetType::Felhunter),
        CharacterClass::Hunter => {
            let hunter_pet = if view_state.team == 1 {
                match_config.team1_hunter_pet_types.get(view_state.slot).copied().unwrap_or_default()
            } else {
                match_config.team2_hunter_pet_types.get(view_state.slot).copied().unwrap_or_default()
            };
            Some(match hunter_pet {
                HunterPetType::Spider => PetType::Spider,
                HunterPetType::Boar => PetType::Boar,
                HunterPetType::Bird => PetType::Bird,
            })
        }
        _ => None,
    };

    // Compute equipment bonuses for the stats panel
    let equip_overrides = if view_state.team == 1 {
        match_config.team1_equipment.get(view_state.slot).cloned().unwrap_or_default()
    } else {
        match_config.team2_equipment.get(view_state.slot).cloned().unwrap_or_default()
    };
    let resolved_loadout =
        resolve_equipped_loadout(class, &default_loadouts, &equip_overrides, &item_definitions);
    let equip_bonuses = EquipmentBonuses::from_loadout(&resolved_loadout, &item_definitions, class);

    // Everything the shared encyclopedia widgets need to draw a link and its
    // tooltip. This screen already held every one of these resources.
    let encyclopedia_data = EncyclopediaData {
        items: &item_definitions,
        abilities: &ability_definitions,
        item_icons: item_icons.as_deref(),
        class_icons: Some(&*class_icons),
        ability_icons: ability_icons.as_deref(),
    };
    // Set by whichever reference surface was clicked this frame, and applied
    // once after the frame is drawn — so navigation is one decision at the end
    // rather than a state transition fired from inside a draw closure.
    let mut open_topic: Option<Topic> = None;

    // Get class color
    let class_color = class.color();
    let class_color32 = egui::Color32::from_rgb(
        (class_color.to_srgba().red * 255.0) as u8,
        (class_color.to_srgba().green * 255.0) as u8,
        (class_color.to_srgba().blue * 255.0) as u8,
    );

    // Get screen dimensions for responsive layout
    let screen_width = ctx.screen_rect().width();

    // Calculate panel dimensions based on screen size
    // Panels take up ~70% of screen width, split between two columns
    let content_width = (screen_width * 0.7).min(700.0).max(500.0);
    let spacing = 20.0;
    let panel_width = (content_width - spacing) / 2.0;

    // Fixed heights for consistency
    let main_panel_height = 220.0;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.add_space(10.0);

            // Back button - positioned in top-left
            let back_rect =
                egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(80.0, 36.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(back_rect), |ui| {
                if ui
                    .button(egui::RichText::new("BACK").size(20.0))
                    .clicked()
                {
                    commands.remove_resource::<ViewCombatantState>();
                    next_state.set(GameState::ConfigureMatch);
                }
            });

            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {

            // Title - centered
            ui.vertical_centered(|ui| {
                ui.heading(
                    egui::RichText::new("VIEW COMBATANT")
                        .size(42.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );
            });

            ui.add_space(30.0);

            // Class header card - centered, width matches content area
            ui.vertical_centered(|ui| {
                let header_width = content_width.min(500.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(header_width, 90.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(35, 35, 45))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::same(15))
                            .stroke(egui::Stroke::new(2.0, class_color32.gamma_multiply(0.6)))
                            .show(ui, |ui| {
                                ui.set_min_width(header_width - 30.0);

                                // Class icon — the first half of the header's
                                // link to the class's encyclopedia page.
                                let icon_size = 54.0;
                                if let Some(&texture_id) = class_icons.textures.get(&class) {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(icon_size, icon_size),
                                        egui::Sense::click(),
                                    );
                                    ui.painter().image(
                                        texture_id,
                                        rect,
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        egui::Color32::WHITE,
                                    );
                                    ui.painter().rect_stroke(
                                        rect,
                                        6.0,
                                        egui::Stroke::new(
                                            if response.hovered() { 3.0 } else { 2.0 },
                                            class_color32,
                                        ),
                                        egui::StrokeKind::Outside,
                                    );
                                    open_topic = open_topic.or(widget::link(
                                        response,
                                        Topic::Class(class),
                                        &encyclopedia_data,
                                    ));
                                }

                                ui.add_space(20.0);

                                ui.vertical(|ui| {
                                    // ...and the second half. Name and icon are
                                    // one affordance split in two, so either
                                    // one opens the class page.
                                    let name = ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(class.name().to_uppercase())
                                                .size(28.0)
                                                .color(class_color32)
                                                .strong(),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    open_topic = open_topic.or(widget::link(
                                        name,
                                        Topic::Class(class),
                                        &encyclopedia_data,
                                    ));
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(class.description())
                                            .size(16.0)
                                            .color(egui::Color32::from_rgb(153, 153, 153)),
                                    );
                                });
                            });
                    },
                );
            });

            ui.add_space(25.0);

            // Center all content
            ui.vertical_centered(|ui| {
                // Two-column layout for Stats and Abilities
                ui.allocate_ui_with_layout(
                    egui::vec2(content_width, main_panel_height),
                    egui::Layout::left_to_right(egui::Align::TOP),
                    |ui| {
                        // Stats panel
                        ui.allocate_ui_with_layout(
                            egui::vec2(panel_width, main_panel_height),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                render_stats_panel(ui, &stats, &equip_bonuses, panel_width, main_panel_height);
                            },
                        );

                        ui.add_space(spacing);

                        // Abilities panel
                        ui.allocate_ui_with_layout(
                            egui::vec2(panel_width, main_panel_height),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                open_topic = open_topic.or(render_abilities_panel(ui, &abilities, active_pet, panel_width, main_panel_height, &encyclopedia_data));
                            },
                        );
                    },
                );

                // Rogue-specific: Stealth Opener panel
                if class == CharacterClass::Rogue {
                    ui.add_space(15.0);

                    let opener_panel_height = 120.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, opener_panel_height),
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            render_rogue_opener_panel(
                                ui,
                                content_width,
                                opener_panel_height,
                                &view_state,
                                &mut match_config,
                                &ability_icons,
                                &encyclopedia_data,
                            );
                        },
                    );
                }

                // Warrior-specific: Shout Choice panel
                if class == CharacterClass::Warrior {
                    ui.add_space(15.0);

                    let panel_height = 120.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, panel_height),
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            render_strategic_option_panel(
                                ui,
                                content_width,
                                panel_height,
                                "BATTLE SHOUT",
                                &view_state,
                                &ability_icons,
                                &[
                                    ("Battle Shout", WarriorShout::BattleShout),
                                    ("Demoralizing Shout", WarriorShout::DemoralizingShout),
                                    ("Commanding Shout", WarriorShout::CommandingShout),
                                ],
                                |mc, team, slot| {
                                    if team == 1 {
                                        mc.team1_warrior_shouts.get(slot).copied().unwrap_or_default()
                                    } else {
                                        mc.team2_warrior_shouts.get(slot).copied().unwrap_or_default()
                                    }
                                },
                                |mc, team, slot, val| {
                                    let vec = if team == 1 { &mut mc.team1_warrior_shouts } else { &mut mc.team2_warrior_shouts };
                                    if let Some(v) = vec.get_mut(slot) { *v = val; }
                                },
                                &mut match_config,
                                &encyclopedia_data,
                            );
                        },
                    );
                }

                // Mage-specific: Armor Choice panel
                if class == CharacterClass::Mage {
                    ui.add_space(15.0);

                    let panel_height = 120.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, panel_height),
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            render_strategic_option_panel(
                                ui,
                                content_width,
                                panel_height,
                                "MAGE ARMOR",
                                &view_state,
                                &ability_icons,
                                &[
                                    ("Frost Armor", MageArmor::FrostArmor),
                                    ("Mage Armor", MageArmor::MageArmor),
                                    ("Molten Armor", MageArmor::MoltenArmor),
                                ],
                                |mc, team, slot| {
                                    if team == 1 {
                                        mc.team1_mage_armors.get(slot).copied().unwrap_or_default()
                                    } else {
                                        mc.team2_mage_armors.get(slot).copied().unwrap_or_default()
                                    }
                                },
                                |mc, team, slot, val| {
                                    let vec = if team == 1 { &mut mc.team1_mage_armors } else { &mut mc.team2_mage_armors };
                                    if let Some(v) = vec.get_mut(slot) { *v = val; }
                                },
                                &mut match_config,
                                &encyclopedia_data,
                            );
                        },
                    );
                }

                // Paladin-specific: Aura Choice panel
                if class == CharacterClass::Paladin {
                    ui.add_space(15.0);

                    let panel_height = 120.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, panel_height),
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            render_strategic_option_panel(
                                ui,
                                content_width,
                                panel_height,
                                "PALADIN AURA",
                                &view_state,
                                &ability_icons,
                                &[
                                    ("Devotion Aura", PaladinAura::DevotionAura),
                                    ("Shadow Resistance Aura", PaladinAura::ShadowResistanceAura),
                                    ("Concentration Aura", PaladinAura::ConcentrationAura),
                                ],
                                |mc, team, slot| {
                                    if team == 1 {
                                        mc.team1_paladin_auras.get(slot).copied().unwrap_or_default()
                                    } else {
                                        mc.team2_paladin_auras.get(slot).copied().unwrap_or_default()
                                    }
                                },
                                |mc, team, slot, val| {
                                    let vec = if team == 1 { &mut mc.team1_paladin_auras } else { &mut mc.team2_paladin_auras };
                                    if let Some(v) = vec.get_mut(slot) { *v = val; }
                                },
                                &mut match_config,
                                &encyclopedia_data,
                            );
                        },
                    );
                }

                // Hunter-specific: Pet Type panel
                if class == CharacterClass::Hunter {
                    ui.add_space(15.0);

                    let pet_panel_height = 120.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, pet_panel_height),
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            render_hunter_pet_panel(
                                ui,
                                content_width,
                                pet_panel_height,
                                &view_state,
                                &mut match_config,
                                &pet_icons,
                            );
                        },
                    );
                }

                // Warlock-specific: Curse Preferences panel
                if class == CharacterClass::Warlock {
                    ui.add_space(15.0);

                    // Curse panel needs enough height for up to 3 enemy slots stacked vertically
                    let curse_panel_height = 280.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, curse_panel_height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            render_warlock_curse_panel(
                                ui,
                                content_width,
                                curse_panel_height,
                                &view_state,
                                &mut match_config,
                                &ability_icons,
                                &class_icons,
                                &encyclopedia_data,
                            );
                        },
                    );
                }

                ui.add_space(15.0);

                // Equipment panel (full width, replaces Gear + Talents placeholders)
                open_topic = open_topic.or(render_equipment_panel(
                    ui,
                    content_width,
                    &view_state,
                    &mut match_config,
                    &item_definitions,
                    &default_loadouts,
                    &mut picker_state,
                    class,
                    &resolved_loadout,
                    &equip_overrides,
                    &item_icons,
                ));
            });
            }); // ScrollArea
        });

    // A reference surface was clicked: open the encyclopedia ON that page,
    // returning here. `ViewCombatantState` is deliberately LEFT IN PLACE —
    // it is what makes Back come home to this same team and slot, mid-pick
    // picker and all.
    if let Some(topic) = open_topic {
        encyclopedia.open_at(topic, GameState::ViewCombatant);
        next_state.set(GameState::Encyclopedia);
    }
}

/// Render a stat row with integer values and instant tooltip.
fn stat_row_int(
    ui: &mut egui::Ui, label: &str, base: i32, bonus: i32, suffix: &str,
    neutral: egui::Color32, green: egui::Color32, red: egui::Color32, label_color: egui::Color32,
) {
    let effective = base + bonus;
    let color = if bonus > 0 { green } else if bonus < 0 { red } else { neutral };

    ui.label(egui::RichText::new(label).size(14.0).color(label_color));
    let response = ui.label(egui::RichText::new(format!("{}{}", effective, suffix)).size(14.0).color(color));

    if bonus != 0 && response.hovered() {
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with(label), |ui| {
            ui.label(format!("{} + {} from equipment", base, bonus));
        });
    }
    ui.end_row();
}

/// Render a stat row with float values and instant tooltip.
fn stat_row_float(
    ui: &mut egui::Ui, label: &str, base: f32, bonus: f32, suffix: &str,
    neutral: egui::Color32, green: egui::Color32, red: egui::Color32, label_color: egui::Color32,
) {
    let effective = base + bonus;
    let color = if bonus > 0.0 { green } else if bonus < 0.0 { red } else { neutral };

    ui.label(egui::RichText::new(label).size(14.0).color(label_color));
    let response = ui.label(egui::RichText::new(format!("{:.1}{}", effective, suffix)).size(14.0).color(color));

    if bonus != 0.0 && response.hovered() {
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with(label), |ui| {
            ui.label(format!("{:.1} base + {:.1} from equipment", base, bonus));
        });
    }
    ui.end_row();
}

/// Render the Stats panel with effective totals (base + equipment).
/// Stats boosted by equipment are green; negative would be red.
/// Hover tooltip shows the breakdown.
fn render_stats_panel(ui: &mut egui::Ui, stats: &ClassBaseStats, equip: &EquipmentBonuses, width: f32, height: f32) {
    let neutral = egui::Color32::from_rgb(230, 230, 230);
    let green = egui::Color32::from_rgb(100, 255, 100);
    let red = egui::Color32::from_rgb(255, 100, 100);
    let label_color = egui::Color32::from_rgb(170, 170, 170);

    ui.group(|ui| {
        ui.set_min_width(width - 20.0);
        ui.set_min_height(height - 20.0);

        ui.label(
            egui::RichText::new("STATS")
                .size(18.0)
                .color(egui::Color32::from_rgb(230, 204, 153))
                .strong(),
        );

        ui.add_space(12.0);

        egui::Grid::new("stats_grid")
            .num_columns(2)
            .spacing([40.0, 8.0])
            .show(ui, |ui| {
                stat_row_int(ui, "Health:", stats.max_health as i32, equip.health as i32, "", neutral, green, red, label_color);

                // Resource: show mana bonus if applicable
                let mana_bonus = if stats.resource_type == ResourceType::Mana { equip.mana as i32 } else { 0 };
                let resource_effective = stats.max_resource as i32 + mana_bonus;
                let resource_color = if mana_bonus > 0 { green } else if mana_bonus < 0 { red } else { neutral };
                ui.label(egui::RichText::new("Resource:").size(14.0).color(label_color));
                let res_response = ui.label(egui::RichText::new(format!("{} {}", stats.resource_type.name(), resource_effective)).size(14.0).color(resource_color));
                if mana_bonus != 0 && res_response.hovered() {
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("resource_tooltip"), |ui| {
                        ui.label(format!("{} + {} from equipment", stats.max_resource, mana_bonus));
                    });
                }
                ui.end_row();

                stat_row_int(ui, "Attack Power:", stats.attack_power as i32, equip.attack_power as i32, "", neutral, green, red, label_color);
                stat_row_int(ui, "Spell Power:", stats.spell_power as i32, equip.spell_power as i32, "", neutral, green, red, label_color);

                // Crit chance: every class has a non-zero base (Rogue 10% down to
                // Priest 4%), so this row always shows. It used to appear only when
                // equipment granted crit, and then reported the base as 0%.
                let effective_crit = stats.crit_chance + equip.crit_chance;
                ui.label(egui::RichText::new("Crit Chance:").size(14.0).color(label_color));
                let crit_text = format!("{:.1}%", effective_crit * 100.0);
                let crit_color = if equip.crit_chance > 0.0 { green } else if equip.crit_chance < 0.0 { red } else { neutral };
                let crit_response = ui.label(egui::RichText::new(&crit_text).size(14.0).color(crit_color));
                if equip.crit_chance != 0.0 && crit_response.hovered() {
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("crit_tooltip"), |ui| {
                        ui.label(format!("{:.1}% base + {:.1}% from equipment", stats.crit_chance * 100.0, equip.crit_chance * 100.0));
                    });
                }
                ui.end_row();

                // Mana regen (only show if equipment provides it)
                if equip.mana_regen > 0.0 {
                    ui.label(egui::RichText::new("Mana Regen:").size(14.0).color(label_color));
                    let regen_text = format!("+{:.1} MP5", equip.mana_regen);
                    let regen_response = ui.label(egui::RichText::new(&regen_text).size(14.0).color(green));
                    if regen_response.hovered() {
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("regen_tooltip"), |ui| {
                            ui.label(format!("{:.1} from equipment", equip.mana_regen));
                        });
                    }
                    ui.end_row();
                }

                // Attack speed: show weapon replacement if a weapon overrides it
                if let Some(weapon_speed) = equip.weapon_attack_speed {
                    ui.label(egui::RichText::new("Attack Speed:").size(14.0).color(label_color));
                    let speed_text = format!("{:.1}/s", weapon_speed);
                    let speed_color = if (weapon_speed - stats.attack_speed).abs() > 0.01 { green } else { neutral };
                    let speed_response = ui.label(egui::RichText::new(&speed_text).size(14.0).color(speed_color));
                    if (weapon_speed - stats.attack_speed).abs() > 0.01 && speed_response.hovered() {
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("speed_tooltip"), |ui| {
                            ui.label(format!("{:.1} base → {:.1} from weapon", stats.attack_speed, weapon_speed));
                        });
                    }
                    ui.end_row();
                } else {
                    stat_row_float(ui, "Attack Speed:", stats.attack_speed, 0.0, "/s", neutral, green, red, label_color);
                }

                stat_row_float(ui, "Move Speed:", stats.movement_speed, equip.move_speed, "/s", neutral, green, red, label_color);

                // Armor (only show if equipment provides it, since base is 0)
                if equip.armor > 0.0 {
                    let effective_armor = stats.armor + equip.armor;
                    let reduction_pct = effective_armor / (effective_armor + 5500.0) * 100.0;
                    ui.label(egui::RichText::new("Armor:").size(14.0).color(label_color));
                    let armor_text = format!("{:.0}", effective_armor);
                    let armor_response = ui.label(egui::RichText::new(&armor_text).size(14.0).color(green));
                    if armor_response.hovered() {
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("armor_tooltip"), |ui| {
                            ui.label(format!("{:.0} from equipment ({:.1}% physical reduction)", equip.armor, reduction_pct));
                        });
                    }
                    ui.end_row();
                }

                // Spell resistances (only show non-zero values)
                let resistances: &[(&str, f32, &str)] = &[
                    ("Fire Resist:", equip.fire_resistance, "fire_res"),
                    ("Frost Resist:", equip.frost_resistance, "frost_res"),
                    ("Shadow Resist:", equip.shadow_resistance, "shadow_res"),
                    ("Arcane Resist:", equip.arcane_resistance, "arcane_res"),
                    ("Nature Resist:", equip.nature_resistance, "nature_res"),
                    ("Holy Resist:", equip.holy_resistance, "holy_res"),
                ];
                for (label, value, tooltip_id) in resistances {
                    if *value > 0.0 {
                        let reduction_pct = value / (value * 5.0 / 3.0 + 300.0) * 100.0;
                        ui.label(egui::RichText::new(*label).size(14.0).color(label_color));
                        let res_text = format!("{:.0}", value);
                        let res_response = ui.label(egui::RichText::new(&res_text).size(14.0).color(green));
                        if res_response.hovered() {
                            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with(*tooltip_id), |ui| {
                                ui.label(format!("{:.0} from equipment ({:.1}% damage reduction)", value, reduction_pct));
                            });
                        }
                        ui.end_row();
                    }
                }
            });
    });
}

/// Render the Abilities panel.
///
/// Every row is a click-through to the ability's encyclopedia page; the panel
/// returns the topic whose row was clicked this frame.
fn render_abilities_panel(
    ui: &mut egui::Ui,
    abilities: &[AbilityType],
    active_pet: Option<PetType>,
    width: f32,
    height: f32,
    data: &EncyclopediaData,
) -> Option<Topic> {
    ui.group(|ui| {
        let mut clicked = None;

        ui.set_min_width(width - 20.0);
        ui.set_min_height(height - 20.0);

        ui.label(
            egui::RichText::new("ABILITIES")
                .size(18.0)
                .color(egui::Color32::from_rgb(230, 204, 153))
                .strong(),
        );

        ui.add_space(12.0);

        for ability in abilities {
            clicked = clicked.or(render_ability_row(ui, *ability, data));
        }

        // Pet subsection, labeled by the pet that casts them. Before ability
        // attribution existed these five abilities (Spell Lock, Devour Magic,
        // Web, Boar Charge, Master's Call) appeared on no class screen at all.
        if let Some(pet) = active_pet {
            let pet_abilities = data.abilities.abilities_for_pet(pet);
            if !pet_abilities.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!("{} (PET)", pet.name().to_uppercase()))
                        .size(14.0)
                        .color(egui::Color32::from_rgb(190, 170, 220))
                        .strong(),
                );
                ui.add_space(6.0);
                for ability in pet_abilities {
                    clicked = clicked.or(render_ability_row(ui, ability, data));
                }
            }
        }

        clicked
    })
    .inner
}

/// Render one ability row: icon, name, a one-line hover summary, and a click
/// through to the ability's encyclopedia page.
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
    let ability_name = topic.name(data);
    let icon_texture = topic.icon(data);

    // Allocate space for the row first, as a single clickable area
    let row_height = 26.0;
    let available_width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(available_width, row_height),
        egui::Sense::click(),
    );

    // Draw content manually using painter
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(
            rect,
            3.0,
            egui::Color32::from_rgba_premultiplied(255, 255, 255, 15),
        );
    }
    let icon_size = 22.0;
    let icon_rect = egui::Rect::from_min_size(
        rect.min + egui::vec2(0.0, (row_height - icon_size) / 2.0),
        egui::vec2(icon_size, icon_size),
    );

    // Draw icon
    if let Some(texture_id) = icon_texture {
        painter.image(
            texture_id,
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        painter.rect_stroke(
            icon_rect,
            3.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 80, 100)),
            egui::StrokeKind::Outside,
        );
    } else {
        painter.rect_filled(icon_rect, 3.0, egui::Color32::from_rgb(50, 50, 65));
        painter.rect_stroke(
            icon_rect,
            3.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 80, 100)),
            egui::StrokeKind::Outside,
        );
    }

    // Draw ability name
    let text_pos = rect.min + egui::vec2(icon_size + 10.0, (row_height - 14.0) / 2.0);
    painter.text(
        text_pos,
        egui::Align2::LEFT_TOP,
        &ability_name,
        egui::FontId::proportional(14.0),
        egui::Color32::from_rgb(220, 220, 220),
    );

    ui.add_space(4.0);

    // The SLIM tooltip: name, stat strip, one sentence. The long-form version
    // this screen used to render is the encyclopedia's page now, one click away.
    widget::link_with(response, topic, |ui| {
        encyclopedia_abilities::slim_tooltip(ui, ability, data)
    })
}

/// Equipment slot groups for the panel layout
const ARMOR_SLOTS: &[ItemSlot] = &[
    ItemSlot::Head, ItemSlot::Shoulders, ItemSlot::Chest, ItemSlot::Wrists,
    ItemSlot::Hands, ItemSlot::Waist, ItemSlot::Legs, ItemSlot::Feet,
];
const ACCESSORY_SLOTS: &[ItemSlot] = &[
    ItemSlot::Neck, ItemSlot::Back, ItemSlot::Ring1, ItemSlot::Ring2,
    ItemSlot::Trinket1, ItemSlot::Trinket2,
];
const WEAPON_SLOTS: &[ItemSlot] = &[
    ItemSlot::MainHand, ItemSlot::OffHand, ItemSlot::Ranged,
];

/// Render the equipment loadout panel — slot list and picker.
///
/// LEFT-click is the editor: a slot row opens the picker, a picker row equips.
/// RIGHT-click is the reference: it opens that item's encyclopedia page, from
/// the worn row and from the picker alike, so "what is this actually?" never
/// costs you the pick you were making. Returns the topic to open, if any.
fn render_equipment_panel(
    ui: &mut egui::Ui,
    width: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
    items: &Res<ItemDefinitions>,
    defaults: &Res<DefaultLoadouts>,
    picker_state: &mut EquipmentPickerState,
    class: CharacterClass,
    resolved: &Loadout,
    overrides: &Loadout,
    item_icons: &Option<Res<ItemIcons>>,
) -> Option<Topic> {
    let gold = egui::Color32::from_rgb(255, 215, 0);
    let title_color = egui::Color32::from_rgb(230, 204, 153);
    let subtitle_color = egui::Color32::from_rgb(170, 170, 170);
    let muted_color = egui::Color32::from_rgb(90, 90, 90);
    let override_color = egui::Color32::from_rgb(100, 255, 100); // green for overrides

    // Track which slot was clicked to open picker
    let mut clicked_slot: Option<ItemSlot> = None;
    let mut restore_clicked = false;
    // Track the item whose page a right-click asked for.
    let mut open_topic: Option<Topic> = None;

    ui.group(|ui| {
        ui.set_min_width(width - 20.0);

        // The item tooltips stay as they are (AS-65 decision 4), so the panel's
        // own chrome is the only place right-click can be announced.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("EQUIPMENT")
                    .size(18.0)
                    .color(title_color)
                    .strong(),
            );
            ui.add_space(10.0);
            widget::secondary_click_chrome_hint(ui);
        });

        ui.add_space(12.0);

        // Render slot groups
        let slot_groups: &[(&str, &[ItemSlot])] = &[
            ("Armor", ARMOR_SLOTS),
            ("Accessories", ACCESSORY_SLOTS),
            ("Weapons", WEAPON_SLOTS),
        ];

        for (group_name, slots) in slot_groups {
            ui.label(
                egui::RichText::new(*group_name)
                    .size(13.0)
                    .color(subtitle_color)
                    .strong(),
            );
            ui.add_space(2.0);

            for slot in *slots {
                let item_id = resolved.get(slot);
                let is_override = is_effective_override(*slot, overrides, resolved);

                let (item_name, name_color) = if let Some(id) = item_id {
                    if let Some(item) = items.get(id) {
                        let color = if is_override { override_color } else { egui::Color32::from_rgb(220, 220, 220) };
                        (item.name.as_str().to_string(), color)
                    } else {
                        ("— Unknown —".to_string(), muted_color)
                    }
                } else {
                    ("— Empty —".to_string(), muted_color)
                };

                let icon_size = 22.0;
                let row_height = 22.0;
                let total_width = width - 30.0;

                // Allocate a row for icon + text as a single clickable area
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(total_width, row_height),
                    egui::Sense::click(),
                );

                // Highlight on hover
                if response.hovered() {
                    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgba_premultiplied(255, 255, 255, 15));
                }

                let painter = ui.painter();

                // Draw item icon if available
                let mut text_offset_x = 0.0;
                if let Some(id) = item_id {
                    if let Some(icons) = item_icons {
                        if let Some(&texture_id) = icons.textures.get(id) {
                            let icon_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(icon_size, icon_size),
                            );
                            painter.image(texture_id, icon_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                            text_offset_x = icon_size + 4.0;
                        }
                    }
                }

                // Draw text: "Slot: Item Name"
                let label_text = format!("{}: {}", slot.name(), item_name);
                let text_pos = rect.min + egui::vec2(text_offset_x, (row_height - 13.0) / 2.0);
                painter.text(
                    text_pos,
                    egui::Align2::LEFT_TOP,
                    &label_text,
                    egui::FontId::proportional(13.0),
                    name_color,
                );

                if response.clicked() {
                    clicked_slot = Some(*slot);
                }
                if response.secondary_clicked() {
                    if let Some(id) = item_id {
                        open_topic = open_topic.or(Some(Topic::Item(*id)));
                    }
                }

                // Tooltip on hover
                if let Some(id) = item_id {
                    if let Some(item) = items.get(id) {
                        // The item tooltip is deliberately UNCHANGED (AS-65
                        // decision 4): the encyclopedia's item page is what
                        // right-click reaches, not a second stat block here.
                        response.on_hover_ui(|ui| {
                            render_item_tooltip(ui, item);
                        });
                    }
                }
            }

            ui.add_space(6.0);
        }

        // One whole-set restore rather than a per-slot reset. Clearing every
        // override lands on the RON default exactly as a fresh match would,
        // and that default is validated unique-equipped at load — so unlike a
        // single-slot reset, a restore can never collide with a sibling
        // socket's explicit pick and hand the conflict to the resolver.
        let restore = ui
            .add_enabled(
                !overrides.is_empty(),
                egui::Button::new(
                    egui::RichText::new("↩ Restore defaults")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(255, 180, 100)),
                ),
            )
            .on_hover_text("Clear every equipment override for this combatant");
        if restore.clicked() {
            restore_clicked = true;
        }
    });

    if restore_clicked {
        if let Some(equip_map) = equipment_overrides_mut(match_config, view_state) {
            restore_default_equipment(equip_map);
        }
    }

    // Open picker if a slot was clicked
    if let Some(slot) = clicked_slot {
        picker_state.open_slot = Some(slot);
    }

    // Render the picker window if open
    if let Some(open_slot) = picker_state.open_slot {
        let mut keep_open = true;
        let mut selection: Option<ItemId> = None;

        egui::Window::new(format!("Select: {}", open_slot.name()))
            .collapsible(false)
            .resizable(false)
            .min_width(300.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut keep_open)
            .show(ui.ctx(), |ui| {
                // The picker is its own window, so it needs the hint of its own.
                widget::secondary_click_chrome_hint(ui);
                ui.add_space(4.0);

                // List valid items for this socket and class. Anything already
                // worn in the sibling socket is absent — items are
                // unique-equipped, so it is not selectable here.
                let valid_items = items.selectable_items_for_slot(open_slot, class, resolved);
                let current_item = resolved.get(&open_slot);

                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    let picker_icon_size = 22.0;

                    for (item_id, item) in &valid_items {
                        let is_equipped = current_item == Some(item_id);

                        let stat_text = format_item_stats(item);
                        let display = if stat_text.is_empty() {
                            item.name.clone()
                        } else {
                            format!("{}  —  {}", item.name, stat_text)
                        };

                        let name_color = if is_equipped { gold } else { egui::Color32::from_rgb(220, 220, 220) };

                        // Row with icon + text
                        let response = ui.horizontal(|ui| {
                            // Draw item icon if available
                            if let Some(icons) = item_icons {
                                if let Some(&texture_id) = icons.textures.get(item_id) {
                                    let (icon_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(picker_icon_size, picker_icon_size),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().image(texture_id, icon_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                }
                            }

                            ui.selectable_label(is_equipped,
                                egui::RichText::new(&display)
                                    .size(13.0)
                                    .color(name_color),
                            )
                        }).inner;

                        if response.clicked() {
                            selection = Some(*item_id);
                        }
                        // Right-click reads instead of equipping. The picker
                        // stays open and this screen keeps its state, so the
                        // trip to the item's page costs the player nothing:
                        // Back lands them right back on this pick.
                        if response.secondary_clicked() {
                            open_topic = open_topic.or(Some(Topic::Item(*item_id)));
                        }
                    }
                });
            });

        // Handle Escape to close
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            keep_open = false;
        }

        // Apply selection
        if let Some(item_id) = selection {
            set_equipment_override(match_config, view_state, open_slot, item_id, items, defaults, class);
            keep_open = false;
        }

        if !keep_open {
            picker_state.open_slot = None;
        }
    }

    open_topic
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
    match_config: &'a mut ResMut<MatchConfig>,
    view_state: &Res<ViewCombatantState>,
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
    match_config: &mut ResMut<MatchConfig>,
    view_state: &Res<ViewCombatantState>,
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
            let mh_is_2h = pre_resolved.get(&ItemSlot::MainHand)
                .and_then(|id| items.get(id))
                .map_or(false, |item| item.two_handed);
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
                if new_item.two_handed {
                    equip_map.remove(&ItemSlot::OffHand);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::play_match::equipment::{enforce_unique_equipped, LoadoutsConfig};

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
        assert_eq!(resolved.get(&ItemSlot::Ring2), None, "precondition: resolver strips the duplicate");
        assert!(!is_effective_override(ItemSlot::Ring2, &overrides, &resolved));
    }

    #[test]
    fn an_override_that_is_worn_is_drawn_as_overridden() {
        let defaults = warrior_defaults();
        let mut overrides = Loadout::new();
        overrides.insert(ItemSlot::Ring1, ItemId::SignetOfFocus);

        let resolved = resolve(&defaults, &overrides);
        assert!(is_effective_override(ItemSlot::Ring1, &overrides, &resolved));
        assert!(!is_effective_override(ItemSlot::Ring2, &overrides, &resolved), "a default is not an override");
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

/// Render the Rogue Stealth Opener selection panel with ability icons.
///
/// Its two options ARE abilities, so each icon hovers to the same slim summary
/// the kit rows show — one builder. Click is the pick and nothing else: the
/// kit list is where Ambush and Cheap Shot link on to their pages.
fn render_rogue_opener_panel(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
    ability_icons: &Option<Res<AbilityIcons>>,
    data: &EncyclopediaData,
) {
    // Get current opener preference for this combatant
    let current_opener = if view_state.team == 1 {
        match_config.team1_rogue_openers.get(view_state.slot).copied().unwrap_or_default()
    } else {
        match_config.team2_rogue_openers.get(view_state.slot).copied().unwrap_or_default()
    };

    ui.group(|ui| {
        ui.set_min_width(width - 20.0);
        ui.set_min_height(height - 20.0);

        ui.label(
            egui::RichText::new("STEALTH OPENER")
                .size(18.0)
                .color(egui::Color32::from_rgb(230, 204, 153))
                .strong(),
        );

        ui.add_space(12.0);

        // Opener selection with icons
        let icon_size = 48.0;
        let gold = egui::Color32::from_rgb(255, 215, 0);
        let gray = egui::Color32::from_rgb(80, 80, 90);

        // Track which opener was clicked (if any)
        let mut clicked_opener: Option<RogueOpener> = None;

        ui.horizontal(|ui| {
            // Define opener options
            let openers = [
                (RogueOpener::Ambush, "Ambush"),
                (RogueOpener::CheapShot, "Cheap Shot"),
            ];

            for (i, (opener, icon_key)) in openers.iter().enumerate() {
                if i > 0 {
                    ui.add_space(20.0);
                }

                let is_selected = current_opener == *opener;
                let border_color = if is_selected { gold } else { gray };
                let border_width = if is_selected { 3.0 } else { 2.0 };

                ui.vertical(|ui| {
                    // Get icon texture
                    let icon_texture = ability_icons.as_ref().and_then(|icons| {
                        icons.textures.get(*icon_key).copied()
                    });

                    // Allocate space for the icon button
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(icon_size, icon_size),
                        egui::Sense::click(),
                    );

                    // Draw icon or placeholder
                    let painter = ui.painter();
                    if let Some(texture_id) = icon_texture {
                        painter.image(
                            texture_id,
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    } else {
                        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(50, 50, 65));
                    }

                    // Draw border
                    painter.rect_stroke(rect, 4.0, egui::Stroke::new(border_width, border_color), egui::StrokeKind::Outside);

                    // Track click
                    if response.clicked() && !is_selected {
                        clicked_opener = Some(*opener);
                    }

                    // Hover says what the opener does — the same slim summary
                    // the kit rows show, from the same builder.
                    let ability = opener.ability();
                    response.on_hover_ui(|ui| {
                        encyclopedia_abilities::slim_tooltip(ui, ability, data)
                    });

                    // Label below icon
                    ui.add_space(4.0);
                    let label_color = if is_selected {
                        gold
                    } else {
                        egui::Color32::from_rgb(180, 180, 180)
                    };
                    ui.label(
                        egui::RichText::new(opener.name())
                            .size(13.0)
                            .color(label_color),
                    );
                });
            }
        });

        // Apply click outside of the loop to avoid borrow issues
        if let Some(opener) = clicked_opener {
            if view_state.team == 1 {
                if let Some(o) = match_config.team1_rogue_openers.get_mut(view_state.slot) {
                    *o = opener;
                }
            } else {
                if let Some(o) = match_config.team2_rogue_openers.get_mut(view_state.slot) {
                    *o = opener;
                }
            }
        }

        ui.add_space(8.0);

        // Description of current opener
        let description = current_opener.description();
        ui.label(
            egui::RichText::new(description)
                .size(13.0)
                .color(egui::Color32::from_rgb(170, 170, 170))
                .italics(),
        );
    });
}

/// Generic strategic option selection panel for Warrior Shout, Mage Armor, Paladin Aura.
/// Follows the same visual pattern as the Rogue Opener panel.
///
/// Every option is an ability, so every icon hovers to that ability's slim
/// summary — the one the kit rows show. Click is the pick; the kit list is
/// where each of these abilities links on to its page.
fn render_strategic_option_panel<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    title: &str,
    view_state: &Res<ViewCombatantState>,
    ability_icons: &Option<Res<AbilityIcons>>,
    options: &[(&str, T)],  // (icon_key/ability_name, enum value)
    get_current: impl Fn(&MatchConfig, u8, usize) -> T,
    set_value: impl Fn(&mut MatchConfig, u8, usize, T),
    match_config: &mut ResMut<MatchConfig>,
    data: &EncyclopediaData,
) where T: HasNameDescription {
    let current = get_current(match_config, view_state.team, view_state.slot);

    ui.group(|ui| {
        ui.set_min_width(width - 20.0);
        ui.set_min_height(height - 20.0);

        ui.label(
            egui::RichText::new(title)
                .size(18.0)
                .color(egui::Color32::from_rgb(230, 204, 153))
                .strong(),
        );

        ui.add_space(12.0);

        let icon_size = 48.0;
        let gold = egui::Color32::from_rgb(255, 215, 0);
        let gray = egui::Color32::from_rgb(80, 80, 90);

        let mut clicked_index: Option<usize> = None;

        ui.horizontal(|ui| {
            for (i, (icon_key, option)) in options.iter().enumerate() {
                if i > 0 {
                    ui.add_space(20.0);
                }

                let is_selected = current == *option;
                let border_color = if is_selected { gold } else { gray };
                let border_width = if is_selected { 3.0 } else { 2.0 };

                ui.vertical(|ui| {
                    let icon_texture = ability_icons.as_ref().and_then(|icons| {
                        icons.textures.get(*icon_key).copied()
                    });

                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(icon_size, icon_size),
                        egui::Sense::click(),
                    );

                    let painter = ui.painter();
                    if let Some(texture_id) = icon_texture {
                        painter.image(
                            texture_id,
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    } else {
                        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(50, 50, 65));
                    }

                    painter.rect_stroke(rect, 4.0, egui::Stroke::new(border_width, border_color), egui::StrokeKind::Outside);

                    if response.clicked() && !is_selected {
                        clicked_index = Some(i);
                    }

                    let ability = option.ability();
                    response.on_hover_ui(|ui| {
                        encyclopedia_abilities::slim_tooltip(ui, ability, data)
                    });

                    ui.add_space(4.0);
                    let label_color = if is_selected {
                        gold
                    } else {
                        egui::Color32::from_rgb(180, 180, 180)
                    };
                    ui.label(
                        egui::RichText::new(option.name())
                            .size(13.0)
                            .color(label_color),
                    );
                });
            }
        });

        if let Some(idx) = clicked_index {
            set_value(match_config, view_state.team, view_state.slot, options[idx].1);
        }

        ui.add_space(8.0);

        let description = current.description();
        ui.label(
            egui::RichText::new(description)
                .size(13.0)
                .color(egui::Color32::from_rgb(170, 170, 170))
                .italics(),
        );
    });
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

impl HasNameDescription for WarriorShout {
    fn name(&self) -> &str { self.name() }
    fn description(&self) -> &str { self.description() }
    fn ability(&self) -> AbilityType { self.ability() }
}

impl HasNameDescription for MageArmor {
    fn name(&self) -> &str { self.name() }
    fn description(&self) -> &str { self.description() }
    fn ability(&self) -> AbilityType { self.ability() }
}

impl HasNameDescription for PaladinAura {
    fn name(&self) -> &str { self.name() }
    fn description(&self) -> &str { self.description() }
    fn ability(&self) -> AbilityType { self.ability() }
}

/// Render the Hunter Pet Type selection panel.
///
/// DELIBERATELY NOT a reference surface. Alone among the strategic-option
/// panels its options are not abilities — Spider, Boar and Bird are pets, and
/// the encyclopedia has no pet topic to link to. The abilities that choice
/// actually buys (Web, Charge, Master's Call) DO get the contract: picking a
/// pet here swaps the pet subsection of the Abilities panel, whose rows are
/// already click-throughs.
fn render_hunter_pet_panel(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
    pet_icons: &Option<Res<HunterPetIcons>>,
) {
    let current_pet = if view_state.team == 1 {
        match_config.team1_hunter_pet_types.get(view_state.slot).copied().unwrap_or_default()
    } else {
        match_config.team2_hunter_pet_types.get(view_state.slot).copied().unwrap_or_default()
    };

    ui.group(|ui| {
        ui.set_min_width(width - 20.0);
        ui.set_min_height(height - 20.0);

        ui.label(
            egui::RichText::new("PET TYPE")
                .size(18.0)
                .color(egui::Color32::from_rgb(230, 204, 153))
                .strong(),
        );

        ui.add_space(12.0);

        let icon_size = 48.0;
        let gold = egui::Color32::from_rgb(255, 215, 0);
        let gray = egui::Color32::from_rgb(80, 80, 90);

        let mut clicked_pet: Option<HunterPetType> = None;

        ui.horizontal(|ui| {
            let pets = [
                HunterPetType::Spider,
                HunterPetType::Boar,
                HunterPetType::Bird,
            ];

            for (i, pet) in pets.iter().enumerate() {
                if i > 0 {
                    ui.add_space(20.0);
                }

                let is_selected = current_pet == *pet;
                let border_color = if is_selected { gold } else { gray };
                let border_width = if is_selected { 3.0 } else { 2.0 };

                ui.vertical(|ui| {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(icon_size, icon_size),
                        egui::Sense::click(),
                    );

                    let icon_texture = pet_icons
                        .as_ref()
                        .and_then(|icons| icons.textures.get(pet).copied());

                    let painter = ui.painter();
                    if let Some(texture_id) = icon_texture {
                        painter.image(
                            texture_id,
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    } else {
                        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(50, 50, 65));
                    }
                    painter.rect_stroke(rect, 4.0, egui::Stroke::new(border_width, border_color), egui::StrokeKind::Outside);

                    if response.clicked() && !is_selected {
                        clicked_pet = Some(*pet);
                    }

                    ui.add_space(4.0);
                    let label_color = if is_selected {
                        gold
                    } else {
                        egui::Color32::from_rgb(180, 180, 180)
                    };
                    ui.label(
                        egui::RichText::new(pet.name())
                            .size(13.0)
                            .color(label_color),
                    );
                });
            }
        });

        if let Some(pet) = clicked_pet {
            let pet_types = if view_state.team == 1 {
                &mut match_config.team1_hunter_pet_types
            } else {
                &mut match_config.team2_hunter_pet_types
            };
            while pet_types.len() <= view_state.slot {
                pet_types.push(HunterPetType::default());
            }
            pet_types[view_state.slot] = pet;
        }

        ui.add_space(8.0);

        let description = current_pet.description();
        ui.label(
            egui::RichText::new(description)
                .size(13.0)
                .color(egui::Color32::from_rgb(170, 170, 170))
                .italics(),
        );
    });
}

/// Render the Warlock Curse Preferences panel with ability icons.
///
/// Every curse is an ability, so each icon hovers to that curse's slim summary
/// — the one the kit rows show. The hand-written stat lines this panel used to
/// show on hover are gone — they were a third copy of numbers `abilities.ron`
/// already owns. Click is the pick; the kit list links each curse to its page.
fn render_warlock_curse_panel(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
    ability_icons: &Option<Res<AbilityIcons>>,
    class_icons: &Res<ClassIcons>,
    data: &EncyclopediaData,
) {
    // Clone enemy team composition to avoid borrow conflicts
    let enemy_team: Vec<Option<CharacterClass>> = if view_state.team == 1 {
        match_config.team2.clone()
    } else {
        match_config.team1.clone()
    };
    let enemy_size = enemy_team.len();

    // Get current curse preferences for this combatant
    let current_prefs = if view_state.team == 1 {
        match_config.team1_warlock_curse_prefs.get(view_state.slot).cloned().unwrap_or_default()
    } else {
        match_config.team2_warlock_curse_prefs.get(view_state.slot).cloned().unwrap_or_default()
    };

    ui.group(|ui| {
        ui.set_min_width(width - 20.0);
        ui.set_min_height(height - 20.0);

        ui.label(
            egui::RichText::new("CURSE PREFERENCES")
                .size(18.0)
                .color(egui::Color32::from_rgb(230, 204, 153))
                .strong(),
        );

        ui.add_space(8.0);

        ui.label(
            egui::RichText::new("Select which curse to apply to each enemy target:")
                .size(12.0)
                .color(egui::Color32::from_rgb(170, 170, 170)),
        );

        ui.add_space(8.0);

        // Track which curse was changed
        let mut changed_curse: Option<(usize, WarlockCurse)> = None;

        let icon_size = 42.0;
        let gold = egui::Color32::from_rgb(255, 215, 0);
        let gray = egui::Color32::from_rgb(80, 80, 90);

        // One section per enemy slot
        for enemy_slot in 0..enemy_size {
            // Get enemy class for this slot
            let enemy_class = enemy_team.get(enemy_slot).and_then(|c| *c);

            // Enemy target header with class icon and name
            ui.horizontal(|ui| {
                // Small class icon
                let class_icon_size = 20.0;
                if let Some(class) = enemy_class {
                    if let Some(&texture_id) = class_icons.textures.get(&class) {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(class_icon_size, class_icon_size),
                            egui::Sense::hover(),
                        );
                        ui.painter().image(
                            texture_id,
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(class.name())
                            .size(14.0)
                            .color(egui::Color32::from_rgb(200, 180, 140))
                            .strong(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(format!("Enemy Target {}", enemy_slot + 1))
                            .size(14.0)
                            .color(egui::Color32::from_rgb(200, 180, 140))
                            .strong(),
                    );
                }
            });

            ui.add_space(6.0);

            // Get current curse for this enemy
            let current_curse = current_prefs.get(enemy_slot).copied().unwrap_or_default();

            // Curse options displayed horizontally with labels below each icon
            let curses = [
                (WarlockCurse::Agony, "Curse of Agony", "Agony"),
                (WarlockCurse::Weakness, "Curse of Weakness", "Weakness"),
                (WarlockCurse::Tongues, "Curse of Tongues", "Tongues"),
            ];

            ui.horizontal(|ui| {
                for (i, (curse, icon_key, label)) in curses.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(16.0);
                    }

                    let is_selected = current_curse == *curse;
                    let border_color = if is_selected { gold } else { gray };
                    let border_width = if is_selected { 3.0 } else { 1.0 };

                    ui.vertical(|ui| {
                        // Get icon texture
                        let icon_texture = ability_icons.as_ref().and_then(|icons| {
                            icons.textures.get(*icon_key).copied()
                        });

                        // Allocate space for the icon button
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(icon_size, icon_size),
                            egui::Sense::click(),
                        );

                        // Draw icon or placeholder
                        let painter = ui.painter();
                        if let Some(texture_id) = icon_texture {
                            painter.image(
                                texture_id,
                                rect,
                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        } else {
                            painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(50, 50, 65));
                        }

                        // Draw border
                        painter.rect_stroke(rect, 4.0, egui::Stroke::new(border_width, border_color), egui::StrokeKind::Outside);

                        // Track click
                        if response.clicked() && !is_selected {
                            changed_curse = Some((enemy_slot, *curse));
                        }

                        // Hover, from the shared builder. The hand-written stat
                        // strings that used to live here said "-20% physical
                        // damage" next to a config that could change underneath
                        // them.
                        let ability = curse.ability();
                        response.on_hover_ui(|ui| {
                            encyclopedia_abilities::slim_tooltip(ui, ability, data)
                        });

                        // Label below icon
                        ui.add_space(4.0);
                        let label_color = if is_selected {
                            gold
                        } else {
                            egui::Color32::from_rgb(150, 150, 150)
                        };
                        ui.label(
                            egui::RichText::new(*label)
                                .size(11.0)
                                .color(label_color),
                        );
                    });
                }
            });

            // Add separator between targets (but not after the last one)
            if enemy_slot < enemy_size - 1 {
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
            }
        }

        // Apply change outside of the loop to avoid borrow issues
        if let Some((enemy_slot, curse)) = changed_curse {
            match_config.set_curse_pref(view_state.team, view_state.slot, enemy_slot, curse);
        }
    });
}
