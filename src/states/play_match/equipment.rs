//! Equipment System
//!
//! Data-driven equipment definitions loaded from RON config files.
//! Items provide stat bonuses applied to combatants at spawn time.
//!
//! ## Usage
//! ```ignore
//! fn my_system(items: Res<ItemDefinitions>, loadouts: Res<DefaultLoadouts>) {
//!     let item = items.get(&ItemId::ArcaniteReaper).unwrap();
//!     println!("Arcanite Reaper attack damage: {}-{}", item.attack_damage_min, item.attack_damage_max);
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::states::match_config::CharacterClass;

/// What a character wears: one item per equip socket.
///
/// **A `BTreeMap`, deliberately, and not for lookup speed.** Applying a loadout
/// sums float stats across its entries ([`crate::states::play_match::components::Combatant::apply_equipment`],
/// and the View Combatant screen's `EquipmentBonuses::from_loadout`), and float
/// addition is not associative — so iteration order decides the last ULP of every
/// derived stat. A `HashMap` with the default `RandomState` is seeded per process,
/// which made those sums differ between runs of one unmodified binary (AS-58:
/// Rogue `crit_chance` took three distinct bit patterns, 0x3e2e147a..0x3e2e147c,
/// across 40 runs). That is fatal for a project whose verification protocol is
/// headless byte-identity.
///
/// `BTreeMap` puts the ordering in the TYPE rather than in a rule each summation
/// site must remember. Iteration follows [`ItemSlot`]'s derived `Ord`, which is
/// its declaration order and therefore [`ItemSlot::all`]'s canonical order.
/// Never widen a loadout back to a `HashMap`; `loadout_is_ordered` in this
/// module's tests fails if you do.
pub type Loadout = BTreeMap<ItemSlot, ItemId>;

// ============================================================================
// ENUMS
// ============================================================================

/// What KIND of slot an item occupies — a property of the ITEM, declared in
/// `items.ron` as `slot:`.
///
/// Distinct from [`ItemSlot`], which is a socket on a CHARACTER. Most kinds map
/// to exactly one socket, but `Ring` and `Trinket` each have two, and any item
/// of that kind fits either of them (see [`ItemSlotType::sockets`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ItemSlotType {
    Head,
    Neck,
    Shoulders,
    Back,
    Chest,
    Wrists,
    Hands,
    Waist,
    Legs,
    Feet,
    Ring,
    Trinket,
    MainHand,
    OffHand,
    Ranged,
}

impl ItemSlotType {
    /// Every slot kind, in the canonical presentation order (mirrors
    /// [`ItemSlot::all`], with each paired kind appearing once).
    pub fn all() -> &'static [ItemSlotType] {
        &[
            ItemSlotType::Head, ItemSlotType::Neck, ItemSlotType::Shoulders,
            ItemSlotType::Back, ItemSlotType::Chest, ItemSlotType::Wrists,
            ItemSlotType::Hands, ItemSlotType::Waist, ItemSlotType::Legs,
            ItemSlotType::Feet, ItemSlotType::Ring, ItemSlotType::Trinket,
            ItemSlotType::MainHand, ItemSlotType::OffHand, ItemSlotType::Ranged,
        ]
    }

    /// Display name of the slot kind — "Ring", never "Ring 1". This is what
    /// every item-facing surface (encyclopedia chips, subtitles, tooltips)
    /// labels an item with.
    pub fn name(&self) -> &'static str {
        match self {
            ItemSlotType::Head => "Head",
            ItemSlotType::Neck => "Neck",
            ItemSlotType::Shoulders => "Shoulders",
            ItemSlotType::Back => "Back",
            ItemSlotType::Chest => "Chest",
            ItemSlotType::Wrists => "Wrists",
            ItemSlotType::Hands => "Hands",
            ItemSlotType::Waist => "Waist",
            ItemSlotType::Legs => "Legs",
            ItemSlotType::Feet => "Feet",
            ItemSlotType::Ring => "Ring",
            ItemSlotType::Trinket => "Trinket",
            ItemSlotType::MainHand => "Main Hand",
            ItemSlotType::OffHand => "Off Hand",
            ItemSlotType::Ranged => "Ranged",
        }
    }

    /// Whether items of this kind are WEAPONS — held in a hand or in the
    /// ranged socket — and therefore gated by class weapon proficiency rather
    /// than by armor type. Every weapon carries `armor_type: None`, which sits
    /// in every class's allowed list, so this is the seam that tells a weapon
    /// apart from an accessory.
    pub fn is_weapon_slot(&self) -> bool {
        matches!(
            self,
            ItemSlotType::MainHand | ItemSlotType::OffHand | ItemSlotType::Ranged
        )
    }

    /// The character sockets that accept this kind of item. Single-socket kinds
    /// return one entry; `Ring` and `Trinket` return their two siblings in
    /// canonical order.
    pub fn sockets(&self) -> &'static [ItemSlot] {
        match self {
            ItemSlotType::Head => &[ItemSlot::Head],
            ItemSlotType::Neck => &[ItemSlot::Neck],
            ItemSlotType::Shoulders => &[ItemSlot::Shoulders],
            ItemSlotType::Back => &[ItemSlot::Back],
            ItemSlotType::Chest => &[ItemSlot::Chest],
            ItemSlotType::Wrists => &[ItemSlot::Wrists],
            ItemSlotType::Hands => &[ItemSlot::Hands],
            ItemSlotType::Waist => &[ItemSlot::Waist],
            ItemSlotType::Legs => &[ItemSlot::Legs],
            ItemSlotType::Feet => &[ItemSlot::Feet],
            ItemSlotType::Ring => &[ItemSlot::Ring1, ItemSlot::Ring2],
            ItemSlotType::Trinket => &[ItemSlot::Trinket1, ItemSlot::Trinket2],
            ItemSlotType::MainHand => &[ItemSlot::MainHand],
            ItemSlotType::OffHand => &[ItemSlot::OffHand],
            ItemSlotType::Ranged => &[ItemSlot::Ranged],
        }
    }
}

/// Equip SOCKET on a character — 17 sockets matching WoW Classic.
///
/// This is the key of a loadout (`loadouts.ron`, `MatchConfig::teamN_equipment`,
/// [`resolve_loadout`]): it answers "what is worn HERE". An item never names a
/// socket; it names an [`ItemSlotType`], and the socket decides which kinds it
/// accepts via [`ItemSlot::slot_type`].
///
/// `Ord` is derived, so it follows the declaration order below — which is also
/// [`ItemSlot::all`]'s canonical presentation order. That is what gives
/// [`Loadout`] its deterministic iteration; reordering these variants changes the
/// order equipment stats are summed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ItemSlot {
    Head,
    Neck,
    Shoulders,
    Back,
    Chest,
    Wrists,
    Hands,
    Waist,
    Legs,
    Feet,
    Ring1,
    Ring2,
    Trinket1,
    Trinket2,
    MainHand,
    OffHand,
    Ranged,
}

impl ItemSlot {
    pub fn all() -> &'static [ItemSlot] {
        &[
            ItemSlot::Head, ItemSlot::Neck, ItemSlot::Shoulders, ItemSlot::Back,
            ItemSlot::Chest, ItemSlot::Wrists, ItemSlot::Hands, ItemSlot::Waist,
            ItemSlot::Legs, ItemSlot::Feet, ItemSlot::Ring1, ItemSlot::Ring2,
            ItemSlot::Trinket1, ItemSlot::Trinket2, ItemSlot::MainHand,
            ItemSlot::OffHand, ItemSlot::Ranged,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            ItemSlot::Head => "Head",
            ItemSlot::Neck => "Neck",
            ItemSlot::Shoulders => "Shoulders",
            ItemSlot::Back => "Back",
            ItemSlot::Chest => "Chest",
            ItemSlot::Wrists => "Wrists",
            ItemSlot::Hands => "Hands",
            ItemSlot::Waist => "Waist",
            ItemSlot::Legs => "Legs",
            ItemSlot::Feet => "Feet",
            ItemSlot::Ring1 => "Ring 1",
            ItemSlot::Ring2 => "Ring 2",
            ItemSlot::Trinket1 => "Trinket 1",
            ItemSlot::Trinket2 => "Trinket 2",
            ItemSlot::MainHand => "Main Hand",
            ItemSlot::OffHand => "Off Hand",
            ItemSlot::Ranged => "Ranged",
        }
    }

    /// Whether this socket holds a weapon. One authority, in
    /// [`ItemSlotType::is_weapon_slot`], so a socket and the item kind it
    /// accepts can never disagree about what counts as a weapon.
    pub fn is_weapon_slot(&self) -> bool {
        self.slot_type().is_weapon_slot()
    }

    /// The kind of item this socket holds. Both ring sockets report `Ring`,
    /// both trinket sockets report `Trinket`; every other socket is 1:1.
    pub fn slot_type(&self) -> ItemSlotType {
        match self {
            ItemSlot::Head => ItemSlotType::Head,
            ItemSlot::Neck => ItemSlotType::Neck,
            ItemSlot::Shoulders => ItemSlotType::Shoulders,
            ItemSlot::Back => ItemSlotType::Back,
            ItemSlot::Chest => ItemSlotType::Chest,
            ItemSlot::Wrists => ItemSlotType::Wrists,
            ItemSlot::Hands => ItemSlotType::Hands,
            ItemSlot::Waist => ItemSlotType::Waist,
            ItemSlot::Legs => ItemSlotType::Legs,
            ItemSlot::Feet => ItemSlotType::Feet,
            ItemSlot::Ring1 | ItemSlot::Ring2 => ItemSlotType::Ring,
            ItemSlot::Trinket1 | ItemSlot::Trinket2 => ItemSlotType::Trinket,
            ItemSlot::MainHand => ItemSlotType::MainHand,
            ItemSlot::OffHand => ItemSlotType::OffHand,
            ItemSlot::Ranged => ItemSlotType::Ranged,
        }
    }

    /// Whether an item of the given kind may be equipped in this socket.
    pub fn accepts(&self, slot_type: ItemSlotType) -> bool {
        self.slot_type() == slot_type
    }

    /// The other socket of the same kind, for the kinds that have two
    /// (rings, trinkets). `None` for every 1:1 socket.
    ///
    /// An item is unique-equipped: it may not occupy a socket and its sibling
    /// at once — see [`enforce_unique_equipped`].
    pub fn sibling(&self) -> Option<ItemSlot> {
        match self {
            ItemSlot::Ring1 => Some(ItemSlot::Ring2),
            ItemSlot::Ring2 => Some(ItemSlot::Ring1),
            ItemSlot::Trinket1 => Some(ItemSlot::Trinket2),
            ItemSlot::Trinket2 => Some(ItemSlot::Trinket1),
            _ => None,
        }
    }
}

/// Every pair of sibling sockets, primary first. The primary is the socket a
/// unique-equipped conflict resolves in favour of.
///
/// Derived from [`ItemSlotType::sockets`] rather than listed by hand, so a
/// kind that grows a second socket is unique-equipped-enforced the moment it
/// is — there is no separate list to forget. The primary is the kind's first
/// socket in canonical order.
fn sibling_socket_pairs() -> impl Iterator<Item = (ItemSlot, ItemSlot)> {
    ItemSlotType::all().iter().filter_map(|kind| match kind.sockets() {
        [primary, secondary] => Some((*primary, *secondary)),
        _ => None,
    })
}

