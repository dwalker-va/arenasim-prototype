//! Every aura's removal class, checked against its TBC dispel type.
//!
//! An aura's removal class decides who can take it off: a dispel takes MAGIC, a
//! cleanse takes magic and POISON, nothing in the arena takes a CURSE, and no
//! removal takes a PHYSICAL effect or one with no dispel type at all
//! (UNTYPED). `abilities.ron` declares the class only where the school gets it
//! wrong (`dispel_type: Poison`, `dispel_type: Curse`, `dispel_type:
//! Untyped`); everything else is derived from the school, and Nature, Shadow
//! and the rest derive to magic. So an ability that TBC calls a poison and that
//! forgets the declaration ships as dispellable magic, with nothing to say so.
//! That is how Serpent Sting came to be dispelled by Priests.
//!
//! [`expected`] is the guard: an EXPLICIT row per ability, written by hand, in
//! an exhaustive `match` with no wildcard arm. A new `AbilityType` does not
//! compile until someone writes its row, and the row has to state what TBC
//! calls the aura and what the sim resolves it to. The tests then check the row
//! against the aura the engine actually builds — from the RON for a config aura,
//! from the engine's own constructor for an aura built in code (traps, totems).
//!
//! **Where the sim and TBC disagree, the row says why.** A row whose two
//! classes disagree must carry a `divergence` reason, and a row whose classes
//! agree must not, so a divergence is always a decision someone wrote down and
//! never an accident the table absorbed.
//!
//! ## The TBC column
//!
//! TBC dispel rules are the guide (user ruling, 2026-10-11). Exceptions are
//! the user's, case by case, and each one is a `divergence` row.
//!
//! The column is `SpellCategories.DispelType` from the TBC Classic client,
//! wago.tools build **2.5.6.69795** (the `wow_anniversary` product): 1 Magic,
//! 2 Curse, 3 Disease, 4 Poison, 0 none. It is read at `DifficultyID` 0. This
//! build carries extra rows per difficulty for some spells, and they can
//! disagree: an NPC Psychic Scream, 13704, reads both Magic and none. The
//! spell read is the one that carries the aura. That is the highest player
//! rank (the ranks agree), or the aura's own spell where it is separate from
//! the ability: Freezing Trap Effect 3355, Frost Trap Aura 13810, Crippling
//! Poison 11201. Wowhead TBC agrees on every row spot-checked against it.
//!
//! **Why not the Classic Era client.** Era (1.15.9) reproduces patch 1.12,
//! and this sim does not follow 1.12. Psychic Scream is the clearest case:
//! every Era rank reads DispelType 0. That is a faithful reading of the Era
//! data, not a misread column or a triggered spell. Patch 2.0.1 made Psychic
//! Scream "a magic effect [that] can now be dispelled", and every TBC rank
//! reads Magic. Era and TBC also disagree on these:
//! - Concussive Shot: Era Magic, TBC none.
//! - Frost Trap Aura: Era Magic, TBC none.
//! - Divine Shield: Era none, TBC Magic.
//! - the Mage armors: Era Magic, TBC none.
//!
//! TBC's "none" is satisfied by a PHYSICAL aura, an UNTYPED one, or a
//! schoolless one that declares no class. In every case no dispel, cleanse or
//! purge takes it as magic.
//!
//! The class is the subject here, not the mechanic. Whether a dispel takes a
//! magic STUN (it does not — `AuraType::is_magic_dispellable`) is the aura
//! type's question, and is answered there.

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::shaman::totem_spec;
use arenasim::states::play_match::class_ai::{ally_dispel_scope, ally_removal};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, DispelPending,
    DispelScope, DispelType, GameRng, TotemElement,
};
use arenasim::states::play_match::effects::process_dispels;
use arenasim::states::play_match::totems::make_totem_aura;
use arenasim::states::play_match::traps::{freezing_trap_aura, frost_trap_slow_aura};
use arenasim::CharacterClass;

