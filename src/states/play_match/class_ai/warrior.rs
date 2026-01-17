//! Warrior AI Module
//!
//! Handles AI decision-making for the Warrior class.
//!
//! ## Priority Order
//! 1. Battle Shout (buff self and allies pre-combat)
//! 2. Charge (gap closer when out of melee range)
//! 3. Pummel (interrupt enemy casts)
//! 4. Mortal Strike (main damage, healing reduction)
//! 5. Rend (bleed DoT)

use super::{AbilityDecision, ClassAI, CombatContext};
use crate::states::play_match::components::Combatant;

/// Warrior AI implementation
pub struct WarriorAI;

impl ClassAI for WarriorAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate warrior AI logic from combat_ai.rs
        // For now, decisions are still handled by the legacy code
        AbilityDecision::None
    }
}
