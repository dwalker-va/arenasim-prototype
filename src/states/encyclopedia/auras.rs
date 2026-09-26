//! The Buffs & Debuffs section — the catalog of NAMED AURAS.
//!
//! ## What an entry is
//!
//! An entry is a named aura, not an [`AuraType`]. Players see *Rend*,
//! *Corruption* and *Serpent Sting* on the actor frames as three distinct
//! debuffs, and the engine agrees with them: every applied [`Aura`] carries an
//! `ability_name` and the HUD icons key off it. A catalog with one "Damage over
//! Time" entry would contradict what the player is looking at.
//!
//! `AuraType` is therefore each entry's MECHANIC — rendered as a badge, and the
//! basis for the "other Damage over Time effects" cross-links that tie Rend,
//! Corruption and Serpent Sting together.
//!
//! ## Where entries come from
//!
//! Two sources, both derived — no hand-authored prose, no per-entry code:
//!
//! 1. **`abilities.ron`** — one entry per ability carrying an `applies_aura`
//!    block. Name and icon are the ability's; duration, magnitude, tick and
//!    break-on-damage come from the aura block. Entry N+1 appears the moment
//!    the RON gains an `applies_aura`, which is the encyclopedia's
//!    zero-marginal-cost rule.
//! 2. **[`EngineAura`]** — the short, explicit registry of auras the engine
//!    applies from code with a hardcoded name, which the RON walk cannot see:
//!    Weakened Soul, Shadow Sight, the Frost Trap zone's slow, the totem
//!    pulses, the school lockout every interrupt leaves behind, the Rogue's
//!    weapon-coating marker, Unstable Affliction's dispel-backlash silence and
//!    the Frost Armor chill. The nested variants EXPAND from their own
//!    sources (`TotemElement::ALL`, `RoguePoison::ALL`, the interrupt flags),
//!    so they stay zero-marginal-cost too.
//!
//!    `tests/aura_catalog_audit.rs` is what stops that registry rotting: it
//!    scans every `Aura { .. }` literal under `src/` and fails unless its
//!    `(ability_name, effect_type)` pair resolves to a catalog entry. The key
//!    is a PAIR because the engine reuses three names across six distinct
//!    auras — see [`EngineAura`]'s collision note.
//!
//! ## Where the per-entry facts come from
//!
//! Every classification on a page is answered by asking the ENGINE about a
//! representative [`Aura`] built the same way the simulation builds it
//! (`AuraPending::from_ability` for RON entries, and the apply site's own
//! shared constructor for most engine ones). So `can_be_dispelled`,
//! `is_cleansable_poison`, `can_be_purged` and `dr_category` are the real
//! predicates, not a second copy of their rules — including the per-aura ones a
//! type-level answer would get wrong (Rend is a PHYSICAL damage-over-time and
//! is not dispellable; Corruption is Shadow and is).
//!
//! The same rule holds for the NUMBERS: every engine entry's break-on-damage
//! threshold comes from the constant its apply site reads, so no page can print
//! "Breaks on damage: Never" over an aura the first hit removes. Shadow Sight
//! was exactly that case for a while (a `0.0` typo where its comment claimed
//! "never"); it reads `SHADOW_SIGHT_BREAK_ON_DAMAGE` here, so the page follows
//! the value the orb pickup applies rather than a restatement of it.

use bevy_egui::egui;

use crate::states::ability_text::build_aura_description;
use crate::states::match_config::RoguePoison;
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::frost_armor_chill_auras;
use crate::states::play_match::components::{
    weapon_poison_marker_aura, Aura, AuraPending, AuraType, DRCategory, DispelType, TotemElement,
    WEAPON_POISON_MARKER_DURATION,
};
use crate::states::play_match::constants::{
    DR_MULTIPLIERS, DR_RESET_TIMER, FROST_TRAP_SLOW_MAGNITUDE, FROST_TRAP_ZONE_DURATION,
    TOTEM_DURATION, WEAKENED_SOUL_DURATION,
};
use crate::states::play_match::effects::backlash::{
    dispel_backlash_silence_aura, DISPEL_BACKLASH_SILENCE_DURATION,
};
use crate::states::play_match::equipment::{ItemDefinitions, ItemId};
use crate::states::play_match::proc_trinkets::proc_description;
use crate::states::play_match::rendering::{is_buff_aura, item_aura_icon_key};
use crate::states::play_match::shadow_sight::{
    SHADOW_SIGHT_BREAK_ON_DAMAGE, SHADOW_SIGHT_DURATION, SHADOW_SIGHT_SPAWN_TIME,
};

use super::search::SearchEntry;
use super::widget;
use super::{EncyclopediaData, Topic, DIM, MUTED, TEXT};

/// Buff green and debuff red, from the blessed mockup's palette.
pub(crate) const BUFF: egui::Color32 = egui::Color32::from_rgb(111, 174, 126);
pub(crate) const DEBUFF: egui::Color32 = egui::Color32::from_rgb(208, 106, 91);

/// Cap on an index column's width. Rows are name + mechanic tag, so a column
/// stretched across half a wide screen leaves a lake of dead space between the
/// two, and the eye loses the pairing.
const COLUMN_MAX_WIDTH: f32 = 430.0;

// ============================================================================
// ADDRESS
// ============================================================================

/// The address of one named aura — the payload of [`Topic::Aura`].
///
/// This is what makes the catalog's entries NAMED auras rather than aura types.
/// It is `Copy` and cheap to compare, so it slots into `Topic` (itself `Copy`)
/// without the navigation stack, search registry or linked-icon widget having
/// to learn anything new.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuraId {
    /// The aura an ability's `applies_aura` block defines. The ability supplies
    /// the entry's name, icon and every number on its page.
    ///
    /// This address structurally assumes AT MOST ONE aura per ability, which
    /// holds while `AbilityConfig::applies_aura` is an `Option`. If it ever
    /// becomes a list, every ability-addressed aura turns ambiguous and this
    /// variant needs an index (or the aura's own key) alongside the ability.
    Ability(AbilityType),
    /// An aura the engine applies from code with a hardcoded name.
    Engine(EngineAura),
}

/// Auras applied in code rather than through an `applies_aura` block.
///
/// Deliberately explicit and deliberately SHORT. Adding to it is a decision, so
/// the drift risk is the opposite one — an engine aura that exists but is not
/// listed — and `tests/aura_catalog_audit.rs` is the guard against exactly that.
///
/// Some of these share a NAME with a real `abilities.ron` entry that carries no
/// `applies_aura` of its own (Frost Trap places a zone; the totems pulse their
/// buff). Those borrow the ability's icon and link back to it as the applying
/// ability. Weakened Soul links back to Power Word: Shield but keeps art of its
/// own — see [`AuraArt`] — and only Shadow Sight has no applying ability at
/// all, which [`AuraSource::Mechanic`] makes it say out loud.
///
/// ## Name collisions
///
/// Three of these carry a `frame_name` that is NOT their catalog name, because
/// the engine hangs two distinct DEBUFFS under one name and a player reading
/// their frames cannot tell which is which:
///
/// - "Crippling Poison" is both the Rogue's own coating marker and the slow it
///   puts on the target.
/// - "Unstable Affliction" is both an 18-second Shadow damage-over-time and the
///   silence its dispel backlash inflicts.
/// - "Frost Armor" is the Mage's self-buff AND the chill it hangs on melee
///   attackers.
///
/// Each gets its own entry with a parenthetical qualifier, and its page says
/// what the frames call it. The alternative — one entry per NAME — sends a
/// player from their own gold-bordered buff to the enemy debuff's page.
///
/// **A collision is two debuffs sharing a name, never two EFFECTS of one
/// debuff.** Frost Armor's chill used to be listed twice, once per effect, and
/// the two rows wore different removal badges over what a player experiences as
/// one thing. That is fixed upstream in the engine rather than papered over
/// here: the chill's effects are bound into a [`CompoundDebuff`](crate::states::play_match::components::CompoundDebuff), so it is one
/// entry that states both (see [`NamedAura::riders`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineAura {
    /// The Power Word: Shield cooldown marker, applied alongside the shield.
    WeakenedSoul,
    /// Granted by picking up an arena Shadow Sight orb.
    ShadowSight,
    /// The slow a triggered Frost Trap's zone re-applies each tick.
    FrostTrapSlow,
    /// The buff one element's totem pulses onto nearby allies. Nested on
    /// [`TotemElement`] so a fifth element needs no variant here.
    TotemBuff(TotemElement),
    /// The school lockout a successful interrupt leaves behind, named after the
    /// interrupting ability (`combat_core::damage` builds it that way). Nested
    /// on [`AbilityType`] and DERIVED from the interrupt flags, so interrupt
    /// N+1 gets a lockout entry with no code here.
    InterruptLockout(AbilityType),
    /// The Rogue's weapon-coating marker — a permanent SELF-BUFF named after
    /// the poison, distinct from the debuff that poison applies on hit. Nested
    /// on [`RoguePoison`] so poison N+1 needs no variant here.
    WeaponPoisonCoating(RoguePoison),
    /// The silence Unstable Affliction's dispel backlash puts on the dispeller,
    /// which shares the DoT's name.
    DispelBacklashSilence,
    /// The chill a Frost Armor proc hangs on a melee attacker — ONE debuff
    /// doing two things (a movement slow and an attack-speed slow), so ONE
    /// entry. It was two entries wearing different removal badges until the
    /// engine learned to bind the two effects into one debuff; see
    /// [`CompoundDebuff`](crate::states::play_match::components::CompoundDebuff).
    FrostArmorChill,
    /// The stat buff a PROC TRINKET grants its wearer, named after the trinket.
    /// Nested on [`ItemId`] and derived from `items.ron` — every item carrying
    /// a `proc:` block gets an entry, so trinket N+1 (the whole point of
    /// AS-61) needs no variant here.
    ProcTrinketBuff(ItemId),
}

impl EngineAura {
    /// Every engine-originated aura, in display order.
    ///
    /// The two nested variants expand from their own sources, so this list only
    /// has to name the genuinely one-off auras.
    pub fn all(abilities: &AbilityDefinitions, items: &ItemDefinitions) -> Vec<EngineAura> {
        let mut all = vec![
            EngineAura::WeakenedSoul,
            EngineAura::ShadowSight,
            EngineAura::FrostTrapSlow,
            EngineAura::DispelBacklashSilence,
            EngineAura::FrostArmorChill,
        ];
        all.extend(TotemElement::ALL.iter().copied().map(EngineAura::TotemBuff));
        all.extend(
            RoguePoison::ALL
                .iter()
                .copied()
                .map(EngineAura::WeaponPoisonCoating),
        );
        let mut interrupts: Vec<AbilityType> = abilities
            .iter()
            .filter(|(_, def)| def.is_interrupt && def.lockout_duration > 0.0)
            .map(|(ability, _)| *ability)
            .collect();
        interrupts.sort_unstable();
        all.extend(interrupts.into_iter().map(EngineAura::InterruptLockout));
        // Walked in `ItemId::all()` order rather than `items.iter()` order:
        // `ItemDefinitions` is backed by a `HashMap`, and the catalog's sort is
        // by NAME, so two items sharing a name would otherwise come out in a
        // per-process order.
        all.extend(
            ItemId::all()
                .iter()
                .filter(|id| items.get(id).is_some_and(|i| i.proc.is_some()))
                .copied()
                .map(EngineAura::ProcTrinketBuff),
        );
        all
    }
}

