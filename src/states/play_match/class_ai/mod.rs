//! Class-Specific AI Modules
//!
//! This module contains the AI decision logic for each character class.
//! Each class has a standalone `decide_<class>_action()` function that is
//! called from `combat_ai.rs` based on the combatant's `CharacterClass`.
//!
//! ## Architecture
//!
//! The combat AI works in two phases:
//! 1. **Context Building**: `CombatContext` collects all game state needed for decisions
//! 2. **Decision Making**: `combat_ai.rs` dispatches to the appropriate class module's
//!    `decide_<class>_action()` function, which directly executes abilities
//!
//! Shared helpers like `CombatContext`, `CombatantInfo`, and healer utilities
//! live in this module and are used by all class AI files.

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

use crate::combat::log::CombatLog;
use super::match_config::CharacterClass;
use super::abilities::AbilityType;
use super::ability_config::AbilityDefinitions;
use super::components::{Aura, ActiveAuras, Combatant, AuraType, DispelPending, PetType, DRCategory, DRTracker};
use super::constants::GCD;
use super::{is_spell_school_locked, is_silenced};
use super::utils::log_ability_use;

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
///
/// The `combatants` map contains ALL entities including pets.
/// Use `alive_enemies()` / `alive_allies()` for primary-combatant-only queries.
/// When iterating `combatants` directly, filter with `!info.is_pet`
/// unless the ability should affect pets (e.g., AoE damage, auto-attacks).
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

    /// Get all alive enemies (excluding pets)
    pub fn alive_enemies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team != my_team && c.is_alive && !c.is_pet)
            .collect()
    }

    /// Get all alive allies (including self, excluding pets)
    pub fn alive_allies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team == my_team && c.is_alive && !c.is_pet)
            .collect()
    }

    /// Get lowest health ally
    pub fn lowest_health_ally(&self) -> Option<&CombatantInfo> {
        self.alive_allies()
            .into_iter()
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
    }

    /// Find the lowest-health ally below a given HP percentage threshold, within range, excluding pets.
    pub fn lowest_health_ally_below(
        &self,
        max_hp_pct: f32,
        max_range: f32,
        my_pos: Vec3,
    ) -> Option<&CombatantInfo> {
        self.alive_allies()
            .into_iter()
            .filter(|info| {
                !info.is_pet
                    && info.health_pct() < max_hp_pct
                    && my_pos.distance(info.position) <= max_range
            })
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
    }

    /// Returns true if all allies are above the given HP threshold.
    pub fn is_team_healthy(&self, threshold: f32, my_pos: Vec3) -> bool {
        self.lowest_health_ally_below(threshold, f32::MAX, my_pos).is_none()
    }

    /// Check if target has a break-on-any-damage CC from a friendly caster.
    /// Uses threshold-based detection: any aura with `break_on_damage_threshold == 0.0`
    /// (breaks on ANY damage) from a same-team caster is protected.
    /// Used to prevent AI from breaking own team's CC with damage/DoTs.
    pub fn has_friendly_breakable_cc(&self, target: Entity) -> bool {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.active_auras
            .get(&target)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.break_on_damage_threshold == 0.0
                        && a.caster
                            .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                            == Some(my_team)
                })
            })
            .unwrap_or(false)
    }

    /// Check if target has DoTs from a friendly caster that would break Polymorph/Freezing Trap.
    pub fn has_friendly_dots_on_target(&self, target: Entity) -> bool {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.active_auras
            .get(&target)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageOverTime
                        && a.caster
                            .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                            == Some(my_team)
                })
            })
            .unwrap_or(false)
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

/// Shared dispel logic used by Priest (Dispel Magic) and Paladin (Cleanse).
///
/// Finds the ally with the highest priority dispellable debuff and casts
/// the specified dispel ability on them. The actual aura removed is randomly
/// selected in process_dispels (WoW Classic behavior).
///
/// The `min_priority` parameter controls which debuffs are considered:
/// - 90: Only urgent CC (Polymorph, Fear)
/// - 50: Include roots and DoTs
/// - 20: Include slows (not recommended)
#[allow(clippy::too_many_arguments)]
pub fn try_dispel_ally(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
    ability_type: AbilityType,
    log_prefix: &'static str,
    log_name: &str,
    caster_class: CharacterClass,
) -> bool {
    let def = abilities.get_unchecked(&ability_type);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }
    // Silence gate (UA backlash). The dispel helper bypasses can_cast_config and
    // deducts mana directly, so this check must live here — otherwise a silenced
    // healer would still successfully dispel.
    if is_silenced(combatant, auras) && def.mana_cost > 0.0 {
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Find ally with highest priority dispellable debuff
    let mut best_candidate: Option<(Entity, i32)> = None;

    for (e, info) in ctx.combatants.iter() {
        // Must be alive ally, skip pets (Felhunter handles its own dispels)
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }

        // Check range
        if my_pos.distance(info.position) > def.range {
            continue;
        }

        // Check if ally has any dispellable debuffs
        let Some(ally_auras) = ctx.active_auras.get(e) else {
            continue;
        };

        // Find highest priority dispellable debuff on this ally
        let mut highest_priority = -1;
        for aura in ally_auras {
            if !aura.can_be_dispelled() {
                continue;
            }

            let priority = dispel_priority(aura.effect_type);

            if priority > highest_priority {
                highest_priority = priority;
            }
        }

        if highest_priority < min_priority {
            continue;
        }

        match best_candidate {
            None => best_candidate = Some((*e, highest_priority)),
            Some((_, best_prio)) if highest_priority > best_prio => {
                best_candidate = Some((*e, highest_priority));
            }
            _ => {}
        }
    }

    let Some((dispel_target, _)) = best_candidate else {
        return false;
    };

    // Execute the ability
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let target_tuple = ctx.combatants.get(&dispel_target).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, log_name, target_tuple, "casts");

    // Spawn pending dispel
    commands.spawn(DispelPending {
        target: dispel_target,
        dispeller: entity,
        log_prefix,
        caster_class,
        heal_on_success: None,
        aura_type_filter: None,
    });

    info!(
        "Team {} {} casts {} on ally",
        combatant.team,
        combatant.class.name(),
        log_name
    );

    true
}
