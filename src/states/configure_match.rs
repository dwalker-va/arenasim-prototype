//! Configure Match Scene
//!
//! UI for configuring a match before playing:
//! - Team size selection (1v1, 2v2, 3v3)
//! - Character selection for each team slot
//! - Map selection
//! - Start Match button

use bevy::prelude::*;

use super::match_config::{ArenaMap, CharacterClass, MatchConfig};
use super::{ConfigureMatchEntity, GameState};

/// Button types in the configure match screen
#[derive(Component, Clone, PartialEq, Eq)]
pub enum ConfigButton {
    /// Decrease team 1 size
    Team1Minus,
    /// Increase team 1 size
    Team1Plus,
    /// Decrease team 2 size
    Team2Minus,
    /// Increase team 2 size
    Team2Plus,
    /// Select a character for a team slot
    CharacterSlot { team: u8, slot: usize },
    /// Character in the roster to pick
    CharacterPick(CharacterClass),
    /// Previous map
    MapPrev,
    /// Next map
    MapNext,
    /// Start the match
    StartMatch,
    /// Back to main menu
    Back,
}

/// Marker for the character picker modal
#[derive(Component)]
pub struct CharacterPickerModal;

/// Tracks which slot is being edited in the character picker
#[derive(Resource, Default)]
pub struct CharacterPickerState {
    pub active: bool,
    pub team: u8,
    pub slot: usize,
}

/// Marker for UI elements that need updating when config changes
#[derive(Component)]
pub struct TeamSizeLabel(pub u8);

#[derive(Component)]
pub struct MapNameLabel;

#[derive(Component)]
pub struct StartMatchButton;

/// Marker for character slots that need updating when config changes
#[derive(Component, Clone, Copy)]
pub struct TeamSlot {
    pub team: u8,
    pub slot: usize,
}

/// Marker for team panel containers that get rebuilt on config change
#[derive(Component)]
pub struct TeamPanel(pub u8);

/// Marker for the main content area that holds team panels
#[derive(Component)]
pub struct MainContentArea;

/// Marker for the map panel (so it doesn't get rebuilt)
#[derive(Component)]
pub struct MapPanel;

/// Track previous config state to detect what actually changed
#[derive(Resource, Clone)]
pub(crate) struct PreviousMatchConfig {
    team1_size: usize,
    team2_size: usize,
    team1: Vec<Option<CharacterClass>>,
    team2: Vec<Option<CharacterClass>>,
}

impl From<&MatchConfig> for PreviousMatchConfig {
    fn from(config: &MatchConfig) -> Self {
        Self {
            team1_size: config.team1_size,
            team2_size: config.team2_size,
            team1: config.team1.clone(),
            team2: config.team2.clone(),
        }
    }
}

/// Colors for the configure match UI
mod colors {
    use bevy::prelude::*;

    pub const PANEL_BG: Color = Color::srgb(0.12, 0.12, 0.16);
    pub const SLOT_EMPTY: Color = Color::srgb(0.2, 0.2, 0.25);
    pub const SLOT_FILLED: Color = Color::srgb(0.25, 0.3, 0.35);
    pub const BUTTON_NORMAL: Color = Color::srgb(0.15, 0.15, 0.20);
    pub const BUTTON_HOVERED: Color = Color::srgb(0.25, 0.25, 0.35);
    pub const BUTTON_PRESSED: Color = Color::srgb(0.35, 0.35, 0.50);
    pub const BUTTON_DISABLED: Color = Color::srgb(0.1, 0.1, 0.12);
    pub const BORDER: Color = Color::srgb(0.4, 0.35, 0.25);
    pub const BORDER_HOVERED: Color = Color::srgb(0.7, 0.6, 0.4);
    pub const TEAM1: Color = Color::srgb(0.2, 0.4, 0.8);
    pub const TEAM2: Color = Color::srgb(0.8, 0.2, 0.2);
    pub const MODAL_OVERLAY: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
}

