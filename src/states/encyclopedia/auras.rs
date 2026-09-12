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
//!    Weakened Soul, Shadow Sight, the Frost Trap zone's slow, the four totem
//!    pulses, and the school lockout every interrupt leaves behind. The last
//!    two EXPAND from their own sources (`TotemElement::ALL`, the interrupt
//!    flags), so they stay zero-marginal-cost too.
//!
//!    `tests/aura_catalog_audit.rs` is what stops that registry rotting: it
//!    scans every `ability_name:` string literal under
//!    `src/states/play_match/` and fails if one resolves to no catalog entry.
//!
//! ## Where the per-entry facts come from
//!
//! Every classification on a page is answered by asking the ENGINE about a
//! representative [`Aura`] built the same way the simulation builds it
//! (`AuraPending::from_ability` for RON entries). So `can_be_dispelled`,
//! `is_cleansable_poison`, `can_be_purged` and `dr_category` are the real
//! predicates, not a second copy of their rules — including the per-aura ones a
//! type-level answer would get wrong (Rend is a PHYSICAL damage-over-time and
//! is not dispellable; Corruption is Shadow and is).

use bevy_egui::egui;

use crate::states::ability_text::build_aura_description;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::{
    Aura, AuraPending, AuraType, DRCategory, DispelType, TotemElement,
};
use crate::states::play_match::constants::{
    DR_MULTIPLIERS, DR_RESET_TIMER, FROST_TRAP_SLOW_MAGNITUDE, FROST_TRAP_ZONE_DURATION,
    TOTEM_DURATION, WEAKENED_SOUL_DURATION,
};
use crate::states::play_match::rendering::is_buff_aura;
use crate::states::play_match::shadow_sight::SHADOW_SIGHT_DURATION;

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
/// ability; only Weakened Soul and Shadow Sight have no ability at all.
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
}

impl EngineAura {
    /// Every engine-originated aura, in display order.
    ///
    /// The two nested variants expand from their own sources, so this list only
    /// has to name the genuinely one-off auras.
    pub fn all(abilities: &AbilityDefinitions) -> Vec<EngineAura> {
        let mut all = vec![
            EngineAura::WeakenedSoul,
            EngineAura::ShadowSight,
            EngineAura::FrostTrapSlow,
        ];
        all.extend(TotemElement::ALL.iter().copied().map(EngineAura::TotemBuff));
        let mut interrupts: Vec<AbilityType> = abilities
            .iter()
            .filter(|(_, def)| def.is_interrupt && def.lockout_duration > 0.0)
            .map(|(ability, _)| *ability)
            .collect();
        interrupts.sort_unstable();
        all.extend(interrupts.into_iter().map(EngineAura::InterruptLockout));
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
}

/// One catalog entry: a named aura, fully resolved.
///
/// Built by [`catalog`]; never stored, so a RON edit is picked up on the next
/// build of the registry.
pub struct NamedAura {
    pub id: AuraId,
    /// The name the player sees on the actor frames.
    pub name: String,
    /// The mechanic badge.
    pub mechanic: AuraType,
    /// The ability that applies it, when there is one to link to.
    pub source: Option<AbilityType>,
    /// A representative aura, built the way the simulation builds it. Every
    /// classification on the page is a question asked of THIS.
    pub sample: Aura,
    pub persistence: Persistence,
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

    /// `Debuff · Damage over Time` — the one-line identity shared by index
    /// rows, search hits and the detail header.
    pub fn subtitle(&self) -> String {
        subtitle_for(self.mechanic)
    }

