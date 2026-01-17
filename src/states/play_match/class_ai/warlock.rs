//! Warlock AI Module
//!
//! Handles AI decision-making for the Warlock class.
//!
//! ## Priority Order
//! 1. Corruption (DoT on enemies without it)
//! 2. Fear (CC on non-main target)
//! 3. Shadow Bolt (main damage spell)

use super::{AbilityDecision, ClassAI, CombatContext};
use crate::states::play_match::components::Combatant;

/// Warlock AI implementation
pub struct WarlockAI;

impl ClassAI for WarlockAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate warlock AI logic from combat_ai.rs
        // For now, decisions are still handled by the legacy code
        AbilityDecision::None
    }
}
