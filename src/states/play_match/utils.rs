//! Shared Utility Functions
//!
//! This module contains utility functions used by multiple combat modules.
//! Having them here breaks circular dependencies between combat_ai and combat_core.

use bevy::prelude::*;
use crate::combat::log::CombatantId;
use super::match_config;
use super::components::{FloatingTextState, SpeechBubble, PlayMatchEntity};

/// Floating combat text horizontal spread (multiplied by -0.5 to +0.5 range)
/// Adjust this to control how far left/right numbers can appear from their spawn point
pub const FCT_HORIZONTAL_SPREAD: f32 = 1.2;

/// Floating combat text vertical spread (0.0 to this value)
/// Adjust this to control the vertical stagger of numbers
pub const FCT_VERTICAL_SPREAD: f32 = 0.8;

/// Helper to generate a consistent combatant ID for the combat log.
///
/// Format: "Team {team} {class}" e.g., "Team 1 Warrior"
pub fn combatant_id(team: u8, class: match_config::CharacterClass) -> CombatantId {
    format!("Team {} {}", team, class.name())
}

/// Helper function to spawn a speech bubble when a combatant uses an ability.
///
/// The speech bubble displays the ability name and fades out after 2 seconds.
pub fn spawn_speech_bubble(commands: &mut Commands, owner: Entity, ability_name: &str) {
    commands.spawn((
        SpeechBubble {
            owner,
            text: format!("{}!", ability_name),
            lifetime: 2.0, // 2 seconds
        },
        PlayMatchEntity,
    ));
}

/// Helper function to get next floating combat text offset and update pattern state.
///
/// Returns (x_offset, y_offset) based on deterministic alternating pattern.
/// This ensures multiple simultaneous FCT numbers don't overlap.
pub fn get_next_fct_offset(state: &mut FloatingTextState) -> (f32, f32) {
    let (x_offset, y_offset) = match state.next_pattern_index {
        0 => (0.0, 0.0),                                                    // Center
        1 => (FCT_HORIZONTAL_SPREAD * 0.4, FCT_VERTICAL_SPREAD * 0.3),      // Right side, slight up
        2 => (FCT_HORIZONTAL_SPREAD * -0.4, FCT_VERTICAL_SPREAD * 0.6),     // Left side, more up
        _ => (0.0, 0.0),                                                    // Fallback to center
    };

    // Cycle to next pattern: 0 -> 1 -> 2 -> 0
    state.next_pattern_index = (state.next_pattern_index + 1) % 3;

    (x_offset, y_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combatant_id_format() {
        let id = combatant_id(1, match_config::CharacterClass::Warrior);
        assert_eq!(id, "Team 1 Warrior");

        let id2 = combatant_id(2, match_config::CharacterClass::Mage);
        assert_eq!(id2, "Team 2 Mage");
    }

    #[test]
    fn test_fct_offset_pattern_cycles() {
        let mut state = FloatingTextState { next_pattern_index: 0 };

        // First call: center
        let (x, y) = get_next_fct_offset(&mut state);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
        assert_eq!(state.next_pattern_index, 1);

        // Second call: right side
        let (x, _y) = get_next_fct_offset(&mut state);
        assert!(x > 0.0); // Right side has positive x
        assert_eq!(state.next_pattern_index, 2);

        // Third call: left side
        let (x, _y) = get_next_fct_offset(&mut state);
        assert!(x < 0.0); // Left side has negative x
        assert_eq!(state.next_pattern_index, 0);

        // Fourth call: back to center
        let (x, y) = get_next_fct_offset(&mut state);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }
}
