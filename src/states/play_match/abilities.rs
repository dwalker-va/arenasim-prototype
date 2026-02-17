//! Ability System - Types and Enums
//!
//! This module contains ability-related types and enums.
//! Actual ability definitions are loaded from `assets/config/abilities.ron`
//! via the `ability_config` module.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use super::components::{ActiveAuras, AuraType, Combatant};

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
    /// Fire magic (Immolate)
    Fire,
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
    CheapShot, // Rogue stealth opener - 4s stun
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
    Corruption,     // Shadow DoT
    Shadowbolt,     // Shadow projectile
    Fear,           // Shadow CC - target flees, breaks on damage
    Immolate,       // Fire direct damage + DoT
    DrainLife,      // Shadow channel - damages target, heals caster
    CurseOfAgony,   // Shadow DoT - 84 damage over 24s
    CurseOfWeakness, // Shadow debuff - reduces target damage dealt
    CurseOfTongues, // Shadow debuff - increases target cast time
    // Buff abilities
    ArcaneIntellect, // Mage buff - increases max mana
    BattleShout,     // Warrior buff - increases attack power
    // Defensive abilities
    IceBarrier,      // Mage self-shield
    PowerWordShield, // Priest shield (self or ally)
    // Crowd Control abilities
    Polymorph, // Mage CC - transforms target into sheep, breaks on any damage
    // Dispel abilities
    DispelMagic, // Priest - removes one magic debuff from ally
    // Paladin abilities
    FlashOfLight,     // Paladin fast heal
    HolyLight,        // Paladin big heal (2.5s cast)
    HolyShock,        // Paladin dual-purpose: damage enemy OR heal ally
    HammerOfJustice,  // Paladin 6s stun
    PaladinCleanse,   // Paladin dispel magic
    DevotionAura,     // Paladin team buff - reduces damage taken by 10%
    DivineShield,     // Paladin bubble - damage immunity, purges debuffs, 50% damage penalty
    // Pet abilities (Felhunter)
    SpellLock,        // Felhunter interrupt (instant, 30yd, 30s CD, 3s silence)
    DevourMagic,      // Felhunter dispel (instant, 30yd, 8s CD, heals pet on success)
}

impl AbilityType {
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

        // Stealth abilities require stealth
        if matches!(self, AbilityType::Ambush | AbilityType::CheapShot) && !caster.stealthed {
            return false;
        }

        true
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
                    4 => SpellSchool::Arcane,
                    5 => SpellSchool::Fire,
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
