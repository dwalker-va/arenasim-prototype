//! Rogue AI Module
//!
//! Handles AI decision-making for the Rogue class.
//!
//! ## Priority Order (Stealthed)
//! 1. Ambush (opener from stealth)
//!
//! ## Priority Order (In Combat)
//! 1. Kick (interrupt enemy casts)
//! 2. Kidney Shot (stun at 5 combo points)
//! 3. Eviscerate (finisher at 5 combo points)
//! 4. Sinister Strike (combo point builder)

use super::{AbilityDecision, ClassAI, CombatContext};
use crate::states::play_match::components::Combatant;

/// Rogue AI implementation
pub struct RogueAI;

impl ClassAI for RogueAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate rogue AI logic from combat_ai.rs
        // For now, decisions are still handled by the legacy code
        AbilityDecision::None
    }
}
