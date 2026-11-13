//! Talents: each class's trees, a combatant's [`Build`], and how a build
//! resolves into that combatant at spawn.
//!
//! Design: `docs/design/proposals/2026-10-11-talents-and-gauntlet.md` §3-4.
//! `assets/config/talents.ron` holds the trees and their nodes
//! ([`TalentDefinitions`], loaded and validated in both modes beside the
//! ability table). A match carries one [`Build`] per combatant
//! (`MatchConfig::team{1,2}_builds`); an empty build is the default.
//!
//! **Resolution happens once, at spawn** ([`TalentDefinitions::resolve`]):
//! - the combatant's [`AbilityBook`] starts from its class's base abilities,
//!   gains every ability a `Grant` names, then takes every `Patch`;
//! - its `StatAdd` effects add points to its totals after its equipment, as
//!   an item adds them, and its `StatPercent` effects then scale those totals
//!   ([`ResolvedBuild::apply`], [`StatChange::applied`]); its `PetStatPercent`
//!   effects scale its pet's ([`ResolvedBuild::apply_to_pet`]), after the pet
//!   takes its share of the owner's (already talented) stats.
//!
//! Nothing changes a resolved build during a match: anything that does is an
//! aura. An empty build resolves to the untalented book and applies nothing,
//! so a match without talents is the match it was before talents existed.
//!
//! **Prices are a sanity check, not a boundary.** A node's stated value is
//! what ships. Each rank still has an item-budget price: a node whose every
//! effect is a `StatPercent` or a `StatAdd` on a stat the item budget weighs
//! is priced by that weight ([`rank_price`]): a `StatAdd` at its points, as
//! an item's stat is priced, and a `StatPercent` at its percentage of the
//! class's reference combatant's total. Every other node declares its price.
//! `tests/talents.rs` prints every rank's price and every legal build's for a
//! designer to read, against the reference of 11 a rank and 22 a capstone
//! ([`REFERENCE_RANK_PRICE`], [`REFERENCE_CAPSTONE_PRICE`]), and fails only on
//! an absurd one ([`TalentConfig::price_sanity`]). A value is never derived to
//! make a rank cost 11.
//!
//! [`rank_price`]: TalentDefinitions::rank_price

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::abilities::{AbilityType, SpellSchool};
use super::ability_book::AbilityBook;
use super::ability_config::{AbilityConfig, AbilityDefinitions};
use super::components::{AuraType, Combatant};
use super::constants::{
    WEIGHT_ATTACK_POWER, WEIGHT_CRIT_CHANCE, WEIGHT_MANA_REGEN, WEIGHT_MAX_HEALTH, WEIGHT_MAX_MANA,
    WEIGHT_MOVEMENT_SPEED, WEIGHT_RESISTANCE, WEIGHT_SPELL_POWER,
};
use super::equipment::{
    resolve_equipped_loadout, validate_class_restrictions, DefaultLoadouts, ItemDefinitions,
    Loadout,
};
use super::talent_choices::{ranked_effects, ChoiceOption, ChoiceRule, ChoiceSlot, SlotChoices};
use super::talent_mechanics::{TalentMechanic, TalentMechanics};
use super::talent_procs::{TalentProc, TalentProcSlot};
use crate::states::match_config::CharacterClass;

/// The most points a build may spend: what a Gauntlet run hands out by its
/// end, and what Custom Match allows.
pub const MAX_TALENT_POINTS: u32 = 8;

/// Points a tree must hold, in the tiers BELOW a tier, before that tier opens:
/// tier 1 at 0, tier 2 at 2, tier 3 at 3, tier 4 (the capstone) at 5.
pub const TIER_OPENS_AT: [u32; 4] = [0, 2, 3, 5];

/// The capstone's tier: the one node a tree prices at two ranks.
pub const CAPSTONE_TIER: u8 = 4;

/// The item-budget price a rank of a node is read against: the starting
/// point the trees were sized at, not a price any rank must hit.
pub const REFERENCE_RANK_PRICE: f32 = 11.0;

/// The price a capstone is read against: two ranks.
pub const REFERENCE_CAPSTONE_PRICE: f32 = 22.0;

/// How far a rank's price may stray from its reference, as a factor either
/// way, before it is absurd rather than a design choice. Wide on purpose: a
/// stated +1% crit chance prices at 3 (0.27x of a rank) and Vitality's +5%
/// health at about 20 (1.8x), and both are values the design states. What it
/// catches is a slipped decimal, which moves a price tenfold.
pub const PRICE_SANITY_FACTOR: f32 = 5.0;

/// Declare [`TalentId`] and everything derived from it: [`TalentId::all`],
/// [`TalentId::as_str`] and its [`FromStr`](std::str::FromStr) impl — the
/// shape of `item_ids!`, so a talent cannot exist without being enumerable
/// and parseable from its own name.
macro_rules! talent_ids {
    ($( $variant:ident ),* $(,)?) => {
        /// One talent node. The variant name is the key `talents.ron` stores
        /// the node under and what a build (and a headless config) names it
        /// by.
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        pub enum TalentId {
            $( $variant, )*
        }

        impl TalentId {
            /// Every talent, in declaration order.
            pub fn all() -> &'static [TalentId] {
                &[ $( TalentId::$variant, )* ]
            }

            /// The talent's canonical string form: its variant name.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( TalentId::$variant => stringify!($variant), )*
                }
            }
        }

        impl std::str::FromStr for TalentId {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $( stringify!($variant) => Ok(TalentId::$variant), )*
                    _ => Err(format!(
                        "Unknown talent: '{s}'. Valid talents are the keys of assets/config/talents.ron"
                    )),
                }
            }
        }
    };
}

talent_ids! {
    // Warlock: Demonology
    DemonicEmbrace,
    FelStamina,
    UnholyPower,
    MasterDemonologist,
    DemonicSacrifice,
    SoulLink,
    // Rogue: Combat
    LightningReflexes,
    DualWieldSpecialization,
    Aggression,
    Vitality,
    Deflection,
    SwiftBlades,
    ExposeArmor,
    // Priest: Discipline
    UnbreakableWill,
    ImprovedPowerWordShield,
    InnerFocus,
    MentalAgility,
    PowerInfusion,
    ReflectiveShield,
    PainSuppression,
    // Priest: Shadow
    ImprovedMindBlast,
    ImprovedPsychicScream,
    Shadowform,
    // Paladin: Protection
    SacredDuty,
    ImprovedHammerOfJustice,
    DivineProtection,
    RetributionAura,
    Toughness,
    BlessingOfSacrifice,
    SacredShield,
    // Warlock: Affliction
    ImprovedDrainLife,
    ImprovedLifeTap,
    AmplifyCurse,
    Nightfall,
    SiphonLife,
    CurseOfExhaustion,
    ShadowMastery,
    // Warlock: Destruction
    ImprovedShadowBolt,
    Bane,
    Aftermath,
    Shadowburn,
    ImprovedImmolate,
    Conflagrate,
    // Paladin: Holy
    ImprovedHolyShock,
    HealingLight,
    InfusionOfLight,
    DivineFavor,
    PurifyingPower,
    LightsGrace,
    AuraMastery,
}

/// A talent tree. A tree is named per class: `(class, TreeId)` is the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TreeId {
    Affliction,
    Demonology,
    Combat,
    Shadow,
    Protection,
    Discipline,
    Destruction,
    Holy,
}

/// One tree of a class, as `talents.ron` lists it. A class's trees are in
/// display order (WoW's "2/6/0" reads in it).
#[derive(Debug, Clone, Deserialize)]
pub struct TreeConfig {
    pub id: TreeId,
    pub name: String,
    pub icon: String,
}

/// A stat a talent raises: one of the combatant totals the item stats feed,
/// read after equipment. A `StatAdd` adds points to it, as an item does; a
/// `StatPercent` takes a percentage of the total, gear and points included
/// ([`StatChange::applied`] is the one rule). A chance or a haste that is 0
/// for every class (dodge, parry, melee attack speed) takes points only
/// ([`TalentStat::takes_points_only`]): a percentage of 0 would be nothing.
/// So does movement speed, whose points are already a fraction of natural
/// speed: a passive source, offered to the one capped passive term
/// ([`Combatant::offer_passive_speed`]) as an item's is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TalentStat {
    MaxHealth,
    MaxMana,
    ManaRegen,
    AttackPower,
    SpellPower,
    CritChance,
    MovementSpeed,
    Armor,
    FireResistance,
    FrostResistance,
    ShadowResistance,
    ArcaneResistance,
    NatureResistance,
    HolyResistance,
    /// The damage of every weapon swing the combatant takes: its live
    /// auto-attack, a Hunter's melee main hand and an off hand.
    WeaponDamage,
    /// The damage of the off-hand weapon's swing only.
    OffHandDamage,
    /// Chance to dodge a melee attack (`Combatant::dodge_chance`). Points
    /// only.
    DodgeChance,
    /// Chance to parry a melee attack (`Combatant::parry_chance`). Points
    /// only.
    ParryChance,
    /// Haste on melee swings only (`Combatant::melee_attack_speed_bonus`).
    /// Points only: `0.10` swings 10% faster.
    MeleeAttackSpeed,
}