// ============================================================================
// CATALOG
// ============================================================================

/// How long an entry's aura lasts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Persistence {
    /// A fixed duration in seconds.
    Seconds(f32),
    /// Re-applied for as long as its source persists (a totem's pulse, a trap
    /// zone's tick). The aura's own duration is a refresh window measured in
    /// frames of gameplay, not a number a player could use, so the page says
    /// what is actually true instead of printing it.
    WhileSourceActive(&'static str),
    /// Stamped once and never expires in play — the Rogue's weapon coating,
    /// whose literal duration (an hour) outlasts the 300s match cap by enough
    /// that printing it would mislead.
    WholeMatch,
}

/// Where a named aura comes from — the one answer to "what puts this on me?",
/// and the one field an ability page reads when it wants the reverse.
///
/// ## Why this is a type and not an `Option<AbilityType>`
///
/// It was one, and "no applying ability" was spelled `None`. That made two
/// different facts identical on the page: an aura whose applying ability the
/// registry had never filled in, and an aura that genuinely has none. Weakened
/// Soul sat in the first group wearing the second group's clothes — Power Word:
/// Shield spawns it on the shielded ally at the Priest's cast site
/// (`class_ai/priest.rs`), but its page said only "applied by an engine
/// mechanic", so the debuff that GATES the shield read as though nothing in the
/// game produced it.
///
/// With a variant per fact the blank is unrepresentable: [`Self::Mechanic`] has
/// to NAME what applies the aura, so an entry cannot go quiet about its origin
/// the way `None` let it.
///
/// ## One model, both directions
///
/// The aura page asks this what to link to; an ability page asks the catalog
/// which entries name it, through [`applied_by`]. Both readings come off this
/// one field, so a link cannot exist in one direction only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraSource {
    /// Applied by an ability with a page of its own. That page is where the
    /// APPLIED BY section links — and, when the entry carries
    /// [`AuraArt::FromSource`], where its icon comes from.
    Ability(AbilityType),
    /// Applied by a match mechanic with no ability behind it — the arena's
    /// Shadow Sight orbs are the only one today. The string names the mechanic
    /// inside [`Self::mechanic_line`]'s sentence; it is not optional, because an
    /// unnamed origin is the hole this type exists to close.
    Mechanic(&'static str),
}

impl AuraSource {
    /// The applying ability, if there is one. The icon lookup and the reverse
    /// lookup both go through this rather than matching on the variant.
    pub fn ability(self) -> Option<AbilityType> {
        match self {
            AuraSource::Ability(ability) => Some(ability),
            AuraSource::Mechanic(_) => None,
        }
    }

    /// What the APPLIED BY section says when there is no ability page to link
    /// to. `None` for an ability source, whose section is a link instead.
    pub fn mechanic_line(self) -> Option<String> {
        match self {
            AuraSource::Ability(_) => None,
            AuraSource::Mechanic(mechanic) => Some(format!(
                "No ability applies this — it comes from {}.",
                mechanic
            )),
        }
    }
}

/// Where an entry's ICON comes from — a separate question from where the AURA
/// comes from, which is why they are separate fields.
///
/// Borrowing the applying ability's icon is the default and the in-match
/// convention (`get_aura_icon_key`), and it is right whenever the aura simply
/// IS that ability's effect. It is wrong for a marker an ability leaves
/// BEHIND: Weakened Soul wearing the Power Word: Shield icon would put the
/// shield's art on the debuff that blocks the next shield.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraArt {
    /// Borrow the applying ability's icon.
    FromSource,
    /// The aura has art of its own, under a `GENERIC_AURA_ICONS` key. That is
    /// the same table the actor frames load from, so a page and a buff bar
    /// cannot show different art for one aura.
    Own(&'static str),
    /// The aura wears the icon of the ITEM that applies it — a proc trinket's
    /// buff. Resolved through `item_aura_icon_key`, the key the buff bar uses.
    Item(ItemId),
}

/// One catalog entry: a named aura, fully resolved.
///
/// Built by [`catalog`]; never stored, so a RON edit is picked up on the next
/// build of the registry.
pub struct NamedAura {
    pub id: AuraId,
    /// The catalog's name for this aura — what the index row, the search hit
    /// and the page header say. Usually identical to [`Self::frame_name`].
    pub name: String,
    /// The name the engine writes into `Aura::ability_name`, which is what the
    /// player reads off the actor frames.
    ///
    /// It differs from [`Self::name`] only where several distinct auras share
    /// one engine name and the catalog has to tell them apart (the Rogue's
    /// poison coating, Unstable Affliction's backlash silence, the two Frost
    /// Armor procs). Keeping both means the catalog can disambiguate for the
    /// reader while `tests/aura_catalog_audit.rs` still resolves an apply site
    /// to the RIGHT entry: the audit keys on (frame name, mechanic).
    pub frame_name: String,
    /// The mechanic badge.
    pub mechanic: AuraType,
    /// What applies it: an ability to link to, or the mechanic to name.
    pub source: AuraSource,
    /// Where its icon comes from. Separate from [`Self::source`] because an
    /// aura can have an applying ability AND art of its own.
    pub art: AuraArt,
    /// A representative aura, built the way the simulation builds it. Every
    /// classification on the page is a question asked of THIS.
    ///
    /// For a COMPOUND debuff (see [`CompoundDebuff`](crate::states::play_match::components::CompoundDebuff)) this is the debuff's
    /// FACE, and [`Self::riders`] holds the rest. Classification asks the face
    /// because that is what the engine asks: a dispel rolls against the face
    /// and takes the riders with it.
    pub sample: Aura,
    /// The other effects of a compound debuff, beyond [`Self::sample`]. Empty
    /// for every ordinary aura — one aura, one effect, one entry.
    ///
    /// This is why the catalog no longer needs two "Frost Armor (…)" rows: the
    /// engine binds the chill's two effects into one debuff, so the catalog
    /// lists one entry that states both.
    pub riders: Vec<Aura>,
    pub persistence: Persistence,
    /// Spell power added to [`Self::sample`]'s magnitude per point, from the
    /// ability's `applies_aura.magnitude_coefficient`. Non-zero only for
    /// Power Word: Shield today; the stat block says so rather than presenting
    /// the unscaled base as the whole story.
    pub magnitude_coefficient: f32,
    /// Generated effect sentence.
    pub description: String,
    /// Extra provenance line for engine-originated entries: where the aura
    /// comes from when there is no ability whose page would say.
    pub provenance: Option<String>,
}

impl NamedAura {
    pub fn is_buff(&self) -> bool {
        is_buff_aura(&self.mechanic)
    }

    /// Every mechanic this ONE entry covers: its face, then each rider's. A
    /// single-effect aura yields exactly its `mechanic`.
    ///
    /// The cross-links and `tests/aura_catalog_audit.rs` both walk this rather
    /// than `mechanic` alone, so a compound debuff is reachable from every
    /// mechanic it actually applies — the Frost Armor chill shows up under
    /// "other Attack Speed Slow effects" even though its badge says Slow.
    pub fn mechanics(&self) -> Vec<AuraType> {
        let mut all = vec![self.mechanic];
        all.extend(self.riders.iter().map(|rider| rider.effect_type));
        all
    }

    /// `Debuff · Damage over Time` — the one-line identity shared by index
    /// rows, search hits and the detail header.
    pub fn subtitle(&self) -> String {
        subtitle_for(self.mechanic)
    }

    /// How this aura comes off early, as a badge label plus a tooltip.
    /// Answered by the ENGINE's own predicates on this entry's representative
    /// aura, so it can never disagree with what a dispel actually does.
    ///
    /// **The wording describes the removal CLASS and names no ability.** A
    /// debuff is Magic, Poison, Disease, Curse or Physical, and each class is
    /// served by a category of effect — dispels, cleanses, curse-removal, or
    /// the rare effects that clear physical harm. Which spells belong to each
    /// category is the engine's business, and it changes: a Mage decurse, a
    /// second physical-removal trinket, a Shaman cleanse would each force a
    /// rewrite of every tooltip that had spelled out a name. So none do.
    ///
    /// A class with no ability serving it today is still a class, not a dead
    /// end — "Curse ... none in the arena yet" is a different statement from
    /// "cannot be removed", and a Warlock reading the page should see the
    /// difference. The absolute rung is reserved for what genuinely nothing
    /// touches: the unpurgeable self-buffs (Divine Shield, Berserker Rage), a
    /// proc trinket's physical buff, and the mechanical markers (Weakened
    /// Soul, Shadow Sight, weapon poisons).
    ///
    /// The buff half is disjoint by construction: a dispel lifts harmful magic
    /// off an ally, a purge strips buffs off an enemy.
    fn removal(&self) -> (&'static str, &'static str) {
        if self.sample.can_be_dispelled() {
            ("Dispellable", "Magic. Removed by dispels.")
        } else if self.sample.is_cleansable_poison() {
            (
                "Cleansable",
                "Poison. Not affected by dispels; removed by cleanses.",
            )
        } else if self.sample.is_curse() {
            (
                "Curse",
                "Curse. Not affected by dispels or cleanses; removed by curse-removal effects — \
                 none in the arena yet.",
            )
        } else if self.sample.is_physical() && !self.sample.is_hostile_effect() {
            // A physical BUFF — what an item does for its wearer. Nothing
            // clears it early: the "effects that clear physical debuffs" the
            // debuff arm below points at are about harm, and a buff is not.
            (
                "Cannot be removed",
                "Physical — an item's effect, not a spell, however it looks. No dispel, cleanse \
                 or purge can take it off. It ends when it ends.",
            )
        } else if self.sample.is_physical() {
            (
                "Immune to dispel",
                "Physical — a wound or an impact, with no magic on it to dissipate. Not affected \
                 by dispels, cleanses or purges; removed only by effects that clear physical \
                 debuffs.",
            )
        } else if self.sample.can_be_purged() {
            (
                "Purgeable",
                "Beneficial magic. An enemy can strip this with a purge.",
            )
        } else if self.sample.is_hostile_effect() {
            // Stuns and interrupt lockouts: not a physical debuff, but not a
            // dispellable KIND of effect either. Deliberately says nothing
            // about the class — some of these are magic (Hammer of Justice is
            // Holy) and some are schoolless with no determined class at all.
            //
            // A compound debuff is classified by its FACE, which is why Frost
            // Armor's chill no longer lands here: its attack-speed effect is a
            // rider on a dispellable slow rather than an entry of its own.
            (
                "Immune to dispel",
                "No dispel, cleanse or purge removes an effect of this kind; only effects that \
                 clear harmful effects outright end it early.",
            )
        } else {
            (
                "Cannot be removed",
                "No dispel, cleanse or purge in the game can take this off. It ends when it ends.",
            )
        }
    }
}

