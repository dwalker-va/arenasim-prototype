//! Game state management
//!
//! Defines the core game states and transitions between them.

use bevy::prelude::*;

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
        app.add_systems(OnEnter(GameState::MainMenu), setup_main_menu)
            .add_systems(OnExit(GameState::MainMenu), cleanup_main_menu)
            .add_systems(OnEnter(GameState::ConfigureMatch), setup_configure_match)
            .add_systems(OnExit(GameState::ConfigureMatch), cleanup_configure_match)
            .add_systems(OnEnter(GameState::PlayMatch), setup_play_match)
            .add_systems(OnExit(GameState::PlayMatch), cleanup_play_match)
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

fn setup_main_menu(mut _commands: Commands) {
    // TODO: Implement main menu UI
    info!("Entering MainMenu state");
}

fn cleanup_main_menu(mut commands: Commands, query: Query<Entity, With<MainMenuEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn setup_configure_match(mut _commands: Commands) {
    // TODO: Implement match configuration UI
    info!("Entering ConfigureMatch state");
}

fn cleanup_configure_match(
    mut commands: Commands,
    query: Query<Entity, With<ConfigureMatchEntity>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn setup_play_match(mut _commands: Commands) {
    // TODO: Implement match gameplay
    info!("Entering PlayMatch state");
}

fn cleanup_play_match(mut commands: Commands, query: Query<Entity, With<PlayMatchEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

fn setup_results(mut _commands: Commands) {
    // TODO: Implement results UI
    info!("Entering Results state");
}

fn cleanup_results(mut commands: Commands, query: Query<Entity, With<ResultsEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

