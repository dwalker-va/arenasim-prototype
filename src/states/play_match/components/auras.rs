use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use super::super::abilities::SpellSchool;
use super::super::ability_config::AbilityConfig;
use super::super::constants::{DR_RESET_TIMER, DR_IMMUNE_LEVEL, DR_MULTIPLIERS};

// ============================================================================
// Aura Types
// ============================================================================

/// Types of aura effects.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize, Default)]
pub enum AuraType {
    /// Reduces movement speed by a percentage (magnitude = multiplier, e.g., 0.7 = 30% slow)
    #[default]
    MovementSpeedSlow,
    /// Prevents movement (rooted in place) - magnitude unused
    Root,
    /// Prevents all actions (movement, casting, auto-attacks, abilities) - magnitude unused
    Stun,
    /// Increases maximum health by a flat amount (magnitude = HP bonus)
    MaxHealthIncrease,
    /// Deals damage periodically (magnitude = damage per tick, tick_interval determines frequency)
    DamageOverTime,
    /// Spell school lockout - prevents casting spells of a specific school
    /// The magnitude field stores the locked school as f32 (cast from SpellSchool enum)
    SpellSchoolLockout,
    /// Reduces healing received by a percentage (magnitude = multiplier, e.g., 0.65 = 35% reduction)
    HealingReduction,
    /// Fear - target runs around randomly, unable to act. Breaks on damage.
    Fear,
    /// Increases maximum mana by a flat amount (magnitude = mana bonus)
    MaxManaIncrease,
    /// Increases attack power by a flat amount (magnitude = AP bonus)
    AttackPowerIncrease,
    /// Shadow Sight - reveals stealthed enemies AND makes the holder visible to enemies
    ShadowSight,
    /// Absorbs incoming damage (magnitude = remaining absorb amount)
    /// When damage is absorbed, magnitude decreases. Aura removed when magnitude reaches 0.
    Absorb,
    /// Weakened Soul - prevents receiving Power Word: Shield (applied by PW:S)
    WeakenedSoul,
    /// Polymorph - target wanders slowly, can't attack/cast, breaks on ANY damage.
    /// Separate from Stun for diminishing returns categories (incapacitates vs stuns).
    Polymorph,
    /// Reduces outgoing physical damage by a percentage (magnitude = 0.2 means 20% reduction)
    /// Used by Curse of Weakness to reduce enemy physical damage dealt.
    DamageReduction,
    /// Increases cast time by a percentage (magnitude = multiplier, e.g., 0.5 = 50% slower)
    /// Used by Curse of Tongues to slow enemy casting.
    CastTimeIncrease,
    /// Reduces incoming damage taken by a percentage (magnitude = 0.10 means 10% reduction)
    /// Used by Devotion Aura to reduce all damage taken by the target.
    DamageTakenReduction,
    /// Complete damage immunity - all incoming damage is negated, all hostile auras are blocked.
    /// Used by Divine Shield. Magnitude unused (always 1.0 by convention).
    DamageImmunity,
    /// Incapacitate - target is frozen in place, can't attack/cast, breaks on ANY damage.
    /// Unlike Polymorph (target wanders), incapacitated targets stand still.
    /// Shares DRCategory::Incapacitates with Polymorph.
    /// Used by Freezing Trap.
    Incapacitate,
    /// Increases spell resistance for a specific school.
    /// The spell_school field on the Aura identifies which school, magnitude = resistance amount.
    /// Stacks additively with equipment resistance.
    SpellResistanceBuff,
    /// Reduces attack power by a flat amount (magnitude = AP reduction)
    /// Used by Demoralizing Shout to weaken enemies.
    AttackPowerReduction,
    /// Increases critical strike chance by a percentage (magnitude = crit bonus, e.g. 0.05 = 5%)
    /// Used by Molten Armor.
    CritChanceIncrease,
    /// Increases mana regeneration by a flat amount per second (magnitude = mana/sec bonus)
    /// Used by Mage Armor.
    ManaRegenIncrease,
    /// Reduces attack speed by a percentage (magnitude = slow amount, e.g. 0.25 = 25% slower)
    /// Used by Frost Armor proc on melee attackers.
    AttackSpeedSlow,
    /// Reduces spell school lockout duration from interrupts by a percentage
    /// (magnitude = reduction, e.g. 0.50 = 50% shorter lockouts).
    /// Used by Concentration Aura.
    LockoutDurationReduction,
    /// Marks a combatant as having Frost Armor active.
    /// When a melee attacker hits this combatant, they receive MovementSpeedSlow + AttackSpeedSlow.
    /// Magnitude unused (always 1.0 by convention).
    FrostArmorBuff,
    /// Blanket silence — prevents the affected combatant from using any ability that has
    /// a mana cost > 0, but only if the caster's resource type is Mana. Does not affect
    /// rage, energy, or zero-cost abilities (auto-attacks, Divine Shield if zero-cost, etc.).
    /// Applied by Unstable Affliction dispel backlash. Has its own DR category.
    /// Magnitude unused (always 1.0 by convention).
    Silence,
    /// Marker buff signifying a Rogue has a weapon poison coated (e.g. Crippling
    /// Poison). Purely informational — drives the buff-bar indicator; the on-hit
    /// proc logic reads the Rogue's `rogue_poison` config, not this aura.
    /// Magnitude unused (always 1.0 by convention). Not dispellable, no DR.
    WeaponPoison,
    /// Increases spell power by a flat amount (magnitude = spell power bonus).
    /// Used by the Shaman's Flametongue Totem. (Behavior wired in U2.)
    SpellPowerIncrease,
    /// Heals the affected combatant periodically (magnitude = healing per tick,
    /// tick_interval determines frequency). Used by the Shaman's Healing Stream
    /// Totem. (Behavior wired in U2.)
    HealingOverTime,
    /// Empowers melee allies' auto-attacks with a proc-style bonus attack.
    /// Inert for ranged/caster allies. Used by the Shaman's Windfury Totem.
    /// (Behavior wired in U2.)
    WindfuryBuff,
    /// Immunity to Fear effects — blocks new Fear applications on the holder.
    /// Deliberately does NOT block Death Coil's horror (a Fear-type aura with
    /// `dr_category_override: Some(DRCategory::Horror)`) — horror bypasses fear
    /// immunity, matching WoW TBC. Used by the Warrior's Berserker Rage.
    /// Magnitude unused (always 1.0 by convention). Not purgeable (physical
    /// enrage, not magic).
    FearImmunity,
}