impl TalentStat {
    /// The item budget's weight for this stat, per point: what a point of it
    /// costs on an item. `None` for a stat items carry for free (armor, weapon
    /// damage), or that no item carries at all (dodge, parry, melee haste):
    /// a percentage of one has no computed price, so its node declares one —
    /// even for haste, which the budget prices only as a proc
    /// (`WEIGHT_ATTACK_SPEED`). Giving such a stat a weight here would
    /// re-price every item that carries it.
    pub fn item_weight(self) -> Option<f32> {
        match self {
            TalentStat::MaxHealth => Some(WEIGHT_MAX_HEALTH),
            TalentStat::MaxMana => Some(WEIGHT_MAX_MANA),
            TalentStat::ManaRegen => Some(WEIGHT_MANA_REGEN),
            TalentStat::AttackPower => Some(WEIGHT_ATTACK_POWER),
            TalentStat::SpellPower => Some(WEIGHT_SPELL_POWER),
            TalentStat::CritChance => Some(WEIGHT_CRIT_CHANCE),
            TalentStat::MovementSpeed => Some(WEIGHT_MOVEMENT_SPEED),
            TalentStat::FireResistance
            | TalentStat::FrostResistance
            | TalentStat::ShadowResistance
            | TalentStat::ArcaneResistance
            | TalentStat::NatureResistance
            | TalentStat::HolyResistance => Some(WEIGHT_RESISTANCE),
            TalentStat::Armor
            | TalentStat::WeaponDamage
            | TalentStat::OffHandDamage
            | TalentStat::DodgeChance
            | TalentStat::ParryChance
            | TalentStat::MeleeAttackSpeed => None,
        }
    }

    /// Whether only a `StatAdd` may raise this stat: a chance or haste every
    /// class has at 0, so a percentage of its total would be nothing, or
    /// movement speed, whose points are already a percentage. A `StatPercent`
    /// of one is refused at load.
    pub fn takes_points_only(self) -> bool {
        match self {
            TalentStat::DodgeChance
            | TalentStat::ParryChance
            | TalentStat::MeleeAttackSpeed
            | TalentStat::MovementSpeed => true,
            TalentStat::MaxHealth
            | TalentStat::MaxMana
            | TalentStat::ManaRegen
            | TalentStat::AttackPower
            | TalentStat::SpellPower
            | TalentStat::CritChance
            | TalentStat::Armor
            | TalentStat::FireResistance
            | TalentStat::FrostResistance
            | TalentStat::ShadowResistance
            | TalentStat::ArcaneResistance
            | TalentStat::NatureResistance
            | TalentStat::HolyResistance
            | TalentStat::WeaponDamage
            | TalentStat::OffHandDamage => false,
        }
    }

    /// The combatant's total of this stat (for a weapon, its live swing's
    /// damage; for movement speed, its strongest passive source).
    pub fn total(self, c: &Combatant) -> f32 {
        match self {
            TalentStat::MaxHealth => c.max_health,
            TalentStat::MaxMana => c.max_mana,
            TalentStat::ManaRegen => c.mana_regen,
            TalentStat::AttackPower => c.attack_power,
            TalentStat::SpellPower => c.spell_power,
            TalentStat::CritChance => c.crit_chance,
            TalentStat::MovementSpeed => c.passive_speed,
            TalentStat::Armor => c.armor,
            TalentStat::FireResistance => c.fire_resistance,
            TalentStat::FrostResistance => c.frost_resistance,
            TalentStat::ShadowResistance => c.shadow_resistance,
            TalentStat::ArcaneResistance => c.arcane_resistance,
            TalentStat::NatureResistance => c.nature_resistance,
            TalentStat::HolyResistance => c.holy_resistance,
            TalentStat::WeaponDamage => c.attack_damage,
            TalentStat::OffHandDamage => c.offhand_damage,
            TalentStat::DodgeChance => c.dodge_chance,
            TalentStat::ParryChance => c.parry_chance,
            TalentStat::MeleeAttackSpeed => c.melee_attack_speed_bonus,
        }
    }

    /// Apply `change` to this stat of the combatant ([`StatChange::applied`]);
    /// movement speed's points are offered as a passive source instead.
    fn apply(self, c: &mut Combatant, change: StatChange) {
        let stat = |value: &mut f32| *value = change.applied(*value);
        match self {
            TalentStat::MaxHealth => stat(&mut c.max_health),
            TalentStat::MaxMana => stat(&mut c.max_mana),
            TalentStat::ManaRegen => stat(&mut c.mana_regen),
            TalentStat::AttackPower => stat(&mut c.attack_power),
            TalentStat::SpellPower => stat(&mut c.spell_power),
            TalentStat::CritChance => stat(&mut c.crit_chance),
            TalentStat::MovementSpeed => c.offer_passive_speed(change.points),
            TalentStat::Armor => stat(&mut c.armor),
            TalentStat::FireResistance => stat(&mut c.fire_resistance),
            TalentStat::FrostResistance => stat(&mut c.frost_resistance),
            TalentStat::ShadowResistance => stat(&mut c.shadow_resistance),
            TalentStat::ArcaneResistance => stat(&mut c.arcane_resistance),
            TalentStat::NatureResistance => stat(&mut c.nature_resistance),
            TalentStat::HolyResistance => stat(&mut c.holy_resistance),
            TalentStat::WeaponDamage => {
                stat(&mut c.attack_damage);
                stat(&mut c.melee_damage);
                stat(&mut c.offhand_damage);
            }
            TalentStat::OffHandDamage => stat(&mut c.offhand_damage),
            TalentStat::DodgeChance => stat(&mut c.dodge_chance),
            TalentStat::ParryChance => stat(&mut c.parry_chance),
            TalentStat::MeleeAttackSpeed => stat(&mut c.melee_attack_speed_bonus),
        }
    }
}

/// What a build does to one of its combatant's stats: the points its
/// `StatAdd` ranks add and the fraction its `StatPercent` ranks take.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StatChange {
    /// Added to the total as an item's stat is: `0.02` crit chance is +2%.
    pub points: f32,
    /// A percentage of the total, gear and points included: `0.06` is +6%.
    pub fraction: f32,
}

impl StatChange {
    /// `value` with this change applied: the points added, as an item adds
    /// them, then the total scaled by `1 + fraction`. The one rule a spawn and
    /// every display of a talented stat use.
    pub fn applied(self, value: f32) -> f32 {
        (value + self.points) * (1.0 + self.fraction)
    }
}

/// What a `Patch` rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum PatchTarget {
    /// One ability of the class (a base ability, or one a talent grants).
    Ability(AbilityType),
    /// Every ability in the book of this school that has the field.
    School(SpellSchool),
}

/// How a `Patch` rewrites its field. Ranks ADD, for every effect kind: `n`
/// ranks of `Mul(m)` scale by `1 + n·(m − 1)` (two ranks of `Mul(1.15)` take
/// 10 to 13, never 13.2), and every `Mul` on a field sums its bonus the same
/// way. Every `Add` applies before the multiply:
/// `(value + Σ adds) × (1 + Σ ranks·(m − 1))`, whatever order the build lists
/// them in.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum PatchOp {
    Add(f32),
    Mul(f32),
}

/// The numeric fields of an [`AbilityConfig`] a `Patch` may rewrite. Named,
/// never strings; `tests/talents.rs` destructures `AbilityConfig` without a
/// rest pattern, so a new numeric field fails to compile there until it is
/// placed here or declared not patchable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub enum AbilityField {
    CastTime,
    Range,
    /// Present only on an ability with a `min_range`.
    MinRange,
    ManaCost,
    Cooldown,
    DamageBaseMin,
    DamageBaseMax,
    DamageCoefficient,
    HealingBaseMin,
    HealingBaseMax,
    HealingCoefficient,
    /// Present only on an ability with an `application_chance`.
    ApplicationChance,
    /// Present only on a projectile.
    ProjectileSpeed,
    LockoutDuration,
    /// Present only on a channel.
    ChannelDuration,
    ChannelTickInterval,
    ManaBurnAmount,
    /// The applied aura's fields: present only on an ability that applies one.
    AuraDuration,
    AuraMagnitude,
    AuraMagnitudeCoefficient,
    AuraBreakOnDamage,
    AuraTickInterval,
    /// The ability's whole damage: its base range and its coefficient
    /// ([`Self::parts`]), so `Mul(1.06)` makes every hit 6% harder. Takes
    /// only a `Mul`: one number cannot be added to a range and a coefficient
    /// alike.
    Damage,
    /// The ability's own crit chance, added to its caster's for every crit
    /// roll the ability makes (Conviction, Malice, Improved Ambush, Holy
    /// Specialization). `0.0` on every shipped ability.
    CritChance,
    /// The applied aura's STRENGTH: its magnitude, read the way round that
    /// grows with the effect. For an aura whose magnitude is the fraction LEFT
    /// ([`strength_is_remainder`]: a movement slow, a healing cut) the strength
    /// is `1 - magnitude`, so `Mul(1.1)` on Crippling Poison's 70% slow makes
    /// it 77%; for every other aura it is the magnitude itself (Deadly
    /// Poison's damage, Mind-numbing's +60% cast time). Present only on an
    /// ability that applies an aura. Vile Poisons' field.
    AuraStrength,
    /// The damage of the applied aura's periodic ticks: its magnitude and its
    /// spell-power coefficient together ([`Self::parts`]). Present only on an
    /// ability whose aura deals damage over time (Corruption, Curse of Agony,
    /// Siphon Life), so a school-wide patch of it leaves every other aura's
    /// magnitude alone (Curse of Weakness's cut, Curse of Tongues' slowing).
    /// Takes only a `Mul`, as `Damage` does. Shadow Mastery's field, with
    /// `Damage`.
    PeriodicDamage,
    /// The mana a health-to-mana conversion gives per point of health it
    /// takes (Life Tap: 1.0). Present only on an ability that converts.
    /// Improved Life Tap's field.
    ManaPerHealth,
    /// The ability's whole healing: its base range and its coefficient
    /// ([`Self::parts`]), so `Mul(1.05)` makes every heal 5% larger. Takes
    /// only a `Mul`, as `Damage` does. Healing Light's field.
    Healing,
}