/// Build the whole catalog, sorted by name.
///
/// Deterministic: `AbilityDefinitions` iterates a `HashMap`, so the sort is
/// what makes the index, the search registry and the snapshots stable.
pub fn catalog(abilities: &AbilityDefinitions, items: &ItemDefinitions) -> Vec<NamedAura> {
    let mut entries: Vec<NamedAura> = abilities
        .iter()
        .filter_map(|(ability, _)| ron_entry(*ability, abilities))
        .collect();

    entries.extend(
        EngineAura::all(abilities, items)
            .into_iter()
            .filter_map(|engine| engine_entry(engine, abilities, items)),
    );
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Build the ONE entry an address names, without walking the catalog. Used
/// where only a single aura is on screen (a tooltip); the index and the
/// sibling cross-links still need the whole thing.
pub fn entry_of(
    id: AuraId,
    abilities: &AbilityDefinitions,
    items: &ItemDefinitions,
) -> Option<NamedAura> {
    match id {
        AuraId::Ability(ability) => ron_entry(ability, abilities),
        AuraId::Engine(engine) => engine_entry(engine, abilities, items),
    }
}

/// The entry an ability's `applies_aura` block produces. `None` for an ability
/// that applies no aura — which is most of them.
fn ron_entry(ability: AbilityType, abilities: &AbilityDefinitions) -> Option<NamedAura> {
    let def = abilities.get(&ability)?;
    let effect = def.applies_aura.as_ref()?;
    // Built through the SIMULATION's own constructor, so the sample cannot
    // drift from the aura a cast actually applies.
    let pending = AuraPending::from_ability(
        bevy::prelude::Entity::PLACEHOLDER,
        bevy::prelude::Entity::PLACEHOLDER,
        def,
    )?;
    Some(NamedAura {
        id: AuraId::Ability(ability),
        name: def.name.clone(),
        frame_name: def.name.clone(),
        mechanic: effect.aura_type,
        source: AuraSource::Ability(ability),
        art: AuraArt::FromSource,
        sample: pending.aura,
        // `AbilityConfig::applies_aura` is a single `Option`, so a RON-defined
        // ability applies exactly one effect and can never be a compound.
        riders: Vec::new(),
        persistence: Persistence::Seconds(effect.duration),
        magnitude_coefficient: effect.magnitude_coefficient,
        description: build_aura_description(effect),
        provenance: None,
    })
}

/// `Debuff · Damage over Time` — polarity plus mechanic, the one-line identity
/// of any named aura.
pub fn subtitle_for(mechanic: AuraType) -> String {
    format!(
        "{} · {}",
        if is_buff_aura(&mechanic) {
            "Buff"
        } else {
            "Debuff"
        },
        mechanic.display_name()
    )
}

/// The named auras sharing `mechanic`, excluding `self_id` — the cross-links.
pub fn siblings(catalog: &[NamedAura], mechanic: AuraType, self_id: AuraId) -> Vec<&NamedAura> {
    catalog
        .iter()
        .filter(|entry| entry.mechanics().contains(&mechanic) && entry.id != self_id)
        .collect()
}

/// Every named aura `ability` applies — the reverse of [`AuraSource::Ability`].
///
/// An ability page needs this to show what its casts leave behind; an aura page
/// needs [`AuraSource`] to point back. Reading one field both ways is what keeps
/// the two directions from disagreeing.
///
/// Power Word: Shield is the case that earns it: it applies its own absorb and
/// the Weakened Soul marker, so an ability page derived from `applies_aura`
/// alone would show half of what the cast does.
pub fn applied_by(catalog: &[NamedAura], ability: AbilityType) -> Vec<&NamedAura> {
    catalog
        .iter()
        .filter(|entry| entry.source.ability() == Some(ability))
        .collect()
}

/// The OTHER named auras the same ability applies — [`applied_by`] minus the
/// entry you are standing on.
///
/// An ability is not limited to one aura: Frost Armor is a Mage self-buff and
/// the procs it hangs on melee attackers, Unstable Affliction is a
/// damage-over-time and the silence its dispel backlash inflicts, a Rogue
/// poison is a coating marker and the debuff that coating applies. Each of
/// those already pointed at its ability, and `applied_by` gives an ability page
/// the full set — but between the AURA pages the link ran one way only, so the
/// page a Mage would open to ask what Frost Armor does was the page that never
/// mentioned the procs.
///
/// Reads the same `source` field as both of those, so an ability that grows a
/// second aura is cross-linked in every direction with no code per entry.
pub fn source_siblings(
    catalog: &[NamedAura],
    source: AuraSource,
    self_id: AuraId,
) -> Vec<&NamedAura> {
    let Some(ability) = source.ability() else {
        return Vec::new();
    };
    applied_by(catalog, ability)
        .into_iter()
        .filter(|entry| entry.id != self_id)
        .collect()
}

pub fn find(catalog: &[NamedAura], id: AuraId) -> Option<&NamedAura> {
    catalog.iter().find(|entry| entry.id == id)
}

// ============================================================================
// ENGINE REGISTRY
// ============================================================================

/// Everything an [`EngineAura`] arm has to answer. A struct rather than a
/// seven-slot tuple because two of the fields (the sample's break-on-damage
/// threshold, and the frame name) are exactly the ones a wrong default gets
/// silently wrong.
struct EngineSpec {
    /// The catalog's name for the entry.
    name: String,
    /// What the actor frames call it. `None` means "same as `name`".
    frame_name: Option<String>,
    mechanic: AuraType,
    source: AuraSource,
    magnitude: f32,
    school: Option<SpellSchool>,
    /// Read from the constant the apply site reads. NOT defaulted: Shadow Sight
    /// once applied `0.0` (breaks on any damage) where every other engine aura
    /// applies `-1.0`, and a shared default printed "Never" on a page for an
    /// aura the first hit removed. Every entry names its own value so the page
    /// can only ever say what the engine does.
    break_on_damage: f32,
    /// The removal class, read from the constructor the apply site uses
    /// wherever there is one. Not defaulted, for the same reason as
    /// `break_on_damage`: a proc trinket's buff is `Physical`, and a page
    /// defaulting it to `Auto` would advertise a purge that cannot happen.
    dispel_type: DispelType,
    persistence: Persistence,
    /// The RIDER effects of a compound debuff — the effects this entry covers
    /// beyond its face. Empty for every ordinary aura. See
    /// [`CompoundDebuff`](crate::states::play_match::components::CompoundDebuff); the arm builds these from the same constructor the
    /// apply site uses, so the page lists exactly what lands.
    riders: Vec<Aura>,
    provenance: String,
}

impl EngineSpec {
    /// The default for every arm that is not a compound debuff. Spelled out as
    /// a helper rather than a `Default` impl so the other eight fields stay
    /// mandatory — `break_on_damage` in particular must never default.
    fn no_riders() -> Vec<Aura> {
        Vec::new()
    }
}

/// Which engine auras carry art of their own, and which borrow their ability's.
///
/// Exhaustive on purpose — no `_` arm. Engine aura N+1 has to state which it is,
/// because the failure mode is silent: a wrong borrow renders a plausible icon
/// that means something else, and a missing one renders the placeholder tile
/// that sent this card here in the first place.
fn engine_art(engine: EngineAura) -> AuraArt {
    match engine {
        // Applied by Power Word: Shield, but it is the debuff that BLOCKS the
        // next shield — borrowing the shield's icon would say the opposite.
        EngineAura::WeakenedSoul => AuraArt::Own("aura_weakened_soul"),
        // An arena orb pickup: no ability icon exists to borrow.
        EngineAura::ShadowSight => AuraArt::Own("aura_shadow_sight"),
        EngineAura::FrostTrapSlow
        | EngineAura::TotemBuff(_)
        | EngineAura::InterruptLockout(_)
        | EngineAura::WeaponPoisonCoating(_)
        | EngineAura::DispelBacklashSilence
        | EngineAura::FrostArmorChill => AuraArt::FromSource,
        // No ability applies it; the TRINKET does, and the buff wears the
        // trinket's own icon — as it does in the buff bar, which reads the
        // same key off the aura's `source_item`.
        EngineAura::ProcTrinketBuff(item) => AuraArt::Item(item),
    }
}

/// Resolve one engine-originated aura into a catalog entry.
///
/// Every value here is read from the constant, shared constructor or spec the
/// apply site reads, so the page cannot state a number the simulation does not
/// use.
/// `None` only for a `ProcTrinketBuff` address whose item no longer carries a
/// proc — an address that `EngineAura::all` cannot produce, but that a stale
/// navigation stack can still hold.
fn engine_entry(
    engine: EngineAura,
    abilities: &AbilityDefinitions,
    items: &ItemDefinitions,
) -> Option<NamedAura> {
    let spec = match engine {
        EngineAura::WeakenedSoul => EngineSpec {
            name: "Weakened Soul".to_string(),
            frame_name: None,
            mechanic: AuraType::WeakenedSoul,
            source: AuraSource::Ability(AbilityType::PowerWordShield),
            magnitude: 0.0,
            school: None,
            break_on_damage: -1.0,
            dispel_type: DispelType::Auto,
            persistence: Persistence::Seconds(WEAKENED_SOUL_DURATION),
            riders: EngineSpec::no_riders(),
            provenance: "Placed on the ally the moment the shield lands: one cast applies both."
                .to_string(),
        },
        EngineAura::ShadowSight => EngineSpec {
            name: "Shadow Sight".to_string(),
            frame_name: None,
            mechanic: AuraType::ShadowSight,
            source: AuraSource::Mechanic("a Shadow Sight orb"),
            magnitude: 1.0,
            school: None,
            break_on_damage: SHADOW_SIGHT_BREAK_ON_DAMAGE,
            dispel_type: DispelType::Auto,
            persistence: Persistence::Seconds(SHADOW_SIGHT_DURATION),
            riders: EngineSpec::no_riders(),
            provenance: format!(
                "Granted by picking up one of the two orbs that spawn in mid-arena {:.0} sec \
                 after the gates open.",
                SHADOW_SIGHT_SPAWN_TIME
            ),
        },
        EngineAura::FrostTrapSlow => EngineSpec {
            name: "Frost Trap".to_string(),
            frame_name: None,
            mechanic: AuraType::MovementSpeedSlow,
            source: AuraSource::Ability(AbilityType::FrostTrap),
            magnitude: FROST_TRAP_SLOW_MAGNITUDE,
            school: Some(SpellSchool::Frost),
            break_on_damage: -1.0,
            dispel_type: DispelType::Auto,
            persistence: Persistence::WhileSourceActive("while you stand in the zone"),
            riders: EngineSpec::no_riders(),
            provenance: format!(
                "Re-applied every tick by a triggered Frost Trap's slow zone, which itself \
                 lasts {:.0} sec.",
                FROST_TRAP_ZONE_DURATION
            ),
        },
        EngineAura::TotemBuff(element) => {
            let (ability, aura_type, magnitude, school) =
                crate::states::play_match::class_ai::shaman::totem_spec(element);
            EngineSpec {
                name: element.buff_name().to_string(),
                frame_name: None,
                mechanic: aura_type,
                source: AuraSource::Ability(ability),
                magnitude,
                school: Some(school),
                break_on_damage: -1.0,
                dispel_type: DispelType::Auto,
                persistence: Persistence::WhileSourceActive("while you stand near the totem"),
                riders: EngineSpec::no_riders(),
                provenance: format!(
                    "Pulsed onto nearby allies by a dropped totem, which itself lasts {:.0} sec.",
                    TOTEM_DURATION
                ),
            }
        }
        EngineAura::InterruptLockout(ability) => {
            let def = abilities.get(&ability);
            EngineSpec {
                name: def
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("{:?}", ability)),
                frame_name: None,
                mechanic: AuraType::SpellSchoolLockout,
                source: AuraSource::Ability(ability),
                // The magnitude encodes WHICH school was locked and depends on
                // what the interrupt caught, so it is not a fact about this
                // aura; `magnitude_row` prints nothing for this mechanic.
                magnitude: 0.0,
                school: None,
                break_on_damage: -1.0,
                dispel_type: DispelType::Auto,
                persistence: Persistence::Seconds(
                    def.map(|d| d.lockout_duration).unwrap_or_default(),
                ),
                riders: EngineSpec::no_riders(),
                provenance: "Left behind when this interrupt lands. Only the school of the \
                             interrupted spell is locked — the target's other schools keep \
                             working."
                    .to_string(),
            }
        }
        EngineAura::WeaponPoisonCoating(poison) => {
            let sample = weapon_poison_marker_aura(poison);
            EngineSpec {
                name: format!("{} (weapon coating)", poison.name()),
                frame_name: Some(sample.ability_name.clone()),
                mechanic: sample.effect_type,
                source: AuraSource::Ability(poison.ability()),
                magnitude: sample.magnitude,
                school: sample.spell_school,
                break_on_damage: sample.break_on_damage_threshold,
                dispel_type: sample.dispel_type,
                persistence: Persistence::WholeMatch,
                riders: EngineSpec::no_riders(),
                provenance: "A Rogue carries this from the opening bell — it marks the coated \
                             weapon. The slow itself comes from the coating's on-hit proc, not \
                             from this mark."
                    .to_string(),
            }
        }
        EngineAura::DispelBacklashSilence => {
            let sample = dispel_backlash_silence_aura(None, DISPEL_BACKLASH_SILENCE_DURATION);
            EngineSpec {
                name: "Unstable Affliction (dispel backlash)".to_string(),
                frame_name: Some(sample.ability_name.clone()),
                mechanic: sample.effect_type,
                source: AuraSource::Ability(AbilityType::UnstableAffliction),
                magnitude: sample.magnitude,
                school: sample.spell_school,
                break_on_damage: sample.break_on_damage_threshold,
                dispel_type: sample.dispel_type,
                persistence: Persistence::Seconds(sample.duration),
                riders: EngineSpec::no_riders(),
                provenance: "Punishes whoever lifts an Unstable Affliction off an ally, \
                             alongside the backlash damage. Dispelling it on your own team \
                             costs nothing."
                    .to_string(),
            }
        }
        EngineAura::FrostArmorChill => {
            // Built from the simulation's own constructor for the whole
            // debuff, so the page cannot list an effect the proc does not
            // apply — or miss one it does.
            let [face, rider] = frost_armor_chill_auras();
            EngineSpec {
                // No disambiguating qualifier on the mechanic: there is one
                // chill now, and the only other "Frost Armor" is the Mage's
                // self-buff, which is a BUFF and files on the other side of
                // the catalog.
                name: "Frost Armor (chill)".to_string(),
                frame_name: Some(face.ability_name.clone()),
                mechanic: face.effect_type,
                source: AuraSource::Ability(AbilityType::FrostArmor),
                magnitude: face.magnitude,
                school: face.spell_school,
                break_on_damage: face.break_on_damage_threshold,
                dispel_type: face.dispel_type,
                persistence: Persistence::Seconds(face.duration),
                riders: vec![rider],
                provenance: "Hung on any melee attacker who strikes a Mage wearing Frost Armor. \
                             One debuff doing two things — a dispel that lifts it takes both. It \
                             does not stack with itself: a second hit while it is up refreshes \
                             nothing."
                    .to_string(),
            }
        }
        EngineAura::ProcTrinketBuff(item) => {
            let def = items.get(&item)?;
            let proc = def.proc.as_ref()?;
            EngineSpec {
                name: def.name.clone(),
                frame_name: None,
                mechanic: proc.effect,
                // The trinket is not an ability and has no page of its own to
                // link to from here; the ITEM's page is where its numbers live,
                // and the provenance line below names it.
                source: AuraSource::Mechanic("a proc trinket you are wearing"),
                magnitude: proc.magnitude,
                school: None,
                // A proc buff is never broken by damage — see `ProcConfig::aura`.
                break_on_damage: -1.0,
                dispel_type: proc.aura(item, &def.name).dispel_type,
                persistence: Persistence::Seconds(proc.duration),
                riders: EngineSpec::no_riders(),
                // Read straight off the same `ProcConfig` the simulation rolls
                // against, so the page cannot state a chance, a duration or a
                // cooldown the trinket does not have. The trinket is not named
                // again here — the page header IS its name.
                provenance: proc_description(proc),
            }
        }
    };

    let EngineSpec {
        name,
        frame_name,
        mechanic,
        source,
        magnitude,
        school,
        break_on_damage,
        dispel_type,
        persistence,
        riders,
        provenance,
    } = spec;

    let tick_interval = if mechanic == AuraType::HealingOverTime {
        1.0
    } else {
        0.0
    };
    let duration = match persistence {
        Persistence::Seconds(secs) => secs,
        // The refresh window itself; the page prints the persistence note
        // instead, but the sample must still be a realistic aura.
        Persistence::WhileSourceActive(_) => 2.0,
        Persistence::WholeMatch => WEAPON_POISON_MARKER_DURATION,
    };
    let sample = Aura {
        effect_type: mechanic,
        duration,
        magnitude,
        break_on_damage_threshold: break_on_damage,
        tick_interval,
        spell_school: school,
        dispel_type,
        ..Default::default()
    };

    // Engine auras have no `applies_aura` block to generate prose from, so the
    // MECHANIC's own player-facing sentence carries the page — one sentence
    // per effect for a compound debuff, in face-then-riders order, so the
    // prose covers everything the debuff actually does.
    let description = std::iter::once(mechanic)
        .chain(riders.iter().map(|rider| rider.effect_type))
        .map(|effect| effect.description())
        .collect::<Vec<_>>()
        .join(" ");

    // A disambiguated entry has to say what the frames actually call it, or a
    // player matching the buff bar against the catalog finds no such name.
    // Appended here rather than written into each arm, so collision N+1 gets
    // the sentence for free.
    let frame_name = frame_name.unwrap_or_else(|| name.clone());
    let provenance = if frame_name == name {
        provenance
    } else {
        format!(
            "{} Shown on the actor frames as “{}”.",
            provenance, frame_name
        )
    };

    Some(NamedAura {
        id: AuraId::Engine(engine),
        name,
        frame_name,
        mechanic,
        source,
        art: engine_art(engine),
        sample,
        riders,
        persistence,
        magnitude_coefficient: 0.0,
        description,
        provenance: Some(provenance),
    })
}

