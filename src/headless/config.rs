//! JSON configuration parsing for headless mode
//!
//! Parses JSON match configurations and converts them to the game's MatchConfig format.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::states::match_config::{ArenaMap, CharacterClass, HunterPetType, MatchConfig, RogueOpener, WarlockCurse};
use crate::states::play_match::equipment::{ItemId, ItemSlot};

/// Headless match configuration loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessMatchConfig {
    /// Team 1 composition (1-3 class names)
    pub team1: Vec<String>,
    /// Team 2 composition (1-3 class names)
    pub team2: Vec<String>,
    /// Arena map name (default: "BasicArena")
    #[serde(default = "default_map")]
    pub map: String,
    /// Team 1's kill target priority (0-based index into enemy team)
    #[serde(default)]
    pub team1_kill_target: Option<usize>,
    /// Team 2's kill target priority (0-based index into enemy team)
    #[serde(default)]
    pub team2_kill_target: Option<usize>,
    /// Team 1's CC target priority (0-based index into enemy team, None = use heuristics)
    #[serde(default)]
    pub team1_cc_target: Option<usize>,
    /// Team 2's CC target priority (0-based index into enemy team, None = use heuristics)
    #[serde(default)]
    pub team2_cc_target: Option<usize>,
    /// Custom output path for match log (optional)
    #[serde(default)]
    pub output_path: Option<String>,
    /// Maximum match duration in seconds (default: 300)
    #[serde(default = "default_max_duration")]
    pub max_duration_secs: f32,
    /// Random seed for deterministic match reproduction
    /// If provided, the match will use a seeded RNG for reproducible results
    #[serde(default)]
    pub random_seed: Option<u64>,
    /// Team 1's rogue opener preferences (one per slot: "Ambush" or "CheapShot")
    #[serde(default)]
    pub team1_rogue_openers: Vec<String>,
    /// Team 2's rogue opener preferences (one per slot: "Ambush" or "CheapShot")
    #[serde(default)]
    pub team2_rogue_openers: Vec<String>,
    /// Team 1's warlock curse preferences: outer vec indexed by slot, inner vec indexed by enemy target
    /// Values: "Agony", "Weakness", "Tongues" (defaults to Agony)
    #[serde(default)]
    pub team1_warlock_curse_prefs: Vec<Option<Vec<String>>>,
    /// Team 2's warlock curse preferences: outer vec indexed by slot, inner vec indexed by enemy target
    #[serde(default)]
    pub team2_warlock_curse_prefs: Vec<Option<Vec<String>>>,
    /// Team 1's hunter pet type preferences (one per slot: "Spider", "Boar", "Bird")
    #[serde(default)]
    pub team1_hunter_pet_types: Vec<String>,
    /// Team 2's hunter pet type preferences (one per slot: "Spider", "Boar", "Bird")
    #[serde(default)]
    pub team2_hunter_pet_types: Vec<String>,
    /// Team 1's equipment overrides: outer vec indexed by slot, inner map is slot_name -> item_name
    #[serde(default)]
    pub team1_equipment: Vec<HashMap<String, String>>,
    /// Team 2's equipment overrides: outer vec indexed by slot, inner map is slot_name -> item_name
    #[serde(default)]
    pub team2_equipment: Vec<HashMap<String, String>>,
}

fn default_map() -> String {
    "BasicArena".to_string()
}

fn default_max_duration() -> f32 {
    300.0
}