impl AbilityField {
    /// The field of `config` this names, or `None` when the ability has no
    /// such field (an optional one it does not set).
    pub fn of(self, config: &mut AbilityConfig) -> Option<&mut f32> {
        let aura = config.applies_aura.as_mut();
        match self {
            AbilityField::CastTime => Some(&mut config.cast_time),
            AbilityField::Range => Some(&mut config.range),
            AbilityField::MinRange => config.min_range.as_mut(),
            AbilityField::ManaCost => Some(&mut config.mana_cost),
            AbilityField::Cooldown => Some(&mut config.cooldown),
            AbilityField::DamageBaseMin => Some(&mut config.damage_base_min),
            AbilityField::DamageBaseMax => Some(&mut config.damage_base_max),
            AbilityField::DamageCoefficient => Some(&mut config.damage_coefficient),
            AbilityField::HealingBaseMin => Some(&mut config.healing_base_min),
            AbilityField::HealingBaseMax => Some(&mut config.healing_base_max),
            AbilityField::HealingCoefficient => Some(&mut config.healing_coefficient),
            AbilityField::ApplicationChance => config.application_chance.as_mut(),
            AbilityField::ProjectileSpeed => config.projectile_speed.as_mut(),
            AbilityField::LockoutDuration => Some(&mut config.lockout_duration),
            AbilityField::ChannelDuration => config.channel_duration.as_mut(),
            AbilityField::ChannelTickInterval => Some(&mut config.channel_tick_interval),
            AbilityField::ManaBurnAmount => Some(&mut config.mana_burn_amount),
            AbilityField::AuraDuration => aura.map(|a| &mut a.duration),
            AbilityField::AuraMagnitude => aura.map(|a| &mut a.magnitude),
            AbilityField::AuraMagnitudeCoefficient => aura.map(|a| &mut a.magnitude_coefficient),
            AbilityField::AuraBreakOnDamage => aura.map(|a| &mut a.break_on_damage),
            AbilityField::AuraTickInterval => aura.map(|a| &mut a.tick_interval),
            // A composite: it names several values, so no one of them. A patch
            // of it resolves onto its `parts`.
            AbilityField::Damage => None,
            AbilityField::CritChance => Some(&mut config.crit_chance),
            // A derived value, not a stored one: read and written through
            // `get` / `set`.
            AbilityField::AuraStrength => None,
            // A composite: patched through its `parts`.
            AbilityField::PeriodicDamage | AbilityField::Healing => None,
            AbilityField::ManaPerHealth => config
                .health_to_mana
                .as_mut()
                .map(|conversion| &mut conversion.mana_per_health),
        }
    }

    /// The value of this field on `config`, or `None` when it has none.
    pub fn get(self, config: &AbilityConfig) -> Option<f32> {
        match self {
            // A composite names several values, so no one of them.
            AbilityField::Damage | AbilityField::PeriodicDamage | AbilityField::Healing => None,
            AbilityField::AuraStrength => config.applies_aura.as_ref().map(|a| {
                if strength_is_remainder(a.aura_type) {
                    1.0 - a.magnitude
                } else {
                    a.magnitude
                }
            }),
            stored => stored.of(&mut config.clone()).map(|v| *v),
        }
    }

    /// Write `value` to this field on `config` (a no-op when it has none).
    pub fn set(self, config: &mut AbilityConfig, value: f32) {
        match self {
            // Clamped: a remainder's strength is at most 1 (a 100% slow), so
            // no patch or power pushes its magnitude below 0; no strength is
            // below 0.
            AbilityField::AuraStrength => {
                if let Some(a) = config.applies_aura.as_mut() {
                    a.magnitude = if strength_is_remainder(a.aura_type) {
                        1.0 - value.clamp(0.0, 1.0)
                    } else {
                        value.max(0.0)
                    };
                }
            }
            // A composite is written through its `parts`, never as one value.
            AbilityField::Damage | AbilityField::PeriodicDamage | AbilityField::Healing => {}
            stored => {
                if let Some(v) = stored.of(config) {
                    *v = value;
                }
            }
        }
    }

    /// The plain fields a patch of this field rewrites: itself, or a
    /// composite's members.
    pub fn parts(self) -> &'static [AbilityField] {
        match self {
            AbilityField::Damage => &[
                AbilityField::DamageBaseMin,
                AbilityField::DamageBaseMax,
                AbilityField::DamageCoefficient,
            ],
            AbilityField::PeriodicDamage => &[
                AbilityField::AuraMagnitude,
                AbilityField::AuraMagnitudeCoefficient,
            ],
            AbilityField::Healing => &[
                AbilityField::HealingBaseMin,
                AbilityField::HealingBaseMax,
                AbilityField::HealingCoefficient,
            ],
            AbilityField::ManaPerHealth => &[AbilityField::ManaPerHealth],
            AbilityField::CastTime => &[AbilityField::CastTime],
            AbilityField::Range => &[AbilityField::Range],
            AbilityField::MinRange => &[AbilityField::MinRange],
            AbilityField::ManaCost => &[AbilityField::ManaCost],
            AbilityField::Cooldown => &[AbilityField::Cooldown],
            AbilityField::DamageBaseMin => &[AbilityField::DamageBaseMin],
            AbilityField::DamageBaseMax => &[AbilityField::DamageBaseMax],
            AbilityField::DamageCoefficient => &[AbilityField::DamageCoefficient],
            AbilityField::HealingBaseMin => &[AbilityField::HealingBaseMin],
            AbilityField::HealingBaseMax => &[AbilityField::HealingBaseMax],
            AbilityField::HealingCoefficient => &[AbilityField::HealingCoefficient],
            AbilityField::ApplicationChance => &[AbilityField::ApplicationChance],
            AbilityField::ProjectileSpeed => &[AbilityField::ProjectileSpeed],
            AbilityField::LockoutDuration => &[AbilityField::LockoutDuration],
            AbilityField::ChannelDuration => &[AbilityField::ChannelDuration],
            AbilityField::ChannelTickInterval => &[AbilityField::ChannelTickInterval],
            AbilityField::ManaBurnAmount => &[AbilityField::ManaBurnAmount],
            AbilityField::AuraDuration => &[AbilityField::AuraDuration],
            AbilityField::AuraMagnitude => &[AbilityField::AuraMagnitude],
            AbilityField::AuraMagnitudeCoefficient => &[AbilityField::AuraMagnitudeCoefficient],
            AbilityField::AuraBreakOnDamage => &[AbilityField::AuraBreakOnDamage],
            AbilityField::AuraTickInterval => &[AbilityField::AuraTickInterval],
            AbilityField::CritChance => &[AbilityField::CritChance],
            AbilityField::AuraStrength => &[AbilityField::AuraStrength],
        }
    }

    /// Whether `config` has this field: a value through [`Self::get`], or,
    /// for a composite, one for every part of it.
    pub fn present_on(self, config: &AbilityConfig) -> bool {
        let dot = || {
            config
                .applies_aura
                .as_ref()
                .is_some_and(|a| a.aura_type == AuraType::DamageOverTime)
        };
        let parts = || self.parts().iter().all(|part| part.get(config).is_some());
        match self {
            // Its parts are any aura's: it is present only where they are a
            // damage over time's.
            AbilityField::PeriodicDamage => dot() && parts(),
            _ => parts(),
        }
    }
}

