//! Mage AI Module
//!
//! Handles AI decision-making for the Mage class.
//!
//! ## Priority Order
//! 1. Ice Barrier (self-shield when no shield or HP < 80%)
//! 2. Arcane Intellect (buff mana-using allies pre-combat)
//! 3. Frost Nova (defensive AoE when enemies in melee)
//! 4. Frostbolt (main damage spell with kiting behavior)

use super::{AbilityDecision, ClassAI, CombatContext};
use crate::states::play_match::components::Combatant;

/// Mage AI implementation
pub struct MageAI;

impl ClassAI for MageAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate mage AI logic from combat_ai.rs
        // For now, decisions are still handled by the legacy code
        AbilityDecision::None
    }
}
