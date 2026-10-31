//! The Rogue's four weapon poisons (AS-330): Crippling (the default), Deadly,
//! Wound and Mind-numbing — a per-match choice, rolled once per landed weapon
//! hit at the poison's own `application_chance`.
//!
//! Three layers, each pinned where it is decided:
//! - the CHOICE reaches the proc: a played match coats the Rogue with the
//!   poison its config names, and with no other;
//! - each poison's aura behaves as its RON says when it lands through the one
//!   aura funnel (`apply_pending_auras`): Deadly stacks to five, and Wound and
//!   Mind-numbing refresh without doubling (one application is the whole
//!   effect — user ruling, 2026-10-30);
//! - what each one DOES once it is up: Wound halves healing, and only the
//!   strongest healing reduction applies; Mind-numbing slows casting, and only
//!   the larger cast-time increase applies; Deadly ticks per stack.

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
/// chance: Deadly 30%, the others 50% (kept high by ruling; TBC's are lower).
#[test]
fn every_poison_is_an_ability_with_its_chance() {
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

/// The `[t] ... <Poison> procs on <target>` lines of a log, as (time, target).
fn proc_lines(log: &str, poison: RoguePoison) -> Vec<(String, String)> {
    let marker = format!("{} procs on ", poison.name());
    log.lines()
        .filter_map(|l| {
            let (_, target) = l.split_once(&marker)?;
            let time = l.split(']').next()?.to_string();
            Some((time, target.to_string()))
        })
        .collect()
}

/// Both of a dual-wielding Rogue's hands can land, and proc, in one frame,
/// before the first proc's aura has reached the funnel. The first application
/// is still logged once. The situation — a first application in a frame where
/// both hands landed on that target — is counted, so the test cannot pass on
/// a bank where it never happened; with both hands proccing at 50%, about half
/// of those frames carry two procs.
#[test]
fn a_first_application_from_both_hands_logs_once() {
    let mut both_hands_frames = 0;
    for poison in [RoguePoison::Wound, RoguePoison::MindNumbing] {
        for seed in 0..8 {
            let played = common::staged::play_seeded(HeadlessMatchConfig {
                team1: vec!["Rogue".into(), "Paladin".into()],
                team2: vec!["Warrior".into(), "Paladin".into()],
                team1_rogue_poisons: vec![format!("{poison:?}")],
                random_seed: Some(seed),
                max_duration_secs: 90.0,
                ..Default::default()
            });
            let procs = proc_lines(&played.log, poison);
            let mut seen = std::collections::BTreeSet::new();
            for (time, target) in &procs {
                assert!(
                    seen.insert((time.clone(), target.clone())),
                    "{poison:?} seed {seed}: two first applications on {target} at {time}"
                );
                let hands = played
                    .log
                    .lines()
                    .filter(|l| {
                        l.starts_with(time.as_str())
                            && l.contains("Team 1 Rogue #1's Auto Attack")
                            && l.contains(target.as_str())
                    })
                    .count();
                if hands >= 2 {
                    both_hands_frames += 1;
                }
            }
        }
    }
    assert!(
        both_hands_frames >= 10,
        "only {both_hands_frames} first applications landed with both hands"
    );
}

// ── each poison's aura, through the funnel ──────────────────────────────────

/// Deadly gains a stack per application up to five, and stays there; Wound
/// and Mind-numbing refresh, never doubling (one application is the whole
/// effect). Each is one aura per target.
#[test]
fn deadly_stacks_to_five_and_wound_and_mind_numbing_refresh() {
    for (ability, cap) in [
        (AbilityType::DeadlyPoison, 5),
        (AbilityType::WoundPoison, 1),
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

/// The healing-reduction aura `ability` lands.
fn reduction(ability: AbilityType) -> Aura {
    let mut world = world();
    let caster = unit(&mut world, 1, CharacterClass::Rogue);
    let target = unit(&mut world, 2, CharacterClass::Mage);
    let aura = aura_of(ability, target, caster);
    assert_eq!(aura.effect_type, AuraType::HealingReduction, "{ability:?}");
    aura
}

/// One Wound Poison application halves healing received — TBC's five-stack
/// maximum, all at once.
#[test]
fn one_wound_poison_halves_healing() {
    assert_eq!(healed(vec![], 200.0), 200.0, "control: no poison");
    assert_eq!(
        healed(vec![reduction(AbilityType::WoundPoison)], 200.0),
        100.0
    );
}

/// Only the strongest healing reduction applies. Mortal Strike's and Aimed
/// Shot's Mortal Wound (35% each) together still cut 35%, and Wound Poison
/// (50%) under either cuts 50%, whichever order they sit in.
#[test]
fn only_the_strongest_healing_reduction_applies() {
    let (ms, aimed, wound) = (
        reduction(AbilityType::MortalStrike),
        reduction(AbilityType::AimedShot),
        reduction(AbilityType::WoundPoison),
    );
    assert_eq!(healed(vec![ms.clone()], 200.0), 200.0 * 0.65, "control");
    assert_eq!(healed(vec![ms.clone(), aimed.clone()], 200.0), 200.0 * 0.65);
    assert_eq!(healed(vec![ms.clone(), wound.clone()], 200.0), 100.0);
    assert_eq!(healed(vec![wound, aimed, ms], 200.0), 100.0);
}

/// Mind-numbing Poison adds 60% to the bearer's cast times. Only the larger
/// cast-time increase applies: under Curse of Tongues (50%) it is still 60%,
/// and Tongues alone is 50%.
#[test]
fn only_the_larger_cast_time_increase_applies() {
    let mut world = world();
    let rogue = unit(&mut world, 1, CharacterClass::Rogue);
    let warlock = unit(&mut world, 1, CharacterClass::Warlock);
    let mage = unit(&mut world, 2, CharacterClass::Mage);
    let priest = unit(&mut world, 2, CharacterClass::Priest);
    assert_eq!(get_cast_time_increase(None), 0.0, "control");
    land(
        &mut world,
        priest,
        aura_of(AbilityType::CurseOfTongues, priest, warlock),
    );
    let tongues = get_cast_time_increase(world.entity(priest).get::<ActiveAuras>());
    assert!((tongues - 0.5).abs() < 1e-6, "{tongues}");
    for _ in 0..2 {
        land(
            &mut world,
            mage,
            aura_of(AbilityType::MindNumbingPoison, mage, rogue),
        );
    }
    let increase = || get_cast_time_increase(world.entity(mage).get::<ActiveAuras>());
    assert!((increase() - 0.6).abs() < 1e-6, "{}", increase());
    land(
        &mut world,
        mage,
        aura_of(AbilityType::CurseOfTongues, mage, warlock),
    );
    let both = get_cast_time_increase(world.entity(mage).get::<ActiveAuras>());
    assert!((both - 0.6).abs() < 1e-6, "{both}");
}

// ── what the encyclopedia and View Combatant say ───────────────────────────

/// Deadly Poison says it stacks, and how deep; an aura that does not stack
/// (Mortal Wound) and the refresh-only poisons (Wound, Mind-numbing) do not.
#[test]
fn the_poison_texts_state_their_effect() {
    use arenasim::states::ability_text::build_aura_description;
    let defs = AbilityDefinitions::default();
    let text = |a: AbilityType| {
        build_aura_description(defs.get_unchecked(&a).applies_aura.as_ref().unwrap())
    };
    assert_eq!(
        text(AbilityType::DeadlyPoison),
        "Deals 13 damage over 12 sec. Stacks up to 5 times."
    );
    assert_eq!(
        text(AbilityType::WoundPoison),
        "Reduces healing received by 50% for 15 sec."
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