// ============================================================================
// SEARCH
// ============================================================================

/// Contribute every named aura to the search registry.
pub fn search_entries(
    abilities: &AbilityDefinitions,
    items: &ItemDefinitions,
    out: &mut Vec<SearchEntry>,
) {
    for entry in catalog(abilities, items) {
        out.push(SearchEntry::new(
            Topic::Aura(entry.id),
            entry.name.clone(),
            entry.subtitle(),
        ));
    }
}

/// Name, mechanic, source and art of ONE address, resolved without building the
/// whole catalog.
///
/// The linked-icon widget asks a `Topic` for its name and icon once per widget
/// it draws, and the index draws fifty of them, so this path has to stay cheap.
/// Resolving one engine aura through [`engine_entry`] is fine — that builds a
/// single entry, not the catalog.
struct Identity {
    name: String,
    mechanic: AuraType,
    source: AuraSource,
    art: AuraArt,
}

fn identity(
    id: AuraId,
    abilities: &AbilityDefinitions,
    items: &ItemDefinitions,
) -> Option<Identity> {
    match id {
        AuraId::Ability(ability) => {
            let def = abilities.get(&ability)?;
            let effect = def.applies_aura.as_ref()?;
            Some(Identity {
                name: def.name.clone(),
                mechanic: effect.aura_type,
                source: AuraSource::Ability(ability),
                art: AuraArt::FromSource,
            })
        }
        AuraId::Engine(engine) => {
            let entry = engine_entry(engine, abilities, items)?;
            Some(Identity {
                name: entry.name,
                mechanic: entry.mechanic,
                source: entry.source,
                art: entry.art,
            })
        }
    }
}

/// The display name of one aura address, for [`Topic::name`].
pub fn name_of(
    id: AuraId,
    abilities: &AbilityDefinitions,
    items: &ItemDefinitions,
) -> Option<String> {
    identity(id, abilities, items).map(|identity| identity.name)
}

/// The `Buff · Mechanic` subtitle of one aura address, for [`Topic::subtitle`].
pub fn subtitle_of(
    id: AuraId,
    abilities: &AbilityDefinitions,
    items: &ItemDefinitions,
) -> Option<String> {
    identity(id, abilities, items).map(|identity| subtitle_for(identity.mechanic))
}

/// The icon key of one aura address, in the keyspace `AbilityIcons` uses: an
/// ability's display NAME for a borrowed icon, or a `GENERIC_AURA_ICONS` key
/// for an aura with art of its own. `None` only if the address resolves to
/// nothing — every catalog entry has an icon.
pub fn icon_key(
    id: AuraId,
    abilities: &AbilityDefinitions,
    items: &ItemDefinitions,
) -> Option<String> {
    let identity = identity(id, abilities, items)?;
    match identity.art {
        AuraArt::Own(key) => Some(key.to_string()),
        AuraArt::Item(item) => Some(item_aura_icon_key(item)),
        AuraArt::FromSource => {
            let ability = identity.source.ability()?;
            abilities.get(&ability).map(|def| def.name.clone())
        }
    }
}

// ============================================================================
// INDEX PAGE
// ============================================================================

/// The buff/debuff index. Returns the aura whose row was clicked.
pub fn render_index(ui: &mut egui::Ui, data: &EncyclopediaData) -> Option<Topic> {
    let catalog = catalog(data.abilities, data.items);

    ui.label(
        egui::RichText::new(
            "Named as players see them on the actor frames — Rend and Corruption are distinct \
             debuffs, not one “damage over time”. The tag on each row is the shared MECHANIC \
             underneath.",
        )
        .size(13.0)
        .color(MUTED)
        .italics(),
    );
    ui.add_space(12.0);

    let mut clicked = None;
    // Buffs and debuffs sit SIDE BY SIDE, each column top-aligned: `ui.columns`
    // rather than a wrapped row, because the two lists are different lengths
    // and a wrapped row starts the second one below the end of the first.
    ui.columns(2, |columns| {
        for (column, (label, want_buff, color)) in columns
            .iter_mut()
            .zip([("BUFFS", true, BUFF), ("DEBUFFS", false, DEBUFF)])
        {
            let width = column.available_width().min(COLUMN_MAX_WIDTH);
            let group: Vec<&NamedAura> = catalog
                .iter()
                .filter(|e| e.is_buff() == want_buff)
                .collect();
            column.label(
                egui::RichText::new(format!("{}  ({})", label, group.len()))
                    .size(12.5)
                    .color(color),
            );
            column.add_space(4.0);
            for entry in group {
                if let Some(topic) = widget::row(
                    column,
                    Topic::Aura(entry.id),
                    entry.mechanic.display_name(),
                    width,
                    data,
                ) {
                    clicked = Some(topic);
                }
            }
        }
    });

    clicked
}

// ============================================================================
// DETAIL PAGE
// ============================================================================

