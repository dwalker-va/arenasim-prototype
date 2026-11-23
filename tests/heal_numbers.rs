//! AS-508 — one rule for every heal's floating number.
//!
//! A heal floats the amount it landed, as the combat log prints it; a heal
//! that landed nothing, because its target was at full health, floats no
//! number (`heal_numbers::heal_number_text`). Every heal number is spawned
//! through `heal_numbers`, whichever family the heal belongs to. Here: the
//! direct heals (a cast's, Holy Shock's and Holy Nova's), heal over time
//! ticks, blooms and leech heals, staged at each site, and whole seeded
//! matches; and a reader of `src/` that fails on the heal green named outside
//! `heal_numbers`. A killing-blow save's heal is pinned in
//! `tests/guardian_spirit_visual_probes.rs`.
//!
//! Each match here is played through the production schedule, with an
//! observer that records every floating number as it is spawned: the
//! combat-log clock, its text, whether it is a heal's green, and the unit it
//! floats over (read off its anchor: a number stands at its unit's x and z,
//! moved along x by its pattern's offset only). The claim is checked against
//! the combat log the same match wrote: at every moment, over every unit, the
//! green numbers are exactly the heal lines naming that unit then whose amount
//! is not 0 — amount for amount and crit for crit. So a fully overhealed heal
//! (a "for 0" line) floats nothing, and a heal that landed part of itself
//! floats what it landed.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::class_ai::QueuedAoeHeal;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, CastingState, ChannelingState, Combatant,
    FloatingCombatText, HolyShockHealPending, Pet, PlayMatchEntity,
};
use arenasim::states::play_match::instant_landings::InstantLandings;
use arenasim::states::play_match::reference_builds::BuildSpec;
use arenasim::states::play_match::talents::{Build, TalentId};
use arenasim::states::play_match::utils::{combat_log_id_for, FCT_HORIZONTAL_SPREAD};
use arenasim::states::play_match::AbilityType;
use common::source_audit::{
    blank_comments_and_strings, blank_test_modules, load_sources, use_statements,
};
use common::staged::{play, Played};
use regex::Regex;

/// One floating number as it was spawned.
#[derive(Clone, Debug)]
struct Number {
    /// The combat-log clock, as the log prints it.
    stamp: String,
    /// The unit it floats over, by its combat-log name.
    over: String,
    text: String,
    heal: bool,
    crit: bool,
}

/// A stage step that records every floating number spawned from here on into
/// `sink`.
fn record_numbers(sink: Arc<Mutex<Vec<Number>>>) -> impl FnOnce(&mut World) + Send + Sync {
    move |world: &mut World| {
        world.add_observer(
            move |trigger: Trigger<OnAdd, FloatingCombatText>,
                  texts: Query<&FloatingCombatText>,
                  log: Res<CombatLog>,
                  units: Query<(&Combatant, &Transform, Option<&Pet>)>| {
                let text = texts.get(trigger.target()).expect("the number");
                let at = text.world_position;
                let offsets = [
                    0.0,
                    FCT_HORIZONTAL_SPREAD * 0.4,
                    FCT_HORIZONTAL_SPREAD * -0.4,
                ];
                let over: Vec<String> = units
                    .iter()
                    .filter(|(_, t, _)| {
                        (at.z - t.translation.z).abs() < 1e-3
                            && offsets
                                .iter()
                                .any(|o| (at.x - o - t.translation.x).abs() < 1e-3)
                    })
                    .map(|(c, _, pet)| combat_log_id_for(c, pet).to_string())
                    .collect();
                assert_eq!(
                    over.len(),
                    1,
                    "the number {:?} at {at} stands over exactly one unit: {over:?}",
                    text.text
                );
                sink.lock().unwrap().push(Number {
                    stamp: format!("{:.2}", log.match_time),
                    over: over[0].clone(),
                    text: text.text.clone(),
                    heal: text.color == arenasim::ui::theme::fct::HEAL,
                    crit: text.is_crit,
                });
            },
        );
    }
}

/// One heal line of the log: when, which ability, who it healed, how much.
#[derive(Clone, Debug)]
struct HealLine {
    stamp: String,
    ability: String,
    to: String,
    amount: String,
    crit: bool,
    /// A bloom's line ("<caster>'s <aura> blooms on <target> for N").
    bloom: bool,
}

/// Devour Magic's self heal floats no number of any kind, and never has: its
/// site (`process_dispels`) logs the heal and spawns no floating text. It is
/// the one heal line the comparison sets aside.
const FLOATS_NO_NUMBER: &str = " from Devour Magic";

