//! View Combatant UI - Character Details Screen
//!
//! This module displays detailed information about a combatant:
//! - Base stats (health, resource, attack/spell power, attack/move speed)
//! - List of abilities with icons
//! - Equipment loadout editor (view/change gear per slot)
//!
//! Accessed by clicking a filled character slot in Configure Match.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;
use super::{GameState, match_config::{CharacterClass, HunterPetType, MatchConfig, RogueOpener, WarlockCurse}};
use super::configure_match_ui::ClassIcons;
use super::play_match::AbilityType;
use super::play_match::abilities::{ScalingStat, SpellSchool};
use super::play_match::ability_config::{AbilityDefinitions, AbilityConfig};
use super::play_match::components::AuraType;
use super::play_match::rendering::get_ability_icon_path;
use super::play_match::equipment::{ItemSlot, ItemId, ItemConfig, ItemDefinitions, DefaultLoadouts, resolve_loadout};

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

/// Base stats for a class (used for display)
struct ClassStats {
    health: u32,
    resource_name: &'static str,
    resource_max: u32,
    attack_power: u32,
    spell_power: u32,
    attack_speed: f32,
    move_speed: f32,
}

/// Equipment stat contributions for the stats panel
#[derive(Default)]
struct EquipmentBonuses {
    health: f32,
    mana: f32,
    attack_power: f32,
    spell_power: f32,
    crit_chance: f32,
    move_speed: f32,
}

impl EquipmentBonuses {
    fn from_loadout(loadout: &HashMap<ItemSlot, ItemId>, items: &ItemDefinitions) -> Self {
        let mut bonuses = Self::default();
        for (_, item_id) in loadout {
            if let Some(item) = items.get(item_id) {
                bonuses.health += item.max_health;
                bonuses.mana += item.max_mana;
                bonuses.attack_power += item.attack_power;
                bonuses.spell_power += item.spell_power;
                bonuses.crit_chance += item.crit_chance;
                bonuses.move_speed += item.movement_speed;
            }
        }
        bonuses
    }
}

/// Get the base stats for a class
fn get_class_stats(class: CharacterClass) -> ClassStats {
    match class {
        CharacterClass::Warrior => ClassStats {
            health: 200,
            resource_name: "Rage",
            resource_max: 100,
            attack_power: 30,
            spell_power: 0,
            attack_speed: 1.0,
            move_speed: 5.0,
        },
        CharacterClass::Mage => ClassStats {
            health: 150,
            resource_name: "Mana",
            resource_max: 200,
            attack_power: 0,
            spell_power: 50,
            attack_speed: 0.7,
            move_speed: 4.5,
        },
        CharacterClass::Rogue => ClassStats {
            health: 175,
            resource_name: "Energy",
            resource_max: 100,
            attack_power: 35,
            spell_power: 0,
            attack_speed: 1.3,
            move_speed: 6.0,
        },
        CharacterClass::Priest => ClassStats {
            health: 150,
            resource_name: "Mana",
            resource_max: 150,
            attack_power: 0,
            spell_power: 40,
            attack_speed: 0.8,
            move_speed: 5.0,
        },
        CharacterClass::Warlock => ClassStats {
            health: 160,
            resource_name: "Mana",
            resource_max: 180,
            attack_power: 0,
            spell_power: 45,
            attack_speed: 0.7,
            move_speed: 4.5,
        },
        CharacterClass::Paladin => ClassStats {
            health: 175,
            resource_name: "Mana",
            resource_max: 160,
            attack_power: 20,
            spell_power: 35,
            attack_speed: 0.9,
            move_speed: 5.0,
        },
        CharacterClass::Hunter => ClassStats {
            health: 165,
            resource_name: "Mana",
            resource_max: 150,
            attack_power: 30,
            spell_power: 0,
            attack_speed: 0.4,
            move_speed: 5.0,
        },
    }
}

