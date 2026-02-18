//! Match configuration data structures and resource
//!
//! This module defines the data that persists between the Configure Match
//! and Play Match states.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Rogue stealth opener choice
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RogueOpener {
    /// High damage opener from stealth
    #[default]
    Ambush,
    /// 4 second stun opener from stealth
    CheapShot,
}

impl RogueOpener {
    /// Get the display name
    pub fn name(&self) -> &'static str {
        match self {
            RogueOpener::Ambush => "Ambush",
            RogueOpener::CheapShot => "Cheap Shot",
        }
    }

    /// Get a short description
    pub fn description(&self) -> &'static str {
        match self {
            RogueOpener::Ambush => "High damage opener",
            RogueOpener::CheapShot => "4 sec stun opener",
        }
    }
}

/// Warlock curse preference for a specific enemy target
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum WarlockCurse {
    /// Curse of Agony - DoT: 84 Shadow damage over 24 seconds
    #[default]
    Agony,
    /// Curse of Weakness - reduces target's damage dealt by 3 for 2 minutes
    Weakness,
    /// Curse of Tongues - increases target's cast time by 50% for 30 seconds
    Tongues,
}

impl WarlockCurse {
    /// Get a short description for UI display
    pub fn description(&self) -> &'static str {
        match self {
            WarlockCurse::Agony => "84 damage over 24s",
            WarlockCurse::Weakness => "-20% physical damage",
            WarlockCurse::Tongues => "+50% cast time",
        }
    }
}

/// Available character classes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharacterClass {
    Warrior,
    Mage,
    Rogue,
    Priest,
    Warlock,
    Paladin,
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
            CharacterClass::Paladin,
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
            CharacterClass::Paladin => "Paladin",
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
            CharacterClass::Paladin => "Holy warrior and healer",
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
            CharacterClass::Paladin => Color::srgb(0.96, 0.55, 0.73), // Pink (WoW Paladin color)
        }
    }

    /// Whether this class attacks in melee range (vs. ranged/wand).
    pub fn is_melee(&self) -> bool {
        matches!(self, CharacterClass::Warrior | CharacterClass::Rogue | CharacterClass::Paladin)
    }

    /// Whether this class is primarily a healer (for CC target prioritization).
    pub fn is_healer(&self) -> bool {
        matches!(self, CharacterClass::Priest | CharacterClass::Paladin)
    }

    /// Whether this class uses mana as its resource.
    pub fn uses_mana(&self) -> bool {
        matches!(
            self,
            CharacterClass::Mage | CharacterClass::Priest | CharacterClass::Warlock | CharacterClass::Paladin
        )
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
            // Paladin: Holy warrior — melee positioning for auto-attacks + Hammer of Justice
            // All heals are 40yd range, so melee positioning doesn't limit healing
            CharacterClass::Paladin => 2.0,
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
    /// Team 1's CC target priority (index into enemy team, None = use heuristics)
    pub team1_cc_target: Option<usize>,
    /// Team 2's CC target priority (index into enemy team, None = use heuristics)
    pub team2_cc_target: Option<usize>,
    /// Team 1's rogue opener preferences (one per slot, defaults to Ambush)
    pub team1_rogue_openers: Vec<RogueOpener>,
    /// Team 2's rogue opener preferences (one per slot, defaults to Ambush)
    pub team2_rogue_openers: Vec<RogueOpener>,
    /// Team 1's warlock curse preferences: [warlock_slot][enemy_target_index] -> curse
    /// Outer vec indexed by team slot, inner vec indexed by enemy target slot
    pub team1_warlock_curse_prefs: Vec<Vec<WarlockCurse>>,
    /// Team 2's warlock curse preferences: [warlock_slot][enemy_target_index] -> curse
    pub team2_warlock_curse_prefs: Vec<Vec<WarlockCurse>>,
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
            team1_cc_target: None,   // Use heuristics by default
            team2_cc_target: None,   // Use heuristics by default
            team1_rogue_openers: vec![RogueOpener::default()],
            team2_rogue_openers: vec![RogueOpener::default()],
            // One vec per slot, inner vec has curse pref per enemy (defaults to Agony)
            team1_warlock_curse_prefs: vec![vec![WarlockCurse::default()]],
            team2_warlock_curse_prefs: vec![vec![WarlockCurse::default()]],
        }
    }
}

impl MatchConfig {
    /// Set team 1 size, adjusting the slots vector
    pub fn set_team1_size(&mut self, size: usize) {
        let size = size.clamp(1, 3);
        self.team1_size = size;
        self.team1.resize(size, None);
        self.team1_rogue_openers.resize(size, RogueOpener::default());
        // Resize curse prefs: one inner vec per slot, each sized to enemy team
        let enemy_size = self.team2_size;
        self.team1_warlock_curse_prefs.resize(size, vec![WarlockCurse::default(); enemy_size]);
        for prefs in &mut self.team1_warlock_curse_prefs {
            prefs.resize(enemy_size, WarlockCurse::default());
        }
    }

    /// Set team 2 size, adjusting the slots vector
    pub fn set_team2_size(&mut self, size: usize) {
        let size = size.clamp(1, 3);
        self.team2_size = size;
        self.team2.resize(size, None);
        self.team2_rogue_openers.resize(size, RogueOpener::default());
        // Resize curse prefs: one inner vec per slot, each sized to enemy team
        let enemy_size = self.team1_size;
        self.team2_warlock_curse_prefs.resize(size, vec![WarlockCurse::default(); enemy_size]);
        for prefs in &mut self.team2_warlock_curse_prefs {
            prefs.resize(enemy_size, WarlockCurse::default());
        }
        // Also resize team1's curse prefs inner vecs to match new enemy count
        for prefs in &mut self.team1_warlock_curse_prefs {
            prefs.resize(size, WarlockCurse::default());
        }
    }

    /// Check if the match configuration is valid (all slots filled)
    pub fn is_valid(&self) -> bool {
        self.team1.iter().all(|slot| slot.is_some())
            && self.team2.iter().all(|slot| slot.is_some())
    }

    /// Get the curse preference for a specific warlock slot and enemy target
    pub fn get_curse_pref(&self, team: u8, slot: usize, enemy_target: usize) -> WarlockCurse {
        let prefs = if team == 1 {
            &self.team1_warlock_curse_prefs
        } else {
            &self.team2_warlock_curse_prefs
        };
        prefs
            .get(slot)
            .and_then(|slot_prefs| slot_prefs.get(enemy_target))
            .copied()
            .unwrap_or_default()
    }

    /// Set the curse preference for a specific warlock slot and enemy target
    pub fn set_curse_pref(&mut self, team: u8, slot: usize, enemy_target: usize, curse: WarlockCurse) {
        let prefs = if team == 1 {
            &mut self.team1_warlock_curse_prefs
        } else {
            &mut self.team2_warlock_curse_prefs
        };
        // Ensure the outer vec is large enough
        if prefs.len() <= slot {
            let enemy_size = if team == 1 { self.team2_size } else { self.team1_size };
            prefs.resize(slot + 1, vec![WarlockCurse::default(); enemy_size]);
        }
        // Ensure the inner vec is large enough
        if prefs[slot].len() <= enemy_target {
            prefs[slot].resize(enemy_target + 1, WarlockCurse::default());
        }
        prefs[slot][enemy_target] = curse;
    }
}