/// Every heal line of `log`. A line in a shape this cannot read fails the
/// test, so no heal is quietly left out of the comparison.
fn heal_lines(log: &str) -> Vec<HealLine> {
    log.lines()
        .filter(|l| l.contains("] [HEAL] ") && !l.contains(FLOATS_NO_NUMBER))
        .map(|l| {
            let stamp = l[1..l.find("s]").expect("a stamp")].trim().to_string();
            let message = &l[l.find("] [HEAL] ").unwrap() + "] [HEAL] ".len()..];
            let unread = || panic!("an unread heal line: {l}");
            let (head, amount) = message.rsplit_once(" for ").unwrap_or_else(unread);
            let crit = head.contains(" CRITICALLY heals");
            let bloom = head.contains(" blooms on ");
            let (caster, rest) = head.split_once("'s ").unwrap_or((head, ""));
            let (ability, to) = if let Some(ability) = rest.strip_suffix(" heals") {
                // A heal its caster landed on itself: "<caster>'s <ability> heals for N".
                (ability, caster)
            } else if let Some((ability, to)) = rest.rsplit_once(" blooms on ") {
                (ability, to)
            } else if let Some((ability, to)) = rest
                .rsplit_once(" CRITICALLY heals ")
                .or_else(|| rest.rsplit_once(" heals "))
            {
                (ability, to)
            } else if let Some((_, to)) = head.rsplit_once(" heals ") {
                // A save's heal: "[SAVED] <save> heals <holder> for N".
                ("save", to)
            } else {
                unread()
            };
            HealLine {
                stamp,
                ability: ability.to_string(),
                to: to.to_string(),
                amount: amount
                    .split_whitespace()
                    .next()
                    .expect("an amount")
                    .to_string(),
                crit,
                bloom,
            }
        })
        .collect()
}

/// A played match and every number it floated.
struct Run {
    played: Played,
    numbers: Vec<Number>,
}

impl Run {
    fn play(config: HeadlessMatchConfig, stage: Stage) -> Self {
        let sink = Arc::new(Mutex::new(Vec::new()));
        let played = play(config, stage.world(record_numbers(sink.clone())));
        let numbers = sink.lock().unwrap().clone();
        Run { played, numbers }
    }

    fn heals(&self) -> Vec<HealLine> {
        heal_lines(&self.played.log)
    }

    /// The heal lines of `ability`.
    fn lines_of(&self, ability: &str) -> Vec<HealLine> {
        self.heals()
            .into_iter()
            .filter(|h| h.ability == ability)
            .collect()
    }

    /// The green numbers over `unit` at `stamp`.
    fn numbers_at(&self, stamp: &str, unit: &str) -> Vec<&str> {
        self.numbers
            .iter()
            .filter(|n| n.heal && n.stamp == stamp && n.over == unit)
            .map(|n| n.text.as_str())
            .collect()
    }

    /// The green numbers over `unit` at `stamp`, with their crit flags.
    fn crits_at(&self, stamp: &str, unit: &str) -> Vec<(String, bool)> {
        self.numbers
            .iter()
            .filter(|n| n.heal && n.stamp == stamp && n.over == unit)
            .map(|n| (n.text.clone(), n.crit))
            .collect()
    }

    /// At every (moment, unit) a heal line or a green number names, the green
    /// numbers are exactly the heal lines whose amount is not 0, each with
    /// the line's amount and crit. Returns (the heal lines checked, those of
    /// them that landed 0).
    fn assert_heal_numbers_follow_the_rule(&self) -> (usize, usize) {
        let heals = self.heals();
        let mut lines: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        let mut floats: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for h in &heals {
            let entry = lines.entry((h.stamp.clone(), h.to.clone())).or_default();
            if h.amount != "0" {
                entry.push(format!("+{} crit={}", h.amount, h.crit));
            }
            floats.entry((h.stamp.clone(), h.to.clone())).or_default();
        }
        for n in self.numbers.iter().filter(|n| n.heal) {
            lines.entry((n.stamp.clone(), n.over.clone())).or_default();
            floats
                .entry((n.stamp.clone(), n.over.clone()))
                .or_default()
                .push(format!("{} crit={}", n.text, n.crit));
        }
        for v in lines.values_mut().chain(floats.values_mut()) {
            v.sort();
        }
        assert_eq!(
            floats, lines,
            "the green numbers over each unit, against its heal lines that landed something"
        );
        let zero = heals.iter().filter(|h| h.amount == "0").count();
        (heals.len(), zero)
    }
}

/// `who`'s health set `missing` below its maximum.
fn missing(who: Unit, missing: f32) -> impl FnOnce(&mut World) + Send + Sync {
    move |world: &mut World| {
        let e = who.entity(world);
        let mut c = world.get_mut::<Combatant>(e).unwrap();
        c.current_health = c.max_health - missing;
    }
}

/// A stun no damage breaks, for `secs`.
fn held(secs: f32) -> Aura {
    Aura {
        effect_type: AuraType::Stun,
        duration: secs,
        break_on_damage_threshold: -1.0,
        ability_name: "Staged Stun".into(),
        ..Default::default()
    }
}