/// Whether an aura's magnitude is the fraction of something LEFT, so its
/// strength is `1 - magnitude`: a movement slow (0.3 = a 70% slow) and a
/// healing cut (0.65 = 35% less healing). Every other aura's magnitude grows
/// with its effect.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** A new aura type must
/// say which way its magnitude reads, or a talent strengthening it would
/// weaken it.
pub fn strength_is_remainder(aura: AuraType) -> bool {
    match aura {
        AuraType::MovementSpeedSlow | AuraType::HealingReduction => true,
        AuraType::Root
        | AuraType::Stun
        | AuraType::MaxHealthIncrease
        | AuraType::DamageOverTime
        | AuraType::SpellSchoolLockout
        | AuraType::Fear
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::ShadowSight
        | AuraType::Absorb
        | AuraType::WeakenedSoul
        | AuraType::Polymorph
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::Incapacitate
        | AuraType::SpellResistanceBuff
        | AuraType::AttackPowerReduction
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::AttackSpeedSlow
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::Silence
        | AuraType::WeaponPoison
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::WindfuryBuff
        | AuraType::FearImmunity
        | AuraType::SilenceInterruptImmunity
        | AuraType::ArmorIncrease
        | AuraType::Cyclone
        | AuraType::TravelForm
        | AuraType::AttackSpeedIncrease
        | AuraType::NextCast
        | AuraType::DamageTakenIncrease
        | AuraType::DamageDealtIncrease
        | AuraType::ArmorPercentIncrease
        | AuraType::ArmorPercentReduction
        | AuraType::DamageTransfer
        | AuraType::KillingBlowSave
        | AuraType::DamageReflect
        | AuraType::TriggeredAbsorb => false,
    }
}

/// What one rank of a node does.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum TalentEffect {
    /// A percentage of one of the combatant's totals, gear included. Every
    /// rank adds its fraction: two ranks of +3% are +6% of the total.
    StatPercent(TalentStat, f32),
    /// The same, on the combatant's pet, after the pet takes its share of
    /// its owner's stats.
    PetStatPercent(TalentStat, f32),
    /// Points added to one of the combatant's totals, as an item's stat adds
    /// them, before any `StatPercent` takes its percentage. Every rank adds
    /// its points: two ranks of +1% crit chance are +2%. Every roll that
    /// reads the total reads them (crit: every spell, ability and auto-attack).
    StatAdd(TalentStat, f32),
    /// Rewrites a numeric field of an ability in the combatant's book.
    Patch {
        on: PatchTarget,
        field: AbilityField,
        op: PatchOp,
    },
    /// Adds a `talent: true` ability to the combatant's book.
    Grant(AbilityType),
    /// A trigger and an effect on the combatant (`talent_procs.rs`). Its
    /// ranks add to its chance or to its effect, as it declares.
    Proc(TalentProc),
    /// Makes a per-match option selectable that is hidden without this node
    /// (`talent_choices`): `WidenChoice(WarlockCurse(Tongues))`.
    WidenChoice(ChoiceOption),
    /// Opens a second slot of a per-match menu (`talent_choices`).
    AddChoiceSlot(ChoiceSlot),
    /// A bespoke rule with one code site (`talent_mechanics`).
    Mechanic(TalentMechanic),
    /// `effect`, applied once per rank from this rank on: rank 1 of a node
    /// adds an option and rank 2 patches what it added (Retribution Aura:
    /// `[WidenChoice(..), FromRank(2, Patch(..))]`).
    FromRank(u8, Box<TalentEffect>),
}

impl TalentEffect {
    /// Whether this effect is priced by the item budget's weights: a
    /// `StatPercent` or a `StatAdd` on a weighted stat.
    fn is_computed(&self) -> bool {
        matches!(
            self,
            TalentEffect::StatPercent(stat, _) | TalentEffect::StatAdd(stat, _)
                if stat.item_weight().is_some()
        )
    }
}

/// One talent node, as `talents.ron` writes it.
#[derive(Debug, Clone, Deserialize)]
pub struct TalentConfig {
    pub name: String,
    pub class: CharacterClass,
    pub tree: TreeId,
    /// 1 to 4; 4 is the capstone.
    pub tier: u8,
    /// 0 to 2, the node's place in its tier.
    pub column: u8,
    pub icon: String,
    pub max_rank: u8,
    /// The price of one rank, for a node whose price is not computed. `None`
    /// exactly when every effect is a `StatPercent` or a `StatAdd` on a
    /// weighted stat.
    #[serde(default)]
    pub budget: Option<f32>,
    /// A node of the same tree, at a lower tier, and the rank it needs.
    #[serde(default)]
    pub requires: Option<(TalentId, u8)>,
    /// What each rank does, applied once per rank.
    pub per_rank: Vec<TalentEffect>,
}

impl TalentConfig {
    /// Whether the node's price is computed from the item budget's weights
    /// rather than declared.
    pub fn is_computed(&self) -> bool {
        self.per_rank.iter().all(TalentEffect::is_computed)
    }

    /// The price a rank of this node is read against: the capstone's or a
    /// rank's reference.
    pub fn reference_price(&self) -> f32 {
        if self.tier == CAPSTONE_TIER {
            REFERENCE_CAPSTONE_PRICE
        } else {
            REFERENCE_RANK_PRICE
        }
    }