/// The removal class the sim resolves an aura to — the engine's own word for
/// it, [`Aura::removal_class_name`], plus [`Aura::is_untyped`] for the class
/// that has no word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sim {
    Magic,
    Poison,
    Curse,
    Disease,
    Physical,
    /// Declared to carry no dispel type (`DispelType::Untyped`).
    Untyped,
    /// Schoolless and undeclared: the engine names no class.
    Unclassed,
}

/// TBC's `SpellCategories.DispelType` for the aura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tbc {
    Magic,
    Poison,
    Curse,
    #[allow(dead_code)] // no disease in the sim yet; the row type is ready for one
    Disease,
    /// DispelType 0.
    NoType,
}

impl Tbc {
    fn agrees_with(self, sim: Sim) -> bool {
        match self {
            Tbc::Magic => sim == Sim::Magic,
            Tbc::Poison => sim == Sim::Poison,
            Tbc::Curse => sim == Sim::Curse,
            Tbc::Disease => sim == Sim::Disease,
            Tbc::NoType => matches!(sim, Sim::Physical | Sim::Untyped | Sim::Unclassed),
        }
    }
}

/// Where the aura an ability lands is built.
enum Source {
    /// The ability's `applies_aura` block, through `AuraPending::from_ability`.
    Config,
    /// An engine constructor. The RON block, if the ability has one, is not
    /// what lands.
    Engine(fn() -> Aura),
}

/// One row of the table.
enum Row {
    /// The ability lands no aura of its own (interrupt lockouts are schoolless
    /// engine auras with no class, and are not the ability's aura).
    NoAura,
    Aura {
        source: Source,
        sim: Sim,
        tbc: Tbc,
        /// Why the sim departs from TBC. Required exactly when it does.
        divergence: Option<&'static str>,
    },
}

const fn config(sim: Sim, tbc: Tbc) -> Row {
    Row::Aura {
        source: Source::Config,
        sim,
        tbc,
        divergence: None,
    }
}

const fn config_diverging(sim: Sim, tbc: Tbc, why: &'static str) -> Row {
    Row::Aura {
        source: Source::Config,
        sim,
        tbc,
        divergence: Some(why),
    }
}

fn totem(element: TotemElement) -> Aura {
    let (_, aura_type, magnitude, school) = totem_spec(element);
    make_totem_aura(
        aura_type,
        magnitude,
        Entity::PLACEHOLDER,
        school,
        element.buff_name(),
    )
}

/// A totem's buff, as the totem pulses it. TBC gives totem auras no dispel
/// type (Strength of Earth 25527, Windfury Totem Passive 25582, Flametongue
/// Totem Passive 25556, Healing Stream 25566).
const fn totem_row(build: fn() -> Aura) -> Row {
    Row::Aura {
        source: Source::Engine(build),
        sim: Sim::Untyped,
        tbc: Tbc::NoType,
        divergence: None,
    }
}

