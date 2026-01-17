//! Ability System - Definitions and Data
//!
//! This module contains all ability-related types, definitions, and logic.
//! Abilities are the core combat actions that combatants can perform.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use super::components::{ActiveAuras, AuraType, Combatant};

// Re-export constants from parent module
use super::{MELEE_RANGE};

/// Spell schools - determines which spells share lockouts when interrupted.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum SpellSchool {
    /// Physical abilities (melee attacks, weapon strikes)
    Physical,
    /// Frost magic (Frostbolt, Frost Nova)
    Frost,
    /// Holy magic (Flash Heal, Power Word: Fortitude)
    Holy,
    /// Shadow magic (Mind Blast)
    Shadow,
    /// Arcane magic (Arcane Intellect, Polymorph)
    Arcane,
    /// No spell school (can't be locked out)
    None,
}

/// What stat an ability scales with for damage/healing
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum ScalingStat {
    /// Scales with Attack Power (physical abilities and auto-attacks)
    AttackPower,
    /// Scales with Spell Power (magical abilities and healing)
    SpellPower,
    /// Doesn't scale with any stat (CC abilities, utility)
    None,
}

/// Enum representing available abilities.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum AbilityType {
    Frostbolt,
    FlashHeal,
    HeroicStrike,
    Ambush,
    FrostNova,
    MindBlast,
    SinisterStrike,
    Charge,
    KidneyShot,
    PowerWordFortitude,
    Rend,
    MortalStrike, // Warrior damage + healing reduction
    Pummel,    // Warrior interrupt
    Kick,      // Rogue interrupt
    // Warlock abilities
    Corruption, // Shadow DoT
    Shadowbolt, // Shadow projectile
    Fear,       // Shadow CC - target flees, breaks on damage
    // Buff abilities
    ArcaneIntellect, // Mage buff - increases max mana
    BattleShout,     // Warrior buff - increases attack power
    // Defensive abilities
    IceBarrier,      // Mage self-shield
    PowerWordShield, // Priest shield (self or ally)
}

/// Ability definition with all parameters.
pub struct AbilityDefinition {
    pub name: &'static str,
    /// Cast time in seconds (0.0 = instant)
    pub cast_time: f32,
    /// Maximum range in units
    pub range: f32,
    /// Mana cost to cast
    pub mana_cost: f32,
    /// Cooldown after cast (in seconds)
    pub cooldown: f32,
    /// Base minimum damage (before stat scaling)
    pub damage_base_min: f32,
    /// Base maximum damage (before stat scaling)
    pub damage_base_max: f32,
    /// Coefficient: how much damage per point of Attack Power or Spell Power
    /// Formula: Damage = Base + (Stat × Coefficient)
    pub damage_coefficient: f32,
    /// What stat this ability's damage scales with
    pub damage_scales_with: ScalingStat,
    /// Base minimum healing (before stat scaling)
    pub healing_base_min: f32,
    /// Base maximum healing (before stat scaling)
    pub healing_base_max: f32,
    /// Coefficient: how much healing per point of Spell Power
    /// Formula: Healing = Base + (Spell Power × Coefficient)
    pub healing_coefficient: f32,
    /// Optional aura to apply: (AuraType, duration, magnitude, break_on_damage_threshold)
    /// break_on_damage_threshold: 0.0 = never breaks on damage
    pub applies_aura: Option<(AuraType, f32, f32, f32)>,
    /// Projectile travel speed in units/second (None = instant effect, no projectile)
    pub projectile_speed: Option<f32>,
    /// Spell school (determines lockout when interrupted)
    pub spell_school: SpellSchool,
    /// Whether this ability interrupts the target's casting
    pub is_interrupt: bool,
    /// Lockout duration in seconds (for interrupt abilities)
    pub lockout_duration: f32,
}

