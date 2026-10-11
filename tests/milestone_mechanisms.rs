//! Per-match mechanism metrics for a milestone balance sweep — a MEASUREMENT a
//! human reads, not an assertion.
//!
//! Win rate is one bit a match. The questions a milestone sweep also has to
//! answer are about what happened INSIDE matches: how a healer's mana runs
//! down, how often the Druid casts Cyclone, whether it dies shifted, whether a
//! root it never broke was one it could not afford to break, which traps a
//! Hunter lays and on whom. This runs a JSONL of ordinary
//! `HeadlessMatchConfig`s (the `--batch` input format) through the OBSERVED
//! runner, and writes one JSON row per match carrying those raw per-unit
//! quantities. Aggregation is `docs/design/balance/sweeps/
//! 2026-10-10-as164-mechanisms.py`, so every rate in the findings doc
//! recomputes from the committed rows.
//!
//! The observer is read-only (`observed_run_does_not_perturb_outcomes`), so a
//! row's winner and duration are the batch runner's for the same config.
//!
//! ```bash
//! MECH_IN=sweep.jsonl MECH_OUT=rows.jsonl MECH_LOGS=/tmp/logs MECH_JOBS=6 \
//!   cargo test --release --test milestone_mechanisms -- --ignored --nocapture
//! ```
//!
//! What each per-unit field means is documented on [`UnitRow`].

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::AuraType;
use arenasim::states::play_match::constants::{DAMPENING_RAMP_SECS, DAMPENING_START_SECS};
use arenasim::CharacterClass;
use bevy::prelude::Entity;
use serde::Serialize;

/// The countdown before the gates; `sim_time` includes it.
const COUNTDOWN_SECS: f32 = 10.0;
/// Mana is sampled every this many seconds after the gates.
const MANA_SAMPLE_SECS: f32 = 5.0;

/// One root (any `AuraType::Root`) on one unit, from onset to its end.
#[derive(Serialize, Default, Clone)]
struct RootEpisode {
    /// Seconds after the gates.
    start: f32,
    end: f32,
    /// The unit entered Travel Form while the root was on it.
    broken_by_shift: bool,
    mana_at_start: f32,
    /// The most mana the unit held at any frame of the root.
    max_mana_during: f32,
}

#[derive(Serialize, Default)]
struct UnitRow {
    team: u8,
    slot: u8,
    class: String,
    /// Seconds after the gates the unit died, or `None` if it lived.
    death: Option<f32>,
    /// Travel Form was on the unit on its last living frame.
    died_shifted: bool,
    max_mana: f32,
    /// The class's cheapest heal that needs nothing else to land (a Swiftmend
    /// needs a heal over time to consume, so it is not counted). Zero for a
    /// class without a heal.
    cheapest_heal: f32,
    /// First post-gate second the unit could not afford `cheapest_heal`.
    first_oom: Option<f32>,
    /// Seconds below `cheapest_heal`, over the post-gate frames it was alive.
    oom_secs: f32,
    /// The same, counting only frames before dampening reaches 50%
    /// (`druid_report`'s horizon), so it is comparable across match lengths.
    oom_secs_before_half_dampening: f32,
    /// Mana fraction at every `MANA_SAMPLE_SECS` after the gates while alive
    /// and the match running (index i = (i+1) x 5s).
    mana_frac: Vec<f32>,
    /// Travel Form onsets, and seconds spent in the form.
    shifts: u32,
    shifted_secs: f32,
    /// Cyclone onsets ON this unit.
    cyclones_taken: u32,
    /// Interrupt lockouts (`AuraType::SpellSchoolLockout` onsets) ON this unit: a
    /// Kick, Pummel, Spell Lock or Wind Shear that caught a cast.
    lockouts_taken: u32,
    roots: Vec<RootEpisode>,
    /// Uses by this unit, by ability name: its `casts`, `fires` and `uses` log
    /// lines. Most classes log a cast-time spell's START as `begins casting`
    /// (not counted) and its landing elsewhere; the Druid logs `casts` at the
    /// START of Cyclone and Entangling Roots, so for those two this counts
    /// ATTEMPTS, interrupted ones included. `cyclones_taken` counts landings.
    casts: BTreeMap<String, u32>,
    /// `[TRAP]` triggers of this unit's traps: "Freezing:<victim class>" or
    /// "Frost:<victim class>".
    trap_triggers: BTreeMap<String, u32>,
    /// Every trap this unit threw, in order: (seconds after the gates, name).
    trap_uses: Vec<(f32, String)>,
    /// Seconds alive after the gates.
    alive_secs: f32,
    /// Damage dealt over the match, pet credit included (`CombatantResult`).
    damage_dealt: f32,
}

#[derive(Serialize)]
struct MatchRow {
    label: String,
    map: String,
    team1: String,
    team2: String,
    seed: u64,
    winner: String,
    duration: f32,
    units: Vec<UnitRow>,
}

