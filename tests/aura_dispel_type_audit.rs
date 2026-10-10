//! Every aura's removal class, checked against its Classic dispel type.
//!
//! An aura's removal class decides who can take it off: a dispel takes MAGIC, a
//! cleanse takes magic and POISON, nothing in the arena takes a CURSE, and no
//! removal takes a PHYSICAL effect. `abilities.ron` declares the class only
//! where the school gets it wrong (`dispel_type: Poison`, `dispel_type:
//! Curse`); everything else is derived from the school, and Nature, Shadow and
//! the rest derive to magic. So an ability that Classic calls a poison and that
//! forgets the declaration ships as dispellable magic, with nothing to say so.
//! That is how Serpent Sting came to be dispelled by Priests.
//!
//! [`expected`] is the guard: an EXPLICIT row per ability, written by hand, in
//! an exhaustive `match` with no wildcard arm. A new `AbilityType` does not
//! compile until someone writes its row, and the row has to state what Classic
//! calls the aura and what the sim resolves it to. The tests then check the row
//! against the aura the engine actually builds — from the RON for a config aura,
//! from the engine's own constructor for an aura built in code (traps, totems).
//!
//! **Where the sim and Classic disagree, the row says why.** A row whose two
//! classes disagree must carry a `divergence` reason, and a row whose classes
//! agree must not, so a divergence is always a decision someone wrote down and
//! never an accident the table absorbed.
//!
//! ## The Classic column
//!
//! `SpellCategories.DispelType` from the Classic client (wago.tools build
//! 1.15.9.69547) for the spell that carries the aura (the highest rank; the
//! ranks agree): 1 Magic, 2 Curse, 3 Disease, 4 Poison, 0 none. Where the
//! aura is a separate spell from the ability, the aura's spell is the one read
//! (Freezing Trap Effect 3355, Frost Trap Aura 13810). TBC abilities the Classic
//! client does not carry (Unstable Affliction, Lifebloom, Molten Armor,
//! Commanding Shout, Cyclone) take their TBC dispel type, marked `TBC`.
//!
//! Classic's "none" is satisfied by a PHYSICAL aura or by a schoolless one that
//! declares no class: either way no dispel, cleanse or purge takes it as magic.
//!
//! The class is the subject here, not the mechanic. Whether a dispel takes a
//! magic STUN (it does not — `AuraType::is_magic_dispellable`) is the aura
//! type's question, and is answered there.

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::ally_dispel_scope;
use arenasim::states::play_match::class_ai::shaman::totem_spec;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, DispelPending,
    DispelScope, GameRng, TotemElement,
};
use arenasim::states::play_match::effects::process_dispels;
use arenasim::states::play_match::totems::make_totem_aura;
use arenasim::states::play_match::traps::{freezing_trap_aura, frost_trap_slow_aura};
use arenasim::CharacterClass;

/// The removal class the sim resolves an aura to — the engine's own word for
/// it, [`Aura::removal_class_name`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sim {
    Magic,
    Poison,
    Curse,
    Disease,
    Physical,
    /// Schoolless and undeclared: the engine names no class.
    Unclassed,
}

/// Classic's `SpellCategories.DispelType` for the aura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Classic {
    Magic,
    Poison,
    Curse,
    #[allow(dead_code)] // no disease in the sim yet; the row type is ready for one
    Disease,
    /// DispelType 0.
    NoType,
}