/// A debuff's REMOVAL CLASS — what kind of thing it is for the purpose of
/// taking it off. Orthogonal to `AuraType`, so a single mechanic (e.g. a
/// `MovementSpeedSlow`) can be magic, poison, curse OR physical.
///
/// This is the only axis that decides removability — `AuraType` says what an
/// effect does, not how it comes off. The distinction matters because the same
/// mechanic arrives by different means: a slow is Frostbolt's frost magic,
/// Crippling Poison's coating, or Concussive Shot's arrow to the leg, and only
/// the first of those is something a dispel can lift.
///
/// The class is a property of the DEBUFF; which abilities act on each class is
/// a property of the ABILITIES, and lives in their predicates
/// ([`Aura::can_be_dispelled`], [`Aura::is_cleansable_poison`]). Nothing here
/// names an ability, so adding a decurse — or a second effect that clears
/// physical harm — changes the predicates and nothing else. The classes, and
/// who serves them today:
///
/// | class    | removed by                                                     |
/// |----------|----------------------------------------------------------------|
/// | Magic    | dispels (Dispel Magic, Cleanse, Devour Magic)                   |
/// | Poison   | cleanses (Cleanse)                                              |
/// | Disease  | cleanses — no disease exists in the sim yet                     |
/// | Curse    | curse-removal — NOTHING in the sim yet                          |
/// | Physical | effects that clear physical harm — never a dispel               |
///
/// A class with no server today is still a real class, not "permanent": a curse
/// is removable in principle, and the arena simply has no ability that does it.
/// Player-facing wording has to say that rather than claim nothing takes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum DispelType {
    /// Derive the class from the applying ability's school: the `Physical`
    /// school yields [`DispelType::Physical`], every other school yields MAGIC.
    /// The default, and the right answer for most abilities.
    ///
    /// Magic is the only class whose removability then also depends on the
    /// MECHANIC — a magic stun is still not dispellable, because stuns are not
    /// (see [`AuraType::is_magic_dispellable`]).
    #[default]
    Auto,
    /// A poison debuff — no dispel touches it; a cleanse does. Crippling Poison
    /// uses this, and has to declare it because its school (`Nature`) would
    /// otherwise read as magic.
    Poison,
    /// A disease debuff — reserved for future use; same shape as `Poison`,
    /// removed by a cleanse rather than a dispel.
    Disease,
    /// A CURSE — Curse of Agony, Curse of Weakness, Curse of Tongues. Removed
    /// by curse-removal effects, of which the arena has NONE (in WoW the Mage's
    /// and Druid's Remove Curse; there is no Druid here and the Mage has no
    /// decurse yet).
    ///
    /// Declared in the RON, never inferred. Curses are Shadow-school and Shadow
    /// is magic, so the school cannot tell Curse of Agony from Corruption; and
    /// inferring from the ability NAME would put a classification rule inside a
    /// display string. `every_curse_declares_the_curse_class` pins the known
    /// family against the data instead, so a fourth curse cannot land untyped.
    ///
    /// A curse is NOT permanent and NOT "immune to removal" — it is waiting for
    /// an ability that does not exist yet. When a decurse lands, the exhaustive
    /// matches on this enum force every removal site to decide about it.
    Curse,
    /// A PHYSICAL debuff — an arrow in the leg, a torn wound, a boot to the
    /// head. There is no magic on it to dissipate, so no ordinary removal
    /// touches it: not a dispel, not a cleanse, not a purge.
    ///
    /// **Physical is not permanent either.** It yields to an effect that clears
    /// physical harm specifically. Such effects are rare and will get less rare
    /// (a PvP trinket, Master's Call); the engine's predicates decide which
    /// abilities qualify, and nothing written about the CLASS should name one —
    /// otherwise every such addition rewrites a tooltip.
    ///
    /// Derived from the applying ability's `spell_school`, not hand-declared:
    /// see [`DispelType::for_ability`]. That keeps one fact ("Concussive Shot
    /// is physical") in one place instead of restating it on the aura, where
    /// the two copies could disagree.
    Physical,
}

impl DispelType {
    /// The removal class an ability's aura carries: the ability's declared
    /// `dispel_type` when it names one, otherwise derived from its school —
    /// a `Physical` ability yields a physical debuff, everything else `Auto`.
    ///
    /// An explicit declaration wins so a magic-school ability can still be a
    /// poison (Crippling Poison is `Nature` + `Poison`) or a curse (the three
    /// Warlock curses are `Shadow` + `Curse`); nothing in the RON declares a
    /// type on a physical ability today, and if one ever does it means the
    /// author knew better than the school.
    ///
    /// Note what is NOT here: no school maps to `Curse`. Curses are Shadow, and
    /// so is Corruption — the school cannot separate them, so the RON must.
    ///
    /// **Exhaustive on purpose — do not add a `_ =>` arm.** Same hazard class
    /// as [`AuraType::is_magic_dispellable`]: a school silently falling into
    /// the wrong arm changes what a dispel can strip, and no test in this repo
    /// observes "the debuff that was never a dispel candidate".
    pub fn for_ability(declared: DispelType, school: SpellSchool) -> DispelType {
        match declared {
            DispelType::Poison => DispelType::Poison,
            DispelType::Disease => DispelType::Disease,
            DispelType::Curse => DispelType::Curse,
            DispelType::Physical => DispelType::Physical,
            DispelType::Auto => match school {
                SpellSchool::Physical => DispelType::Physical,
                // Schoolless is NOT physical. `SpellSchool::None` means "no
                // school, cannot be locked out" — Freezing Trap declares it,
                // yet the trap the engine actually springs is Frost and IS
                // dispellable. Reading schoolless as physical would have made
                // the encyclopedia's Freezing Trap page contradict the trap.
                SpellSchool::None
                | SpellSchool::Frost
                | SpellSchool::Holy
                | SpellSchool::Shadow
                | SpellSchool::Arcane
                | SpellSchool::Fire
                | SpellSchool::Nature => DispelType::Auto,
            },
        }
    }
}