/// `caster`'s aura of `ability` on `target`, as the ability applies it.
fn aura_of(world: &mut World, ability: AbilityType, target: Entity, caster: Entity) -> Aura {
    let def = world
        .get::<Combatant>(caster)
        .unwrap()
        .book()
        .get(&ability)
        .expect("the caster's book holds the ability")
        .clone();
    AuraPending::from_ability(target, caster, &def)
        .expect("the ability applies an aura")
        .aura
}

// ---------------------------------------------------------------------------
// A healer and its Warrior against a Mage held out of the fight
// ---------------------------------------------------------------------------

const HEALER: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(1, 1);
const MAGE: Unit = Unit::new(2, 0);

const WARRIOR_ID: &str = "Team 1 Warrior #2";

/// `healer` and a Warrior against a Mage held stunned out of reach the whole
/// match, so nothing but the stage hurts or heals anyone.
fn party(healer: &str, secs: f32, more: impl FnOnce(Stage) -> Stage) -> Run {
    party_built(healer, None, secs, more)
}

/// [`party`], the healer on `build`.
fn party_built(
    healer: &str,
    build: Option<BuildSpec>,
    secs: f32,
    more: impl FnOnce(Stage) -> Stage,
) -> Run {
    let config = HeadlessMatchConfig {
        team1: vec![healer.into(), "Warrior".into()],
        team2: vec!["Mage".into()],
        random_seed: Some(508),
        max_duration_secs: secs,
        team1_builds: build.map(|b| vec![b]).unwrap_or_default(),
        ..Default::default()
    };
    let stage = Stage::new()
        .place(HEALER, -10.0, 0.0)
        .place(WARRIOR, -6.0, 2.0)
        .place(MAGE, 30.0, 0.0)
        .aura(MAGE, held(secs + 5.0));
    Run::play(config, more(stage))
}

// --- Direct heal: a Flash Heal that lands as the gates open ---------------

/// The Priest's Flash Heal on its Warrior, staged a frame or two from landing.
fn flash_heal(warrior_missing: f32) -> Run {
    party("Priest", 2.0, |s| {
        s.world(missing(WARRIOR, warrior_missing)).world(|world| {
            let target = WARRIOR.entity(world);
            let priest = HEALER.entity(world);
            world.entity_mut(priest).insert(CastingState::new(
                AbilityType::FlashHeal,
                target,
                0.02,
            ));
        })
    })
}

/// A Flash Heal on a Warrior at full health lands 0, and floats nothing.
#[test]
fn a_fully_overhealed_direct_heal_floats_nothing() {
    let run = flash_heal(0.0);
    let heals = run.lines_of("Flash Heal");
    assert_eq!(
        heals.len(),
        1,
        "the staged heal landed:\n{}",
        run.played.log
    );
    assert_eq!(
        (heals[0].to.as_str(), heals[0].amount.as_str()),
        (WARRIOR_ID, "0")
    );
    assert_eq!(
        run.numbers_at(&heals[0].stamp, WARRIOR_ID),
        Vec::<&str>::new()
    );
    run.assert_heal_numbers_follow_the_rule();
}

/// A Flash Heal on a Warrior 3 below full lands 3, and floats "+3".
#[test]
fn a_partly_overhealed_direct_heal_floats_what_it_landed() {
    let run = flash_heal(3.0);
    let heals = run.lines_of("Flash Heal");
    assert_eq!(
        heals.len(),
        1,
        "the staged heal landed:\n{}",
        run.played.log
    );
    assert_eq!(
        (heals[0].to.as_str(), heals[0].amount.as_str()),
        (WARRIOR_ID, "3")
    );
    assert_eq!(run.numbers_at(&heals[0].stamp, WARRIOR_ID), vec!["+3"]);
    run.assert_heal_numbers_follow_the_rule();
}

// --- Direct heal: Holy Shock's heal --------------------------------------

/// A Holy Shock heal of the Paladin's on its Warrior, staged pending as the
/// gates open (`process_holy_shock_heals` lands it), at `crit` crit chance.
/// The Paladin has no mana, so it casts nothing of its own.
fn holy_shock(warrior_health: f32, crit: f32) -> Run {
    party("Paladin", 2.0, move |s| {
        s.mana(HEALER, 0.0)
            .world(move |world| {
                let e = WARRIOR.entity(world);
                let mut c = world.get_mut::<Combatant>(e).unwrap();
                c.current_health = c.max_health - warrior_health;
            })
            .world(move |world| {
                let target = WARRIOR.entity(world);
                let caster = HEALER.entity(world);
                let paladin = world.get::<Combatant>(caster).unwrap();
                let pending = HolyShockHealPending {
                    caster,
                    caster_book: paladin.book().clone(),
                    caster_spell_power: paladin.spell_power,
                    caster_crit_chance: crit,
                    caster_team: paladin.team,
                    caster_slot: paladin.slot,
                    caster_class: paladin.class,
                    target,
                };
                world.spawn((pending, PlayMatchEntity));
            })
    })
}