/// Get the list of abilities for a class
fn get_class_abilities(class: CharacterClass) -> Vec<AbilityType> {
    match class {
        CharacterClass::Warrior => vec![
            AbilityType::BattleShout,
            AbilityType::Charge,
            AbilityType::Rend,
            AbilityType::MortalStrike,
            AbilityType::Pummel,
            AbilityType::HeroicStrike,
        ],
        CharacterClass::Mage => vec![
            AbilityType::Frostbolt,
            AbilityType::FrostNova,
            AbilityType::ArcaneIntellect,
            AbilityType::IceBarrier,
            AbilityType::Polymorph,
        ],
        CharacterClass::Rogue => vec![
            AbilityType::Ambush,
            AbilityType::CheapShot,
            AbilityType::SinisterStrike,
            AbilityType::KidneyShot,
            AbilityType::Kick,
        ],
        CharacterClass::Priest => vec![
            AbilityType::FlashHeal,
            AbilityType::MindBlast,
            AbilityType::PowerWordFortitude,
            AbilityType::PowerWordShield,
            AbilityType::DispelMagic,
        ],
        CharacterClass::Warlock => vec![
            AbilityType::Corruption,
            AbilityType::Shadowbolt,
            AbilityType::Fear,
            AbilityType::Immolate,
            AbilityType::DrainLife,
        ],
        CharacterClass::Paladin => vec![
            AbilityType::DevotionAura,
            AbilityType::DivineShield,
            AbilityType::FlashOfLight,
            AbilityType::HolyLight,
            AbilityType::HolyShock,
            AbilityType::HammerOfJustice,
            AbilityType::PaladinCleanse,
        ],
        CharacterClass::Hunter => vec![
            AbilityType::AimedShot,
            AbilityType::ArcaneShot,
            AbilityType::ConcussiveShot,
            AbilityType::Disengage,
            AbilityType::FreezingTrap,
            AbilityType::FrostTrap,
        ],
    }
}

/// Get the display name for an ability
fn get_ability_name(ability: AbilityType) -> &'static str {
    match ability {
        AbilityType::Frostbolt => "Frostbolt",
        AbilityType::FlashHeal => "Flash Heal",
        AbilityType::HeroicStrike => "Heroic Strike",
        AbilityType::Ambush => "Ambush",
        AbilityType::CheapShot => "Cheap Shot",
        AbilityType::FrostNova => "Frost Nova",
        AbilityType::MindBlast => "Mind Blast",
        AbilityType::SinisterStrike => "Sinister Strike",
        AbilityType::Charge => "Charge",
        AbilityType::KidneyShot => "Kidney Shot",
        AbilityType::PowerWordFortitude => "Power Word: Fortitude",
        AbilityType::Rend => "Rend",
        AbilityType::MortalStrike => "Mortal Strike",
        AbilityType::Pummel => "Pummel",
        AbilityType::Kick => "Kick",
        AbilityType::Corruption => "Corruption",
        AbilityType::Shadowbolt => "Shadow Bolt",
        AbilityType::Fear => "Fear",
        AbilityType::Immolate => "Immolate",
        AbilityType::DrainLife => "Drain Life",
        AbilityType::ArcaneIntellect => "Arcane Intellect",
        AbilityType::BattleShout => "Battle Shout",
        AbilityType::IceBarrier => "Ice Barrier",
        AbilityType::PowerWordShield => "Power Word: Shield",
        AbilityType::Polymorph => "Polymorph",
        AbilityType::DispelMagic => "Dispel Magic",
        AbilityType::CurseOfAgony => "Curse of Agony",
        AbilityType::CurseOfWeakness => "Curse of Weakness",
        AbilityType::CurseOfTongues => "Curse of Tongues",
        // Paladin abilities
        AbilityType::FlashOfLight => "Flash of Light",
        AbilityType::HolyLight => "Holy Light",
        AbilityType::HolyShock => "Holy Shock",
        AbilityType::HammerOfJustice => "Hammer of Justice",
        AbilityType::PaladinCleanse => "Cleanse",
        AbilityType::DevotionAura => "Devotion Aura",
        AbilityType::DivineShield => "Divine Shield",
        // Pet abilities (Felhunter)
        AbilityType::SpellLock => "Spell Lock",
        AbilityType::DevourMagic => "Devour Magic",
        // Hunter abilities
        AbilityType::AimedShot => "Aimed Shot",
        AbilityType::ArcaneShot => "Arcane Shot",
        AbilityType::ConcussiveShot => "Concussive Shot",
        AbilityType::Disengage => "Disengage",
        AbilityType::FreezingTrap => "Freezing Trap",
        AbilityType::FrostTrap => "Frost Trap",
        // Hunter pet abilities
        AbilityType::SpiderWeb => "Web",
        AbilityType::BoarCharge => "Boar Charge",
        AbilityType::MastersCall => "Master's Call",
    }
}