impl AuraType {
    /// Player-facing name of this MECHANIC, as the encyclopedia's mechanic
    /// badge renders it and as the catalog groups siblings by.
    ///
    /// A mechanic is not an aura: players see *Rend*, *Corruption* and *Serpent
    /// Sting*, which share the one `DamageOverTime` mechanic. The catalog is
    /// built from named auras and uses this only for the badge and the
    /// "other X effects" cross-links.
    ///
    /// **Exhaustive on purpose — do not add a `_ =>` arm.** A wildcard would
    /// let variant N+1 ship with a machine-readable label ("MaxHealthIncrease")
    /// in front of players. The compiler asking for a name is the only thing
    /// that catches that, because no test can tell a bad label from a good one.
    pub fn display_name(self) -> &'static str {
        match self {
            AuraType::MovementSpeedSlow => "Slow",
            AuraType::Root => "Root",
            AuraType::Stun => "Stun",
            AuraType::Fear => "Fear",
            AuraType::Polymorph => "Polymorph",
            AuraType::Incapacitate => "Incapacitate",
            AuraType::Silence => "Silence",
            AuraType::SpellSchoolLockout => "School Lockout",
            AuraType::DamageOverTime => "Damage over Time",
            AuraType::HealingOverTime => "Healing over Time",
            AuraType::HealingReduction => "Mortal Wound",
            AuraType::DamageReduction => "Weakness",
            AuraType::CastTimeIncrease => "Casting Slow",
            AuraType::AttackPowerReduction => "Attack Power Reduction",
            AuraType::AttackSpeedSlow => "Attack Speed Slow",
            AuraType::Absorb => "Absorb Shield",
            AuraType::DamageTakenReduction => "Damage Reduction",
            AuraType::DamageImmunity => "Damage Immunity",
            AuraType::FearImmunity => "Fear Immunity",
            AuraType::MaxHealthIncrease => "Health Buff",
            AuraType::MaxManaIncrease => "Mana Buff",
            AuraType::AttackPowerIncrease => "Attack Power Buff",
            AuraType::SpellPowerIncrease => "Spell Power Buff",
            AuraType::CritChanceIncrease => "Critical Strike Buff",
            AuraType::ManaRegenIncrease => "Mana Regeneration Buff",
            AuraType::SpellResistanceBuff => "Resistance Buff",
            AuraType::LockoutDurationReduction => "Lockout Reduction",
            AuraType::WindfuryBuff => "Windfury",
            AuraType::FrostArmorBuff => "Frost Armor",
            AuraType::WeaponPoison => "Weapon Poison",
            AuraType::WeakenedSoul => "Weakened Soul",
            AuraType::ShadowSight => "Shadow Sight",
        }
    }

    /// One player-facing sentence about what this MECHANIC does, independent of
    /// any ability that applies it. Shown on the mechanic badge's tooltip and
    /// above the cross-links to an aura's mechanic siblings.
    ///
    /// Mechanics-honest and written for a player: these are deliberately NOT
    /// the enum's doc comments above, which describe the implementation
    /// (magnitude encodings, "behavior wired in U2", `f32` casts).
    ///
    /// **Exhaustive on purpose — do not add a `_ =>` arm**, for the same reason
    /// as [`AuraType::display_name`].
    pub fn description(self) -> &'static str {
        match self {
            AuraType::MovementSpeedSlow => {
                "Reduces how fast the target can move. Does not stop casting or attacking."
            }
            AuraType::Root => {
                "Pins the target in place. It can still cast, attack and turn — it just cannot move."
            }
            AuraType::Stun => "The target cannot move, attack or cast at all.",
            AuraType::Fear => {
                "The target flees under its own power, unable to act, until the effect ends or \
                 enough damage shakes it loose."
            }
            AuraType::Polymorph => {
                "The target is transformed and wanders harmlessly. Any damage ends it immediately."
            }
            AuraType::Incapacitate => {
                "The target is frozen where it stands, unable to act. Any damage frees it."
            }
            AuraType::Silence => {
                "The target cannot use any ability that costs mana. Rage, energy and free \
                 abilities still work."
            }
            AuraType::SpellSchoolLockout => {
                "One school of magic is locked out by an interrupt. Spells of other schools are \
                 unaffected."
            }
            AuraType::DamageOverTime => "Deals damage in periodic ticks until it expires.",
            AuraType::HealingOverTime => "Restores health in periodic ticks until it expires.",
            AuraType::HealingReduction => {
                "Reduces the healing the target receives — the counter to a healer out-sustaining \
                 your damage."
            }
            AuraType::DamageReduction => "Reduces the physical damage the target deals.",
            AuraType::CastTimeIncrease => "Makes the target's spells take longer to cast.",
            AuraType::AttackPowerReduction => {
                "Lowers the target's attack power, weakening its physical hits."
            }
            AuraType::AttackSpeedSlow => "Slows how often the target swings its weapon.",
            AuraType::Absorb => {
                "Soaks incoming damage until the shield is spent or expires. Absorbed damage never \
                 reaches health."
            }
            AuraType::DamageTakenReduction => "Reduces all damage the holder takes.",
            AuraType::DamageImmunity => {
                "Negates all incoming damage and blocks new harmful effects for its duration."
            }
            AuraType::FearImmunity => {
                "Breaks fear and blocks new fear effects. Horror effects bypass it."
            }
            AuraType::MaxHealthIncrease => "Raises the holder's maximum health.",
            AuraType::MaxManaIncrease => "Raises the holder's maximum mana.",
            AuraType::AttackPowerIncrease => "Raises the holder's attack power.",
            AuraType::SpellPowerIncrease => "Raises the holder's spell power.",
            AuraType::CritChanceIncrease => "Raises the holder's chance to land a critical strike.",
            AuraType::ManaRegenIncrease => "Restores extra mana every second.",
            AuraType::SpellResistanceBuff => {
                "Raises resistance to one school of magic, reducing damage from it."
            }
            AuraType::LockoutDurationReduction => {
                "Shortens the school lockout an interrupt inflicts on the holder."
            }
            AuraType::WindfuryBuff => {
                "Gives the holder's melee swings a chance at an extra attack. Inert for ranged \
                 attackers and casters."
            }
            AuraType::FrostArmorBuff => {
                "Chills melee attackers who strike the holder, slowing their movement and swings."
            }
            AuraType::WeaponPoison => {
                "Marks a weapon as coated. The coating's effect applies on hit, not from this mark."
            }
            AuraType::WeakenedSoul => {
                "The target's soul is spent — it cannot receive another Power Word: Shield until \
                 this fades."
            }
            AuraType::ShadowSight => {
                "Reveals stealthed enemies to the holder — and makes the holder visible to the \
                 enemy team in turn."
            }
        }
    }

    /// Returns true if this aura type is dispellable WHEN IT IS MAGIC.
    /// This covers CC effects that are always magical in WoW, plus Silence (which is
    /// removable by Dispel Magic / Cleanse).
    ///
    /// This is a question about the MECHANIC only, and it is asked second:
    /// [`Aura::can_be_dispelled`] rules out every non-magic removal class —
    /// poison, disease, curse, physical — before it gets here, so a `true` arm
    /// below means "dispellable if magic", not "always dispellable". Concussive
    /// Shot's snare is a `MovementSpeedSlow` and still undispellable, because it
    /// is a physical arrow rather than a frost spell.
    ///
    /// **Exhaustive on purpose — do not add a `_ =>` arm.** This is the same
    /// hazard class as [`AuraType::is_hostile_effect`] below, one degree worse:
    /// a hostility miss puts a sandbox preview over the wrong unit (cosmetic),
    /// whereas a variant silently missing from THIS list is an aura that no
    /// dispel and no cleanse can ever remove. That changes matchup outcomes and
    /// is invisible to every test in this repo — nothing observes "the debuff
    /// that was never a dispel candidate". The compiler refusing to build until
    /// variant N+1 is classified is the only guard that actually holds.
    ///
    /// The exclusions below are deliberate and stay exclusions — "correcting"
    /// one into the general rule changes what a dispel can strip, which is a
    /// balance change, not a cleanup.
    pub fn is_magic_dispellable(&self) -> bool {
        match self {
            // Crowd control, plus Silence (the Unstable Affliction dispel
            // backlash), which Dispel Magic / Cleanse lifts — provided the
            // aura is magic. A physical instance of any of these (Concussive
            // Shot's snare) is filtered out upstream by its removal class.
            AuraType::MovementSpeedSlow
            | AuraType::Root
            | AuraType::Fear
            | AuraType::Polymorph
            | AuraType::Incapacitate
            | AuraType::Silence => true,

            // Classified PER AURA, not per type: [`Aura::can_be_dispelled`]
            // admits a DoT only when its `spell_school` is non-physical, so
            // Corruption and Immolate are dispellable and Rend is not. Listing
            // the type here would make a SCHOOLLESS DoT dispellable — the
            // removal class already stops the physical ones.
            AuraType::DamageOverTime => false,

            // Stat debuffs, not crowd control — this classifier's true arm is
            // scoped to CC. Stuns are never dispellable in WoW at all; the
            // healing reduction (Mortal Strike, Aimed Shot) and the AP cut
            // (Demoralizing Shout) are physical. `AttackSpeedSlow` is the
            // frost-school half of Frost Armor's proc and is the one judgement
            // call here: its paired `MovementSpeedSlow` half IS dispellable, so
            // a dispel lifts half a proc. Widening this arm would change what
            // the dispel pool contains, so it stays a stat debuff until someone
            // measures the alternative.
            AuraType::Stun
            | AuraType::HealingReduction
            | AuraType::AttackPowerReduction
            | AuraType::AttackSpeedSlow => false,

            // Today these two mechanics arrive only from curses (Curse of
            // Weakness, Curse of Tongues), and the CURSE class already stops
            // them upstream — see `Aura::can_be_dispelled`. They stay false
            // here anyway: if a magic-school ability ever applies a damage cut
            // or a cast-time stretch, this is the arm that decides whether a
            // dispel takes it, and "no" is the status quo answer.
            AuraType::DamageReduction | AuraType::CastTimeIncrease => false,

            // Interrupt lockout. Not a dispellable debuff in WoW — an interrupt
            // that could be dispelled off would make interrupts worthless.
            AuraType::SpellSchoolLockout => false,

            // Mechanical markers, not effects. Lifting `WeakenedSoul` would hand
            // the Priest a free Power Word: Shield reset; the other two are
            // tracking state.
            AuraType::WeakenedSoul | AuraType::ShadowSight | AuraType::WeaponPoison => false,

            // Beneficial auras. Dispel Magic here is friendly-only (see
            // `try_dispel_ally` and the Felhunter's Devour Magic, both of which
            // scan ALLIES); stripping enemy buffs is the Shaman Purge's job and
            // goes through [`Aura::can_be_purged`] instead.
            AuraType::Absorb
            | AuraType::MaxHealthIncrease
            | AuraType::MaxManaIncrease
            | AuraType::AttackPowerIncrease
            | AuraType::SpellPowerIncrease
            | AuraType::HealingOverTime
            | AuraType::WindfuryBuff
            | AuraType::DamageTakenReduction
            | AuraType::DamageImmunity
            | AuraType::CritChanceIncrease
            | AuraType::ManaRegenIncrease
            | AuraType::LockoutDurationReduction
            | AuraType::FrostArmorBuff
            | AuraType::SpellResistanceBuff
            | AuraType::FearImmunity => false,
        }
    }

    /// Returns true if this aura TYPE is a HOSTILE effect — one that a full immunity
    /// (Divine Shield) both BLOCKS on application and CLEARS from its holder.
    ///
    /// Single source of truth for that classification, and for "is this aura
    /// something you put on an ENEMY" generally — the animation sandbox's target
    /// rule reads it too. It previously existed as three separate hand-maintained
    /// lists (the Divine Shield purge, the apply-time immunity gate in `auras.rs`,
    /// and `is_ccd`), which had already drifted apart: the purge list omitted
    /// `Incapacitate`, and the apply gate omitted `Silence`, `AttackPowerReduction`
    /// and `AttackSpeedSlow`. A FOURTH copy then grew in the sandbox and drifted the
    /// same way — it missed `DamageReduction`, so Curse of Weakness previewed on the
    /// caster instead of the dummy. That copy is gone; everything delegates here.
    ///
    /// **Exhaustive on purpose — do not add a `_ =>` arm.** This started life as
    /// an inline allowlist in `process_divine_shield`, and when `Incapacitate`
    /// was added to `AuraType` later nobody updated it, so Freezing Trap survived
    /// the bubble: a trapped Paladin popped Divine Shield, the log cheerfully
    /// reported "removes 3 debuffs", and the Paladin then stood still for the
    /// remaining 8 seconds while its partner died. A wildcard arm would silently
    /// reintroduce exactly that class of bug; the compiler refusing to build until
    /// a new variant is classified is the whole point.
    ///
    /// `WeakenedSoul` is deliberately NOT cleared despite being a debuff — it is
    /// the Power Word: Shield cooldown marker, and stripping it would let a Priest
    /// re-shield instantly. Mechanical markers are not CC. It reads correctly for
    /// the sandbox too: the Priest hangs it on the ALLY it just shielded, so a
    /// preview of it belongs on the caster, not the dummy.
    pub fn is_hostile_effect(self) -> bool {
        match self {
            // Hostile: crowd control, damage-over-time, and stat/casting debuffs.
            AuraType::MovementSpeedSlow
            | AuraType::Root
            | AuraType::Stun
            | AuraType::Fear
            | AuraType::Polymorph
            | AuraType::Incapacitate
            | AuraType::Silence
            | AuraType::SpellSchoolLockout
            | AuraType::DamageOverTime
            | AuraType::HealingReduction
            | AuraType::DamageReduction
            | AuraType::CastTimeIncrease
            | AuraType::AttackPowerReduction
            | AuraType::AttackSpeedSlow => true,

            // Beneficial — never stripped from their holder.
            AuraType::Absorb
            | AuraType::MaxHealthIncrease
            | AuraType::MaxManaIncrease
            | AuraType::AttackPowerIncrease
            | AuraType::SpellPowerIncrease
            | AuraType::HealingOverTime
            | AuraType::WindfuryBuff
            | AuraType::DamageTakenReduction
            | AuraType::DamageImmunity
            | AuraType::CritChanceIncrease
            | AuraType::ManaRegenIncrease
            | AuraType::LockoutDurationReduction
            | AuraType::FrostArmorBuff
            | AuraType::SpellResistanceBuff
            | AuraType::FearImmunity => false,

            // Mechanical markers, not effects: clearing these would grant a
            // cooldown reset (WeakenedSoul) or corrupt tracking state.
            AuraType::WeakenedSoul | AuraType::ShadowSight | AuraType::WeaponPoison => false,
        }
    }
}

