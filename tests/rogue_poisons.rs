//! The Rogue's four weapon poisons (AS-330): Crippling (the default), Deadly,
//! Wound and Mind-numbing — a per-match choice, rolled once per landed weapon
//! hit at the poison's own `application_chance`.
//!
//! Three layers, each pinned where it is decided:
//! - the CHOICE reaches the proc: a played match coats the Rogue with the
//!   poison its config names, and with no other;
//! - each poison's aura behaves as its RON says when it lands through the one
//!   aura funnel (`apply_pending_auras`): Deadly and Wound stack to five, and
//!   Mind-numbing refreshes without doubling;
//! - what each one DOES once it is up: Wound cuts healing per stack and
//!   compounds with Mortal Strike's Mortal Wound; Mind-numbing sums with Curse
//!   of Tongues; Deadly ticks per stack.

mod common;

use bevy::ecs::system::RunSystemOnce;
use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::match_config::{CharacterClass, RoguePoison};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::combat_core::{apply_healing, get_cast_time_increase};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, DRTracker,
};

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::components::GameRng;

// ── scaffolding ─────────────────────────────────────────────────────────────

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(AbilityDefinitions::default());
    world
}

fn unit(world: &mut World, team: u8, class: CharacterClass) -> Entity {
    world
        .spawn((
            Combatant::new(team, 0, class),
            Transform::default(),
            DRTracker::default(),
        ))
        .id()
}

/// The aura `ability` lands from `caster`, exactly as the RON describes it.
fn aura_of(ability: AbilityType, target: Entity, caster: Entity) -> Aura {
    let defs = AbilityDefinitions::default();
    AuraPending::from_ability(target, caster, defs.get_unchecked(&ability))
        .expect("the ability applies an aura")
        .aura
}

/// Land `aura` through the one aura funnel.
fn land(world: &mut World, target: Entity, aura: Aura) {
    world.spawn(AuraPending { target, aura });
    world.run_system_once(apply_pending_auras).unwrap();
}

/// The auras on `entity` named `name`.
fn named<'a>(world: &'a World, entity: Entity, name: &str) -> Vec<&'a Aura> {
    world
        .entity(entity)
        .get::<ActiveAuras>()
        .map(|a| {
            a.auras
                .iter()
                .filter(|aura| aura.ability_name == name)
                .collect()
        })
        .unwrap_or_default()
}

/// Heal `amount` onto a fresh 100/1000-health target carrying `auras`, and
/// return the health it gained.
fn healed(auras: Vec<Aura>, amount: f32) -> f32 {
    let mut target = Combatant::new(1, 0, CharacterClass::Warrior);
    target.max_health = 1000.0;
    target.current_health = 100.0;
    let world = World::new();
    let mut queue = CommandQueue::default();
    let mut commands = Commands::new(&mut queue, &world);
    apply_healing(
        &mut commands,
        Entity::PLACEHOLDER,
        &mut target,
        Some(&ActiveAuras { auras }),
        &ArenaDampening::default(),
        amount,
    )
}

// ── the config: each poison is an ability with its own chance ──────────────

/// Every poison names its own ability, every one of those carries a per-hit
/// chance, and the chances are TBC's: Deadly 30%, the others 50%.
#[test]
fn every_poison_is_an_ability_with_its_tbc_chance() {
    let defs = AbilityDefinitions::default();
    let chances: Vec<(RoguePoison, f32)> = RoguePoison::ALL
        .into_iter()
        .map(|p| {
            let def = defs.get_unchecked(&p.ability());
            assert_eq!(def.name, p.name(), "{p:?} names its ability");
            (p, def.application_chance.expect("a weapon poison rolls"))
        })
        .collect();
    assert_eq!(
        chances,
        [
            (RoguePoison::Crippling, 0.5),
            (RoguePoison::Deadly, 0.3),
            (RoguePoison::Wound, 0.5),
            (RoguePoison::MindNumbing, 0.5),
        ]
    );
}