/// System to load ability icons for the view combatant screen.
pub fn load_ability_icons(
    mut contexts: EguiContexts,
    asset_server: Res<AssetServer>,
    mut ability_icons: ResMut<AbilityIcons>,
    mut icon_handles: ResMut<AbilityIconHandles>,
    images: Res<Assets<Image>>,
) {
    // Only load once
    if ability_icons.loaded {
        return;
    }

    // Dynamically collect all ability names from all classes
    let all_classes = [
        CharacterClass::Warrior,
        CharacterClass::Mage,
        CharacterClass::Rogue,
        CharacterClass::Priest,
        CharacterClass::Warlock,
        CharacterClass::Paladin,
        CharacterClass::Hunter,
    ];
    let mut ability_names: Vec<&'static str> = Vec::new();
    for class in &all_classes {
        for ability in get_class_abilities(*class) {
            let name = get_ability_name(ability);
            if !ability_names.contains(&name) {
                ability_names.push(name);
            }
        }
    }
    // Add curse variants (not in class abilities list but used in UI)
    for extra in ["Curse of Agony", "Curse of Weakness", "Curse of Tongues"] {
        if !ability_names.contains(&extra) {
            ability_names.push(extra);
        }
    }

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        for ability in &ability_names {
            if let Some(path) = get_ability_icon_path(ability) {
                let handle: Handle<Image> = asset_server.load(path);
                icon_handles.handles.push((ability.to_string(), handle));
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
    ability_definitions: Res<AbilityDefinitions>,
    mut match_config: ResMut<MatchConfig>,
    item_definitions: Res<ItemDefinitions>,
    default_loadouts: Res<DefaultLoadouts>,
    mut picker_state: Local<EquipmentPickerState>,
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
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(20, 20, 30)))
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
    let stats = get_class_stats(class);
    let abilities = get_class_abilities(class);

    // Compute equipment bonuses for the stats panel
    let equip_overrides = if view_state.team == 1 {
        match_config.team1_equipment.get(view_state.slot).cloned().unwrap_or_default()
    } else {
        match_config.team2_equipment.get(view_state.slot).cloned().unwrap_or_default()
    };
    let resolved_loadout = resolve_loadout(class, &default_loadouts, &equip_overrides);
    let equip_bonuses = EquipmentBonuses::from_loadout(&resolved_loadout, &item_definitions);

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
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin::same(20.0)),
        )
        .show(ctx, |ui| {
            ui.add_space(10.0);

            // Back button - positioned in top-left
            let back_rect =
                egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(80.0, 36.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(back_rect), |ui| {
                if ui
                    .button(egui::RichText::new("<- BACK").size(20.0))
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
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(35, 35, 45))
                            .rounding(8.0)
                            .inner_margin(egui::Margin::same(15.0))
                            .stroke(egui::Stroke::new(2.0, class_color32.gamma_multiply(0.6)))
                            .show(ui, |ui| {
                                ui.set_min_width(header_width - 30.0);

                                // Class icon
                                let icon_size = 54.0;
                                if let Some(&texture_id) = class_icons.textures.get(&class) {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(icon_size, icon_size),
                                        egui::Sense::hover(),
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
                                        egui::Stroke::new(2.0, class_color32),
                                    );
                                }

                                ui.add_space(20.0);

                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(class.name().to_uppercase())
                                            .size(28.0)
                                            .color(class_color32)
                                            .strong(),
                                    );
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
                                render_abilities_panel(ui, &abilities, panel_width, main_panel_height, &ability_icons, &ability_definitions, &stats);
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
                            );
                        },
                    );
                }

                ui.add_space(15.0);

                // Equipment panel (full width, replaces Gear + Talents placeholders)
                render_equipment_panel(
                    ui,
                    content_width,
                    &view_state,
                    &mut match_config,
                    &item_definitions,
                    &default_loadouts,
                    &mut picker_state,
                    class,
                );
            });
            }); // ScrollArea
        });
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
fn render_stats_panel(ui: &mut egui::Ui, stats: &ClassStats, equip: &EquipmentBonuses, width: f32, height: f32) {
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
                stat_row_int(ui, "Health:", stats.health as i32, equip.health as i32, "", neutral, green, red, label_color);

                // Resource: show mana bonus if applicable
                let mana_bonus = if stats.resource_name == "Mana" { equip.mana as i32 } else { 0 };
                let resource_effective = stats.resource_max as i32 + mana_bonus;
                let resource_color = if mana_bonus > 0 { green } else if mana_bonus < 0 { red } else { neutral };
                ui.label(egui::RichText::new("Resource:").size(14.0).color(label_color));
                let res_response = ui.label(egui::RichText::new(format!("{} {}", stats.resource_name, resource_effective)).size(14.0).color(resource_color));
                if mana_bonus != 0 && res_response.hovered() {
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("resource_tooltip"), |ui| {
                        ui.label(format!("{} + {} from equipment", stats.resource_max, mana_bonus));
                    });
                }
                ui.end_row();

                stat_row_int(ui, "Attack Power:", stats.attack_power as i32, equip.attack_power as i32, "", neutral, green, red, label_color);
                stat_row_int(ui, "Spell Power:", stats.spell_power as i32, equip.spell_power as i32, "", neutral, green, red, label_color);

                // Crit chance (only show if equipment provides it)
                if equip.crit_chance > 0.0 {
                    ui.label(egui::RichText::new("Crit Chance:").size(14.0).color(label_color));
                    let crit_text = format!("{:.1}%", equip.crit_chance * 100.0);
                    let crit_response = ui.label(egui::RichText::new(&crit_text).size(14.0).color(green));
                    if crit_response.hovered() {
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("crit_tooltip"), |ui| {
                            ui.label(format!("0% base + {:.1}% from equipment", equip.crit_chance * 100.0));
                        });
                    }
                    ui.end_row();
                }

                stat_row_float(ui, "Attack Speed:", stats.attack_speed, 0.0, "/s", neutral, green, red, label_color);
                stat_row_float(ui, "Move Speed:", stats.move_speed, equip.move_speed, "/s", neutral, green, red, label_color);
            });
    });
}