pub fn setup_configure_match(mut commands: Commands, config: Res<MatchConfig>) {
    info!("Entering ConfigureMatch state");

    // Spawn 2D camera for UI
    commands.spawn((Camera2d::default(), ConfigureMatchEntity));

    // Initialize picker state and previous config tracker
    commands.insert_resource(CharacterPickerState::default());
    commands.insert_resource(PreviousMatchConfig::from(config.as_ref()));

    // Root container
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(40.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.08, 0.12)),
            ConfigureMatchEntity,
        ))
        .with_children(|parent| {
            // Header row
            spawn_header(parent);

            // Main content area
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(40.0),
                        margin: UiRect::vertical(Val::Px(30.0)),
                        ..default()
                    },
                    MainContentArea,
                ))
                .with_children(|content| {
                    // Team 1 panel
                    spawn_team_panel(content, 1, &config);

                    // Center panel (map selection)
                    spawn_map_panel(content, &config);

                    // Team 2 panel
                    spawn_team_panel(content, 2, &config);
                });

            // Footer with Start Match button
            spawn_footer(parent, &config);
        });
}

fn spawn_header(parent: &mut ChildBuilder) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|header| {
            // Back button
            header
                .spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor(colors::BORDER),
                    BorderRadius::all(Val::Px(6.0)),
                    BackgroundColor(colors::BUTTON_NORMAL),
                    ConfigButton::Back,
                ))
                .with_child((
                    Text::new("← BACK"),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.8, 0.75, 0.7)),
                ));

            // Title
            header.spawn((
                Text::new("CONFIGURE MATCH"),
                TextFont {
                    font_size: 42.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.8, 0.6)),
            ));

            // Spacer for symmetry
            header.spawn(Node {
                width: Val::Px(100.0),
                ..default()
            });
        });
}

fn spawn_team_panel(parent: &mut ChildBuilder, team: u8, config: &MatchConfig) {
    let team_color = if team == 1 {
        colors::TEAM1
    } else {
        colors::TEAM2
    };
    let team_size = if team == 1 {
        config.team1_size
    } else {
        config.team2_size
    };
    let team_slots = if team == 1 {
        &config.team1
    } else {
        &config.team2
    };

    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(20.0)),
                border: UiRect::all(Val::Px(3.0)),
                row_gap: Val::Px(15.0),
                ..default()
            },
            BorderColor(team_color),
            BorderRadius::all(Val::Px(10.0)),
            BackgroundColor(colors::PANEL_BG),
            TeamPanel(team),
        ))
        .with_children(|panel| {
            // Team header with size controls
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|header| {
                    // Team label
                    header.spawn((
                        Text::new(format!("TEAM {}", team)),
                        TextFont {
                            font_size: 28.0,
                            ..default()
                        },
                        TextColor(team_color),
                    ));

                    // Size controls
                    header
                        .spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(10.0),
                            ..default()
                        })
                        .with_children(|controls| {
                            // Minus button
                            let minus_btn = if team == 1 {
                                ConfigButton::Team1Minus
                            } else {
                                ConfigButton::Team2Minus
                            };
                            spawn_size_button(controls, "-", minus_btn);

                            // Size display
                            controls.spawn((
                                Text::new(format!("{}", team_size)),
                                TextFont {
                                    font_size: 24.0,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                                TeamSizeLabel(team),
                            ));

                            // Plus button
                            let plus_btn = if team == 1 {
                                ConfigButton::Team1Plus
                            } else {
                                ConfigButton::Team2Plus
                            };
                            spawn_size_button(controls, "+", plus_btn);
                        });
                });

            // Character slots
            for slot in 0..3 {
                let character = team_slots.get(slot).and_then(|c| *c);
                let is_active = slot < team_size;
                spawn_character_slot(panel, team, slot, character, is_active);
            }
        });
}