// ============================================================================
// Aura Struct
// ============================================================================

/// An active aura/debuff effect on a combatant.
#[derive(Clone, Default)]
pub struct Aura {
    /// Type of aura effect
    pub effect_type: AuraType,
    /// Time remaining before the aura expires (in seconds)
    pub duration: f32,
    /// Magnitude of the effect (e.g., 0.7 = 30% slow)
    pub magnitude: f32,
    /// Damage threshold before the aura breaks (0.0 = never breaks on damage)
    pub break_on_damage_threshold: f32,
    /// Accumulated damage taken while this aura is active
    pub accumulated_damage: f32,
    /// For DoT effects: how often damage is applied (in seconds)
    pub tick_interval: f32,
    /// For DoT effects: time remaining until next tick
    pub time_until_next_tick: f32,
    /// For DoT effects: who applied this aura (for damage attribution)
    pub caster: Option<Entity>,
    /// Name of the ability that created this aura (for logging)
    pub ability_name: String,
    /// For Fear: current run direction (x, z normalized)
    pub fear_direction: (f32, f32),
    /// For Fear: time until direction change
    pub fear_direction_timer: f32,
    /// Spell school of the ability that created this aura, for DAMAGE purposes:
    /// the school a DoT tick lands as, and the school a `SpellResistanceBuff`
    /// matches against. `None` covers BOTH physical and schoolless, which is
    /// why it cannot answer a removability question — the two are the same here
    /// and must not be to a dispel (a physical arrow is immune, the schoolless
    /// Freezing Trap is not). Removability lives on `dispel_type`.
    pub spell_school: Option<SpellSchool>,
    /// True on the frame the aura was applied — prevents the applying ability's own damage
    /// from counting toward the break threshold
    pub applied_this_frame: bool,
    /// Snapshot of dispel-backlash damage, populated at cast time for abilities that carry
    /// a `DispelBacklashConfig` (currently only Unstable Affliction). If the aura is dispelled
    /// by an opposing-team combatant, this value is applied as direct damage to the dispeller.
    /// Stored on the aura (rather than recomputed at dispel time) so caster death or stat
    /// changes after application do not change the backlash amount. None for all non-UA auras.
    pub backlash_damage: Option<f32>,
    /// Per-aura diminishing-returns category override. When `Some`, this wins over the
    /// category derived from `effect_type` (see [`Aura::dr_category`]). Used so two abilities
    /// that apply the same `AuraType::Stun` can land in different DR buckets — Kidney Shot
    /// carries `Some(DRCategory::KidneyShotStun)` so it does not share DR with Cheap Shot or
    /// other stuns. `None` for every other aura (they fall back to `from_aura_type`).
    pub dr_category_override: Option<DRCategory>,
    /// Removal class — `Auto` (magic, unless the school is physical) by
    /// default, or `Poison`, `Disease`, `Curse`, `Physical`. Decouples
    /// removability from `effect_type` so the one `MovementSpeedSlow` mechanic
    /// can be Frostbolt's dispellable chill, Crippling Poison's cleansable
    /// coating, or Concussive Shot's physical snare that no dispel touches.
    /// Set by [`DispelType::for_ability`] for every RON-defined aura.
    pub dispel_type: DispelType,
}

impl Aura {
    /// Resolved diminishing-returns category for this aura: the per-aura
    /// `dr_category_override` if set, otherwise derived from `effect_type`.
    /// Returns `None` for non-CC auras. This is the single source of truth for
    /// an aura's DR bucket — all DR-application and CC-replacement logic must go
    /// through here so per-ability overrides (e.g. Kidney Shot's dedicated
    /// `KidneyShotStun` bucket) are honored consistently.
    pub fn dr_category(&self) -> Option<DRCategory> {
        self.dr_category_override
            .or_else(|| DRCategory::from_aura_type(&self.effect_type))
    }

