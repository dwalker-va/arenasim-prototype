//! The Abilities section — one page per ability in `abilities.ron`.
//!
//! The index is the whole file: every ability, filterable by owning class and
//! by spell school, in the same derived kit order the class pages use. The
//! detail page is the ability's config rendered field-by-field, plus the shared
//! [`ability_text`](crate::states::ability_text) generator for the prose.
//!
//! Nothing is hand-authored. Ability N+1 appears on its class page, in this
//! index, in search and with a full detail page the moment it exists in the
//! RON — which is the point of the `class` attribution AS-30 added.

use bevy_egui::egui;

use crate::states::ability_text::{build_ability_description, build_aura_description};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::{ScalingStat, SpellSchool};
use crate::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use crate::states::play_match::components::class_base_stats;
use crate::states::play_match::constants::{GCD, MELEE_RANGE};
use crate::states::play_match::AbilityType;

use super::auras::AuraId;
use super::search::SearchEntry;
use super::widget;
use super::{EncyclopediaData, Topic, DIM, MUTED, TEXT};

// ============================================================================
// FILTERS
// ============================================================================

/// Chip-bar filter state for the Abilities index. Single-select per axis (the
/// blessed mockup's contract), ANDed across the two axes.
#[derive(Default)]
pub struct AbilityFilters {
    /// `None` = "All classes".
    pub class: Option<CharacterClass>,
    /// `None` = "All schools".
    pub school: Option<SpellSchool>,
}

impl AbilityFilters {
    /// Whether the given ability passes both axes.
    pub fn matches(&self, config: &AbilityConfig) -> bool {
        if let Some(class) = self.class {
            if config.class != class {
                return false;
            }
        }
        if let Some(school) = self.school {
            if config.spell_school != school {
                return false;
            }
        }
        true
    }
}

/// Schools offered as filter chips, derived from what the corpus actually uses
/// so a school with no abilities never shows an empty chip.
fn schools_in_use(abilities: &AbilityDefinitions) -> Vec<SpellSchool> {
    let mut schools: Vec<SpellSchool> = Vec::new();
    for school in SpellSchool::all() {
        if abilities.iter().any(|(_, c)| c.spell_school == *school) {
            schools.push(*school);
        }
    }
    schools
}

// ============================================================================
// ORDERING
// ============================================================================

/// Every ability, grouped by owning class in [`CharacterClass::all`] order and,
/// within a class, in the derived kit order (`AbilityType` declaration order,
/// own abilities then pet abilities by pet).
///
/// This is a CONCATENATION of the per-class kits, not a second sort: the index
/// and the class pages therefore list an ability in exactly one order, and
/// there is one definition of that order (AS-30's `abilities_for_class`).
/// Every ability carries a required `class`, so this walk is exhaustive and
/// visits each ability once — pinned by a test below.
pub fn all_in_kit_order(abilities: &AbilityDefinitions) -> Vec<AbilityType> {
    CharacterClass::all()
        .iter()
        .flat_map(|class| abilities.abilities_for_class(*class))
        .collect()
}

// ============================================================================
// SEARCH REGISTRY
// ============================================================================

/// Contribute every ability to the search registry — a straight walk of
/// `abilities.ron`.
pub fn search_entries(abilities: &AbilityDefinitions, out: &mut Vec<SearchEntry>) {
    for (ability, config) in abilities.iter() {
        out.push(SearchEntry::new(
            Topic::Ability(*ability),
            config.name.clone(),
            subtitle(config),
        ));
    }
}

/// `Warlock · Shadow` — who owns it and what school it is. Pet abilities name
/// the pet: `Warlock · Felhunter · Shadow`.
pub fn subtitle(config: &AbilityConfig) -> String {
    let mut parts = vec![config.class.name().to_string()];
    if let Some(pet) = config.pet {
        parts.push(pet.name().to_string());
    }
    parts.push(format!("{:?}", config.spell_school));
    parts.join(" · ")
}

// ============================================================================
// SHARED TEXT
// ============================================================================

