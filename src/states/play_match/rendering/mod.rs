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
use super::components::{SpellIcons, SpellIconHandles, Aura, AuraType};

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
        "Cheap Shot" => Some("icons/abilities/ability_cheapshot.jpg"),
        "Sinister Strike" => Some("icons/abilities/spell_shadow_ritualofsacrifice.jpg"),
        "Kidney Shot" => Some("icons/abilities/ability_rogue_kidneyshot.jpg"),
        "Corruption" => Some("icons/abilities/spell_shadow_abominationexplosion.jpg"),
        "Shadowbolt" | "Shadow Bolt" => Some("icons/abilities/spell_shadow_shadowbolt.jpg"),
        "Fear" => Some("icons/abilities/spell_shadow_possession.jpg"),
        "Immolate" => Some("icons/abilities/spell_fire_immolation.jpg"),
        "Drain Life" => Some("icons/abilities/spell_shadow_lifedrain02.jpg"),
        "Pummel" => Some("icons/abilities/inv_gauntlets_04.jpg"),
        "Kick" => Some("icons/abilities/ability_kick.jpg"),
        "Arcane Intellect" => Some("icons/abilities/spell_holy_magicalsentry.jpg"),
        "Battle Shout" => Some("icons/abilities/ability_warrior_battleshout.jpg"),
        "Ice Barrier" => Some("icons/abilities/spell_ice_lament.jpg"),
        "Power Word: Shield" => Some("icons/abilities/spell_holy_powerwordshield.jpg"),
        "Polymorph" => Some("icons/abilities/spell_nature_polymorph.jpg"),
        "Dispel Magic" => Some("icons/abilities/spell_holy_dispelmagic.jpg"),
        "Curse of Agony" => Some("icons/abilities/spell_shadow_curseofsargeras.jpg"),
        "Curse of Weakness" => Some("icons/abilities/spell_shadow_curseofmannoroth.jpg"),
        "Curse of Tongues" => Some("icons/abilities/spell_shadow_curseoftounges.jpg"),
        // Paladin abilities
        "Flash of Light" => Some("icons/abilities/spell_holy_flashheal.jpg"),
        "Holy Light" => Some("icons/abilities/spell_holy_holybolt.jpg"),
        "Holy Shock" => Some("icons/abilities/spell_holy_searinglight.jpg"),
        "Holy Shock (Heal)" => Some("icons/abilities/spell_holy_searinglight.jpg"),
        "Holy Shock (Damage)" => Some("icons/abilities/spell_holy_searinglight.jpg"),
        "Hammer of Justice" => Some("icons/abilities/spell_holy_sealofmight.jpg"),
        "Cleanse" => Some("icons/abilities/spell_holy_renew.jpg"),
        "Devotion Aura" => Some("icons/abilities/spell_holy_devotionaura.jpg"),
        "Divine Shield" => Some("icons/abilities/spell_holy_divineintervention.jpg"),
        _ => None,
    }
}

/// All abilities that have icons
pub const SPELL_ICON_ABILITIES: &[&str] = &[
    "Frostbolt", "Frost Nova", "Flash Heal", "Mind Blast", "Power Word: Fortitude",
    "Charge", "Rend", "Mortal Strike", "Heroic Strike", "Ambush", "Cheap Shot",
    "Sinister Strike", "Kidney Shot", "Corruption", "Shadowbolt", "Fear", "Immolate",
    "Drain Life", "Pummel", "Kick", "Arcane Intellect", "Battle Shout",
    "Ice Barrier", "Power Word: Shield", "Polymorph", "Dispel Magic",
    "Curse of Agony", "Curse of Weakness", "Curse of Tongues",
    // Paladin abilities
    "Flash of Light", "Holy Light", "Holy Shock", "Holy Shock (Heal)", "Holy Shock (Damage)",
    "Hammer of Justice", "Cleanse", "Devotion Aura", "Divine Shield",
];

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
pub fn get_aura_icon_key(aura: &Aura) -> String {
    // Check if the ability that created this aura has a specific icon
    if get_ability_icon_path(&aura.ability_name).is_some() {
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
        AuraType::DamageImmunity
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
) {
    // Only load once
    if spell_icons.loaded {
        return;
    }

    // Load handles if not already loaded
    if icon_handles.handles.is_empty() {
        // Load ability icons
        for ability in SPELL_ICON_ABILITIES {
            if let Some(path) = get_ability_icon_path(ability) {
                let handle: Handle<Image> = asset_server.load(path);
                icon_handles.handles.push((ability.to_string(), handle));
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