impl Classic {
    fn agrees_with(self, sim: Sim) -> bool {
        match self {
            Classic::Magic => sim == Sim::Magic,
            Classic::Poison => sim == Sim::Poison,
            Classic::Curse => sim == Sim::Curse,
            Classic::Disease => sim == Sim::Disease,
            Classic::NoType => matches!(sim, Sim::Physical | Sim::Unclassed),
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
        classic: Classic,
        /// Why the sim departs from Classic. Required exactly when it does.
        divergence: Option<&'static str>,
    },
}

const fn config(sim: Sim, classic: Classic) -> Row {
    Row::Aura {
        source: Source::Config,
        sim,
        classic,
        divergence: None,
    }
}

const fn config_diverging(sim: Sim, classic: Classic, why: &'static str) -> Row {
    Row::Aura {
        source: Source::Config,
        sim,
        classic,
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

const TOTEM_DIVERGENCE: &str = "Classic totem auras carry no dispel type; the sim's pulse buffs \
     are Nature-school magic, so a purge can take the purgeable ones";

const PALADIN_AURA_DIVERGENCE: &str = "Classic Paladin auras carry no dispel type; the sim's \
     read as magic from their school";

/// The table. **No wildcard arm** — a new ability does not compile until its
/// row is written.
fn expected(ability: AbilityType) -> Row {
    use Classic as C;
    use Sim as S;
    match ability {
        // ---- Mage ----
        AbilityType::Frostbolt => config(S::Magic, C::Magic),
        AbilityType::FrostNova => config(S::Magic, C::Magic),
        AbilityType::ArcaneIntellect => config(S::Magic, C::Magic),
        AbilityType::IceBarrier => config(S::Magic, C::Magic),
        AbilityType::Polymorph => config(S::Magic, C::Magic),
        AbilityType::FrostArmor => config(S::Magic, C::Magic),
        AbilityType::MageArmorSpell => config(S::Magic, C::Magic),
        AbilityType::MoltenArmor => config(S::Magic, C::Magic), // TBC

        // ---- Priest ----
        AbilityType::FlashHeal => Row::NoAura,
        AbilityType::MindBlast => Row::NoAura,
        AbilityType::PowerWordFortitude => config(S::Magic, C::Magic),
        AbilityType::PowerWordShield => config(S::Magic, C::Magic),
        AbilityType::DispelMagic => Row::NoAura,
        AbilityType::PsychicScream => config_diverging(
            S::Magic,
            C::NoType,
            "Classic gives Psychic Scream (8122) no dispel type; the sim's fears are all \
             Shadow-school magic, so a dispel frees its victims",
        ),
        AbilityType::ManaBurn => Row::NoAura,

        // ---- Warrior ----
        AbilityType::HeroicStrike => Row::NoAura,
        AbilityType::Charge => Row::NoAura,
        AbilityType::Rend => config(S::Physical, C::NoType),
        AbilityType::MortalStrike => config(S::Physical, C::NoType),
        AbilityType::Pummel => Row::NoAura,
        AbilityType::BerserkerRage => config(S::Unclassed, C::NoType),
        AbilityType::BattleShout => config(S::Unclassed, C::NoType),
        AbilityType::DemoralizingShout => config(S::Unclassed, C::NoType),
        AbilityType::CommandingShout => config(S::Unclassed, C::NoType), // TBC

        // ---- Rogue ----
        AbilityType::Ambush => Row::NoAura,
        AbilityType::SinisterStrike => Row::NoAura,
        AbilityType::CheapShot => config(S::Physical, C::NoType),
        AbilityType::KidneyShot => config(S::Physical, C::NoType),
        AbilityType::Kick => Row::NoAura,
        AbilityType::CripplingPoison => config(S::Poison, C::Poison),

        // ---- Warlock ----
        AbilityType::Corruption => config(S::Magic, C::Magic),
        AbilityType::Shadowbolt => Row::NoAura,
        AbilityType::Fear => config(S::Magic, C::Magic),
        AbilityType::Immolate => config(S::Magic, C::Magic),
        AbilityType::DrainLife => Row::NoAura,
        AbilityType::CurseOfAgony => config(S::Curse, C::Curse),
        AbilityType::CurseOfWeakness => config(S::Curse, C::Curse),
        AbilityType::CurseOfTongues => config(S::Curse, C::Curse),
        AbilityType::UnstableAffliction => config(S::Magic, C::Magic), // TBC
        AbilityType::DeathCoil => config(S::Magic, C::Magic),
        AbilityType::SpellLock => Row::NoAura,
        AbilityType::DevourMagic => Row::NoAura,

        // ---- Paladin ----
        AbilityType::FlashOfLight => Row::NoAura,
        AbilityType::HolyLight => Row::NoAura,
        AbilityType::HolyShock => Row::NoAura,
        AbilityType::HammerOfJustice => config(S::Magic, C::Magic),
        AbilityType::PaladinCleanse => Row::NoAura,
        AbilityType::DivineShield => config(S::Unclassed, C::NoType),
        AbilityType::DevotionAura => config_diverging(S::Magic, C::NoType, PALADIN_AURA_DIVERGENCE),
        AbilityType::ShadowResistanceAura => {
            config_diverging(S::Magic, C::NoType, PALADIN_AURA_DIVERGENCE)
        }
        AbilityType::ConcentrationAura => {
            config_diverging(S::Magic, C::NoType, PALADIN_AURA_DIVERGENCE)
        }

        // ---- Hunter ----
        AbilityType::AimedShot => config(S::Physical, C::NoType),
        AbilityType::ArcaneShot => Row::NoAura,
        AbilityType::ConcussiveShot => config_diverging(
            S::Physical,
            C::Magic,
            "Classic's Concussive Shot daze (5116) is Magic; the sim reads an arrow in the \
             leg as physical, which no dispel takes (`DispelType::Physical`)",
        ),
        AbilityType::SerpentSting => config(S::Poison, C::Poison),
        AbilityType::Disengage => Row::NoAura,
        AbilityType::FreezingTrap => Row::Aura {
            source: Source::Engine(|| freezing_trap_aura(Entity::PLACEHOLDER)),
            sim: S::Magic,
            classic: C::Magic, // Freezing Trap Effect, 3355
            divergence: None,
        },
        AbilityType::FrostTrap => Row::Aura {
            source: Source::Engine(|| frost_trap_slow_aura(Entity::PLACEHOLDER, 0.5)),
            sim: S::Magic,
            classic: C::Magic, // Frost Trap Aura, 13810
            divergence: None,
        },
        AbilityType::Flare => Row::NoAura,

        // ---- Hunter pets ----
        AbilityType::SpiderWeb => config_diverging(
            S::Magic,
            C::NoType,
            "Classic's Web (4167) has no dispel type; the sim's is a Nature-school root, \
             which a dispel takes",
        ),
        AbilityType::BoarCharge => config(S::Physical, C::NoType),
        AbilityType::MastersCall => Row::NoAura,

        // ---- Shaman ----
        AbilityType::LightningBolt => Row::NoAura,
        AbilityType::FrostShock => config(S::Magic, C::Magic),
        AbilityType::LesserHealingWave => Row::NoAura,
        AbilityType::Purge => Row::NoAura,
        AbilityType::WindShear => Row::NoAura,
        AbilityType::AirTotem => Row::Aura {
            source: Source::Engine(|| totem(TotemElement::Air)),
            sim: S::Magic,
            classic: C::NoType,
            divergence: Some(TOTEM_DIVERGENCE),
        },
        AbilityType::WaterTotem => Row::Aura {
            source: Source::Engine(|| totem(TotemElement::Water)),
            sim: S::Magic,
            classic: C::NoType,
            divergence: Some(TOTEM_DIVERGENCE),
        },
        AbilityType::EarthTotem => Row::Aura {
            source: Source::Engine(|| totem(TotemElement::Earth)),
            sim: S::Magic,
            classic: C::NoType,
            divergence: Some(TOTEM_DIVERGENCE),
        },
        AbilityType::FireTotem => Row::Aura {
            source: Source::Engine(|| totem(TotemElement::Fire)),
            sim: S::Magic,
            classic: C::NoType,
            divergence: Some(TOTEM_DIVERGENCE),
        },

        // ---- Druid ----
        AbilityType::Rejuvenation => config(S::Magic, C::Magic),
        AbilityType::Lifebloom => config(S::Magic, C::Magic), // TBC
        AbilityType::Swiftmend => Row::NoAura,
        AbilityType::Moonfire => config(S::Magic, C::Magic),
        AbilityType::MarkOfTheWild => config(S::Magic, C::Magic),
        AbilityType::Innervate => config(S::Magic, C::Magic),
        AbilityType::EntanglingRoots => config(S::Magic, C::Magic),
        AbilityType::Cyclone => config_diverging(
            S::Magic,
            C::NoType, // TBC
            "Cyclone takes its class from the Nature school, but no dispel takes it by RULE \
             (`AuraType::Cyclone` is never a dispel candidate)",
        ),
        AbilityType::TravelForm => config_diverging(
            S::Magic,
            C::NoType,
            "a shapeshift takes its class from the Nature school, but `TravelForm` is no \
             dispel or purge candidate by type",
        ),
    }
}

fn sim_class(aura: &Aura) -> Sim {
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
fn every_divergence_from_classic_is_explained() {
    let defs = AbilityDefinitions::default();
    let mut problems = Vec::new();
    let mut divergent = Vec::new();

    for (&ability, _) in defs.iter() {
        let Row::Aura {
            sim,
            classic,
            divergence,
            ..
        } = expected(ability)
        else {
            continue;
        };
        match (classic.agrees_with(sim), divergence) {
            (true, Some(_)) => problems.push(format!(
                "{ability:?}: {sim:?} agrees with Classic {classic:?} but carries a divergence reason"
            )),
            (false, None) => problems.push(format!(
                "{ability:?}: the sim's {sim:?} departs from Classic {classic:?} with no reason given"
            )),
            (false, Some(_)) => divergent.push(format!("{ability:?}")),
            (true, None) => {}
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // The divergences are a fixed, named set: a new one is a decision, not drift.
    divergent.sort();
    assert_eq!(
        divergent,
        [
            "AirTotem",
            "ConcentrationAura",
            "ConcussiveShot",
            "Cyclone",
            "DevotionAura",
            "EarthTotem",
            "FireTotem",
            "PsychicScream",
            "ShadowResistanceAura",
            "SpiderWeb",
            "TravelForm",
            "WaterTotem",
        ]
    );
}

/// Classic's poisons are the sim's poisons, and nothing else is. Cleanse is the
/// only removal that reaches a poison, so this set is exactly what a Priest's
/// Dispel Magic and a Felhunter's Devour Magic may not touch.
#[test]
fn the_poisons_are_crippling_poison_and_serpent_sting() {
    let defs = AbilityDefinitions::default();
    let mut poisons: Vec<String> = defs
        .iter()
        .filter_map(|(&ability, _)| config_aura(&defs, ability).map(|a| (ability, a)))
        .filter(|(_, aura)| aura.is_cleansable_poison())
        .map(|(ability, _)| format!("{ability:?}"))
        .collect();
    poisons.sort();
    assert_eq!(poisons, ["CripplingPoison", "SerpentSting"]);
}

// ---------------------------------------------------------------------------
// Serpent Sting against every removal, through the real `process_dispels`
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

/// Serpent Sting exactly as the Hunter lands it, on a Warrior, then one removal
/// of `scope` queued against it. Returns whether the sting survived.
fn sting_survives(scope: DispelScope, dispeller_class: CharacterClass) -> bool {
    let defs = AbilityDefinitions::default();
    let sting =
        config_aura(&defs, AbilityType::SerpentSting).expect("Serpent Sting applies an aura");
    assert_eq!(sting.effect_type, AuraType::DamageOverTime);

    let mut app = harness();
    let victim = app
        .world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            ActiveAuras { auras: vec![sting] },
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
        .is_some_and(|auras| {
            auras
                .auras
                .iter()
                .any(|a| a.ability_name == "Serpent Sting")
        })
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

/// Purge is offensive and magic-only: pinned to the sting's own type, it still
/// finds nothing to take.
#[test]
fn shaman_purge_cannot_take_serpent_sting() {
    assert!(sting_survives(
        DispelScope::Purge(AuraType::DamageOverTime),
        CharacterClass::Shaman
    ));
    assert!(sting_survives(
        DispelScope::PurgeSource {
            effect: AuraType::DamageOverTime,
            source: "Serpent Sting".to_string(),
            owner: None,
        },
        CharacterClass::Shaman
    ));
}
