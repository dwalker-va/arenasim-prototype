//! Shared Utility Functions
//!
//! This module contains utility functions used by multiple combat modules.
//! Having them here breaks circular dependencies between combat_ai and combat_core.

use bevy::prelude::*;
use crate::combat::log::{CombatLog, CombatantId};
use super::match_config::{self, CharacterClass};
use super::components::{FloatingTextState, SpeechBubble, PlayMatchEntity, PetType};

/// Floating combat text horizontal spread (multiplied by -0.5 to +0.5 range)
/// Adjust this to control how far left/right numbers can appear from their spawn point
pub const FCT_HORIZONTAL_SPREAD: f32 = 1.2;

/// Floating combat text vertical spread (0.0 to this value)
/// Adjust this to control the vertical stagger of numbers
pub const FCT_VERTICAL_SPREAD: f32 = 0.8;

/// Helper to generate a consistent, per-entity-unique combatant ID for the
/// combat log.
///
/// Format: `"Team {team} {class} #{slot+1}"` e.g. `"Team 1 Warrior #1"`. The
/// 1-based slot suffix (matching the `Slot N` numbering in the saved match
/// report) disambiguates same-class teammates, which would otherwise share an
/// identity and have their damage/healing/CC/kills silently merged in every
/// `CombatLog` aggregation and on the Results screen.
pub fn combatant_id(team: u8, slot: u8, class: match_config::CharacterClass) -> CombatantId {
    combat_log_id(team, slot, class.name())
}

/// Base combat-log id builder from an already-resolved display name (class name
/// or pet type name). `combatant_id` / `pet_combatant_id` delegate here; call it
/// directly only where the name is already a string (e.g. the auto-attack
/// snapshot, which resolves owner-vs-pet naming once up front).
pub fn combat_log_id(team: u8, slot: u8, display_name: &str) -> CombatantId {
    format!("Team {} {} #{}", team, display_name, slot + 1)
}

/// Combat-log ID for a pet, keyed to its OWNER's slot so it lines up with the
/// owner's id (e.g. `"Team 1 Spider #2"` belongs to `"Team 1 Hunter #2"`).
///
/// `owner_slot` is the owner's team slot (0-based). A pet's own `Combatant.slot`
/// is `PET_SLOT_BASE + owner_slot`, so callers holding the pet's combatant pass
/// `pet.slot - PET_SLOT_BASE`.
///
/// KNOWN LIMITATION: sites that log a pet as a *target* (AoE completion —
/// Frost Nova, Psychic Scream, Frost Shock) still build the target id from the
/// pet's raw `Combatant` via [`combatant_id`], where `class` is the owner's
/// class and `slot` is the un-adjusted `PET_SLOT_BASE + owner_slot`. That id
/// (`"Team 2 Warlock #11"`) matches neither the pet's registered id nor the
/// owner's, so CC/damage dealt *to* an enemy pet is orphaned from the
/// structured log. It corrupts no displayed number today (pets have no Results
/// row; the TKN column reads a live counter, not the log; and a primary's
/// CC-received is now *more* accurate for excluding pet CC). Fixing it needs a
/// pet-aware target-id path (owner slot + `pet_type`) threaded into the
/// borrow-sensitive casting/projectile systems — deferred as a separate change.
pub fn pet_combatant_id(team: u8, owner_slot: u8, pet_type: PetType) -> CombatantId {
    combat_log_id(team, owner_slot, pet_type.name())
}

/// Helper to log an ability cast with consistent formatting.
///
/// Builds caster/target IDs from team + class, formats the message, and delegates
/// to `CombatLog::log_ability_cast()`.
///
/// `verb` should match the action: `"casts"` for spells, `"uses"` for instants,
/// `"begins casting"` / `"begins channeling"` for cast-start logs, or any custom verb.
/// `target` is `None` for self-buffs and untargeted abilities.
pub fn log_ability_use(
    combat_log: &mut CombatLog,
    caster_team: u8,
    caster_slot: u8,
    caster_class: CharacterClass,
    ability_name: &str,
    target: Option<(u8, u8, CharacterClass)>,
    verb: &str,
) {
    let caster_id = combatant_id(caster_team, caster_slot, caster_class);
    let target_id = target.map(|(team, slot, class)| combatant_id(team, slot, class));
    let message = match &target_id {
        Some(tid) => format!("{} {} {} on {}", caster_id, verb, ability_name, tid),
        None => format!("{} {} {}", caster_id, verb, ability_name),
    };
    combat_log.log_ability_cast(caster_id, ability_name.to_string(), target_id, message);
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

/// Whether an aura type is an incapacitating CC (prevents all actions).
/// Root does NOT count — it only prevents movement.
pub fn is_incapacitating(aura_type: &super::components::AuraType) -> bool {
    matches!(
        aura_type,
        super::components::AuraType::Stun
            | super::components::AuraType::Fear
            | super::components::AuraType::Polymorph
            | super::components::AuraType::Incapacitate
    )
}

/// Check if a combatant is incapacitated by crowd control (Stun, Fear, or Polymorph).
/// Root does NOT count as incapacitation — it only prevents movement.
pub fn is_incapacitated(auras: Option<&super::components::ActiveAuras>) -> bool {
    auras.map_or(false, |a| {
        a.auras.iter().any(|aura| is_incapacitating(&aura.effect_type))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combatant_id_format() {
        let id = combatant_id(1, 0, match_config::CharacterClass::Warrior);
        assert_eq!(id, "Team 1 Warrior #1");

        let id2 = combatant_id(2, 2, match_config::CharacterClass::Mage);
        assert_eq!(id2, "Team 2 Mage #3");
    }

    #[test]
    fn test_pet_combatant_id_matches_owner_slot() {
        // A pet's suffix uses the OWNER's slot, so it lines up with the owner id.
        let owner = combatant_id(1, 1, match_config::CharacterClass::Hunter);
        let pet = pet_combatant_id(1, 1, PetType::Spider);
        assert_eq!(owner, "Team 1 Hunter #2");
        assert_eq!(pet, "Team 1 Spider #2");
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
