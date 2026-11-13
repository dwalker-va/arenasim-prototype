//! AS-494 — a combatant's Results HEAL total is the sum of its healing
//! breakdown, and a killing-blow save's heal is a row in it.
//!
//! The Results screen shows two numbers for what a combatant healed: its HEAL
//! total (`Combatant::healing_done`, credited through
//! `combat_core::credit_healing_done`) and the per-ability breakdown under it
//! (the combat log's `log_healing` rows). They are summed from independent
//! sources, so every heal site must credit the one and log the other with the
//! same amount. Guardian Spirit's save heal and Devour Magic's logged a plain
//! line and no row; Holy Shock's and Holy Nova's heals logged a row and
//! credited nothing. `tests/heal_site_audit.rs` holds every site to the shape;
//! this plays it:
//!
//! 1. **The totals**: over a seeded batch of healer comps (a Holy Priest with
//!    Guardian Spirit and Holy Nova, a Paladin, a Druid, a Shaman, Warlocks
//!    draining and coiling), every combatant's total — pets' included —
//!    equals the sum of its rows, and the batch is not vacuous: each kind of
//!    heal it exists for occurred.
//! 2. **The save**, as a staged pair: a Mage beating on a Priest's Warrior
//!    teammate at 4% health, the Priest silenced so no heal of its own lands,
//!    with Guardian Spirit landed on the Warrior. Its heal is one row in the
//!    Priest's breakdown, under Guardian Spirit — and in the control, the
//!    same match with no spirit, where the Warrior dies unsaved, there is none.

use std::collections::{BTreeMap, BTreeSet};

use arenasim::combat::log::{CombatLog, StructuredEventData};
use arenasim::headless::runner::{
    build_headless_app, build_headless_app_staged, HeadlessMatchState,
};
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::components::{Aura, AuraType, Combatant, Pet};
use arenasim::states::play_match::talents::{Build, TalentId::*};
use arenasim::states::play_match::utils::{combat_log_id_for, combatant_id};
use arenasim::states::play_match::AbilityType;
use bevy::prelude::App;

const PRIEST: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(1, 1);
const MAGE: Unit = Unit::new(2, 0);

/// Every Holy node, Guardian Spirit among them.
fn full_holy() -> Build {
    Build::of(&[
        (HolySpecialization, 1),
        (SpiritualHealing, 1),
        (Inspiration, 2),
        (BlessedRecovery, 1),
        (HolyConcentration, 1),
        (HolyNova, 1),
        (GuardianSpirit, 1),
    ])
}

/// A match played to its end: its combat log, and every combatant's HEAL
/// total (pets' included), by log id.
struct Finished {
    log: CombatLog,
    totals: BTreeMap<String, f32>,
}

fn run(mut app: App) -> Finished {
    for _ in 0..(60 * 60 * 5) {
        app.update();
        if app.world().resource::<HeadlessMatchState>().match_complete {
            break;
        }
    }
    let world = app.world_mut();
    assert!(
        world.resource::<HeadlessMatchState>().match_complete,
        "the match ran to its end"
    );
    let log = std::mem::take(&mut *world.resource_mut::<CombatLog>());
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    let totals = q
        .iter(world)
        .map(|(c, pet)| (combat_log_id_for(c, pet), c.healing_done))
        .collect();
    Finished { log, totals }
}

// ---------------------------------------------------------------------------
// The totals
// ---------------------------------------------------------------------------

/// The heals the batch exists for, by row name: each must occur in it, so a
/// total that agrees with its rows is not agreeing over nothing.
const BATCH_HEALS: &[&str] = &[
    "Flash Heal",
    "Guardian Spirit",
    "Holy Nova",
    "Flash of Light",
    "Holy Shock",
    "Rejuvenation",
    "Lifebloom",
    "Swiftmend",
    "Lesser Healing Wave",
    "Healing Stream Totem",
    "Drain Life",
    "Death Coil",
];

