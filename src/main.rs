//! ArenaSim - Arena Combat Autobattler Prototype
//!
//! A prototype implementation of an autobattler where players configure teams
//! of combatants and watch them battle CPU vs CPU.

use bevy::prelude::*;
use bevy_egui::EguiPlugin;

mod camera;
mod combat;
mod states;
mod ui;

use camera::CameraPlugin;
use combat::CombatPlugin;
use states::{GameState, StatesPlugin};
use ui::UiPlugin;

fn main() {
    App::new()
        // Bevy default plugins with custom window settings
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ArenaSim".to_string(),
                resolution: (1280.0, 720.0).into(),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        // Our game plugins
        .add_plugins((
            EguiPlugin,
            StatesPlugin,
            CameraPlugin,
            CombatPlugin,
            UiPlugin,
        ))
        // Start in the main menu state
        .init_state::<GameState>()
        .run();
}