fn spawn_size_button(parent: &mut ChildBuilder, label: &str, button_type: ConfigButton) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(36.0),
                height: Val::Px(36.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor(colors::BORDER),
            BorderRadius::all(Val::Px(6.0)),
            BackgroundColor(colors::BUTTON_NORMAL),
            button_type,
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: 24.0,
                ..default()
            },
            TextColor(Color::WHITE),
        ));
}

fn spawn_character_slot(
    parent: &mut ChildBuilder,
    team: u8,
    slot: usize,
    character: Option<CharacterClass>,
    is_active: bool,
) {
    let (bg_color, border_alpha) = if is_active {
        if character.is_some() {
            (colors::SLOT_FILLED, 1.0)
        } else {
            (colors::SLOT_EMPTY, 0.7)
        }
    } else {
        (colors::BUTTON_DISABLED, 0.3)
    };

    let team_color = if team == 1 {
        colors::TEAM1
    } else {
        colors::TEAM2
    };
    let border_color = team_color.with_alpha(border_alpha);

    let mut slot_entity = parent.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(70.0),
            padding: UiRect::all(Val::Px(12.0)),
            border: UiRect::all(Val::Px(2.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(15.0),
            ..default()
        },
        BorderColor(border_color),
        BorderRadius::all(Val::Px(8.0)),
        BackgroundColor(bg_color),
        ConfigButton::CharacterSlot { team, slot },
        TeamSlot { team, slot },
    ));

    if is_active {
        slot_entity.insert(Button);
    }

    slot_entity.with_children(|slot_content| {
        if let Some(class) = character {
            // Class icon placeholder (colored box)
            slot_content.spawn((
                Node {
                    width: Val::Px(46.0),
                    height: Val::Px(46.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor(class.color()),
                BorderRadius::all(Val::Px(6.0)),
                BackgroundColor(class.color().with_alpha(0.3)),
            ));

            // Class name and description
            slot_content
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|text_col| {
                    text_col.spawn((
                        Text::new(class.name()),
                        TextFont {
                            font_size: 20.0,
                            ..default()
                        },
                        TextColor(class.color()),
                    ));
                    text_col.spawn((
                        Text::new(class.description()),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.6, 0.6, 0.6)),
                    ));
                });
        } else if is_active {
            // Empty slot prompt
            slot_content.spawn((
                Text::new("Click to select character"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.5, 0.5, 0.5)),
            ));
        } else {
            // Inactive slot
            slot_content.spawn((
                Text::new("—"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.3, 0.3, 0.3)),
            ));
        }
    });
}

fn spawn_map_panel(parent: &mut ChildBuilder, config: &MatchConfig) {
    parent
        .spawn((
            Node {
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(20.0)),
                border: UiRect::all(Val::Px(2.0)),
                row_gap: Val::Px(15.0),
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor(colors::BORDER),
            BorderRadius::all(Val::Px(10.0)),
            BackgroundColor(colors::PANEL_BG),
            MapPanel,
        ))
        .with_children(|panel| {
            // Map label
            panel.spawn((
                Text::new("ARENA"),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.8, 0.6)),
            ));

            // Map preview placeholder
            panel.spawn((
                Node {
                    width: Val::Px(200.0),
                    height: Val::Px(150.0),
                    margin: UiRect::vertical(Val::Px(10.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor(Color::srgb(0.3, 0.3, 0.3)),
                BorderRadius::all(Val::Px(8.0)),
                BackgroundColor(Color::srgb(0.15, 0.15, 0.18)),
            ));

            // Map selection controls
            panel
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(15.0),
                    ..default()
                })
                .with_children(|controls| {
                    // Prev button
                    spawn_size_button(controls, "◀", ConfigButton::MapPrev);

                    // Map name
                    controls.spawn((
                        Text::new(config.map.name()),
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        MapNameLabel,
                    ));

                    // Next button
                    spawn_size_button(controls, "▶", ConfigButton::MapNext);
                });

            // Map description
            panel.spawn((
                Text::new(config.map.description()),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.6, 0.6)),
            ));

            // VS indicator
            panel.spawn((
                Text::new("VS"),
                TextFont {
                    font_size: 48.0,
                    ..default()
                },
                TextColor(Color::srgb(0.5, 0.45, 0.4)),
                Node {
                    margin: UiRect::vertical(Val::Px(20.0)),
                    ..default()
                },
            ));
        });
}