#[test]
fn every_heal_total_is_the_sum_of_its_breakdown() {
    let comps: [([&str; 2], [&str; 2]); 3] = [
        (["Warlock", "Priest"], ["Warrior", "Paladin"]),
        (["Warlock", "Druid"], ["Mage", "Priest"]),
        (["Warrior", "Shaman"], ["Warlock", "Paladin"]),
    ];
    let builds = |team: [&str; 2]| -> Vec<Build> {
        team.iter()
            .map(|c| {
                if *c == "Priest" {
                    full_holy()
                } else {
                    Build::default()
                }
            })
            .collect()
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut healers = 0;
    let mut bad = Vec::new();
    for map in ["BasicArena", "PillaredArena"] {
        for seed in 0..8 {
            for (t1, t2) in comps {
                let config = HeadlessMatchConfig {
                    team1: t1.iter().map(|c| c.to_string()).collect(),
                    team2: t2.iter().map(|c| c.to_string()).collect(),
                    random_seed: Some(seed),
                    map: map.into(),
                    team1_builds: builds(t1),
                    team2_builds: builds(t2),
                    ..Default::default()
                };
                let finished = run(build_headless_app(&config, true, None));
                for entry in &finished.log.entries {
                    if let Some(StructuredEventData::Healing { ability, .. }) =
                        &entry.structured_data
                    {
                        seen.insert(ability.clone());
                    }
                }
                for (id, total) in &finished.totals {
                    let rows = finished.log.total_healing_done(id);
                    if *total > 0.0 {
                        healers += 1;
                    }
                    // The two sum the same heals in different orders.
                    if (rows - total).abs() > 1e-3 * total.max(1.0) {
                        bad.push(format!(
                            "{map} seed {seed} {t1:?} v {t2:?}: {id} HEAL {total}, rows {rows} \
                             {:?}",
                            finished.log.healing_by_ability(id)
                        ));
                    }
                }
            }
        }
    }
    let missing: Vec<_> = BATCH_HEALS.iter().filter(|h| !seen.contains(**h)).collect();
    assert!(
        missing.is_empty(),
        "the batch never logged a row for {missing:?} (saw {seen:?})"
    );
    assert!(healers > 0, "no combatant healed");
    assert!(
        bad.is_empty(),
        "A HEAL total is not the sum of its breakdown rows:\n{}",
        bad.join("\n")
    );
}

// ---------------------------------------------------------------------------
// The save
// ---------------------------------------------------------------------------

fn silenced() -> Aura {
    Aura {
        effect_type: AuraType::Silence,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// One played match: its combat log and the Priest's HEAL total.
struct Played {
    log: CombatLog,
    priest_healing_done: f32,
}

fn play(spirited: bool) -> Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Mage".into()],
        random_seed: Some(3),
        max_duration_secs: 8.0,
        team1_builds: vec![full_holy()],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(PRIEST, -20.0, 0.0)
        .place(WARRIOR, 0.0, 0.0)
        .place(MAGE, 20.0, 0.0)
        .health(WARRIOR, 0.04)
        .target(MAGE, WARRIOR)
        .target(WARRIOR, MAGE)
        .aura(PRIEST, silenced());
    let stage = if spirited {
        stage.land(WARRIOR, AbilityType::GuardianSpirit, PRIEST)
    } else {
        stage
    };
    let finished = run(build_headless_app_staged(&config, true, None, stage));
    let priest_healing_done = finished
        .totals
        .get(&priest_id())
        .copied()
        .expect("the Priest");
    Played {
        log: finished.log,
        priest_healing_done,
    }
}

fn priest_id() -> String {
    combatant_id(1, 0, CharacterClass::Priest)
}

fn warrior_id() -> String {
    combatant_id(1, 1, CharacterClass::Warrior)
}

/// The Priest's breakdown, as the Results screen reads it.
fn breakdown(played: &Played) -> BTreeMap<String, f32> {
    played
        .log
        .healing_by_ability(&priest_id())
        .into_iter()
        .collect()
}

/// Every message logged, for a failure.
fn text(log: &CombatLog) -> String {
    log.entries
        .iter()
        .map(|e| format!("[{:.2}] {}", e.timestamp, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_guardian_spirit_save_heal_is_a_row_under_guardian_spirit() {
    let saved = play(true);
    let log = &saved.log;
    // The save heal's line, and the row it is.
    let lines: Vec<_> = log
        .entries
        .iter()
        .filter(|e| e.message.starts_with("[SAVED] Guardian Spirit heals"))
        .collect();
    assert_eq!(lines.len(), 1, "one save heal:\n{}", text(log));
    let Some(StructuredEventData::Healing {
        source,
        target,
        ability,
        amount,
        is_crit,
    }) = &lines[0].structured_data
    else {
        panic!("the save heal is not a structured row:\n{}", text(log));
    };
    assert_eq!(
        (source.as_str(), target.as_str(), ability.as_str(), *is_crit),
        (
            priest_id().as_str(),
            warrior_id().as_str(),
            "Guardian Spirit",
            false
        )
    );
    assert!(*amount > 0.0, "the save healed: {amount}");
    assert_eq!(
        lines[0].message,
        format!(
            "[SAVED] Guardian Spirit heals {} for {amount:.0}",
            warrior_id()
        ),
        "the row's amount is the line's"
    );
    // The silenced Priest's whole breakdown is the save's heal, and its HEAL
    // total is that breakdown.
    assert_eq!(
        breakdown(&saved),
        BTreeMap::from([("Guardian Spirit".to_string(), *amount)]),
        "{}",
        text(log)
    );
    assert_eq!(saved.priest_healing_done, *amount);
}

#[test]
fn with_no_save_there_is_no_guardian_spirit_row() {
    let control = play(false);
    let log = &control.log;
    // The situation: the Warrior died unsaved.
    assert!(
        log.entries.iter().any(|e| e
            .message
            .contains(&format!("{} has been eliminated", warrior_id())))
            && !log.entries.iter().any(|e| e.message.contains("[SAVED]")),
        "{}",
        text(log)
    );
    assert_eq!(breakdown(&control), BTreeMap::new(), "{}", text(log));
    assert_eq!(control.priest_healing_done, 0.0);
}