/// The one heal line of `ability` in `run`, on the Warrior.
fn the_heal(run: &Run, ability: &str) -> HealLine {
    let heals = run.lines_of(ability);
    assert_eq!(
        heals.len(),
        1,
        "the staged heal landed:\n{}",
        run.played.log
    );
    assert_eq!(heals[0].to, WARRIOR_ID);
    heals[0].clone()
}

/// A Holy Shock heal on a Warrior at full health lands 0, and floats nothing.
#[test]
fn a_fully_overhealed_holy_shock_floats_nothing() {
    let run = holy_shock(0.0, 0.0);
    let heal = the_heal(&run, "Holy Shock");
    assert_eq!((heal.amount.as_str(), heal.crit), ("0", false));
    assert_eq!(run.numbers_at(&heal.stamp, WARRIOR_ID), Vec::<&str>::new());
    run.assert_heal_numbers_follow_the_rule();
}

/// A Holy Shock heal on a Warrior 3 below full lands 3, and floats "+3".
#[test]
fn a_partly_overhealed_holy_shock_floats_what_it_landed() {
    let run = holy_shock(3.0, 0.0);
    let heal = the_heal(&run, "Holy Shock");
    assert_eq!((heal.amount.as_str(), heal.crit), ("3", false));
    assert_eq!(run.numbers_at(&heal.stamp, WARRIOR_ID), vec!["+3"]);
    run.assert_heal_numbers_follow_the_rule();
}

/// A critical Holy Shock heal on a Warrior far below full floats its whole
/// amount as a crit.
#[test]
fn a_critical_holy_shock_floats_a_crit() {
    let run = holy_shock(200.0, 1.0);
    let heal = the_heal(&run, "Holy Shock");
    assert!(heal.crit, "the heal crit: {heal:?}");
    assert_ne!(heal.amount, "0");
    assert_eq!(
        run.crits_at(&heal.stamp, WARRIOR_ID),
        vec![(format!("+{}", heal.amount), true)]
    );
    run.assert_heal_numbers_follow_the_rule();
}

// --- Direct heal: Holy Nova's area heal -----------------------------------

/// Every Holy node, Holy Nova among them (`tests/holy_talents.rs`'s build).
fn holy_nova_build() -> BuildSpec {
    Build::of(&[
        (TalentId::HolySpecialization, 1),
        (TalentId::SpiritualHealing, 1),
        (TalentId::Inspiration, 2),
        (TalentId::BlessedRecovery, 1),
        (TalentId::HolyConcentration, 1),
        (TalentId::HolyNova, 1),
        (TalentId::GuardianSpirit, 1),
    ])
    .into()
}

/// A Holy Priest's Holy Nova heal on its Warrior, `heal` before reductions,
/// staged in the frame's instant landings as the gates open
/// (`land_instant_effects` lands it). The Priest has no mana, so it casts
/// nothing of its own.
fn holy_nova(warrior_health: f32, heal: f32, is_crit: bool) -> Run {
    party_built("Priest", Some(holy_nova_build()), 2.0, move |s| {
        s.mana(HEALER, 0.0)
            .world(move |world| {
                let e = WARRIOR.entity(world);
                let mut c = world.get_mut::<Combatant>(e).unwrap();
                c.current_health = c.max_health - warrior_health;
            })
            .world(move |world| {
                let target = WARRIOR.entity(world);
                let caster = HEALER.entity(world);
                let priest = world.get::<Combatant>(caster).unwrap();
                assert!(
                    priest.book().get(&AbilityType::HolyNova).is_some(),
                    "the Holy Priest knows Holy Nova"
                );
                let queued = QueuedAoeHeal {
                    caster,
                    target,
                    heal,
                    is_crit,
                    caster_team: priest.team,
                    caster_slot: priest.slot,
                    caster_class: priest.class,
                    ability: AbilityType::HolyNova,
                };
                world
                    .resource_mut::<InstantLandings>()
                    .area_heals
                    .push(queued);
            })
    })
}

/// A Holy Nova heal on a Warrior at full health lands 0, and floats nothing.
#[test]
fn a_fully_overhealed_holy_nova_floats_nothing() {
    let run = holy_nova(0.0, 27.0, false);
    let heal = the_heal(&run, "Holy Nova");
    assert_eq!((heal.amount.as_str(), heal.crit), ("0", false));
    assert_eq!(run.numbers_at(&heal.stamp, WARRIOR_ID), Vec::<&str>::new());
    run.assert_heal_numbers_follow_the_rule();
}

