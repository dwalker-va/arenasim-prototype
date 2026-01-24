//! View Combatant UI - Character Details Screen
//!
//! This module displays detailed information about a combatant:
//! - Base stats (health, resource, attack/spell power, attack/move speed)
//! - List of abilities with icons
//! - Placeholder sections for Gear and Talents (Coming Soon)
//!
//! Accessed by clicking a filled character slot in Configure Match.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;
use super::{GameState, match_config::CharacterClass};
use super::configure_match_ui::ClassIcons;
use super::play_match::AbilityType;
use super::play_match::abilities::SpellSchool;
use super::play_match::ability_config::{AbilityDefinitions, AbilityConfig};
use super::play_match::components::AuraType;
use super::play_match::rendering::get_ability_icon_path;

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
            AbilityType::SinisterStrike,
            AbilityType::KidneyShot,
            AbilityType::Kick,
        ],
        CharacterClass::Priest => vec![
            AbilityType::FlashHeal,
            AbilityType::MindBlast,
            AbilityType::PowerWordFortitude,
            AbilityType::PowerWordShield,
        ],
        CharacterClass::Warlock => vec![
            AbilityType::Corruption,
            AbilityType::Shadowbolt,
            AbilityType::Fear,
            AbilityType::Immolate,
            AbilityType::DrainLife,
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

    // All abilities we need icons for
    let abilities = [
        "Frostbolt", "Frost Nova", "Flash Heal", "Mind Blast", "Power Word: Fortitude",
        "Charge", "Rend", "Mortal Strike", "Heroic Strike", "Ambush",
        "Sinister Strike", "Kidney Shot", "Corruption", "Shadowbolt", "Fear", "Immolate",
        "Drain Life", "Pummel", "Kick", "Arcane Intellect", "Battle Shout",
        "Ice Barrier", "Power Word: Shield", "Polymorph",
    ];

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        for ability in &abilities {
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
    let bottom_panel_height = 100.0;

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
                                render_stats_panel(ui, &stats, panel_width, main_panel_height);
                            },
                        );

                        ui.add_space(spacing);

                        // Abilities panel
                        ui.allocate_ui_with_layout(
                            egui::vec2(panel_width, main_panel_height),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                render_abilities_panel(ui, &abilities, panel_width, main_panel_height, &ability_icons, &ability_definitions);
                            },
                        );
                    },
                );

                ui.add_space(15.0);

                // Two-column layout for Gear and Talents (Coming Soon)
                ui.allocate_ui_with_layout(
                    egui::vec2(content_width, bottom_panel_height),
                    egui::Layout::left_to_right(egui::Align::TOP),
                    |ui| {
                        // Gear panel
                        ui.allocate_ui_with_layout(
                            egui::vec2(panel_width, bottom_panel_height),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                render_coming_soon_panel(ui, "GEAR", panel_width, bottom_panel_height);
                            },
                        );

                        ui.add_space(spacing);

                        // Talents panel
                        ui.allocate_ui_with_layout(
                            egui::vec2(panel_width, bottom_panel_height),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                render_coming_soon_panel(ui, "TALENTS", panel_width, bottom_panel_height);
                            },
                        );
                    },
                );
            });
        });
}

/// Render the Stats panel
fn render_stats_panel(ui: &mut egui::Ui, stats: &ClassStats, width: f32, height: f32) {
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
                ui.label(egui::RichText::new("Health:").size(14.0).color(egui::Color32::from_rgb(170, 170, 170)));
                ui.label(egui::RichText::new(format!("{}", stats.health)).size(14.0).color(egui::Color32::from_rgb(230, 230, 230)));
                ui.end_row();

                ui.label(egui::RichText::new("Resource:").size(14.0).color(egui::Color32::from_rgb(170, 170, 170)));
                ui.label(egui::RichText::new(format!("{} {}", stats.resource_name, stats.resource_max)).size(14.0).color(egui::Color32::from_rgb(230, 230, 230)));
                ui.end_row();

                ui.label(egui::RichText::new("Attack Power:").size(14.0).color(egui::Color32::from_rgb(170, 170, 170)));
                ui.label(egui::RichText::new(format!("{}", stats.attack_power)).size(14.0).color(egui::Color32::from_rgb(230, 230, 230)));
                ui.end_row();

                ui.label(egui::RichText::new("Spell Power:").size(14.0).color(egui::Color32::from_rgb(170, 170, 170)));
                ui.label(egui::RichText::new(format!("{}", stats.spell_power)).size(14.0).color(egui::Color32::from_rgb(230, 230, 230)));
                ui.end_row();

                ui.label(egui::RichText::new("Attack Speed:").size(14.0).color(egui::Color32::from_rgb(170, 170, 170)));
                ui.label(egui::RichText::new(format!("{:.1}/s", stats.attack_speed)).size(14.0).color(egui::Color32::from_rgb(230, 230, 230)));
                ui.end_row();

                ui.label(egui::RichText::new("Move Speed:").size(14.0).color(egui::Color32::from_rgb(170, 170, 170)));
                ui.label(egui::RichText::new(format!("{:.1}/s", stats.move_speed)).size(14.0).color(egui::Color32::from_rgb(230, 230, 230)));
                ui.end_row();
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
                let icon_key = match ability_name {
                    "Shadow Bolt" => "Shadowbolt",
                    other => other,
                };
                icons.textures.get(icon_key).copied()
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
                            render_ability_tooltip(ui, ability_name, config);
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
        SpellSchool::None => egui::Color32::from_rgb(220, 220, 220),     // Gray
    }
}

/// Render a WoW-style ability tooltip
fn render_ability_tooltip(ui: &mut egui::Ui, name: &str, config: &AbilityConfig) {
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

    // Description - build dynamically based on ability effects
    let description = build_ability_description(config);
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

/// Build a description string for an ability based on its config
fn build_ability_description(config: &AbilityConfig) -> String {
    let mut parts = Vec::new();

    // Damage
    if config.damage_base_max > 0.0 {
        if config.channel_duration.is_some() {
            // Channeled damage - show per tick
            parts.push(format!("Deals {:.0}-{:.0} damage per tick.", config.damage_base_min, config.damage_base_max));
        } else {
            parts.push(format!("Deals {:.0}-{:.0} damage.", config.damage_base_min, config.damage_base_max));
        }
    }

    // Healing
    if config.healing_base_max > 0.0 {
        parts.push(format!("Heals for {:.0}-{:.0}.", config.healing_base_min, config.healing_base_max));
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
    }
}

/// Render a "Coming Soon" panel
fn render_coming_soon_panel(ui: &mut egui::Ui, title: &str, width: f32, height: f32) {
    ui.group(|ui| {
        ui.set_min_width(width - 20.0);
        ui.set_min_height(height - 20.0);

        ui.label(
            egui::RichText::new(title)
                .size(18.0)
                .color(egui::Color32::from_rgb(120, 115, 105))
                .strong(),
        );

        ui.add_space(15.0);

        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Coming Soon")
                    .size(16.0)
                    .color(egui::Color32::from_rgb(90, 90, 90))
                    .italics(),
            );
        });
    });
}