impl HeadlessMatchConfig {
    /// Load configuration from a JSON file
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: HeadlessMatchConfig = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration
    fn validate(&self) -> Result<(), String> {
        // Validate team sizes
        if self.team1.is_empty() || self.team1.len() > 3 {
            return Err("team1 must have 1-3 members".to_string());
        }
        if self.team2.is_empty() || self.team2.len() > 3 {
            return Err("team2 must have 1-3 members".to_string());
        }

        // Validate class names
        for class_name in self.team1.iter().chain(self.team2.iter()) {
            Self::parse_class(class_name)?;
        }

        // Validate map name
        Self::parse_map(&self.map)?;

        // Validate kill targets
        if let Some(target) = self.team1_kill_target {
            if target >= self.team2.len() {
                return Err(format!(
                    "team1_kill_target {} is out of range (team2 has {} members)",
                    target,
                    self.team2.len()
                ));
            }
        }
        if let Some(target) = self.team2_kill_target {
            if target >= self.team1.len() {
                return Err(format!(
                    "team2_kill_target {} is out of range (team1 has {} members)",
                    target,
                    self.team1.len()
                ));
            }
        }

        // Validate CC targets
        if let Some(target) = self.team1_cc_target {
            if target >= self.team2.len() {
                return Err(format!(
                    "team1_cc_target {} is out of range (team2 has {} members)",
                    target,
                    self.team2.len()
                ));
            }
        }
        if let Some(target) = self.team2_cc_target {
            if target >= self.team1.len() {
                return Err(format!(
                    "team2_cc_target {} is out of range (team1 has {} members)",
                    target,
                    self.team1.len()
                ));
            }
        }

        // Validate max duration
        if self.max_duration_secs <= 0.0 {
            return Err("max_duration_secs must be positive".to_string());
        }

        Ok(())
    }

    /// Parse a class name string into CharacterClass
    fn parse_class(name: &str) -> Result<CharacterClass, String> {
        match name {
            "Warrior" => Ok(CharacterClass::Warrior),
            "Mage" => Ok(CharacterClass::Mage),
            "Rogue" => Ok(CharacterClass::Rogue),
            "Priest" => Ok(CharacterClass::Priest),
            "Warlock" => Ok(CharacterClass::Warlock),
            "Paladin" => Ok(CharacterClass::Paladin),
            "Hunter" => Ok(CharacterClass::Hunter),
            _ => Err(format!(
                "Unknown class: '{}'. Valid classes: Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter",
                name
            )),
        }
    }

    /// Parse a map name string into ArenaMap
    fn parse_map(name: &str) -> Result<ArenaMap, String> {
        match name {
            "BasicArena" => Ok(ArenaMap::BasicArena),
            "PillaredArena" => Ok(ArenaMap::PillaredArena),
            _ => Err(format!(
                "Unknown map: '{}'. Valid maps: BasicArena, PillaredArena",
                name
            )),
        }
    }

    /// Parse a rogue opener name string into RogueOpener
    fn parse_rogue_opener(name: &str) -> RogueOpener {
        match name {
            "CheapShot" | "Cheap Shot" => RogueOpener::CheapShot,
            _ => RogueOpener::Ambush, // Default to Ambush for unknown values
        }
    }

    /// Parse a hunter pet type name string into HunterPetType
    fn parse_hunter_pet_type(name: &str) -> HunterPetType {
        match name {
            "Boar" => HunterPetType::Boar,
            "Bird" => HunterPetType::Bird,
            _ => HunterPetType::Spider, // Default to Spider for unknown values
        }
    }

    /// Parse a warlock curse name string into WarlockCurse
    fn parse_warlock_curse(name: &str) -> WarlockCurse {
        match name {
            "Weakness" | "CurseOfWeakness" | "Curse of Weakness" => WarlockCurse::Weakness,
            "Tongues" | "CurseOfTongues" | "Curse of Tongues" => WarlockCurse::Tongues,
            _ => WarlockCurse::Agony, // Default to Agony for unknown values
        }
    }