/// A Holy Nova heal on a Warrior 3 below full lands 3, and floats "+3".
#[test]
fn a_partly_overhealed_holy_nova_floats_what_it_landed() {
    let run = holy_nova(3.0, 27.0, false);
    let heal = the_heal(&run, "Holy Nova");
    assert_eq!((heal.amount.as_str(), heal.crit), ("3", false));
    assert_eq!(run.numbers_at(&heal.stamp, WARRIOR_ID), vec!["+3"]);
    run.assert_heal_numbers_follow_the_rule();
}

/// A critical Holy Nova heal on a Warrior far below full floats its whole
/// amount as a crit.
#[test]
fn a_critical_holy_nova_floats_a_crit() {
    let run = holy_nova(200.0, 40.0, true);
    let heal = the_heal(&run, "Holy Nova");
    assert_eq!((heal.amount.as_str(), heal.crit), ("40", true));
    assert_eq!(
        run.crits_at(&heal.stamp, WARRIOR_ID),
        vec![("+40".to_string(), true)]
    );
    run.assert_heal_numbers_follow_the_rule();
}

// --- Heal over time: Rejuvenation's ticks ---------------------------------

/// The Druid's Rejuvenation on its Warrior, 3 below full, its first tick due
/// just after the gates: the first tick lands the 3, and every later tick
/// finds the Warrior full.
#[test]
fn heal_over_time_ticks_float_what_they_landed_and_nothing_once_full() {
    let run = party("Druid", 8.0, |s| {
        s.world(missing(WARRIOR, 3.0)).world(|world| {
            let target = WARRIOR.entity(world);
            let druid = HEALER.entity(world);
            let mut rejuvenation = aura_of(world, AbilityType::Rejuvenation, target, druid);
            rejuvenation.time_until_next_tick = 0.1;
            world
                .get_mut::<ActiveAuras>(target)
                .unwrap()
                .auras
                .push(rejuvenation);
        })
    });
    let ticks: Vec<HealLine> = run
        .lines_of("Rejuvenation")
        .into_iter()
        .filter(|h| h.to == WARRIOR_ID)
        .collect();
    assert!(ticks.len() >= 3, "three ticks or more:\n{}", run.played.log);
    assert_eq!(
        ticks[0].amount, "3",
        "the first tick lands the 3: {ticks:?}"
    );
    assert_eq!(run.numbers_at(&ticks[0].stamp, WARRIOR_ID), vec!["+3"]);
    for tick in &ticks[1..] {
        assert_eq!(tick.amount, "0", "the Warrior is full: {ticks:?}");
        assert_eq!(run.numbers_at(&tick.stamp, WARRIOR_ID), Vec::<&str>::new());
    }
    let (_, zero) = run.assert_heal_numbers_follow_the_rule();
    assert!(zero >= ticks.len() - 1);
}

// --- Bloom: Lifebloom running out -----------------------------------------

/// The Druid's Lifebloom on its Warrior with a fifth of a second left and no
/// tick due before it ends, so its last tick and its bloom land as it runs
/// out. The Warrior is `missing` below full.
fn lifebloom(missing_after_tick: Option<f32>) -> Run {
    party("Druid", 2.0, move |s| {
        s.world(move |world| {
            let target = WARRIOR.entity(world);
            let druid = HEALER.entity(world);
            let mut bloom = aura_of(world, AbilityType::Lifebloom, target, druid);
            bloom.duration = 0.2;
            bloom.time_until_next_tick = 5.0;
            let tick = bloom.tick_amount();
            world
                .get_mut::<ActiveAuras>(target)
                .unwrap()
                .auras
                .push(bloom);
            if let Some(after) = missing_after_tick {
                missing(WARRIOR, tick + after)(world);
            }
        })
    })
}

/// The bloom's line, and the last tick's that landed with it.
fn bloom_and_tick(run: &Run) -> (HealLine, HealLine) {
    let (blooms, ticks): (Vec<HealLine>, Vec<HealLine>) =
        run.lines_of("Lifebloom").into_iter().partition(|h| h.bloom);
    assert_eq!(
        (blooms.len(), ticks.len()),
        (1, 1),
        "the last tick and the bloom:\n{}",
        run.played.log
    );
    assert_eq!(blooms[0].stamp, ticks[0].stamp, "landed together");
    (blooms[0].clone(), ticks[0].clone())
}

/// A bloom on a Warrior at full health lands 0, as does the last tick beside
/// it, and neither floats a number.
#[test]
fn a_fully_overhealed_bloom_floats_nothing() {
    let run = lifebloom(None);
    let (bloom, tick) = bloom_and_tick(&run);
    assert_eq!(
        (bloom.to.as_str(), bloom.amount.as_str()),
        (WARRIOR_ID, "0")
    );
    assert_eq!(tick.amount, "0");
    assert_eq!(run.numbers_at(&bloom.stamp, WARRIOR_ID), Vec::<&str>::new());
    run.assert_heal_numbers_follow_the_rule();
}