/// The headless config takes each poison by its variant or display name, and
/// refuses one it does not know rather than quietly playing Crippling.
#[test]
fn the_headless_config_names_every_poison_and_refuses_a_stranger() {
    let parse = |names: &[&str]| {
        HeadlessMatchConfig {
            team1: vec!["Rogue".into()],
            team2: vec!["Warrior".into()],
            team1_rogue_poisons: names.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
        .to_match_config()
        .map(|c| c.team1_rogue_poisons)
    };
    for poison in RoguePoison::ALL {
        assert_eq!(parse(&[&format!("{poison:?}")]), Ok(vec![poison]));
        assert_eq!(parse(&[poison.name()]), Ok(vec![poison]));
    }
    assert_eq!(parse(&[]), Ok(vec![RoguePoison::Crippling]), "the default");
    assert!(parse(&["Instant"]).is_err(), "an unknown poison is refused");
}

// ── the choice reaches the proc ─────────────────────────────────────────────

/// The lines the combat log gives a poison's application. Crippling keeps its
/// long-standing line; the others log their first proc on a target.
fn applied_line(poison: RoguePoison) -> String {
    match poison {
        RoguePoison::Crippling => "Crippling Poison applied to".to_string(),
        other => format!("{} procs on", other.name()),
    }
}

/// A Rogue coated with each poison applies THAT poison to the enemies it hits,
/// and never another; and a poison other than Crippling lands through the aura
/// funnel, whose refresh line only it writes. The situation — weapon hits
/// landing on an enemy — is asserted to have occurred, so the test cannot pass
/// on a Rogue that never swung. A healer on each side keeps the match long.
#[test]
fn the_chosen_poison_is_the_one_the_rogue_applies() {
    for poison in RoguePoison::ALL {
        let played = common::staged::play_seeded(HeadlessMatchConfig {
            team1: vec!["Rogue".into(), "Priest".into()],
            team2: vec!["Warrior".into(), "Priest".into()],
            team1_rogue_poisons: vec![format!("{poison:?}")],
            random_seed: Some(11),
            max_duration_secs: 90.0,
            ..Default::default()
        });
        let hits = played
            .log
            .lines()
            .filter(|l| l.contains("Team 1 Rogue #1's Auto Attack hits Team 2"))
            .count();
        assert!(hits >= 15, "{poison:?}: the Rogue landed only {hits} hits");
        for other in RoguePoison::ALL {
            let applied = played.log.contains(&applied_line(other));
            assert_eq!(
                applied,
                other == poison,
                "{poison:?} coated: {other:?} applied = {applied}"
            );
        }
        if poison != RoguePoison::Crippling {
            let refreshed = format!("{} refreshed", poison.name());
            assert!(
                played.log.contains(&refreshed),
                "{poison:?} never reached the funnel's refresh"
            );
        }
    }
}

// ── each poison's aura, through the funnel ──────────────────────────────────

/// Deadly and Wound gain a stack per application up to five, and stay there;
/// Mind-numbing refreshes, never doubling. All three are one aura per target.
#[test]
fn deadly_and_wound_stack_to_five_and_mind_numbing_refreshes() {
    for (ability, cap) in [
        (AbilityType::DeadlyPoison, 5),
        (AbilityType::WoundPoison, 5),
        (AbilityType::MindNumbingPoison, 1),
    ] {
        let mut world = world();
        let rogue = unit(&mut world, 1, CharacterClass::Rogue);
        let mage = unit(&mut world, 2, CharacterClass::Mage);
        let name = AbilityDefinitions::default()
            .get_unchecked(&ability)
            .name
            .clone();
        for applied in 1..=7u8 {
            land(&mut world, mage, aura_of(ability, mage, rogue));
            let auras = named(&world, mage, &name);
            assert_eq!(auras.len(), 1, "{ability:?}: one aura after {applied}");
            assert_eq!(auras[0].stack_count(), applied.min(cap), "{ability:?}");
        }
    }
}

/// A Deadly Poison tick is its per-stack magnitude times its stacks.
#[test]
fn deadly_poison_ticks_per_stack() {
    let mut world = world();
    let rogue = unit(&mut world, 1, CharacterClass::Rogue);
    let mage = unit(&mut world, 2, CharacterClass::Mage);
    for _ in 0..3 {
        land(
            &mut world,
            mage,
            aura_of(AbilityType::DeadlyPoison, mage, rogue),
        );
    }
    let auras = named(&world, mage, "Deadly Poison");
    assert_eq!(auras[0].effect_type, AuraType::DamageOverTime);
    assert_eq!(auras[0].tick_amount(), 3.0 * auras[0].magnitude);
}

// ── what each one does ──────────────────────────────────────────────────────

fn wound_poison(stacks: u8) -> Aura {
    let mut world = world();
    let rogue = unit(&mut world, 1, CharacterClass::Rogue);
    let mage = unit(&mut world, 2, CharacterClass::Mage);
    for _ in 0..stacks {
        land(
            &mut world,
            mage,
            aura_of(AbilityType::WoundPoison, mage, rogue),
        );
    }
    named(&world, mage, "Wound Poison")[0].clone()
}

/// Wound Poison cuts 10% of healing received a stack: a 200 heal lands as
/// 180, 160, 140, 120 and 100 under one to five stacks, against 200 with none.
#[test]
fn wound_poison_cuts_healing_ten_percent_a_stack() {
    assert_eq!(healed(vec![], 200.0), 200.0, "control: no poison");
    let landed: Vec<f32> = (1..=5)
        .map(|n| healed(vec![wound_poison(n)], 200.0).round())
        .collect();
    assert_eq!(landed, [180.0, 160.0, 140.0, 120.0, 100.0]);
}

/// Wound Poison and Mortal Strike's Mortal Wound compound, as every two
/// healing reductions do: five stacks (x0.5) under Mortal Wound (x0.65) leave
/// a 200 heal at 65. Mortal Wound alone still leaves exactly 130.
#[test]
fn wound_poison_compounds_with_mortal_wound() {
    let mut world = world();
    let warrior = unit(&mut world, 1, CharacterClass::Warrior);
    let mage = unit(&mut world, 2, CharacterClass::Mage);
    let mortal_wound = aura_of(AbilityType::MortalStrike, mage, warrior);
    assert_eq!(mortal_wound.effect_type, AuraType::HealingReduction);
    assert_eq!(healed(vec![mortal_wound.clone()], 200.0), 200.0 * 0.65);
    let both = healed(vec![mortal_wound, wound_poison(5)], 200.0);
    assert_eq!(both.round(), 65.0);
}

/// Mind-numbing Poison adds 60% to the bearer's cast times, and sums with
/// Curse of Tongues (50%) to 110%, as any two cast-time increases do.
#[test]
fn mind_numbing_poison_slows_casting_and_sums_with_tongues() {
    let mut world = world();
    let rogue = unit(&mut world, 1, CharacterClass::Rogue);
    let warlock = unit(&mut world, 1, CharacterClass::Warlock);
    let mage = unit(&mut world, 2, CharacterClass::Mage);
    assert_eq!(get_cast_time_increase(None), 0.0, "control");
    land(
        &mut world,
        mage,
        aura_of(AbilityType::MindNumbingPoison, mage, rogue),
    );
    land(
        &mut world,
        mage,
        aura_of(AbilityType::MindNumbingPoison, mage, rogue),
    );
    let increase = || get_cast_time_increase(world.entity(mage).get::<ActiveAuras>());
    assert!((increase() - 0.6).abs() < 1e-6, "{}", increase());
    land(
        &mut world,
        mage,
        aura_of(AbilityType::CurseOfTongues, mage, warlock),
    );
    let increase = get_cast_time_increase(world.entity(mage).get::<ActiveAuras>());
    assert!((increase - 1.1).abs() < 1e-6, "{increase}");
}

// ── what the encyclopedia and View Combatant say ───────────────────────────

/// A poison whose numbers are per stack says so, and how deep it goes; an
/// aura that does not stack (Mortal Wound) and a refresh-only one
/// (Mind-numbing) say neither.
#[test]
fn stacking_poisons_describe_their_stacks() {
    use arenasim::states::ability_text::build_aura_description;
    let defs = AbilityDefinitions::default();
    let text = |a: AbilityType| {
        build_aura_description(defs.get_unchecked(&a).applies_aura.as_ref().unwrap())
    };
    assert_eq!(
        text(AbilityType::WoundPoison),
        "Reduces healing received by 10% a stack for 15 sec. Stacks up to 5 times."
    );
    assert_eq!(
        text(AbilityType::DeadlyPoison),
        "Deals 13 damage over 12 sec. Stacks up to 5 times."
    );
    assert_eq!(
        text(AbilityType::MortalStrike),
        "Reduces healing received by 35% for 10 sec."
    );
    assert_eq!(
        text(AbilityType::MindNumbingPoison),
        "Increases cast time by 60% for 10 sec."
    );
}