/// One named aura's page. Returns the topic a link on it opened.
pub fn render_detail(ui: &mut egui::Ui, id: AuraId, data: &EncyclopediaData) -> Option<Topic> {
    let catalog = catalog(data.abilities, data.items);
    let Some(entry) = find(&catalog, id) else {
        ui.label(egui::RichText::new("Unknown aura").size(16.0).color(MUTED));
        return None;
    };

    widget::detail_header(ui, Topic::Aura(id), "", data);
    ui.add_space(8.0);

    // --- Badges: polarity, mechanic, how it comes off ---
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let (polarity, color) = if entry.is_buff() {
            ("Buff", BUFF)
        } else {
            ("Debuff", DEBUFF)
        };
        badge(ui, polarity, color).on_hover_text(if entry.is_buff() {
            "A beneficial effect. Buffs show a gold border on the actor frames."
        } else {
            "A harmful effect. Debuffs show a red border on the actor frames."
        });
        badge(
            ui,
            &format!("Mechanic — {}", entry.mechanic.display_name()),
            MUTED,
        )
        .on_hover_text(entry.mechanic.description());
        let (removal, removal_note) = entry.removal();
        badge(ui, removal, MUTED).on_hover_text(removal_note);
    });

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(&entry.description)
            .size(14.0)
            .color(TEXT),
    );
    if let Some(provenance) = &entry.provenance {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(provenance)
                .size(12.0)
                .color(DIM)
                .italics(),
        );
    }

    ui.add_space(12.0);
    widget::stat_rows(ui, "encyclopedia_aura_stats", &stat_rows(entry));

    // --- Diminishing returns ---
    if let Some(category) = entry.sample.dr_category() {
        widget::section_heading(ui, "DIMINISHING RETURNS");
        ui.label(
            egui::RichText::new(format!("Category: {}", dr_category_name(category)))
                .size(13.5)
                .color(TEXT),
        );
        ui.add_space(4.0);
        ui.label(egui::RichText::new(dr_rules_text()).size(12.5).color(MUTED));
    }

    let mut clicked = None;

    // --- Applied by, and anything else the same ability applies ---
    widget::section_heading(ui, "APPLIED BY");
    match entry.source {
        AuraSource::Ability(ability) => {
            let width = ui.available_width().min(COLUMN_MAX_WIDTH);
            if let Some(topic) = widget::row(ui, Topic::Ability(ability), "", width, data) {
                clicked = Some(topic);
            }
            // The link from an aura to its ability always existed; the link
            // BETWEEN the auras one ability hangs did not, so a Mage on the
            // Frost Armor buff's own page was never told the procs exist.
            let kin = source_siblings(&catalog, entry.source, id);
            if !kin.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} also applies:",
                        Topic::Ability(ability).name(data)
                    ))
                    .size(12.5)
                    .color(MUTED),
                );
                ui.add_space(2.0);
                for sibling in kin {
                    if let Some(topic) = widget::row(
                        ui,
                        Topic::Aura(sibling.id),
                        &sibling.subtitle(),
                        width,
                        data,
                    ) {
                        clicked = Some(topic);
                    }
                }
            }
        }
        // No page to link to — so the section NAMES the mechanic instead of
        // saying there is nothing to name.
        AuraSource::Mechanic(_) => {
            ui.label(
                egui::RichText::new(entry.source.mechanic_line().unwrap_or_default())
                    .size(13.0)
                    .color(MUTED)
                    .italics(),
            );
        }
    }

    // --- Mechanic siblings ---
    let siblings = siblings(&catalog, entry.mechanic, id);
    if !siblings.is_empty() {
        widget::section_heading(
            ui,
            &format!(
                "OTHER {} EFFECTS",
                entry.mechanic.display_name().to_uppercase()
            ),
        );
        ui.label(
            egui::RichText::new(entry.mechanic.description())
                .size(12.5)
                .color(MUTED),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for sibling in siblings {
                if let Some(topic) = widget::icon_link(ui, Topic::Aura(sibling.id), data) {
                    clicked = Some(topic);
                }
            }
        });
    }

    clicked
}

/// The tooltip shown wherever a named aura's icon is hovered.
pub fn render_tooltip(ui: &mut egui::Ui, id: AuraId, data: &EncyclopediaData) {
    let Some(entry) = entry_of(id, data.abilities, data.items) else {
        ui.label(egui::RichText::new("Unknown aura").size(14.0).color(MUTED));
        return;
    };
    let entry = &entry;
    ui.label(
        egui::RichText::new(&entry.name)
            .size(14.0)
            .color(if entry.is_buff() { BUFF } else { DEBUFF })
            .strong(),
    );
    ui.label(
        egui::RichText::new(entry.subtitle())
            .size(12.0)
            .color(MUTED),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(&entry.description)
            .size(12.5)
            .color(TEXT),
    );
    for (key, value) in stat_rows(entry) {
        ui.label(
            egui::RichText::new(format!("{}: {}", key, value))
                .size(11.5)
                .color(DIM),
        );
    }
}

/// Key/value rows for the detail page's stat block and the tooltip.
fn stat_rows(entry: &NamedAura) -> Vec<(String, String)> {
    let aura = &entry.sample;
    let mut rows = Vec::new();

    rows.push((
        "Duration".to_string(),
        match entry.persistence {
            Persistence::Seconds(secs) => format!("{:.0} sec", secs),
            Persistence::WhileSourceActive(note) => format!("Refreshed {}", note),
            Persistence::WholeMatch => "The whole match".to_string(),
        },
    ));

    if let Some((label, mut value)) = magnitude_row(entry) {
        // The sample is built at zero spell power, so a scaled aura's magnitude
        // is its BASE. Power Word: Shield reads 25 here and soaks roughly three
        // times that in play — a page that presents itself as authoritative has
        // to say so. Derived from the RON coefficient, so ability N+1 that
        // gains one is covered without another edit here.
        if entry.magnitude_coefficient != 0.0 {
            value = format!(
                "{}, +{} per point of spell power",
                value, entry.magnitude_coefficient
            );
        }
        rows.push((label, value));
    }

    // A compound debuff's riders are effects of the SAME debuff, so their
    // numbers belong in the same stat block, right under the face's. The page
    // is the only place a player can learn that one dispel takes both.
    for rider in &entry.riders {
        if let Some(rider_row) = magnitude_row_for(rider.effect_type, rider.magnitude) {
            rows.push(rider_row);
        }
    }

    if aura.tick_interval > 0.0 {
        rows.push((
            "Ticks every".to_string(),
            format!("{:.0} sec", aura.tick_interval),
        ));
    }

    rows.push((
        "Breaks on damage".to_string(),
        if aura.break_on_damage_threshold < 0.0 {
            "Never".to_string()
        } else if aura.break_on_damage_threshold == 0.0 {
            "Any damage".to_string()
        } else {
            format!("After {:.0} damage", aura.break_on_damage_threshold)
        },
    ));

    // `spell_school` is the DAMAGE school and is `None` for a physical aura,
    // so the row would otherwise go missing on exactly the auras whose school
    // is the reason they cannot be dispelled. The removal class knows.
    if let Some(school) = aura.spell_school {
        rows.push(("School".to_string(), format!("{:?}", school)));
    } else if aura.is_physical() {
        rows.push(("School".to_string(), "Physical".to_string()));
    }

    // The removal class is a SEPARATE fact from the school, and the page has to
    // show both or it cannot be read correctly: Corruption and Curse of Agony
    // are both Shadow, one is Magic and one is a Curse, and a dispel takes only
    // the first. Debuffs only — on a buff the badge already says whether a
    // purge reaches it, and "Magic" on Power Word: Fortitude answers a question
    // nobody asked. `None` (a schoolless undeclared aura) prints no row rather
    // than a guess; see `Aura::removal_class_name`.
    if aura.is_hostile_effect() {
        if let Some(class) = aura.removal_class_name() {
            rows.push(("Removal class".to_string(), class.to_string()));
        }
    }

    rows
}

/// The magnitude row, worded for the mechanic it belongs to. `None` where the
/// magnitude carries no player-visible meaning (the CC types set it to 1.0 by
/// convention and never read it).
///
/// Two magnitude CONVENTIONS live in `AuraEffect` and the wording has to
/// respect both: some mechanics store a remaining-fraction MULTIPLIER (a
/// movement slow of `0.7` leaves you at 70% speed), others store the amount
/// TAKEN (an attack-speed slow of `0.25` costs you 25%). The percentages below
/// match `build_aura_description`'s sentence for the same aura — a row that
/// said "70%" beside prose that said "30%" would read as a bug.
fn magnitude_row(entry: &NamedAura) -> Option<(String, String)> {
    magnitude_row_for(entry.mechanic, entry.sample.magnitude)
}

/// [`magnitude_row`] for one (mechanic, magnitude) pair, so a compound
/// debuff's RIDERS get the same worded row as its face rather than a second
/// copy of the wording rules.
fn magnitude_row_for(mechanic: AuraType, m: f32) -> Option<(String, String)> {
    let pct = |v: f32| format!("{:.0}%", v * 100.0);
    let row = |label: &str, value: String| Some((label.to_string(), value));
    match mechanic {
        // Remaining-fraction multipliers.
        AuraType::MovementSpeedSlow => row("Movement slowed by", pct(1.0 - m)),
        AuraType::HealingReduction => row("Healing reduced by", pct(1.0 - m)),
        // Amount-taken fractions.
        AuraType::AttackSpeedSlow => row("Attack speed slowed by", pct(m)),
        AuraType::DamageReduction => row("Physical damage reduced by", pct(m)),
        AuraType::CastTimeIncrease => row("Cast time increased by", pct(m)),
        AuraType::DamageTakenReduction => row("Damage taken reduced by", pct(m)),
        AuraType::CritChanceIncrease => row("Critical strike", format!("+{}", pct(m))),
        AuraType::WindfuryBuff => row("Extra-attack chance", pct(m)),
        AuraType::DamageOverTime => row("Damage per tick", format!("{:.0}", m)),
        AuraType::HealingOverTime => row("Healing per tick", format!("{:.0}", m)),
        AuraType::Absorb => row("Absorbs", format!("{:.0} damage", m)),
        AuraType::MaxHealthIncrease => row("Maximum health", format!("+{:.0}", m)),
        AuraType::MaxManaIncrease => row("Maximum mana", format!("+{:.0}", m)),
        AuraType::AttackPowerIncrease => row("Attack power", format!("+{:.0}", m)),
        AuraType::AttackPowerReduction => row("Attack power", format!("-{:.0}", m)),
        AuraType::SpellPowerIncrease => row("Spell power", format!("+{:.0}", m)),
        AuraType::ManaRegenIncrease => row("Mana regeneration", format!("+{:.0}/sec", m)),
        AuraType::SpellResistanceBuff => row("Resistance", format!("+{:.0}", m)),
        AuraType::LockoutDurationReduction => row("Lockout shortened by", pct(m)),
        // Magnitude unused by convention for the rest (CC, immunities, markers,
        // lockouts) — printing "1" would be noise dressed up as a stat.
        _ => None,
    }
}

fn dr_category_name(category: DRCategory) -> &'static str {
    match category {
        DRCategory::Stuns => "Stuns",
        DRCategory::Fears => "Fears",
        DRCategory::Incapacitates => "Incapacitates",
        DRCategory::Roots => "Roots",
        DRCategory::Slows => "Slows",
        DRCategory::Silence => "Silences",
        DRCategory::KidneyShotStun => "Kidney Shot (its own bucket)",
        DRCategory::Horror => "Horror (separate from Fear)",
    }
}