    /// Parse an item slot name string into ItemSlot
    fn parse_item_slot(name: &str) -> Result<ItemSlot, String> {
        match name {
            "Head" => Ok(ItemSlot::Head),
            "Neck" => Ok(ItemSlot::Neck),
            "Shoulders" => Ok(ItemSlot::Shoulders),
            "Back" => Ok(ItemSlot::Back),
            "Chest" => Ok(ItemSlot::Chest),
            "Wrists" => Ok(ItemSlot::Wrists),
            "Hands" => Ok(ItemSlot::Hands),
            "Waist" => Ok(ItemSlot::Waist),
            "Legs" => Ok(ItemSlot::Legs),
            "Feet" => Ok(ItemSlot::Feet),
            "Ring1" => Ok(ItemSlot::Ring1),
            "Ring2" => Ok(ItemSlot::Ring2),
            "Trinket1" => Ok(ItemSlot::Trinket1),
            "Trinket2" => Ok(ItemSlot::Trinket2),
            "MainHand" => Ok(ItemSlot::MainHand),
            "OffHand" => Ok(ItemSlot::OffHand),
            "Ranged" => Ok(ItemSlot::Ranged),
            _ => Err(format!(
                "Unknown item slot: '{}'. Valid slots: Head, Neck, Shoulders, Back, Chest, Wrists, Hands, Waist, Legs, Feet, Ring1, Ring2, Trinket1, Trinket2, MainHand, OffHand, Ranged",
                name
            )),
        }
    }

    /// Parse an item ID name string into ItemId
    fn parse_item_id(name: &str) -> Result<ItemId, String> {
        match name {
            // Plate Armor
            "LionheartHelm" => Ok(ItemId::LionheartHelm),
            "OnslaughtHeadGuard" => Ok(ItemId::OnslaughtHeadGuard),
            "ConquerorsChestplate" => Ok(ItemId::ConquerorsChestplate),
            "LegplatesOfWrath" => Ok(ItemId::LegplatesOfWrath),
            "GauntletsOfMight" => Ok(ItemId::GauntletsOfMight),
            "SabatonsBattleBorn" => Ok(ItemId::SabatonsBattleBorn),
            "WaistguardOfHeroism" => Ok(ItemId::WaistguardOfHeroism),
            "WristguardsOfStability" => Ok(ItemId::WristguardsOfStability),
            "ShoulderplatesOfValor" => Ok(ItemId::ShoulderplatesOfValor),
            // Mail Armor
            "BeaststalkerHelm" => Ok(ItemId::BeaststalkerHelm),
            "BeaststalkerTunic" => Ok(ItemId::BeaststalkerTunic),
            "BeaststalkerLegs" => Ok(ItemId::BeaststalkerLegs),
            "BeaststalkerGloves" => Ok(ItemId::BeaststalkerGloves),
            "BeaststalkerBoots" => Ok(ItemId::BeaststalkerBoots),
            "BeaststalkerBelt" => Ok(ItemId::BeaststalkerBelt),
            "BeaststalkerBracers" => Ok(ItemId::BeaststalkerBracers),
            "BeaststalkerMantle" => Ok(ItemId::BeaststalkerMantle),
            // Leather Armor
            "NightstalkerCowl" => Ok(ItemId::NightstalkerCowl),
            "NightstalkerTunic" => Ok(ItemId::NightstalkerTunic),
            "NightstalkerLegs" => Ok(ItemId::NightstalkerLegs),
            "NightstalkerGloves" => Ok(ItemId::NightstalkerGloves),
            "NightstalkerBoots" => Ok(ItemId::NightstalkerBoots),
            "NightstalkerBelt" => Ok(ItemId::NightstalkerBelt),
            "NightstalkerBracers" => Ok(ItemId::NightstalkerBracers),
            "NightstalkerMantle" => Ok(ItemId::NightstalkerMantle),
            // Cloth Armor
            "MagistersCrown" => Ok(ItemId::MagistersCrown),
            "MagistersRobes" => Ok(ItemId::MagistersRobes),
            "MagistersLeggings" => Ok(ItemId::MagistersLeggings),
            "MagistersGloves" => Ok(ItemId::MagistersGloves),
            "MagistersBoots" => Ok(ItemId::MagistersBoots),
            "MagistersBelt" => Ok(ItemId::MagistersBelt),
            "MagistersBracers" => Ok(ItemId::MagistersBracers),
            "MagistersMantle" => Ok(ItemId::MagistersMantle),
            // Cloaks
            "CloakOfTheShieldWall" => Ok(ItemId::CloakOfTheShieldWall),
            "CloakOfConcentration" => Ok(ItemId::CloakOfConcentration),
            // Necklaces
            "AmuletOfPower" => Ok(ItemId::AmuletOfPower),
            "AmuletOfResilience" => Ok(ItemId::AmuletOfResilience),
            // Rings
            "BandOfAccuria" => Ok(ItemId::BandOfAccuria),
            "SignetOfFocus" => Ok(ItemId::SignetOfFocus),
            "RingOfProtection" => Ok(ItemId::RingOfProtection),
            // Trinkets
            "MarkOfTheChampion" => Ok(ItemId::MarkOfTheChampion),
            "EssenceOfEternalLife" => Ok(ItemId::EssenceOfEternalLife),
            // Melee Weapons
            "ArcaniteReaper" => Ok(ItemId::ArcaniteReaper),
            "FrostbiteBlade" => Ok(ItemId::FrostbiteBlade),
            "SerpentFangDagger" => Ok(ItemId::SerpentFangDagger),
            "HammerOfTheRighteous" => Ok(ItemId::HammerOfTheRighteous),
            "CrescentStaff" => Ok(ItemId::CrescentStaff),
            // Ranged Weapons
            "WandOfShadows" => Ok(ItemId::WandOfShadows),
            "StaffOfDominance" => Ok(ItemId::StaffOfDominance),
            "AshwoodBow" => Ok(ItemId::AshwoodBow),
            "SniperScope" => Ok(ItemId::SniperScope),
            // Off Hand
            "TomeOfKnowledge" => Ok(ItemId::TomeOfKnowledge),
            "WallOfTheDeadShield" => Ok(ItemId::WallOfTheDeadShield),
            _ => Err(format!(
                "Unknown item: '{}'. Valid items: LionheartHelm, OnslaughtHeadGuard, ConquerorsChestplate, LegplatesOfWrath, GauntletsOfMight, SabatonsBattleBorn, WaistguardOfHeroism, WristguardsOfStability, ShoulderplatesOfValor, BeaststalkerHelm, BeaststalkerTunic, BeaststalkerLegs, BeaststalkerGloves, BeaststalkerBoots, BeaststalkerBelt, BeaststalkerBracers, BeaststalkerMantle, NightstalkerCowl, NightstalkerTunic, NightstalkerLegs, NightstalkerGloves, NightstalkerBoots, NightstalkerBelt, NightstalkerBracers, NightstalkerMantle, MagistersCrown, MagistersRobes, MagistersLeggings, MagistersGloves, MagistersBoots, MagistersBelt, MagistersBracers, MagistersMantle, CloakOfTheShieldWall, CloakOfConcentration, AmuletOfPower, AmuletOfResilience, BandOfAccuria, SignetOfFocus, RingOfProtection, MarkOfTheChampion, EssenceOfEternalLife, ArcaniteReaper, FrostbiteBlade, SerpentFangDagger, HammerOfTheRighteous, CrescentStaff, WandOfShadows, StaffOfDominance, AshwoodBow, SniperScope, TomeOfKnowledge, WallOfTheDeadShield",
                name
            )),
        }
    }