fn spawn_footer(parent: &mut ChildBuilder, config: &MatchConfig) {
    let is_valid = config.is_valid();
    let (btn_color, text_color) = if is_valid {
        (
            Color::srgb(0.2, 0.5, 0.2),
            Color::srgb(0.9, 0.95, 0.9),
        )
    } else {
        (colors::BUTTON_DISABLED, Color::srgb(0.4, 0.4, 0.4))
    };

    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|footer| {
            footer
                .spawn((
                    Button,
                    Node {
                        width: Val::Px(300.0),
                        height: Val::Px(60.0),
                        border: UiRect::all(Val::Px(3.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor(if is_valid {
                        Color::srgb(0.3, 0.7, 0.3)
                    } else {
                        colors::BORDER
                    }),
                    BorderRadius::all(Val::Px(8.0)),
                    BackgroundColor(btn_color),
                    ConfigButton::StartMatch,
                    StartMatchButton,
                ))
                .with_child((
                    Text::new(if is_valid {
                        "START MATCH"
                    } else {
                        "SELECT CHARACTERS"
                    }),
                    TextFont {
                        font_size: 28.0,
                        ..default()
                    },
                    TextColor(text_color),
                ));
        });
}

/// Spawn the character picker modal
pub fn spawn_character_picker(commands: &mut Commands, team: u8, slot: usize) {
    let team_color = if team == 1 {
        colors::TEAM1
    } else {
        colors::TEAM2
    };

    // Modal overlay
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(colors::MODAL_OVERLAY),
            CharacterPickerModal,
            ConfigureMatchEntity,
        ))
        .with_children(|overlay| {
            // Modal content
            overlay
                .spawn((
                    Node {
                        width: Val::Px(500.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(30.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        row_gap: Val::Px(20.0),
                        ..default()
                    },
                    BorderColor(team_color),
                    BorderRadius::all(Val::Px(12.0)),
                    BackgroundColor(Color::srgb(0.1, 0.1, 0.14)),
                ))
                .with_children(|modal| {
                    // Title
                    modal.spawn((
                        Text::new(format!("Select Character - Team {} Slot {}", team, slot + 1)),
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(team_color),
                    ));

                    // Character options
                    for class in CharacterClass::all() {
                        modal
                            .spawn((
                                Button,
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(70.0),
                                    padding: UiRect::all(Val::Px(12.0)),
                                    border: UiRect::all(Val::Px(2.0)),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(15.0),
                                    ..default()
                                },
                                BorderColor(class.color().with_alpha(0.5)),
                                BorderRadius::all(Val::Px(8.0)),
                                BackgroundColor(colors::SLOT_EMPTY),
                                ConfigButton::CharacterPick(*class),
                            ))
                            .with_children(|option| {
                                // Class color indicator
                                option.spawn((
                                    Node {
                                        width: Val::Px(46.0),
                                        height: Val::Px(46.0),
                                        border: UiRect::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BorderColor(class.color()),
                                    BorderRadius::all(Val::Px(6.0)),
                                    BackgroundColor(class.color().with_alpha(0.3)),
                                ));

                                // Class info
                                option
                                    .spawn(Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(4.0),
                                        ..default()
                                    })
                                    .with_children(|text_col| {
                                        text_col.spawn((
                                            Text::new(class.name()),
                                            TextFont {
                                                font_size: 22.0,
                                                ..default()
                                            },
                                            TextColor(class.color()),
                                        ));
                                        text_col.spawn((
                                            Text::new(class.description()),
                                            TextFont {
                                                font_size: 14.0,
                                                ..default()
                                            },
                                            TextColor(Color::srgb(0.6, 0.6, 0.6)),
                                        ));
                                    });
                            });
                    }
                });
        });
}

/// Handle button interactions in the configure match screen
pub fn handle_configure_buttons(
    mut commands: Commands,
    mut interaction_query: Query<
        (&Interaction, &ConfigButton, &mut BackgroundColor, &mut BorderColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut config: ResMut<MatchConfig>,
    mut picker_state: ResMut<CharacterPickerState>,
    mut next_state: ResMut<NextState<GameState>>,
    modal_query: Query<Entity, With<CharacterPickerModal>>,
) {
    for (interaction, button, mut bg_color, mut border_color) in &mut interaction_query {
        // Determine base colors based on button type
        let (normal_bg, hover_bg, pressed_bg) = match button {
            ConfigButton::StartMatch if config.is_valid() => (
                Color::srgb(0.2, 0.5, 0.2),
                Color::srgb(0.25, 0.6, 0.25),
                Color::srgb(0.3, 0.7, 0.3),
            ),
            ConfigButton::StartMatch => (
                colors::BUTTON_DISABLED,
                colors::BUTTON_DISABLED,
                colors::BUTTON_DISABLED,
            ),
            _ => (colors::BUTTON_NORMAL, colors::BUTTON_HOVERED, colors::BUTTON_PRESSED),
        };

        match *interaction {
            Interaction::Pressed => {
                *bg_color = pressed_bg.into();
                *border_color = colors::BORDER_HOVERED.into();

                match button {
                    ConfigButton::Back => {
                        next_state.set(GameState::MainMenu);
                    }
                    ConfigButton::Team1Minus => {
                        let new_size = config.team1_size.saturating_sub(1).max(1);
                        config.set_team1_size(new_size);
                    }
                    ConfigButton::Team1Plus => {
                        let new_size = (config.team1_size + 1).min(3);
                        config.set_team1_size(new_size);
                    }
                    ConfigButton::Team2Minus => {
                        let new_size = config.team2_size.saturating_sub(1).max(1);
                        config.set_team2_size(new_size);
                    }
                    ConfigButton::Team2Plus => {
                        let new_size = (config.team2_size + 1).min(3);
                        config.set_team2_size(new_size);
                    }
                    ConfigButton::CharacterSlot { team, slot } => {
                        // Open character picker
                        picker_state.active = true;
                        picker_state.team = *team;
                        picker_state.slot = *slot;
                        spawn_character_picker(&mut commands, *team, *slot);
                    }
                    ConfigButton::CharacterPick(class) => {
                        // Assign character to the slot
                        if picker_state.team == 1 {
                            if picker_state.slot < config.team1.len() {
                                config.team1[picker_state.slot] = Some(*class);
                            }
                        } else {
                            if picker_state.slot < config.team2.len() {
                                config.team2[picker_state.slot] = Some(*class);
                            }
                        }
                        // Close picker
                        picker_state.active = false;
                        for entity in modal_query.iter() {
                            commands.entity(entity).despawn_recursive();
                        }
                    }
                    ConfigButton::MapPrev => {
                        let maps = ArenaMap::all();
                        let current_idx = maps.iter().position(|m| *m == config.map).unwrap_or(0);
                        let new_idx = if current_idx == 0 {
                            maps.len() - 1
                        } else {
                            current_idx - 1
                        };
                        config.map = maps[new_idx];
                    }
                    ConfigButton::MapNext => {
                        let maps = ArenaMap::all();
                        let current_idx = maps.iter().position(|m| *m == config.map).unwrap_or(0);
                        let new_idx = (current_idx + 1) % maps.len();
                        config.map = maps[new_idx];
                    }
                    ConfigButton::StartMatch => {
                        if config.is_valid() {
                            info!("Starting match with config: {:?}", *config);
                            next_state.set(GameState::PlayMatch);
                        }
                    }
                }
            }
            Interaction::Hovered => {
                *bg_color = hover_bg.into();
                *border_color = colors::BORDER_HOVERED.into();
            }
            Interaction::None => {
                *bg_color = normal_bg.into();
                *border_color = colors::BORDER.into();
            }
        }
    }
}

/// Update UI labels when config changes
pub fn update_config_ui(
    mut commands: Commands,
    config: Res<MatchConfig>,
    mut prev_config: ResMut<PreviousMatchConfig>,
    mut team_size_labels: Query<(&mut Text, &TeamSizeLabel)>,
    mut map_label: Query<&mut Text, (With<MapNameLabel>, Without<TeamSizeLabel>)>,
    mut start_button: Query<
        (&mut BackgroundColor, &mut BorderColor, &Children),
        With<StartMatchButton>,
    >,
    mut text_query: Query<&mut TextColor, Without<TeamSizeLabel>>,
    content_area: Query<Entity, With<MainContentArea>>,
) {
    if !config.is_changed() {
        return;
    }

    // Check what actually changed - only rebuild if team compositions changed
    let teams_changed = config.team1_size != prev_config.team1_size
        || config.team2_size != prev_config.team2_size
        || config.team1 != prev_config.team1
        || config.team2 != prev_config.team2;

    // Rebuild entire content area only if team compositions changed
    // Team1Panel -> MapPanel -> Team2Panel (left to right in flexbox)
    if teams_changed {
        if let Ok(content_entity) = content_area.get_single() {
            // Despawn all children of content area
            commands.entity(content_entity).despawn_descendants();

            // Respawn everything in the correct order
            commands.entity(content_entity).with_children(|content| {
                // Team 1 panel (left)
                spawn_team_panel(content, 1, &config);

                // Map panel (center)
                spawn_map_panel(content, &config);

                // Team 2 panel (right)
                spawn_team_panel(content, 2, &config);
            });
        }

        // Update the previous config tracker
        *prev_config = PreviousMatchConfig::from(config.as_ref());
    }

    // Update team size labels
    for (mut text, label) in &mut team_size_labels {
        let size = if label.0 == 1 {
            config.team1_size
        } else {
            config.team2_size
        };
        **text = format!("{}", size);
    }

    // Update map label
    for mut text in &mut map_label {
        **text = config.map.name().to_string();
    }

    // Update start button appearance
    let is_valid = config.is_valid();
    for (mut bg_color, mut border_color, children) in &mut start_button {
        if is_valid {
            *bg_color = Color::srgb(0.2, 0.5, 0.2).into();
            *border_color = Color::srgb(0.3, 0.7, 0.3).into();
        } else {
            *bg_color = colors::BUTTON_DISABLED.into();
            *border_color = colors::BORDER.into();
        }

        // Update button text
        for child in children.iter() {
            if let Ok(mut text_color) = text_query.get_mut(*child) {
                *text_color = if is_valid {
                    Color::srgb(0.9, 0.95, 0.9).into()
                } else {
                    Color::srgb(0.4, 0.4, 0.4).into()
                };
            }
        }
    }
}

pub fn cleanup_configure_match(
    mut commands: Commands,
    query: Query<Entity, With<ConfigureMatchEntity>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    // Clean up resources
    commands.remove_resource::<CharacterPickerState>();
    commands.remove_resource::<PreviousMatchConfig>();
}

/// Handle ESC key to close modal or return to main menu
pub fn handle_configure_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<GameState>>,
    mut picker_state: ResMut<CharacterPickerState>,
    modal_query: Query<Entity, With<CharacterPickerModal>>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        // If modal is open, close it first
        if picker_state.active {
            picker_state.active = false;
            for entity in modal_query.iter() {
                commands.entity(entity).despawn_recursive();
            }
        } else {
            // Otherwise return to main menu
            next_state.set(GameState::MainMenu);
        }
    }
}