/// The table. **No wildcard arm** — a new ability does not compile until its
/// row is written.
fn expected(ability: AbilityType) -> Row {
    use Sim as S;
    use Tbc as T;
    match ability {
        // ---- Mage ----
        AbilityType::Frostbolt => config(S::Magic, T::Magic),
        AbilityType::FrostNova => config(S::Magic, T::Magic),
        AbilityType::ArcaneIntellect => config(S::Magic, T::Magic),
        AbilityType::IceBarrier => config(S::Magic, T::Magic),
        AbilityType::Polymorph => config(S::Magic, T::Magic),
        // Mage armors: no dispel type in TBC (7301, 27125, 30482); the user
        // ruled them unpurgeable (2026-10-11).
        AbilityType::FrostArmor => config(S::Untyped, T::NoType),
        AbilityType::MageArmorSpell => config(S::Untyped, T::NoType),
        AbilityType::MoltenArmor => config(S::Untyped, T::NoType),

        // ---- Priest ----
        AbilityType::FlashHeal => Row::NoAura,
        AbilityType::MindBlast => Row::NoAura,
        AbilityType::PowerWordFortitude => config(S::Magic, T::Magic),
        AbilityType::PowerWordShield => config(S::Magic, T::Magic),
        AbilityType::DispelMagic => Row::NoAura,
        // 10890. Magic since 2.0.1; the user ruled it dispellable (2026-10-11).
        AbilityType::PsychicScream => config(S::Magic, T::Magic),
        AbilityType::ManaBurn => Row::NoAura,
        // 589 .. 25368, every rank DispelType 1 (Magic).
        AbilityType::ShadowWordPain => config(S::Magic, T::Magic),

        // ---- Warrior ----
        AbilityType::HeroicStrike => Row::NoAura,
        AbilityType::Charge => Row::NoAura,
        AbilityType::Rend => config(S::Physical, T::NoType),
        AbilityType::MortalStrike => config(S::Physical, T::NoType),
        AbilityType::Pummel => Row::NoAura,
        AbilityType::BerserkerRage => config(S::Unclassed, T::NoType),
        AbilityType::BattleShout => config(S::Unclassed, T::NoType),
        AbilityType::DemoralizingShout => config(S::Unclassed, T::NoType),
        AbilityType::CommandingShout => config(S::Unclassed, T::NoType),

        // ---- Rogue ----
        AbilityType::Ambush => Row::NoAura,
        AbilityType::SinisterStrike => Row::NoAura,
        AbilityType::CheapShot => config(S::Physical, T::NoType),
        AbilityType::KidneyShot => config(S::Physical, T::NoType),
        AbilityType::Kick => Row::NoAura,
        AbilityType::CripplingPoison => config(S::Poison, T::Poison),
        AbilityType::DeadlyPoison => config(S::Poison, T::Poison),
        AbilityType::WoundPoison => config(S::Poison, T::Poison),
        AbilityType::MindNumbingPoison => config(S::Poison, T::Poison),

        // ---- Warlock ----
        AbilityType::Corruption => config(S::Magic, T::Magic),
        AbilityType::Shadowbolt => Row::NoAura,
        AbilityType::Fear => config(S::Magic, T::Magic),
        AbilityType::Immolate => config(S::Magic, T::Magic),
        AbilityType::DrainLife => Row::NoAura,
        AbilityType::CurseOfAgony => config(S::Curse, T::Curse),
        AbilityType::CurseOfWeakness => config(S::Curse, T::Curse),
        AbilityType::CurseOfTongues => config(S::Curse, T::Curse),
        AbilityType::UnstableAffliction => config(S::Magic, T::Magic),
        AbilityType::DeathCoil => config(S::Magic, T::Magic),
        AbilityType::SpellLock => Row::NoAura,
        AbilityType::DevourMagic => Row::NoAura,

        // ---- Paladin ----
        AbilityType::FlashOfLight => Row::NoAura,
        AbilityType::HolyLight => Row::NoAura,
        AbilityType::HolyShock => Row::NoAura,
        AbilityType::HammerOfJustice => config(S::Magic, T::Magic),
        AbilityType::PaladinCleanse => Row::NoAura,
        AbilityType::DivineShield => config_diverging(
            S::Unclassed,
            T::Magic, // 1020
            "TBC's Divine Shield is Magic, but only Mass Dispel removes it, and the sim has \
             no Mass Dispel; no purge takes it (`AuraType::DamageImmunity`). Kept by user \
             ruling, 2026-10-11",
        ),
        // Paladin auras: no dispel type in TBC, and the user ruled them
        // unpurgeable (2026-10-11).
        AbilityType::DevotionAura => config(S::Untyped, T::NoType),
        AbilityType::ShadowResistanceAura => config(S::Untyped, T::NoType),
        AbilityType::ConcentrationAura => config(S::Untyped, T::NoType),

        // ---- Hunter ----
        AbilityType::AimedShot => config(S::Physical, T::NoType),
        AbilityType::ArcaneShot => Row::NoAura,
        AbilityType::ConcussiveShot => config(S::Physical, T::NoType), // 5116
        AbilityType::SerpentSting => config(S::Poison, T::Poison),
        AbilityType::Disengage => Row::NoAura,
        AbilityType::FreezingTrap => Row::Aura {
            source: Source::Engine(|| freezing_trap_aura(Entity::PLACEHOLDER)),
            sim: S::Magic,
            tbc: T::Magic, // Freezing Trap Effect, 3355
            divergence: None,
        },
        AbilityType::FrostTrap => Row::Aura {
            source: Source::Engine(|| frost_trap_slow_aura(Entity::PLACEHOLDER, 0.5)),
            // No dispel type; the user ruled it undispellable (2026-10-11).
            sim: S::Untyped,
            tbc: T::NoType, // Frost Trap Aura, 13810
            divergence: None,
        },
        AbilityType::Flare => Row::NoAura,

        // ---- Hunter pets ----
        // Web, 4167: a Physical-school root with no dispel type. The user
        // ruled it physical (2026-10-11), so no dispel frees it.
        AbilityType::SpiderWeb => config(S::Physical, T::NoType),
        AbilityType::BoarCharge => config(S::Physical, T::NoType),
        AbilityType::MastersCall => Row::NoAura,

        // ---- Shaman ----
        AbilityType::LightningBolt => Row::NoAura,
        AbilityType::FrostShock => config(S::Magic, T::Magic),
        AbilityType::LesserHealingWave => Row::NoAura,
        AbilityType::Purge => Row::NoAura,
        AbilityType::WindShear => Row::NoAura,
        // Totem buffs: no dispel type in TBC, and the user ruled them
        // unpurgeable (2026-10-11).
        AbilityType::AirTotem => totem_row(|| totem(TotemElement::Air)),
        AbilityType::WaterTotem => totem_row(|| totem(TotemElement::Water)),
        AbilityType::EarthTotem => totem_row(|| totem(TotemElement::Earth)),
        AbilityType::FireTotem => totem_row(|| totem(TotemElement::Fire)),

        // ---- Druid ----
        AbilityType::Rejuvenation => config(S::Magic, T::Magic),
        AbilityType::Lifebloom => config(S::Magic, T::Magic),
        AbilityType::Swiftmend => Row::NoAura,
        AbilityType::Moonfire => config(S::Magic, T::Magic),
        AbilityType::MarkOfTheWild => config(S::Magic, T::Magic),
        AbilityType::Innervate => config(S::Magic, T::Magic),
        AbilityType::EntanglingRoots => config(S::Magic, T::Magic),
        // 33786, no dispel type (user ruling, 2026-10-11). Its TYPE is never a
        // dispel candidate either (`AuraType::Cyclone`).
        AbilityType::Cyclone => config(S::Untyped, T::NoType),
        // 783. A shapeshift, not a magic buff: the user ruled nothing purges
        // or dispels it (2026-10-11).
        AbilityType::TravelForm => config(S::Untyped, T::NoType),
    }
}

