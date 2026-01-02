//! UI System
//!
//! Handles all user interface elements including:
//! - Main menu
//! - Match configuration
//! - In-match HUD (health bars, combat log, simulation controls)
//! - Results screen

use bevy::prelude::*;

/// Plugin for UI management
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_ui);
    }
}

/// Marker component for UI root entities
#[derive(Component)]
pub struct UiRoot;

fn setup_ui(mut _commands: Commands) {
    // TODO: Set up UI camera and root elements
    info!("UI system initialized");
}

/// Common colors used throughout the UI
pub mod colors {
    use bevy::prelude::*;

    /// Team 1 color (blue-ish)
    pub const TEAM_1: Color = Color::srgb(0.2, 0.4, 0.8);
    /// Team 2 color (red-ish)
    pub const TEAM_2: Color = Color::srgb(0.8, 0.2, 0.2);
    /// Health bar color
    pub const HEALTH: Color = Color::srgb(0.2, 0.8, 0.2);
    /// Health bar low color
    pub const HEALTH_LOW: Color = Color::srgb(0.8, 0.2, 0.2);
    /// Mana color
    pub const MANA: Color = Color::srgb(0.2, 0.4, 0.9);
    /// Rage color
    pub const RAGE: Color = Color::srgb(0.8, 0.2, 0.2);
    /// Energy color
    pub const ENERGY: Color = Color::srgb(0.9, 0.9, 0.2);
    /// Buff border color
    pub const BUFF: Color = Color::srgb(0.2, 0.6, 0.2);
    /// Debuff border color
    pub const DEBUFF: Color = Color::srgb(0.6, 0.2, 0.2);
}

/// Font sizes used throughout the UI
pub mod fonts {
    /// Large title text
    pub const TITLE: f32 = 48.0;
    /// Section headers
    pub const HEADER: f32 = 32.0;
    /// Normal body text
    pub const BODY: f32 = 18.0;
    /// Small labels and annotations
    pub const SMALL: f32 = 14.0;
    /// Combat log text
    pub const COMBAT_LOG: f32 = 12.0;
}

