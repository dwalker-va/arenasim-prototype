//! Equipment System
//!
//! Data-driven equipment definitions loaded from RON config files.
//! Items provide stat bonuses applied to combatants at spawn time.
//!
//! ## Usage
//! ```ignore
//! fn my_system(items: Res<ItemDefinitions>, loadouts: Res<DefaultLoadouts>) {
//!     let item = items.get(&ItemId::ArcaniteReaper).unwrap();
//!     println!("Arcanite Reaper attack damage: {}-{}", item.attack_damage_min, item.attack_damage_max);
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::states::match_config::CharacterClass;

// ============================================================================
// ENUMS
// ============================================================================

/// Equipment slot — 17 slots matching WoW Classic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemSlot {
    Head,
    Neck,
    Shoulders,
    Back,
    Chest,
    Wrists,
    Hands,
    Waist,
    Legs,
    Feet,
    Ring1,
    Ring2,
    Trinket1,
    Trinket2,
    MainHand,
    OffHand,
    Ranged,
}

impl ItemSlot {
    pub fn all() -> &'static [ItemSlot] {
        &[
            ItemSlot::Head, ItemSlot::Neck, ItemSlot::Shoulders, ItemSlot::Back,
            ItemSlot::Chest, ItemSlot::Wrists, ItemSlot::Hands, ItemSlot::Waist,
            ItemSlot::Legs, ItemSlot::Feet, ItemSlot::Ring1, ItemSlot::Ring2,
            ItemSlot::Trinket1, ItemSlot::Trinket2, ItemSlot::MainHand,
            ItemSlot::OffHand, ItemSlot::Ranged,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            ItemSlot::Head => "Head",
            ItemSlot::Neck => "Neck",
            ItemSlot::Shoulders => "Shoulders",
            ItemSlot::Back => "Back",
            ItemSlot::Chest => "Chest",
            ItemSlot::Wrists => "Wrists",
            ItemSlot::Hands => "Hands",
            ItemSlot::Waist => "Waist",
            ItemSlot::Legs => "Legs",
            ItemSlot::Feet => "Feet",
            ItemSlot::Ring1 => "Ring 1",
            ItemSlot::Ring2 => "Ring 2",
            ItemSlot::Trinket1 => "Trinket 1",
            ItemSlot::Trinket2 => "Trinket 2",
            ItemSlot::MainHand => "Main Hand",
            ItemSlot::OffHand => "Off Hand",
            ItemSlot::Ranged => "Ranged",
        }
    }

    /// Whether this slot holds a weapon (determines stat replacement behavior)
    pub fn is_weapon_slot(&self) -> bool {
        matches!(self, ItemSlot::MainHand | ItemSlot::OffHand | ItemSlot::Ranged)
    }
}

/// Armor type restriction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArmorType {
    Cloth,
    Leather,
    Mail,
    Plate,
    /// Accessories (rings, trinkets, neck, back) and weapons
    None,
}

/// Weapon type for flavor/naming
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WeaponType {
    Sword,
    Mace,
    Axe,
    Dagger,
    Staff,
    Polearm,
    Fist,
    Bow,
    Gun,
    Crossbow,
    Wand,
    Thrown,
    Shield,
    OffhandFrill,
    None,
}

/// Unique item identifier — each named item in the game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemId {
    // === Plate Armor (Warrior, Paladin) ===
    LionheartHelm,
    OnslaughtHeadGuard,
    ConquerorsChestplate,
    LegplatesOfWrath,
    GauntletsOfMight,
    SabatonsBattleBorn,
    WaistguardOfHeroism,
    WristguardsOfStability,
    ShoulderplatesOfValor,

    // === Mail Armor (Hunter, Warrior, Paladin) ===
    BeaststalkerHelm,
    BeaststalkerTunic,
    BeaststalkerLegs,
    BeaststalkerGloves,
    BeaststalkerBoots,
    BeaststalkerBelt,
    BeaststalkerBracers,
    BeaststalkerMantle,

    // === Leather Armor (Rogue, Hunter) ===
    NightstalkerCowl,
    NightstalkerTunic,
    NightstalkerLegs,
    NightstalkerGloves,
    NightstalkerBoots,
    NightstalkerBelt,
    NightstalkerBracers,
    NightstalkerMantle,

    // === Cloth Armor (Mage, Priest, Warlock) ===
    MagistersCrown,
    MagistersRobes,
    MagistersLeggings,
    MagistersGloves,
    MagistersBoots,
    MagistersBelt,
    MagistersBracers,
    MagistersMantle,

    // === Cloaks (all classes) ===
    CloakOfTheShieldWall,
    CloakOfConcentration,

    // === Necklaces (all classes) ===
    AmuletOfPower,
    AmuletOfResilience,

    // === Rings (all classes) ===
    BandOfAccuria,
    SignetOfFocus,
    RingOfProtection,

    // === Trinkets (all classes) ===
    MarkOfTheChampion,
    EssenceOfEternalLife,

    // === Melee Weapons ===
    ArcaniteReaper,
    FrostbiteBlade,
    SerpentFangDagger,
    HammerOfTheRighteous,
    CrescentStaff,

    // === Ranged Weapons ===
    WandOfShadows,
    StaffOfDominance,
    AshwoodBow,
    SniperScope,

    // === Off Hand ===
    TomeOfKnowledge,
    WallOfTheDeadShield,
}