fn sim_class(aura: &Aura) -> Sim {
    if aura.is_untyped() {
        return Sim::Untyped;
    }
    match aura.removal_class_name() {
        Some("Magic") => Sim::Magic,
        Some("Poison") => Sim::Poison,
        Some("Curse") => Sim::Curse,
        Some("Disease") => Sim::Disease,
        Some("Physical") => Sim::Physical,
        None => Sim::Unclassed,
        Some(other) => panic!("unknown removal class word {other:?}"),
    }
}

fn config_aura(defs: &AbilityDefinitions, ability: AbilityType) -> Option<Aura> {
    AuraPending::from_ability(
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
        defs.get_unchecked(&ability),
    )
    .map(|pending| pending.aura)
}

/// The aura an ability lands, built where the table says it is built.
fn landed_aura(defs: &AbilityDefinitions, ability: AbilityType) -> Aura {
    match expected(ability) {
        Row::Aura {
            source: Source::Engine(build),
            ..
        } => build(),
        Row::Aura {
            source: Source::Config,
            ..
        } => config_aura(defs, ability).expect("tabled as a config aura"),
        Row::NoAura => panic!("{ability:?} lands no aura"),
    }
}

/// Every ability's row matches the aura the engine builds for it.
#[test]
fn every_aura_resolves_to_its_tabled_class() {
    let defs = AbilityDefinitions::default();
    let mut wrong = Vec::new();
    let mut checked = 0;

    for (&ability, _) in defs.iter() {
        let config = config_aura(&defs, ability);
        match expected(ability) {
            Row::NoAura => {
                if let Some(aura) = config {
                    wrong.push(format!(
                        "{ability:?}: tabled as landing no aura, but its config applies {:?}",
                        aura.effect_type
                    ));
                }
            }
            Row::Aura { source, sim, .. } => {
                let aura = match source {
                    Source::Config => match config {
                        Some(aura) => aura,
                        None => {
                            wrong.push(format!(
                                "{ability:?}: tabled as a config aura, but has no `applies_aura`"
                            ));
                            continue;
                        }
                    },
                    Source::Engine(build) => build(),
                };
                checked += 1;
                let actual = sim_class(&aura);
                if actual != sim {
                    wrong.push(format!(
                        "{ability:?}: the engine resolves {:?} ({}) to {actual:?}, the table says {sim:?}",
                        aura.effect_type, aura.ability_name
                    ));
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "dispel-type table mismatches:\n{}",
        wrong.join("\n")
    );
    // Non-vacuity: the table is not all `NoAura`.
    assert!(checked >= 50, "only {checked} auras were checked");
}

/// A row whose classes disagree says why, and a row whose classes agree does
/// not carry a stale reason.
#[test]
fn every_divergence_from_tbc_is_explained() {
    let defs = AbilityDefinitions::default();
    let mut problems = Vec::new();
    let mut divergent = Vec::new();

    for (&ability, _) in defs.iter() {
        let Row::Aura {
            sim,
            tbc,
            divergence,
            ..
        } = expected(ability)
        else {
            continue;
        };
        match (tbc.agrees_with(sim), divergence) {
            (true, Some(_)) => problems.push(format!(
                "{ability:?}: {sim:?} agrees with TBC {tbc:?} but carries a divergence reason"
            )),
            (false, None) => problems.push(format!(
                "{ability:?}: the sim's {sim:?} departs from TBC {tbc:?} with no reason given"
            )),
            (false, Some(_)) => divergent.push(format!("{ability:?}")),
            (true, None) => {}
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // The divergences are a fixed, named set: a new one is a decision, not drift.
    divergent.sort();
    assert_eq!(divergent, ["DivineShield"]);
}

/// TBC's poisons are the sim's poisons, and nothing else is. Cleanse is the
/// only removal that reaches a poison, so this set is exactly what a Priest's
/// Dispel Magic and a Felhunter's Devour Magic may not touch.
#[test]
fn the_poisons_are_the_rogue_weapon_poisons_and_serpent_sting() {
    let defs = AbilityDefinitions::default();
    let mut poisons: Vec<String> = defs
        .iter()
        .filter_map(|(&ability, _)| config_aura(&defs, ability).map(|a| (ability, a)))
        .filter(|(_, aura)| aura.is_cleansable_poison())
        .map(|(ability, _)| format!("{ability:?}"))
        .collect();
    poisons.sort();
    assert_eq!(
        poisons,
        [
            "CripplingPoison",
            "DeadlyPoison",
            "MindNumbingPoison",
            "SerpentSting",
            "WoundPoison"
        ]
    );
}

// ---------------------------------------------------------------------------
// Removals against real auras, through the real `process_dispels`
// ---------------------------------------------------------------------------

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(CombatLog::default());
    app.insert_resource(GameRng::from_seed(7));
    app.insert_resource(ArenaDampening::default());
    app.add_systems(Update, process_dispels);
    app
}

/// `aura`, worn by a Warrior, then one removal of `scope` queued against it by
/// a `dispeller_class`. Returns whether an aura named like `aura` survived.
fn survives(aura: Aura, scope: DispelScope, dispeller_class: CharacterClass) -> bool {
    let name = aura.ability_name.clone();
    let mut app = harness();
    let victim = app
        .world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            ActiveAuras { auras: vec![aura] },
        ))
        .id();
    let dispeller = app
        .world_mut()
        .spawn(Combatant::new(1, 1, dispeller_class))
        .id();
    app.world_mut().spawn(DispelPending {
        target: victim,
        dispeller,
        log_prefix: "[DISPEL]",
        caster_class: dispeller_class,
        heal_on_success: None,
        scope,
    });
    app.update();

    app.world()
        .entity(victim)
        .get::<ActiveAuras>()
        .is_some_and(|auras| auras.auras.iter().any(|a| a.ability_name == name))
}

/// Serpent Sting exactly as the Hunter lands it, against one removal.
fn sting_survives(scope: DispelScope, dispeller_class: CharacterClass) -> bool {
    let defs = AbilityDefinitions::default();
    let sting =
        config_aura(&defs, AbilityType::SerpentSting).expect("Serpent Sting applies an aura");
    assert_eq!(sting.effect_type, AuraType::DamageOverTime);
    survives(sting, scope, dispeller_class)
}

#[test]
fn priest_dispel_magic_cannot_take_serpent_sting() {
    assert!(sting_survives(
        ally_dispel_scope(AbilityType::DispelMagic),
        CharacterClass::Priest
    ));
}

#[test]
fn paladin_cleanse_takes_serpent_sting() {
    assert!(!sting_survives(
        ally_dispel_scope(AbilityType::PaladinCleanse),
        CharacterClass::Paladin
    ));
}

#[test]
fn felhunter_devour_magic_cannot_take_serpent_sting() {
    assert!(sting_survives(
        ally_dispel_scope(AbilityType::DevourMagic),
        CharacterClass::Warlock
    ));
}

/// Purge is offensive and magic-only: it takes enemy buffs, so a sting on its
/// target is nothing it can take.
#[test]
fn shaman_purge_cannot_take_serpent_sting() {
    assert!(sting_survives(DispelScope::Purge, CharacterClass::Shaman));
}

/// Every way `ability`'s buff could be taken that the rulings forbid: a
/// Shaman's Purge and every ally-side removal. Checked on the aura as the
/// engine lands it, through the real `process_dispels`. The buff is the
/// target's only aura, so a purge that could take anything takes it.
///
/// With `control`, also the SAME aura with its class put back to the school's
/// default, which a purge must take. So a survival is the class at work, not a
/// purge that could never have reached the aura's type.
fn purge_failures(defs: &AbilityDefinitions, ability: AbilityType, control: bool) -> Vec<String> {
    let aura = landed_aura(defs, ability);
    let mut wrong = Vec::new();
    if !survives(aura.clone(), DispelScope::Purge, CharacterClass::Shaman) {
        wrong.push(format!("{ability:?}: Purge took {}", aura.ability_name));
    }
    if control {
        let mut as_magic = aura.clone();
        as_magic.dispel_type = DispelType::Auto;
        if survives(as_magic, DispelScope::Purge, CharacterClass::Shaman) {
            wrong.push(format!(
                "{ability:?}: control: Purge could not take it even as magic, so the \
                 check above proves nothing"
            ));
        }
    }
    wrong.extend(dispel_failures(&aura, ability));
    wrong
}

/// Every ally-side removal that takes `aura` (it should take none).
fn dispel_failures(aura: &Aura, ability: AbilityType) -> Vec<String> {
    let mut wrong = Vec::new();
    for dispel in [
        AbilityType::DispelMagic,
        AbilityType::PaladinCleanse,
        AbilityType::DevourMagic,
    ] {
        if !survives(
            aura.clone(),
            ally_dispel_scope(dispel),
            CharacterClass::Priest,
        ) {
            wrong.push(format!(
                "{ability:?}: {dispel:?} took {}",
                aura.ability_name
            ));
        }
    }
    wrong
}

/// User ruling, 2026-10-11: no purge takes a Paladin aura, a totem's buff or
/// Travel Form.
///
/// Travel Form has no control: its TYPE is unpurgeable too
/// (`Aura::can_be_purged`), so two rules hold it, and the table above pins
/// its class.
#[test]
fn purge_cannot_take_a_paladin_aura_a_totem_buff_or_travel_form() {
    let defs = AbilityDefinitions::default();
    let mut wrong = Vec::new();
    for ability in [
        AbilityType::DevotionAura,
        AbilityType::ShadowResistanceAura,
        AbilityType::ConcentrationAura,
        AbilityType::AirTotem,
        AbilityType::WaterTotem,
        AbilityType::EarthTotem,
        AbilityType::FireTotem,
    ] {
        wrong.extend(purge_failures(&defs, ability, true));
    }
    wrong.extend(purge_failures(&defs, AbilityType::TravelForm, false));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// User ruling, 2026-10-11: no purge takes a Mage armor (TBC gives the armors
/// no dispel type).
#[test]
fn purge_cannot_take_a_mage_armor() {
    let defs = AbilityDefinitions::default();
    let mut wrong = Vec::new();
    for ability in [
        AbilityType::FrostArmor,
        AbilityType::MageArmorSpell,
        AbilityType::MoltenArmor,
    ] {
        wrong.extend(purge_failures(&defs, ability, true));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `ability`'s debuff, as the engine lands it, survives every ally-side
/// removal. The control is the same debuff with its class put back to the
/// school's default, which Dispel Magic must take: so a survival is the class
/// at work, not a dispel that never reaches the mechanic.
fn assert_undispellable(ability: AbilityType) {
    let defs = AbilityDefinitions::default();
    let aura = landed_aura(&defs, ability);
    let wrong = dispel_failures(&aura, ability);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    let mut as_magic = aura;
    as_magic.dispel_type = DispelType::Auto;
    assert!(
        !survives(
            as_magic,
            ally_dispel_scope(AbilityType::DispelMagic),
            CharacterClass::Priest
        ),
        "control: Dispel Magic cannot take {ability:?} even as magic, so the check proves nothing"
    );
}

/// User ruling, 2026-10-11: the Spider's Web is physical (TBC's Web is a
/// Physical-school root), so no dispel frees its victim. Master's Call still
/// does: it clears movement impairments whatever their class.
#[test]
fn no_dispel_frees_a_web() {
    assert_undispellable(AbilityType::SpiderWeb);
}

/// User ruling, 2026-10-11: Frost Trap's slow has no dispel type (TBC's Frost
/// Trap Aura), so no dispel lifts it.
#[test]
fn no_dispel_lifts_the_frost_trap_slow() {
    assert_undispellable(AbilityType::FrostTrap);
}

/// Master's Call clears movement impairments whatever their class, so the
/// rulings that put the Web and the Frost Trap slow out of every dispel's reach
/// leave them in its: the Bird still frees its team from both.
#[test]
fn masters_call_still_frees_a_web_and_the_frost_trap_slow() {
    let defs = AbilityDefinitions::default();
    let scope = ally_removal(AbilityType::MastersCall)
        .expect("Master's Call frees allies")
        .scope;
    for ability in [AbilityType::SpiderWeb, AbilityType::FrostTrap] {
        assert!(
            !survives(
                landed_aura(&defs, ability),
                scope.clone(),
                CharacterClass::Hunter
            ),
            "Master's Call left {ability:?} on its ally"
        );
    }
}

/// User ruling, 2026-10-11: Psychic Scream is dispellable. A Priest's Dispel
/// Magic takes its fear off an ally, through the real `process_dispels`.
#[test]
fn dispel_magic_takes_psychic_scream_off_an_ally() {
    let defs = AbilityDefinitions::default();
    let scream = landed_aura(&defs, AbilityType::PsychicScream);
    assert_eq!(scream.effect_type, AuraType::Fear);
    assert!(
        !survives(
            scream,
            ally_dispel_scope(AbilityType::DispelMagic),
            CharacterClass::Priest
        ),
        "Dispel Magic left Psychic Scream's fear on the ally"
    );
}
