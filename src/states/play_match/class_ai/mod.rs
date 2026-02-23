//! Class-Specific AI Modules
//!
//! This module contains the AI decision logic for each character class.
//! Each class has its own module that implements the `ClassAI` trait.
//!
//! ## Architecture
//!
//! The combat AI works in two phases:
//! 1. **Context Building**: `CombatContext` collects all game state needed for decisions
//! 2. **Decision Making**: Each class's `decide_action()` returns an `AbilityDecision`
//!
//! This separation allows:
//! - Each class AI to be tested in isolation
//! - New classes to be added without modifying other files
//! - Clear boundaries between shared state and class-specific logic

pub mod mage;
pub mod priest;
pub mod warrior;
pub mod rogue;
pub mod warlock;
pub mod paladin;
pub mod hunter;
pub mod pet_ai;

use bevy::prelude::*;
use std::collections::HashMap;

use super::match_config::CharacterClass;
use super::abilities::AbilityType;
use super::components::{Aura, Combatant, AuraType, PetType, DRCategory, DRTracker};

/// Per-frame snapshot of a single combatant, used for AI decision making.
#[derive(Clone, Copy, Debug)]
pub struct CombatantInfo {
    pub entity: Entity,
    pub team: u8,
    pub slot: u8,
    pub class: CharacterClass,
    pub current_health: f32,
    pub max_health: f32,
    pub current_mana: f32,
    pub max_mana: f32,
    /// Per-frame snapshot from Transform.
    pub position: Vec3,
    pub is_alive: bool,
    pub stealthed: bool,
    pub target: Option<Entity>,
    pub is_pet: bool,
    pub pet_type: Option<PetType>,
}

/// Deferred instant melee attack (Mortal Strike, Ambush, Sinister Strike, etc.)
#[derive(Clone, Copy)]
pub struct QueuedInstantAttack {
    pub attacker: Entity,
    pub target: Entity,
    pub damage: f32,
    pub attacker_team: u8,
    pub attacker_class: CharacterClass,
    pub ability: AbilityType,
    pub is_crit: bool,
}

/// Deferred AoE damage (Frost Nova).
#[derive(Clone, Copy)]
pub struct QueuedAoeDamage {
    pub caster: Entity,
    pub target: Entity,
    pub damage: f32,
    pub caster_team: u8,
    pub caster_class: CharacterClass,
    pub target_pos: Vec3,
    pub is_crit: bool,
}

impl CombatantInfo {
    /// Health as a percentage (0.0 to 1.0)
    pub fn health_pct(&self) -> f32 {
        if self.max_health > 0.0 {
            self.current_health / self.max_health
        } else {
            0.0
        }
    }

    /// Mana as a percentage (0.0 to 1.0)
    pub fn mana_pct(&self) -> f32 {
        if self.max_mana > 0.0 {
            self.current_mana / self.max_mana
        } else {
            0.0
        }
    }

    /// Distance to another position
    pub fn distance_to(&self, other_pos: Vec3) -> f32 {
        self.position.distance(other_pos)
    }
}

/// Shared context for AI decision making.
///
/// This struct provides a read-only view of the game state that AI modules
/// can use to make decisions without directly accessing ECS queries.
pub struct CombatContext<'a> {
    /// Map of entity to combatant info (per-frame snapshot)
    pub combatants: &'a HashMap<Entity, CombatantInfo>,
    /// Map of entity to their active auras
    pub active_auras: &'a HashMap<Entity, Vec<Aura>>,
    /// Map of entity to their DR tracker (for immunity queries)
    pub dr_trackers: &'a HashMap<Entity, DRTracker>,
    /// The combatant making the decision
    pub self_entity: Entity,
}

impl<'a> CombatContext<'a> {
    /// Get info about self
    pub fn self_info(&self) -> Option<&CombatantInfo> {
        self.combatants.get(&self.self_entity)
    }

    /// Get info about target (if any)
    pub fn target_info(&self) -> Option<&CombatantInfo> {
        self.self_info()
            .and_then(|info| info.target)
            .and_then(|target| self.combatants.get(&target))
    }

    /// Get auras on self
    pub fn self_auras(&self) -> Option<&Vec<Aura>> {
        self.active_auras.get(&self.self_entity)
    }

    /// Get auras on target
    pub fn target_auras(&self) -> Option<&Vec<Aura>> {
        self.target_info()
            .and_then(|info| self.active_auras.get(&info.entity))
    }

    /// Check if self has a specific aura type
    pub fn has_aura(&self, aura_type: AuraType) -> bool {
        self.self_auras()
            .map(|auras| auras.iter().any(|a| a.effect_type == aura_type))
            .unwrap_or(false)
    }

    /// Check if target has a specific aura type
    pub fn target_has_aura(&self, aura_type: AuraType) -> bool {
        self.target_auras()
            .map(|auras| auras.iter().any(|a| a.effect_type == aura_type))
            .unwrap_or(false)
    }

