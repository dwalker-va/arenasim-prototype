//! Match configuration data structures and resource
//!
//! This module defines the data that persists between the Configure Match
//! and Play Match states.

use bevy::prelude::*;

/// Available character classes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharacterClass {
    Warrior,
    Mage,
    Rogue,
    Priest,
    Warlock,
}

impl CharacterClass {
    /// Get all available character classes
    pub fn all() -> &'static [CharacterClass] {
        &[
            CharacterClass::Warrior,
            CharacterClass::Mage,
            CharacterClass::Rogue,
            CharacterClass::Priest,
            CharacterClass::Warlock,
        ]
    }

    /// Get the display name
    pub fn name(&self) -> &'static str {
        match self {
            CharacterClass::Warrior => "Warrior",
            CharacterClass::Mage => "Mage",
            CharacterClass::Rogue => "Rogue",
            CharacterClass::Priest => "Priest",
            CharacterClass::Warlock => "Warlock",
        }
    }

    /// Get a short description
    pub fn description(&self) -> &'static str {
        match self {
            CharacterClass::Warrior => "Sturdy melee fighter",
            CharacterClass::Mage => "Powerful spellcaster",
            CharacterClass::Rogue => "Swift shadow striker",
            CharacterClass::Priest => "Healer and support",
            CharacterClass::Warlock => "Shadow magic and curses",
        }
    }

    /// Get the class color for UI
    pub fn color(&self) -> Color {
        match self {
            CharacterClass::Warrior => Color::srgb(0.78, 0.61, 0.43), // Brown/tan
            CharacterClass::Mage => Color::srgb(0.41, 0.80, 0.94),    // Light blue
            CharacterClass::Rogue => Color::srgb(1.0, 0.96, 0.41),    // Yellow
            CharacterClass::Priest => Color::srgb(1.0, 1.0, 1.0),     // White
            CharacterClass::Warlock => Color::srgb(0.58, 0.51, 0.79), // Purple
        }
    }

    /// Get the preferred combat range for this class.
    /// This is the optimal distance to maintain - close enough for all important
    /// abilities without putting themselves in unnecessary danger.
    pub fn preferred_range(&self) -> f32 {
        match self {
            // Melee classes want to be in melee range
            CharacterClass::Warrior => 2.0,
            CharacterClass::Rogue => 2.0,
            // Mage stays at max range (squishy, relies on kiting)
            // Frostbolt: 40, but stay slightly back for safety
            CharacterClass::Mage => 38.0,
            // Priest and Warlock position for their shortest-range abilities
            // Priest: Wand 30, so stay at ~28 to use everything
            CharacterClass::Priest => 28.0,
            // Warlock: Fear 30, Shadowbolt 40, Corruption 40, Wand 30
            // Stay at ~28 to cast Fear without repositioning
            CharacterClass::Warlock => 28.0,
        }
    }
}

/// Available arena maps
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ArenaMap {
    #[default]
    BasicArena,
    PillaredArena,
}

impl ArenaMap {
    /// Get all available maps
    pub fn all() -> &'static [ArenaMap] {
        &[ArenaMap::BasicArena, ArenaMap::PillaredArena]
    }

    /// Get the display name
    pub fn name(&self) -> &'static str {
        match self {
            ArenaMap::BasicArena => "Basic Arena",
            ArenaMap::PillaredArena => "Pillared Arena",
        }
    }

    /// Get a short description
    pub fn description(&self) -> &'static str {
        match self {
            ArenaMap::BasicArena => "Simple rectangular arena",
            ArenaMap::PillaredArena => "Arena with pillars for cover",
        }
    }
}

/// The match configuration resource
#[derive(Resource, Debug, Clone)]
pub struct MatchConfig {
    /// Team 1 size (1-3)
    pub team1_size: usize,
    /// Team 2 size (1-3)
    pub team2_size: usize,
    /// Characters assigned to Team 1 slots
    pub team1: Vec<Option<CharacterClass>>,
    /// Characters assigned to Team 2 slots
    pub team2: Vec<Option<CharacterClass>>,
    /// Selected map
    pub map: ArenaMap,
    /// Team 1's kill target priority (index into enemy team, None = no priority)
    pub team1_kill_target: Option<usize>,
    /// Team 2's kill target priority (index into enemy team, None = no priority)
    pub team2_kill_target: Option<usize>,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            team1_size: 1,
            team2_size: 1,
            team1: vec![None],
            team2: vec![None],
            map: ArenaMap::BasicArena,
            team1_kill_target: None, // No priority by default
            team2_kill_target: None, // No priority by default
        }
    }
}

impl MatchConfig {
    /// Set team 1 size, adjusting the slots vector
    pub fn set_team1_size(&mut self, size: usize) {
        let size = size.clamp(1, 3);
        self.team1_size = size;
        self.team1.resize(size, None);
    }

    /// Set team 2 size, adjusting the slots vector
    pub fn set_team2_size(&mut self, size: usize) {
        let size = size.clamp(1, 3);
        self.team2_size = size;
        self.team2.resize(size, None);
    }

    /// Check if the match configuration is valid (all slots filled)
    pub fn is_valid(&self) -> bool {
        self.team1.iter().all(|slot| slot.is_some())
            && self.team2.iter().all(|slot| slot.is_some())
    }
}