    /// Parse a string-keyed equipment map into typed ItemSlot/ItemId map
    fn parse_equipment_map(map: &HashMap<String, String>) -> Result<HashMap<ItemSlot, ItemId>, String> {
        let mut result = HashMap::new();
        for (slot_str, item_str) in map {
            let slot = Self::parse_item_slot(slot_str)?;
            let item = Self::parse_item_id(item_str)?;
            result.insert(slot, item);
        }
        Ok(result)
    }

    /// Parse equipment overrides from JSON, resizing to team size with empty defaults
    fn parse_equipment_overrides(
        raw: &[HashMap<String, String>],
        team_size: usize,
    ) -> Result<Vec<HashMap<ItemSlot, ItemId>>, String> {
        let mut result = Vec::with_capacity(team_size);
        for (i, map) in raw.iter().enumerate() {
            if i >= team_size {
                break;
            }
            result.push(Self::parse_equipment_map(map)?);
        }
        // Pad remaining slots with empty maps
        result.resize(team_size, HashMap::new());
        Ok(result)
    }

    /// Parse warlock curse preferences from JSON format
    /// Outer vec indexed by slot, inner vec indexed by enemy target
    fn parse_warlock_curse_prefs(
        prefs: &[Option<Vec<String>>],
        team_size: usize,
        enemy_size: usize,
    ) -> Vec<Vec<WarlockCurse>> {
        let mut result = Vec::with_capacity(team_size);
        for slot in 0..team_size {
            let slot_prefs = prefs
                .get(slot)
                .and_then(|opt| opt.as_ref())
                .map(|curses| {
                    let mut parsed: Vec<WarlockCurse> = curses
                        .iter()
                        .map(|s| Self::parse_warlock_curse(s))
                        .collect();
                    parsed.resize(enemy_size, WarlockCurse::default());
                    parsed
                })
                .unwrap_or_else(|| vec![WarlockCurse::default(); enemy_size]);
            result.push(slot_prefs);
        }
        result
    }