    /// Returns true if this aura can be removed by a DISPEL — Dispel Magic,
    /// Cleanse, or the Felhunter's Devour Magic. All three take magic and only
    /// magic; Cleanse additionally takes poison, which it asks about through
    /// [`Aura::is_cleansable_poison`] rather than here.
    ///
    /// The REMOVAL CLASS decides first, and it can only ever say no: a poison,
    /// a disease, a curse and a physical debuff are not magic, so there is
    /// nothing for a dispel to take hold of. Only once an aura is magic does
    /// the mechanic get a vote — magic-dispellable types (slows, roots, fear,
    /// polymorph, silence) are dispellable, and a DoT is dispellable when it
    /// carries a magic school.
    ///
    /// The physical gate lives HERE, above the type check, rather than being
    /// spelled out per aura type. That ordering is the point: the rule was
    /// previously written down only inside the `DamageOverTime` arm, so a
    /// physical DoT (Rend) was correctly undispellable while a physical SLOW
    /// (Concussive Shot) was dispellable — same school, opposite answers,
    /// because the rule existed in only one of the two places. A rule about
    /// physical effects belongs on the physical axis, once. The curse gate is
    /// the same rule for the same reason: Curse of Agony is a Shadow DoT, so
    /// the school test below would have called it dispellable magic.
    pub fn can_be_dispelled(&self) -> bool {
        // **Exhaustive on purpose — do not add a `_ =>` arm.** A removal class
        // silently falling through to the magic branch is a debuff becoming
        // dispellable without anyone deciding it should be.
        match self.dispel_type {
            // Not magic. Poisons/diseases come off to a cleanse (see
            // `is_cleansable_poison`); curses come off to curse-removal, which
            // no ability in the arena performs YET — when one lands, this arm
            // is where `Curse` moves out of.
            DispelType::Poison
            | DispelType::Disease
            | DispelType::Curse
            | DispelType::Physical => return false,
            DispelType::Auto => {}
        }

        // Inherently magic-dispellable aura types
        if self.effect_type.is_magic_dispellable() {
            return true;
        }

        // DoTs are dispellable only if magic school. Physical DoTs never reach
        // this arm — they are `DispelType::Physical` and returned above — but
        // the school check stays as the backstop for a DoT built by hand in
        // engine code, which never passes through `DispelType::for_ability`.
        if matches!(self.effect_type, AuraType::DamageOverTime) {
            if let Some(school) = self.spell_school {
                return school != SpellSchool::Physical;
            }
        }

        false
    }

    /// Returns true if this aura is PHYSICAL — immune to every dispel, cleanse
    /// and purge, and removable only by an effect that clears physical harm
    /// specifically.
    ///
    /// Exists so player-facing surfaces can say WHY an aura resists removal
    /// without re-deriving it from the school. The encyclopedia's removal badge
    /// reads this.
    pub fn is_physical(&self) -> bool {
        matches!(self.dispel_type, DispelType::Physical)
    }

    /// Returns true if this aura is a CURSE — removable in principle by
    /// curse-removal, which no ability in the arena performs yet. Distinct from
    /// "cannot be removed": the class has no server, not no answer.
    pub fn is_curse(&self) -> bool {
        matches!(self.dispel_type, DispelType::Curse)
    }

    /// Returns true if this aura is a poison/disease debuff removable by a
    /// cleanse. Mutually exclusive with `can_be_dispelled` (magic).
    pub fn is_cleansable_poison(&self) -> bool {
        matches!(self.dispel_type, DispelType::Poison | DispelType::Disease)
    }

    /// This aura's removal class as a player-facing word — `Magic`, `Poison`,
    /// `Disease`, `Curse`, `Physical` — or `None` when the data does not
    /// determine one. The encyclopedia stat block prints it on a line of its
    /// OWN, separate from the spell school, because the two are genuinely
    /// different facts: Shadow-school Corruption is Magic and Shadow-school
    /// Curse of Agony is a Curse, and only the first comes off to a dispel.
    ///
    /// `None` is the honest answer for a SCHOOLLESS aura that declares no class.
    /// `SpellSchool::None` means "no school, cannot be locked out"; it does NOT
    /// mean magic, and it does not mean physical either — Demoralizing Shout and
    /// the interrupt lockouts are conceptually physical while Freezing Trap is
    /// sprung as Frost. Nothing in the data distinguishes them, so the page says
    /// nothing rather than asserting a class it cannot know. (Inert for
    /// removability either way: every schoolless `Auto` aura today carries a
    /// mechanic no dispel takes.)
    ///
    /// **Exhaustive on purpose — do not add a `_ =>` arm.** A class with no
    /// word is a page that silently mislabels a debuff.
    pub fn removal_class_name(&self) -> Option<&'static str> {
        match self.dispel_type {
            DispelType::Poison => Some("Poison"),
            DispelType::Disease => Some("Disease"),
            DispelType::Curse => Some("Curse"),
            DispelType::Physical => Some("Physical"),
            // `Auto` means "derived from the school", and the only school that
            // derives to anything but magic is Physical — which
            // `DispelType::for_ability` has already turned into the arm above.
            // An `Auto` aura built by hand in engine code never passes through
            // that constructor, so the school check stays as the backstop; it
            // is the same backstop `can_be_dispelled` keeps for hand-built DoTs.
            DispelType::Auto => match self.spell_school {
                Some(SpellSchool::Physical) => Some("Physical"),
                Some(SpellSchool::None) | None => None,
                Some(_) => Some("Magic"),
            },
        }
    }

    /// Returns true if this aura is a HOSTILE effect — see
    /// [`AuraType::is_hostile_effect`], which owns the (exhaustive)
    /// classification. Hostility is a property of the aura TYPE alone.
    pub fn is_hostile_effect(&self) -> bool {
        self.effect_type.is_hostile_effect()
    }

    /// Returns true if this aura is a BENEFICIAL (buff) effect that an enemy
    /// offensive dispel (Shaman's Purge) can strip.
    ///
    /// Only beneficial auras qualify — Purge removes enemy buffs, never their
    /// debuffs/CC (those are the *target's* problem, not ours).
    ///
    /// **Exhaustive on purpose — do not add a `_ =>` arm.** Same reasoning as
    /// [`AuraType::is_magic_dispellable`]: a buff silently missing from this
    /// list is a buff the Shaman can never strip, which is a matchup swing no
    /// test in this repo observes. A wildcard arm would let variant N+1 default
    /// into "unpurgeable" without anyone deciding that.
    ///
    /// Purgeable is BENEFICIAL-minus-exceptions, and the two exceptions are
    /// deliberate self buffs that must survive this list rather than be folded
    /// into the general rule:
    /// - `DamageImmunity` — Divine Shield is unpurgeable by design.
    /// - `FearImmunity` — Berserker Rage is a physical enrage, not magic, so
    ///   there is nothing for a magic purge to take.
    ///
    /// `ShadowSight` and `WeaponPoison` are mechanical markers rather than real
    /// buffs, and the whole debuff/CC half belongs to the target.
    ///
    /// The purgeable set is disjoint from [`AuraType::is_hostile_effect`] by
    /// construction, and `an_entry_previews_on_the_caster_only_for_a_beneficial_aura`
    /// / `an_entry_previews_on_the_dummy_only_for_a_non_beneficial_aura` in the
    /// animation sandbox pin both directions of that.
    pub fn can_be_purged(&self) -> bool {
        match self.effect_type {
            // Beneficial buffs: defensives, throughput, and minor utility.
            AuraType::Absorb
            | AuraType::MaxHealthIncrease
            | AuraType::MaxManaIncrease
            | AuraType::AttackPowerIncrease
            | AuraType::SpellPowerIncrease
            | AuraType::HealingOverTime
            | AuraType::WindfuryBuff
            | AuraType::DamageTakenReduction
            | AuraType::CritChanceIncrease
            | AuraType::ManaRegenIncrease
            | AuraType::LockoutDurationReduction
            | AuraType::FrostArmorBuff
            | AuraType::SpellResistanceBuff => true,

            // Beneficial, but DELIBERATELY unpurgeable — see the doc above.
            AuraType::DamageImmunity | AuraType::FearImmunity => false,

            // Mechanical markers, not buffs.
            AuraType::ShadowSight | AuraType::WeaponPoison | AuraType::WeakenedSoul => false,

            // Debuffs and crowd control — the target's problem, not ours.
            AuraType::MovementSpeedSlow
            | AuraType::Root
            | AuraType::Stun
            | AuraType::Fear
            | AuraType::Polymorph
            | AuraType::Incapacitate
            | AuraType::Silence
            | AuraType::SpellSchoolLockout
            | AuraType::DamageOverTime
            | AuraType::HealingReduction
            | AuraType::DamageReduction
            | AuraType::CastTimeIncrease
            | AuraType::AttackPowerReduction
            | AuraType::AttackSpeedSlow => false,
        }
    }
}