/// `25 Mana · 30 yd range · Instant · 30 sec cooldown` — the stat strip under
/// an ability's name, in tooltips and on its page.
pub fn cost_line(config: &AbilityConfig) -> String {
    let mut bits = Vec::new();
    if config.mana_cost > 0.0 {
        bits.push(format!("{:.0} {}", config.mana_cost, resource_name(config)));
    }
    bits.push(range_text(config));
    bits.push(cast_text(config));
    if config.cooldown > 0.0 {
        bits.push(format!("{:.0} sec cooldown", config.cooldown));
    }
    bits.join(" · ")
}

/// The resource an ability's `mana_cost` is denominated in — the owning class's
/// pool, so a Warrior ability reads "Rage" and a Rogue's "Energy".
fn resource_name(config: &AbilityConfig) -> &'static str {
    class_base_stats(config.class).resource_type.name()
}

fn range_text(config: &AbilityConfig) -> String {
    if config.range <= 0.0 {
        "Self".to_string()
    } else if config.range <= MELEE_RANGE {
        "Melee range".to_string()
    } else {
        format!("{:.0} yd range", config.range)
    }
}

/// The cast phrasing for the stat STRIP, where it stands alone and has to say
/// what the number means: `1.5 sec cast`, `5 sec channel`, `Instant`.
fn cast_text(config: &AbilityConfig) -> String {
    if config.cast_time > 0.0 {
        format!("{:.1} sec cast", config.cast_time)
    } else if let Some(channel) = config.channel_duration {
        format!("{:.0} sec channel", channel)
    } else {
        "Instant".to_string()
    }
}

/// The same fact for a LABELLED stat row, where "Cast time" already says what
/// it is: `1.5 sec`, `Instant (5 sec channel)`, `Instant`.
fn cast_value(config: &AbilityConfig) -> String {
    if config.cast_time > 0.0 {
        format!("{:.1} sec", config.cast_time)
    } else if let Some(channel) = config.channel_duration {
        format!("Instant ({:.0} sec channel)", channel)
    } else {
        "Instant".to_string()
    }
}

/// `40 yd`, `2.5 yd (melee)`, `Self` — a whole number keeps no decimal point,
/// but melee's 2.5 must not round to 3.
fn yards(distance: f32) -> String {
    if (distance.fract()).abs() < 0.05 {
        format!("{:.0} yd", distance)
    } else {
        format!("{:.1} yd", distance)
    }
}

/// The full generated mechanics text: what the ability does, plus a sentence
/// for the aura it applies. Both come from the shared generator, so this reads
/// identically to the same ability's tooltip in View Combatant.
pub fn mechanics_text(ability: AbilityType, config: &AbilityConfig) -> String {
    let stats = class_base_stats(config.class);
    let mut text = build_ability_description(ability, config, &stats);
    if let Some(aura) = &config.applies_aura {
        let mut sentence = build_aura_description(aura);
        // A proc-chance ability applies its aura only some of the time; saying
        // so is the difference between Crippling Poison and a guaranteed slow.
        if let Some(chance) = config.application_chance {
            if chance < 1.0 {
                sentence = format!("{:.0}% chance: {}", chance * 100.0, sentence);
            }
        }
        text.push(' ');
        text.push_str(&sentence);
    }
    text
}

// ============================================================================
// TOOLTIP
// ============================================================================

/// The ability tooltip — name in its school colour, owner, stat strip, and the
/// generated mechanics text.
pub fn tooltip(ui: &mut egui::Ui, ability: AbilityType, data: &EncyclopediaData) {
    let Some(config) = data.abilities.get(&ability) else {
        ui.label(egui::RichText::new(Topic::Ability(ability).name(data)).size(14.0).color(TEXT));
        return;
    };
    ui.label(
        egui::RichText::new(&config.name)
            .size(14.0)
            .color(school_color(config.spell_school))
            .strong(),
    );
    ui.label(egui::RichText::new(subtitle(config)).size(12.0).color(MUTED));
    ui.label(egui::RichText::new(cost_line(config)).size(12.0).color(DIM));
    ui.add_space(3.0);
    ui.label(egui::RichText::new(mechanics_text(ability, config)).size(12.0).color(TEXT));
}

/// A spell school as an egui colour — the shared `SpellSchool::color_rgb8`
/// authority, not a second table.
pub fn school_color(school: SpellSchool) -> egui::Color32 {
    let (r, g, b) = school.color_rgb8();
    egui::Color32::from_rgb(r, g, b)
}

// ============================================================================
// INDEX PAGE
// ============================================================================