    /// Whether `price`, one rank of this node, is sane: finite, positive and
    /// within [`PRICE_SANITY_FACTOR`] of its [`reference_price`] either way.
    /// The only failure a price can cause; any sane price is the design's.
    ///
    /// [`reference_price`]: Self::reference_price
    pub fn price_sanity(&self, price: f32) -> Result<(), String> {
        let reference = self.reference_price();
        let (low, high) = (
            reference / PRICE_SANITY_FACTOR,
            reference * PRICE_SANITY_FACTOR,
        );
        if !price.is_finite() || price <= 0.0 {
            return Err(format!("price {price} is not a positive number"));
        }
        if price < low || price > high {
            return Err(format!(
                "price {price:.2} is outside {low:.2} to {high:.2} \
                 ({PRICE_SANITY_FACTOR}x either way of {reference})"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct TalentsFile {
    trees: BTreeMap<CharacterClass, Vec<TreeConfig>>,
    talents: BTreeMap<TalentId, TalentConfig>,
    #[serde(default)]
    reference: BTreeMap<CharacterClass, Loadout>,
    #[serde(default)]
    choice_rules: Vec<ChoiceRule>,
}

/// A combatant's talents: a rank for each node it took. A node it did not
/// take has no entry (a rank of 0 is not written). Ordered, so everything a
/// build sums it sums in one order (the AS-58 rule).
///
/// Serializes as the bare map, the shape a headless config writes:
/// `{"DemonicEmbrace": 2, "FelStamina": 2}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Build {
    pub ranks: BTreeMap<TalentId, u8>,
}

impl Build {
    /// A build from `(talent, rank)` pairs.
    pub fn of(ranks: &[(TalentId, u8)]) -> Self {
        Self {
            ranks: ranks.iter().copied().collect(),
        }
    }

    /// Whether the build takes no talent at all: the default, which changes
    /// nothing.
    pub fn is_empty(&self) -> bool {
        self.ranks.is_empty()
    }

    /// The points the build spends.
    pub fn points(&self) -> u32 {
        self.ranks.values().map(|r| u32::from(*r)).sum()
    }

    /// The rank taken in `talent` (0 when not taken).
    pub fn rank(&self, talent: TalentId) -> u8 {
        self.ranks.get(&talent).copied().unwrap_or(0)
    }
}

/// The talent table: every class's trees and their nodes, loaded from
/// `assets/config/talents.ron` and validated against the ability table.
#[derive(Resource, Debug, Clone)]
pub struct TalentDefinitions {
    trees: BTreeMap<CharacterClass, Vec<TreeConfig>>,
    talents: BTreeMap<TalentId, TalentConfig>,
    reference: BTreeMap<CharacterClass, Loadout>,
    choice_rules: Vec<ChoiceRule>,
}

/// Load and validate `assets/config/talents.ron` against `abilities`.
pub fn load_talent_definitions(
    abilities: &AbilityDefinitions,
) -> Result<TalentDefinitions, String> {
    let path = crate::paths::asset_path_str("config/talents.ron");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    TalentDefinitions::from_ron(&text, abilities).map_err(|e| format!("{path}: {e}"))
}

impl TalentDefinitions {
    /// Parse and validate a talents file against `abilities`.
    pub fn from_ron(text: &str, abilities: &AbilityDefinitions) -> Result<Self, String> {
        let file: TalentsFile = ron::from_str(text).map_err(|e| format!("parse error: {e}"))?;
        let defs = Self {
            trees: file.trees,
            talents: file.talents,
            reference: file.reference,
            choice_rules: file.choice_rules,
        };
        defs.validate(abilities)?;
        Ok(defs)
    }

    /// The shipped table, loaded once a process against the shipped ability
    /// table: what a config validates its builds against before it has a
    /// match. Panics, like [`AbilityDefinitions::shipped`], if it cannot load.
    pub fn shipped() -> &'static TalentDefinitions {
        static SHIPPED: std::sync::OnceLock<TalentDefinitions> = std::sync::OnceLock::new();
        SHIPPED.get_or_init(|| {
            load_talent_definitions(AbilityDefinitions::shipped())
                .unwrap_or_else(|e| panic!("Failed to load talent definitions: {e}"))
        })
    }

    /// The node `talent`.
    pub fn get(&self, talent: TalentId) -> Option<&TalentConfig> {
        self.talents.get(&talent)
    }

    /// Every node, in `TalentId` order.
    pub fn iter(&self) -> impl Iterator<Item = (TalentId, &TalentConfig)> {
        self.talents.iter().map(|(id, t)| (*id, t))
    }

    /// The nodes of `class`, in `TalentId` order.
    pub fn talents_for_class(
        &self,
        class: CharacterClass,
    ) -> impl Iterator<Item = (TalentId, &TalentConfig)> {
        self.iter().filter(move |(_, t)| t.class == class)
    }

    /// The trees of `class`, in display order (empty for a class with none).
    pub fn trees(&self, class: CharacterClass) -> &[TreeConfig] {
        self.trees.get(&class).map_or(&[], Vec::as_slice)
    }

    /// The sockets `class`'s reference combatant fills with its three
    /// Gauntlet picks, over its starting kit.
    pub fn reference_picks(&self, class: CharacterClass) -> Option<&Loadout> {
        self.reference.get(&class)
    }

    /// Every class with a reference combatant.
    pub fn reference_classes(&self) -> impl Iterator<Item = CharacterClass> + '_ {
        self.reference.keys().copied()
    }

    /// This table with only the nodes `keep` names, and only the trees that
    /// still hold one: a catalogue whose membership does not grow when
    /// `talents.ron` does. The encyclopedia snapshots draw one
    /// (`tests/common/pinned_catalogue.rs`), so a node or a tree added later
    /// moves none of their pixels. Not validated again: it is for drawing,
    /// never for resolving a build.
    pub fn restricted_to(&self, keep: &[TalentId]) -> Self {
        let talents: BTreeMap<TalentId, TalentConfig> = keep
            .iter()
            .filter_map(|id| self.talents.get(id).map(|node| (*id, node.clone())))
            .collect();
        let trees = self
            .trees
            .iter()
            .map(|(class, trees)| {
                let kept: Vec<TreeConfig> = trees
                    .iter()
                    .filter(|tree| {
                        talents
                            .values()
                            .any(|node| node.class == *class && node.tree == tree.id)
                    })
                    .cloned()
                    .collect();
                (*class, kept)
            })
            .filter(|(_, kept)| !kept.is_empty())
            .collect();
        Self {
            trees,
            talents,
            reference: self.reference.clone(),
            choice_rules: self.choice_rules.clone(),
        }
    }

    /// The choice rules (`talent_choices::ChoiceRule`).
    pub(super) fn rules(&self) -> &[ChoiceRule] {
        &self.choice_rules
    }

    fn validate(&self, abilities: &AbilityDefinitions) -> Result<(), String> {
        let missing: Vec<&str> = TalentId::all()
            .iter()
            .filter(|id| !self.talents.contains_key(id))
            .map(TalentId::as_str)
            .collect();
        if !missing.is_empty() {
            return Err(format!("no entry for {}", missing.join(", ")));
        }

        for (class, trees) in &self.trees {
            let mut seen = BTreeSet::new();
            for tree in trees {
                if !seen.insert(tree.id) {
                    return Err(format!("{class:?} lists the tree {:?} twice", tree.id));
                }
                if tree.icon.is_empty() {
                    return Err(format!("{class:?}'s {:?} tree has no icon", tree.id));
                }
            }
        }

        let mut positions = BTreeSet::new();
        for (id, talent) in &self.talents {
            let at = |problem: String| format!("{id:?}: {problem}");
            if !self.trees(talent.class).iter().any(|t| t.id == talent.tree) {
                return Err(at(format!(
                    "its tree {:?} is not one of {:?}'s",
                    talent.tree, talent.class
                )));
            }
            if !(1..=CAPSTONE_TIER).contains(&talent.tier) {
                return Err(at(format!("tier {} is not 1 to 4", talent.tier)));
            }
            if talent.column > 2 {
                return Err(at(format!("column {} is not 0 to 2", talent.column)));
            }
            if !positions.insert((talent.class, talent.tree, talent.tier, talent.column)) {
                return Err(at(format!(
                    "another node of {:?}'s {:?} tree sits at tier {} column {}",
                    talent.class, talent.tree, talent.tier, talent.column
                )));
            }
            let rank_cap = if talent.tier <= 2 { 2 } else { 1 };
            if talent.max_rank == 0 || talent.max_rank > rank_cap {
                return Err(at(format!(
                    "max_rank {} at tier {} (a tier-{} node has 1 to {rank_cap})",
                    talent.max_rank, talent.tier, talent.tier
                )));
            }
            if talent.icon.is_empty() {
                return Err(at("has no icon".to_string()));
            }
            if talent.per_rank.is_empty() {
                return Err(at("does nothing: per_rank is empty".to_string()));
            }
            if let Some((required, rank)) = talent.requires {
                let Some(req) = self.talents.get(&required) else {
                    return Err(at(format!("requires {required:?}, which has no entry")));
                };
                if req.class != talent.class || req.tree != talent.tree {
                    return Err(at(format!("requires {required:?}, from another tree")));
                }
                if req.tier >= talent.tier {
                    return Err(at(format!(
                        "requires {required:?} at tier {}, not below its own tier {}",
                        req.tier, talent.tier
                    )));
                }
                if rank == 0 || rank > req.max_rank {
                    return Err(at(format!(
                        "requires {required:?} at rank {rank}, which has 1 to {}",
                        req.max_rank
                    )));
                }
            }
            match (talent.is_computed(), talent.budget) {
                (true, Some(_)) => {
                    return Err(at(
                        "declares a budget, but its price is computed: every effect is a \
                         StatPercent or a StatAdd on a stat the item budget weighs"
                            .to_string(),
                    ))
                }
                (false, None) => {
                    return Err(at(
                        "declares no budget: only a node of StatPercent and StatAdd effects \
                         on weighted stats has a computed price"
                            .to_string(),
                    ))
                }
                (false, Some(b)) if !b.is_finite() || b <= 0.0 => {
                    return Err(at(format!("budget {b} is not positive")))
                }
                _ => {}
            }
            for effect in &talent.per_rank {
                self.validate_effect(talent, effect, abilities)
                    .map_err(at)?;
            }
        }

        self.validate_choice_shapes(abilities)?;

        // Each talent ability is granted by exactly one node.
        for ability in abilities.talent_abilities() {
            let granting: Vec<TalentId> = self
                .iter()
                .filter(|(_, t)| t.per_rank.contains(&TalentEffect::Grant(ability)))
                .map(|(id, _)| id)
                .collect();
            if granting.len() != 1 {
                return Err(format!(
                    "the talent ability {ability:?} is granted by {granting:?}: exactly one \
                     node grants each"
                ));
            }
        }
        Ok(())
    }

    fn validate_effect(
        &self,
        talent: &TalentConfig,
        effect: &TalentEffect,
        abilities: &AbilityDefinitions,
    ) -> Result<(), String> {
        match effect {
            TalentEffect::StatPercent(stat, fraction)
            | TalentEffect::PetStatPercent(stat, fraction) => {
                if !fraction.is_finite() || *fraction <= 0.0 {
                    return Err(format!("a stat percentage of {fraction} is not positive"));
                }
                if stat.takes_points_only() {
                    let why = match stat {
                        TalentStat::MovementSpeed => "whose points are already a percentage",
                        _ => "which every class has at 0",
                    };
                    return Err(format!(
                        "takes a percentage of {stat:?}, {why}: a StatAdd adds it in points"
                    ));
                }
                if matches!(effect, TalentEffect::PetStatPercent(..)) && !has_pet(talent.class) {
                    return Err(format!("{:?} has no pet", talent.class));
                }
            }
            TalentEffect::StatAdd(_, points) => {
                if !points.is_finite() || *points <= 0.0 {
                    return Err(format!("adds {points} points, which is not positive"));
                }
            }
            TalentEffect::Grant(ability) => {
                let Some(def) = abilities.get(ability) else {
                    return Err(format!("grants {ability:?}, which abilities.ron lacks"));
                };
                if !def.talent {
                    return Err(format!(
                        "grants {ability:?}, which is not marked `talent: true`"
                    ));
                }
                if def.class != talent.class {
                    return Err(format!(
                        "grants {ability:?}, a {:?} ability, to a {:?}",
                        def.class, talent.class
                    ));
                }
                if talent.max_rank != 1 {
                    return Err("a node that grants an ability has one rank".to_string());
                }
            }
            TalentEffect::Proc(proc) => {
                let reachable = self.reachable_abilities(talent.class, abilities);
                proc.validate(talent.max_rank, &reachable)?;
            }
            TalentEffect::WidenChoice(option) => {
                if option.menu().class() != talent.class {
                    return Err(format!(
                        "widens {option:?}, not a {:?} choice",
                        talent.class
                    ));
                }
                if option.is_default() {
                    return Err(format!(
                        "widens {option:?}, its menu's default, which is never hidden"
                    ));
                }
            }
            TalentEffect::AddChoiceSlot(slot) => {
                if slot.menu().class() != talent.class {
                    return Err(format!("opens {slot:?}, not a {:?} slot", talent.class));
                }
            }
            TalentEffect::Mechanic(mechanic) => {
                mechanic.validate(talent.class, talent.max_rank)?;
            }
            TalentEffect::FromRank(rank, inner) => {
                if *rank < 2 || *rank > talent.max_rank {
                    return Err(format!(
                        "FromRank({rank}, ..) on a node of {} ranks: it starts at 2 to {}",
                        talent.max_rank, talent.max_rank
                    ));
                }
                if matches!(
                    inner.as_ref(),
                    TalentEffect::FromRank(..) | TalentEffect::Grant(_)
                ) {
                    return Err(format!("FromRank({rank}, ..) wraps {inner:?}"));
                }
                self.validate_effect(talent, inner, abilities)?;
            }
            TalentEffect::Patch { on, field, op } => {
                match op {
                    PatchOp::Add(n) if !n.is_finite() => {
                        return Err(format!("adds {n}, which is not finite"))
                    }
                    PatchOp::Mul(n) if !n.is_finite() || *n <= 0.0 => {
                        return Err(format!("multiplies by {n}, which is not positive"))
                    }
                    PatchOp::Add(_) if field.parts().len() > 1 => {
                        return Err(format!(
                            "adds to {field:?}, which names several values: it takes only Mul"
                        ))
                    }
                    _ => {}
                }
                let reachable = self.reachable_abilities(talent.class, abilities);
                let targets: Vec<AbilityType> = reachable
                    .iter()
                    .copied()
                    .filter(|a| patch_reaches(*on, *a, abilities.get_unchecked(a)))
                    .collect();
                if let PatchTarget::Ability(ability) = on {
                    if !reachable.contains(ability) {
                        return Err(format!(
                            "patches {ability:?}, which no {:?} can have",
                            talent.class
                        ));
                    }
                }
                let with_field: Vec<AbilityType> = targets
                    .iter()
                    .copied()
                    .filter(|a| field.present_on(abilities.get_unchecked(a)))
                    .collect();
                if with_field.is_empty() {
                    return Err(format!(
                        "patches {field:?} on {on:?}, and no {:?} ability there has it",
                        talent.class
                    ));
                }
                if *field == AbilityField::Cooldown {
                    if let Some(a) = with_field
                        .iter()
                        .find(|a| abilities.get_unchecked(a).cooldown_category.is_some())
                    {
                        return Err(format!(
                            "patches the cooldown of {a:?}, a cooldown category's member: a \
                             category has one cooldown"
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Every ability a combatant of `class` can have: its base abilities and
    /// every ability one of its talents grants.
    pub(super) fn reachable_abilities(
        &self,
        class: CharacterClass,
        abilities: &AbilityDefinitions,
    ) -> BTreeSet<AbilityType> {
        let mut all: BTreeSet<AbilityType> = abilities
            .base_abilities_for_class(class)
            .into_iter()
            .collect();
        for (_, talent) in self.talents_for_class(class) {
            for effect in &talent.per_rank {
                if let TalentEffect::Grant(ability) = effect {
                    all.insert(*ability);
                }
            }
        }
        all
    }

    /// Whether `build` is a legal build for a combatant of `class`: every
    /// node is the class's, at a rank it has; the build spends at most
    /// [`MAX_TALENT_POINTS`]; every node's tier is open (its tree holds
    /// [`TIER_OPENS_AT`] points in the tiers below it); every prerequisite is
    /// met.
    pub fn validate_build(&self, class: CharacterClass, build: &Build) -> Result<(), String> {
        for (id, rank) in &build.ranks {
            let Some(talent) = self.talents.get(id) else {
                return Err(format!("{id:?} has no entry in talents.ron"));
            };
            if talent.class != class {
                return Err(format!(
                    "{id:?} is a {:?} talent, not a {class:?}'s",
                    talent.class
                ));
            }
            if *rank == 0 || *rank > talent.max_rank {
                return Err(format!(
                    "{id:?} at rank {rank}: it has ranks 1 to {}",
                    talent.max_rank
                ));
            }
        }
        if build.points() > MAX_TALENT_POINTS {
            return Err(format!(
                "the build spends {} points; a build spends at most {MAX_TALENT_POINTS}",
                build.points()
            ));
        }
        for id in build.ranks.keys() {
            let talent = &self.talents[id];
            let below: u32 = build
                .ranks
                .iter()
                .filter(|(other, _)| {
                    let o = &self.talents[other];
                    o.tree == talent.tree && o.tier < talent.tier
                })
                .map(|(_, r)| u32::from(*r))
                .sum();
            let needed = TIER_OPENS_AT[usize::from(talent.tier - 1)];
            if below < needed {
                return Err(format!(
                    "{id:?} is in tier {}, which opens at {needed} points in the tiers below \
                     it; the build has {below} there",
                    talent.tier
                ));
            }
            if let Some((required, rank)) = talent.requires {
                if build.rank(required) < rank {
                    return Err(format!("{id:?} requires {required:?} at rank {rank}"));
                }
            }
        }
        Ok(())
    }

    /// Resolve a legal `build` for a combatant of `class`: its book, with
    /// every grant and patch in, and the stat percentages its spawn applies.
    /// An empty build resolves to the untalented book and no stats.
    pub fn resolve(
        &self,
        abilities: &AbilityDefinitions,
        class: CharacterClass,
        build: &Build,
    ) -> Result<ResolvedBuild, String> {
        self.resolve_with_choices(abilities, class, build, &SlotChoices::default())
    }

    /// [`Self::resolve`] for a slot that made `choices`: the choices are
    /// checked against the build ([`Self::validate_choices`]), a talent they
    /// switch off contributes nothing, and the abilities they lock are locked
    /// in the book (`talent_choices`).
    pub fn resolve_with_choices(
        &self,
        abilities: &AbilityDefinitions,
        class: CharacterClass,
        build: &Build,
        choices: &SlotChoices,
    ) -> Result<ResolvedBuild, String> {
        self.validate_build(class, build)?;
        self.validate_choices(class, build, choices)?;
        let switched_off = self.switched_off(choices);
        let mut mechanics = TalentMechanics::default();
        let mut book = AbilityBook::for_class(abilities, class);
        let mut stats: BTreeMap<TalentStat, StatChange> = BTreeMap::new();
        let mut pet_stats: BTreeMap<TalentStat, StatChange> = BTreeMap::new();
        // (ability, field) -> (Σ adds, Σ multiply bonuses), over every rank in
        // build order. Ranks add, for a Mul as for everything else.
        let mut patches: BTreeMap<(AbilityType, AbilityField), (f32, f32)> = BTreeMap::new();
        let mut patch_effects: Vec<(PatchTarget, AbilityField, PatchOp, u8)> = Vec::new();
        let mut procs: Vec<TalentProcSlot> = Vec::new();

        for (id, rank) in &build.ranks {
            if switched_off.contains(id) {
                continue;
            }
            for (effect, rank) in ranked_effects(&self.talents[id].per_rank, *rank) {
                match effect {
                    TalentEffect::Grant(ability) => {
                        book = book.granting(abilities, *ability);
                    }
                    TalentEffect::StatPercent(stat, fraction) => {
                        stats.entry(*stat).or_default().fraction += fraction * f32::from(rank);
                    }
                    TalentEffect::StatAdd(stat, points) => {
                        stats.entry(*stat).or_default().points += points * f32::from(rank);
                    }
                    TalentEffect::PetStatPercent(stat, fraction) => {
                        pet_stats.entry(*stat).or_default().fraction += fraction * f32::from(rank);
                    }
                    TalentEffect::Patch { on, field, op } => {
                        patch_effects.push((*on, *field, *op, rank));
                    }
                    TalentEffect::Proc(proc) => {
                        procs.push(TalentProcSlot::new(
                            *id,
                            &self.talents[id].name,
                            proc.at_rank(rank),
                        ));
                    }
                    TalentEffect::Mechanic(mechanic) => {
                        mechanics.insert(mechanic.at_rank(rank));
                    }
                    // Read from the build by `offers` / `opens`, not resolved
                    // into the combatant.
                    TalentEffect::WidenChoice(_) | TalentEffect::AddChoiceSlot(_) => {}
                    // `ranked_effects` unwraps these.
                    TalentEffect::FromRank(..) => unreachable!("ranked_effects unwraps FromRank"),
                }
            }
        }

        // Patches resolve against the book with every grant in, so a patch
        // reaches a granted ability.
        for (on, field, op, rank) in patch_effects {
            let targets: Vec<AbilityType> = book
                .iter()
                .filter(|(a, c)| patch_reaches(on, **a, c) && field.present_on(c))
                .map(|(a, _)| *a)
                .collect();
            for ability in targets {
                for part in field.parts() {
                    let entry = patches.entry((ability, *part)).or_insert((0.0, 0.0));
                    let ranks = f32::from(rank);
                    match op {
                        PatchOp::Add(n) => entry.0 += n * ranks,
                        PatchOp::Mul(n) => entry.1 += (n - 1.0) * ranks,
                    }
                }
            }
        }
        for ((ability, field), (add, bonus)) in patches {
            let mul = 1.0 + bonus;
            let mut negative = None;
            book = book.patching(ability, |config| {
                let value = field.get(config).expect("a patch's targets have its field");
                let patched = (value + add) * mul;
                field.set(config, patched);
                if patched < 0.0 {
                    negative = Some(patched);
                }
            });
            if let Some(value) = negative {
                return Err(format!(
                    "the build patches {ability:?}'s {field:?} to {value}, below zero"
                ));
            }
        }

        let locked = self.locked_by(choices);
        if !locked.is_empty() {
            book = book.locking(&locked);
        }

        Ok(ResolvedBuild {
            book,
            build: build.clone(),
            stats,
            pet_stats,
            procs,
            mechanics,
            offhand_poison: match choices.second(ChoiceSlot::OffHandPoison) {
                Some(ChoiceOption::RoguePoison(poison)) => Some(poison),
                _ => None,
            },
            sacrifices_pet: choices.chose(ChoiceOption::WarlockDemon(
                crate::states::match_config::WarlockDemon::Sacrifice,
            )),
        })
    }

    /// Every legal build of `class` spending exactly `points`, in a fixed
    /// order. The build-parity test's enumeration, and what a talent panel
    /// can offer.
    pub fn legal_builds(&self, class: CharacterClass, points: u32) -> Vec<Build> {
        let nodes: Vec<(TalentId, u8)> = self
            .talents_for_class(class)
            .map(|(id, t)| (id, t.max_rank))
            .collect();
        let mut out = Vec::new();
        let mut current = Build::default();
        self.enumerate(class, &nodes, 0, points, &mut current, &mut out);
        out
    }

    fn enumerate(
        &self,
        class: CharacterClass,
        nodes: &[(TalentId, u8)],
        index: usize,
        remaining: u32,
        current: &mut Build,
        out: &mut Vec<Build>,
    ) {
        if remaining == 0 {
            if self.validate_build(class, current).is_ok() {
                out.push(current.clone());
            }
            return;
        }
        let Some(&(id, max_rank)) = nodes.get(index) else {
            return;
        };
        for rank in 0..=u32::from(max_rank).min(remaining) {
            if rank > 0 {
                current.ranks.insert(id, rank as u8);
            }
            self.enumerate(class, nodes, index + 1, remaining - rank, current, out);
            current.ranks.remove(&id);
        }
    }

    /// The reference combatant of `class` at mid-run: its starting kit with
    /// its three Gauntlet picks in place, as a spawn equips it. What a
    /// computed price is read from.
    pub fn reference_combatant(
        &self,
        class: CharacterClass,
        abilities: &AbilityDefinitions,
        items: &ItemDefinitions,
        loadouts: &DefaultLoadouts,
    ) -> Result<Combatant, String> {
        let picks = self
            .reference
            .get(&class)
            .ok_or_else(|| format!("{class:?} has no reference combatant in talents.ron"))?;
        if picks.len() != 3 {
            return Err(format!(
                "{class:?}'s reference combatant names {} picks; a mid-run combatant has three",
                picks.len()
            ));
        }
        validate_class_restrictions(class, picks, items)
            .map_err(|e| format!("{class:?}'s reference combatant: {e}"))?;
        for (slot, item) in picks {
            let tier = items.get(item).map_or(0, |i| i.item_tier);
            if tier == 0 {
                return Err(format!(
                    "{class:?}'s reference pick {item:?} in {slot:?} is starting-kit gear, not \
                     a Gauntlet pick"
                ));
            }
        }
        let loadout = resolve_equipped_loadout(class, loadouts, picks, items);
        let mut combatant =
            Combatant::new_with_book(1, 0, class, AbilityBook::for_class(abilities, class));
        combatant.apply_equipment(&loadout, items);
        Ok(combatant)
    }

    /// The price of one rank of `talent`: its declared budget, or — for a
    /// node of `StatPercent` and `StatAdd` effects on weighted stats — the
    /// item budget's weight for each stat times what the rank adds of it: a
    /// `StatAdd`'s points, as an item's stat is priced, and a `StatPercent`'s
    /// percentage of `reference`'s total. `reference` is the class's
    /// [`reference_combatant`]; a node with a declared budget does not read
    /// it, and a node of `StatAdd` effects alone prices the same at any.
    ///
    /// [`reference_combatant`]: Self::reference_combatant
    pub fn rank_price(
        &self,
        talent: TalentId,
        reference: Option<&Combatant>,
    ) -> Result<f32, String> {
        let node = self
            .talents
            .get(&talent)
            .ok_or_else(|| format!("{talent:?} has no entry"))?;
        if let Some(budget) = node.budget {
            return Ok(budget);
        }
        let reference = reference.ok_or_else(|| {
            format!("{talent:?} has a computed price and needs its class's reference combatant")
        })?;
        Ok(node
            .per_rank
            .iter()
            .map(|effect| match effect {
                TalentEffect::StatPercent(stat, fraction) => {
                    stat.item_weight().unwrap_or(0.0) * fraction * stat.total(reference)
                }
                TalentEffect::StatAdd(stat, points) => stat.item_weight().unwrap_or(0.0) * points,
                _ => 0.0,
            })
            .sum())
    }

    /// A build as the `[TALENTS]` log line writes it: each node with its rank
    /// out of its ranks, in tree, tier and column order, then the points in
    /// each tree in display order ("Demonic Embrace 2/2, Fel Stamina 1/2
    /// (Demonology 3)").
    pub fn describe(&self, class: CharacterClass, build: &Build) -> String {
        let trees = self.trees(class);
        let tree_index = |tree: TreeId| trees.iter().position(|t| t.id == tree).unwrap_or(0);
        let mut nodes: Vec<(TalentId, u8, &TalentConfig)> = build
            .ranks
            .iter()
            .filter_map(|(id, rank)| self.talents.get(id).map(|t| (*id, *rank, t)))
            .collect();
        nodes.sort_by_key(|(_, _, t)| (tree_index(t.tree), t.tier, t.column));
        let listed: Vec<String> = nodes
            .iter()
            .map(|(_, rank, t)| format!("{} {}/{}", t.name, rank, t.max_rank))
            .collect();
        let per_tree: Vec<String> = trees
            .iter()
            .map(|tree| {
                let points: u32 = nodes
                    .iter()
                    .filter(|(_, _, t)| t.tree == tree.id)
                    .map(|(_, r, _)| u32::from(*r))
                    .sum();
                format!("{} {points}", tree.name)
            })
            .collect();
        format!("{} ({})", listed.join(", "), per_tree.join(", "))
    }

    /// The points `build` spends in each of `class`'s trees, in display
    /// order (WoW's "2/6/0" reads in it).
    pub fn tree_points(&self, class: CharacterClass, build: &Build) -> Vec<(TreeId, u32)> {
        self.trees(class)
            .iter()
            .map(|tree| {
                let points = build
                    .ranks
                    .iter()
                    .filter(|(id, _)| self.talents.get(id).is_some_and(|t| t.tree == tree.id))
                    .map(|(_, rank)| u32::from(*rank))
                    .sum();
                (tree.id, points)
            })
            .collect()
    }

    /// The points `build` spends in the tiers of `tree` below `tier`: what
    /// opens `tier` ([`TIER_OPENS_AT`]).
    pub fn points_below(&self, tree: TreeId, tier: u8, build: &Build) -> u32 {
        build
            .ranks
            .iter()
            .filter(|(id, _)| {
                self.talents
                    .get(id)
                    .is_some_and(|t| t.tree == tree && t.tier < tier)
            })
            .map(|(_, rank)| u32::from(*rank))
            .sum()
    }

    /// The spec a viewer reads off `build`: the tree with the most points
    /// names it ("Demonology Warlock"), and WoW's shorthand gives the points
    /// in every tree in display order ("2/6/0"). Two trees tied for the most
    /// read as a hybrid. `None` for an empty build, or a class without trees.
    pub fn spec(&self, class: CharacterClass, build: &Build) -> Option<Spec> {
        let named: Vec<(&str, u32)> = self
            .tree_points(class, build)
            .into_iter()
            .map(|(tree, points)| {
                let name = self
                    .trees(class)
                    .iter()
                    .find(|t| t.id == tree)
                    .map_or("", |t| t.name.as_str());
                (name, points)
            })
            .collect();
        Spec::from_tree_points(class, &named)
    }

    /// Why `build` cannot take one more rank of `talent` for a combatant of
    /// `class`, or `None` when it can. The same rules as
    /// [`validate_build`](Self::validate_build), answered in terms a talent
    /// panel can show: `None` exactly when the build with that rank added is
    /// legal (`tests/talent_panel.rs` holds the two to each other over every
    /// legal build).
    pub fn learn_blocker(
        &self,
        class: CharacterClass,
        build: &Build,
        talent: TalentId,
    ) -> Option<LearnBlocker> {
        let Some(node) = self.talents.get(&talent).filter(|t| t.class == class) else {
            return Some(LearnBlocker::NotThisClass);
        };
        if build.rank(talent) >= node.max_rank {
            return Some(LearnBlocker::MaxRank);
        }
        if build.points() >= MAX_TALENT_POINTS {
            return Some(LearnBlocker::NoPoints);
        }
        let needed = TIER_OPENS_AT[usize::from(node.tier - 1)];
        let has = self.points_below(node.tree, node.tier, build);
        if has < needed {
            return Some(LearnBlocker::TierClosed {
                tree: node.tree,
                needed,
                has,
            });
        }
        if let Some((required, rank)) = node.requires {
            if build.rank(required) < rank {
                return Some(LearnBlocker::Requires {
                    talent: required,
                    rank,
                });
            }
        }
        None
    }

    /// `build` with one more rank of `talent`, when that build is legal.
    pub fn learn(
        &self,
        class: CharacterClass,
        build: &Build,
        talent: TalentId,
    ) -> Result<Build, LearnBlocker> {
        if let Some(blocker) = self.learn_blocker(class, build, talent) {
            return Err(blocker);
        }
        let mut next = build.clone();
        *next.ranks.entry(talent).or_insert(0) += 1;
        Ok(next)
    }

    /// `build` with one rank of `talent` taken back, when the build left is
    /// legal: a rank a higher tier's gate or a prerequisite still leans on
    /// stays. `None` when it cannot be taken back.
    pub fn unlearn(&self, class: CharacterClass, build: &Build, talent: TalentId) -> Option<Build> {
        let rank = build.rank(talent);
        if rank == 0 {
            return None;
        }
        let mut next = build.clone();
        if rank == 1 {
            next.ranks.remove(&talent);
        } else {
            next.ranks.insert(talent, rank - 1);
        }
        self.validate_build(class, &next).ok().map(|()| next)
    }
}

/// A build's spec, as the team frames, Results and the talent panel name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// "Demonology Warlock", or "Affliction-Demonology hybrid" for a tie.
    pub label: String,
    /// The points in each tree, in display order: "2/6/0".
    pub short: String,
}

impl Spec {
    /// The spec named by the points in each of `class`'s trees, given in
    /// display order as `(tree name, points)`: the tree with the most points
    /// names it, two trees tied for the most read as a hybrid of the two
    /// ("Affliction-Demonology hybrid", in display order), and three or more
    /// level at the top as a plain "Hybrid Warlock". `None` when no point is
    /// spent.
    pub fn from_tree_points(class: CharacterClass, trees: &[(&str, u32)]) -> Option<Spec> {
        let total: u32 = trees.iter().map(|(_, p)| p).sum();
        if total == 0 {
            return None;
        }
        let mut ranked: Vec<(&str, u32)> = trees.to_vec();
        // Most points first; a tie keeps display order (the sort is stable).
        ranked.sort_by_key(|t| std::cmp::Reverse(t.1));
        let label = match ranked.as_slice() {
            // Three or more trees level at the top: no pair names it.
            [(_, a), (_, b), (_, c), ..] if a == b && b == c => {
                format!("Hybrid {}", class.name())
            }
            [(first, a), (second, b), ..] if a == b => format!("{first}-{second} hybrid"),
            [(first, _), ..] => format!("{first} {}", class.name()),
            [] => return None,
        };
        let short = trees
            .iter()
            .map(|(_, p)| p.to_string())
            .collect::<Vec<_>>()
            .join("/");
        Some(Spec { label, short })
    }

    /// The label and the shorthand on one line: "Demonology Warlock 2/6/0".
    pub fn line(&self) -> String {
        format!("{} {}", self.label, self.short)
    }
}

/// Why a talent cannot take another rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnBlocker {
    /// The talent is not one of this class's.
    NotThisClass,
    /// Every rank is taken.
    MaxRank,
    /// The build has spent [`MAX_TALENT_POINTS`].
    NoPoints,
    /// The talent's tier is not open yet: its tree holds `has` of the
    /// `needed` points in the tiers below it.
    TierClosed { tree: TreeId, needed: u32, has: u32 },
    /// A prerequisite is not at its rank yet.
    Requires { talent: TalentId, rank: u8 },
}

/// Whether a class fields a pet a `PetStatPercent` can reach.
fn has_pet(class: CharacterClass) -> bool {
    match class {
        CharacterClass::Warlock | CharacterClass::Hunter => true,
        CharacterClass::Warrior
        | CharacterClass::Mage
        | CharacterClass::Rogue
        | CharacterClass::Priest
        | CharacterClass::Paladin
        | CharacterClass::Shaman
        | CharacterClass::Druid => false,
    }
}

/// Whether a patch aimed `on` reaches `ability`.
fn patch_reaches(on: PatchTarget, ability: AbilityType, config: &AbilityConfig) -> bool {
    match on {
        PatchTarget::Ability(target) => target == ability,
        PatchTarget::School(school) => config.spell_school == school,
    }
}

/// One combatant's build, resolved at its spawn: the book it carries and the
/// stat changes its spawn applies to it and to its pet.
#[derive(Debug, Clone)]
pub struct ResolvedBuild {
    book: AbilityBook,
    build: Build,
    stats: BTreeMap<TalentStat, StatChange>,
    pet_stats: BTreeMap<TalentStat, StatChange>,
    procs: Vec<TalentProcSlot>,
    mechanics: TalentMechanics,
    /// The off-hand poison the slot chose, when its build opens the slot.
    offhand_poison: Option<crate::states::match_config::RoguePoison>,
    /// Whether the slot chose to sacrifice its demon at the gates.
    sacrifices_pet: bool,
}

impl ResolvedBuild {
    /// An untalented combatant of `class`: its class's base book and nothing
    /// else. What [`TalentDefinitions::resolve`] gives an empty build.
    pub fn untalented(abilities: &AbilityDefinitions, class: CharacterClass) -> Self {
        Self {
            book: AbilityBook::for_class(abilities, class),
            build: Build::default(),
            stats: BTreeMap::new(),
            pet_stats: BTreeMap::new(),
            procs: Vec::new(),
            mechanics: TalentMechanics::default(),
            offhand_poison: None,
            sacrifices_pet: false,
        }
    }

    /// The build's mechanics (`talent_mechanics`).
    pub fn mechanics(&self) -> &TalentMechanics {
        &self.mechanics
    }

    /// Whether the slot sacrifices its demon at the gates (the Warlock's
    /// demon choice, `Sacrifice`).
    pub fn sacrifices_pet(&self) -> bool {
        self.sacrifices_pet
    }

    /// The combatant's book.
    pub fn book(&self) -> &AbilityBook {
        &self.book
    }

    /// The build this was resolved from.
    pub fn build(&self) -> &Build {
        &self.build
    }

    /// The summed change the build makes to each of the combatant's stats:
    /// the points its `StatAdd` ranks add and the percentage its
    /// `StatPercent` ranks take.
    pub fn stat_changes(&self) -> &BTreeMap<TalentStat, StatChange> {
        &self.stats
    }

    /// The same, for its pet (`PetStatPercent`: a percentage only).
    pub fn pet_stat_changes(&self) -> &BTreeMap<TalentStat, StatChange> {
        &self.pet_stats
    }

    /// The build's procs, one slot each, in build order.
    pub fn procs(&self) -> &[TalentProcSlot] {
        &self.procs
    }

    /// Apply the build to its combatant: its `StatAdd` and `StatPercent`
    /// effects, after `apply_equipment` (points added as an item adds them,
    /// then a percentage of the total, gear included) and before the pet
    /// spawns from it, refilling the pools to their new pre-combat levels as
    /// equipping does; and its procs, which it carries from here
    /// (`Combatant::talent_procs`).
    pub fn apply(&self, combatant: &mut Combatant) {
        combatant.talent_mechanics = self.mechanics.clone();
        combatant.rogue_offhand_poison = self.offhand_poison;
        combatant.sacrifice_pending = self.sacrifices_pet;
        apply_changes(&self.stats, combatant);
        combatant.talent_procs.slots = self.procs.clone();
    }

    /// Apply the build's `PetStatPercent` effects to its combatant's pet, once
    /// the pet has taken its share of its owner's stats.
    pub fn apply_to_pet(&self, pet: &mut Combatant) {
        apply_changes(&self.pet_stats, pet);
    }
}

fn apply_changes(changes: &BTreeMap<TalentStat, StatChange>, combatant: &mut Combatant) {
    if changes.is_empty() {
        return;
    }
    for (stat, change) in changes {
        stat.apply(combatant, *change);
    }
    combatant.current_health = combatant.max_health;
    combatant.current_mana = combatant.pre_combat_resource();
}

/// Resolve a spawn's build, which every entrance has already validated (a
/// headless or replay config in `to_match_config`; the client's talent panel
/// offers only legal builds). Panics, naming the combatant, on one that is
/// not: a bug at that entrance.
pub fn resolve_for_spawn(
    talents: &TalentDefinitions,
    abilities: &AbilityDefinitions,
    team: u8,
    slot: usize,
    class: CharacterClass,
    build: &Build,
    choices: &SlotChoices,
) -> ResolvedBuild {
    talents
        .resolve_with_choices(abilities, class, build, choices)
        .unwrap_or_else(|e| panic!("team {team} slot {slot} ({class:?}): illegal build: {e}"))
}