    /// How this aura comes off early, as a badge label plus the tooltip that
    /// names the abilities. Answered by the ENGINE's own predicates on this
    /// entry's representative aura, so it can never disagree with what a dispel
    /// actually does.
    ///
    /// The two halves are disjoint by construction: a friendly dispel lifts
    /// harmful magic off an ally, while Purge strips buffs off an enemy.
    fn removal(&self) -> (&'static str, &'static str) {
        if self.sample.can_be_dispelled() {
            (
                "Dispellable",
                "A Priest's Dispel Magic, a Paladin's Cleanse or a Felhunter's Devour Magic can \
                 lift this off an ally.",
            )
        } else if self.sample.is_cleansable_poison() {
            (
                "Cleansable",
                "A poison. Dispel Magic cannot touch it — only a Paladin's Cleanse.",
            )
        } else if self.sample.can_be_purged() {
            (
                "Purgeable",
                "A Shaman's Purge can strip this off an enemy.",
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
pub fn catalog(abilities: &AbilityDefinitions) -> Vec<NamedAura> {
    let mut entries: Vec<NamedAura> = abilities
        .iter()
        .filter_map(|(ability, _)| ron_entry(*ability, abilities))
        .collect();

    entries.extend(
        EngineAura::all(abilities)
            .into_iter()
            .map(|engine| engine_entry(engine, abilities)),
    );
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Build the ONE entry an address names, without walking the catalog. Used
/// where only a single aura is on screen (a tooltip); the index and the
/// sibling cross-links still need the whole thing.
pub fn entry_of(id: AuraId, abilities: &AbilityDefinitions) -> Option<NamedAura> {
    match id {
        AuraId::Ability(ability) => ron_entry(ability, abilities),
        AuraId::Engine(engine) => Some(engine_entry(engine, abilities)),
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
        mechanic: effect.aura_type,
        source: Some(ability),
        sample: pending.aura,
        persistence: Persistence::Seconds(effect.duration),
        description: build_aura_description(effect),
        provenance: None,
    })
}

/// `Debuff · Damage over Time` — polarity plus mechanic, the one-line identity
/// of any named aura.
pub fn subtitle_for(mechanic: AuraType) -> String {
    format!(
        "{} · {}",
        if is_buff_aura(&mechanic) { "Buff" } else { "Debuff" },
        mechanic.display_name()
    )
}

/// The named auras sharing `mechanic`, excluding `self_id` — the cross-links.
pub fn siblings(catalog: &[NamedAura], mechanic: AuraType, self_id: AuraId) -> Vec<&NamedAura> {
    catalog
        .iter()
        .filter(|entry| entry.mechanic == mechanic && entry.id != self_id)
        .collect()
}

pub fn find<'a>(catalog: &'a [NamedAura], id: AuraId) -> Option<&'a NamedAura> {
    catalog.iter().find(|entry| entry.id == id)
}

// ============================================================================
// ENGINE REGISTRY
// ============================================================================

/// Resolve one engine-originated aura into a catalog entry.
///
/// Every value here is read from the constant or spec the apply site reads, so
/// the page cannot state a number the simulation does not use.
fn engine_entry(engine: EngineAura, abilities: &AbilityDefinitions) -> NamedAura {
    let (name, mechanic, source, magnitude, school, persistence, provenance) = match engine {
        EngineAura::WeakenedSoul => (
            "Weakened Soul".to_string(),
            AuraType::WeakenedSoul,
            None,
            0.0,
            None,
            Persistence::Seconds(WEAKENED_SOUL_DURATION),
            "Placed on the ally a Priest shields, alongside Power Word: Shield.".to_string(),
        ),
        EngineAura::ShadowSight => (
            "Shadow Sight".to_string(),
            AuraType::ShadowSight,
            None,
            1.0,
            None,
            Persistence::Seconds(SHADOW_SIGHT_DURATION),
            "Granted by picking up a Shadow Sight orb in the arena.".to_string(),
        ),
        EngineAura::FrostTrapSlow => (
            "Frost Trap".to_string(),
            AuraType::MovementSpeedSlow,
            Some(AbilityType::FrostTrap),
            FROST_TRAP_SLOW_MAGNITUDE,
            Some(SpellSchool::Frost),
            Persistence::WhileSourceActive("while you stand in the zone"),
            format!(
                "Re-applied every tick by a triggered Frost Trap's slow zone, which itself \
                 lasts {:.0} sec.",
                FROST_TRAP_ZONE_DURATION
            ),
        ),
        EngineAura::TotemBuff(element) => {
            let (ability, aura_type, magnitude, school) =
                crate::states::play_match::class_ai::shaman::totem_spec(element);
            (
                element.buff_name().to_string(),
                aura_type,
                Some(ability),
                magnitude,
                Some(school),
                Persistence::WhileSourceActive("while you stand near the totem"),
                format!(
                    "Pulsed onto nearby allies by a dropped totem, which itself lasts {:.0} sec.",
                    TOTEM_DURATION
                ),
            )
        }
        EngineAura::InterruptLockout(ability) => {
            let def = abilities.get(&ability);
            (
                def.map(|d| d.name.clone()).unwrap_or_else(|| format!("{:?}", ability)),
                AuraType::SpellSchoolLockout,
                Some(ability),
                // The magnitude encodes WHICH school was locked and depends on
                // what the interrupt caught, so it is not a fact about this
                // aura; `magnitude_row` prints nothing for this mechanic.
                0.0,
                None,
                Persistence::Seconds(def.map(|d| d.lockout_duration).unwrap_or_default()),
                "Left behind when this interrupt lands. Only the school of the interrupted \
                 spell is locked — the target's other schools keep working."
                    .to_string(),
            )
        }
    };

    let tick_interval = if mechanic == AuraType::HealingOverTime { 1.0 } else { 0.0 };
    let duration = match persistence {
        Persistence::Seconds(secs) => secs,
        // The refresh window itself; the page prints the persistence note
        // instead, but the sample must still be a realistic aura.
        Persistence::WhileSourceActive(_) => 2.0,
    };
    let sample = Aura {
        effect_type: mechanic,
        duration,
        magnitude,
        break_on_damage_threshold: -1.0,
        tick_interval,
        spell_school: school,
        dispel_type: DispelType::Auto,
        ..Default::default()
    };

    // Engine auras have no `applies_aura` block to generate prose from, so the
    // MECHANIC's own player-facing sentence carries the page.
    let description = mechanic.description().to_string();

    NamedAura {
        id: AuraId::Engine(engine),
        name,
        mechanic,
        source,
        sample,
        persistence,
        description,
        provenance: Some(provenance),
    }
}

// ============================================================================
// SEARCH
// ============================================================================

/// Contribute every named aura to the search registry.
pub fn search_entries(abilities: &AbilityDefinitions, out: &mut Vec<SearchEntry>) {
    for entry in catalog(abilities) {
        out.push(SearchEntry::new(
            Topic::Aura(entry.id),
            entry.name.clone(),
            entry.subtitle(),
        ));
    }
}

/// Name, mechanic and applying ability of ONE address, resolved without
/// building the whole catalog.
///
/// The linked-icon widget asks a `Topic` for its name and icon once per widget
/// it draws, and the index draws fifty of them, so this path has to stay cheap.
/// Resolving one engine aura through [`engine_entry`] is fine — that builds a
/// single entry, not the catalog.
fn identity(
    id: AuraId,
    abilities: &AbilityDefinitions,
) -> Option<(String, AuraType, Option<AbilityType>)> {
    match id {
        AuraId::Ability(ability) => {
            let def = abilities.get(&ability)?;
            let effect = def.applies_aura.as_ref()?;
            Some((def.name.clone(), effect.aura_type, Some(ability)))
        }
        AuraId::Engine(engine) => {
            let entry = engine_entry(engine, abilities);
            Some((entry.name, entry.mechanic, entry.source))
        }
    }
}

/// The display name of one aura address, for [`Topic::name`].
pub fn name_of(id: AuraId, abilities: &AbilityDefinitions) -> Option<String> {
    identity(id, abilities).map(|(name, _, _)| name)
}

/// The `Buff · Mechanic` subtitle of one aura address, for [`Topic::subtitle`].
pub fn subtitle_of(id: AuraId, abilities: &AbilityDefinitions) -> Option<String> {
    identity(id, abilities).map(|(_, mechanic, _)| subtitle_for(mechanic))
}

/// The icon key of one aura address: the applying ability's name, which is how
/// `AbilityIcons` is keyed. `None` for the two entries with no applying ability
/// at all, which render the widget's placeholder tile.
pub fn icon_key(id: AuraId, abilities: &AbilityDefinitions) -> Option<String> {
    let source = identity(id, abilities)?.2?;
    abilities.get(&source).map(|def| def.name.clone())
}

// ============================================================================
// INDEX PAGE
// ============================================================================

/// The buff/debuff index. Returns the aura whose row was clicked.
pub fn render_index(ui: &mut egui::Ui, data: &EncyclopediaData) -> Option<Topic> {
    let catalog = catalog(data.abilities);

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
            let group: Vec<&NamedAura> =
                catalog.iter().filter(|e| e.is_buff() == want_buff).collect();
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
    let catalog = catalog(data.abilities);
    let Some(entry) = find(&catalog, id) else {
        ui.label(egui::RichText::new("Unknown aura").size(16.0).color(MUTED));
        return None;
    };

    widget::detail_header(ui, Topic::Aura(id), "", data);
    ui.add_space(8.0);

    // --- Badges: polarity, mechanic, how it comes off ---
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let (polarity, color) =
            if entry.is_buff() { ("Buff", BUFF) } else { ("Debuff", DEBUFF) };
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
    ui.label(egui::RichText::new(&entry.description).size(14.0).color(TEXT));
    if let Some(provenance) = &entry.provenance {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(provenance).size(12.0).color(DIM).italics());
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

    // --- Applied by ---
    widget::section_heading(ui, "APPLIED BY");
    match entry.source {
        Some(ability) => {
            let width = ui.available_width().min(430.0);
            if let Some(topic) = widget::row(ui, Topic::Ability(ability), "", width, data) {
                clicked = Some(topic);
            }
        }
        None => {
            ui.label(
                egui::RichText::new(
                    "Applied by an engine mechanic — it has no entry in the ability list.",
                )
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
            &format!("OTHER {} EFFECTS", entry.mechanic.display_name().to_uppercase()),
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
    let Some(entry) = entry_of(id, data.abilities) else {
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
    ui.label(egui::RichText::new(entry.subtitle()).size(12.0).color(MUTED));
    ui.add_space(4.0);
    ui.label(egui::RichText::new(&entry.description).size(12.5).color(TEXT));
    for (key, value) in stat_rows(entry) {
        ui.label(egui::RichText::new(format!("{}: {}", key, value)).size(11.5).color(DIM));
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
        },
    ));

    if let Some(value) = magnitude_row(entry) {
        rows.push(value);
    }

    if aura.tick_interval > 0.0 {
        rows.push(("Ticks every".to_string(), format!("{:.0} sec", aura.tick_interval)));
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

    if let Some(school) = aura.spell_school {
        rows.push(("School".to_string(), format!("{:?}", school)));
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
    let m = entry.sample.magnitude;
    let pct = |v: f32| format!("{:.0}%", v * 100.0);
    let row = |label: &str, value: String| Some((label.to_string(), value));
    match entry.mechanic {
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
    painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
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

    fn abilities() -> AbilityDefinitions {
        load_ability_definitions().expect("abilities.ron must load")
    }

    #[test]
    fn every_ability_with_an_applies_aura_becomes_an_entry() {
        let abilities = abilities();
        let expected = abilities.iter().filter(|(_, def)| def.applies_aura.is_some()).count();
        let entries = catalog(&abilities);
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
            from_ron + EngineAura::all(&abilities).len(),
            "every engine-registry aura must also produce an entry"
        );
    }

    #[test]
    fn the_catalog_is_named_auras_not_aura_types() {
        // The finding this card exists for: one AuraType, many named entries.
        let entries = catalog(&abilities());
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
        let a: Vec<String> = catalog(&abilities).into_iter().map(|e| e.name).collect();
        let b: Vec<String> = catalog(&abilities).into_iter().map(|e| e.name).collect();
        assert_eq!(a, b, "a HashMap-backed source must be sorted into a stable order");
        assert!(a.windows(2).all(|w| w[0] <= w[1]), "entries sort by name");

        // Two entries sharing a name would be indistinguishable in the index.
        let mut unique = a.clone();
        unique.dedup();
        assert_eq!(unique.len(), a.len(), "duplicate entry names: {:?}", a);
    }

    #[test]
    fn every_entry_renders_a_complete_page() {
        for entry in catalog(&abilities()) {
            assert!(!entry.name.is_empty(), "{:?} has no name", entry.id);
            assert!(
                !entry.description.is_empty(),
                "{} has no effect sentence",
                entry.name
            );
            assert!(!entry.subtitle().is_empty());
            for (key, value) in stat_rows(&entry) {
                assert!(!key.is_empty() && !value.is_empty(), "{} has a blank stat row", entry.name);
            }
        }
    }

    /// Classification is resolved per-AURA, not per-type — the distinction the
    /// card's amendment turns on. Rend and Corruption share `DamageOverTime`
    /// but a dispel can only take Corruption.
    #[test]
    fn dispel_classification_is_per_aura_not_per_mechanic() {
        let entries = catalog(&abilities());
        let by_name = |name: &str| {
            entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{} missing", name))
        };
        assert!(!by_name("Rend").sample.can_be_dispelled(), "Rend is physical");
        assert!(by_name("Corruption").sample.can_be_dispelled(), "Corruption is Shadow");
        assert!(
            by_name("Crippling Poison").sample.is_cleansable_poison(),
            "a poison is cleansed, not dispelled"
        );
    }

    #[test]
    fn engine_auras_carry_the_simulations_own_numbers() {
        let entries = catalog(&abilities());
        let weakened = entries.iter().find(|e| e.name == "Weakened Soul").expect("registered");
        assert_eq!(weakened.persistence, Persistence::Seconds(WEAKENED_SOUL_DURATION));
        assert!(weakened.source.is_none(), "Weakened Soul has no applying ability");

        // The totem buffs and the Frost Trap slow DO have an ability to link
        // back to — one with no `applies_aura` of its own.
        let windfury = entries.iter().find(|e| e.name == "Windfury Totem").expect("registered");
        assert_eq!(windfury.source, Some(AbilityType::AirTotem));
        assert!(matches!(windfury.persistence, Persistence::WhileSourceActive(_)));
        let frost_trap = entries.iter().find(|e| e.name == "Frost Trap").expect("registered");
        assert_eq!(frost_trap.sample.magnitude, FROST_TRAP_SLOW_MAGNITUDE);
    }

    /// `combat_core::damage` names the lockout after the interrupt that caused
    /// it, so every interrupt that locks a school is a named debuff players see
    /// — and the entry's duration must be that interrupt's own lockout.
    #[test]
    fn every_interrupt_with_a_lockout_is_a_named_debuff() {
        let abilities = abilities();
        let entries = catalog(&abilities);
        let mut checked = 0;
        for (_, def) in abilities.iter() {
            if !def.is_interrupt || def.lockout_duration <= 0.0 {
                continue;
            }
            let entry = entries
                .iter()
                .find(|e| e.name == def.name && e.mechanic == AuraType::SpellSchoolLockout)
                .unwrap_or_else(|| panic!("{} locks a school but has no catalog entry", def.name));
            assert_eq!(entry.persistence, Persistence::Seconds(def.lockout_duration));
            assert!(!entry.is_buff(), "a lockout is a debuff");
            checked += 1;
        }
        assert!(checked >= 4, "expected the four interrupts, checked {}", checked);
    }

    #[test]
    fn siblings_are_the_other_entries_sharing_a_mechanic() {
        let entries = catalog(&abilities());
        let rend = entries.iter().find(|e| e.name == "Rend").expect("Rend");
        let sibs = siblings(&entries, rend.mechanic, rend.id);
        assert!(sibs.iter().all(|s| s.id != rend.id), "an entry is not its own sibling");
        assert!(
            sibs.iter().any(|s| s.name == "Corruption"),
            "Rend must cross-link to the other damage-over-time effects"
        );
    }

    #[test]
    fn buffs_and_debuffs_both_have_entries() {
        let entries = catalog(&abilities());
        assert!(entries.iter().any(|e| e.is_buff()));
        assert!(entries.iter().any(|e| !e.is_buff()));
    }
}