/// Armor type restriction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArmorType {
    Cloth,
    Leather,
    Mail,
    Plate,
    /// Accessories (rings, trinkets, neck, back) and weapons
    None,
}

/// What kind of weapon an item is. Gates equipping through
/// [`weapon_proficiency`]; `OffhandFrill` (a held-in-off-hand tome or orb) and
/// `None` are the two values that are not weapons and need no proficiency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WeaponType {
    Sword,
    Mace,
    Axe,
    Dagger,
    Staff,
    Polearm,
    Fist,
    Bow,
    Gun,
    Crossbow,
    Wand,
    Thrown,
    Shield,
    OffhandFrill,
    None,
}

impl WeaponType {
    /// Every weapon type, in declaration order. Pinned complete by
    /// `weapon_type_all_lists_every_variant`.
    pub fn all() -> &'static [WeaponType] {
        &[
            WeaponType::Sword, WeaponType::Mace, WeaponType::Axe,
            WeaponType::Dagger, WeaponType::Staff, WeaponType::Polearm,
            WeaponType::Fist, WeaponType::Bow, WeaponType::Gun,
            WeaponType::Crossbow, WeaponType::Wand, WeaponType::Thrown,
            WeaponType::Shield, WeaponType::OffhandFrill, WeaponType::None,
        ]
    }
}

/// Unique item identifier — each named item in the game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemId {
    // === Plate Armor — DPS (Warrior) ===
    LionheartHelm,
    OnslaughtHeadGuard,
    ConquerorsChestplate,
    LegplatesOfWrath,
    GauntletsOfMight,
    SabatonsBattleBorn,
    WaistguardOfHeroism,
    WristguardsOfStability,
    ShoulderplatesOfValor,

    // === Plate Armor — Holy (Paladin) ===
    LawbringerHelm,
    LawbringerSpaulders,
    LawbringerChestguard,
    LawbringerBracers,
    LawbringerGauntlets,
    LawbringerBelt,
    LawbringerLegplates,
    LawbringerBoots,

    // === Mail Armor (Hunter, Warrior, Paladin) ===
    BeaststalkerHelm,
    BeaststalkerTunic,
    BeaststalkerLegs,
    BeaststalkerGloves,
    BeaststalkerBoots,
    BeaststalkerBelt,
    BeaststalkerBracers,
    BeaststalkerMantle,

    // === Caster Mail Armor (Shaman) — spell power + mana on mail ===
    EarthfuryHelmet,
    EarthfuryVestments,
    EarthfuryLegguards,
    EarthfuryGauntlets,
    EarthfuryBoots,
    EarthfuryBelt,
    EarthfuryBracers,
    EarthfuryEpaulets,

    // === Leather Armor (Rogue, Hunter) ===
    NightstalkerCowl,
    NightstalkerTunic,
    NightstalkerLegs,
    NightstalkerGloves,
    NightstalkerBoots,
    NightstalkerBelt,
    NightstalkerBracers,
    NightstalkerMantle,

    // === Cloth Armor (Mage, Priest, Warlock) ===
    MagistersCrown,
    MagistersRobes,
    MagistersLeggings,
    MagistersGloves,
    MagistersBoots,
    MagistersBelt,
    MagistersBracers,
    MagistersMantle,

    // === Cloaks (all classes) ===
    CloakOfTheShieldWall,
    CloakOfConcentration,
    CloakOfFrostWarding,

    // === Necklaces (all classes) ===
    AmuletOfPower,
    AmuletOfResilience,
    AmuletOfShadowWard,

    // === Rings (all classes) ===
    BandOfAccuria,
    SignetOfFocus,
    RingOfProtection,
    BandOfElementalResistance,

    // === Trinkets (all classes) ===
    MarkOfTheChampion,
    EssenceOfEternalLife,

    // === Melee Weapons ===
    ArcaniteReaper,
    FrostbiteBlade,
    SerpentFangDagger,
    HammerOfTheRighteous,
    CrescentStaff,

    // === Ranged Weapons ===
    WandOfShadows,
    StaffOfDominance,
    AshwoodBow,
    SniperScope,

    // === Off Hand ===
    TomeOfKnowledge,
    WallOfTheDeadShield,
    AegisOfTheBloodGod,

    // ====================================================================
    // TIER 1 ITEMS (Item Level 69-75)
    // ====================================================================

    // === Tier 1: Plate Armor — DPS (Warrior) ===
    WarlordsCrown,
    WarlordsVizard,
    WarlordsBreastplate,
    WarlordsLegguards,
    WarlordsGauntlets,
    WarlordsSpaulders,
    WarlordsGreaves,
    WarlordsGirdle,
    WarlordsBracers,

    // === Tier 1: Plate Armor — Holy (Paladin) ===
    JudgementCrown,
    JudgementBreastplate,
    JudgementLegguards,
    JudgementSpaulders,
    JudgementGauntlets,
    JudgementBelt,
    JudgementSabatons,
    JudgementBracers,

    // === Tier 1: Mail Armor (Hunter) ===
    GiantstalkerHelm,
    GiantstalkerTunic,
    GiantstalkerLegs,
    GiantstalkerGloves,
    GiantstalkerEpaulets,
    GiantstalkerBoots,
    GiantstalkerBelt,
    GiantstalkerBracers,

    // === Tier 1: Leather Armor (Rogue) ===
    DeathdealerCowl,
    DeathdealerTunic,
    DeathdealerLegs,
    DeathdealerGloves,
    DeathdealerMantle,
    DeathdealerBoots,
    DeathdealerBelt,
    DeathdealerBracers,

    // === Tier 1: Cloth Armor (Mage, Priest, Warlock) ===
    ArcanistCrown,
    ArcanistRobes,
    ArcanistLeggings,
    ArcanistGloves,
    ArcanistMantle,
    ArcanistBoots,
    ArcanistBelt,
    ArcanistBracers,

    // === Tier 1: Cloaks (all classes) ===
    CloakOfConquest,
    CloakOfWisdom,
    CloakOfNatureWarding,

    // === Tier 1: Necklaces (all classes) ===
    PendantOfMight,
    PendantOfClarity,
    PendantOfArcaneWarding,

    // === Tier 1: Rings (all classes) ===
    RingOfBruteForce,
    RingOfArcaneInsight,
    RingOfFortitude,
    RingOfElementalMastery,

    // === Tier 1: Trinkets (all classes) ===
    InsigniaOfTheAlliance,
    TalismanOfEphemeralPower,

    // === Tier 1: Melee Weapons ===
    BloodlordsBattleaxe,
    StormbladeEdge,
    FangOfTheViper,
    MaceOfTheRedeemer,
    RunestaffOfElements,

    // === Tier 1: Ranged Weapons ===
    WandOfTheInvoker,
    EaglestrikeBow,
    DeadeyeCrossbow,

    // === Tier 1: Off Hand ===
    GrimoireOfShadows,
    BulwarkOfTheGuardian,
}

// ============================================================================
// ITEM CONFIG
// ============================================================================

/// Item definition loaded from RON
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemConfig {
    /// Display name
    pub name: String,
    /// Item level (informational, determines stat budget)
    #[serde(default)]
    pub item_level: u32,
    /// Item tier (0 = base, 1 = first upgrade tier, etc.)
    #[serde(default)]
    pub item_tier: u32,
    /// Icon asset path (e.g. "icons/items/inv_helmet_36.jpg")
    #[serde(default)]
    pub icon: String,
    /// What KIND of slot this item equips to. `Ring`, not `Ring1` — the socket
    /// is chosen by the loadout, not baked into the item.
    pub slot: ItemSlotType,
    /// Armor type restriction
    #[serde(default = "default_armor_type")]
    pub armor_type: ArmorType,
    /// Weapon type (flavor only)
    #[serde(default = "default_weapon_type")]
    pub weapon_type: WeaponType,
    /// If set, only these classes can equip this item
    #[serde(default)]
    pub allowed_classes: Option<Vec<CharacterClass>>,
    /// Whether this item is a weapon (replaces attack_damage/attack_speed instead of adding)
    #[serde(default)]
    pub is_weapon: bool,
    /// Whether this is a two-handed weapon (prevents off-hand equip)
    #[serde(default)]
    pub two_handed: bool,

    // === Stat Bonuses ===
    #[serde(default)]
    pub max_health: f32,
    #[serde(default)]
    pub max_mana: f32,
    #[serde(default)]
    pub mana_regen: f32,
    #[serde(default)]
    pub attack_power: f32,
    #[serde(default)]
    pub spell_power: f32,
    #[serde(default)]
    pub crit_chance: f32,
    #[serde(default)]
    pub movement_speed: f32,
    /// Armor rating (reduces incoming Physical damage)
    #[serde(default)]
    pub armor: f32,
    /// Fire spell resistance
    #[serde(default)]
    pub fire_resistance: f32,
    /// Frost spell resistance
    #[serde(default)]
    pub frost_resistance: f32,
    /// Shadow spell resistance
    #[serde(default)]
    pub shadow_resistance: f32,
    /// Arcane spell resistance
    #[serde(default)]
    pub arcane_resistance: f32,
    /// Nature spell resistance
    #[serde(default)]
    pub nature_resistance: f32,
    /// Holy spell resistance
    #[serde(default)]
    pub holy_resistance: f32,

    // === Weapon Stats (only for is_weapon: true) ===
    /// Weapon minimum damage (replaces combatant attack_damage for primary weapon slot)
    #[serde(default)]
    pub attack_damage_min: f32,
    /// Weapon maximum damage (replaces combatant attack_damage for primary weapon slot)
    #[serde(default)]
    pub attack_damage_max: f32,
    /// Weapon attack speed (replaces combatant attack_speed for primary weapon slot)
    #[serde(default)]
    pub attack_speed: f32,
}

fn default_armor_type() -> ArmorType {
    ArmorType::None
}

fn default_weapon_type() -> WeaponType {
    WeaponType::None
}

// ============================================================================
// CLASS RESTRICTION HELPERS
// ============================================================================

/// Get the highest armor type a class can wear
fn max_armor_type(class: CharacterClass) -> &'static [ArmorType] {
    match class {
        CharacterClass::Warrior | CharacterClass::Paladin => &[ArmorType::Cloth, ArmorType::Leather, ArmorType::Mail, ArmorType::Plate, ArmorType::None],
        CharacterClass::Hunter | CharacterClass::Shaman => &[ArmorType::Cloth, ArmorType::Leather, ArmorType::Mail, ArmorType::None],
        CharacterClass::Rogue => &[ArmorType::Cloth, ArmorType::Leather, ArmorType::None],
        CharacterClass::Mage | CharacterClass::Priest | CharacterClass::Warlock => &[ArmorType::Cloth, ArmorType::None],
    }
}

// ============================================================================
// WEAPON PROFICIENCY
// ============================================================================

