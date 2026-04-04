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

    /// Whether two slots accept the same item pool.
    /// Ring1/Ring2 are interchangeable, Trinket1/Trinket2 are interchangeable,
    /// all other slots must match exactly.
    pub fn is_same_slot_type(&self, other: &ItemSlot) -> bool {
        match (self, other) {
            (ItemSlot::Ring1, ItemSlot::Ring1 | ItemSlot::Ring2) => true,
            (ItemSlot::Ring2, ItemSlot::Ring1 | ItemSlot::Ring2) => true,
            (ItemSlot::Trinket1, ItemSlot::Trinket1 | ItemSlot::Trinket2) => true,
            (ItemSlot::Trinket2, ItemSlot::Trinket1 | ItemSlot::Trinket2) => true,
            _ => self == other,
        }
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
    // === Plate Armor — DPS (Warrior) ===
    LionheartHelm,
    OnslaughtHeadGuard,
    ConquerorsChestplate,
    LegplatesOfWrath,
    GauntletsOfMight,
    SabatonsBattleBorn,
    WaistguardOfHeroism,
    WristguardsOfStability,
    ShoulderplatesOfValor,

    // === Plate Armor — Holy (Paladin) ===
    LawbringerHelm,
    LawbringerSpaulders,
    LawbringerChestguard,
    LawbringerBracers,
    LawbringerGauntlets,
    LawbringerBelt,
    LawbringerLegplates,
    LawbringerBoots,

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
    CloakOfFrostWarding,

    // === Necklaces (all classes) ===
    AmuletOfPower,
    AmuletOfResilience,
    AmuletOfShadowWard,

    // === Rings (all classes) ===
    BandOfAccuria,
    SignetOfFocus,
    RingOfProtection,
    BandOfElementalResistance,

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
    AegisOfTheBloodGod,
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
    /// Whether this is a two-handed weapon (prevents off-hand equip)
    #[serde(default)]
    pub two_handed: bool,

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
    /// Armor rating (reduces incoming Physical damage)
    #[serde(default)]
    pub armor: f32,
    /// Fire spell resistance
    #[serde(default)]
    pub fire_resistance: f32,
    /// Frost spell resistance
    #[serde(default)]
    pub frost_resistance: f32,
    /// Shadow spell resistance
    #[serde(default)]
    pub shadow_resistance: f32,
    /// Arcane spell resistance
    #[serde(default)]
    pub arcane_resistance: f32,
    /// Nature spell resistance
    #[serde(default)]
    pub nature_resistance: f32,
    /// Holy spell resistance
    #[serde(default)]
    pub holy_resistance: f32,

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

/// Strip off-hand from a resolved loadout when the main-hand is a two-handed weapon.
/// Call this after `resolve_loadout` to enforce the 2H constraint.
pub fn enforce_two_hand_conflicts(loadout: &mut HashMap<ItemSlot, ItemId>, items: &ItemDefinitions) {
    let has_2h = loadout.get(&ItemSlot::MainHand)
        .and_then(|id| items.get(id))
        .map_or(false, |item| item.two_handed);
    if has_2h {
        loadout.remove(&ItemSlot::OffHand);
    }
}

/// Find the first available one-handed main-hand weapon for a class, sorted by name.
/// Returns None if only two-handed weapons exist.
pub fn find_one_handed_mainhand(items: &ItemDefinitions, class: CharacterClass) -> Option<ItemId> {
    items.items_for_slot(ItemSlot::MainHand, class)
        .into_iter()
        .find(|(_, item)| !item.two_handed)
        .map(|(id, _)| id)
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

    /// Return all items valid for a given slot and class, sorted by name.
    /// Ring1/Ring2 and Trinket1/Trinket2 share item pools.
    pub fn items_for_slot(&self, slot: ItemSlot, class: CharacterClass) -> Vec<(ItemId, &ItemConfig)> {
        let mut items: Vec<(ItemId, &ItemConfig)> = self.definitions.iter()
            .filter(|(_, item)| slot.is_same_slot_type(&item.slot) && can_equip(class, item))
            .map(|(id, item)| (*id, item))
            .collect();
        items.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        items
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;

    /// Build a minimal ItemDefinitions from a list of (ItemId, ItemConfig) pairs
    fn make_item_defs(items: Vec<(ItemId, ItemConfig)>) -> ItemDefinitions {
        let map: HashMap<ItemId, ItemConfig> = items.into_iter().collect();
        ItemDefinitions {
            definitions: map,
        }
    }

    /// Build a minimal ItemConfig for a non-weapon armor piece
    fn armor_item(name: &str, slot: ItemSlot, armor_type: ArmorType) -> ItemConfig {
        ItemConfig {
            name: name.to_string(),
            item_level: 60,
            slot,
            armor_type,
            weapon_type: WeaponType::None,
            allowed_classes: None,
            is_weapon: false,
            two_handed: false,
            max_health: 50.0,
            max_mana: 20.0,
            mana_regen: 1.0,
            attack_power: 10.0,
            spell_power: 5.0,
            crit_chance: 0.01,
            movement_speed: 0.0,
            armor: 0.0,
            fire_resistance: 0.0,
            frost_resistance: 0.0,
            shadow_resistance: 0.0,
            arcane_resistance: 0.0,
            nature_resistance: 0.0,
            holy_resistance: 0.0,
            attack_damage_min: 0.0,
            attack_damage_max: 0.0,
            attack_speed: 0.0,
        }
    }

    /// Build a minimal weapon ItemConfig
    fn weapon_item(name: &str, slot: ItemSlot, dmg_min: f32, dmg_max: f32, speed: f32) -> ItemConfig {
        ItemConfig {
            name: name.to_string(),
            item_level: 60,
            slot,
            armor_type: ArmorType::None,
            weapon_type: WeaponType::Sword,
            allowed_classes: None,
            is_weapon: true,
            two_handed: false,
            max_health: 0.0,
            max_mana: 0.0,
            mana_regen: 0.0,
            attack_power: 5.0,
            spell_power: 0.0,
            crit_chance: 0.0,
            movement_speed: 0.0,
            armor: 0.0,
            fire_resistance: 0.0,
            frost_resistance: 0.0,
            shadow_resistance: 0.0,
            arcane_resistance: 0.0,
            nature_resistance: 0.0,
            holy_resistance: 0.0,
            attack_damage_min: dmg_min,
            attack_damage_max: dmg_max,
            attack_speed: speed,
        }
    }

    // ---- apply_equipment tests ----

    #[test]
    fn apply_equipment_adds_armor_stats() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlot::Head, ArmorType::Plate)),
        ]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);
        let base_health = combatant.max_health;
        let base_ap = combatant.attack_power;

        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        combatant.apply_equipment(&loadout, &items);

        assert_eq!(combatant.max_health, base_health + 50.0);
        assert_eq!(combatant.attack_power, base_ap + 10.0);
        // current_health should be synced to new max
        assert_eq!(combatant.current_health, combatant.max_health);
    }

    #[test]
    fn apply_equipment_empty_loadout_unchanged() {
        let items = make_item_defs(vec![]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);
        let base_health = combatant.max_health;
        let base_damage = combatant.attack_damage;

        let loadout = HashMap::new();
        combatant.apply_equipment(&loadout, &items);

        assert_eq!(combatant.max_health, base_health);
        assert_eq!(combatant.attack_damage, base_damage);
    }

    #[test]
    fn apply_equipment_weapon_replaces_damage_for_melee() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, weapon_item("Reaper", ItemSlot::MainHand, 20.0, 30.0, 0.5)),
        ]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);

        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        combatant.apply_equipment(&loadout, &items);

        // Weapon should replace attack_damage with average
        assert_eq!(combatant.attack_damage, 25.0); // (20+30)/2
        assert_eq!(combatant.attack_speed, 0.5);
        // attack_power from weapon should still be added
        assert_eq!(combatant.attack_power, 30.0 + 5.0); // base 30 + weapon 5
    }

    #[test]
    fn apply_equipment_weapon_replaces_damage_for_ranged() {
        let items = make_item_defs(vec![
            (ItemId::WandOfShadows, weapon_item("Wand", ItemSlot::Ranged, 10.0, 14.0, 0.8)),
        ]);
        // Mage is ranged, so Ranged slot is primary weapon slot
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Mage);

        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::Ranged, ItemId::WandOfShadows);
        combatant.apply_equipment(&loadout, &items);

        assert_eq!(combatant.attack_damage, 12.0); // (10+14)/2
        assert_eq!(combatant.attack_speed, 0.8);
    }

    #[test]
    fn apply_equipment_offhand_weapon_does_not_replace_damage() {
        let items = make_item_defs(vec![
            (ItemId::WallOfTheDeadShield, weapon_item("Shield", ItemSlot::OffHand, 100.0, 200.0, 2.0)),
        ]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);
        let base_damage = combatant.attack_damage;
        let base_speed = combatant.attack_speed;

        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);
        combatant.apply_equipment(&loadout, &items);

        // Off hand weapon should NOT replace attack damage/speed
        assert_eq!(combatant.attack_damage, base_damage);
        assert_eq!(combatant.attack_speed, base_speed);
        // But attack_power from the off-hand should still be added
        assert_eq!(combatant.attack_power, 30.0 + 5.0);
    }

    // ---- resolve_loadout tests ----

    #[test]
    fn resolve_loadout_uses_defaults_when_no_overrides() {
        let mut loadout_map = HashMap::new();
        let mut warrior_loadout = HashMap::new();
        warrior_loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        warrior_loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout_map.insert(CharacterClass::Warrior, warrior_loadout);

        let defaults = DefaultLoadouts { loadouts: loadout_map };
        let overrides = HashMap::new();

        let result = resolve_loadout(CharacterClass::Warrior, &defaults, &overrides);
        assert_eq!(result.get(&ItemSlot::Head), Some(&ItemId::LionheartHelm));
        assert_eq!(result.get(&ItemSlot::MainHand), Some(&ItemId::ArcaniteReaper));
    }

    #[test]
    fn resolve_loadout_overrides_replace_defaults() {
        let mut loadout_map = HashMap::new();
        let mut warrior_loadout = HashMap::new();
        warrior_loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout_map.insert(CharacterClass::Warrior, warrior_loadout);

        let defaults = DefaultLoadouts { loadouts: loadout_map };
        let mut overrides = HashMap::new();
        overrides.insert(ItemSlot::MainHand, ItemId::FrostbiteBlade);

        let result = resolve_loadout(CharacterClass::Warrior, &defaults, &overrides);
        assert_eq!(result.get(&ItemSlot::MainHand), Some(&ItemId::FrostbiteBlade));
    }

    #[test]
    fn resolve_loadout_missing_class_returns_only_overrides() {
        let defaults = DefaultLoadouts { loadouts: HashMap::new() };
        let mut overrides = HashMap::new();
        overrides.insert(ItemSlot::Head, ItemId::LionheartHelm);

        let result = resolve_loadout(CharacterClass::Warrior, &defaults, &overrides);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get(&ItemSlot::Head), Some(&ItemId::LionheartHelm));
    }

    // ---- can_equip tests ----

    #[test]
    fn can_equip_plate_on_warrior() {
        let item = armor_item("Plate Helm", ItemSlot::Head, ArmorType::Plate);
        assert!(can_equip(CharacterClass::Warrior, &item));
    }

    #[test]
    fn can_equip_plate_on_mage_fails() {
        let item = armor_item("Plate Helm", ItemSlot::Head, ArmorType::Plate);
        assert!(!can_equip(CharacterClass::Mage, &item));
    }

    #[test]
    fn can_equip_cloth_on_warrior() {
        let item = armor_item("Cloth Robe", ItemSlot::Chest, ArmorType::Cloth);
        assert!(can_equip(CharacterClass::Warrior, &item));
    }

    #[test]
    fn can_equip_class_restricted_item() {
        let mut item = armor_item("Warrior Only Helm", ItemSlot::Head, ArmorType::Plate);
        item.allowed_classes = Some(vec![CharacterClass::Warrior]);
        assert!(can_equip(CharacterClass::Warrior, &item));
        assert!(!can_equip(CharacterClass::Paladin, &item));
    }

    #[test]
    fn can_equip_accessory_on_any_class() {
        let item = armor_item("Ring", ItemSlot::Ring1, ArmorType::None);
        assert!(can_equip(CharacterClass::Mage, &item));
        assert!(can_equip(CharacterClass::Warrior, &item));
        assert!(can_equip(CharacterClass::Rogue, &item));
    }

    // ---- format_loadout tests ----

    #[test]
    fn format_loadout_empty() {
        let items = make_item_defs(vec![]);
        let loadout = HashMap::new();
        assert_eq!(format_loadout(&loadout, &items), "No equipment");
    }

    #[test]
    fn format_loadout_single_item() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Lionheart Helm", ItemSlot::Head, ArmorType::Plate)),
        ]);
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        let result = format_loadout(&loadout, &items);
        assert_eq!(result, "Head=Lionheart Helm");
    }

    #[test]
    fn format_loadout_respects_slot_order() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, weapon_item("Arcanite Reaper", ItemSlot::MainHand, 20.0, 30.0, 0.5)),
            (ItemId::LionheartHelm, armor_item("Lionheart Helm", ItemSlot::Head, ArmorType::Plate)),
        ]);
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        let result = format_loadout(&loadout, &items);
        // Head comes before MainHand in ItemSlot::all() ordering
        assert!(result.starts_with("Head="));
        assert!(result.contains("Main Hand=Arcanite Reaper"));
    }

    // ---- validate_class_restrictions tests ----

    #[test]
    fn validate_class_restrictions_passes_for_valid_loadout() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlot::Head, ArmorType::Plate)),
        ]);
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Warrior, &loadout, &items).is_ok());
    }

    #[test]
    fn validate_class_restrictions_fails_wrong_armor_type() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlot::Head, ArmorType::Plate)),
        ]);
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Mage, &loadout, &items).is_err());
    }

    #[test]
    fn validate_class_restrictions_fails_wrong_slot() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlot::Head, ArmorType::Plate)),
        ]);
        let mut loadout = HashMap::new();
        // Place a Head item in the Chest slot
        loadout.insert(ItemSlot::Chest, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Warrior, &loadout, &items).is_err());
    }

    #[test]
    fn validate_class_restrictions_fails_unknown_item() {
        let items = make_item_defs(vec![]); // empty
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Warrior, &loadout, &items).is_err());
    }

    // ---- is_same_slot_type tests ----

    #[test]
    fn is_same_slot_type_exact_match() {
        assert!(ItemSlot::Head.is_same_slot_type(&ItemSlot::Head));
        assert!(ItemSlot::MainHand.is_same_slot_type(&ItemSlot::MainHand));
    }

    #[test]
    fn is_same_slot_type_ring_interchangeable() {
        assert!(ItemSlot::Ring1.is_same_slot_type(&ItemSlot::Ring2));
        assert!(ItemSlot::Ring2.is_same_slot_type(&ItemSlot::Ring1));
        assert!(ItemSlot::Ring1.is_same_slot_type(&ItemSlot::Ring1));
    }

    #[test]
    fn is_same_slot_type_trinket_interchangeable() {
        assert!(ItemSlot::Trinket1.is_same_slot_type(&ItemSlot::Trinket2));
        assert!(ItemSlot::Trinket2.is_same_slot_type(&ItemSlot::Trinket1));
    }

    #[test]
    fn is_same_slot_type_different_slots() {
        assert!(!ItemSlot::Ring1.is_same_slot_type(&ItemSlot::Neck));
        assert!(!ItemSlot::Head.is_same_slot_type(&ItemSlot::Chest));
        assert!(!ItemSlot::Trinket1.is_same_slot_type(&ItemSlot::Ring1));
    }

    // ---- items_for_slot tests ----

    #[test]
    fn items_for_slot_filters_by_armor_type() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Plate Helm", ItemSlot::Head, ArmorType::Plate)),
            (ItemId::MagistersCrown, armor_item("Cloth Crown", ItemSlot::Head, ArmorType::Cloth)),
        ]);
        // Warrior can wear plate; Mage cannot
        let warrior_head = items.items_for_slot(ItemSlot::Head, CharacterClass::Warrior);
        assert_eq!(warrior_head.len(), 2); // warrior can wear both plate and cloth
        let mage_head = items.items_for_slot(ItemSlot::Head, CharacterClass::Mage);
        assert_eq!(mage_head.len(), 1); // mage can only wear cloth
        assert_eq!(mage_head[0].0, ItemId::MagistersCrown);
    }

    #[test]
    fn items_for_slot_ring2_shows_all_rings() {
        let items = make_item_defs(vec![
            (ItemId::BandOfAccuria, armor_item("Band of Accuria", ItemSlot::Ring1, ArmorType::None)),
            (ItemId::RingOfProtection, armor_item("Ring of Protection", ItemSlot::Ring2, ArmorType::None)),
            (ItemId::SignetOfFocus, armor_item("Signet of Focus", ItemSlot::Ring1, ArmorType::None)),
        ]);
        let ring2_items = items.items_for_slot(ItemSlot::Ring2, CharacterClass::Mage);
        assert_eq!(ring2_items.len(), 3); // all ring items available for Ring2
    }

    #[test]
    fn items_for_slot_trinket_shows_all_trinkets() {
        let items = make_item_defs(vec![
            (ItemId::MarkOfTheChampion, armor_item("Mark of Champion", ItemSlot::Trinket1, ArmorType::None)),
            (ItemId::EssenceOfEternalLife, armor_item("Essence of Life", ItemSlot::Trinket1, ArmorType::None)),
        ]);
        let trinket2_items = items.items_for_slot(ItemSlot::Trinket2, CharacterClass::Warrior);
        assert_eq!(trinket2_items.len(), 2); // both trinkets available for Trinket2
    }

    #[test]
    fn items_for_slot_respects_class_restrictions() {
        let mut warrior_only = armor_item("Warrior Helm", ItemSlot::Head, ArmorType::Plate);
        warrior_only.allowed_classes = Some(vec![CharacterClass::Warrior]);
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, warrior_only),
        ]);
        let warrior_items = items.items_for_slot(ItemSlot::Head, CharacterClass::Warrior);
        assert_eq!(warrior_items.len(), 1);
        let paladin_items = items.items_for_slot(ItemSlot::Head, CharacterClass::Paladin);
        assert_eq!(paladin_items.len(), 0);
    }

    #[test]
    fn items_for_slot_sorted_by_name() {
        let items = make_item_defs(vec![
            (ItemId::BandOfAccuria, armor_item("Zebra Ring", ItemSlot::Ring1, ArmorType::None)),
            (ItemId::SignetOfFocus, armor_item("Alpha Ring", ItemSlot::Ring1, ArmorType::None)),
        ]);
        let ring_items = items.items_for_slot(ItemSlot::Ring1, CharacterClass::Warrior);
        assert_eq!(ring_items[0].1.name, "Alpha Ring");
        assert_eq!(ring_items[1].1.name, "Zebra Ring");
    }

    // ---- enforce_two_hand_conflicts tests ----

    fn two_handed_weapon(name: &str) -> ItemConfig {
        let mut item = weapon_item(name, ItemSlot::MainHand, 20.0, 30.0, 0.9);
        item.two_handed = true;
        item
    }

    #[test]
    fn enforce_2h_strips_offhand_when_mainhand_is_2h() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, two_handed_weapon("Arcanite Reaper")),
            (ItemId::WallOfTheDeadShield, armor_item("Shield", ItemSlot::OffHand, ArmorType::None)),
        ]);
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);

        enforce_two_hand_conflicts(&mut loadout, &items);

        assert_eq!(loadout.get(&ItemSlot::MainHand), Some(&ItemId::ArcaniteReaper));
        assert!(!loadout.contains_key(&ItemSlot::OffHand), "Off-hand should be stripped when 2H is equipped");
    }

    #[test]
    fn enforce_2h_keeps_offhand_when_mainhand_is_1h() {
        let items = make_item_defs(vec![
            (ItemId::FrostbiteBlade, weapon_item("Frostbite", ItemSlot::MainHand, 10.0, 14.0, 1.1)),
            (ItemId::WallOfTheDeadShield, armor_item("Shield", ItemSlot::OffHand, ArmorType::None)),
        ]);
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::MainHand, ItemId::FrostbiteBlade);
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);

        enforce_two_hand_conflicts(&mut loadout, &items);

        assert!(loadout.contains_key(&ItemSlot::OffHand), "Off-hand should remain with 1H weapon");
    }

    #[test]
    fn enforce_2h_no_mainhand_is_noop() {
        let items = make_item_defs(vec![
            (ItemId::WallOfTheDeadShield, armor_item("Shield", ItemSlot::OffHand, ArmorType::None)),
        ]);
        let mut loadout = HashMap::new();
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);

        enforce_two_hand_conflicts(&mut loadout, &items);

        assert!(loadout.contains_key(&ItemSlot::OffHand), "Off-hand should remain when no main-hand");
    }

    // ---- find_one_handed_mainhand tests ----

    #[test]
    fn find_1h_returns_first_non_2h_weapon() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, two_handed_weapon("Arcanite Reaper")),
            (ItemId::FrostbiteBlade, weapon_item("Frostbite Blade", ItemSlot::MainHand, 10.0, 14.0, 1.1)),
        ]);
        let result = find_one_handed_mainhand(&items, CharacterClass::Warrior);
        assert_eq!(result, Some(ItemId::FrostbiteBlade));
    }

    #[test]
    fn find_1h_returns_none_when_only_2h_exist() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, two_handed_weapon("Arcanite Reaper")),
            (ItemId::CrescentStaff, two_handed_weapon("Crescent Staff")),
        ]);
        let result = find_one_handed_mainhand(&items, CharacterClass::Warrior);
        assert_eq!(result, None);
    }
}
