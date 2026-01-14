//! JSON configuration parsing for headless mode
//!
//! Parses JSON match configurations and converts them to the game's MatchConfig format.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::states::match_config::{ArenaMap, CharacterClass, MatchConfig};

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
    /// Custom output path for match log (optional)
    #[serde(default)]
    pub output_path: Option<String>,
    /// Maximum match duration in seconds (default: 300)
    #[serde(default = "default_max_duration")]
    pub max_duration_secs: f32,
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
            _ => Err(format!(
                "Unknown class: '{}'. Valid classes: Warrior, Mage, Rogue, Priest, Warlock",
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

    /// Convert to the game's MatchConfig format
    pub fn to_match_config(&self) -> Result<MatchConfig, String> {
        let team1: Vec<Option<CharacterClass>> = self
            .team1
            .iter()
            .map(|s| Self::parse_class(s).ok())
            .collect();

        let team2: Vec<Option<CharacterClass>> = self
            .team2
            .iter()
            .map(|s| Self::parse_class(s).ok())
            .collect();

        let map = Self::parse_map(&self.map)?;

        Ok(MatchConfig {
            team1_size: team1.len(),
            team2_size: team2.len(),
            team1,
            team2,
            map,
            team1_kill_target: self.team1_kill_target,
            team2_kill_target: self.team2_kill_target,
        })
    }
}