// ============================================================================
// ActiveAuras Component
// ============================================================================

/// Component tracking active auras/debuffs on a combatant.
#[derive(Component, Default)]
pub struct ActiveAuras {
    pub auras: Vec<Aura>,
}

// ============================================================================
// AuraPending Component
// ============================================================================

/// Temporary component for pending auras to be applied.
/// Used to avoid borrow checker issues when applying auras during casting.
#[derive(Component)]
pub struct AuraPending {
    pub target: Entity,
    pub aura: Aura,
}

impl AuraPending {
    /// Create an AuraPending from an ability config.
    ///
    /// This is a helper method that extracts the aura info from an AbilityConfig
    /// and creates an AuraPending with appropriate defaults.
    ///
    /// Returns None if the ability doesn't apply an aura.
    pub fn from_ability(
        target: Entity,
        caster: Entity,
        ability_def: &AbilityConfig,
    ) -> Option<Self> {
        Self::from_ability_scaled(target, caster, ability_def, 0.0)
    }

    /// Like [`Self::from_ability`], but scales the aura magnitude with the
    /// caster's spell power: `magnitude + spell_power × magnitude_coefficient`
    /// (Power Word: Shield absorb). Pass the caster's EFFECTIVE spell power
    /// (base + gear + aura bonuses). `ability_config::validate()` rejects a
    /// non-zero coefficient on any ability whose apply site doesn't call this
    /// variant, so plain `from_ability` callers can't silently drop scaling.
    pub fn from_ability_scaled(
        target: Entity,
        caster: Entity,
        ability_def: &AbilityConfig,
        spell_power: f32,
    ) -> Option<Self> {
        let aura_effect = ability_def.applies_aura.as_ref()?;

        // Convert spell school to Option. Physical and schoolless both store
        // `None` here — this field feeds DAMAGE (resistance matching, DoT tick
        // school), not removability. Removability is `dispel_type` below, which
        // keeps Physical distinct from schoolless.
        let spell_school = match ability_def.spell_school {
            SpellSchool::Physical | SpellSchool::None => None,
            school => Some(school),
        };
        let dispel_type =
            DispelType::for_ability(aura_effect.dispel_type, ability_def.spell_school);

        Some(Self {
            target,
            aura: Aura {
                effect_type: aura_effect.aura_type,
                duration: aura_effect.duration,
                magnitude: aura_effect.magnitude
                    + spell_power * aura_effect.magnitude_coefficient,
                break_on_damage_threshold: aura_effect.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: aura_effect.tick_interval,
                time_until_next_tick: aura_effect.tick_interval,
                caster: Some(caster),
                ability_name: ability_def.name.clone(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school,
                applied_this_frame: false,
                backlash_damage: None,
                dr_category_override: aura_effect.dr_category,
                dispel_type,
            },
        })
    }

    /// Create an AuraPending for a DoT (Damage over Time) effect.
    ///
    /// DoTs have tick intervals and need special handling for damage attribution.
    pub fn from_ability_dot(
        target: Entity,
        caster: Entity,
        ability_def: &AbilityConfig,
        tick_interval: f32,
    ) -> Option<Self> {
        let aura_effect = ability_def.applies_aura.as_ref()?;

        // Convert spell school to Option. Physical and schoolless both store
        // `None` here — this field feeds DAMAGE (resistance matching, DoT tick
        // school), not removability. Removability is `dispel_type` below, which
        // keeps Physical distinct from schoolless.
        let spell_school = match ability_def.spell_school {
            SpellSchool::Physical | SpellSchool::None => None,
            school => Some(school),
        };
        let dispel_type =
            DispelType::for_ability(aura_effect.dispel_type, ability_def.spell_school);

        Some(Self {
            target,
            aura: Aura {
                effect_type: aura_effect.aura_type,
                duration: aura_effect.duration,
                magnitude: aura_effect.magnitude,
                break_on_damage_threshold: aura_effect.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval,
                time_until_next_tick: tick_interval, // First tick after interval
                caster: Some(caster),
                ability_name: ability_def.name.clone(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school,
                applied_this_frame: false,
                backlash_damage: None,
                dr_category_override: aura_effect.dr_category,
                dispel_type,
            },
        })
    }

    /// Create an AuraPending with a custom ability name override.
    ///
    /// Useful when the display name should differ from the ability definition.
    pub fn from_ability_with_name(
        target: Entity,
        caster: Entity,
        ability_def: &AbilityConfig,
        ability_name: String,
    ) -> Option<Self> {
        let aura_effect = ability_def.applies_aura.as_ref()?;

        // Convert spell school to Option. Physical and schoolless both store
        // `None` here — this field feeds DAMAGE (resistance matching, DoT tick
        // school), not removability. Removability is `dispel_type` below, which
        // keeps Physical distinct from schoolless.
        let spell_school = match ability_def.spell_school {
            SpellSchool::Physical | SpellSchool::None => None,
            school => Some(school),
        };
        let dispel_type =
            DispelType::for_ability(aura_effect.dispel_type, ability_def.spell_school);

        Some(Self {
            target,
            aura: Aura {
                effect_type: aura_effect.aura_type,
                duration: aura_effect.duration,
                magnitude: aura_effect.magnitude,
                break_on_damage_threshold: aura_effect.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: aura_effect.tick_interval,
                time_until_next_tick: aura_effect.tick_interval,
                caster: Some(caster),
                ability_name,
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school,
                applied_this_frame: false,
                backlash_damage: None,
                dr_category_override: aura_effect.dr_category,
                dispel_type,
            },
        })
    }
}

// ============================================================================
// Diminishing Returns
// ============================================================================

/// DR categories — fixed enum with known size for array indexing.
/// Each category is independent: Stun DR doesn't affect Fear DR.
/// `Deserialize` exists for trace-payload roundtripping
/// (`decision_trace::EventPayload` derives `Deserialize` and
/// `RejectionReason::DRImmune` carries this enum), not for config loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DRCategory {
    Stuns = 0,
    Fears = 1,
    Incapacitates = 2,
    Roots = 3,
    Slows = 4,
    Silence = 5,
    /// Kidney Shot's dedicated stun DR bucket. Unique in this game — nothing
    /// else shares it — so a Cheap Shot opener into an immediate Kidney Shot
    /// lands two undiminished stuns back-to-back. Set only via an aura's
    /// `dr_category_override`, never returned by `from_aura_type`.
    KidneyShotStun = 6,
    /// Horror's dedicated DR bucket (Warlock Death Coil). In WoW Classic the
    /// fear-flee "horror" effect diminishes separately from Fear, so a Fear and
    /// a Death Coil on one target do not share DR. Death Coil reuses
    /// `AuraType::Fear` for the flee locomotion + dispel classification but
    /// carries `dr_category_override: Some(Horror)` to land here. Set only via
    /// the override, never returned by `from_aura_type`.
    Horror = 7,
}

impl DRCategory {
    pub const COUNT: usize = 8;

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    /// Map an AuraType to its DR category. Returns None for non-CC auras.
    pub fn from_aura_type(aura_type: &AuraType) -> Option<DRCategory> {
        match aura_type {
            AuraType::Stun => Some(DRCategory::Stuns),
            AuraType::Fear => Some(DRCategory::Fears),
            AuraType::Polymorph | AuraType::Incapacitate => Some(DRCategory::Incapacitates),
            AuraType::Root => Some(DRCategory::Roots),
            AuraType::MovementSpeedSlow => Some(DRCategory::Slows),
            AuraType::Silence => Some(DRCategory::Silence),
            _ => None,
        }
    }
}

/// Per-category DR state. Tracks diminishment level and reset timer.
#[derive(Debug, Clone, Copy, Default)]
pub struct DRState {
    /// 0 = fresh, 1 = next will be 50%, 2 = next will be 25%, 3 = immune
    level: u8,
    /// Seconds remaining until DR resets (counts down from 15.0)
    timer: f32,
}

/// Fixed-size DR tracker component. No heap allocation, fully inline in archetype table.
/// Uses [DRState; 5] indexed by DRCategory discriminant — O(1) access.
#[derive(Component, Debug, Clone)]
pub struct DRTracker {
    states: [DRState; DRCategory::COUNT],
}

impl Default for DRTracker {
    fn default() -> Self {
        Self {
            states: [DRState::default(); DRCategory::COUNT],
        }
    }
}

impl DRTracker {
    /// Apply a CC of the given category. Returns the duration multiplier (1.0, 0.5, 0.25, or 0.0).
    /// Advances DR level and resets the 15s timer (unless already immune).
    #[inline]
    pub fn apply(&mut self, category: DRCategory) -> f32 {
        let state = &mut self.states[category.index()];
        let multiplier = DR_MULTIPLIERS[state.level.min(3) as usize];
        if state.level < DR_IMMUNE_LEVEL {
            state.level += 1;
            state.timer = DR_RESET_TIMER;
        }
        // Immune applications do NOT restart the timer (decision #2)
        multiplier
    }