/// A bloom on a Warrior 3 below full once the last tick has landed lands 3,
/// and floats "+3" beside the tick's number.
#[test]
fn a_partly_overhealed_bloom_floats_what_it_landed() {
    let run = lifebloom(Some(3.0));
    let (bloom, tick) = bloom_and_tick(&run);
    assert_eq!(
        (bloom.to.as_str(), bloom.amount.as_str()),
        (WARRIOR_ID, "3")
    );
    assert_ne!(tick.amount, "0", "the tick landed whole");
    let mut over = run.numbers_at(&bloom.stamp, WARRIOR_ID);
    over.retain(|n| *n == "+3");
    assert_eq!(over, vec!["+3"]);
    run.assert_heal_numbers_follow_the_rule();
}

// --- Leech: Drain Life ----------------------------------------------------

const LOCK: Unit = Unit::new(1, 0);
const FOE: Unit = Unit::new(2, 0);
const LOCK_ID: &str = "Team 1 Warlock #1";

/// An Affliction Warlock `missing` below full drains a Warrior held stunned
/// at its side for five ticks. Returns the run and the stamps of the drain's
/// damage ticks.
fn drain(missing_health: f32) -> (Run, Vec<String>) {
    let config = HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(508),
        max_duration_secs: 7.0,
        team1_builds: vec![BuildSpec::Reference("Affliction".into())],
        ..Default::default()
    };
    let pet = Unit::pet_of(LOCK.team, LOCK.slot);
    let stage = Stage::new()
        .place(LOCK, 5.0, 5.0)
        .place(FOE, 7.0, 5.5)
        .place(pet, -20.0, 30.0)
        .target(FOE, LOCK)
        .aura(FOE, held(10.0))
        .world(missing(LOCK, missing_health))
        .world(|world| {
            let target = FOE.entity(world);
            let caster = LOCK.entity(world);
            world.entity_mut(caster).insert(ChannelingState {
                ability: AbilityType::DrainLife,
                duration_remaining: 5.0,
                time_until_next_tick: 0.0,
                tick_interval: 1.0,
                target,
                interrupted: false,
                interrupted_display_time: 0.0,
                ticks_applied: 0,
            });
        });
    let run = Run::play(config, stage);
    let ticks: Vec<String> = run
        .played
        .log
        .lines()
        .filter(|l| l.contains("[DMG] Team 1 Warlock #1's Drain Life ticks on "))
        .map(|l| l[1..l.find("s]").unwrap()].trim().to_string())
        .collect();
    assert!(ticks.len() >= 3, "three drain ticks:\n{}", run.played.log);
    (run, ticks)
}

/// A Warlock 3 below full: the drain's first tick heals the 3 and floats
/// "+3"; every later tick finds the Warlock full, so its heal lands nothing —
/// no line (`leech::land` logs a leech heal only when it lands something) and
/// no number.
#[test]
fn a_leech_heal_floats_what_it_landed_and_nothing_once_full() {
    let (run, ticks) = drain(3.0);
    let heals = run.lines_of("Drain Life");
    assert_eq!(
        heals.len(),
        1,
        "one drain heal landed something:\n{}",
        run.played.log
    );
    assert_eq!(
        (heals[0].to.as_str(), heals[0].amount.as_str()),
        (LOCK_ID, "3")
    );
    assert_eq!(heals[0].stamp, ticks[0]);
    assert_eq!(run.numbers_at(&ticks[0], LOCK_ID), vec!["+3"]);
    for stamp in &ticks[1..] {
        assert_eq!(run.numbers_at(stamp, LOCK_ID), Vec::<&str>::new());
    }
    run.assert_heal_numbers_follow_the_rule();
}

/// A Warlock a third of a point below full: the drain's first tick heals that
/// third, which the log prints as "for 0" and which floats no number.
#[test]
fn a_leech_heal_that_rounds_to_nothing_floats_nothing() {
    let (run, ticks) = drain(0.3);
    let heals = run.lines_of("Drain Life");
    assert_eq!(heals.len(), 1, "the first tick healed:\n{}", run.played.log);
    assert_eq!(
        (heals[0].to.as_str(), heals[0].amount.as_str()),
        (LOCK_ID, "0")
    );
    assert_eq!(heals[0].stamp, ticks[0]);
    assert_eq!(run.numbers_at(&ticks[0], LOCK_ID), Vec::<&str>::new());
    run.assert_heal_numbers_follow_the_rule();
}

// ---------------------------------------------------------------------------
// Whole matches: every heal site the healers reach
// ---------------------------------------------------------------------------