/// The filtered ability grid. Returns the ability whose tile was clicked.
pub fn render_index(
    ui: &mut egui::Ui,
    filters: &mut AbilityFilters,
    data: &EncyclopediaData,
) -> Option<Topic> {
    render_chip_bar(ui, filters, data);
    ui.add_space(8.0);

    // Grouped by owning class, which is the structure the kit order already
    // has — the index is the eight class kits laid end to end, with their
    // seams drawn in rather than hidden. A wall of 70 unlabelled icons is not
    // navigable; eight labelled kits are.
    let groups: Vec<(CharacterClass, Vec<AbilityType>)> = CharacterClass::all()
        .iter()
        .map(|class| {
            let kit = data
                .abilities
                .abilities_for_class(*class)
                .into_iter()
                .filter(|a| data.abilities.get(a).is_some_and(|c| filters.matches(c)))
                .collect();
            (*class, kit)
        })
        .filter(|(_, kit): &(CharacterClass, Vec<AbilityType>)| !kit.is_empty())
        .collect();

    let total = data.abilities.ability_types().count();
    let shown: usize = groups.iter().map(|(_, kit)| kit.len()).sum();
    ui.label(
        egui::RichText::new(format!("{} of {} abilities", shown, total))
            .size(12.5)
            .color(DIM),
    );

    if shown == 0 {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No abilities match these filters.").size(15.0).color(MUTED),
            );
            ui.add_space(10.0);
            if ui.button(egui::RichText::new("Clear filters").size(13.0).color(TEXT)).clicked() {
                *filters = AbilityFilters::default();
            }
        });
        return None;
    }

    let mut clicked = None;
    for (class, kit) in groups {
        widget::sub_heading(ui, &class.name().to_uppercase());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for ability in kit {
                if let Some(topic) = widget::icon_link(ui, Topic::Ability(ability), data) {
                    clicked = Some(topic);
                }
            }
        });
    }
    clicked
}

/// Two chip rows: owning class, then spell school. Single-select per row, with
/// an explicit "All" chip so clearing an axis needs no second gesture.
fn render_chip_bar(ui: &mut egui::Ui, filters: &mut AbilityFilters, data: &EncyclopediaData) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("CLASS").size(12.0).color(DIM));
        if ui.selectable_label(filters.class.is_none(), "All").clicked() {
            filters.class = None;
        }
        for class in CharacterClass::all() {
            if ui.selectable_label(filters.class == Some(*class), class.name()).clicked() {
                filters.class = Some(*class);
            }
        }
    });

    ui.add_space(4.0);

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("SCHOOL").size(12.0).color(DIM));
        if ui.selectable_label(filters.school.is_none(), "All").clicked() {
            filters.school = None;
        }
        for school in schools_in_use(data.abilities) {
            let active = filters.school == Some(school);
            let label = egui::RichText::new(format!("{:?}", school))
                // The chip wears its school's colour, except when selected —
                // the selection fill is gold, so the label goes dark to stay
                // legible on it.
                .color(if active { super::BG } else { school_color(school) });
            if ui.selectable_label(active, label).clicked() {
                filters.school = Some(school);
            }
        }

        ui.separator();
        if ui.button(egui::RichText::new("Clear").size(12.5).color(TEXT)).clicked() {
            *filters = AbilityFilters::default();
        }
    });
}

// ============================================================================
// DETAIL PAGE
// ============================================================================

