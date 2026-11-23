//! AS-502 — every leech heal floats its green number over its recipient.
//!
//! A leech heal lands through `combat_core::leech::land`, which logs its line
//! and floats its number there, whichever site the damage that fed it landed
//! at: a cast (`process_casting`), a projectile (`process_projectile_hits`),
//! a damage-over-time tick (`process_dot_ticks`) or a channel tick
//! (`process_channeling`, the channel's own leech and a leech on its target
//! both).
//!
//! Each match here is staged and played through the production schedule; an
//! observer installed as the stage lands records every floating number as it
//! is spawned, with the combat-log clock and the unit it floats over (read
//! off its anchor: a number stands at its unit's x and z, moved along x by
//! the pattern's offset only). The claim is checked against the combat log
//! the same match wrote: at every moment a leech heal line names a recipient,
//! the green numbers over that recipient are exactly the heal lines naming it
//! then, amount for amount and crit for crit — one number per heal, on the
//! unit healed, none extra. And the numbers one unit takes in one frame stand
//! in distinct slots of its floating-text pattern, so a leech number never
//! lands on top of that frame's other numbers.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, ChannelingState, Combatant, FloatingCombatText, Pet,
    PlayMatchEntity, Projectile,
};
use arenasim::states::play_match::reference_builds::BuildSpec;
use arenasim::states::play_match::talents::{Build, TalentId};
use arenasim::states::play_match::utils::{combat_log_id_for, FCT_HORIZONTAL_SPREAD};
use arenasim::states::play_match::AbilityType;
use common::staged::{play, Played};

/// The leeches: every ability whose heal lands through `leech::land`.
const LEECHES: [&str; 4] = [
    "Vampiric Embrace",
    "Siphon Life",
    "Drain Life",
    "Death Coil",
];

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
    /// Whether the unit it floats over carried no auras at all.
    bare: bool,
    /// The frame it was spawned on, as the app clock's nanoseconds.
    frame: u128,
    /// Its slot in its unit's floating-text pattern: 0 centre, 1 right, 2
    /// left (`utils::get_next_fct_offset`).
    slot: usize,
}

/// A stage step that records every floating number spawned from here on into
/// `sink`.
fn record_numbers(sink: Arc<Mutex<Vec<Number>>>) -> impl FnOnce(&mut World) + Send + Sync {
    move |world: &mut World| {
        world.add_observer(
            move |trigger: Trigger<OnAdd, FloatingCombatText>,
                  texts: Query<&FloatingCombatText>,
                  log: Res<CombatLog>,
                  time: Res<Time>,
                  units: Query<(&Combatant, &Transform, Option<&Pet>, Option<&ActiveAuras>)>| {
                let text = texts.get(trigger.target()).expect("the number");
                let at = text.world_position;
                // The anchor is the unit's position, raised, and moved along
                // x by one of the pattern's three offsets.
                let offsets = [0.0, FCT_HORIZONTAL_SPREAD * 0.4, FCT_HORIZONTAL_SPREAD * -0.4];
                let over: Vec<(String, bool, usize)> = units
                    .iter()
                    .filter_map(|(c, t, pet, auras)| {
                        if (at.z - t.translation.z).abs() >= 1e-3 {
                            return None;
                        }
                        let slot = offsets
                            .iter()
                            .position(|o| (at.x - o - t.translation.x).abs() < 1e-3)?;
                        Some((combat_log_id_for(c, pet).to_string(), auras.is_none(), slot))
                    })
                    .collect();
                assert_eq!(
                    over.len(),
                    1,
                    "the number {:?} at {at} stands over exactly one unit: {over:?}",
                    text.text
                );
                sink.lock().unwrap().push(Number {
                    stamp: format!("{:.2}", log.match_time),
                    over: over[0].0.clone(),
                    text: text.text.clone(),
                    heal: text.color == arenasim::ui::theme::fct::HEAL,
                    crit: text.is_crit,
                    bare: over[0].1,
                    frame: time.elapsed().as_nanos(),
                    slot: over[0].2,
                });
            },
        );
    }
}

/// One `[HEAL]` line of the log: when, whose ability, which, who it healed,
/// and how much.
#[derive(Clone, Debug)]
struct HealLine {
    stamp: String,
    caster: String,
    ability: String,
    to: String,
    amount: String,
    crit: bool,
}