    /// Check if self is incapacitated (stunned, feared, or polymorphed).
    /// NOTE: The canonical CC type list lives in `utils::is_incapacitated`.
    /// CombatContext can't delegate because it stores auras as `&[Aura]`, not `&ActiveAuras`.
    pub fn is_incapacitated(&self) -> bool {
        self.has_aura(AuraType::Stun) || self.has_aura(AuraType::Fear) || self.has_aura(AuraType::Polymorph) || self.has_aura(AuraType::Incapacitate)
    }

    /// Check if an entity is currently CC'd (Stun, Fear, Root, or Polymorph).
    /// Useful for preventing CC overlap on targets.
    pub fn is_ccd(&self, entity: Entity) -> bool {
        self.active_auras
            .get(&entity)
            .map(|auras| {
                auras.iter().any(|a| {
                    matches!(
                        a.effect_type,
                        AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph | AuraType::Incapacitate
                    )
                })
            })
            .unwrap_or(false)
    }

    /// Get all alive enemies
    pub fn alive_enemies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team != my_team && c.is_alive)
            .collect()
    }

    /// Get all alive allies (including self)
    pub fn alive_allies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team == my_team && c.is_alive)
            .collect()
    }

    /// Get lowest health ally
    pub fn lowest_health_ally(&self) -> Option<&CombatantInfo> {
        self.alive_allies()
            .into_iter()
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
    }

    /// Check if an entity has damage immunity (Divine Shield).
    pub fn entity_is_immune(&self, entity: Entity) -> bool {
        self.active_auras
            .get(&entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity))
            .unwrap_or(false)
    }

    /// Check if an entity is DR-immune to a specific CC category.
    /// AI uses this to avoid wasting CC abilities into immunity.
    pub fn is_dr_immune(&self, entity: Entity, category: DRCategory) -> bool {
        self.dr_trackers
            .get(&entity)
            .map(|tracker| tracker.is_immune(category))
            .unwrap_or(false)
    }

}

/// The result of an AI decision.
#[derive(Debug, Clone)]
pub enum AbilityDecision {
    /// Do nothing this frame
    None,
    /// Use an instant ability on a target
    InstantAbility {
        ability: AbilityType,
        target: Entity,
    },
    /// Start casting a spell on a target
    StartCast {
        ability: AbilityType,
        target: Entity,
    },
    /// Apply a buff to self
    SelfBuff {
        ability: AbilityType,
    },
    /// Apply a buff to an ally
    AllyBuff {
        ability: AbilityType,
        target: Entity,
    },
    /// Use an AoE ability centered on self
    AoeAbility {
        ability: AbilityType,
    },
    /// Place a ground-targeted ability at a specific world position (traps)
    GroundTargetAbility {
        ability: AbilityType,
        position: Vec3,
    },
}

/// Trait for class-specific AI logic.
///
/// Each class implements this trait to provide its decision-making logic.
/// The trait takes a read-only context and returns a decision.
pub trait ClassAI {
    /// Decide what ability (if any) to use this frame.
    ///
    /// Returns `AbilityDecision::None` if no ability should be used.
    fn decide_action(&self, ctx: &CombatContext, combatant: &Combatant) -> AbilityDecision;
}

/// Get the AI implementation for a given class.
pub fn get_class_ai(class: CharacterClass) -> Box<dyn ClassAI> {
    match class {
        CharacterClass::Mage => Box::new(mage::MageAI),
        CharacterClass::Priest => Box::new(priest::PriestAI),
        CharacterClass::Warrior => Box::new(warrior::WarriorAI),
        CharacterClass::Rogue => Box::new(rogue::RogueAI),
        CharacterClass::Warlock => Box::new(warlock::WarlockAI),
        CharacterClass::Paladin => Box::new(paladin::PaladinAI),
        CharacterClass::Hunter => Box::new(hunter::HunterAI),
    }
}

// ============================================================================
// Shared Healer Utilities
// ============================================================================

/// Calculate dispel priority for an aura type.
/// Higher values = more urgent to dispel.
/// Used by Priest (Dispel Magic) and Paladin (Cleanse).
pub fn dispel_priority(aura_type: AuraType) -> i32 {
    match aura_type {
        AuraType::Polymorph => 100,       // Complete incapacitate
        AuraType::Fear => 90,              // Loss of control
        AuraType::Root => 80,              // Can't move
        AuraType::DamageOverTime => 50,    // Taking damage
        AuraType::MovementSpeedSlow => 20, // Minor (typically not worth dispelling)
        _ => 0,
    }
}

/// Check if the team's HP is stable enough for maintenance tasks.
/// Returns true if all living allies are above 70% HP.
///
/// Used by Priest and Paladin to determine when to do maintenance dispels
/// vs focusing on healing.
pub fn is_team_healthy(
    team: u8,
    combatant_info: &HashMap<Entity, CombatantInfo>,
) -> bool {
    for info in combatant_info.values() {
        if info.team != team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }
        let hp_percent = info.current_health / info.max_health;
        if hp_percent < 0.70 {
            return false;
        }
    }
    true
}