/// Render the Abilities panel
fn render_abilities_panel(
    ui: &mut egui::Ui,
    abilities: &[AbilityType],
    width: f32,
    height: f32,
    ability_icons: &Option<Res<AbilityIcons>>,
    ability_definitions: &AbilityDefinitions,
    stats: &ClassStats,
) {
    ui.group(|ui| {
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
            let ability_name = get_ability_name(*ability);
            let ability_config = ability_definitions.get(ability);

            // Get icon texture if available
            let icon_texture = ability_icons.as_ref().and_then(|icons| {
                icons.textures.get(ability_name).copied()
            });

            // Allocate space for the row first, with hover sense
            let row_height = 26.0;
            let available_width = ui.available_width();
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(available_width, row_height),
                egui::Sense::hover(),
            );

            // Draw content manually using painter
            let painter = ui.painter();
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
                );
            } else {
                painter.rect_filled(icon_rect, 3.0, egui::Color32::from_rgb(50, 50, 65));
                painter.rect_stroke(
                    icon_rect,
                    3.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 80, 100)),
                );
            }

            // Draw ability name
            let text_pos = rect.min + egui::vec2(icon_size + 10.0, (row_height - 14.0) / 2.0);
            painter.text(
                text_pos,
                egui::Align2::LEFT_TOP,
                ability_name,
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgb(220, 220, 220),
            );

            // Attach tooltip using show_tooltip_at_pointer when hovered
            if let Some(config) = ability_config {
                if response.hovered() {
                    egui::show_tooltip_at_pointer(
                        ui.ctx(),
                        ui.layer_id(),
                        ui.id().with(ability_name),
                        |ui| {
                            render_ability_tooltip(ui, ability_name, config, stats);
                        },
                    );
                }
            }

            ui.add_space(4.0);
        }
    });
}

/// Get the color for a spell school
fn get_spell_school_color(school: SpellSchool) -> egui::Color32 {
    match school {
        SpellSchool::Physical => egui::Color32::from_rgb(199, 156, 110), // Brown/tan
        SpellSchool::Frost => egui::Color32::from_rgb(100, 180, 255),    // Ice blue
        SpellSchool::Fire => egui::Color32::from_rgb(255, 128, 64),      // Orange-red
        SpellSchool::Shadow => egui::Color32::from_rgb(148, 130, 201),   // Purple
        SpellSchool::Arcane => egui::Color32::from_rgb(255, 128, 255),   // Pink/magenta
        SpellSchool::Holy => egui::Color32::from_rgb(255, 230, 150),     // Golden yellow
        SpellSchool::Nature => egui::Color32::from_rgb(76, 196, 30),      // Green
        SpellSchool::None => egui::Color32::from_rgb(220, 220, 220),     // Gray
    }
}