    /// Convert to the game's MatchConfig format
    pub fn to_match_config(&self) -> Result<MatchConfig, String> {
        let team1: Vec<Option<CharacterClass>> = self
            .team1
            .iter()
            .map(|s| Self::parse_class(s).map(Some))
            .collect::<Result<Vec<_>, _>>()?;

        let team2: Vec<Option<CharacterClass>> = self
            .team2
            .iter()
            .map(|s| Self::parse_class(s).map(Some))
            .collect::<Result<Vec<_>, _>>()?;

        let map = Self::parse_map(&self.map)?;

        // Parse rogue openers, defaulting to Ambush for missing entries
        let mut team1_rogue_openers: Vec<RogueOpener> = self
            .team1_rogue_openers
            .iter()
            .map(|s| Self::parse_rogue_opener(s))
            .collect();
        team1_rogue_openers.resize(team1.len(), RogueOpener::default());

        let mut team2_rogue_openers: Vec<RogueOpener> = self
            .team2_rogue_openers
            .iter()
            .map(|s| Self::parse_rogue_opener(s))
            .collect();
        team2_rogue_openers.resize(team2.len(), RogueOpener::default());

        // Parse warlock curse preferences
        let team1_warlock_curse_prefs = Self::parse_warlock_curse_prefs(
            &self.team1_warlock_curse_prefs,
            team1.len(),
            team2.len(),
        );
        let team2_warlock_curse_prefs = Self::parse_warlock_curse_prefs(
            &self.team2_warlock_curse_prefs,
            team2.len(),
            team1.len(),
        );

        // Parse hunter pet types, defaulting to Spider for missing entries
        let mut team1_hunter_pet_types: Vec<HunterPetType> = self
            .team1_hunter_pet_types
            .iter()
            .map(|s| Self::parse_hunter_pet_type(s))
            .collect();
        team1_hunter_pet_types.resize(team1.len(), HunterPetType::default());

        let mut team2_hunter_pet_types: Vec<HunterPetType> = self
            .team2_hunter_pet_types
            .iter()
            .map(|s| Self::parse_hunter_pet_type(s))
            .collect();
        team2_hunter_pet_types.resize(team2.len(), HunterPetType::default());

        // Parse equipment overrides, defaulting to empty maps for missing entries
        let team1_equipment = Self::parse_equipment_overrides(&self.team1_equipment, team1.len())?;
        let team2_equipment = Self::parse_equipment_overrides(&self.team2_equipment, team2.len())?;

        Ok(MatchConfig {
            team1_size: team1.len(),
            team2_size: team2.len(),
            team1,
            team2,
            map,
            team1_kill_target: self.team1_kill_target,
            team2_kill_target: self.team2_kill_target,
            team1_cc_target: self.team1_cc_target,
            team2_cc_target: self.team2_cc_target,
            team1_rogue_openers,
            team2_rogue_openers,
            team1_warlock_curse_prefs,
            team2_warlock_curse_prefs,
            team1_hunter_pet_types,
            team2_hunter_pet_types,
            team1_equipment,
            team2_equipment,
        })
    }
}