// ============================================================================
// ITEM CONFIG
// ============================================================================

/// Item definition loaded from RON
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemConfig {
    /// Display name
    pub name: String,
    /// Item level (informational, determines stat budget)
    #[serde(default)]
    pub item_level: u32,
    /// Which slot this item equips to
    pub slot: ItemSlot,
    /// Armor type restriction
    #[serde(default = "default_armor_type")]
    pub armor_type: ArmorType,
    /// Weapon type (flavor only)
    #[serde(default = "default_weapon_type")]
    pub weapon_type: WeaponType,
    /// If set, only these classes can equip this item
    #[serde(default)]
    pub allowed_classes: Option<Vec<CharacterClass>>,
    /// Whether this item is a weapon (replaces attack_damage/attack_speed instead of adding)
    #[serde(default)]
    pub is_weapon: bool,

    // === Stat Bonuses ===
    #[serde(default)]
    pub max_health: f32,
    #[serde(default)]
    pub max_mana: f32,
    #[serde(default)]
    pub mana_regen: f32,
    #[serde(default)]
    pub attack_power: f32,
    #[serde(default)]
    pub spell_power: f32,
    #[serde(default)]
    pub crit_chance: f32,
    #[serde(default)]
    pub movement_speed: f32,

    // === Weapon Stats (only for is_weapon: true) ===
    /// Weapon minimum damage (replaces combatant attack_damage for primary weapon slot)
    #[serde(default)]
    pub attack_damage_min: f32,
    /// Weapon maximum damage (replaces combatant attack_damage for primary weapon slot)
    #[serde(default)]
    pub attack_damage_max: f32,
    /// Weapon attack speed (replaces combatant attack_speed for primary weapon slot)
    #[serde(default)]
    pub attack_speed: f32,
}

fn default_armor_type() -> ArmorType {
    ArmorType::None
}

fn default_weapon_type() -> WeaponType {
    WeaponType::None
}

// ============================================================================
// CLASS RESTRICTION HELPERS
// ============================================================================

/// Get the highest armor type a class can wear
fn max_armor_type(class: CharacterClass) -> &'static [ArmorType] {
    match class {
        CharacterClass::Warrior | CharacterClass::Paladin => &[ArmorType::Cloth, ArmorType::Leather, ArmorType::Mail, ArmorType::Plate, ArmorType::None],
        CharacterClass::Hunter => &[ArmorType::Cloth, ArmorType::Leather, ArmorType::Mail, ArmorType::None],
        CharacterClass::Rogue => &[ArmorType::Cloth, ArmorType::Leather, ArmorType::None],
        CharacterClass::Mage | CharacterClass::Priest | CharacterClass::Warlock => &[ArmorType::Cloth, ArmorType::None],
    }
}

/// Check if a class can equip a specific item
pub fn can_equip(class: CharacterClass, item: &ItemConfig) -> bool {
    // Check class restriction list
    if let Some(ref allowed) = item.allowed_classes {
        if !allowed.contains(&class) {
            return false;
        }
    }

    // Check armor type
    if !max_armor_type(class).contains(&item.armor_type) {
        return false;
    }

    true
}

/// Validate that all items in a loadout are equippable by the given class
pub fn validate_class_restrictions(
    class: CharacterClass,
    loadout: &HashMap<ItemSlot, ItemId>,
    items: &ItemDefinitions,
) -> Result<(), String> {
    for (slot, item_id) in loadout {
        if let Some(item) = items.get(item_id) {
            if !can_equip(class, item) {
                return Err(format!(
                    "{} cannot equip {} ({:?}) in {:?} slot — armor type {:?} not allowed",
                    class.name(), item.name, item_id, slot, item.armor_type
                ));
            }
            if item.slot != *slot {
                return Err(format!(
                    "{:?} is a {:?} item but was placed in {:?} slot",
                    item_id, item.slot, slot
                ));
            }
        } else {
            return Err(format!("Unknown item {:?} in {:?} slot", item_id, slot));
        }
    }
    Ok(())
}

// ============================================================================
// LOADOUT RESOLUTION
// ============================================================================