    /// Check if target is immune to a DR category (level >= 3).
    #[inline]
    pub fn is_immune(&self, category: DRCategory) -> bool {
        self.states[category.index()].level >= DR_IMMUNE_LEVEL
    }

    /// Clear every category back to fresh (level 0, no timer). Used by the
    /// animation sandbox's sustain system so looping a CC entry replays at
    /// full duration instead of escalating to immunity — never called from
    /// match code.
    pub fn reset(&mut self) {
        self.states = [DRState::default(); DRCategory::COUNT];
    }

    /// Tick all DR timers. Called from update_auras() each frame.
    pub fn tick_timers(&mut self, dt: f32) {
        for state in &mut self.states {
            if state.timer > 0.0 {
                state.timer -= dt;
                if state.timer <= 0.0 {
                    state.level = 0;
                    state.timer = 0.0;
                }
            }
        }
    }

    /// Get current DR level for a category (for combat log / AI queries).
    #[inline]
    pub fn level(&self, category: DRCategory) -> u8 {
        self.states[category.index()].level
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal aura carrying only `effect_type` — `can_be_purged`
    /// inspects nothing else.
    fn aura(effect_type: AuraType) -> Aura {
        Aura {
            effect_type,
            ..Default::default()
        }
    }

    /// Beneficial auras the Shaman's Purge strips: defensives, throughput
    /// buffs, and minor utility buffs. Mirrors the `can_be_purged` match.
    #[test]
    fn can_be_purged_true_for_beneficial_buffs() {
        for ty in [
            AuraType::Absorb,
            AuraType::MaxHealthIncrease,
            AuraType::MaxManaIncrease,
            AuraType::DamageTakenReduction,
            AuraType::AttackPowerIncrease,
            AuraType::SpellPowerIncrease,
            AuraType::HealingOverTime,
            AuraType::WindfuryBuff,
            AuraType::CritChanceIncrease,
            AuraType::ManaRegenIncrease,
            AuraType::LockoutDurationReduction,
            AuraType::FrostArmorBuff,
            AuraType::SpellResistanceBuff,
        ] {
            assert!(
                aura(ty).can_be_purged(),
                "{:?} is a beneficial buff and must be purgeable",
                ty
            );
            // The purgeable set and the hostile set are disjoint by
            // construction. Nothing enforces that across two separate matches,
            // so pin it: a buff that starts reading as hostile would be cleared
            // off its own holder by Divine Shield.
            assert!(
                !aura(ty).is_hostile_effect(),
                "{ty:?} is purgeable AND hostile — the two classifications must \
                 stay disjoint"
            );
        }
    }

    /// Regression: Divine Shield must clear EVERY crowd-control type, including
    /// `Incapacitate`. Freezing Trap applies `Incapacitate`, and the old inline
    /// allowlist in `process_divine_shield` omitted it, so a trapped Paladin
    /// bubbled, was told "removes 3 debuffs", and then stood still for 8 seconds
    /// while its partner died.
    #[test]
    fn immunity_clears_every_crowd_control_type() {
        for ty in [
            AuraType::Stun,
            AuraType::Root,
            AuraType::Fear,
            AuraType::Polymorph,
            AuraType::Incapacitate,
            AuraType::Silence,
            AuraType::SpellSchoolLockout,
        ] {
            assert!(
                aura(ty).is_hostile_effect(),
                "{ty:?} is crowd control and MUST be cleared by Divine Shield"
            );
        }
    }

    /// Immunity must not strip the holder's own buffs — including the
    /// `DamageImmunity` aura that represents the shield itself.
    #[test]
    fn immunity_never_clears_buffs_or_markers() {
        for ty in [
            AuraType::DamageImmunity,
            AuraType::Absorb,
            AuraType::AttackPowerIncrease,
            AuraType::SpellPowerIncrease,
            AuraType::HealingOverTime,
            AuraType::FrostArmorBuff,
            AuraType::FearImmunity,
            // Mechanical markers: clearing WeakenedSoul would hand the Priest a
            // free Power Word: Shield reset.
            AuraType::WeakenedSoul,
            AuraType::ShadowSight,
            AuraType::WeaponPoison,
        ] {
            assert!(
                !aura(ty).is_hostile_effect(),
                "{ty:?} must NOT be cleared by Divine Shield"
            );
        }
    }

    /// Every CC type `is_ccd` recognises must be clearable by immunity, or a
    /// Paladin can bubble and remain unable to act — which is the failure the
    /// Freezing Trap bug produced. Pins the two lists together.
    #[test]
    fn every_cc_recognised_by_is_ccd_is_hostile_effect() {
        for ty in [
            AuraType::Stun,
            AuraType::Fear,
            AuraType::Root,
            AuraType::Polymorph,
            AuraType::Incapacitate,
        ] {
            assert!(
                aura(ty).is_hostile_effect(),
                "{ty:?} counts as CC for is_ccd but survives immunity — a bubbled \
                 Paladin would stay locked out"
            );
        }
    }

    /// Purge must NOT strip: damage immunity (Divine Shield — unpurgeable by
    /// design), the ShadowSight / WeaponPoison mechanical markers, or any
    /// debuff / CC effect (those are the target's problem, not ours).
    #[test]
    fn can_be_purged_false_for_immunity_markers_and_debuffs() {
        for ty in [
            AuraType::DamageImmunity,
            AuraType::ShadowSight,
            AuraType::WeaponPoison,
            AuraType::Stun,
            AuraType::Root,
            AuraType::Fear,
            AuraType::Polymorph,
            AuraType::DamageOverTime,
            AuraType::MovementSpeedSlow,
            AuraType::Silence,
            AuraType::WeakenedSoul,
        ] {
            assert!(
                !aura(ty).can_be_purged(),
                "{:?} must NOT be purgeable",
                ty
            );
        }
    }

    // ========================================================================
    // Physical debuffs are immune to ORDINARY removal
    // ========================================================================

    /// The removal class derived for an ability's aura. A PHYSICAL ability
    /// yields a physical debuff; a schoolless one does NOT — `SpellSchool::None`
    /// means "no school, cannot be locked out", and the schoolless Freezing
    /// Trap is sprung by the engine as a Frost aura that stays dispellable.
    #[test]
    fn for_ability_derives_physical_from_the_school_only() {
        assert_eq!(
            DispelType::for_ability(DispelType::Auto, SpellSchool::Physical),
            DispelType::Physical
        );
        for school in [
            SpellSchool::None,
            SpellSchool::Frost,
            SpellSchool::Holy,
            SpellSchool::Shadow,
            SpellSchool::Arcane,
            SpellSchool::Fire,
            SpellSchool::Nature,
        ] {
            assert_eq!(
                DispelType::for_ability(DispelType::Auto, school),
                DispelType::Auto,
                "{:?} is not physical",
                school
            );
        }
    }

    /// An explicitly declared class wins over the school, so a Nature-school
    /// poison (Crippling Poison) stays a poison rather than becoming magic,
    /// and a Shadow-school curse stays a curse rather than becoming one.
    #[test]
    fn for_ability_keeps_an_explicitly_declared_class() {
        assert_eq!(
            DispelType::for_ability(DispelType::Poison, SpellSchool::Nature),
            DispelType::Poison
        );
        assert_eq!(
            DispelType::for_ability(DispelType::Disease, SpellSchool::Physical),
            DispelType::Disease
        );
        assert_eq!(
            DispelType::for_ability(DispelType::Curse, SpellSchool::Shadow),
            DispelType::Curse
        );
    }

    /// No school derives to `Curse`. Curses are Shadow and so is Corruption, so
    /// the day someone "simplifies" the curse declaration away by reading the
    /// school, this fails.
    #[test]
    fn no_school_derives_a_curse() {
        for school in [
            SpellSchool::None,
            SpellSchool::Physical,
            SpellSchool::Frost,
            SpellSchool::Holy,
            SpellSchool::Shadow,
            SpellSchool::Arcane,
            SpellSchool::Fire,
            SpellSchool::Nature,
        ] {
            assert_ne!(
                DispelType::for_ability(DispelType::Auto, school),
                DispelType::Curse,
                "{school:?} must not derive a curse — curses are declared, not inferred"
            );
        }
    }

    /// A curse is removable by NOTHING today — not a dispel, not a cleanse, not
    /// a purge — while still being a hostile effect (so an effect that clears
    /// harmful effects outright, like Divine Shield, still takes it). Written
    /// per MECHANIC because the three curses use three different ones, and
    /// Curse of Agony's is `DamageOverTime`, which the school test below would
    /// otherwise wave through as dispellable Shadow magic.
    #[test]
    fn a_curse_is_removable_by_nothing_today() {
        for ty in [
            AuraType::DamageOverTime,
            AuraType::DamageReduction,
            AuraType::CastTimeIncrease,
        ] {
            let curse = Aura {
                effect_type: ty,
                spell_school: Some(SpellSchool::Shadow),
                dispel_type: DispelType::Curse,
                ..Default::default()
            };
            assert!(!curse.can_be_dispelled(), "{ty:?} as a curse is not magic");
            assert!(!curse.is_cleansable_poison(), "{ty:?} as a curse is not a poison");
            assert!(!curse.can_be_purged(), "{ty:?} as a curse is a debuff, not a buff");
            assert!(curse.is_curse());
            assert!(!curse.is_physical(), "a curse is not physical");
            assert_eq!(curse.removal_class_name(), Some("Curse"));
        }

        // The Shadow DoT that is NOT a curse still comes off to a dispel —
        // Corruption. Same school, same mechanic, different class.
        let corruption = Aura {
            effect_type: AuraType::DamageOverTime,
            spell_school: Some(SpellSchool::Shadow),
            dispel_type: DispelType::Auto,
            ..Default::default()
        };
        assert!(corruption.can_be_dispelled(), "an ordinary Shadow DoT is dispellable magic");
        assert_eq!(corruption.removal_class_name(), Some("Magic"));
    }

    /// A schoolless `Auto` aura gets NO removal class, because the data does not
    /// determine one: `SpellSchool::None` is doing double duty ("cannot be
    /// locked out" and "the author did not say"), and Demoralizing Shout is
    /// conceptually physical while Freezing Trap is sprung as Frost. The page
    /// must not invent an answer for them.
    #[test]
    fn a_schoolless_undeclared_aura_has_no_removal_class() {
        let shout = Aura {
            effect_type: AuraType::AttackPowerReduction,
            spell_school: None,
            dispel_type: DispelType::Auto,
            ..Default::default()
        };
        assert_eq!(shout.removal_class_name(), None);
        assert!(!shout.can_be_dispelled(), "inert either way — no dispel takes an AP cut");
    }

    /// The rule, stated on the mechanic-by-mechanic grid it used to be wrong
    /// on: the SAME mechanic is dispellable as magic and immune as physical.
    /// A frost slow (Frostbolt) comes off; an arrow slow (Concussive Shot)
    /// does not.
    #[test]
    fn a_physical_debuff_is_never_dispellable_whatever_its_mechanic() {
        for ty in [
            AuraType::MovementSpeedSlow,
            AuraType::Root,
            AuraType::Fear,
            AuraType::Polymorph,
            AuraType::Incapacitate,
            AuraType::Silence,
            AuraType::DamageOverTime,
        ] {
            let magic = Aura {
                effect_type: ty,
                spell_school: Some(SpellSchool::Frost),
                dispel_type: DispelType::Auto,
                ..Default::default()
            };
            let physical = Aura {
                effect_type: ty,
                spell_school: None,
                dispel_type: DispelType::Physical,
                ..Default::default()
            };
            assert!(
                magic.can_be_dispelled(),
                "{:?} as magic must stay dispellable",
                ty
            );
            assert!(
                !physical.can_be_dispelled(),
                "{:?} as a PHYSICAL effect must not be dispellable",
                ty
            );
            assert!(physical.is_physical());
        }
    }

    /// Physical is immune to ORDINARY removal, not permanent: every removal
    /// predicate says no, while Divine Shield's `is_hostile_effect` retain
    /// still takes it. Losing this is how "physical" quietly becomes
    /// "unremovable".
    #[test]
    fn a_physical_debuff_is_still_cleared_by_divine_shield() {
        let concussive = Aura {
            effect_type: AuraType::MovementSpeedSlow,
            dispel_type: DispelType::Physical,
            ..Default::default()
        };
        assert!(!concussive.can_be_dispelled());
        assert!(!concussive.is_cleansable_poison());
        assert!(!concussive.can_be_purged());
        assert!(
            concussive.is_hostile_effect(),
            "Divine Shield retains against hostile effects — a physical debuff \
             must remain one, or the bubble stops clearing it"
        );
    }
}
