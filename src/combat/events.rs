//! Combat events
//!
//! Defines the events that occur during combat for logging and processing.

use bevy::prelude::*;

use super::components::CrowdControlType;

/// Event fired when damage is dealt
#[derive(Event)]
pub struct DamageEvent {
    /// Entity dealing the damage
    pub source: Entity,
    /// Entity receiving the damage
    pub target: Entity,
    /// Amount of damage before mitigation
    pub amount: f32,
    /// Amount of damage after mitigation
    pub final_amount: f32,
    /// Name of the ability that caused the damage (None for auto-attack)
    pub ability_name: Option<String>,
    /// Whether this was a critical hit
    pub is_critical: bool,
    /// Damage type
    pub damage_type: DamageType,
}

/// Types of damage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageType {
    Physical,
    Fire,
    Frost,
    Nature,
    Shadow,
    Holy,
    Arcane,
}

/// Event fired when healing is done
#[derive(Event)]
pub struct HealingEvent {
    /// Entity doing the healing
    pub source: Entity,
    /// Entity receiving the healing
    pub target: Entity,
    /// Amount healed
    pub amount: f32,
    /// Name of the healing ability
    pub ability_name: String,
    /// Whether this was a critical heal
    pub is_critical: bool,
}

/// Event fired when an ability is used
#[derive(Event)]
pub struct AbilityUsedEvent {
    /// Entity using the ability
    pub caster: Entity,
    /// Target of the ability (if any)
    pub target: Option<Entity>,
    /// Name of the ability
    pub ability_name: String,
}

/// Event fired when an aura is applied
#[derive(Event)]
pub struct AuraAppliedEvent {
    /// Entity that applied the aura
    pub source: Entity,
    /// Entity the aura is applied to
    pub target: Entity,
    /// Name of the aura
    pub aura_name: String,
    /// Duration in seconds
    pub duration: Option<f32>,
    /// Whether this is a buff
    pub is_buff: bool,
}

/// Event fired when an aura is removed
#[derive(Event)]
pub struct AuraRemovedEvent {
    /// Entity the aura was on
    pub target: Entity,
    /// Name of the aura
    pub aura_name: String,
    /// Why it was removed
    pub reason: AuraRemovalReason,
}

/// Reason an aura was removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraRemovalReason {
    /// Duration expired
    Expired,
    /// Dispelled by an ability
    Dispelled,
    /// Target died
    TargetDied,
    /// Replaced by a new application
    Replaced,
}

/// Event fired when crowd control is applied
#[derive(Event)]
pub struct CrowdControlEvent {
    /// Entity applying the CC
    pub source: Entity,
    /// Entity receiving the CC
    pub target: Entity,
    /// Type of crowd control
    pub cc_type: CrowdControlType,
    /// Duration in seconds
    pub duration: f32,
}

/// Event fired when a combatant dies
#[derive(Event)]
pub struct CombatantDeathEvent {
    /// Entity that died
    pub victim: Entity,
    /// Entity that dealt the killing blow
    pub killer: Entity,
}