impl AbilityType {
    /// Get ability definition (cast time, range, cost, etc.)
    pub fn definition(&self) -> AbilityDefinition {
        match self {
            AbilityType::Frostbolt => AbilityDefinition {
                name: "Frostbolt",
                cast_time: 1.5, // Reduced from 2.5s to see projectiles more often
                range: 40.0,  // 40 yard range like WoW Classic
                mana_cost: 20.0,
                cooldown: 0.0,
                damage_base_min: 10.0,
                damage_base_max: 15.0,
                damage_coefficient: 0.8, // 80% of Spell Power added to damage
                damage_scales_with: ScalingStat::SpellPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: Some((AuraType::MovementSpeedSlow, 5.0, 0.7, 0.0)), // 30% slow for 5s, doesn't break on damage
                projectile_speed: Some(35.0), // Fast projectile
                spell_school: SpellSchool::Frost,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::FlashHeal => AbilityDefinition {
                name: "Flash Heal",
                cast_time: 1.5,
                range: 40.0, // Longer range than Frostbolt
                mana_cost: 25.0,
                cooldown: 0.0,
                damage_base_min: 0.0,
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 15.0,
                healing_base_max: 20.0,
                healing_coefficient: 0.75, // 75% of Spell Power added to healing
                applies_aura: None,
                projectile_speed: None, // Instant effect, no projectile
                spell_school: SpellSchool::Holy,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::HeroicStrike => AbilityDefinition {
                name: "Heroic Strike",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE,
                mana_cost: 15.0, // Costs 15 Rage
                cooldown: 0.0, // No cooldown
                damage_base_min: 0.0, // No direct damage - enhances next auto-attack
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Melee ability, no projectile
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Ambush => AbilityDefinition {
                name: "Ambush",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE,
                mana_cost: 60.0, // High energy cost
                cooldown: 0.0,
                damage_base_min: 10.0, // High burst damage
                damage_base_max: 15.0,
                damage_coefficient: 1.2, // 120% of Attack Power - very high!
                damage_scales_with: ScalingStat::AttackPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Melee ability, no projectile
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::FrostNova => AbilityDefinition {
                name: "Frost Nova",
                cast_time: 0.0, // Instant cast
                range: 10.0, // AOE range - affects all enemies within this distance
                mana_cost: 30.0,
                cooldown: 25.0, // 25 second cooldown
                damage_base_min: 5.0, // Small AOE damage
                damage_base_max: 10.0,
                damage_coefficient: 0.2, // 20% of Spell Power
                damage_scales_with: ScalingStat::SpellPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: Some((AuraType::Root, 6.0, 1.0, 35.0)), // Root for 6s, breaks on 35+ damage
                projectile_speed: None, // Instant AOE, no projectile
                spell_school: SpellSchool::Frost,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::MindBlast => AbilityDefinition {
                name: "Mind Blast",
                cast_time: 1.5, // Same as Frostbolt
                range: 30.0, // Ranged spell
                mana_cost: 25.0,
                cooldown: 8.0, // Short cooldown for consistent damage
                damage_base_min: 15.0, // Good damage
                damage_base_max: 20.0,
                damage_coefficient: 0.6, // 60% of Spell Power
                damage_scales_with: ScalingStat::SpellPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None, // Pure damage, no debuff
                projectile_speed: None, // Instant effect (shadow magic)
                spell_school: SpellSchool::Shadow,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::SinisterStrike => AbilityDefinition {
                name: "Sinister Strike",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 40.0, // 40 energy cost
                cooldown: 0.0, // No inherent cooldown, uses GCD
                damage_base_min: 5.0, // Base weapon damage
                damage_base_max: 10.0,
                damage_coefficient: 0.5, // 50% of Attack Power
                damage_scales_with: ScalingStat::AttackPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Instant melee strike
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Charge => AbilityDefinition {
                name: "Charge",
                cast_time: 0.0, // Instant cast
                range: 25.0, // Max 25 units (minimum 8 units checked separately)
                mana_cost: 0.0, // No rage cost (generates rage in WoW, but we'll keep it simple)
                cooldown: 15.0, // Medium cooldown - can't spam it
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Movement ability, not a projectile
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::KidneyShot => AbilityDefinition {
                name: "Kidney Shot",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 60.0, // 60 energy cost (significant)
                cooldown: 30.0, // Long cooldown - powerful CC
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Stun for 6 seconds, doesn't break on damage (break_threshold = 0.0)
                applies_aura: Some((AuraType::Stun, 6.0, 1.0, 0.0)),
                projectile_speed: None, // Instant melee strike
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::PowerWordFortitude => AbilityDefinition {
                name: "Power Word: Fortitude",
                cast_time: 0.0, // Instant cast
                range: 40.0, // Same range as Flash Heal
                mana_cost: 30.0, // Moderate mana cost
                cooldown: 0.0, // No cooldown - can buff entire team quickly
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0, // Not a heal
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Increase max HP by 30 for 600 seconds (10 minutes, effectively permanent)
                // Magnitude = 30 HP, duration = 600s, no damage breaking
                applies_aura: Some((AuraType::MaxHealthIncrease, 600.0, 30.0, 0.0)),
                projectile_speed: None, // Instant buff
                spell_school: SpellSchool::Holy,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Rend => AbilityDefinition {
                name: "Rend",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 10.0, // 10 rage cost
                cooldown: 0.0, // No cooldown, but can't be reapplied if target already has it
                damage_base_min: 0.0, // No direct damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Apply DoT: 15 second duration, 8 damage per tick (ticks every 3 seconds = 5 ticks total)
                // Magnitude = damage per tick, tick_interval stored separately in Aura
                applies_aura: Some((AuraType::DamageOverTime, 15.0, 8.0, 0.0)),
                projectile_speed: None, // Instant melee application
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::MortalStrike => AbilityDefinition {
                name: "Mortal Strike",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 30.0, // 30 rage cost (expensive)
                cooldown: 6.0, // 6 second cooldown
                damage_base_min: 15.0, // Good physical damage
                damage_base_max: 25.0,
                damage_coefficient: 1.0, // 100% of Attack Power
                damage_scales_with: ScalingStat::AttackPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Apply healing reduction: 10 second duration, 0.65 magnitude = 35% healing reduction
                applies_aura: Some((AuraType::HealingReduction, 10.0, 0.65, 0.0)),
                projectile_speed: None, // Instant melee strike
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Pummel => AbilityDefinition {
                name: "Pummel",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 10.0, // 10 rage cost
                cooldown: 12.0, // Medium cooldown
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None, // Interrupt is handled specially
                projectile_speed: None, // Instant melee interrupt
                spell_school: SpellSchool::Physical,
                is_interrupt: true,
                lockout_duration: 4.0, // 4 second lockout
            },
            AbilityType::Kick => AbilityDefinition {
                name: "Kick",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 25.0, // 25 energy cost
                cooldown: 12.0, // Medium cooldown
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None, // Interrupt is handled specially
                projectile_speed: None, // Instant melee interrupt
                spell_school: SpellSchool::Physical,
                is_interrupt: true,
                lockout_duration: 4.0, // 4 second lockout
            },
            // Warlock abilities
            AbilityType::Corruption => AbilityDefinition {
                name: "Corruption",
                cast_time: 0.0, // Instant cast (WoW classic style)
                range: 30.0, // Ranged spell
                mana_cost: 25.0,
                cooldown: 0.0, // No cooldown, but can only have one on target
                damage_base_min: 0.0, // No direct damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Shadow DoT: 18 second duration, 10 damage per tick (ticks every 3 seconds = 6 ticks, 60 total)
                applies_aura: Some((AuraType::DamageOverTime, 18.0, 10.0, 0.0)),
                projectile_speed: None, // Instant application
                spell_school: SpellSchool::Shadow,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Shadowbolt => AbilityDefinition {
                name: "Shadowbolt",
                cast_time: 2.0, // Slightly longer than Frostbolt
                range: 30.0, // Ranged spell
                mana_cost: 25.0,
                cooldown: 0.0, // No cooldown, spam spell
                damage_base_min: 12.0, // Good base damage
                damage_base_max: 18.0,
                damage_coefficient: 0.85, // 85% of Spell Power - slightly higher than Frostbolt
                damage_scales_with: ScalingStat::SpellPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None, // Pure damage, no debuff (unlike Frostbolt's slow)
                projectile_speed: Some(35.0), // Fast projectile
                spell_school: SpellSchool::Shadow,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Fear => AbilityDefinition {
                name: "Fear",
                cast_time: 1.5, // Classic WoW Fear cast time
                range: 30.0, // Classic WoW Fear range
                mana_cost: 30.0,
                cooldown: 30.0, // 30 second cooldown
                damage_base_min: 0.0,
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Fear: 8 second duration, breaks after 30 damage
                applies_aura: Some((AuraType::Fear, 8.0, 0.0, 30.0)),
                projectile_speed: None, // Instant application on cast complete
                spell_school: SpellSchool::Shadow,
                is_interrupt: false,
                lockout_duration: 0.0,
            },

            // ==================== BUFF ABILITIES ====================

            AbilityType::ArcaneIntellect => AbilityDefinition {
                name: "Arcane Intellect",
                cast_time: 0.0, // Instant cast
                range: 40.0, // Same range as other buffs
                mana_cost: 40.0, // Moderate mana cost
                cooldown: 0.0, // No cooldown - can buff entire team quickly
                damage_base_min: 0.0,
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Increase max mana by 40 for 600 seconds (10 minutes, effectively permanent)
                applies_aura: Some((AuraType::MaxManaIncrease, 600.0, 40.0, 0.0)),
                projectile_speed: None, // Instant buff
                spell_school: SpellSchool::Arcane,
                is_interrupt: false,
                lockout_duration: 0.0,
            },

            AbilityType::BattleShout => AbilityDefinition {
                name: "Battle Shout",
                cast_time: 0.0, // Instant cast
                range: 0.0, // Self-cast AOE, affects nearby allies
                mana_cost: 0.0, // Free in pre-combat (Warriors start with 0 rage)
                cooldown: 0.0, // No cooldown
                damage_base_min: 0.0,
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Increase attack power by 20 for 120 seconds (2 minutes)
                applies_aura: Some((AuraType::AttackPowerIncrease, 120.0, 20.0, 0.0)),
                projectile_speed: None, // Instant buff
                spell_school: SpellSchool::None, // Physical/shout, can't be locked out
                is_interrupt: false,
                lockout_duration: 0.0,
            },

            // ==================== DEFENSIVE ABILITIES ====================

            AbilityType::IceBarrier => AbilityDefinition {
                name: "Ice Barrier",
                cast_time: 0.0, // Instant cast
                range: 0.0, // Self only
                mana_cost: 30.0,
                cooldown: 30.0, // 30 second cooldown
                damage_base_min: 0.0,
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Absorb 60 damage, duration 60s (effectively until broken)
                applies_aura: Some((AuraType::Absorb, 60.0, 60.0, 0.0)),
                projectile_speed: None, // Instant buff
                spell_school: SpellSchool::Frost,
                is_interrupt: false,
                lockout_duration: 0.0,
            },

            AbilityType::PowerWordShield => AbilityDefinition {
                name: "Power Word: Shield",
                cast_time: 0.0, // Instant cast
                range: 40.0, // Can target allies
                mana_cost: 25.0,
                cooldown: 0.0, // No caster cooldown - limited by Weakened Soul on target
                damage_base_min: 0.0,
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Absorb 50 damage, duration 30s
                // NOTE: Also applies Weakened Soul (15s) - handled in AI
                applies_aura: Some((AuraType::Absorb, 30.0, 50.0, 0.0)),
                projectile_speed: None, // Instant buff
                spell_school: SpellSchool::Holy,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
        }
    }
    
    /// Check if a combatant can cast this ability (has mana, in range, not casting, etc.)
    pub fn can_cast_config(
        &self,
        caster: &Combatant,
        target_position: Vec3,
        caster_position: Vec3,
        ability_def: &super::ability_config::AbilityConfig,
    ) -> bool {
        // Check mana/resource
        if caster.current_mana < ability_def.mana_cost {
            return false;
        }

        // Check range
        let distance = caster_position.distance(target_position);
        if distance > ability_def.range {
            return false;
        }

        // Ambush requires stealth
        if matches!(self, AbilityType::Ambush) && !caster.stealthed {
            return false;
        }

        true
    }
}

impl AbilityDefinition {
    /// Returns true if this is a damage ability
    pub fn is_damage(&self) -> bool {
        self.damage_base_max > 0.0 || self.damage_coefficient > 0.0
    }
    
    /// Returns true if this is a healing ability
    pub fn is_heal(&self) -> bool {
        self.healing_base_max > 0.0 || self.healing_coefficient > 0.0
    }
}

/// Helper function to check if a spell school is currently locked out for a combatant
pub fn is_spell_school_locked(spell_school: SpellSchool, auras: Option<&ActiveAuras>) -> bool {
    if let Some(auras) = auras {
        auras.auras.iter().any(|aura| {
            if aura.effect_type == AuraType::SpellSchoolLockout {
                // Convert magnitude back to spell school
                let locked_school = match aura.magnitude as u8 {
                    0 => SpellSchool::Physical,
                    1 => SpellSchool::Frost,
                    2 => SpellSchool::Holy,
                    3 => SpellSchool::Shadow,
                    _ => SpellSchool::None,
                };
                locked_school == spell_school
            } else {
                false
            }
        })
    } else {
        false
    }
}

