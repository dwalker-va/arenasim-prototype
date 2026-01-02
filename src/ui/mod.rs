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
    fn build(&self, _app: &mut App) {
        // UI setup is now handled per-state in states/mod.rs
        // This plugin will hold shared UI utilities and components
    }
}

/// Marker component for UI root entities
#[derive(Component)]
pub struct UiRoot;

/// Common colors used throughout the UI
pub mod colors {
    use bevy::prelude::*;

    // Theme colors
    pub const BACKGROUND_DARK: Color = Color::srgb(0.08, 0.08, 0.12);
    pub const BACKGROUND_MEDIUM: Color = Color::srgb(0.12, 0.12, 0.16);
    pub const TEXT_PRIMARY: Color = Color::srgb(0.9, 0.85, 0.75);
    pub const TEXT_SECONDARY: Color = Color::srgb(0.6, 0.55, 0.5);
    pub const TEXT_MUTED: Color = Color::srgb(0.4, 0.4, 0.4);
    pub const ACCENT_GOLD: Color = Color::srgb(0.9, 0.8, 0.6);

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
    pub const TITLE: f32 = 72.0;
    /// Section headers
    pub const HEADER: f32 = 48.0;
    /// Sub-headers
    pub const SUBHEADER: f32 = 32.0;
    /// Button text
    pub const BUTTON: f32 = 28.0;
    /// Normal body text
    pub const BODY: f32 = 18.0;
    /// Small labels and annotations
    pub const SMALL: f32 = 14.0;
    /// Combat log text
    pub const COMBAT_LOG: f32 = 12.0;
}