/// One ability's page: identity header, owning-class link, generated mechanics
/// text, the full numeric block, and a link to the aura it applies.
pub fn render_detail(
    ui: &mut egui::Ui,
    ability: AbilityType,
    data: &EncyclopediaData,
) -> Option<Topic> {
    let topic = Topic::Ability(ability);
    let Some(config) = data.abilities.get(&ability) else {
        ui.label(egui::RichText::new("Unknown ability").size(16.0).color(MUTED));
        return None;
    };

    widget::detail_header(ui, topic, &cost_line(config), data);

    // --- Owning class, as a link back the way it came ---
    let mut clicked = None;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if let Some(topic) = widget::chip(ui, Topic::Class(config.class), data) {
            clicked = Some(topic);
        }
        if let Some(pet) = config.pet {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("Cast by the {}", pet.name()))
                    .size(12.5)
                    .color(MUTED),
            );
        }
    });

    ui.add_space(12.0);
    widget::prose_block(ui, &mechanics_text(ability, config));

    ui.add_space(12.0);
    widget::stat_rows(ui, "encyclopedia_ability_stats", &stat_rows(config));

    // --- The aura it applies ---
    //
    // A forward link into the Buffs & Debuffs section, addressed by the NAMED
    // aura this ability's `applies_aura` block defines — the same `AuraId` the
    // catalog keys its entry on, so the row lands on that aura's own page and
    // the page links back here as the applying ability.
    if let Some(aura) = &config.applies_aura {
        widget::section_heading(ui, "APPLIES");
        let trailing = format!("{:.0} sec", aura.duration);
        let width = ui.available_width().min(430.0);
        let topic = Topic::Aura(AuraId::Ability(ability));
        if let Some(topic) = widget::row(ui, topic, &trailing, width, data) {
            clicked = Some(topic);
        }
    }

    clicked
}

/// Key/value rows for the ability's numeric block. Derived field-by-field from
/// [`AbilityConfig`], so a new config field needs one line here and nothing per
/// ability.
fn stat_rows(config: &AbilityConfig) -> Vec<(String, String)> {
    let mut rows = vec![("Cast time".to_string(), cast_value(config))];

    rows.push((
        "Range".to_string(),
        if config.range <= 0.0 {
            "Self".to_string()
        } else if config.range <= MELEE_RANGE {
            format!("{} (melee)", yards(config.range))
        } else {
            yards(config.range)
        },
    ));
    if let Some(min) = config.min_range {
        // The Hunter dead zone: closer than this and the shot cannot be fired.
        rows.push(("Minimum range".to_string(), yards(min)));
    }
    if config.mana_cost > 0.0 {
        rows.push((
            "Cost".to_string(),
            format!("{:.0} {}", config.mana_cost, resource_name(config)),
        ));
    }
    rows.push((
        "Cooldown".to_string(),
        if config.cooldown > 0.0 {
            format!("{:.0} sec", config.cooldown)
        } else {
            "None".to_string()
        },
    ));
    // Interrupts are off the GCD (WoW-faithful, and the rule the sim actually
    // implements — `combat_ai.rs` never sets `global_cooldown` on the interrupt
    // path). Everything else pays the standard 1.5s.
    rows.push((
        "Global cooldown".to_string(),
        if config.is_interrupt {
            "Not triggered".to_string()
        } else {
            format!("{:.1} sec", GCD)
        },
    ));

    if config.damage_base_max > 0.0 {
        rows.push((
            "Damage".to_string(),
            scaled_range(
                config.damage_base_min,
                config.damage_base_max,
                config.damage_coefficient,
                scaling_stat_name(config.damage_scales_with),
            ),
        ));
    }
    if config.healing_base_max > 0.0 {
        rows.push((
            "Healing".to_string(),
            scaled_range(
                config.healing_base_min,
                config.healing_base_max,
                config.healing_coefficient,
                Some("spell power"),
            ),
        ));
    }
    if let Some(channel) = config.channel_duration {
        rows.push((
            "Channel".to_string(),
            format!("{:.0} sec, ticks every {:.0} sec", channel, config.channel_tick_interval),
        ));
    }
    if config.channel_healing_per_tick > 0.0 {
        rows.push((
            "Health per tick".to_string(),
            format!("+{:.0} to the caster", config.channel_healing_per_tick),
        ));
    }
    if config.mana_burn_amount > 0.0 {
        rows.push(("Mana burned".to_string(), format!("{:.0}", config.mana_burn_amount)));
    }
    if config.is_interrupt && config.lockout_duration > 0.0 {
        rows.push(("School lockout".to_string(), format!("{:.0} sec", config.lockout_duration)));
    }
    if let Some(chance) = config.application_chance {
        rows.push(("Application chance".to_string(), format!("{:.0}%", chance * 100.0)));
    }
    if let Some(speed) = config.projectile_speed {
        rows.push(("Projectile speed".to_string(), format!("{:.0} yd/sec", speed)));
    }
    if config.requires_stealth {
        rows.push(("Requires".to_string(), "Stealth".to_string()));
    }
    rows.push(("Spell school".to_string(), format!("{:?}", config.spell_school)));

    rows
}

