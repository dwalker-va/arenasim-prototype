//! UI System
//!
//! Handles all user interface elements using bevy_egui:
//! - Main menu
//! - Options menu
//! - Match configuration
//! - In-match HUD (health bars, combat log, simulation controls)
//! - Results screen
//!
//! All UI is implemented using immediate-mode egui rather than retained-mode Bevy UI.
//! This provides better maintainability and is more suited to agentic development.

use bevy::prelude::*;

/// Plugin for UI management.
/// 
/// Currently, all UI logic is handled per-state in `states/mod.rs` and related modules.
/// This plugin exists as a placeholder for future shared UI utilities.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, _app: &mut App) {
        // UI setup is now handled per-state in states/mod.rs
        // This plugin will hold shared UI utilities and components
    }
}
