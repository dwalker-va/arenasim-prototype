//! Hunter AI — Ranged physical DPS with pet, traps, and dead zone management.
//!
//! The Hunter prioritizes maintaining distance and controlling space over raw damage.
//! Key mechanics: dead zone (can't use ranged abilities within 8 yards), kiting,
//! trap placement, and pet coordination.

use super::{AbilityDecision, ClassAI, CombatContext};
use super::super::components::Combatant;

pub struct HunterAI;

impl ClassAI for HunterAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // Stub — full AI logic will be implemented in Phase 3
        AbilityDecision::None
    }
}