/// `15 – 25 + 100% of attack power` — the base range plus its scaling term.
fn scaled_range(min: f32, max: f32, coefficient: f32, stat: Option<&str>) -> String {
    let base = if (max - min).abs() < f32::EPSILON {
        format!("{:.0}", min)
    } else {
        format!("{:.0} – {:.0}", min, max)
    };
    match stat {
        Some(stat) if coefficient != 0.0 => {
            format!("{} + {:.0}% of {}", base, coefficient * 100.0, stat)
        }
        _ => base,
    }
}

fn scaling_stat_name(stat: ScalingStat) -> Option<&'static str> {
    match stat {
        ScalingStat::AttackPower => Some("attack power"),
        ScalingStat::SpellPower => Some("spell power"),
        ScalingStat::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::ability_config::load_ability_definitions;
    use std::collections::HashSet;

    /// The index walk is exhaustive and duplicate-free: concatenating the
    /// per-class kits must visit every ability in `abilities.ron` exactly once.
    /// If this ever fails, an ability is either missing from the index or shown
    /// twice — both invisible without it.
    #[test]
    fn the_index_lists_every_ability_exactly_once() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        let order = all_in_kit_order(&abilities);
        let unique: HashSet<AbilityType> = order.iter().copied().collect();
        assert_eq!(order.len(), unique.len(), "an ability is listed twice");
        assert_eq!(unique.len(), abilities.ability_types().count());
    }

    /// A class page and the index agree on order, because one derives from the
    /// other rather than re-sorting.
    #[test]
    fn the_index_preserves_each_classs_kit_order() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        let order = all_in_kit_order(&abilities);
        for class in CharacterClass::all() {
            let kit = abilities.abilities_for_class(*class);
            let from_index: Vec<AbilityType> = order
                .iter()
                .copied()
                .filter(|a| abilities.get_unchecked(a).class == *class)
                .collect();
            assert_eq!(from_index, kit, "{:?} index order diverged from its kit", class);
        }
    }

    /// Every ability renders a full page's worth of derived content — no blank
    /// headers, no empty stat blocks.
    #[test]
    fn every_ability_has_a_subtitle_cost_line_and_stat_rows() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        for (ability, config) in abilities.iter() {
            assert!(!subtitle(config).is_empty(), "{:?} has no subtitle", ability);
            assert!(!cost_line(config).is_empty(), "{:?} has no cost line", ability);
            assert!(!mechanics_text(*ability, config).trim().is_empty());
            let rows = stat_rows(config);
            // Cast time, range, cooldown, GCD and school are unconditional.
            assert!(rows.len() >= 5, "{:?} produced a thin stat block", ability);
            for (key, value) in &rows {
                assert!(!key.is_empty() && !value.is_empty());
            }
        }
    }

    /// Interrupts say they are off the GCD; everything else pays it.
    #[test]
    fn the_gcd_row_follows_the_interrupt_flag() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        for (_, config) in abilities.iter() {
            let gcd = stat_rows(config)
                .into_iter()
                .find(|(k, _)| k == "Global cooldown")
                .expect("every ability reports its GCD interaction")
                .1;
            if config.is_interrupt {
                assert_eq!(gcd, "Not triggered");
            } else {
                assert_eq!(gcd, "1.5 sec");
            }
        }
    }

    #[test]
    fn filters_and_across_axes() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        let mut filters = AbilityFilters::default();
        let count = |f: &AbilityFilters| abilities.iter().filter(|(_, c)| f.matches(c)).count();

        let all = count(&filters);
        assert_eq!(all, abilities.ability_types().count());

        filters.class = Some(CharacterClass::Mage);
        let mage = count(&filters);
        assert!(mage > 0 && mage < all);

        filters.school = Some(SpellSchool::Frost);
        let mage_frost = count(&filters);
        assert!(mage_frost > 0 && mage_frost <= mage, "a second axis can only narrow");
    }

    /// Every school chip offered has at least one ability behind it.
    #[test]
    fn school_chips_are_derived_from_the_corpus() {
        let abilities = load_ability_definitions().expect("abilities.ron must load");
        let schools = schools_in_use(&abilities);
        assert!(!schools.is_empty());
        for school in schools {
            assert!(abilities.iter().any(|(_, c)| c.spell_school == school));
        }
    }
}