/// Every `[HEAL]` line of `log`. A line in a shape this cannot read fails the
/// test, so no heal is quietly left out of the comparison.
fn heal_lines(log: &str) -> Vec<HealLine> {
    log.lines()
        .filter(|l| l.contains("] [HEAL] "))
        .map(|l| {
            let stamp = l[1..l.find("s]").expect("a stamp")].trim().to_string();
            let message = &l[l.find("] [HEAL] ").unwrap() + "] [HEAL] ".len()..];
            let (caster, rest) = message
                .split_once("'s ")
                .unwrap_or_else(|| panic!("an unread heal line: {l}"));
            let crit = rest.contains(" CRITICALLY heals ");
            let (ability, healed) = rest
                .split_once(" CRITICALLY heals ")
                .or_else(|| rest.split_once(" heals "))
                .unwrap_or_else(|| panic!("an unread heal line: {l}"));
            let (to, amount) = match healed.strip_prefix("for ") {
                Some(amount) => (caster, amount),
                None => healed
                    .rsplit_once(" for ")
                    .unwrap_or_else(|| panic!("an unread heal line: {l}")),
            };
            HealLine {
                stamp,
                caster: caster.to_string(),
                ability: ability.to_string(),
                to: to.to_string(),
                amount: amount
                    .split_whitespace()
                    .next()
                    .expect("an amount")
                    .to_string(),
                crit,
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

    /// The leech heal lines of `ability`.
    fn leech_lines(&self, ability: &str) -> Vec<HealLine> {
        self.heals()
            .into_iter()
            .filter(|h| h.ability == ability)
            .collect()
    }

    /// At every (moment, recipient) a leech heal line names, the green
    /// numbers over that recipient then are exactly the heal lines naming it
    /// then, each with the line's amount and crit. Returns how many leech
    /// heals were checked.
    fn assert_every_leech_heal_floats_its_number(&self) -> usize {
        let heals = self.heals();
        let keys: BTreeSet<(String, String)> = heals
            .iter()
            .filter(|h| LEECHES.contains(&h.ability.as_str()))
            .map(|h| (h.stamp.clone(), h.to.clone()))
            .collect();
        assert!(!keys.is_empty(), "the match landed no leech heal");
        let mut lines: BTreeMap<&(String, String), Vec<String>> = BTreeMap::new();
        let mut floats: BTreeMap<&(String, String), Vec<String>> = BTreeMap::new();
        for key in &keys {
            let mut l: Vec<String> = heals
                .iter()
                .filter(|h| (&h.stamp, &h.to) == (&key.0, &key.1))
                .map(|h| format!("+{} crit={}", h.amount, h.crit))
                .collect();
            l.sort();
            lines.insert(key, l);
            let mut f: Vec<String> = self
                .numbers
                .iter()
                .filter(|n| n.heal && (&n.stamp, &n.over) == (&key.0, &key.1))
                .map(|n| format!("{} crit={}", n.text, n.crit))
                .collect();
            f.sort();
            floats.insert(key, f);
        }
        assert_eq!(
            floats, lines,
            "the green numbers over each leech heal's recipient, against its heal lines"
        );
        heals
            .iter()
            .filter(|h| LEECHES.contains(&h.ability.as_str()))
            .count()
    }

    /// The numbers one unit takes in one frame stand in distinct slots of its
    /// floating-text pattern, so none is drawn over another. Returns how many
    /// such frames (two or more numbers over one unit) hold a leech heal's
    /// number.
    fn assert_a_units_numbers_in_a_frame_take_distinct_slots(&self) -> usize {
        let leech_keys: BTreeSet<(String, String)> = self
            .heals()
            .into_iter()
            .filter(|h| LEECHES.contains(&h.ability.as_str()))
            .map(|h| (h.stamp, h.to))
            .collect();
        let mut frames: BTreeMap<(u128, &str), Vec<&Number>> = BTreeMap::new();
        for n in &self.numbers {
            frames
                .entry((n.frame, n.over.as_str()))
                .or_default()
                .push(n);
        }
        let mut with_leech = 0;
        for numbers in frames.values().filter(|ns| ns.len() > 1) {
            let slots: BTreeSet<usize> = numbers.iter().map(|n| n.slot).collect();
            assert_eq!(
                slots.len(),
                numbers.len(),
                "one unit's numbers in one frame share a slot: {numbers:?}"
            );
            if numbers
                .iter()
                .any(|n| n.heal && leech_keys.contains(&(n.stamp.clone(), n.over.clone())))
            {
                with_leech += 1;
            }
        }
        with_leech
    }
}

// ---------------------------------------------------------------------------
// Vampiric Embrace on a party of two
// ---------------------------------------------------------------------------

const PRIEST: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(1, 1);
const MAGE: Unit = Unit::new(2, 0);

const PRIEST_ID: &str = "Team 1 Priest #1";
const WARRIOR_ID: &str = "Team 1 Warrior #2";

/// A Shadowform Priest with Mind Flay and Vampiric Embrace, and a Warrior,
/// both at half health, against a Mage that carries the Priest's embrace —
/// so Mind Blast's casts, Shadow Word: Pain's ticks and Mind Flay's channel
/// ticks each feed the party's heals. `more` adds to the stage.
fn embrace_run(more: impl FnOnce(Stage) -> Stage) -> Run {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Mage".into()],
        random_seed: Some(502),
        max_duration_secs: 40.0,
        team1_builds: vec![Build::of(&[
            (TalentId::ImprovedMindBlast, 2),
            (TalentId::Blackout, 1),
            (TalentId::MindFlay, 1),
            (TalentId::Shadowform, 1),
            (TalentId::VampiricEmbrace, 1),
        ])
        .into()],
        team1_priest_forms: vec!["Shadowform".into()],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(PRIEST, 6.0, 4.0)
        .place(WARRIOR, 14.0, -3.0)
        .place(MAGE, 24.0, 2.0)
        .target(PRIEST, MAGE)
        .target(WARRIOR, MAGE)
        .health(PRIEST, 0.5)
        .health(WARRIOR, 0.5)
        // No Ice Barrier to soak the Shadow damage that feeds the embrace.
        .clear_auras(MAGE)
        .land(MAGE, AbilityType::VampiricEmbrace, PRIEST);
    Run::play(config, more(stage))
}

/// Every Vampiric Embrace heal floats one number, its amount, over the party
/// member it healed — the Priest and its Warrior both — fed by a cast, a
/// damage-over-time tick and a channel tick.
#[test]
fn every_vampiric_embrace_heal_floats_its_number_over_its_recipient() {
    let run = embrace_run(|s| s);
    let embrace = run.leech_lines("Vampiric Embrace");
    let healed: BTreeSet<&str> = embrace.iter().map(|h| h.to.as_str()).collect();
    assert_eq!(
        healed,
        BTreeSet::from([PRIEST_ID, WARRIOR_ID]),
        "the embrace heals the party of two:\n{}",
        run.played.log
    );
    // Each of the three Shadow damage sites fed a heal: a heal of the
    // embrace's lands at the moment of a Priest damage line of each.
    let log = &run.played.log;
    for (fed_by, site) in [
        ("Mind Blast", "process_casting"),
        ("Shadow Word: Pain", "process_dot_ticks"),
        ("Mind Flay", "process_channeling"),
    ] {
        let stamps: BTreeSet<String> = log
            .lines()
            .filter(|l| l.contains("[DMG] Team 1 Priest #1's ") && l.contains(fed_by))
            .map(|l| l[1..l.find("s]").unwrap()].trim().to_string())
            .collect();
        assert!(
            embrace.iter().any(|h| stamps.contains(&h.stamp)),
            "{fed_by} ({site}) fed an embrace heal:\n{log}"
        );
    }
    let checked = run.assert_every_leech_heal_floats_its_number();
    run.assert_a_units_numbers_in_a_frame_take_distinct_slots();
    assert_eq!(checked, embrace.len(), "only the embrace leeches here");
}

/// The projectile site (`process_projectile_hits`) lands a party leech too:
/// no Priest spell flies today, so a Mind Blast of the Priest's is staged in
/// flight at the Mage as the gates open. Its embrace heals float their
/// numbers over the Priest and the Warrior like any other.
#[test]
fn a_projectile_fed_embrace_heal_floats_its_number_over_each_recipient() {
    let run = embrace_run(|s| {
        s.world(|world| {
            let caster = PRIEST.entity(world);
            let target = MAGE.entity(world);
            let at = world.get::<Transform>(target).unwrap().translation + Vec3::Y;
            world.spawn((
                Projectile {
                    caster,
                    target,
                    ability: AbilityType::MindBlast,
                    speed: 40.0,
                    caster_team: 1,
                    caster_slot: 0,
                    caster_class: CharacterClass::Priest,
                    caster_pet_type: None,
                    next_cast: None,
                },
                Transform::from_translation(at),
                PlayMatchEntity,
            ));
        })
    });
    // The staged Mind Blast lands within a frame or two of the gates, before
    // any Mind Blast the Priest casts (a 1.5s cast) could.
    let gates = run.played.landed().app;
    let arrived: BTreeSet<String> = run
        .played
        .log
        .lines()
        .filter(|l| l.contains("[DMG] Team 1 Priest #1's Mind Blast "))
        .map(|l| l[1..l.find("s]").unwrap()].trim().to_string())
        .filter(|stamp| stamp.parse::<f32>().unwrap() < gates + 0.5)
        .collect();
    assert_eq!(
        arrived.len(),
        1,
        "the staged Mind Blast landed:\n{}",
        run.played.log
    );
    let fed: BTreeSet<String> = run
        .leech_lines("Vampiric Embrace")
        .into_iter()
        .filter(|h| arrived.contains(&h.stamp))
        .map(|h| h.to)
        .collect();
    assert_eq!(
        fed,
        BTreeSet::from([PRIEST_ID.to_string(), WARRIOR_ID.to_string()]),
        "the staged Mind Blast fed both party members' heals:\n{}",
        run.played.log
    );
    run.assert_every_leech_heal_floats_its_number();
}

// ---------------------------------------------------------------------------
// The Warlock's leeches: Siphon Life, Drain Life, Death Coil
// ---------------------------------------------------------------------------

const LOCK: Unit = Unit::new(1, 0);
const FOE: Unit = Unit::new(2, 0);

/// The Warlock's Siphon Life on `target`, as the talent's ability applies
/// it, with its first tick due in `first_tick` seconds.
fn siphon_life(world: &mut World, target: Entity, caster: Entity, first_tick: f32) -> Aura {
    let def = world
        .get::<Combatant>(caster)
        .unwrap()
        .book()
        .get(&AbilityType::SiphonLife)
        .expect("the talent grants Siphon Life")
        .clone();
    let mut siphon = AuraPending::from_ability(target, caster, &def)
        .expect("Siphon Life applies an aura")
        .aura;
    siphon.time_until_next_tick = first_tick;
    siphon
}

/// An Affliction Warlock, hurt, drains a Warrior held stunned at its side
/// that carries its Siphon Life: the life the Warrior loses to the Siphon's
/// ticks (`process_dot_ticks`) and to the drain's (`process_channeling`) comes
/// back to the Warlock. The Siphon's first tick is due on the drain's, so two
/// leech numbers float over the Warlock in one frame. When the stun ends the
/// Warrior is in melee of the Warlock, whose Death Coil peel
/// (`process_projectile_hits`) heals it for the hit.
fn warlock_run() -> Run {
    let config = HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(502),
        max_duration_secs: 12.0,
        team1_builds: vec![BuildSpec::Reference("Affliction".into())],
        ..Default::default()
    };
    let pet = Unit::pet_of(LOCK.team, LOCK.slot);
    let stage = Stage::new()
        .place(LOCK, 5.0, 5.0)
        .place(FOE, 7.0, 5.5)
        .place(pet, -20.0, 30.0)
        .target(FOE, LOCK)
        .health(LOCK, 0.6)
        // Held through the drain, so no Pummel ends it.
        .aura(
            FOE,
            Aura {
                effect_type: AuraType::Stun,
                duration: 6.0,
                break_on_damage_threshold: -1.0,
                ability_name: "Staged Stun".into(),
                ..Default::default()
            },
        )
        .world(|world| {
            let target = FOE.entity(world);
            let caster = LOCK.entity(world);
            let siphon = siphon_life(world, target, caster, 0.0);
            world
                .get_mut::<ActiveAuras>(target)
                .unwrap()
                .auras
                .push(siphon);
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
    Run::play(config, stage)
}

/// Siphon Life's, Drain Life's and Death Coil's heals each float one number,
/// their amount, over the Warlock: never two for a heal, and where the
/// drain's heal and the Siphon's land in one frame, the two stand in
/// different slots of the Warlock's pattern rather than on top of each other.
#[test]
fn every_warlock_leech_heal_floats_its_number() {
    let run = warlock_run();
    // The drain's and the Siphon's heals landed together at least once.
    let together = run
        .leech_lines("Siphon Life")
        .iter()
        .filter(|siphon| {
            run.leech_lines("Drain Life")
                .iter()
                .any(|drain| drain.stamp == siphon.stamp)
        })
        .count();
    assert!(
        together > 0,
        "a drain heal and a Siphon heal in one frame:\n{}",
        run.played.log
    );
    let stacked = run.assert_a_units_numbers_in_a_frame_take_distinct_slots();
    assert!(
        stacked >= together,
        "each frame with both heals is checked for its slots: {stacked} < {together}"
    );
    for ability in ["Siphon Life", "Drain Life", "Death Coil"] {
        let lines = run.leech_lines(ability);
        assert!(
            !lines.is_empty(),
            "{ability} healed the Warlock:\n{}",
            run.played.log
        );
        assert!(
            lines.iter().all(|h| h.to == h.caster),
            "{ability} heals its caster"
        );
    }
    run.assert_every_leech_heal_floats_its_number();
}

/// The control: the record reads the numbers it claims to, telling a heal
/// number from a damage number and the unit each floats over.
#[test]
fn the_record_tells_heal_numbers_from_damage_numbers_and_who_they_float_over() {
    let run = warlock_run();
    assert!(run
        .numbers
        .iter()
        .any(|n| n.heal && n.over == "Team 1 Warlock #1" && n.text.starts_with('+')));
    assert!(run
        .numbers
        .iter()
        .any(|n| !n.heal && n.over == "Team 2 Warrior #1"));
}

/// A Warlock that carries no auras at all is healed by its Siphon Life's
/// tick through the damage-over-time site's other lookup (a unit with no
/// `ActiveAuras`), and the number floats over it all the same. Its auras are
/// cleared and its Siphon's first tick is due a few frames after the gates,
/// once the emptied aura list is gone; the Warrior is held stunned at its
/// side so nothing else happens.
#[test]
fn a_heal_to_a_unit_without_auras_floats_its_number() {
    let config = HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(502),
        max_duration_secs: 1.0,
        team1_builds: vec![BuildSpec::Reference("Affliction".into())],
        ..Default::default()
    };
    let pet = Unit::pet_of(LOCK.team, LOCK.slot);
    let stage = Stage::new()
        .place(LOCK, 5.0, 5.0)
        .place(FOE, 7.0, 5.5)
        .place(pet, -20.0, 30.0)
        .health(LOCK, 0.6)
        // No mana: the Warlock casts nothing that would give it an aura.
        .mana(LOCK, 0.0)
        .clear_auras(LOCK)
        .clear_auras(FOE)
        .world(|world| {
            let target = FOE.entity(world);
            let caster = LOCK.entity(world);
            let siphon = siphon_life(world, target, caster, 0.1);
            let stun = Aura {
                effect_type: AuraType::Stun,
                duration: 5.0,
                break_on_damage_threshold: -1.0,
                ability_name: "Staged Stun".into(),
                ..Default::default()
            };
            world.get_mut::<ActiveAuras>(target).unwrap().auras = vec![siphon, stun];
        });
    let run = Run::play(config, stage);
    let siphon = run.leech_lines("Siphon Life");
    assert_eq!(siphon.len(), 1, "one tick healed:\n{}", run.played.log);
    let over: Vec<&Number> = run
        .numbers
        .iter()
        .filter(|n| n.heal && n.stamp == siphon[0].stamp)
        .collect();
    assert_eq!(over.len(), 1, "one heal number then: {over:?}");
    assert!(
        over[0].bare,
        "the Warlock carried no auras as it was healed: {over:?}"
    );
    run.assert_every_leech_heal_floats_its_number();
}