/// Render a WoW-style ability tooltip
fn render_ability_tooltip(ui: &mut egui::Ui, name: &str, config: &AbilityConfig, stats: &ClassStats) {
    ui.set_min_width(250.0);
    ui.set_max_width(300.0);

    // Ability name (colored by spell school)
    let name_color = get_spell_school_color(config.spell_school);
    ui.label(
        egui::RichText::new(name)
            .size(16.0)
            .color(name_color)
            .strong(),
    );

    ui.add_space(4.0);

    // Resource cost and range on same line
    ui.horizontal(|ui| {
        // Mana/Energy/Rage cost
        if config.mana_cost > 0.0 {
            ui.label(
                egui::RichText::new(format!("{:.0} Resource", config.mana_cost))
                    .size(12.0)
                    .color(egui::Color32::from_rgb(180, 180, 255)),
            );
        }

        // Range
        if config.range > 0.0 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0} yd range", config.range))
                        .size(12.0)
                        .color(egui::Color32::from_rgb(180, 180, 180)),
                );
            });
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new("Self")
                        .size(12.0)
                        .color(egui::Color32::from_rgb(180, 180, 180)),
                );
            });
        }
    });

    // Cast time and cooldown on same line
    ui.horizontal(|ui| {
        // Cast time
        let cast_text = if config.cast_time > 0.0 {
            format!("{:.1} sec cast", config.cast_time)
        } else if config.channel_duration.is_some() {
            format!("{:.0} sec channel", config.channel_duration.unwrap())
        } else {
            "Instant".to_string()
        };
        ui.label(
            egui::RichText::new(cast_text)
                .size(12.0)
                .color(egui::Color32::from_rgb(180, 180, 180)),
        );

        // Cooldown
        if config.cooldown > 0.0 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0} sec cooldown", config.cooldown))
                        .size(12.0)
                        .color(egui::Color32::from_rgb(180, 180, 180)),
                );
            });
        }
    });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // Description - build dynamically based on ability effects and stats
    let description = build_ability_description(config, stats);
    ui.label(
        egui::RichText::new(description)
            .size(12.0)
            .color(egui::Color32::from_rgb(255, 209, 0)), // Yellow like WoW tooltips
    );

    // Special effects (aura application)
    if let Some(ref aura) = config.applies_aura {
        ui.add_space(4.0);
        let aura_desc = build_aura_description(aura);
        ui.label(
            egui::RichText::new(aura_desc)
                .size(12.0)
                .color(egui::Color32::from_rgb(255, 209, 0)),
        );
    }
}

/// Build a description string for an ability based on its config and combatant stats
fn build_ability_description(config: &AbilityConfig, stats: &ClassStats) -> String {
    let mut parts = Vec::new();

    // Calculate stat contribution for damage
    let damage_stat_value = match config.damage_scales_with {
        ScalingStat::AttackPower => stats.attack_power as f32,
        ScalingStat::SpellPower => stats.spell_power as f32,
        ScalingStat::None => 0.0,
    };
    let damage_bonus = damage_stat_value * config.damage_coefficient;

    // Calculate stat contribution for healing (uses spell power)
    let healing_bonus = stats.spell_power as f32 * config.healing_coefficient;

    // Damage
    if config.damage_base_max > 0.0 {
        let min_damage = config.damage_base_min + damage_bonus;
        let max_damage = config.damage_base_max + damage_bonus;
        if config.channel_duration.is_some() {
            // Channeled damage - show per tick
            parts.push(format!("Deals {:.0}-{:.0} damage per tick.", min_damage, max_damage));
        } else {
            parts.push(format!("Deals {:.0}-{:.0} damage.", min_damage, max_damage));
        }
    }

    // Healing
    if config.healing_base_max > 0.0 {
        let min_heal = config.healing_base_min + healing_bonus;
        let max_heal = config.healing_base_max + healing_bonus;
        parts.push(format!("Heals for {:.0}-{:.0}.", min_heal, max_heal));
    }

    // Channel healing (Drain Life style)
    if config.channel_healing_per_tick > 0.0 {
        parts.push(format!("Restores {:.0} health to the caster per tick.", config.channel_healing_per_tick));
    }

    // Interrupt
    if config.is_interrupt {
        if config.lockout_duration > 0.0 {
            parts.push(format!("Interrupts spellcasting and locks out the school for {:.1} sec.", config.lockout_duration));
        } else {
            parts.push("Interrupts spellcasting.".to_string());
        }
    }

    // Charge
    if config.is_charge {
        parts.push("Charges to the target.".to_string());
    }

    // Stealth requirement
    if config.requires_stealth {
        parts.push("Must be stealthed.".to_string());
    }

    // Dispel
    if config.is_dispel {
        parts.push("Removes one magic debuff from an ally.".to_string());
    }

    if parts.is_empty() {
        "Utility ability.".to_string()
    } else {
        parts.join(" ")
    }
}