/// Merge default loadout with optional per-slot overrides
pub fn resolve_loadout(
    class: CharacterClass,
    defaults: &DefaultLoadouts,
    overrides: &HashMap<ItemSlot, ItemId>,
) -> HashMap<ItemSlot, ItemId> {
    let mut loadout = defaults.get(class).cloned().unwrap_or_default();
    for (slot, item_id) in overrides {
        loadout.insert(*slot, *item_id);
    }
    loadout
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Root structure for items.ron
#[derive(Debug, Serialize, Deserialize)]
pub struct ItemsConfig {
    pub items: HashMap<ItemId, ItemConfig>,
}

/// Resource containing all item definitions
#[derive(Resource)]
pub struct ItemDefinitions {
    definitions: HashMap<ItemId, ItemConfig>,
}

impl ItemDefinitions {
    pub fn new(config: ItemsConfig) -> Self {
        Self {
            definitions: config.items,
        }
    }

    pub fn get(&self, item: &ItemId) -> Option<&ItemConfig> {
        self.definitions.get(item)
    }

    pub fn get_unchecked(&self, item: &ItemId) -> &ItemConfig {
        self.definitions.get(item)
            .unwrap_or_else(|| panic!("Item {:?} not found in definitions", item))
    }

    pub fn item_count(&self) -> usize {
        self.definitions.len()
    }
}

/// Root structure for loadouts.ron
#[derive(Debug, Serialize, Deserialize)]
pub struct LoadoutsConfig {
    pub loadouts: HashMap<CharacterClass, HashMap<ItemSlot, ItemId>>,
}

/// Resource containing default loadouts per class
#[derive(Resource)]
pub struct DefaultLoadouts {
    loadouts: HashMap<CharacterClass, HashMap<ItemSlot, ItemId>>,
}

impl DefaultLoadouts {
    pub fn new(config: LoadoutsConfig) -> Self {
        Self {
            loadouts: config.loadouts,
        }
    }

    pub fn get(&self, class: CharacterClass) -> Option<&HashMap<ItemSlot, ItemId>> {
        self.loadouts.get(&class)
    }
}

// ============================================================================
// LOADING
// ============================================================================

/// Load item definitions from assets/config/items.ron
pub fn load_item_definitions() -> Result<ItemDefinitions, String> {
    let config_path = "assets/config/items.ron";

    let contents = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;

    let config: ItemsConfig = ron::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {}", config_path, e))?;

    let definitions = ItemDefinitions::new(config);

    info!("Loaded {} item definitions from {}", definitions.item_count(), config_path);

    Ok(definitions)
}

/// Load default loadouts from assets/config/loadouts.ron
pub fn load_default_loadouts(items: &ItemDefinitions) -> Result<DefaultLoadouts, String> {
    let config_path = "assets/config/loadouts.ron";

    let contents = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;

    let config: LoadoutsConfig = ron::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {}", config_path, e))?;

    let loadouts = DefaultLoadouts::new(config);

    // Validate all loadout references resolve and pass class restrictions
    for class in CharacterClass::all() {
        if let Some(loadout) = loadouts.get(*class) {
            for (slot, item_id) in loadout {
                if items.get(item_id).is_none() {
                    return Err(format!(
                        "Default loadout for {} references unknown item {:?} in {:?} slot",
                        class.name(), item_id, slot
                    ));
                }
            }
            validate_class_restrictions(*class, loadout, items)?;
        }
    }

    info!("Loaded default loadouts from {}", config_path);

    Ok(loadouts)
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Bevy plugin for equipment loading
pub struct EquipmentPlugin;

impl Plugin for EquipmentPlugin {
    fn build(&self, app: &mut App) {
        match load_item_definitions() {
            Ok(definitions) => {
                match load_default_loadouts(&definitions) {
                    Ok(loadouts) => {
                        app.insert_resource(definitions);
                        app.insert_resource(loadouts);
                    }
                    Err(e) => {
                        panic!("Failed to load default loadouts: {}", e);
                    }
                }
            }
            Err(e) => {
                panic!("Failed to load item definitions: {}", e);
            }
        }
    }
}

// ============================================================================
// EQUIPMENT FORMATTING (for combat log)
// ============================================================================

/// Format an equipment loadout for combat log display
pub fn format_loadout(
    loadout: &HashMap<ItemSlot, ItemId>,
    items: &ItemDefinitions,
) -> String {
    if loadout.is_empty() {
        return "No equipment".to_string();
    }

    let mut parts: Vec<String> = Vec::new();
    for slot in ItemSlot::all() {
        if let Some(item_id) = loadout.get(slot) {
            if let Some(item) = items.get(item_id) {
                parts.push(format!("{}={}", slot.name(), item.name));
            }
        }
    }

    if parts.is_empty() {
        "No equipment".to_string()
    } else {
        parts.join(", ")
    }
}
