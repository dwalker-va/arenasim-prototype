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
pub mod emoji;
pub mod hud;
pub mod overlays;
pub mod team_frames;

// Re-export all public items for backwards compatibility
pub use combat_log::*;
pub use effects::*;
pub use emoji::*;
pub use hud::*;
pub use overlays::*;
pub use team_frames::*;

use super::ability_config::AbilityDefinitions;
use super::components::{Aura, AuraType, SpellIconHandles, SpellIcons};
use super::equipment::{ItemDefinitions, ItemId};
use bevy::prelude::*;
use bevy_egui::egui;

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
    (
        "aura_healing_reduction",
        "icons/auras/healing_reduction.jpg",
    ),
    ("aura_max_health", "icons/auras/max_health_buff.jpg"),
    ("aura_lockout", "icons/auras/lockout.jpg"),
    ("aura_weakened_soul", "icons/auras/weakened_soul.jpg"),
    ("aura_shadow_sight", "icons/auras/shadow_sight.jpg"),
];

/// The icon key an aura applied by `item` draws under — a proc trinket's buff
/// wears its trinket's own icon.
///
/// Lives in the same string keyspace as the ability names and the
/// `GENERIC_AURA_ICONS` keys, and is registered into BOTH aura-drawing icon
/// registries by [`item_aura_icons`] — `SpellIcons` in-match (buff bar, team
/// frames) and `AbilityIcons` outside it (encyclopedia, View Combatant) — so
/// every surface draws one aura with one icon. The `item:` prefix cannot
/// collide with an ability name or a generic key.
pub fn item_aura_icon_key(item: ItemId) -> String {
    format!("item:{:?}", item)
}

/// `(key, path)` for every item that can APPLY an aura — today, every item
/// carrying a `proc:` — keyed by [`item_aura_icon_key`]. Derived from
/// `items.ron`, so trinket N+1 gets its icon registered with no code change.
pub fn item_aura_icons(items: &ItemDefinitions) -> Vec<(String, String)> {
    let mut icons: Vec<(String, String)> = items
        .iter()
        .filter(|(_, item)| item.proc.is_some() && !item.icon.is_empty())
        .map(|(id, item)| (item_aura_icon_key(*id), item.icon.clone()))
        .collect();
    // `ItemDefinitions` is a HashMap; sort so the load order is not
    // per-process. Nothing reads the order, but nothing should have to know.
    icons.sort();
    icons
}

/// Get the icon key for an aura.
///
/// An aura an ITEM applied draws the item's icon ([`item_aura_icon_key`]).
/// Otherwise: the applying ability's name if it has a specific icon, else a
/// generic per-type key.
pub fn get_aura_icon_key(aura: &Aura, ability_definitions: &AbilityDefinitions) -> String {
    if let Some(item) = aura.source_item {
        return item_aura_icon_key(item);
    }
    // Check if the ability that created this aura has a specific icon
    let has_icon = ability_definitions
        .iter()
        .any(|(_, config)| config.name == aura.ability_name && !config.icon.is_empty());
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
        AuraType::ShadowSight => "aura_shadow_sight".to_string(),
        AuraType::DamageReduction => "aura_dot".to_string(), // Curse debuff, reuse DoT icon
        AuraType::CastTimeIncrease => "aura_dot".to_string(), // Curse debuff, reuse DoT icon
        AuraType::DamageTakenReduction => "aura_max_health".to_string(), // Devotion Aura buff, reuse buff icon
        AuraType::DamageImmunity => "aura_absorb".to_string(), // Divine Shield, reuse absorb icon as fallback
        AuraType::Incapacitate => "aura_stun".to_string(),     // Reuse stun icon (frozen in place)
        AuraType::SpellResistanceBuff => "aura_max_health".to_string(), // Resistance buff, reuse buff icon
        AuraType::AttackPowerReduction => "aura_dot".to_string(),       // Debuff, reuse DoT icon
        AuraType::CritChanceIncrease => "aura_max_health".to_string(),  // Buff, reuse buff icon
        AuraType::ManaRegenIncrease => "aura_max_health".to_string(),   // Buff, reuse buff icon
        AuraType::AttackSpeedSlow => "aura_slow".to_string(),           // Slow debuff
        AuraType::LockoutDurationReduction => "aura_max_health".to_string(), // Buff, reuse buff icon
        AuraType::FrostArmorBuff => "aura_absorb".to_string(), // Self-buff, reuse absorb icon
        AuraType::Silence => "aura_silence".to_string(),
        AuraType::WeaponPoison => "aura_dot".to_string(), // Poison self-buff, reuse DoT icon
        AuraType::SpellPowerIncrease => "aura_max_health".to_string(), // Totem buff, reuse buff icon
        AuraType::HealingOverTime => "aura_max_health".to_string(), // Healing Stream Totem buff, reuse buff icon
        AuraType::WindfuryBuff => "aura_max_health".to_string(), // Windfury Totem buff, reuse buff icon
        AuraType::FearImmunity => "aura_max_health".to_string(), // Berserker Rage buff, reuse buff icon (real icon comes from the ability config)
    }
}

