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
pub mod widgets;

use bevy::prelude::*;
use bevy_egui::egui;

/// Install the game's look on `ctx`: the ONE call the client's Startup system
/// and every `egui_kittest` harness make, so a harness cannot render a look the
/// client does not run (`tests/snapshot_font_audit.rs` holds every harness to
/// it).
///
/// It installs the font stack ([`fonts::game_font_definitions`]) and the whole
/// egui `Style` ([`theme::style`]). This is the only `set_style` in the game:
/// egui keeps one sticky `Style` per context, so a screen that patched it
/// leaked the patch into every screen reached after it. A screen that needs a
/// local variation uses `ui.scope(|ui| { ui.style_mut()… })`, which egui
/// restores on exit (`tests/ui_token_audit.rs` counts any other form).
///
/// The theme is pinned to dark, so the style set here is the one in force
/// whatever the operating system's appearance.
///
/// Idempotent and cheap: `Context::set_fonts` compares against the installed
/// definitions and does nothing when they already match, and the style is a
/// pointer swap, so a harness can call this at the top of its per-frame app
/// closure.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts::game_font_definitions());
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style(theme::style());
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