/// Seeded matches of every healer, on both AI profiles and a pillar map: at
/// every moment, over every unit, the green numbers are the heal lines that
/// landed something. Direct heals, Holy Shock, the area heals, heal over time
/// ticks (Healing Stream Totem's pulses among them), blooms and leeches each
/// land here as the AI plays them; many land 0 on a full teammate.
#[test]
fn every_heal_number_in_a_match_follows_the_rule() {
    let comps: [(&[&str], &[&str], &str, &str); 4] = [
        (
            &["Priest", "Warrior"],
            &["Druid", "Rogue"],
            "BasicArena",
            "Legacy",
        ),
        (
            &["Paladin", "Mage"],
            &["Shaman", "Hunter"],
            "PillaredArena",
            "TeamPlan",
        ),
        (
            &["Druid", "Warlock"],
            &["Priest", "Warrior"],
            "TwinPillars",
            "TeamPlan",
        ),
        (
            &["Shaman", "Rogue"],
            &["Paladin", "Warlock"],
            "BasicArena",
            "Legacy",
        ),
    ];
    let mut zero = 0;
    let mut checked = 0;
    let mut abilities = BTreeSet::new();
    for (seed, (team1, team2, map, profile)) in comps.into_iter().enumerate() {
        let config = HeadlessMatchConfig {
            team1: team1.iter().map(|c| c.to_string()).collect(),
            team2: team2.iter().map(|c| c.to_string()).collect(),
            map: map.into(),
            ai_profile: Some(profile.into()),
            random_seed: Some(508 + seed as u64),
            max_duration_secs: 60.0,
            ..Default::default()
        };
        let run = Run::play(config, Stage::new());
        let (lines, zeros) = run.assert_heal_numbers_follow_the_rule();
        checked += lines;
        zero += zeros;
        abilities.extend(run.heals().into_iter().map(|h| h.ability));
    }
    assert!(checked > 0 && zero > 0, "{checked} heals, {zero} of them 0");
    for family in [
        "Flash of Light",
        "Rejuvenation",
        "Lifebloom",
        "Healing Stream",
    ] {
        assert!(
            abilities.iter().any(|a| a.contains(family)),
            "{family} healed: {abilities:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The one site
// ---------------------------------------------------------------------------

/// The one place outside the theme that may name the heal green, and how
/// often: the module that builds every heal number.
const HEAL_GREEN_AT: (&str, &str) = ("src/states/play_match/heal_numbers.rs", "fct::HEAL");

/// Every place `code` (comment-, string- and test-module-blanked) names the
/// heal green, `ui::theme::fct::HEAL`, as (construct, line):
///
/// - a path to it, however the `fct` module is reached: in full
///   (`crate::ui::theme::fct::HEAL`), through `theme` or `fct` imported, or
///   through a renamed `fct` (`use ...::fct as hues; hues::HEAL`, or
///   `use ...::{fct::{self as hues}}`), with any whitespace round the `::`;
/// - an import of it, however the tree is written (`fct::HEAL`,
///   `fct::{HEAL}`, `fct::{HEAL as GREEN}`), `pub use` or not: the import
///   is the hit, since a name it binds can be used anywhere after;
/// - a glob of `fct` (`use ...::fct::*`), which brings it in unseen;
/// - a re-export of the `fct` module itself (`pub use ...::fct`, renamed or
///   not), through which another file could name it under a path this
///   reader, one file at a time, cannot follow;
/// - in the theme file itself, any `HEAL` but its own definition, so no
///   other theme token can be made an alias of it.
///
/// Known limit: a second theme token defined with the same colour value
/// under another name is not seen here (a literal colour outside the theme
/// is `tests/ui_token_audit.rs`'s).
fn heal_green_names(code: &str, is_theme: bool) -> Vec<(String, usize)> {
    let line_of = |at: usize| 1 + code[..at].matches('\n').count();
    let uses = use_statements(code);
    let names_fct =
        |segment: &str, aliases: &BTreeSet<String>| segment == "fct" || aliases.contains(segment);
    // Names that bind the `fct` module under another name.
    let aliases: BTreeSet<String> = uses
        .iter()
        .flat_map(|u| &u.bindings)
        .filter(|b| b.path.last().map(String::as_str) == Some("fct") && b.name != "fct")
        .map(|b| b.name.clone())
        .collect();
    let mut hits = Vec::new();
    for u in &uses {
        let keyword = if u.public { "pub use" } else { "use" };
        for b in &u.bindings {
            let n = b.path.len();
            let last = b.path[n - 1].as_str();
            let parent = (n >= 2).then(|| b.path[n - 2].as_str());
            let in_fct = parent.is_some_and(|p| names_fct(p, &aliases));
            let hit = (in_fct && (last == "HEAL" || last == "*"))
                || (u.public && last == "fct")
                || (is_theme && last == "HEAL");
            if hit {
                hits.push((
                    format!("{keyword} {}", b.path.join("::")),
                    line_of(u.span.start),
                ));
            }
        }
    }
    // Paths outside the `use` statements, which were read above.
    let mut rest = code.to_string();
    for u in &uses {
        rest.replace_range(u.span.clone(), &" ".repeat(u.span.len()));
    }
    let module = std::iter::once("fct".to_string()).chain(aliases.iter().cloned());
    for name in module {
        let path = Regex::new(&format!(r"\b{}\s*::\s*HEAL\b", regex::escape(&name))).unwrap();
        for m in path.find_iter(&rest) {
            // A renamed `fct` is a name at the start of its path:
            // `other::hues::HEAL` is not it.
            if name != "fct" && rest[..m.start()].trim_end().ends_with("::") {
                continue;
            }
            hits.push(("fct::HEAL".to_string(), line_of(m.start())));
        }
    }
    if is_theme {
        // Every `HEAL` in the theme but a definition (`const HEAL`).
        let bare = Regex::new(r"\bHEAL\b").unwrap();
        for m in bare.find_iter(&rest) {
            if !rest[..m.start()].trim_end().ends_with("const") {
                hits.push(("HEAL in the theme".to_string(), line_of(m.start())));
            }
        }
    }
    hits
}

/// The heal green is named in one place outside the theme, once: the module
/// that builds every heal number. A heal number built anywhere else would
/// float without the rule.
#[test]
fn every_heal_number_is_built_by_heal_numbers() {
    let mut found: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        let is_theme = file.rel_display() == "src/ui/theme.rs";
        for (construct, line) in heal_green_names(&code, is_theme) {
            found
                .entry((file.rel_display(), construct))
                .or_default()
                .push(line);
        }
    }
    let expected = BTreeMap::from([(
        (HEAL_GREEN_AT.0.to_string(), HEAL_GREEN_AT.1.to_string()),
        found
            .get(&(HEAL_GREEN_AT.0.to_string(), HEAL_GREEN_AT.1.to_string()))
            .filter(|lines| lines.len() == 1)
            .cloned()
            .unwrap_or_default(),
    )]);
    assert_eq!(
        found, expected,
        "the heal green is named outside heal_numbers (or more than once in it): build a \
         heal's number with `heal_numbers::spawn_heal_number`"
    );
}

/// The reader finds every spelling its doc names, and passes what is not the
/// heal green. Each hit is (line, construct); the set found must be exactly
/// the set named.
#[test]
fn the_audit_reads_every_spelling_of_the_heal_green() {
    let fixture = r#"
use crate::ui::theme::fct::HEAL;
use crate::ui::theme::fct::{HEAL as GREEN, DAMAGE};
use crate::ui::theme::{fct::{self as hues}};
pub use crate::ui::theme::fct::HEAL as G2;
pub use crate::ui::theme::fct as hue_table;
use crate::ui::theme::fct::*;
pub(crate) use crate::ui::theme::fct;
use crate::ui::theme::fct::{DAMAGE as D, ABSORB};
use crate::ui::theme;
fn a() {
    let _ = crate::ui::theme::fct::HEAL;
    let _ = fct :: HEAL;
    let _ = hues::HEAL;
    let _ = hue_table::HEAL;
    let _ = { use crate::ui::theme::fct::{HEAL}; HEAL };
    let _ = theme::fct::HEAL;
    let _ = fct::DAMAGE;
    let _ = other::HEAL;
    let _ = x::hues::HEAL;
    let _ = fct::HEALTH;
}
"#;
    let found: BTreeSet<String> = heal_green_names(fixture, false)
        .into_iter()
        .map(|(construct, line)| format!("{line}: {construct}"))
        .collect();
    let expected: BTreeSet<String> = [
        "2: use crate::ui::theme::fct::HEAL",
        "3: use crate::ui::theme::fct::HEAL",
        "5: pub use crate::ui::theme::fct::HEAL",
        "6: pub use crate::ui::theme::fct",
        "7: use crate::ui::theme::fct::*",
        "8: pub use crate::ui::theme::fct",
        "12: fct::HEAL",
        "13: fct::HEAL",
        "14: fct::HEAL",
        "15: fct::HEAL",
        "16: use crate::ui::theme::fct::HEAL",
        "17: fct::HEAL",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(found, expected);

    // In the theme, the definition is not a hit; another use of it is.
    let theme = r#"
pub mod fct {
    pub const HEAL: Color32 = Color32::from_rgb(100, 255, 100);
    pub const GREEN: Color32 = HEAL;
}
"#;
    let found: Vec<(String, usize)> = heal_green_names(theme, true);
    assert_eq!(found, vec![("HEAL in the theme".to_string(), 4)]);
}
