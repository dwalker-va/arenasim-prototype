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

pub mod driver;
pub mod fonts;
pub mod theme;

use bevy::prelude::*;
use bevy_egui::egui;

/// Install the game's look on `ctx`: the ONE call the client's Startup system
/// and every `egui_kittest` harness make, so a harness cannot render a look the
/// client does not run (`tests/snapshot_font_audit.rs` holds every harness to
/// it).
///
/// Today it installs the font stack ([`fonts::game_font_definitions`]); the
/// shared `Style` built from [`theme`] joins it here, and nowhere else.
///
/// Idempotent: `Context::set_fonts` compares against the installed definitions
/// and does nothing when they already match, so a harness can call this at the
/// top of its per-frame app closure without rebuilding the atlas every frame.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts::game_font_definitions());
}

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