/// Build a description string for an aura effect
fn build_aura_description(aura: &super::play_match::ability_config::AuraEffect) -> String {
    match aura.aura_type {
        AuraType::MovementSpeedSlow => {
            let slow_pct = ((1.0 - aura.magnitude) * 100.0) as i32;
            format!("Slows movement speed by {}% for {:.0} sec.", slow_pct, aura.duration)
        }
        AuraType::Root => {
            if aura.break_on_damage > 0.0 {
                format!("Roots the target for {:.0} sec. Breaks after {:.0} damage.", aura.duration, aura.break_on_damage)
            } else {
                format!("Roots the target for {:.0} sec.", aura.duration)
            }
        }
        AuraType::Stun => {
            format!("Stuns the target for {:.0} sec.", aura.duration)
        }
        AuraType::Fear => {
            format!("Causes the target to flee in fear for {:.0} sec. Breaks on damage.", aura.duration)
        }
        AuraType::Polymorph => {
            format!("Transforms the target into a sheep for {:.0} sec. Breaks on any damage.", aura.duration)
        }
        AuraType::DamageOverTime => {
            let total_ticks = (aura.duration / aura.tick_interval).ceil() as i32;
            let total_damage = aura.magnitude * total_ticks as f32;
            format!("Deals {:.0} damage over {:.0} sec.", total_damage, aura.duration)
        }
        AuraType::HealingReduction => {
            let reduction_pct = ((1.0 - aura.magnitude) * 100.0) as i32;
            format!("Reduces healing received by {}% for {:.0} sec.", reduction_pct, aura.duration)
        }
        AuraType::Absorb => {
            format!("Absorbs {:.0} damage for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::MaxHealthIncrease => {
            format!("Increases maximum health by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::MaxManaIncrease => {
            format!("Increases maximum mana by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::AttackPowerIncrease => {
            format!("Increases attack power by {:.0} for {:.0} sec.", aura.magnitude, aura.duration)
        }
        AuraType::SpellSchoolLockout => {
            format!("Locks out a spell school for {:.0} sec.", aura.duration)
        }
        AuraType::ShadowSight => {
            format!("Reveals stealthed enemies for {:.0} sec.", aura.duration)
        }
        AuraType::WeakenedSoul => {
            format!("Cannot receive Power Word: Shield for {:.0} sec.", aura.duration)
        }
        AuraType::DamageReduction => {
            let reduction_pct = (aura.magnitude * 100.0) as i32;
            format!("Reduces physical damage dealt by {}% for {:.0} sec.", reduction_pct, aura.duration)
        }
        AuraType::CastTimeIncrease => {
            let increase_pct = (aura.magnitude * 100.0) as i32;
            format!("Increases cast time by {}% for {:.0} sec.", increase_pct, aura.duration)
        }
        AuraType::DamageTakenReduction => {
            let reduction_pct = (aura.magnitude * 100.0) as i32;
            format!("Reduces damage taken by {}% for {:.0} sec.", reduction_pct, aura.duration)
        }
        AuraType::DamageImmunity => {
            format!("Immune to all damage for {:.0} sec. Reduces damage dealt by 50%.", aura.duration)
        }
        AuraType::Incapacitate => {
            if aura.break_on_damage > 0.0 {
                format!("Incapacitates the target for {:.0} sec. Breaks on any damage.", aura.duration)
            } else {
                format!("Incapacitates the target for {:.0} sec.", aura.duration)
            }
        }
    }
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

/// Render the equipment loadout panel — slot list, picker, and stat totals.
fn render_equipment_panel(
    ui: &mut egui::Ui,
    width: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
    items: &Res<ItemDefinitions>,
    defaults: &Res<DefaultLoadouts>,
    picker_state: &mut EquipmentPickerState,
    class: CharacterClass,
) {
    let gold = egui::Color32::from_rgb(255, 215, 0);
    let title_color = egui::Color32::from_rgb(230, 204, 153);
    let subtitle_color = egui::Color32::from_rgb(170, 170, 170);
    let muted_color = egui::Color32::from_rgb(90, 90, 90);
    let override_color = egui::Color32::from_rgb(100, 255, 100); // green for overrides

    // Get current overrides for this combatant
    let overrides = if view_state.team == 1 {
        match_config.team1_equipment.get(view_state.slot).cloned().unwrap_or_default()
    } else {
        match_config.team2_equipment.get(view_state.slot).cloned().unwrap_or_default()
    };

    // Resolve the full loadout (defaults + overrides)
    let resolved = resolve_loadout(class, defaults, &overrides);

    // Track which slot was clicked to open picker
    let mut clicked_slot: Option<ItemSlot> = None;

    ui.group(|ui| {
        ui.set_min_width(width - 20.0);

        ui.label(
            egui::RichText::new("EQUIPMENT")
                .size(18.0)
                .color(title_color)
                .strong(),
        );

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
                let is_override = overrides.contains_key(slot);

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

                // Compact row: "Slot: Item Name" as a selectable label for clear hover/click
                let label_text = format!("{}: {}", slot.name(), item_name);
                let response = ui.selectable_label(false,
                    egui::RichText::new(&label_text)
                        .size(13.0)
                        .color(name_color),
                );

                if response.clicked() {
                    clicked_slot = Some(*slot);
                }

                // Tooltip on hover (R8 — nice-to-have)
                if let Some(id) = item_id {
                    if let Some(item) = items.get(id) {
                        response.on_hover_ui(|ui| {
                            render_item_tooltip(ui, item);
                        });
                    }
                }
            }

            ui.add_space(6.0);
        }
    });

    // Open picker if a slot was clicked
    if let Some(slot) = clicked_slot {
        picker_state.open_slot = Some(slot);
    }

    // Render the picker window if open
    if let Some(open_slot) = picker_state.open_slot {
        let mut keep_open = true;
        let mut selection: Option<PickerAction> = None;

        egui::Window::new(format!("Select: {}", open_slot.name()))
            .collapsible(false)
            .resizable(false)
            .min_width(300.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut keep_open)
            .show(ui.ctx(), |ui| {
                // "Reset to Default" option — only when slot has override
                if overrides.contains_key(&open_slot) {
                    let reset_response = ui.selectable_label(false,
                        egui::RichText::new("↩ Reset to Default")
                            .size(14.0)
                            .color(egui::Color32::from_rgb(255, 180, 100)),
                    );
                    if reset_response.clicked() {
                        selection = Some(PickerAction::ResetToDefault(open_slot));
                    }
                    ui.separator();
                }

                // List valid items for this slot and class
                let valid_items = items.items_for_slot(open_slot, class);
                let current_item = resolved.get(&open_slot);

                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    for (item_id, item) in &valid_items {
                        let is_equipped = current_item == Some(item_id);

                        // Build display text: item name + stats on same line
                        let stat_text = format_item_stats(item);
                        let display = if stat_text.is_empty() {
                            item.name.clone()
                        } else {
                            format!("{}  —  {}", item.name, stat_text)
                        };

                        let name_color = if is_equipped { gold } else { egui::Color32::from_rgb(220, 220, 220) };
                        let response = ui.selectable_label(is_equipped,
                            egui::RichText::new(&display)
                                .size(13.0)
                                .color(name_color),
                        );

                        if response.clicked() {
                            selection = Some(PickerAction::SelectItem(open_slot, *item_id));
                        }
                    }
                });
            });

        // Handle Escape to close
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            keep_open = false;
        }

        // Apply selection
        match selection {
            Some(PickerAction::SelectItem(slot, item_id)) => {
                set_equipment_override(match_config, view_state, slot, Some(item_id), items, defaults, class);
                keep_open = false;
            }
            Some(PickerAction::ResetToDefault(slot)) => {
                set_equipment_override(match_config, view_state, slot, None, items, defaults, class);
                keep_open = false;
            }
            None => {}
        }

        if !keep_open {
            picker_state.open_slot = None;
        }
    }
}

