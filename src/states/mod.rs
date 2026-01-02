//! Game state management
//!
//! Defines the core game states and transitions between them.

use bevy::prelude::*;

pub mod configure_match;
pub mod match_config;

use configure_match::{
    cleanup_configure_match, handle_configure_buttons, handle_configure_escape,
    setup_configure_match, update_config_ui,
};
pub use match_config::MatchConfig;

/// The core game states representing the main screens/modes of the game.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Main menu - entry point, navigate to other states
    #[default]
    MainMenu,
    /// Options menu - video/audio settings
    Options,
    /// Match configuration - team setup, map selection
    ConfigureMatch,
    /// Active match - the autobattle simulation
    PlayMatch,
    /// Post-match results - statistics and breakdown
    Results,
}

/// Plugin for managing game states and transitions
pub struct StatesPlugin;

impl Plugin for StatesPlugin {
    fn build(&self, app: &mut App) {
        app
            // Initialize match config resource
            .init_resource::<MatchConfig>()
            // Main menu systems
            .add_systems(OnEnter(GameState::MainMenu), setup_main_menu)
            .add_systems(OnExit(GameState::MainMenu), cleanup_main_menu)
            .add_systems(
                Update,
                handle_main_menu_buttons.run_if(in_state(GameState::MainMenu)),
            )
            // Configure match systems
            .add_systems(OnEnter(GameState::ConfigureMatch), setup_configure_match)
            .add_systems(OnExit(GameState::ConfigureMatch), cleanup_configure_match)
            .add_systems(
                Update,
                (handle_configure_buttons, update_config_ui, handle_configure_escape)
                    .chain()
                    .run_if(in_state(GameState::ConfigureMatch)),
            )
            // Play match systems
            .add_systems(OnEnter(GameState::PlayMatch), setup_play_match)
            .add_systems(OnExit(GameState::PlayMatch), cleanup_play_match)
            // Results systems
            .add_systems(OnEnter(GameState::Results), setup_results)
            .add_systems(OnExit(GameState::Results), cleanup_results);
    }
}

/// Marker component for main menu entities
#[derive(Component)]
pub struct MainMenuEntity;

/// Marker component for configure match entities
#[derive(Component)]
pub struct ConfigureMatchEntity;

/// Marker component for play match entities
#[derive(Component)]
pub struct PlayMatchEntity;

/// Marker component for results entities
#[derive(Component)]
pub struct ResultsEntity;

// ============================================================================
// Main Menu
// ============================================================================

/// Identifies which button this is in the main menu
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuButton {
    Match,
    Options,
    Exit,
}

/// Colors for button states
mod button_colors {
    use bevy::prelude::*;

    pub const NORMAL: Color = Color::srgb(0.15, 0.15, 0.20);
    pub const HOVERED: Color = Color::srgb(0.25, 0.25, 0.35);
    pub const PRESSED: Color = Color::srgb(0.35, 0.35, 0.50);
    pub const BORDER: Color = Color::srgb(0.4, 0.35, 0.25);
    pub const BORDER_HOVERED: Color = Color::srgb(0.7, 0.6, 0.4);
}

fn setup_main_menu(mut commands: Commands) {
    info!("Entering MainMenu state");

    // Spawn 2D camera for UI
    commands.spawn((Camera2d::default(), MainMenuEntity));

    // Root container - full screen, centered content
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(20.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.08, 0.12)),
            MainMenuEntity,
        ))
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("ARENASIM"),
                TextFont {
                    font_size: 72.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.8, 0.6)),
                Node {
                    margin: UiRect::bottom(Val::Px(40.0)),
                    ..default()
                },
            ));

            // Subtitle
            parent.spawn((
                Text::new("Arena Combat Autobattler"),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.55, 0.5)),
                Node {
                    margin: UiRect::bottom(Val::Px(60.0)),
                    ..default()
                },
            ));

            // Match button
            spawn_menu_button(parent, "MATCH", MainMenuButton::Match);

            // Options button
            spawn_menu_button(parent, "OPTIONS", MainMenuButton::Options);

            // Exit button
            spawn_menu_button(parent, "EXIT", MainMenuButton::Exit);

            // Version/footer text
            parent.spawn((
                Text::new("v0.1.0 - Prototype"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::srgb(0.4, 0.4, 0.4)),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(20.0),
                    right: Val::Px(20.0),
                    ..default()
                },
            ));
        });
}

fn spawn_menu_button(parent: &mut ChildBuilder, text: &str, button_type: MainMenuButton) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(280.0),
                height: Val::Px(60.0),
                border: UiRect::all(Val::Px(3.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor(button_colors::BORDER),
            BorderRadius::all(Val::Px(8.0)),
            BackgroundColor(button_colors::NORMAL),
            button_type,
        ))
        .with_child((
            Text::new(text),
            TextFont {
                font_size: 28.0,
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.85, 0.75)),
        ));
}

fn handle_main_menu_buttons(
    mut interaction_query: Query<
        (
            &Interaction,
            &MainMenuButton,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (Changed<Interaction>, With<Button>),
    >,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit_events: EventWriter<AppExit>,
) {
    for (interaction, button_type, mut bg_color, mut border_color) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *bg_color = button_colors::PRESSED.into();
                *border_color = button_colors::BORDER_HOVERED.into();

                // Handle button action
                match button_type {
                    MainMenuButton::Match => {
                        info!("Match button pressed - transitioning to ConfigureMatch");
                        next_state.set(GameState::ConfigureMatch);
                    }
                    MainMenuButton::Options => {
                        info!("Options button pressed - transitioning to Options");
                        next_state.set(GameState::Options);
                    }
                    MainMenuButton::Exit => {
                        info!("Exit button pressed - quitting application");
                        exit_events.send(AppExit::Success);
                    }
                }
            }
            Interaction::Hovered => {
                *bg_color = button_colors::HOVERED.into();
                *border_color = button_colors::BORDER_HOVERED.into();
            }
            Interaction::None => {
                *bg_color = button_colors::NORMAL.into();
                *border_color = button_colors::BORDER.into();
            }
        }
    }
}

fn cleanup_main_menu(mut commands: Commands, query: Query<Entity, With<MainMenuEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

// ============================================================================
// Play Match (placeholder)
// ============================================================================

fn setup_play_match(mut commands: Commands) {
    info!("Entering PlayMatch state");

    // Spawn 2D camera for UI (temporary - will be 3D later)
    commands.spawn((Camera2d::default(), PlayMatchEntity));

    // Placeholder UI
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.08, 0.12)),
            PlayMatchEntity,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Match In Progress"),
                TextFont {
                    font_size: 48.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.8, 0.6)),
            ));

            parent.spawn((
                Text::new("Combat simulation coming soon..."),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.55, 0.5)),
                Node {
                    margin: UiRect::top(Val::Px(20.0)),
                    ..default()
                },
            ));

            parent.spawn((
                Text::new("Press ESC to return to menu"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.5, 0.5, 0.5)),
                Node {
                    margin: UiRect::top(Val::Px(40.0)),
                    ..default()
                },
            ));
        });
}

fn cleanup_play_match(mut commands: Commands, query: Query<Entity, With<PlayMatchEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

// ============================================================================
// Results (placeholder)
// ============================================================================

fn setup_results(mut _commands: Commands) {
    info!("Entering Results state");
}

fn cleanup_results(mut commands: Commands, query: Query<Entity, With<ResultsEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}