/// The DR ladder, stated from the constants that drive it rather than retyped.
fn dr_rules_text() -> String {
    let pct = |v: f32| format!("{:.0}%", v * 100.0);
    format!(
        "Repeated control from this category lands at {} duration, then {}, then {} — the fourth \
         in a row is resisted outright. The ladder resets {:.0} sec after the last application. \
         Each category diminishes on its own.",
        pct(DR_MULTIPLIERS[0]),
        pct(DR_MULTIPLIERS[1]),
        pct(DR_MULTIPLIERS[2]),
        DR_RESET_TIMER,
    )
}

/// A small outlined pill. Returns the response so callers can hang a tooltip.
fn badge(ui: &mut egui::Ui, label: &str, color: egui::Color32) -> egui::Response {
    let font = egui::FontId::proportional(11.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 16.0, 21.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0_f32, color),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::ability_config::load_ability_definitions;
    use crate::states::play_match::rendering::{item_aura_icons, GENERIC_AURA_ICONS};

    fn abilities() -> AbilityDefinitions {
        load_ability_definitions().expect("abilities.ron must load")
    }

    fn items() -> ItemDefinitions {
        crate::states::play_match::equipment::load_item_definitions().expect("items.ron must load")
    }

    /// Every shipped proc trinket's display name — which is also the name of
    /// the buff it grants. DERIVED from `items.ron` rather than listed, so the
    /// content pass can add trinket N+1 without editing a guard; the guards
    /// below still NAME every non-proc member, so an ordinary entry losing its
    /// source or its art is caught exactly as before.
    fn proc_trinket_names() -> Vec<String> {
        let items = items();
        ItemId::all()
            .iter()
            .filter_map(|id| items.get(id))
            .filter(|item| item.proc.is_some())
            .map(|item| item.name.clone())
            .collect()
    }

    /// A CARDINALITY check, and only that: the ron half of the catalog is the
    /// same SIZE as the set of abilities carrying an `applies_aura` block, and
    /// the catalog is the two sources concatenated. It never asks WHICH
    /// ability an entry addresses, so an entry that resolves under the wrong
    /// [`AuraId`] — the drift a reader meets as a dead link on an ability
    /// page — leaves it green. That direction is
    /// `every_ability_page_link_resolves_to_its_own_aura_entry` below.
    #[test]
    fn every_ability_with_an_applies_aura_becomes_an_entry() {
        let abilities = abilities();
        let expected = abilities
            .iter()
            .filter(|(_, def)| def.applies_aura.is_some())
            .count();
        let entries = catalog(&abilities, &items());
        let from_ron = entries
            .iter()
            .filter(|e| matches!(e.id, AuraId::Ability(_)))
            .count();
        assert_eq!(
            from_ron, expected,
            "the catalog must derive one entry per `applies_aura` block — no hand-authored list"
        );
        assert_eq!(
            entries.len(),
            from_ron + EngineAura::all(&abilities, &items()).len(),
            "every engine-registry aura must also produce an entry"
        );
    }

    /// The ability page's FORWARD ADDRESS into this catalog.
    ///
    /// `encyclopedia::abilities` builds `Topic::Aura(AuraId::Ability(ability))`
    /// for every ability whose config carries an `applies_aura` block. It does
    /// so in a module that shares no code with [`entry_of`], which is what
    /// decides whether that address resolves and what it resolves TO. The two
    /// agree today because `ron_entry` returns an entry keyed on the same
    /// literal, but nothing holds them together — and the symptom of them
    /// drifting is a DEAD or MISADDRESSED link, which no count over the
    /// catalog can see.
    #[test]
    fn every_ability_page_link_resolves_to_its_own_aura_entry() {
        let abilities = abilities();
        let mut checked = 0;
        for (ability, def) in abilities.iter() {
            if def.applies_aura.is_none() {
                continue;
            }
            checked += 1;
            let entry =
                entry_of(AuraId::Ability(*ability), &abilities, &items()).unwrap_or_else(|| {
                    panic!(
                        "{:?}'s ability page links to Topic::Aura(AuraId::Ability({:?})), \
                     which resolves to nothing — a dead link",
                        ability, ability
                    )
                });
            assert_eq!(
                entry.id,
                AuraId::Ability(*ability),
                "{:?}'s aura entry answers to a different address",
                ability
            );
            assert_eq!(
                entry.source,
                AuraSource::Ability(*ability),
                "{:?}'s aura entry links back to the wrong ability",
                ability
            );
        }
        assert!(
            checked > 0,
            "no ability carries an `applies_aura` block — the walk went vacuous"
        );
    }

    /// The same address check for the other half of the catalog. [`entry_of`]
    /// is infallible for an engine aura today; this pins that every registered
    /// [`EngineAura`] stays reachable if it ever stops being.
    #[test]
    fn every_engine_aura_address_resolves() {
        let abilities = abilities();
        let engine = EngineAura::all(&abilities, &items());
        assert!(!engine.is_empty(), "the engine registry went empty");
        for aura in engine {
            let entry = entry_of(AuraId::Engine(aura), &abilities, &items())
                .unwrap_or_else(|| panic!("{:?} has no catalog entry — a dead link", aura));
            assert_eq!(
                entry.id,
                AuraId::Engine(aura),
                "{:?}'s entry answers to a different address",
                aura
            );
        }
    }

    #[test]
    fn the_catalog_is_named_auras_not_aura_types() {
        // The finding this card exists for: one AuraType, many named entries.
        let entries = catalog(&abilities(), &items());
        let dots: Vec<&str> = entries
            .iter()
            .filter(|e| e.mechanic == AuraType::DamageOverTime)
            .map(|e| e.name.as_str())
            .collect();
        assert!(
            dots.contains(&"Rend") && dots.contains(&"Corruption"),
            "Rend and Corruption must be SEPARATE entries, got {:?}",
            dots
        );
    }

    #[test]
    fn ordering_is_deterministic_and_names_are_unique() {
        let abilities = abilities();
        let a: Vec<String> = catalog(&abilities, &items())
            .into_iter()
            .map(|e| e.name)
            .collect();
        let b: Vec<String> = catalog(&abilities, &items())
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(
            a, b,
            "a HashMap-backed source must be sorted into a stable order"
        );
        assert!(a.windows(2).all(|w| w[0] <= w[1]), "entries sort by name");

        // Two entries sharing a name would be indistinguishable in the index.
        let mut unique = a.clone();
        unique.dedup();
        assert_eq!(unique.len(), a.len(), "duplicate entry names: {:?}", a);
    }

    #[test]
    fn every_entry_renders_a_complete_page() {
        for entry in catalog(&abilities(), &items()) {
            assert!(!entry.name.is_empty(), "{:?} has no name", entry.id);
            assert!(
                !entry.description.is_empty(),
                "{} has no effect sentence",
                entry.name
            );
            assert!(!entry.subtitle().is_empty());
            for (key, value) in stat_rows(&entry) {
                assert!(
                    !key.is_empty() && !value.is_empty(),
                    "{} has a blank stat row",
                    entry.name
                );
            }
        }
    }

    /// Classification is resolved per-AURA, not per-type — the distinction the
    /// card's amendment turns on. Rend and Corruption share `DamageOverTime`
    /// but a dispel can only take Corruption.
    #[test]
    fn dispel_classification_is_per_aura_not_per_mechanic() {
        let entries = catalog(&abilities(), &items());
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{} missing", name))
        };
        assert!(
            !by_name("Rend").sample.can_be_dispelled(),
            "Rend is physical"
        );
        assert!(
            by_name("Corruption").sample.can_be_dispelled(),
            "Corruption is Shadow"
        );
        assert!(
            by_name("Crippling Poison").sample.is_cleansable_poison(),
            "a poison is cleansed, not dispelled"
        );
    }

    /// The same holds for the SLOW mechanic, which is where the two rules used
    /// to disagree: Frostbolt's chill is frost magic and comes off, Concussive
    /// Shot's is an arrow to the leg and does not. Both are
    /// `AuraType::MovementSpeedSlow`, so a type-level answer cannot be right
    /// for both.
    #[test]
    fn a_physical_slow_is_not_dispellable_but_a_frost_one_is() {
        let entries = catalog(&abilities(), &items());
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{} missing", name))
        };
        let concussive = by_name("Concussive Shot");
        let frostbolt = by_name("Frostbolt");
        assert_eq!(concussive.mechanic, frostbolt.mechanic, "same mechanic");
        assert!(frostbolt.sample.can_be_dispelled(), "a frost slow is magic");
        assert!(
            !concussive.sample.can_be_dispelled(),
            "an arrow is not magic"
        );
        assert!(concussive.sample.is_physical());
    }

    /// The page says what the engine does. A physical debuff describes its
    /// CLASS — and the wording has to be the SAME for the physical slow and the
    /// physical DoT, which is the asymmetry the card was opened on.
    #[test]
    fn a_physical_debuff_reads_as_physical_on_both_the_badge_and_the_stat_block() {
        let entries = catalog(&abilities(), &items());
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{} missing", name))
        };
        for name in ["Concussive Shot", "Rend"] {
            let entry = by_name(name);
            let (badge, tooltip) = entry.removal();
            assert_eq!(badge, "Immune to dispel", "{name} badge");
            assert!(
                tooltip.starts_with("Physical"),
                "{name} names its class first: {tooltip}"
            );
            assert!(
                stat_rows(entry)
                    .iter()
                    .any(|(k, v)| k == "Removal class" && v == "Physical"),
                "{name} should say it is physical on the page, not only in the tooltip"
            );
        }
    }

    /// A curse is a removal CLASS with no ability serving it, not an
    /// unremovable debuff. Curse of Agony is the one that proves the class has
    /// to be declared rather than derived: it is a Shadow DoT, exactly like
    /// Corruption, and the school alone would call it dispellable magic.
    #[test]
    fn a_curse_reads_as_a_curse_not_as_unremovable() {
        let entries = catalog(&abilities(), &items());
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{} missing", name))
        };
        for name in ["Curse of Agony", "Curse of Weakness", "Curse of Tongues"] {
            let entry = by_name(name);
            let (badge, tooltip) = entry.removal();
            assert_eq!(badge, "Curse", "{name} badge");
            assert!(
                tooltip.contains("none in the arena yet"),
                "{name}: {tooltip}"
            );
            assert!(!entry.sample.can_be_dispelled(), "{name} is not magic");
            assert!(
                stat_rows(entry)
                    .iter()
                    .any(|(k, v)| k == "Removal class" && v == "Curse"),
                "{name} stat block"
            );
        }

        // Same school, different removal class — the whole reason the stat
        // block carries both lines.
        let agony = by_name("Curse of Agony");
        let corruption = by_name("Corruption");
        assert_eq!(
            agony.sample.spell_school, corruption.sample.spell_school,
            "both Shadow"
        );
        assert!(
            corruption.sample.can_be_dispelled(),
            "Corruption is ordinary Shadow magic"
        );
        assert_eq!(corruption.removal().0, "Dispellable");
    }

    /// No player-facing removal wording names an ability. The engine's
    /// predicates decide who performs a removal; the page describes the class,
    /// so a decurse or a second physical-removal effect does not send anyone
    /// hunting through tooltips.
    #[test]
    fn removal_wording_describes_the_class_and_names_no_ability() {
        const NAMES: [&str; 9] = [
            "Divine Shield",
            "Dispel Magic",
            "Cleanse",
            "Devour Magic",
            "Purge",
            "Remove Curse",
            "Master's Call",
            "Paladin",
            "Priest",
        ];
        // The badges are a closed set of CATEGORY words, so "Purgeable" is
        // allowed to contain "Purge" — pinning the set is the check that no
        // ability name reaches a badge.
        const BADGES: [&str; 6] = [
            "Dispellable",
            "Cleansable",
            "Curse",
            "Purgeable",
            "Immune to dispel",
            "Cannot be removed",
        ];
        for entry in catalog(&abilities(), &items()) {
            let (badge, tooltip) = entry.removal();
            assert!(
                BADGES.contains(&badge),
                "{} has an unknown badge {badge}",
                entry.name
            );
            for name in NAMES {
                assert!(
                    !tooltip.contains(name),
                    "{}'s removal tooltip names {name}: {tooltip}",
                    entry.name
                );
            }
        }
    }

    /// Every entry the page tells a player no dispel reaches. Two kinds live
    /// here and the split is the card's whole subject: the PHYSICAL debuffs
    /// (arrow, wound, boot) and the mechanics no dispel takes whatever their
    /// school (stuns, interrupt lockouts). Pinned because this rung is where a
    /// widening of `is_magic_dispellable` would silently show up — moving a
    /// name out of this list is a balance change, not a wording change.
    ///
    /// "Frost Armor (attack speed)" used to sit here, beside a dispellable
    /// "Frost Armor (movement slow)" — one debuff on two rungs. AS-54 took it
    /// off this list by taking it out of the CATALOG: the attack-speed effect
    /// is now a rider on the chill, which is dispellable by its face. Note
    /// what did NOT change to achieve that — `is_magic_dispellable` still
    /// refuses `AttackSpeedSlow`, so the five schoolless `[—]` rows below are
    /// exactly as they were.
    #[test]
    fn the_immune_to_dispel_rung_is_pinned() {
        let mut rows: Vec<String> = catalog(&abilities(), &items())
            .iter()
            .filter(|e| e.removal().0 == "Immune to dispel")
            .map(|e| {
                format!(
                    "{} [{}]",
                    e.name,
                    e.sample.removal_class_name().unwrap_or("—")
                )
            })
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                "Aimed Shot [Physical]",
                "Boar Charge [Physical]",
                "Cheap Shot [Physical]",
                "Concussive Shot [Physical]",
                "Demoralizing Shout [—]",
                "Hammer of Justice [Magic]",
                "Kick [—]",
                "Kidney Shot [Physical]",
                "Mortal Strike [Physical]",
                "Pummel [—]",
                "Rend [Physical]",
                "Spell Lock [—]",
                "Wind Shear [—]",
            ]
        );
    }

    /// The absolute rung is for what genuinely nothing touches: the deliberately
    /// unpurgeable self-buffs, every proc trinket's physical buff, and the
    /// mechanical markers. Anything else on it is a page overclaiming.
    #[test]
    fn cannot_be_removed_is_reserved_for_the_untouchable_set() {
        let untouchable: Vec<String> = catalog(&abilities(), &items())
            .into_iter()
            .filter(|e| e.removal().0 == "Cannot be removed")
            .map(|e| e.name.clone())
            .collect();
        let mut sorted = untouchable.clone();
        sorted.sort();
        let mut expected = proc_trinket_names();
        expected.extend(
            [
                "Berserker Rage",
                "Crippling Poison (weapon coating)",
                "Divine Shield",
                "Shadow Sight",
                "Weakened Soul",
            ]
            .map(String::from),
        );
        expected.sort();
        assert_eq!(
            sorted, expected,
            "unexpected entry claiming nothing can remove it"
        );
    }

    #[test]
    fn engine_auras_carry_the_simulations_own_numbers() {
        let entries = catalog(&abilities(), &items());
        let weakened = entries
            .iter()
            .find(|e| e.name == "Weakened Soul")
            .expect("registered");
        assert_eq!(
            weakened.persistence,
            Persistence::Seconds(WEAKENED_SOUL_DURATION)
        );
        assert_eq!(
            weakened.source,
            AuraSource::Ability(AbilityType::PowerWordShield),
            "Power Word: Shield spawns Weakened Soul on the ally it shields, so the page \
             must link to it — `source: None` is what made this page read as though \
             nothing applied it"
        );

        // The totem buffs and the Frost Trap slow DO have an ability to link
        // back to — one with no `applies_aura` of its own.
        let windfury = entries
            .iter()
            .find(|e| e.name == "Windfury Totem")
            .expect("registered");
        assert_eq!(windfury.source, AuraSource::Ability(AbilityType::AirTotem));
        assert!(matches!(
            windfury.persistence,
            Persistence::WhileSourceActive(_)
        ));
        let frost_trap = entries
            .iter()
            .find(|e| e.name == "Frost Trap")
            .expect("registered");
        assert_eq!(frost_trap.sample.magnitude, FROST_TRAP_SLOW_MAGNITUDE);
    }

    /// `combat_core::damage` names the lockout after the interrupt that caused
    /// it, so every interrupt that locks a school is a named debuff players see
    /// — and the entry's duration must be that interrupt's own lockout.
    #[test]
    fn every_interrupt_with_a_lockout_is_a_named_debuff() {
        let abilities = abilities();
        let entries = catalog(&abilities, &items());
        let mut checked = 0;
        for (_, def) in abilities.iter() {
            if !def.is_interrupt || def.lockout_duration <= 0.0 {
                continue;
            }
            let entry = entries
                .iter()
                .find(|e| e.name == def.name && e.mechanic == AuraType::SpellSchoolLockout)
                .unwrap_or_else(|| panic!("{} locks a school but has no catalog entry", def.name));
            assert_eq!(
                entry.persistence,
                Persistence::Seconds(def.lockout_duration)
            );
            assert!(!entry.is_buff(), "a lockout is a debuff");
            checked += 1;
        }
        assert!(
            checked >= 4,
            "expected the four interrupts, checked {}",
            checked
        );
    }

    /// The three reused engine names each address SEVERAL distinct auras, and
    /// the catalog must reach the right one. A name-keyed catalog sent a Rogue
    /// from its own gold-bordered coating marker to the enemy slow's page.
    #[test]
    fn reused_engine_names_get_an_entry_each() {
        let entries = catalog(&abilities(), &items());
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{} missing", name))
        };

        // The Rogue's coating marker: a BUFF, a different mechanic, and a
        // different removal rule from the debuff sharing its frame name.
        let coating = by_name("Crippling Poison (weapon coating)");
        let debuff = by_name("Crippling Poison");
        assert_eq!(
            coating.frame_name, debuff.frame_name,
            "both read the same on the frames"
        );
        assert!(coating.is_buff() && !debuff.is_buff());
        assert_eq!(coating.mechanic, AuraType::WeaponPoison);
        assert_eq!(debuff.mechanic, AuraType::MovementSpeedSlow);
        assert_eq!(coating.persistence, Persistence::WholeMatch);

        // UA's dispel backlash silence vs the damage-over-time it comes from.
        let silence = by_name("Unstable Affliction (dispel backlash)");
        assert_eq!(silence.mechanic, AuraType::Silence);
        assert_eq!(silence.frame_name, "Unstable Affliction");
        assert_eq!(
            silence.persistence,
            Persistence::Seconds(DISPEL_BACKLASH_SILENCE_DURATION)
        );
        assert_eq!(
            by_name("Unstable Affliction").mechanic,
            AuraType::DamageOverTime
        );

        // Frost Armor: the Mage's buff, plus the ONE chill it hangs on melee.
        // The chill is a compound debuff — one entry carrying both effects —
        // so the catalog holds no "(movement slow)" / "(attack speed)" pair.
        let buff = by_name("Frost Armor");
        let chill = by_name("Frost Armor (chill)");
        assert!(buff.is_buff() && !chill.is_buff());
        assert_eq!(chill.frame_name, "Frost Armor");
        let [face, rider] = frost_armor_chill_auras();
        assert_eq!(chill.sample.magnitude, face.magnitude);
        assert_eq!(chill.mechanic, face.effect_type);
        assert_eq!(
            chill
                .riders
                .iter()
                .map(|r| r.effect_type)
                .collect::<Vec<_>>(),
            vec![rider.effect_type],
            "the chill's page must list its attack-speed effect, or the one \
             entry hides half the debuff"
        );
        assert_eq!(chill.riders[0].magnitude, rider.magnitude);
        assert!(
            entries
                .iter()
                .all(|e| !e.name.starts_with("Frost Armor (movement")
                    && !e.name.starts_with("Frost Armor (attack")),
            "the split Frost Armor entries must be gone, not merely hidden"
        );

        // Every disambiguated entry says what the frames call it, or a player
        // matching the buff bar against the catalog finds no such name.
        for entry in entries.iter().filter(|e| e.name != e.frame_name) {
            let provenance = entry.provenance.as_deref().unwrap_or_default();
            assert!(
                provenance.contains(&entry.frame_name),
                "{} never tells the reader the frames say \"{}\"",
                entry.name,
                entry.frame_name
            );
        }
    }

    /// Blocker the module doc turns on: the page states what the simulation
    /// does. Shadow Sight was applied with a 0.0 threshold for a while — it
    /// broke on ANY damage — while a hardcoded -1.0 here printed "Never". The
    /// apply site now applies -1.0 and the page must follow THAT constant, not
    /// a literal of its own.
    #[test]
    // Pinning a relationship between constants IS this test; const-folding is the point.
    #[allow(clippy::assertions_on_constants)]
    fn engine_entries_take_break_on_damage_from_their_apply_site() {
        let entries = catalog(&abilities(), &items());
        let shadow_sight = entries
            .iter()
            .find(|e| e.name == "Shadow Sight")
            .expect("registered");
        assert_eq!(
            shadow_sight.sample.break_on_damage_threshold, SHADOW_SIGHT_BREAK_ON_DAMAGE,
            "the page must read the threshold the orb pickup applies"
        );
        assert!(
            SHADOW_SIGHT_BREAK_ON_DAMAGE < 0.0,
            "Shadow Sight runs its full duration; -1.0 is the never-breaks sentinel"
        );
        let rows = stat_rows(shadow_sight);
        let breaks = rows
            .iter()
            .find(|(key, _)| key == "Breaks on damage")
            .expect("every page states a break-on-damage rule");
        assert_eq!(breaks.1, "Never");
    }

    /// Power Word: Shield's 25 is a BASE that spell power roughly triples in
    /// play. The stat block says so, derived from the RON coefficient.
    #[test]
    fn a_spell_power_scaled_aura_says_it_scales() {
        let entries = catalog(&abilities(), &items());
        let shield = entries
            .iter()
            .find(|e| e.name == "Power Word: Shield")
            .expect("registered");
        assert!(shield.magnitude_coefficient > 0.0);
        let rows = stat_rows(shield);
        let absorb = rows
            .iter()
            .find(|(key, _)| key == "Absorbs")
            .expect("absorb row");
        assert!(
            absorb.1.contains("per point of spell power"),
            "a scaled absorb must not present its base as the whole story: {:?}",
            absorb.1
        );

        // And an UNSCALED aura says nothing of the sort.
        let barrier = entries
            .iter()
            .find(|e| e.name == "Ice Barrier")
            .expect("registered");
        assert_eq!(barrier.magnitude_coefficient, 0.0);
        let rows = stat_rows(barrier);
        let absorb = rows
            .iter()
            .find(|(key, _)| key == "Absorbs")
            .expect("absorb row");
        assert!(!absorb.1.contains("spell power"));
    }

    /// The three mechanics that had `display_name()` and `description()` but no
    /// entry to show them on. A mechanic with no entries is a page a player can
    /// never reach.
    ///
    /// Asked of `mechanics()`, not `mechanic`: `AttackSpeedSlow` reaches a page
    /// as a RIDER on the Frost Armor chill rather than as an entry of its own
    /// (it is half of one debuff, not a debuff). Its stat row is on the chill's
    /// page and its cross-links resolve there, which is the reachability this
    /// test is about — the badge saying "Slow" is a separate question.
    #[test]
    fn every_mechanic_with_a_named_aura_reaches_a_page() {
        let entries = catalog(&abilities(), &items());
        for mechanic in [
            AuraType::Silence,
            AuraType::AttackSpeedSlow,
            AuraType::WeaponPoison,
        ] {
            assert!(
                entries.iter().any(|e| e.mechanics().contains(&mechanic)),
                "{:?} is applied in the engine but has no catalog entry",
                mechanic
            );
        }
    }

    #[test]
    fn siblings_are_the_other_entries_sharing_a_mechanic() {
        let entries = catalog(&abilities(), &items());
        let rend = entries.iter().find(|e| e.name == "Rend").expect("Rend");
        let sibs = siblings(&entries, rend.mechanic, rend.id);
        assert!(
            sibs.iter().all(|s| s.id != rend.id),
            "an entry is not its own sibling"
        );
        assert!(
            sibs.iter().any(|s| s.name == "Corruption"),
            "Rend must cross-link to the other damage-over-time effects"
        );
    }

    /// The page a player would actually open to ask what Frost Armor does —
    /// the Mage's own buff — reaches the procs it hangs on melee attackers.
    ///
    /// That link was one-directional: each proc named Frost Armor as its
    /// source, and the buff named nothing. Pinned on the buff SPECIFICALLY,
    /// because the proc pages were never the broken direction.
    #[test]
    fn a_buff_reaches_the_other_auras_its_ability_applies() {
        let entries = catalog(&abilities(), &items());
        let buff = entries
            .iter()
            .find(|e| e.id == AuraId::Ability(AbilityType::FrostArmor))
            .expect("Frost Armor's self-buff is a RON entry");
        let kin = source_siblings(&entries, buff.source, buff.id);

        assert!(
            !kin.is_empty(),
            "the Frost Armor buff page must reach the proc(s) the same ability applies"
        );
        for other in &kin {
            assert_eq!(
                other.source, buff.source,
                "a source sibling shares the applying ability"
            );
            assert!(!other.is_buff(), "Frost Armor's procs are debuffs");
        }
    }

    /// The relation is symmetric for EVERY entry, not just the one above: if A
    /// lists B, B lists A. A one-directional cross-link is the defect this
    /// helper exists to close, so it must not be able to reintroduce one.
    #[test]
    fn source_siblings_are_symmetric_and_exclude_self() {
        let entries = catalog(&abilities(), &items());
        let mut paired = 0usize;
        for entry in &entries {
            for other in source_siblings(&entries, entry.source, entry.id) {
                assert_ne!(entry.id, other.id, "an entry is not its own sibling");
                assert!(
                    source_siblings(&entries, other.source, other.id)
                        .iter()
                        .any(|back| back.id == entry.id),
                    "{} lists {} but {} does not list it back",
                    entry.name,
                    other.name,
                    other.name
                );
                paired += 1;
            }
        }
        // Non-vacuity: a catalog where no ability hangs two auras would pass
        // the loop above without testing anything. Frost Armor, Unstable
        // Affliction and the Rogue's poisons are all this shape today.
        assert!(
            paired >= 6,
            "only {} source-sibling pairing(s) in the catalog — too few for this to be \
             testing anything",
            paired
        );
    }

    #[test]
    fn buffs_and_debuffs_both_have_entries() {
        let entries = catalog(&abilities(), &items());
        assert!(entries.iter().any(|e| e.is_buff()));
        assert!(entries.iter().any(|e| !e.is_buff()));
    }

    /// The PROVENANCE AUDIT: every page says what applies it, and the entries
    /// that say "nothing does" are a closed, deliberate set.
    ///
    /// The failure this replaces was silent — Weakened Soul's `source: None`
    /// looked exactly like Shadow Sight's, so the one entry with a real
    /// applying ability and the one with none printed the same sentence. The
    /// type now forces a mechanic to be NAMED; this pins that nobody names a
    /// blank one, and that the list of ability-less auras is short enough to
    /// read.
    #[test]
    fn every_entry_names_what_applies_it() {
        let abilities = abilities();
        let mut ability_less: Vec<String> = Vec::new();
        for entry in catalog(&abilities, &items()) {
            match entry.source {
                AuraSource::Ability(ability) => {
                    assert!(
                        abilities.get(&ability).is_some(),
                        "{} points APPLIED BY at {:?}, which has no ability definition — \
                         a dead link",
                        entry.name,
                        ability
                    );
                    assert!(
                        entry.source.mechanic_line().is_none(),
                        "{} has an ability page to link to and must not also narrate one",
                        entry.name
                    );
                }
                AuraSource::Mechanic(mechanic) => {
                    assert!(
                        !mechanic.trim().is_empty(),
                        "{} says no ability applies it without saying what does",
                        entry.name
                    );
                    let line = entry
                        .source
                        .mechanic_line()
                        .expect("a mechanic source always has a line");
                    assert!(
                        line.contains(mechanic),
                        "{}'s APPLIED BY line never names the mechanic: {:?}",
                        entry.name,
                        line
                    );
                    ability_less.push(entry.name.clone());
                }
            }
        }
        ability_less.sort();
        let mut expected = proc_trinket_names();
        expected.push("Shadow Sight".to_string());
        expected.sort();
        assert_eq!(
            ability_less, expected,
            "an arena orb pickup and the proc trinkets are the only auras no ability applies \
             — a new one here is either a genuine mechanic or a `source` nobody filled in"
        );
    }

    /// The other direction of the same field: an ability can ask which auras it
    /// applies. Power Word: Shield is the case that needs it — it applies its
    /// own absorb AND the Weakened Soul marker, so an ability page built from
    /// `applies_aura` alone would show half the cast.
    #[test]
    fn an_ability_can_find_every_aura_it_applies() {
        let abilities = abilities();
        let catalog = catalog(&abilities, &items());

        let mut shield: Vec<&str> = applied_by(&catalog, AbilityType::PowerWordShield)
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        shield.sort_unstable();
        assert_eq!(
            shield,
            vec!["Power Word: Shield", "Weakened Soul"],
            "one cast, two auras — both must be reachable from the ability"
        );

        // The ordinary case still works: an ability with one `applies_aura`
        // block finds exactly its own entry.
        let frostbolt: Vec<&str> = applied_by(&catalog, AbilityType::Frostbolt)
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(frostbolt, vec!["Frostbolt"]);

        // And every ability-sourced entry is reachable from its own ability, so
        // the two directions cannot disagree.
        for entry in &catalog {
            if let Some(ability) = entry.source.ability() {
                assert!(
                    applied_by(&catalog, ability)
                        .iter()
                        .any(|found| found.id == entry.id),
                    "{} names {:?} as its source but the reverse lookup misses it",
                    entry.name,
                    ability
                );
            }
        }
    }

    /// Every entry resolves to an icon key some loader actually registers, and
    /// an entry with art of its own points at a file that exists.
    ///
    /// This is the card's other half. Shadow Sight rendered the placeholder
    /// tile because it had no applying ability and therefore no key at all;
    /// Weakened Soul now HAS an applying ability and must still not wear its
    /// icon, because the shield's art on the debuff that blocks the shield says
    /// the opposite of what is true.
    #[test]
    fn every_entry_resolves_to_an_icon_that_exists() {
        let abilities = abilities();
        let mut own_art: Vec<String> = Vec::new();
        let mut item_art: Vec<String> = Vec::new();
        for entry in catalog(&abilities, &items()) {
            let key = icon_key(entry.id, &abilities, &items())
                .unwrap_or_else(|| panic!("{} has no icon key — a placeholder tile", entry.name));
            match entry.art {
                AuraArt::Own(expected) => {
                    assert_eq!(key, expected);
                    let path = GENERIC_AURA_ICONS
                        .iter()
                        .find(|(registered, _)| *registered == expected)
                        .unwrap_or_else(|| {
                            panic!(
                                "{}'s icon key {:?} is in no loader's table, so nothing ever \
                                 registers a texture for it",
                                entry.name, expected
                            )
                        })
                        .1;
                    let on_disk = std::path::Path::new("assets").join(path);
                    assert!(
                        on_disk.exists(),
                        "{} points at {}, which is not in the asset tree",
                        entry.name,
                        on_disk.display()
                    );
                    own_art.push(entry.name.clone());
                }
                AuraArt::Item(item) => {
                    // The key the buff bar draws a proc under, registered by
                    // `item_aura_icons` — which both loaders read — at the
                    // trinket's own icon file.
                    assert_eq!(key, item_aura_icon_key(item));
                    let path = item_aura_icons(&items())
                        .into_iter()
                        .find(|(registered, _)| *registered == key)
                        .unwrap_or_else(|| {
                            panic!(
                                "{}'s item icon {:?} is registered by no loader",
                                entry.name, key
                            )
                        })
                        .1;
                    assert_eq!(path, items().get(&item).expect("defined").icon);
                    let on_disk = std::path::Path::new("assets").join(&path);
                    assert!(
                        on_disk.exists(),
                        "{} points at {}, which is not in the asset tree",
                        entry.name,
                        on_disk.display()
                    );
                    item_art.push(entry.name.clone());
                }
                AuraArt::FromSource => {
                    let ability = entry
                        .source
                        .ability()
                        .unwrap_or_else(|| panic!("{} borrows art from nothing", entry.name));
                    assert_eq!(key, abilities.get(&ability).expect("defined").name);
                }
            }
        }
        own_art.sort();
        assert_eq!(
            own_art,
            vec!["Shadow Sight".to_string(), "Weakened Soul".to_string()],
            "the auras whose own art beats a borrowed icon"
        );
        item_art.sort();
        let mut expected = proc_trinket_names();
        expected.sort();
        assert_eq!(
            item_art, expected,
            "every proc trinket buff, and nothing else, wears its item's icon"
        );

        // The encyclopedia's loader opens this whole table. A missing file no
        // longer holds the other icons back (`icon_load_settled` counts a
        // failed load as done), but the aura it belongs to still draws a
        // placeholder, so every path must exist.
        for (key, path) in GENERIC_AURA_ICONS {
            let on_disk = std::path::Path::new("assets").join(path);
            assert!(
                on_disk.exists(),
                "generic aura icon {:?} points at {}, which is not in the asset tree",
                key,
                on_disk.display()
            );
        }
    }
}
