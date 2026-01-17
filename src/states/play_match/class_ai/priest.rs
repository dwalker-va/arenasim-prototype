//! Priest AI Module
//!
//! Handles AI decision-making for the Priest class.
//!
//! ## Priority Order
//! 1. Power Word: Fortitude (buff all allies pre-combat)
//! 2. Power Word: Shield (shield low-health allies)
//! 3. Flash Heal (heal injured allies)
//! 4. Mind Blast (damage when allies are healthy)
//! 5. Shadow Word: Pain (DoT on enemies)

use super::{AbilityDecision, ClassAI, CombatContext};
use crate::states::play_match::components::Combatant;

/// Priest AI implementation
pub struct PriestAI;

impl ClassAI for PriestAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate priest AI logic from combat_ai.rs
        // For now, decisions are still handled by the legacy code
        AbilityDecision::None
    }
}