enum PickerAction {
    SelectItem(ItemSlot, ItemId),
    ResetToDefault(ItemSlot),
}

/// Apply or remove an equipment override for the viewed combatant.
/// Handles 2H/OH conflicts: equipping a 2H weapon clears off-hand,
/// equipping an off-hand clears any 2H main-hand weapon.
fn set_equipment_override(
    match_config: &mut ResMut<MatchConfig>,
    view_state: &Res<ViewCombatantState>,
    slot: ItemSlot,
    item: Option<ItemId>,
    items: &ItemDefinitions,
    defaults: &DefaultLoadouts,
    class: CharacterClass,
) {
    let equipment = if view_state.team == 1 {
        match_config.team1_equipment.get_mut(view_state.slot)
    } else {
        match_config.team2_equipment.get_mut(view_state.slot)
    };

    if let Some(equip_map) = equipment {
        match item {
            Some(id) => {
                equip_map.insert(slot, id);

                if let Some(new_item) = items.get(&id) {
                    // Equipping a 2H main-hand → clear off-hand
                    if slot == ItemSlot::MainHand && new_item.two_handed {
                        equip_map.remove(&ItemSlot::OffHand);
                    }

                    // Equipping an off-hand → replace 2H main-hand with a 1H weapon
                    if slot == ItemSlot::OffHand {
                        let resolved = resolve_loadout(class, defaults, equip_map);
                        if let Some(mh_id) = resolved.get(&ItemSlot::MainHand) {
                            if let Some(mh_item) = items.get(mh_id) {
                                if mh_item.two_handed {
                                    // Find the first 1H main-hand weapon this class can use
                                    let one_hand = items.items_for_slot(ItemSlot::MainHand, class)
                                        .into_iter()
                                        .find(|(_, item)| !item.two_handed);
                                    if let Some((replacement_id, _)) = one_hand {
                                        equip_map.insert(ItemSlot::MainHand, replacement_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            None => {
                equip_map.remove(&slot);

                // After resetting, check if the default creates a 2H conflict
                let resolved = resolve_loadout(class, defaults, equip_map);
                if slot == ItemSlot::MainHand {
                    // Reset main-hand to default — if default is 2H, clear off-hand
                    if let Some(mh_id) = resolved.get(&ItemSlot::MainHand) {
                        if let Some(mh_item) = items.get(mh_id) {
                            if mh_item.two_handed {
                                equip_map.remove(&ItemSlot::OffHand);
                            }
                        }
                    }
                } else if slot == ItemSlot::OffHand {
                    // Reset off-hand to default — if default off-hand exists and main-hand is 2H, swap main-hand
                    if resolved.contains_key(&ItemSlot::OffHand) {
                        if let Some(mh_id) = resolved.get(&ItemSlot::MainHand) {
                            if let Some(mh_item) = items.get(mh_id) {
                                if mh_item.two_handed {
                                    let one_hand = items.items_for_slot(ItemSlot::MainHand, class)
                                        .into_iter()
                                        .find(|(_, item)| !item.two_handed);
                                    if let Some((replacement_id, _)) = one_hand {
                                        equip_map.insert(ItemSlot::MainHand, replacement_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Build a list of formatted stat strings for an item.
/// Armor stats use "+X" format; weapons show absolute damage range and speed.
fn item_stat_parts(item: &ItemConfig) -> Vec<String> {
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

    parts
}

/// Format stat bonuses as a comma-separated string for inline display.
fn format_item_stats(item: &ItemConfig) -> String {
    item_stat_parts(item).join(", ")
}

/// Render a tooltip showing an item's full stat breakdown.
fn render_item_tooltip(ui: &mut egui::Ui, item: &ItemConfig) {
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

    if item.armor_type != super::play_match::equipment::ArmorType::None {
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

/// Render the Rogue Stealth Opener selection panel with ability icons
fn render_rogue_opener_panel(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
    ability_icons: &Option<Res<AbilityIcons>>,
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
                    painter.rect_stroke(rect, 4.0, egui::Stroke::new(border_width, border_color));

                    // Track click
                    if response.clicked() && !is_selected {
                        clicked_opener = Some(*opener);
                    }

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

/// Render the Hunter Pet Type selection panel
fn render_hunter_pet_panel(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
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
                (HunterPetType::Spider, egui::Color32::from_rgb(128, 102, 77)),  // Brown
                (HunterPetType::Boar, egui::Color32::from_rgb(153, 102, 77)),    // Dark brown
                (HunterPetType::Bird, egui::Color32::from_rgb(153, 179, 204)),   // Light grey-blue
            ];

            for (i, (pet, color)) in pets.iter().enumerate() {
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

                    let painter = ui.painter();
                    painter.rect_filled(rect, 4.0, *color);
                    painter.rect_stroke(rect, 4.0, egui::Stroke::new(border_width, border_color));

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

/// Render the Warlock Curse Preferences panel with ability icons
fn render_warlock_curse_panel(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    view_state: &Res<ViewCombatantState>,
    match_config: &mut ResMut<MatchConfig>,
    ability_icons: &Option<Res<AbilityIcons>>,
    class_icons: &Res<ClassIcons>,
) {
    // Clone enemy team composition to avoid borrow conflicts
    let enemy_team: Vec<Option<CharacterClass>> = if view_state.team == 1 {
        match_config.team2.clone()
    } else {
        match_config.team1.clone()
    };
    let enemy_size = enemy_team.iter().filter(|c| c.is_some()).count();

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
                        painter.rect_stroke(rect, 4.0, egui::Stroke::new(border_width, border_color));

                        // Track click
                        if response.clicked() && !is_selected {
                            changed_curse = Some((enemy_slot, *curse));
                        }

                        // Tooltip on hover
                        if response.hovered() {
                            let tooltip_text = match curse {
                                WarlockCurse::Agony => "Curse of Agony: DoT - 14 damage per 4s for 24s",
                                WarlockCurse::Weakness => "Curse of Weakness: -20% physical damage for 2 min",
                                WarlockCurse::Tongues => "Curse of Tongues: +50% cast time for 30s",
                            };
                            response.on_hover_text(tooltip_text);
                        }

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