/// Determine if an aura type is a buff (beneficial) or debuff (harmful).
/// Used for border color (gold for buffs, red for debuffs) and for the
/// encyclopedia's buff/debuff split.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** This was a `matches!`
/// allowlist, which silently classified variant N+1 as a DEBUFF: a new buff
/// would have drawn a red border in the buff bar and filed itself under
/// DEBUFFS in the encyclopedia catalog with no test able to notice. The
/// classification below is unchanged from that allowlist — only the compiler
/// guard is new.
pub fn is_buff_aura(aura_type: &AuraType) -> bool {
    match aura_type {
        // Beneficial: defensives, throughput and the two informational markers
        // a player carries on themselves.
        AuraType::Absorb
        | AuraType::MaxHealthIncrease
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::ShadowSight
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::SpellResistanceBuff
        | AuraType::WeaponPoison
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::FearImmunity
        | AuraType::WindfuryBuff => true,

        // Harmful: crowd control, damage over time and stat/casting debuffs.
        AuraType::MovementSpeedSlow
        | AuraType::Root
        | AuraType::Stun
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Silence
        | AuraType::SpellSchoolLockout
        | AuraType::DamageOverTime
        | AuraType::HealingReduction
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::AttackPowerReduction
        | AuraType::AttackSpeedSlow => false,

        // Weakened Soul is the Power Word: Shield cooldown marker the Priest
        // hangs on the ally it just shielded. Not beneficial — it is what stops
        // the next shield — so it reads as a debuff, as it always has.
        AuraType::WeakenedSoul => false,
    }
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
    item_definitions: Res<ItemDefinitions>,
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
        // ...and the art of every item that can apply an aura, so a proc buff
        // draws its trinket.
        for (key, path) in item_aura_icons(&item_definitions) {
            let handle: Handle<Image> = asset_server.load(path);
            icon_handles.handles.push((key, handle));
        }
        return; // Wait for next frame to check if loaded
    }

    // Wait while any handle is still resolving, but treat a FAILED load (e.g. a
    // missing icon file) as resolved — otherwise a single bad path would block
    // the whole registration forever and blank EVERY in-match icon. Once nothing
    // is still loading, register only the textures that actually loaded; a
    // missing icon then degrades to "no icon" instead of breaking the UI.
    use bevy::asset::LoadState;
    let still_loading = icon_handles.handles.iter().any(|(_, h)| {
        matches!(
            asset_server.load_state(h.id()),
            LoadState::Loading | LoadState::NotLoaded
        )
    });
    if still_loading {
        return; // Wait for images to finish loading or fail
    }

    // Register textures with egui (skip any that failed to load)
    for (ability_name, handle) in &icon_handles.handles {
        if !images.contains(handle) {
            warn!(
                "Spell icon for '{}' failed to load; rendering without it",
                ability_name
            );
            continue;
        }
        let texture_id = contexts.add_image(handle.clone());
        spell_icons
            .textures
            .insert(ability_name.clone(), texture_id);
    }

    // Register variant keys for abilities logged with suffixed names (e.g., Paladin AI
    // logs "Holy Shock (Heal)" and "Holy Shock (Damage)" but the canonical name is "Holy Shock")
    if let Some(texture_id) = spell_icons.textures.get("Holy Shock").copied() {
        spell_icons
            .textures
            .insert("Holy Shock (Heal)".to_string(), texture_id);
        spell_icons
            .textures
            .insert("Holy Shock (Damage)".to_string(), texture_id);
    }

    spell_icons.loaded = true;
    info!(
        "Spell icons loaded and registered with egui ({} icons)",
        spell_icons.textures.len()
    );
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
        (-outline_size, 0.0),
        (outline_size, 0.0),
        (0.0, -outline_size),
        (0.0, outline_size),
        (-outline_size * 0.7, -outline_size * 0.7),
        (outline_size * 0.7, -outline_size * 0.7),
        (-outline_size * 0.7, outline_size * 0.7),
        (outline_size * 0.7, outline_size * 0.7),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::equipment::load_item_definitions;

    /// The buff bar and team frames draw a proc buff under its TRINKET's key,
    /// and that key is one the loaders actually register — at the trinket's
    /// own icon, for every shipped proc trinket. Named members, not a count:
    /// the proc set is read from `items.ron` and each must appear.
    #[test]
    fn a_proc_buff_draws_its_own_trinket() {
        let items = load_item_definitions().expect("items.ron must load");
        let abilities = AbilityDefinitions::default();
        let registered = item_aura_icons(&items);
        let mut procs = 0;
        for (id, item) in items.iter() {
            let Some(proc) = &item.proc else { continue };
            procs += 1;
            let aura = proc.aura(*id, &item.name);
            let key = get_aura_icon_key(&aura, &abilities);
            assert_eq!(
                key,
                item_aura_icon_key(*id),
                "{} draws the wrong key",
                item.name
            );
            assert!(
                registered.contains(&(key.clone(), item.icon.clone())),
                "{}'s key {key} is not registered at its icon {}",
                item.name,
                item.icon
            );
        }
        assert!(procs > 0, "no proc trinket ships — vacuous");
        assert_eq!(
            registered.len(),
            procs,
            "a non-proc item is registered as aura art"
        );
    }

    /// An aura with no source item still resolves exactly as before: two
    /// different trinkets never share a key, and an ordinary buff never takes
    /// an item's.
    #[test]
    fn only_an_item_sourced_aura_takes_an_item_key() {
        let abilities = AbilityDefinitions::default();
        let plain = Aura {
            effect_type: AuraType::AttackPowerIncrease,
            ability_name: "Battle Shout".to_string(),
            ..Default::default()
        };
        assert!(!get_aura_icon_key(&plain, &abilities).starts_with("item:"));
        assert_ne!(
            item_aura_icon_key(ItemId::DragonspineTrophy),
            item_aura_icon_key(ItemId::WhetstoneOfFury)
        );
    }
}
