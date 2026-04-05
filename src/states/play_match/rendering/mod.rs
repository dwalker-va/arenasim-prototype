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
use super::ability_config::AbilityDefinitions;
use super::components::{SpellIcons, SpellIconHandles, Aura, AuraType};

// ==============================================================================
// Aura Icon Constants and Helpers
// ==============================================================================

/// Size of aura icons in pixels
pub const AURA_ICON_SIZE: f32 = 24.0;
/// Spacing between aura icons in pixels
pub const AURA_ICON_SPACING: f32 = 2.0;

/// Generic aura icon keys (used when ability doesn't have a specific icon)
pub const GENERIC_AURA_ICONS: &[(&str, &str)] = &[
    ("aura_slow", "icons/auras/slow.jpg"),
    ("aura_root", "icons/auras/root.jpg"),
    ("aura_stun", "icons/auras/stun.jpg"),
    ("aura_fear", "icons/auras/fear.jpg"),
    ("aura_dot", "icons/auras/dot.jpg"),
    ("aura_absorb", "icons/auras/absorb.jpg"),
    ("aura_healing_reduction", "icons/auras/healing_reduction.jpg"),
    ("aura_max_health", "icons/auras/max_health_buff.jpg"),
    ("aura_lockout", "icons/auras/lockout.jpg"),
    ("aura_weakened_soul", "icons/auras/weakened_soul.jpg"),
];

/// Get the icon key for an aura.
/// Returns the ability name if it has a specific icon, otherwise returns a generic key.
pub fn get_aura_icon_key(aura: &Aura, ability_definitions: &AbilityDefinitions) -> String {
    // Check if the ability that created this aura has a specific icon
    let has_icon = ability_definitions.iter().any(|(_, config)| {
        config.name == aura.ability_name && !config.icon.is_empty()
    });
    if has_icon {
        return aura.ability_name.clone();
    }

    // Fall back to generic aura type icon
    match aura.effect_type {
        AuraType::MovementSpeedSlow => "aura_slow".to_string(),
        AuraType::Root => "aura_root".to_string(),
        AuraType::Stun => "aura_stun".to_string(),
        AuraType::Fear => "aura_fear".to_string(),
        AuraType::DamageOverTime => "aura_dot".to_string(),
        AuraType::Absorb => "aura_absorb".to_string(),
        AuraType::HealingReduction => "aura_healing_reduction".to_string(),
        AuraType::MaxHealthIncrease => "aura_max_health".to_string(),
        AuraType::MaxManaIncrease => "aura_max_health".to_string(), // Reuse health icon
        AuraType::AttackPowerIncrease => "aura_max_health".to_string(), // Reuse buff icon
        AuraType::SpellSchoolLockout => "aura_lockout".to_string(),
        AuraType::WeakenedSoul => "aura_weakened_soul".to_string(),
        AuraType::Polymorph => "aura_stun".to_string(), // Reuse stun icon as fallback
        AuraType::ShadowSight => "aura_dot".to_string(), // Reuse DoT icon as fallback
        AuraType::DamageReduction => "aura_dot".to_string(), // Curse debuff, reuse DoT icon
        AuraType::CastTimeIncrease => "aura_dot".to_string(), // Curse debuff, reuse DoT icon
        AuraType::DamageTakenReduction => "aura_max_health".to_string(), // Devotion Aura buff, reuse buff icon
        AuraType::DamageImmunity => "aura_absorb".to_string(), // Divine Shield, reuse absorb icon as fallback
        AuraType::Incapacitate => "aura_stun".to_string(), // Reuse stun icon (frozen in place)
        AuraType::SpellResistanceBuff => "aura_max_health".to_string(), // Resistance buff, reuse buff icon
        AuraType::AttackPowerReduction => "aura_dot".to_string(), // Debuff, reuse DoT icon
        AuraType::CritChanceIncrease => "aura_max_health".to_string(), // Buff, reuse buff icon
        AuraType::ManaRegenIncrease => "aura_max_health".to_string(), // Buff, reuse buff icon
        AuraType::AttackSpeedSlow => "aura_slow".to_string(), // Slow debuff
        AuraType::CastTimeReduction => "aura_max_health".to_string(), // Buff, reuse buff icon
        AuraType::FrostArmorBuff => "aura_absorb".to_string(), // Self-buff, reuse absorb icon
    }
}

/// Determine if an aura type is a buff (beneficial) or debuff (harmful).
/// Used for border color: gold for buffs, red for debuffs.
pub fn is_buff_aura(aura_type: &AuraType) -> bool {
    matches!(aura_type,
        AuraType::Absorb |
        AuraType::MaxHealthIncrease |
        AuraType::MaxManaIncrease |
        AuraType::AttackPowerIncrease |
        AuraType::ShadowSight |
        AuraType::DamageTakenReduction |
        AuraType::DamageImmunity |
        AuraType::CritChanceIncrease |
        AuraType::ManaRegenIncrease |
        AuraType::CastTimeReduction |
        AuraType::FrostArmorBuff |
        AuraType::SpellResistanceBuff
    )
}

/// System to load spell icons and register them with egui.
/// This runs during PlayMatch state update and only loads once.
/// Loads both ability-specific icons and generic aura fallback icons.
pub fn load_spell_icons(
    mut contexts: bevy_egui::EguiContexts,
    asset_server: Res<AssetServer>,
    mut spell_icons: ResMut<SpellIcons>,
    mut icon_handles: ResMut<SpellIconHandles>,
    images: Res<Assets<Image>>,
    ability_definitions: Res<AbilityDefinitions>,
) {
    // Only load once
    if spell_icons.loaded {
        return;
    }

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        // Load ability icons from data-driven definitions
        for (_ability_type, config) in ability_definitions.iter() {
            if !config.icon.is_empty() {
                let handle: Handle<Image> = asset_server.load(&config.icon);
                icon_handles.handles.push((config.name.clone(), handle));
            }
        }
        // Load generic aura icons
        for (key, path) in GENERIC_AURA_ICONS {
            let handle: Handle<Image> = asset_server.load(*path);
            icon_handles.handles.push((key.to_string(), handle));
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

    // Register variant keys for abilities logged with suffixed names (e.g., Paladin AI
    // logs "Holy Shock (Heal)" and "Holy Shock (Damage)" but the canonical name is "Holy Shock")
    if let Some(texture_id) = spell_icons.textures.get("Holy Shock").copied() {
        spell_icons.textures.insert("Holy Shock (Heal)".to_string(), texture_id);
        spell_icons.textures.insert("Holy Shock (Damage)".to_string(), texture_id);
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