fn cheapest_heal(defs: &AbilityDefinitions, class: CharacterClass) -> f32 {
    defs.own_abilities_for_class(class)
        .into_iter()
        .map(|a| defs.get_unchecked(&a))
        .filter(|c| c.consumes_aura.is_none() && c.mana_cost > 0.0)
        .filter(|c| {
            c.healing_base_min > 0.0
                || c.applies_aura
                    .as_ref()
                    .is_some_and(|a| a.aura_type == AuraType::HealingOverTime)
        })
        .map(|c| c.mana_cost)
        .fold(0.0f32, |acc, m| if acc == 0.0 { m } else { acc.min(m) })
}

/// `Team 1 Druid #2` for a unit.
fn log_name(team: u8, slot: u8, class: &str) -> String {
    format!("Team {team} {class} #{}", slot + 1)
}

fn class_of_log_name(name: &str) -> String {
    // "Team 2 Priest #2 — Incapacitated..." -> "Priest"
    name.split_whitespace().nth(2).unwrap_or("?").to_string()
}

/// `[  25.08s] ...` -> 25.08 (log time includes the countdown).
fn parse_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

fn parse_log(log: &str, units: &mut [UnitRow]) {
    let names: Vec<String> = units
        .iter()
        .map(|u| log_name(u.team, u.slot, &u.class))
        .collect();
    for line in log.lines() {
        if let Some(i) = line.find("] [CAST] ") {
            let body = &line[i + 9..];
            for (u, name) in units.iter_mut().zip(&names) {
                // "casts" (spells), "fires" (shots), "uses" (abilities); a
                // "begins casting" line is a start, not a completion.
                for verb in ["casts", "fires", "uses"] {
                    let prefix = format!("{name} {verb} ");
                    if let Some(rest) = body.strip_prefix(&prefix) {
                        let ability = rest.split(" on ").next().unwrap_or(rest).trim();
                        *u.casts.entry(ability.to_string()).or_default() += 1;
                        if ability.ends_with(" Trap") {
                            if let Some(t) = parse_time(line) {
                                u.trap_uses.push((t - COUNTDOWN_SECS, ability.to_string()));
                            }
                        }
                    }
                }
            }
        } else if let Some(i) = line.find("[TRAP] ") {
            let body = &line[i + 7..];
            for (u, name) in units.iter_mut().zip(&names) {
                for (kind, key) in [("Freezing Trap", "Freezing"), ("Frost Trap", "Frost")] {
                    let prefix = format!("{name}'s {kind} triggers on ");
                    if let Some(rest) = body.strip_prefix(&prefix) {
                        let victim = class_of_log_name(rest);
                        *u.trap_triggers.entry(format!("{key}:{victim}")).or_default() += 1;
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct Track {
    alive_prev: bool,
    shifted_prev: bool,
    cyclone_prev: bool,
    lockout_prev: bool,
    root: Option<RootEpisode>,
    next_sample: f32,
}

fn run_one(
    cfg: HeadlessMatchConfig,
    defs: &AbilityDefinitions,
    log_path: String,
) -> MatchRow {
    let mut cfg = cfg;
    cfg.output_path = Some(log_path.clone());
    let label = cfg.label.clone().unwrap_or_default();
    let map = cfg.map.clone();
    let (team1, team2) = (cfg.team1.join("+"), cfg.team2.join("+"));
    let seed = cfg.random_seed.unwrap_or(0);

    let mut units: BTreeMap<Entity, UnitRow> = BTreeMap::new();
    let mut tracks: BTreeMap<Entity, Track> = BTreeMap::new();
    const FRAME: f32 = 1.0 / 60.0;
    let half_dampening = DAMPENING_START_SECS + DAMPENING_RAMP_SECS * 0.5;

    let result = run_headless_match_observed(cfg, false, None, |frame| {
        if !frame.gates_open {
            return;
        }
        let t = frame.sim_time - COUNTDOWN_SECS;
        for (e, c) in frame.combatants.iter().filter(|(_, c)| !c.is_pet) {
            let u = units.entry(*e).or_insert_with(|| UnitRow {
                team: c.team,
                slot: c.slot,
                class: format!("{:?}", c.class),
                max_mana: c.max_mana,
                cheapest_heal: cheapest_heal(defs, c.class),
                ..Default::default()
            });
            let tr = tracks.entry(*e).or_insert_with(|| Track {
                alive_prev: true,
                next_sample: MANA_SAMPLE_SECS,
                ..Default::default()
            });
            let shifted = c.aura_types.contains(&AuraType::TravelForm);
            if !c.alive {
                if tr.alive_prev {
                    u.death = Some(t);
                    u.died_shifted = tr.shifted_prev;
                    if let Some(mut r) = tr.root.take() {
                        r.end = t;
                        u.roots.push(r);
                    }
                }
                tr.alive_prev = false;
                continue;
            }
            u.alive_secs += FRAME;
            if u.cheapest_heal > 0.0 && c.current_mana < u.cheapest_heal {
                u.oom_secs += FRAME;
                if t < half_dampening {
                    u.oom_secs_before_half_dampening += FRAME;
                }
                u.first_oom.get_or_insert(t);
            }
            if t >= tr.next_sample {
                u.mana_frac.push(if c.max_mana > 0.0 {
                    c.current_mana / c.max_mana
                } else {
                    0.0
                });
                tr.next_sample += MANA_SAMPLE_SECS;
            }
            if shifted {
                u.shifted_secs += FRAME;
                if !tr.shifted_prev {
                    u.shifts += 1;
                }
            }
            let cyclone = c.aura_types.contains(&AuraType::Cyclone);
            if cyclone && !tr.cyclone_prev {
                u.cyclones_taken += 1;
            }
            let lockout = c.aura_types.contains(&AuraType::SpellSchoolLockout);
            if lockout && !tr.lockout_prev {
                u.lockouts_taken += 1;
            }
            tr.lockout_prev = lockout;
            let rooted = c.aura_types.contains(&AuraType::Root);
            match (&mut tr.root, rooted) {
                (None, true) => {
                    tr.root = Some(RootEpisode {
                        start: t,
                        end: t,
                        broken_by_shift: false,
                        mana_at_start: c.current_mana,
                        max_mana_during: c.current_mana,
                    })
                }
                (Some(r), true) => {
                    r.max_mana_during = r.max_mana_during.max(c.current_mana);
                    if shifted && !tr.shifted_prev {
                        r.broken_by_shift = true;
                    }
                }
                (Some(_), false) => {
                    let mut r = tr.root.take().unwrap();
                    r.end = t;
                    // The shift that breaks a root removes it on the frame the
                    // form lands, so the onset is seen on the first unrooted
                    // frame.
                    if shifted && !tr.shifted_prev {
                        r.broken_by_shift = true;
                    }
                    u.roots.push(r);
                }
                (None, false) => {}
            }
            tr.shifted_prev = shifted;
            tr.cyclone_prev = cyclone;
            tr.alive_prev = true;
        }
    })
    .expect("match runs");

    for (e, tr) in tracks.iter_mut() {
        if let Some(mut r) = tr.root.take() {
            r.end = result.match_time;
            units.get_mut(e).unwrap().roots.push(r);
        }
    }
    let mut units: Vec<UnitRow> = units.into_values().collect();
    units.sort_by_key(|u| (u.team, u.slot));
    // Results are in query order, not slot order; a team never fields one
    // class twice, so (team, class) names the unit.
    for u in units.iter_mut() {
        let side = if u.team == 1 {
            &result.team1_combatants
        } else {
            &result.team2_combatants
        };
        if let Some(r) = side.iter().find(|r| r.class_name == u.class) {
            u.damage_dealt = r.damage_dealt;
        }
    }
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    parse_log(&log, &mut units);
    MatchRow {
        label,
        map,
        team1,
        team2,
        seed,
        winner: match result.winner {
            Some(1) => "team1".into(),
            Some(2) => "team2".into(),
            _ => "draw".into(),
        },
        duration: result.match_time,
        units,
    }
}

#[test]
#[ignore = "a measurement a human reads; run with MECH_IN / MECH_OUT / MECH_LOGS set"]
fn milestone_mechanisms() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("MECH_IN").expect("MECH_IN: the JSONL of match configs");
    let output = std::env::var("MECH_OUT").expect("MECH_OUT: where the rows go");
    let logs = std::env::var("MECH_LOGS").expect("MECH_LOGS: a directory for match logs");
    let jobs: usize = std::env::var("MECH_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    std::fs::create_dir_all(&logs).unwrap();

    let configs: Vec<HeadlessMatchConfig> = std::io::BufReader::new(
        std::fs::File::open(&input).expect("open MECH_IN"),
    )
    .lines()
    .map(|l| l.unwrap())
    .filter(|l| !l.trim().is_empty())
    .map(|l| serde_json::from_str(&l).expect("a HeadlessMatchConfig"))
    .collect();
    let total = configs.len();
    let configs = Arc::new(configs);
    let defs = Arc::new(AbilityDefinitions::default());
    let shift_cost = defs.get_unchecked(&AbilityType::TravelForm).mana_cost;
    let cursor = Arc::new(AtomicUsize::new(0));
    let rows: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(vec![None; total]));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (configs, defs, cursor, rows, logs) = (
                Arc::clone(&configs),
                Arc::clone(&defs),
                Arc::clone(&cursor),
                Arc::clone(&rows),
                logs.clone(),
            );
            scope.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let row = run_one(
                    configs[i].clone(),
                    &defs,
                    format!("{logs}/match_{i:06}.txt"),
                );
                rows.lock().unwrap()[i] = Some(serde_json::to_string(&row).unwrap());
                if (i + 1) % 200 == 0 {
                    eprintln!("{} / {}", i + 1, total);
                }
            });
        }
    });

    let mut out = std::io::BufWriter::new(std::fs::File::create(&output).unwrap());
    for row in rows.lock().unwrap().iter() {
        writeln!(out, "{}", row.as_ref().expect("every match ran")).unwrap();
    }
    println!("{total} matches -> {output} (Travel Form costs {shift_cost} mana)");
}
