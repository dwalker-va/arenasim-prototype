//! Rendering Systems
//!
//! All UI and visual effect rendering for the Play Match state.
//! Split into focused modules:
//! - `hud`: Health bars, resource bars, cast bars, time controls
//! - `combat_log`: Combat log panel and ability timeline
//! - `overlays`: Countdown and victory celebration screens
//! - `effects`: Floating combat text, spell impacts, speech bubbles, shield bubbles

pub mod combat_log;
pub mod effects;
pub mod hud;
pub mod overlays;

// Re-export all public items for backwards compatibility
pub use combat_log::*;
pub use effects::*;
pub use hud::*;
pub use overlays::*;

use bevy::prelude::*;
use bevy_egui::egui;
use super::components::{SpellIcons, SpellIconHandles};

// ==============================================================================
// Spell Icon Loading (shared by multiple modules)
// ==============================================================================

/// Maps ability names to their icon file paths
pub fn get_ability_icon_path(ability: &str) -> Option<&'static str> {
    match ability {
        "Frostbolt" => Some("icons/abilities/spell_frost_frostbolt02.jpg"),
        "Frost Nova" => Some("icons/abilities/spell_frost_frostnova.jpg"),
        "Flash Heal" => Some("icons/abilities/spell_holy_flashheal.jpg"),
        "Mind Blast" => Some("icons/abilities/spell_shadow_unholyfrenzy.jpg"),
        "Power Word: Fortitude" => Some("icons/abilities/spell_holy_wordfortitude.jpg"),
        "Charge" => Some("icons/abilities/ability_warrior_charge.jpg"),
        "Rend" => Some("icons/abilities/ability_gouge.jpg"),
        "Mortal Strike" => Some("icons/abilities/ability_warrior_savageblow.jpg"),
        "Heroic Strike" => Some("icons/abilities/ability_rogue_ambush.jpg"),
        "Ambush" => Some("icons/abilities/ability_rogue_ambush.jpg"),
        "Sinister Strike" => Some("icons/abilities/spell_shadow_ritualofsacrifice.jpg"),
        "Kidney Shot" => Some("icons/abilities/ability_rogue_kidneyshot.jpg"),
        "Corruption" => Some("icons/abilities/spell_shadow_abominationexplosion.jpg"),
        "Shadowbolt" => Some("icons/abilities/spell_shadow_shadowbolt.jpg"),
        "Fear" => Some("icons/abilities/spell_shadow_possession.jpg"),
        "Pummel" => Some("icons/abilities/inv_gauntlets_04.jpg"),
        "Kick" => Some("icons/abilities/ability_kick.jpg"),
        "Arcane Intellect" => Some("icons/abilities/spell_holy_magicalsentry.jpg"),
        "Battle Shout" => Some("icons/abilities/ability_warrior_battleshout.jpg"),
        "Ice Barrier" => Some("icons/abilities/spell_ice_lament.jpg"),
        "Power Word: Shield" => Some("icons/abilities/spell_holy_powerwordshield.jpg"),
        "Polymorph" => Some("icons/abilities/spell_nature_polymorph.jpg"),
        _ => None,
    }
}

/// All abilities that have icons
pub const SPELL_ICON_ABILITIES: &[&str] = &[
    "Frostbolt", "Frost Nova", "Flash Heal", "Mind Blast", "Power Word: Fortitude",
    "Charge", "Rend", "Mortal Strike", "Heroic Strike", "Ambush",
    "Sinister Strike", "Kidney Shot", "Corruption", "Shadowbolt", "Fear",
    "Pummel", "Kick", "Arcane Intellect", "Battle Shout",
    "Ice Barrier", "Power Word: Shield", "Polymorph",
];

/// System to load spell icons and register them with egui.
/// This runs during PlayMatch state update and only loads once.
pub fn load_spell_icons(
    mut contexts: bevy_egui::EguiContexts,
    asset_server: Res<AssetServer>,
    mut spell_icons: ResMut<SpellIcons>,
    mut icon_handles: ResMut<SpellIconHandles>,
    images: Res<Assets<Image>>,
) {
    // Only load once
    if spell_icons.loaded {
        return;
    }

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        for ability in SPELL_ICON_ABILITIES {
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
        spell_icons.textures.insert(ability_name.clone(), texture_id);
    }

    spell_icons.loaded = true;
    info!("Spell icons loaded and registered with egui ({} icons)", spell_icons.textures.len());
}

// ==============================================================================
// Shared Utility Functions
// ==============================================================================

/// Helper function to draw text with a black outline for visibility.
/// Used by countdown and victory overlays.
pub fn draw_text_with_outline(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
    align: egui::Align2,
    outline_size: f32,
) {
    // Draw black outline (8 directions)
    let offsets = [
        (-outline_size, 0.0), (outline_size, 0.0), (0.0, -outline_size), (0.0, outline_size),
        (-outline_size * 0.7, -outline_size * 0.7), (outline_size * 0.7, -outline_size * 0.7),
        (-outline_size * 0.7, outline_size * 0.7), (outline_size * 0.7, outline_size * 0.7),
    ];

    for (dx, dy) in offsets {
        painter.text(
            egui::pos2(pos.x + dx, pos.y + dy),
            align,
            text,
            font_id.clone(),
            egui::Color32::BLACK,
        );
    }

    // Draw main text
    painter.text(pos, align, text, font_id, color);
}
