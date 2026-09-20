//! Ability System - Types and Enums
//!
//! This module contains ability-related types and enums.
//! Actual ability definitions are loaded from `assets/config/abilities.ron`
//! via the `ability_config` module.

use super::components::{ActiveAuras, AuraType, Combatant};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Declare [`SpellSchool`] and everything derived from it: [`SpellSchool::all`].
///
/// The variant list is written ONCE. `all()` expands from the same tokens as
/// the enum, so a school cannot exist without also being enumerable — the
/// shape `item_ids!` (see `super::equipment`) uses for `ItemId`, for the same
/// reason.
///
/// `all()` used to be a hand-written slice below the enum, and a hand-written
/// slice goes stale in silence. Adding a school drags you into the exhaustive
/// matches — the compiler sees to that — but never into the slice, and every
/// sweep written over `all()` then quietly stops covering the new school while
/// still reading as whole-enum coverage: the encyclopedia's filter chips would
/// never offer it, and the dispel-derivation guards in `components::auras`
/// would skip it. Measured, not theorised: a ninth variant added to the enum
/// and classified at all seven exhaustive matches left `all()` at eight of
/// nine with the school's own guard test still green.
macro_rules! spell_schools {
    ($( $(#[$meta:meta])* $variant:ident ),* $(,)?) => {
        /// Spell schools - determines which spells share lockouts when interrupted.
        ///
        /// Declared via [`spell_schools!`].
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
        pub enum SpellSchool {
            $( $(#[$meta])* $variant, )*
        }

        impl SpellSchool {
            /// Every school, in declaration order. The single source of truth
            /// for any surface that enumerates schools (the encyclopedia's
            /// filter chips). Generated from the enum's own token list, so it
            /// cannot fall behind it.
            pub const fn all() -> &'static [SpellSchool] {
                &[ $( SpellSchool::$variant, )* ]
            }
        }
    };
}

spell_schools! {
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
    /// Nature magic (Spider Web, Master's Call)
    Nature,
    /// No spell school (can't be locked out)
    None,
}

impl SpellSchool {
    /// Canonical per-school RGB (sRGB bytes) — the single color authority shared
    /// by the View Combatant UI (as `egui::Color32`) and world-space casting
    /// visuals (as `bevy::Color`). WoW-canonical hues; exhaustive so a new
    /// school cannot ship without a color.
    pub const fn color_rgb8(self) -> (u8, u8, u8) {
        match self {
            SpellSchool::Physical => (199, 156, 110), // Brown/tan
            SpellSchool::Frost => (100, 180, 255),    // Ice blue
            SpellSchool::Fire => (255, 128, 64),      // Orange-red
            SpellSchool::Shadow => (148, 130, 201),   // Purple
            SpellSchool::Arcane => (255, 128, 255),   // Pink/magenta
            SpellSchool::Holy => (255, 230, 150),     // Golden yellow
            SpellSchool::Nature => (76, 196, 30),     // Green
            SpellSchool::None => (220, 220, 220),     // Gray
        }
    }

    /// The same authority as a `bevy::Color`, for world-space effects. `const`
    /// so an effect module can name it in a constant instead of copying the
    /// bytes by hand — which is how "Frost" drifted to five values.
    pub const fn color(self) -> Color {
        let (r, g, b) = self.color_rgb8();
        Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    /// Encode this school as the `magnitude` of a `SpellSchoolLockout` aura.
    /// The lockout aura carries the locked school in its `magnitude` field (it
    /// has no dedicated school slot), so this and [`SpellSchool::from_lockout_magnitude`]
    /// are the single round-trip used by the interrupt pipeline (writer) and the
    /// AI (reader, e.g. the Rogue's second-school Kidney Shot check).
    pub fn to_lockout_magnitude(self) -> f32 {
        match self {
            SpellSchool::Physical => 0.0,
            SpellSchool::Frost => 1.0,
            SpellSchool::Holy => 2.0,
            SpellSchool::Shadow => 3.0,
            SpellSchool::Arcane => 4.0,
            SpellSchool::Fire => 5.0,
            SpellSchool::Nature => 6.0,
            SpellSchool::None => 7.0,
        }
    }

    /// Decode a `SpellSchoolLockout` aura's `magnitude` back into a school.
    /// Inverse of [`SpellSchool::to_lockout_magnitude`]; unrecognized values
    /// fall back to `None` (treated as "no real school locked").
    pub fn from_lockout_magnitude(magnitude: f32) -> SpellSchool {
        match magnitude.round() as i32 {
            0 => SpellSchool::Physical,
            1 => SpellSchool::Frost,
            2 => SpellSchool::Holy,
            3 => SpellSchool::Shadow,
            4 => SpellSchool::Arcane,
            5 => SpellSchool::Fire,
            6 => SpellSchool::Nature,
            _ => SpellSchool::None,
        }
    }
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
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
    MortalStrike,    // Warrior damage + healing reduction
    Pummel,          // Warrior interrupt
    BerserkerRage,   // Warrior fear break + 10s fear immunity (horror bypasses it)
    Kick,            // Rogue interrupt
    CripplingPoison, // Rogue weapon poison: on-hit chance to slow (passive, not cast)
    // Warlock abilities
    Corruption,         // Shadow DoT
    Shadowbolt,         // Shadow projectile
    Fear,               // Shadow CC - target flees, breaks on damage
    Immolate,           // Fire direct damage + DoT
    DrainLife,          // Shadow channel - damages target, heals caster
    CurseOfAgony,       // Shadow DoT - 84 damage over 24s
    CurseOfWeakness,    // Shadow debuff - reduces target damage dealt
    CurseOfTongues,     // Shadow debuff - increases target cast time
    UnstableAffliction, // Shadow DoT - dispel backlash applies Silence + Shadow damage
    DeathCoil, // Shadow instant - 3s horror (never breaks) + damage + self-heal, peel cooldown
    // Buff abilities
    ArcaneIntellect, // Mage buff - increases max mana
    BattleShout,     // Warrior buff - increases attack power
    // Defensive abilities
    IceBarrier,      // Mage self-shield
    PowerWordShield, // Priest shield (self or ally)
    // Crowd Control abilities
    Polymorph, // Mage CC - transforms target into sheep, breaks on any damage
    // Dispel abilities
    DispelMagic,   // Priest - removes one magic debuff from ally
    PsychicScream, // Priest - instant self-centered AoE fear, breaks on damage
    ManaBurn,      // Priest - cast-time Shadow spell that destroys mana on an enemy mana user
    // Paladin abilities
    FlashOfLight,    // Paladin fast heal
    HolyLight,       // Paladin big heal (2.5s cast)
    HolyShock,       // Paladin dual-purpose: damage enemy OR heal ally
    HammerOfJustice, // Paladin 6s stun
    PaladinCleanse,  // Paladin dispel magic
    DevotionAura,    // Paladin team buff - reduces damage taken by 10%
    DivineShield,    // Paladin bubble - damage immunity, purges debuffs, 50% damage penalty
    // Pet abilities (Felhunter)
    SpellLock,   // Felhunter interrupt (instant, 30yd, 30s CD, 3s silence)
    DevourMagic, // Felhunter dispel (instant, 30yd, 8s CD, heals pet on success)
    // Hunter abilities
    AimedShot,      // Hunter cast-time physical damage + healing reduction (35yd, 10s CD)
    ArcaneShot,     // Hunter instant Arcane damage (35yd, 6s CD)
    ConcussiveShot, // Hunter instant slow (35yd, 12s CD)
    SerpentSting,   // Hunter instant Nature DoT (35yd, no CD, pure DoT)
    Disengage,      // Hunter backward leap (~15 yards, 25s CD, no range req)
    FreezingTrap,   // Hunter trap — incapacitates first enemy (25s CD)
    FrostTrap,      // Hunter trap — creates persistent slow zone (20s CD)
    // Hunter pet abilities
    SpiderWeb,   // Spider ranged root on target (45s CD)
    BoarCharge,  // Boar gap closer + short stun (45s CD)
    MastersCall, // Bird removes movement impairments from friendly (45s CD)
    // Strategic option abilities
    DemoralizingShout,    // Warrior debuff - reduces enemy attack power
    CommandingShout,      // Warrior buff - increases team max health
    FrostArmor,           // Mage self-buff - procs slow on melee attackers
    MageArmorSpell,       // Mage self-buff - increases mana regen
    MoltenArmor,          // Mage self-buff - increases crit chance
    ShadowResistanceAura, // Paladin team aura - shadow resistance
    ConcentrationAura,    // Paladin team aura - reduces interrupt lockout duration
    // Shaman abilities
    LightningBolt,     // Shaman ranged Nature nuke (cast time)
    FrostShock,        // Shaman instant Frost nuke + single-target slow
    LesserHealingWave, // Shaman fast direct heal
    Purge,             // Shaman offensive dispel - removes one enemy buff
    WindShear,         // Shaman ranged instant interrupt
    AirTotem,          // Shaman Windfury Totem - empowers melee allies
    WaterTotem,        // Shaman Healing Stream Totem - periodic ally heal
    EarthTotem,        // Shaman Strength of Earth Totem - ally attack power
    FireTotem,         // Shaman Flametongue Totem - ally spell power
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

        // Check minimum range (Hunter dead zone)
        if let Some(min_range) = ability_def.min_range {
            if distance < min_range {
                return false;
            }
        }

        // Stealth abilities require stealth
        if matches!(self, AbilityType::Ambush | AbilityType::CheapShot) && !caster.stealthed {
            return false;
        }

        true
    }
}

/// Helper function to check if a combatant is currently silenced.
///
/// Silence prevents the combatant from using any mana-cost ability, but ONLY
/// applies to combatants whose resource type is Mana. Warriors (Rage) and Rogues
/// (Energy) are never blocked by this helper, even if they somehow received a
/// Silence aura. This matches the design intent from the brainstorm: silence is
/// a caster-class counter, not a universal CC.
///
/// Applied by Unstable Affliction dispel backlash.
pub fn is_silenced(caster: &super::components::Combatant, auras: Option<&ActiveAuras>) -> bool {
    use super::components::ResourceType;
    if caster.resource_type != ResourceType::Mana {
        return false;
    }
    if let Some(auras) = auras {
        auras
            .auras
            .iter()
            .any(|aura| aura.effect_type == AuraType::Silence)
    } else {
        false
    }
}

/// Helper function to check if a spell school is currently locked out for a combatant
pub fn is_spell_school_locked(spell_school: SpellSchool, auras: Option<&ActiveAuras>) -> bool {
    if let Some(auras) = auras {
        auras.auras.iter().any(|aura| {
            aura.effect_type == AuraType::SpellSchoolLockout
                // The decode is `SpellSchool::from_lockout_magnitude` and only
                // that. This carried its own copy of the magnitude table — the
                // same hand-maintained-list defect one layer down, and the
                // third copy of a codec that has exactly one correct answer.
                && SpellSchool::from_lockout_magnitude(aura.magnitude) == spell_school
        })
    } else {
        false
    }
}

/// Guards for the school codec.
///
/// `SpellSchool::all()` going stale is no longer something these tests have to
/// catch — `spell_schools!` generates it from the enum's own tokens, so the
/// omission cannot be written. What is left to guard is the one part of the
/// codec the compiler cannot reach: [`SpellSchool::from_lockout_magnitude`]
/// ends in a wildcard, so a new school given an unmapped magnitude decodes to
/// `None` rather than failing to build.
#[cfg(test)]
mod spell_school_tests {
    use super::*;
    use crate::states::play_match::components::Aura;

    /// No two schools share a lockout magnitude.
    ///
    /// The lockout aura carries its school in a single `f32`, so a collision
    /// means an interrupt locks out the wrong school with nothing to notice.
    /// Counted against `all()` rather than a literal, so it moves with the
    /// enum instead of needing a bump — which is what the old `seen == 8`
    /// assertion needed, and why it could not see the omission it was written
    /// for.
    #[test]
    fn lockout_magnitudes_are_unique_across_every_school() {
        let mut magnitudes: Vec<u8> = SpellSchool::all()
            .iter()
            .map(|s| s.to_lockout_magnitude() as u8)
            .collect();
        let listed = magnitudes.len();
        magnitudes.sort_unstable();
        magnitudes.dedup();
        assert_eq!(
            magnitudes.len(),
            listed,
            "two of the {listed} schools encode to the same lockout magnitude — \
             the codec cannot tell them apart"
        );
        assert!(listed >= 8, "the school list should not shrink silently");
    }

    /// Every school survives the lockout codec round trip.
    ///
    /// This is the guard that catches an unclassified NEW school.
    /// `to_lockout_magnitude` is exhaustive, so a new school must be given a
    /// magnitude; give it an unmapped one and it decodes to `None` here, give
    /// it an existing one and the uniqueness check above fires. Either way the
    /// omission is loud instead of silently locking out the wrong school.
    #[test]
    fn every_school_round_trips_through_the_lockout_codec() {
        for &school in SpellSchool::all() {
            let magnitude = school.to_lockout_magnitude();
            assert_eq!(
                SpellSchool::from_lockout_magnitude(magnitude),
                school,
                "{school:?} encodes to {magnitude} but decodes to something else — \
                 from_lockout_magnitude's wildcard swallowed it"
            );
        }
    }

    /// The lockout READER agrees with the codec for every school, and answers
    /// `false` for every other school. Swept over `all()`, so a new school is
    /// covered the moment it is declared.
    #[test]
    fn is_spell_school_locked_reads_every_school() {
        for &locked in SpellSchool::all() {
            let auras = ActiveAuras {
                auras: vec![Aura {
                    effect_type: AuraType::SpellSchoolLockout,
                    magnitude: locked.to_lockout_magnitude(),
                    ..Default::default()
                }],
            };
            for &probe in SpellSchool::all() {
                assert_eq!(
                    is_spell_school_locked(probe, Some(&auras)),
                    probe == locked,
                    "a lockout on {locked:?} answered wrongly for {probe:?}"
                );
            }
        }
    }

    /// An aura that is not a lockout never locks anything out.
    #[test]
    fn a_non_lockout_aura_locks_nothing() {
        let auras = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::Stun,
                magnitude: SpellSchool::Frost.to_lockout_magnitude(),
                ..Default::default()
            }],
        };
        for &probe in SpellSchool::all() {
            assert!(
                !is_spell_school_locked(probe, Some(&auras)),
                "a Stun carrying a Frost-shaped magnitude locked out {probe:?}"
            );
        }
    }
}