/// How far a class trained in a weapon type.
///
/// WoW Classic gates weapons by per-class PROFICIENCY, a set of passive skills
/// a class may train (`One-Handed Axes` is spell 196, `Wands` is 5009, and so
/// on through the list). Axes, maces and swords are split into separate
/// one- and two-handed skills, which is why "trained" is three-valued rather
/// than a bool: a Rogue wields a one-handed sword and may never wield a
/// two-handed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proficiency {
    /// Never equippable by this class.
    Untrained,
    /// Only the one-handed form. Applies to the three types Classic splits
    /// (axe, mace, sword); for every other type it is indistinguishable from
    /// [`Proficiency::Trained`], since those types have a single form.
    OneHandedOnly,
    /// Equippable in either form.
    Trained,
}

/// Whether `class` may wield `weapon`, and in which form.
///
/// The rows are WoW Classic's per-class weapon-skill lists, the ones a weapon
/// master teaches: a Mage trains daggers, one-handed swords, staves and wands
/// and nothing else; a Paladin never touches a dagger or a staff; a Hunter
/// never a mace or a shield; a Shaman never a sword. Held-in-off-hand items
/// (`OffhandFrill`) require no proficiency in Classic and are open to every
/// class, as is an item carrying no weapon type at all.
///
/// Both matches are wildcard-free on purpose: a new [`CharacterClass`] or
/// [`WeaponType`] variant fails to compile until every class has an answer for
/// it. That is the point of the table — see `weapon_proficiency_is_exhaustive`.
pub fn weapon_proficiency(class: CharacterClass, weapon: WeaponType) -> Proficiency {
    use Proficiency::{OneHandedOnly, Trained, Untrained};
    use WeaponType as W;

    match class {
        // Every melee weapon, every ranged weapon, shields. No wands.
        CharacterClass::Warrior => match weapon {
            W::Axe | W::Mace | W::Sword | W::Dagger | W::Staff | W::Polearm | W::Fist
            | W::Bow | W::Gun | W::Crossbow | W::Thrown | W::Shield => Trained,
            W::Wand => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
        // Axes, maces, swords (both forms), polearms, shields. No dagger, no
        // staff, no ranged weapon of any kind.
        CharacterClass::Paladin => match weapon {
            W::Axe | W::Mace | W::Sword | W::Polearm | W::Shield => Trained,
            W::Dagger | W::Staff | W::Fist | W::Bow | W::Gun | W::Crossbow
            | W::Thrown | W::Wand => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
        // Bows, guns, crossbows, thrown; axes, swords, daggers, staves,
        // polearms, fist weapons. Never a mace, never a shield.
        CharacterClass::Hunter => match weapon {
            W::Axe | W::Sword | W::Dagger | W::Staff | W::Polearm | W::Fist
            | W::Bow | W::Gun | W::Crossbow | W::Thrown => Trained,
            W::Mace | W::Wand | W::Shield => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
        // Daggers, fist weapons, one-handed maces and swords, all three
        // ranged physical types. No two-handers at all, no axes (those came
        // with the Burning Crusade), no shields.
        CharacterClass::Rogue => match weapon {
            W::Dagger | W::Fist | W::Bow | W::Gun | W::Crossbow | W::Thrown => Trained,
            W::Mace | W::Sword => OneHandedOnly,
            W::Axe | W::Staff | W::Polearm | W::Wand | W::Shield => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
        // Axes and maces in both forms, daggers, staves, fist weapons,
        // shields. Never a sword, never a ranged weapon.
        CharacterClass::Shaman => match weapon {
            W::Axe | W::Mace | W::Dagger | W::Staff | W::Fist | W::Shield => Trained,
            W::Sword | W::Polearm | W::Bow | W::Gun | W::Crossbow | W::Thrown
            | W::Wand => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
        // Daggers, one-handed maces, staves, wands.
        CharacterClass::Priest => match weapon {
            W::Dagger | W::Staff | W::Wand => Trained,
            W::Mace => OneHandedOnly,
            W::Axe | W::Sword | W::Polearm | W::Fist | W::Bow | W::Gun
            | W::Crossbow | W::Thrown | W::Shield => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
        // Daggers, one-handed swords, staves, wands.
        CharacterClass::Mage => match weapon {
            W::Dagger | W::Staff | W::Wand => Trained,
            W::Sword => OneHandedOnly,
            W::Axe | W::Mace | W::Polearm | W::Fist | W::Bow | W::Gun
            | W::Crossbow | W::Thrown | W::Shield => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
        // Daggers, one-handed swords, staves, wands — the Mage's list.
        CharacterClass::Warlock => match weapon {
            W::Dagger | W::Staff | W::Wand => Trained,
            W::Sword => OneHandedOnly,
            W::Axe | W::Mace | W::Polearm | W::Fist | W::Bow | W::Gun
            | W::Crossbow | W::Thrown | W::Shield => Untrained,
            W::OffhandFrill | W::None => Trained,
        },
    }
}

/// Whether a class may wield a weapon of this type in this form.
pub fn can_wield(class: CharacterClass, weapon: WeaponType, two_handed: bool) -> bool {
    match weapon_proficiency(class, weapon) {
        Proficiency::Untrained => false,
        Proficiency::OneHandedOnly => !two_handed,
        Proficiency::Trained => true,
    }
}

/// Why a class may not equip an item, or `None` when it may.
///
/// Three gates, in order: the item's explicit class list, armor type, and —
/// for anything in a weapon socket — [`weapon_proficiency`]. The proficiency
/// gate is the one that makes [`WeaponType`] mean something: armor type cannot
/// speak for weapons, because every weapon declares `ArmorType::None` and that
/// sits in every class's allowed list.
///
/// The reason is a string rather than a bool so a rejected loadout entry can
/// name the gate it failed instead of blaming armor type for a proficiency
/// failure. [`can_equip`] is this predicate.
pub fn equip_rejection(class: CharacterClass, item: &ItemConfig) -> Option<String> {
    // Check class restriction list
    if let Some(ref allowed) = item.allowed_classes {
        if !allowed.contains(&class) {
            return Some(format!("{} is not in the item's class list", class.name()));
        }
    }

    // Check armor type
    if !max_armor_type(class).contains(&item.armor_type) {
        return Some(format!(
            "{} cannot wear {:?} armor",
            class.name(),
            item.armor_type
        ));
    }

    // Check weapon proficiency
    if item.slot.is_weapon_slot() && !can_wield(class, item.weapon_type, item.two_handed) {
        return Some(format!(
            "{} has no proficiency with {}{:?}",
            class.name(),
            if item.two_handed { "two-handed " } else { "" },
            item.weapon_type
        ));
    }

    None
}

/// Check if a class can equip a specific item. See [`equip_rejection`].
pub fn can_equip(class: CharacterClass, item: &ItemConfig) -> bool {
    equip_rejection(class, item).is_none()
}

/// Validate that all items in a loadout are equippable by the given class
pub fn validate_class_restrictions(
    class: CharacterClass,
    loadout: &Loadout,
    items: &ItemDefinitions,
) -> Result<(), String> {
    for (slot, item_id) in loadout {
        if let Some(item) = items.get(item_id) {
            if let Some(reason) = equip_rejection(class, item) {
                return Err(format!(
                    "{} cannot equip {} ({:?}) in {:?} slot — {}",
                    class.name(), item.name, item_id, slot, reason
                ));
            }
            if !slot.accepts(item.slot) {
                return Err(format!(
                    "{:?} is a {:?} item but was placed in {:?} slot",
                    item_id, item.slot, slot
                ));
            }
        } else {
            return Err(format!("Unknown item {:?} in {:?} slot", item_id, slot));
        }
    }
    Ok(())
}

/// Validate that no item occupies both sockets of a sibling pair. Items are
/// unique-equipped: two ring sockets means two DIFFERENT rings.
pub fn validate_unique_equipped(
    loadout: &Loadout,
    items: &ItemDefinitions,
) -> Result<(), String> {
    for (primary, secondary) in sibling_socket_pairs() {
        match (loadout.get(&primary), loadout.get(&secondary)) {
            (Some(a), Some(b)) if a == b => {
                let name = items.get(a).map_or("unknown item", |i| i.name.as_str());
                return Err(format!(
                    "{} ({:?}) is equipped in both {:?} and {:?} — items are unique-equipped",
                    name, a, primary, secondary
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

// ============================================================================
// ITEM BUDGET VALIDATION
// ============================================================================

use super::constants::{
    BUDGET_PER_ILVL, BUDGET_TOLERANCE, WEIGHT_ATTACK_POWER, WEIGHT_CRIT_CHANCE,
    WEIGHT_MAX_HEALTH, WEIGHT_MAX_MANA, WEIGHT_MANA_REGEN, WEIGHT_MOVEMENT_SPEED,
    WEIGHT_RESISTANCE, WEIGHT_SPELL_POWER, slot_budget_multiplier,
};

/// Calculate the total budget usage for an item based on its budgeted stats.
/// Free stats (armor, attack_damage_min/max, attack_speed) are excluded.
pub fn calculate_budget_usage(item: &ItemConfig) -> f32 {
    item.max_health * WEIGHT_MAX_HEALTH
        + item.max_mana * WEIGHT_MAX_MANA
        + item.mana_regen * WEIGHT_MANA_REGEN
        + item.attack_power * WEIGHT_ATTACK_POWER
        + item.spell_power * WEIGHT_SPELL_POWER
        + item.crit_chance * WEIGHT_CRIT_CHANCE
        + item.movement_speed * WEIGHT_MOVEMENT_SPEED
        + item.fire_resistance * WEIGHT_RESISTANCE
        + item.frost_resistance * WEIGHT_RESISTANCE
        + item.shadow_resistance * WEIGHT_RESISTANCE
        + item.arcane_resistance * WEIGHT_RESISTANCE
        + item.nature_resistance * WEIGHT_RESISTANCE
        + item.holy_resistance * WEIGHT_RESISTANCE
}

/// Calculate the effective budget cap for an item based on its level and slot.
pub fn calculate_effective_budget(item: &ItemConfig) -> f32 {
    item.item_level as f32 * BUDGET_PER_ILVL * slot_budget_multiplier(item.slot)
}

/// Validate that an item's stat budget usage does not exceed its effective budget
/// (with tolerance). Returns Ok(()) if within budget, or Err with a diagnostic message.
pub fn validate_item_budget(name: &str, item: &ItemConfig) -> Result<(), String> {
    if item.item_level == 0 {
        return Err(format!(
            "{} ({:?}): item_level is 0 — set a valid item_level for budget validation",
            name, item.slot
        ));
    }

    let usage = calculate_budget_usage(item);
    let budget = calculate_effective_budget(item);
    let max_allowed = budget * (1.0 + BUDGET_TOLERANCE);

    if usage > max_allowed {
        let overage_pct = ((usage / budget) - 1.0) * 100.0;
        Err(format!(
            "{} (ilvl {}, {:?}): budget usage {:.1} exceeds cap {:.1} (budget {:.1} + {:.0}% tolerance) — {:.1}% over budget",
            name, item.item_level, item.slot, usage, max_allowed, budget, BUDGET_TOLERANCE * 100.0, overage_pct
        ))
    } else {
        Ok(())
    }
}

// ============================================================================
// LOADOUT RESOLUTION
// ============================================================================

/// Merge default loadout with optional per-socket overrides.
///
/// The merge is a plain overlay; the equip CONSTRAINTS are separate passes the
/// caller runs afterwards — [`enforce_two_hand_conflicts`] and
/// [`enforce_unique_equipped`].
pub fn resolve_loadout(
    class: CharacterClass,
    defaults: &DefaultLoadouts,
    overrides: &Loadout,
) -> Loadout {
    let mut loadout = defaults.get(class).cloned().unwrap_or_default();
    for (slot, item_id) in overrides {
        loadout.insert(*slot, *item_id);
    }
    loadout
}

/// Strip off-hand from a resolved loadout when the main-hand is a two-handed weapon.
/// Call this after `resolve_loadout` to enforce the 2H constraint.
pub fn enforce_two_hand_conflicts(loadout: &mut Loadout, items: &ItemDefinitions) {
    let has_2h = loadout.get(&ItemSlot::MainHand)
        .and_then(|id| items.get(id))
        .map_or(false, |item| item.two_handed);
    if has_2h {
        loadout.remove(&ItemSlot::OffHand);
    }
}

/// Strip a duplicate from the secondary of a sibling socket pair (rings,
/// trinkets) when both hold the same item. Call this after `resolve_loadout`
/// to enforce the unique-equipped constraint, the same way
/// `enforce_two_hand_conflicts` enforces the 2H one.
///
/// The primary socket keeps the item, mirroring the 2H rule's preference for
/// the main hand, so the outcome is independent of map iteration order.
pub fn enforce_unique_equipped(loadout: &mut Loadout) {
    for (primary, secondary) in sibling_socket_pairs() {
        if let (Some(a), Some(b)) = (loadout.get(&primary), loadout.get(&secondary)) {
            if a == b {
                loadout.remove(&secondary);
            }
        }
    }
}

/// Strip anything `class` may not equip from a resolved loadout — an item in a
/// socket that does not accept its kind, or one that fails [`can_equip`].
///
/// The third constraint pass, alongside [`enforce_two_hand_conflicts`] and
/// [`enforce_unique_equipped`], and it exists for the same reason: an override
/// map survives edits that invalidate it (change a configured slot's class and
/// its weapon override stays behind), so the resolver — not its callers — has
/// to be the one that guarantees a legal loadout.
pub fn enforce_class_restrictions(
    loadout: &mut Loadout,
    class: CharacterClass,
    items: &ItemDefinitions,
) {
    loadout.retain(|slot, item_id| match items.get(item_id) {
        Some(item) => slot.accepts(item.slot) && can_equip(class, item),
        // An id with no definition has no stats to apply; drop it rather than
        // leave a socket that renders as worn and equips nothing.
        None => false,
    });
}

/// The loadout a character actually wears: the class default overlaid with the
/// user's overrides, with every equip constraint enforced.
///
/// The single authority. Graphical spawn, headless spawn and the View
/// Combatant screen all resolve through it, so what the screen shows is what
/// the match equips — a constraint that lives in only some of those three is
/// the "describes but does not constrain" shape this pass exists to remove.
///
/// Order matters: class restrictions first, so stripping an illegal two-handed
/// main-hand leaves a legal off-hand in place rather than taking it down too.
pub fn resolve_equipped_loadout(
    class: CharacterClass,
    defaults: &DefaultLoadouts,
    overrides: &Loadout,
    items: &ItemDefinitions,
) -> Loadout {
    let mut loadout = resolve_loadout(class, defaults, overrides);
    enforce_class_restrictions(&mut loadout, class, items);
    enforce_two_hand_conflicts(&mut loadout, items);
    enforce_unique_equipped(&mut loadout);
    loadout
}

/// Find the first available one-handed main-hand weapon for a class, sorted by name.
/// Returns None if only two-handed weapons exist.
pub fn find_one_handed_mainhand(items: &ItemDefinitions, class: CharacterClass) -> Option<ItemId> {
    items.items_for_slot(ItemSlot::MainHand, class)
        .into_iter()
        .find(|(_, item)| !item.two_handed)
        .map(|(id, _)| id)
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Root structure for items.ron
#[derive(Debug, Serialize, Deserialize)]
pub struct ItemsConfig {
    pub items: HashMap<ItemId, ItemConfig>,
}

/// Resource containing all item definitions
#[derive(Resource, Clone)]
pub struct ItemDefinitions {
    definitions: HashMap<ItemId, ItemConfig>,
}

impl ItemDefinitions {
    pub fn new(config: ItemsConfig) -> Self {
        Self {
            definitions: config.items,
        }
    }

    pub fn get(&self, item: &ItemId) -> Option<&ItemConfig> {
        self.definitions.get(item)
    }

    pub fn get_unchecked(&self, item: &ItemId) -> &ItemConfig {
        self.definitions.get(item)
            .unwrap_or_else(|| panic!("Item {:?} not found in definitions", item))
    }

    pub fn item_count(&self) -> usize {
        self.definitions.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ItemId, &ItemConfig)> {
        self.definitions.iter()
    }

    /// Return all items the given socket accepts for a class, sorted by name.
    /// Both ring sockets share one pool, as do both trinket sockets.
    pub fn items_for_slot(&self, slot: ItemSlot, class: CharacterClass) -> Vec<(ItemId, &ItemConfig)> {
        let mut items: Vec<(ItemId, &ItemConfig)> = self.definitions.iter()
            .filter(|(_, item)| slot.accepts(item.slot) && can_equip(class, item))
            .map(|(id, item)| (*id, item))
            .collect();
        items.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        items
    }

    /// The items an equipment picker may offer for `socket`, given what the
    /// character already wears. Same as [`Self::items_for_slot`] minus anything
    /// already worn in the sibling socket — items are unique-equipped, so an
    /// offer that would duplicate one is not selectable in the first place.
    pub fn selectable_items_for_slot(
        &self,
        slot: ItemSlot,
        class: CharacterClass,
        loadout: &Loadout,
    ) -> Vec<(ItemId, &ItemConfig)> {
        let worn_in_sibling = slot.sibling().and_then(|s| loadout.get(&s)).copied();
        self.items_for_slot(slot, class)
            .into_iter()
            .filter(|(id, _)| Some(*id) != worn_in_sibling)
            .collect()
    }
}

/// Root structure for loadouts.ron
#[derive(Debug, Serialize, Deserialize)]
pub struct LoadoutsConfig {
    pub loadouts: HashMap<CharacterClass, Loadout>,
}

/// Resource containing default loadouts per class
#[derive(Resource, Clone)]
pub struct DefaultLoadouts {
    loadouts: HashMap<CharacterClass, Loadout>,
}

impl DefaultLoadouts {
    pub fn new(config: LoadoutsConfig) -> Self {
        Self {
            loadouts: config.loadouts,
        }
    }

    pub fn get(&self, class: CharacterClass) -> Option<&Loadout> {
        self.loadouts.get(&class)
    }
}

// ============================================================================
// LOADING
// ============================================================================

/// Load item definitions from assets/config/items.ron
pub fn load_item_definitions() -> Result<ItemDefinitions, String> {
    let config_path = crate::paths::asset_path_str("config/items.ron");

    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;

    let config: ItemsConfig = ron::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {}", config_path, e))?;

    let definitions = ItemDefinitions::new(config);

    info!("Loaded {} item definitions from {}", definitions.item_count(), config_path);

    Ok(definitions)
}

/// Load default loadouts from assets/config/loadouts.ron
pub fn load_default_loadouts(items: &ItemDefinitions) -> Result<DefaultLoadouts, String> {
    let config_path = crate::paths::asset_path_str("config/loadouts.ron");

    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;

    let config: LoadoutsConfig = ron::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {}", config_path, e))?;

    let loadouts = DefaultLoadouts::new(config);

    // Validate all loadout references resolve and pass class restrictions
    for class in CharacterClass::all() {
        if let Some(loadout) = loadouts.get(*class) {
            for (slot, item_id) in loadout {
                if items.get(item_id).is_none() {
                    return Err(format!(
                        "Default loadout for {} references unknown item {:?} in {:?} slot",
                        class.name(), item_id, slot
                    ));
                }
            }
            validate_class_restrictions(*class, loadout, items)?;
            validate_unique_equipped(loadout, items)
                .map_err(|e| format!("Default loadout for {}: {}", class.name(), e))?;
        }
    }

    info!("Loaded default loadouts from {}", config_path);

    Ok(loadouts)
}

// ============================================================================
// PLUGIN
// ============================================================================

/// Bevy plugin for equipment loading
pub struct EquipmentPlugin;

impl Plugin for EquipmentPlugin {
    fn build(&self, app: &mut App) {
        match load_item_definitions() {
            Ok(definitions) => {
                match load_default_loadouts(&definitions) {
                    Ok(loadouts) => {
                        app.insert_resource(definitions);
                        app.insert_resource(loadouts);
                    }
                    Err(e) => {
                        panic!("Failed to load default loadouts: {}", e);
                    }
                }
            }
            Err(e) => {
                panic!("Failed to load item definitions: {}", e);
            }
        }
    }
}

// ============================================================================
// EQUIPMENT FORMATTING (for combat log)
// ============================================================================

/// Format an equipment loadout for combat log display
pub fn format_loadout(
    loadout: &Loadout,
    items: &ItemDefinitions,
) -> String {
    if loadout.is_empty() {
        return "No equipment".to_string();
    }

    let mut parts: Vec<String> = Vec::new();
    for slot in ItemSlot::all() {
        if let Some(item_id) = loadout.get(slot) {
            if let Some(item) = items.get(item_id) {
                parts.push(format!("{}={}", slot.name(), item.name));
            }
        }
    }

    if parts.is_empty() {
        "No equipment".to_string()
    } else {
        parts.join(", ")
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;

    /// Build a minimal ItemDefinitions from a list of (ItemId, ItemConfig) pairs
    fn make_item_defs(items: Vec<(ItemId, ItemConfig)>) -> ItemDefinitions {
        let map: HashMap<ItemId, ItemConfig> = items.into_iter().collect();
        ItemDefinitions {
            definitions: map,
        }
    }

    /// Build a minimal ItemConfig for a non-weapon armor piece
    fn armor_item(name: &str, slot: ItemSlotType, armor_type: ArmorType) -> ItemConfig {
        ItemConfig {
            name: name.to_string(),
            item_level: 60,
            item_tier: 0,
            icon: String::new(),
            slot,
            armor_type,
            weapon_type: WeaponType::None,
            allowed_classes: None,
            is_weapon: false,
            two_handed: false,
            max_health: 50.0,
            max_mana: 20.0,
            mana_regen: 1.0,
            attack_power: 10.0,
            spell_power: 5.0,
            crit_chance: 0.01,
            movement_speed: 0.0,
            armor: 0.0,
            fire_resistance: 0.0,
            frost_resistance: 0.0,
            shadow_resistance: 0.0,
            arcane_resistance: 0.0,
            nature_resistance: 0.0,
            holy_resistance: 0.0,
            attack_damage_min: 0.0,
            attack_damage_max: 0.0,
            attack_speed: 0.0,
        }
    }

    /// Build a minimal weapon ItemConfig
    fn weapon_item(name: &str, slot: ItemSlotType, dmg_min: f32, dmg_max: f32, speed: f32) -> ItemConfig {
        ItemConfig {
            name: name.to_string(),
            item_level: 60,
            item_tier: 0,
            icon: String::new(),
            slot,
            armor_type: ArmorType::None,
            weapon_type: WeaponType::Sword,
            allowed_classes: None,
            is_weapon: true,
            two_handed: false,
            max_health: 0.0,
            max_mana: 0.0,
            mana_regen: 0.0,
            attack_power: 5.0,
            spell_power: 0.0,
            crit_chance: 0.0,
            movement_speed: 0.0,
            armor: 0.0,
            fire_resistance: 0.0,
            frost_resistance: 0.0,
            shadow_resistance: 0.0,
            arcane_resistance: 0.0,
            nature_resistance: 0.0,
            holy_resistance: 0.0,
            attack_damage_min: dmg_min,
            attack_damage_max: dmg_max,
            attack_speed: speed,
        }
    }

    // ---- loadout ordering guard (AS-58) ----

    /// A [`Loadout`] must iterate in canonical socket order, because applying one
    /// sums floats and float addition is not associative.
    ///
    /// **Why this test and not a repetition test.** The obvious guard — derive the
    /// same loadout's stats N times and assert they match — cannot fail, whatever
    /// map type is used: `RandomState` is seeded ONCE PER PROCESS, so a `HashMap`
    /// iterates in a fixed (if arbitrary) order for the whole life of a test
    /// binary. The bug is only visible ACROSS processes, which no in-process test
    /// can observe. So the guard is structural instead, in two halves:
    ///
    /// 1. `assert_is_btreemap` is a type assertion. Aliasing [`Loadout`] back to a
    ///    `HashMap` makes this line stop COMPILING — it does not merely fail.
    /// 2. The scrambled-insertion check pins the order to [`ItemSlot::all`], so
    ///    reordering the `ItemSlot` variants or hand-writing a different `Ord`
    ///    (either of which silently re-sums the stats in a new order) fails here.
    #[test]
    fn loadout_is_ordered() {
        fn assert_is_btreemap(_: &BTreeMap<ItemSlot, ItemId>) {}

        // Insert every socket in REVERSE canonical order; an ordered map must
        // still hand them back in canonical order.
        let mut loadout = Loadout::new();
        for slot in ItemSlot::all().iter().rev() {
            loadout.insert(*slot, ItemId::LionheartHelm);
        }
        assert_is_btreemap(&loadout);

        let iterated: Vec<ItemSlot> = loadout.keys().copied().collect();
        assert_eq!(
            iterated,
            ItemSlot::all().to_vec(),
            "a Loadout must iterate in ItemSlot::all() order — equipment stat sums depend on it"
        );
    }

    /// `ItemSlot`'s derived `Ord` is what [`Loadout`] orders by, and
    /// [`ItemSlot::all`] is what every presentation surface orders by. They are
    /// only the same list while the variant declaration order matches `all()`.
    #[test]
    fn item_slot_ord_matches_canonical_order() {
        let mut sorted = ItemSlot::all().to_vec();
        sorted.sort();
        assert_eq!(
            sorted,
            ItemSlot::all().to_vec(),
            "ItemSlot::all() must be in Ord order — it is the loadout summation order"
        );
    }

    // ---- apply_equipment tests ----

    #[test]
    fn apply_equipment_adds_armor_stats() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlotType::Head, ArmorType::Plate)),
        ]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);
        let base_health = combatant.max_health;
        let base_ap = combatant.attack_power;

        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        combatant.apply_equipment(&loadout, &items);

        assert_eq!(combatant.max_health, base_health + 50.0);
        assert_eq!(combatant.attack_power, base_ap + 10.0);
        // current_health should be synced to new max
        assert_eq!(combatant.current_health, combatant.max_health);
    }

    #[test]
    fn apply_equipment_empty_loadout_unchanged() {
        let items = make_item_defs(vec![]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);
        let base_health = combatant.max_health;
        let base_damage = combatant.attack_damage;

        let loadout = Loadout::new();
        combatant.apply_equipment(&loadout, &items);

        assert_eq!(combatant.max_health, base_health);
        assert_eq!(combatant.attack_damage, base_damage);
    }

    #[test]
    fn apply_equipment_weapon_replaces_damage_for_melee() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, weapon_item("Reaper", ItemSlotType::MainHand, 20.0, 30.0, 0.5)),
        ]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);

        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        combatant.apply_equipment(&loadout, &items);

        // Weapon should replace attack_damage with average
        assert_eq!(combatant.attack_damage, 25.0); // (20+30)/2
        assert_eq!(combatant.attack_speed, 0.5);
        // attack_power from weapon should still be added
        assert_eq!(combatant.attack_power, 30.0 + 5.0); // base 30 + weapon 5
    }

    #[test]
    fn apply_equipment_weapon_replaces_damage_for_ranged() {
        let items = make_item_defs(vec![
            (ItemId::WandOfShadows, weapon_item("Wand", ItemSlotType::Ranged, 10.0, 14.0, 0.8)),
        ]);
        // Mage is ranged, so Ranged slot is primary weapon slot
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Mage);

        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Ranged, ItemId::WandOfShadows);
        combatant.apply_equipment(&loadout, &items);

        assert_eq!(combatant.attack_damage, 12.0); // (10+14)/2
        assert_eq!(combatant.attack_speed, 0.8);
    }

    #[test]
    fn apply_equipment_offhand_weapon_does_not_replace_damage() {
        let items = make_item_defs(vec![
            (ItemId::WallOfTheDeadShield, weapon_item("Shield", ItemSlotType::OffHand, 100.0, 200.0, 2.0)),
        ]);
        let mut combatant = super::super::components::combatant::Combatant::new(1, 0, CharacterClass::Warrior);
        let base_damage = combatant.attack_damage;
        let base_speed = combatant.attack_speed;

        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);
        combatant.apply_equipment(&loadout, &items);

        // Off hand weapon should NOT replace attack damage/speed
        assert_eq!(combatant.attack_damage, base_damage);
        assert_eq!(combatant.attack_speed, base_speed);
        // But attack_power from the off-hand should still be added
        assert_eq!(combatant.attack_power, 30.0 + 5.0);
    }

    // ---- resolve_loadout tests ----

    #[test]
    fn resolve_loadout_uses_defaults_when_no_overrides() {
        let mut loadout_map = HashMap::new();
        let mut warrior_loadout = Loadout::new();
        warrior_loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        warrior_loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout_map.insert(CharacterClass::Warrior, warrior_loadout);

        let defaults = DefaultLoadouts { loadouts: loadout_map };
        let overrides = Loadout::new();

        let result = resolve_loadout(CharacterClass::Warrior, &defaults, &overrides);
        assert_eq!(result.get(&ItemSlot::Head), Some(&ItemId::LionheartHelm));
        assert_eq!(result.get(&ItemSlot::MainHand), Some(&ItemId::ArcaniteReaper));
    }

    #[test]
    fn resolve_loadout_overrides_replace_defaults() {
        let mut loadout_map = HashMap::new();
        let mut warrior_loadout = Loadout::new();
        warrior_loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout_map.insert(CharacterClass::Warrior, warrior_loadout);

        let defaults = DefaultLoadouts { loadouts: loadout_map };
        let mut overrides = Loadout::new();
        overrides.insert(ItemSlot::MainHand, ItemId::FrostbiteBlade);

        let result = resolve_loadout(CharacterClass::Warrior, &defaults, &overrides);
        assert_eq!(result.get(&ItemSlot::MainHand), Some(&ItemId::FrostbiteBlade));
    }

    #[test]
    fn resolve_loadout_missing_class_returns_only_overrides() {
        let defaults = DefaultLoadouts { loadouts: HashMap::new() };
        let mut overrides = Loadout::new();
        overrides.insert(ItemSlot::Head, ItemId::LionheartHelm);

        let result = resolve_loadout(CharacterClass::Warrior, &defaults, &overrides);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get(&ItemSlot::Head), Some(&ItemId::LionheartHelm));
    }

    // ---- weapon proficiency (AS-59) ----

    /// `WeaponType::all()` must list every variant exactly once.
    ///
    /// The match below has no wildcard arm, so a NEW variant does not compile
    /// until it is given a position here — and the position assertion then
    /// fails until it is added to `all()` too. That is the whole guard: this
    /// repo's recurring defect is the hand-maintained list that silently omits
    /// a variant.
    #[test]
    fn weapon_type_all_lists_every_variant() {
        fn position(weapon: WeaponType) -> usize {
            match weapon {
                WeaponType::Sword => 0,
                WeaponType::Mace => 1,
                WeaponType::Axe => 2,
                WeaponType::Dagger => 3,
                WeaponType::Staff => 4,
                WeaponType::Polearm => 5,
                WeaponType::Fist => 6,
                WeaponType::Bow => 7,
                WeaponType::Gun => 8,
                WeaponType::Crossbow => 9,
                WeaponType::Wand => 10,
                WeaponType::Thrown => 11,
                WeaponType::Shield => 12,
                WeaponType::OffhandFrill => 13,
                WeaponType::None => 14,
            }
        }

        let all = WeaponType::all();
        assert_eq!(
            all.len(),
            15,
            "a WeaponType variant is missing from (or duplicated in) all()"
        );
        for (index, weapon) in all.iter().enumerate() {
            assert_eq!(position(*weapon), index, "{:?} is out of place in all()", weapon);
        }
    }

    /// The proficiency table answers for every (class, weapon) pair.
    ///
    /// `weapon_proficiency`'s matches are wildcard-free, so coverage is
    /// actually enforced by the COMPILER — a new `CharacterClass` or
    /// `WeaponType` variant fails to build until every row handles it. This
    /// test is the runtime half: it walks the full cross product so a table
    /// that ever grows a fallible lookup is caught, and pins the invariant
    /// that a one-handed-only skill denies the two-handed form and nothing
    /// else.
    #[test]
    fn every_class_has_a_proficiency_for_every_weapon_type() {
        for class in CharacterClass::all() {
            for weapon in WeaponType::all() {
                match weapon_proficiency(*class, *weapon) {
                    Proficiency::Untrained => {
                        assert!(!can_wield(*class, *weapon, false));
                        assert!(!can_wield(*class, *weapon, true));
                    }
                    Proficiency::OneHandedOnly => {
                        assert!(can_wield(*class, *weapon, false));
                        assert!(!can_wield(*class, *weapon, true));
                    }
                    Proficiency::Trained => {
                        assert!(can_wield(*class, *weapon, false));
                        assert!(can_wield(*class, *weapon, true));
                    }
                }
            }
        }
    }

    /// The WoW Classic rows, spot-checked where they bite. Each of these is a
    /// weapon the class in question could never train.
    #[test]
    fn proficiencies_match_wow_classic() {
        use CharacterClass as C;
        use Proficiency::{OneHandedOnly, Trained, Untrained};
        use WeaponType as W;

        // The card's headline case: a Mage may not carry an axe.
        assert_eq!(weapon_proficiency(C::Mage, W::Axe), Untrained);
        assert_eq!(weapon_proficiency(C::Mage, W::Sword), OneHandedOnly);
        assert_eq!(weapon_proficiency(C::Mage, W::Staff), Trained);
        assert_eq!(weapon_proficiency(C::Mage, W::Wand), Trained);

        // Warriors train everything but wands.
        assert_eq!(weapon_proficiency(C::Warrior, W::Wand), Untrained);
        assert_eq!(weapon_proficiency(C::Warrior, W::Axe), Trained);

        // Paladins: no dagger, no staff, nothing ranged.
        assert_eq!(weapon_proficiency(C::Paladin, W::Dagger), Untrained);
        assert_eq!(weapon_proficiency(C::Paladin, W::Staff), Untrained);
        assert_eq!(weapon_proficiency(C::Paladin, W::Bow), Untrained);
        assert_eq!(weapon_proficiency(C::Paladin, W::Shield), Trained);

        // Hunters: no mace, no shield.
        assert_eq!(weapon_proficiency(C::Hunter, W::Mace), Untrained);
        assert_eq!(weapon_proficiency(C::Hunter, W::Shield), Untrained);
        assert_eq!(weapon_proficiency(C::Hunter, W::Bow), Trained);

        // Rogues: no axes (those arrive with the Burning Crusade), no
        // two-handers of any kind.
        assert_eq!(weapon_proficiency(C::Rogue, W::Axe), Untrained);
        assert_eq!(weapon_proficiency(C::Rogue, W::Sword), OneHandedOnly);
        assert_eq!(weapon_proficiency(C::Rogue, W::Mace), OneHandedOnly);
        assert_eq!(weapon_proficiency(C::Rogue, W::Dagger), Trained);

        // Shamans: no sword, ever.
        assert_eq!(weapon_proficiency(C::Shaman, W::Sword), Untrained);
        assert_eq!(weapon_proficiency(C::Shaman, W::Axe), Trained);
        assert_eq!(weapon_proficiency(C::Shaman, W::Shield), Trained);

        // Priests: dagger, one-handed mace, staff, wand — and nothing else.
        assert_eq!(weapon_proficiency(C::Priest, W::Mace), OneHandedOnly);
        assert_eq!(weapon_proficiency(C::Priest, W::Shield), Untrained);
        assert_eq!(weapon_proficiency(C::Priest, W::Sword), Untrained);

        // Warlocks share the Mage's list.
        for weapon in WeaponType::all() {
            assert_eq!(
                weapon_proficiency(C::Warlock, *weapon),
                weapon_proficiency(C::Mage, *weapon),
                "{:?} splits the Mage and Warlock lists",
                weapon
            );
        }

        // A held-in-off-hand item is not a weapon and needs no proficiency.
        for class in CharacterClass::all() {
            assert_eq!(weapon_proficiency(*class, W::OffhandFrill), Trained);
            assert_eq!(weapon_proficiency(*class, W::None), Trained);
        }
    }

    /// The shipped item set, filtered by the shipped rule. This is the card's
    /// repro, run against real data rather than fixtures.
    #[test]
    fn shipped_weapons_respect_proficiency() {
        let items = load_item_definitions().expect("items.ron must load");
        let reaper = items.get(&ItemId::ArcaniteReaper).expect("Arcanite Reaper");

        assert!(!can_equip(CharacterClass::Mage, reaper), "a Mage cannot wield a two-handed axe");
        assert!(can_equip(CharacterClass::Warrior, reaper));

        let mage_main_hands = items.items_for_slot(ItemSlot::MainHand, CharacterClass::Mage);
        assert!(!mage_main_hands.is_empty(), "a Mage keeps daggers, one-handed swords and staves");
        for (id, item) in &mage_main_hands {
            assert!(
                matches!(item.weapon_type, WeaponType::Dagger | WeaponType::Sword | WeaponType::Staff),
                "{:?} is not in the Mage's weapon list",
                id
            );
        }

        // The other half of the card: a Priest may not carry a shield.
        let shield = items.get(&ItemId::WallOfTheDeadShield).expect("Wall of the Dead");
        assert!(!can_equip(CharacterClass::Priest, shield));
        assert!(can_equip(CharacterClass::Paladin, shield));

        // A Hunter's bow stays a Hunter's bow, and stays off a Priest.
        let bow = items.get(&ItemId::AshwoodBow).expect("Ashwood Bow");
        assert!(can_equip(CharacterClass::Hunter, bow));
        assert!(!can_equip(CharacterClass::Priest, bow));
    }

    /// Weapon-slot items must declare a weapon type, and nothing else may.
    ///
    /// The proficiency gate keys off `weapon_type`, so a weapon that forgot to
    /// declare one would default to `WeaponType::None` and be equippable by
    /// everybody — the exact hole this card closes, reopened by a data edit.
    #[test]
    fn weapon_slot_items_declare_a_weapon_type() {
        let items = load_item_definitions().expect("items.ron must load");
        for (id, item) in items.iter() {
            if item.slot.is_weapon_slot() {
                assert_ne!(
                    item.weapon_type,
                    WeaponType::None,
                    "{:?} sits in a weapon socket without a weapon_type — every class could equip it",
                    id
                );
            } else {
                assert_eq!(
                    item.weapon_type,
                    WeaponType::None,
                    "{:?} is not a weapon but declares a weapon_type",
                    id
                );
            }
        }
    }

    /// A socket and the item kind it accepts must agree on what a weapon is.
    #[test]
    fn socket_and_kind_agree_on_weapons() {
        for slot in ItemSlot::all() {
            assert_eq!(slot.is_weapon_slot(), slot.slot_type().is_weapon_slot());
        }
        for kind in ItemSlotType::all() {
            for socket in kind.sockets() {
                assert_eq!(kind.is_weapon_slot(), socket.is_weapon_slot());
            }
        }
    }

    /// Every shipped default loadout stays equippable under the tightened rule
    /// — the card's hard constraint. A loadout that stopped resolving would
    /// silently change a combatant's stats and therefore match outcomes.
    #[test]
    fn default_loadouts_are_classic_legal() {
        let items = load_item_definitions().expect("items.ron must load");
        let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

        for class in CharacterClass::all() {
            let loadout = defaults.get(*class).unwrap_or_else(|| {
                panic!("{} has no default loadout", class.name())
            });
            validate_class_restrictions(*class, loadout, &items)
                .unwrap_or_else(|e| panic!("{}: {}", class.name(), e));

            // Non-vacuity: the assertion above is only meaningful if these
            // loadouts actually carry weapons for the rule to judge.
            let weapons = loadout
                .iter()
                .filter(|(slot, _)| slot.is_weapon_slot())
                .count();
            assert!(weapons > 0, "{}'s default loadout carries no weapon", class.name());

            // The resolver must not strip anything from a default loadout.
            let resolved = resolve_equipped_loadout(*class, &defaults, &Loadout::new(), &items);
            assert_eq!(&resolved, loadout, "{}'s default loadout was altered by the resolver", class.name());

            // Every class keeps a one-handed main-hand to fall back on when an
            // off-hand displaces a two-hander.
            assert!(
                find_one_handed_mainhand(&items, *class).is_some(),
                "{} has no one-handed main-hand it may wield",
                class.name()
            );
        }
    }

    /// An override the class may not wear is stripped, not applied.
    ///
    /// The live case: configure a Warrior slot with a two-handed axe, then
    /// change that slot's class to Mage. The override outlives the class it
    /// was chosen for, so the resolver — not the picker — has to be the thing
    /// that guarantees a legal loadout.
    #[test]
    fn resolver_strips_an_override_the_class_cannot_equip() {
        let items = load_item_definitions().expect("items.ron must load");
        let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");

        let mut overrides = Loadout::new();
        overrides.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        overrides.insert(ItemSlot::Ring1, ItemId::LionheartHelm); // a helm in a ring socket

        let resolved = resolve_equipped_loadout(CharacterClass::Mage, &defaults, &overrides, &items);
        assert!(!resolved.contains_key(&ItemSlot::MainHand), "the axe must not survive");
        assert_ne!(
            resolved.get(&ItemSlot::Ring1),
            Some(&ItemId::LionheartHelm),
            "a helm must not be worn as a ring"
        );
        // The rest of the Mage's default kit is untouched.
        assert_eq!(resolved.get(&ItemSlot::Head), Some(&ItemId::MagistersCrown));
        assert_eq!(resolved.get(&ItemSlot::Ranged), Some(&ItemId::WandOfShadows));

        // A legal override still lands.
        let mut legal = Loadout::new();
        legal.insert(ItemSlot::MainHand, ItemId::CrescentStaff);
        let resolved = resolve_equipped_loadout(CharacterClass::Mage, &defaults, &legal, &items);
        assert_eq!(resolved.get(&ItemSlot::MainHand), Some(&ItemId::CrescentStaff));
        // ...and the two-hand pass still runs after the class pass.
        assert!(!resolved.contains_key(&ItemSlot::OffHand), "a two-hander clears the off-hand");
    }

    // ---- can_equip tests ----

    #[test]
    fn can_equip_plate_on_warrior() {
        let item = armor_item("Plate Helm", ItemSlotType::Head, ArmorType::Plate);
        assert!(can_equip(CharacterClass::Warrior, &item));
    }

    #[test]
    fn can_equip_plate_on_mage_fails() {
        let item = armor_item("Plate Helm", ItemSlotType::Head, ArmorType::Plate);
        assert!(!can_equip(CharacterClass::Mage, &item));
    }

    #[test]
    fn can_equip_cloth_on_warrior() {
        let item = armor_item("Cloth Robe", ItemSlotType::Chest, ArmorType::Cloth);
        assert!(can_equip(CharacterClass::Warrior, &item));
    }

    #[test]
    fn can_equip_class_restricted_item() {
        let mut item = armor_item("Warrior Only Helm", ItemSlotType::Head, ArmorType::Plate);
        item.allowed_classes = Some(vec![CharacterClass::Warrior]);
        assert!(can_equip(CharacterClass::Warrior, &item));
        assert!(!can_equip(CharacterClass::Paladin, &item));
    }

    #[test]
    fn can_equip_accessory_on_any_class() {
        let item = armor_item("Ring", ItemSlotType::Ring, ArmorType::None);
        assert!(can_equip(CharacterClass::Mage, &item));
        assert!(can_equip(CharacterClass::Warrior, &item));
        assert!(can_equip(CharacterClass::Rogue, &item));
    }

    // ---- format_loadout tests ----

    #[test]
    fn format_loadout_empty() {
        let items = make_item_defs(vec![]);
        let loadout = Loadout::new();
        assert_eq!(format_loadout(&loadout, &items), "No equipment");
    }

    #[test]
    fn format_loadout_single_item() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Lionheart Helm", ItemSlotType::Head, ArmorType::Plate)),
        ]);
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        let result = format_loadout(&loadout, &items);
        assert_eq!(result, "Head=Lionheart Helm");
    }

    #[test]
    fn format_loadout_respects_slot_order() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, weapon_item("Arcanite Reaper", ItemSlotType::MainHand, 20.0, 30.0, 0.5)),
            (ItemId::LionheartHelm, armor_item("Lionheart Helm", ItemSlotType::Head, ArmorType::Plate)),
        ]);
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        let result = format_loadout(&loadout, &items);
        // Head comes before MainHand in ItemSlot::all() ordering
        assert!(result.starts_with("Head="));
        assert!(result.contains("Main Hand=Arcanite Reaper"));
    }

    // ---- validate_class_restrictions tests ----

    #[test]
    fn validate_class_restrictions_passes_for_valid_loadout() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlotType::Head, ArmorType::Plate)),
        ]);
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Warrior, &loadout, &items).is_ok());
    }

    #[test]
    fn validate_class_restrictions_fails_wrong_armor_type() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlotType::Head, ArmorType::Plate)),
        ]);
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Mage, &loadout, &items).is_err());
    }

    #[test]
    fn validate_class_restrictions_fails_wrong_slot() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Helm", ItemSlotType::Head, ArmorType::Plate)),
        ]);
        let mut loadout = Loadout::new();
        // Place a Head item in the Chest slot
        loadout.insert(ItemSlot::Chest, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Warrior, &loadout, &items).is_err());
    }

    #[test]
    fn validate_class_restrictions_fails_unknown_item() {
        let items = make_item_defs(vec![]); // empty
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);
        assert!(validate_class_restrictions(CharacterClass::Warrior, &loadout, &items).is_err());
    }

    // ---- socket / slot-type mapping tests ----

    #[test]
    fn sockets_and_slot_types_are_inverse() {
        // Every socket's kind lists that socket among its sockets, and every
        // kind's sockets all report that kind back.
        for socket in ItemSlot::all() {
            assert!(
                socket.slot_type().sockets().contains(socket),
                "{:?} is missing from {:?}'s socket list",
                socket,
                socket.slot_type()
            );
        }
        for slot_type in ItemSlotType::all() {
            for socket in slot_type.sockets() {
                assert_eq!(socket.slot_type(), *slot_type);
            }
        }
        // Every socket is claimed by exactly one kind — no socket is orphaned.
        let claimed: usize = ItemSlotType::all().iter().map(|t| t.sockets().len()).sum();
        assert_eq!(claimed, ItemSlot::all().len());
    }

    #[test]
    fn both_ring_sockets_accept_rings() {
        assert!(ItemSlot::Ring1.accepts(ItemSlotType::Ring));
        assert!(ItemSlot::Ring2.accepts(ItemSlotType::Ring));
        assert!(ItemSlot::Trinket1.accepts(ItemSlotType::Trinket));
        assert!(ItemSlot::Trinket2.accepts(ItemSlotType::Trinket));
    }

    #[test]
    fn sockets_reject_other_slot_types() {
        assert!(!ItemSlot::Ring1.accepts(ItemSlotType::Neck));
        assert!(!ItemSlot::Head.accepts(ItemSlotType::Chest));
        assert!(!ItemSlot::Trinket1.accepts(ItemSlotType::Ring));
    }

    #[test]
    fn only_paired_sockets_have_siblings() {
        assert_eq!(ItemSlot::Ring1.sibling(), Some(ItemSlot::Ring2));
        assert_eq!(ItemSlot::Ring2.sibling(), Some(ItemSlot::Ring1));
        assert_eq!(ItemSlot::Trinket1.sibling(), Some(ItemSlot::Trinket2));
        assert_eq!(ItemSlot::Trinket2.sibling(), Some(ItemSlot::Trinket1));
        assert_eq!(ItemSlot::Head.sibling(), None);
        assert_eq!(ItemSlot::MainHand.sibling(), None);
    }

    #[test]
    fn sibling_pairs_are_exactly_the_two_socket_kinds() {
        // The unique-equipped pairing is derived from `sockets()`, so it must
        // cover every kind with two sockets and nothing else, and it must
        // agree with `sibling()` in both directions.
        let pairs: Vec<_> = sibling_socket_pairs().collect();
        let two_socket_kinds = ItemSlotType::all()
            .iter()
            .filter(|kind| kind.sockets().len() == 2)
            .count();
        assert_eq!(pairs.len(), two_socket_kinds);
        assert_eq!(
            pairs,
            vec![
                (ItemSlot::Ring1, ItemSlot::Ring2),
                (ItemSlot::Trinket1, ItemSlot::Trinket2),
            ]
        );
        for (primary, secondary) in &pairs {
            assert_eq!(primary.sibling(), Some(*secondary));
            assert_eq!(secondary.sibling(), Some(*primary));
            assert_eq!(primary.slot_type(), secondary.slot_type());
        }
        // `sibling()` models pairs only; a kind with three sockets would need
        // both it and the pairing reshaped, so pin the ceiling.
        for kind in ItemSlotType::all() {
            assert!(kind.sockets().len() <= 2, "{:?} has more than two sockets", kind);
        }
    }

    #[test]
    fn slot_type_names_carry_no_socket_number() {
        for slot_type in ItemSlotType::all() {
            let name = slot_type.name();
            assert!(
                !name.contains('1') && !name.contains('2'),
                "{:?} labels items with a socket number: {:?}",
                slot_type,
                name
            );
        }
    }

    // ---- items_for_slot tests ----

    #[test]
    fn items_for_slot_filters_by_armor_type() {
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, armor_item("Plate Helm", ItemSlotType::Head, ArmorType::Plate)),
            (ItemId::MagistersCrown, armor_item("Cloth Crown", ItemSlotType::Head, ArmorType::Cloth)),
        ]);
        // Warrior can wear plate; Mage cannot
        let warrior_head = items.items_for_slot(ItemSlot::Head, CharacterClass::Warrior);
        assert_eq!(warrior_head.len(), 2); // warrior can wear both plate and cloth
        let mage_head = items.items_for_slot(ItemSlot::Head, CharacterClass::Mage);
        assert_eq!(mage_head.len(), 1); // mage can only wear cloth
        assert_eq!(mage_head[0].0, ItemId::MagistersCrown);
    }

    #[test]
    fn items_for_slot_ring2_shows_all_rings() {
        let items = make_item_defs(vec![
            (ItemId::BandOfAccuria, armor_item("Band of Accuria", ItemSlotType::Ring, ArmorType::None)),
            (ItemId::RingOfProtection, armor_item("Ring of Protection", ItemSlotType::Ring, ArmorType::None)),
            (ItemId::SignetOfFocus, armor_item("Signet of Focus", ItemSlotType::Ring, ArmorType::None)),
        ]);
        let ring2_items = items.items_for_slot(ItemSlot::Ring2, CharacterClass::Mage);
        assert_eq!(ring2_items.len(), 3); // all ring items available for Ring2
    }

    #[test]
    fn items_for_slot_trinket_shows_all_trinkets() {
        let items = make_item_defs(vec![
            (ItemId::MarkOfTheChampion, armor_item("Mark of Champion", ItemSlotType::Trinket, ArmorType::None)),
            (ItemId::EssenceOfEternalLife, armor_item("Essence of Life", ItemSlotType::Trinket, ArmorType::None)),
        ]);
        let trinket2_items = items.items_for_slot(ItemSlot::Trinket2, CharacterClass::Warrior);
        assert_eq!(trinket2_items.len(), 2); // both trinkets available for Trinket2
    }

    #[test]
    fn items_for_slot_respects_class_restrictions() {
        let mut warrior_only = armor_item("Warrior Helm", ItemSlotType::Head, ArmorType::Plate);
        warrior_only.allowed_classes = Some(vec![CharacterClass::Warrior]);
        let items = make_item_defs(vec![
            (ItemId::LionheartHelm, warrior_only),
        ]);
        let warrior_items = items.items_for_slot(ItemSlot::Head, CharacterClass::Warrior);
        assert_eq!(warrior_items.len(), 1);
        let paladin_items = items.items_for_slot(ItemSlot::Head, CharacterClass::Paladin);
        assert_eq!(paladin_items.len(), 0);
    }

    #[test]
    fn items_for_slot_sorted_by_name() {
        let items = make_item_defs(vec![
            (ItemId::BandOfAccuria, armor_item("Zebra Ring", ItemSlotType::Ring, ArmorType::None)),
            (ItemId::SignetOfFocus, armor_item("Alpha Ring", ItemSlotType::Ring, ArmorType::None)),
        ]);
        let ring_items = items.items_for_slot(ItemSlot::Ring1, CharacterClass::Warrior);
        assert_eq!(ring_items[0].1.name, "Alpha Ring");
        assert_eq!(ring_items[1].1.name, "Zebra Ring");
    }

    // ---- enforce_two_hand_conflicts tests ----

    fn two_handed_weapon(name: &str) -> ItemConfig {
        let mut item = weapon_item(name, ItemSlotType::MainHand, 20.0, 30.0, 0.9);
        item.two_handed = true;
        item
    }

    #[test]
    fn enforce_2h_strips_offhand_when_mainhand_is_2h() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, two_handed_weapon("Arcanite Reaper")),
            (ItemId::WallOfTheDeadShield, armor_item("Shield", ItemSlotType::OffHand, ArmorType::None)),
        ]);
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);

        enforce_two_hand_conflicts(&mut loadout, &items);

        assert_eq!(loadout.get(&ItemSlot::MainHand), Some(&ItemId::ArcaniteReaper));
        assert!(!loadout.contains_key(&ItemSlot::OffHand), "Off-hand should be stripped when 2H is equipped");
    }

    #[test]
    fn enforce_2h_keeps_offhand_when_mainhand_is_1h() {
        let items = make_item_defs(vec![
            (ItemId::FrostbiteBlade, weapon_item("Frostbite", ItemSlotType::MainHand, 10.0, 14.0, 1.1)),
            (ItemId::WallOfTheDeadShield, armor_item("Shield", ItemSlotType::OffHand, ArmorType::None)),
        ]);
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::MainHand, ItemId::FrostbiteBlade);
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);

        enforce_two_hand_conflicts(&mut loadout, &items);

        assert!(loadout.contains_key(&ItemSlot::OffHand), "Off-hand should remain with 1H weapon");
    }

    #[test]
    fn enforce_2h_no_mainhand_is_noop() {
        let items = make_item_defs(vec![
            (ItemId::WallOfTheDeadShield, armor_item("Shield", ItemSlotType::OffHand, ArmorType::None)),
        ]);
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::OffHand, ItemId::WallOfTheDeadShield);

        enforce_two_hand_conflicts(&mut loadout, &items);

        assert!(loadout.contains_key(&ItemSlot::OffHand), "Off-hand should remain when no main-hand");
    }

    // ---- unique-equipped tests ----

    fn ring_defs() -> ItemDefinitions {
        make_item_defs(vec![
            (ItemId::BandOfAccuria, armor_item("Band of Accuria", ItemSlotType::Ring, ArmorType::None)),
            (ItemId::RingOfProtection, armor_item("Ring of Protection", ItemSlotType::Ring, ArmorType::None)),
            (ItemId::SignetOfFocus, armor_item("Signet of Focus", ItemSlotType::Ring, ArmorType::None)),
            (ItemId::MarkOfTheChampion, armor_item("Mark of the Champion", ItemSlotType::Trinket, ArmorType::None)),
        ])
    }

    #[test]
    fn enforce_unique_strips_the_secondary_socket() {
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Ring1, ItemId::BandOfAccuria);
        loadout.insert(ItemSlot::Ring2, ItemId::BandOfAccuria);

        enforce_unique_equipped(&mut loadout);

        assert_eq!(loadout.get(&ItemSlot::Ring1), Some(&ItemId::BandOfAccuria));
        assert!(!loadout.contains_key(&ItemSlot::Ring2), "the duplicate ring should be stripped");
    }

    #[test]
    fn enforce_unique_keeps_two_different_rings() {
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Ring1, ItemId::BandOfAccuria);
        loadout.insert(ItemSlot::Ring2, ItemId::RingOfProtection);
        loadout.insert(ItemSlot::Trinket1, ItemId::MarkOfTheChampion);

        enforce_unique_equipped(&mut loadout);

        assert_eq!(loadout.len(), 3, "distinct items in sibling sockets are legal");
    }

    #[test]
    fn validate_unique_rejects_a_duplicate_and_accepts_distinct_items() {
        let items = ring_defs();

        let mut duped = Loadout::new();
        duped.insert(ItemSlot::Ring1, ItemId::BandOfAccuria);
        duped.insert(ItemSlot::Ring2, ItemId::BandOfAccuria);
        let err = validate_unique_equipped(&duped, &items).unwrap_err();
        assert!(err.contains("Band of Accuria"), "error should name the item: {}", err);
        assert!(err.contains("unique-equipped"));

        let mut fine = Loadout::new();
        fine.insert(ItemSlot::Ring1, ItemId::BandOfAccuria);
        fine.insert(ItemSlot::Ring2, ItemId::RingOfProtection);
        assert!(validate_unique_equipped(&fine, &items).is_ok());
    }

    #[test]
    fn picker_hides_the_item_worn_in_the_sibling_socket() {
        let items = ring_defs();
        let mut loadout = Loadout::new();
        loadout.insert(ItemSlot::Ring1, ItemId::BandOfAccuria);

        let ring2 = items.selectable_items_for_slot(ItemSlot::Ring2, CharacterClass::Mage, &loadout);
        let offered: Vec<ItemId> = ring2.iter().map(|(id, _)| *id).collect();
        assert!(!offered.contains(&ItemId::BandOfAccuria), "the worn ring is not selectable again");
        assert_eq!(offered.len(), 2, "the other two rings stay on offer");

        // A 1:1 socket has no sibling, so nothing is ever hidden from it.
        let trinket = items.selectable_items_for_slot(ItemSlot::Trinket1, CharacterClass::Mage, &loadout);
        assert_eq!(trinket.len(), items.items_for_slot(ItemSlot::Trinket1, CharacterClass::Mage).len());
    }

    /// The bug this model replaced: a ring declared `slot: Ring1` was rejected
    /// by `validate_class_restrictions` in the OTHER ring socket, so half the
    /// ring pool was inequippable in half the ring sockets. Asserted against the
    /// real `items.ron`, so a future item that re-bakes a socket fails here.
    #[test]
    fn every_item_is_equippable_in_every_socket_of_its_kind() {
        let items = load_item_definitions().expect("items.ron must load");
        for (id, item) in items.iter() {
            let sockets = item.slot.sockets();
            assert!(!sockets.is_empty(), "{:?} has a slot kind with no socket", id);
            for socket in sockets {
                assert!(socket.accepts(item.slot), "{:?} is rejected by {:?}", id, socket);
                let mut loadout = Loadout::new();
                loadout.insert(*socket, *id);
                let class = *CharacterClass::all()
                    .iter()
                    .find(|c| can_equip(**c, item))
                    .unwrap_or_else(|| panic!("{:?} is equippable by no class", id));
                validate_class_restrictions(class, &loadout, &items)
                    .unwrap_or_else(|e| panic!("{:?} in {:?}: {}", id, socket, e));
            }
        }
    }

    #[test]
    fn shipped_default_loadouts_are_unique_equipped() {
        let items = load_item_definitions().expect("items.ron must load");
        let defaults = load_default_loadouts(&items).expect("loadouts.ron must load");
        for class in CharacterClass::all() {
            if let Some(loadout) = defaults.get(*class) {
                validate_unique_equipped(loadout, &items)
                    .unwrap_or_else(|e| panic!("{}: {}", class.name(), e));
            }
        }
    }

    // ---- find_one_handed_mainhand tests ----

    #[test]
    fn find_1h_returns_first_non_2h_weapon() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, two_handed_weapon("Arcanite Reaper")),
            (ItemId::FrostbiteBlade, weapon_item("Frostbite Blade", ItemSlotType::MainHand, 10.0, 14.0, 1.1)),
        ]);
        let result = find_one_handed_mainhand(&items, CharacterClass::Warrior);
        assert_eq!(result, Some(ItemId::FrostbiteBlade));
    }

    #[test]
    fn find_1h_returns_none_when_only_2h_exist() {
        let items = make_item_defs(vec![
            (ItemId::ArcaniteReaper, two_handed_weapon("Arcanite Reaper")),
            (ItemId::CrescentStaff, two_handed_weapon("Crescent Staff")),
        ]);
        let result = find_one_handed_mainhand(&items, CharacterClass::Warrior);
        assert_eq!(result, None);
    }

    // ---- budget validation tests ----

    /// Build a minimal item for budget testing with specific stats
    fn budget_test_item(slot: ItemSlotType, item_level: u32) -> ItemConfig {
        ItemConfig {
            name: "Test Item".to_string(),
            item_level,
            item_tier: 0,
            icon: String::new(),
            slot,
            armor_type: ArmorType::None,
            weapon_type: WeaponType::None,
            allowed_classes: None,
            is_weapon: false,
            two_handed: false,
            max_health: 0.0,
            max_mana: 0.0,
            mana_regen: 0.0,
            attack_power: 0.0,
            spell_power: 0.0,
            crit_chance: 0.0,
            movement_speed: 0.0,
            armor: 0.0,
            fire_resistance: 0.0,
            frost_resistance: 0.0,
            shadow_resistance: 0.0,
            arcane_resistance: 0.0,
            nature_resistance: 0.0,
            holy_resistance: 0.0,
            attack_damage_min: 0.0,
            attack_damage_max: 0.0,
            attack_speed: 0.0,
        }
    }

    #[test]
    fn budget_item_within_budget_passes() {
        let mut item = budget_test_item(ItemSlotType::Head, 60);
        item.max_health = 10.0;
        item.attack_power = 5.0;
        // usage = 10*1.0 + 5*1.5 = 17.5, budget = 60*0.75*1.0 = 45
        assert!(validate_item_budget("Test Helm", &item).is_ok());
    }

    #[test]
    fn budget_item_exactly_at_budget_passes() {
        let mut item = budget_test_item(ItemSlotType::Head, 60);
        // budget = 60 * 0.75 * 1.0 = 45.0
        item.max_health = 45.0; // usage = 45.0, exactly at budget
        assert!(validate_item_budget("Test Helm", &item).is_ok());
    }

    #[test]
    fn budget_item_within_tolerance_passes() {
        let mut item = budget_test_item(ItemSlotType::Head, 60);
        // budget = 45.0, max_allowed = 45 * 1.05 = 47.25
        item.max_health = 47.0; // 104.4% of budget, within 5% tolerance
        assert!(validate_item_budget("Test Helm", &item).is_ok());
    }

    #[test]
    fn budget_item_over_tolerance_fails() {
        let mut item = budget_test_item(ItemSlotType::Head, 60);
        // budget = 45.0, max_allowed = 47.25
        item.max_health = 48.0; // 106.7% of budget, exceeds 5% tolerance
        let result = validate_item_budget("Over Budget Helm", &item);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("Over Budget Helm"));
        assert!(msg.contains("ilvl 60"));
        assert!(msg.contains("Head"));
        assert!(msg.contains("over budget"));
    }

    #[test]
    fn budget_armor_excluded_from_usage() {
        let mut item = budget_test_item(ItemSlotType::Head, 60);
        item.armor = 500.0; // high armor, but free
        item.max_health = 10.0;
        // usage = only 10.0 (armor excluded), budget = 45.0
        assert!(validate_item_budget("Armor Test", &item).is_ok());
        assert_eq!(calculate_budget_usage(&item), 10.0);
    }

    #[test]
    fn budget_weapon_dps_excluded_from_usage() {
        let mut item = budget_test_item(ItemSlotType::MainHand, 60);
        item.is_weapon = true;
        item.attack_damage_min = 100.0;
        item.attack_damage_max = 200.0;
        item.attack_speed = 2.0;
        item.attack_power = 3.0;
        // usage = only 3*1.5 = 4.5 (weapon DPS excluded), budget = 60*0.75*0.5625 = 25.3125
        assert!(validate_item_budget("Weapon Test", &item).is_ok());
        assert_eq!(calculate_budget_usage(&item), 4.5);
    }

    #[test]
    fn budget_zero_stats_passes() {
        let item = budget_test_item(ItemSlotType::Head, 60);
        // zero budgeted stats, budget > 0
        assert!(validate_item_budget("Empty Item", &item).is_ok());
        assert_eq!(calculate_budget_usage(&item), 0.0);
    }

    #[test]
    fn budget_ilvl_zero_fails_with_stats() {
        let mut item = budget_test_item(ItemSlotType::Head, 0);
        item.max_health = 1.0;
        let result = validate_item_budget("Zero iLvl", &item);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("item_level is 0"));
        assert!(!msg.contains("inf"));
    }

    #[test]
    fn budget_ilvl_zero_fails_even_with_zero_stats() {
        let item = budget_test_item(ItemSlotType::Head, 0);
        let result = validate_item_budget("Zero iLvl Empty", &item);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("item_level is 0"));
    }

    // ---- full item pool validation ----

    #[test]
    fn all_items_within_budget() {
        let item_defs = load_item_definitions().expect("items.ron must load");
        let mut violations: Vec<String> = Vec::new();

        for (item_id, item) in &item_defs.definitions {
            if let Err(msg) = validate_item_budget(&format!("{:?}", item_id), item) {
                violations.push(msg);
            }
        }

        assert!(
            violations.is_empty(),
            "Found {} item(s) over budget:\n{}",
            violations.len(),
            violations.join("\n")
        );
    }

    #[test]
    fn all_items_have_icons() {
        let item_defs = load_item_definitions().expect("items.ron must load");
        let mut missing: Vec<String> = Vec::new();

        for (item_id, item) in &item_defs.definitions {
            if item.icon.is_empty() {
                missing.push(format!("{:?} ({}) has no icon", item_id, item.name));
            }
        }

        assert!(
            missing.is_empty(),
            "Found {} item(s) without icons:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }
}
