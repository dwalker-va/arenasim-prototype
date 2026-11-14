//! AS-498 measurement: what a Hunter's Freezing Trap play costs and buys, per
//! Hunter side — a MEASUREMENT a human reads, not an assertion.
//!
//! Runs a JSONL of ordinary `HeadlessMatchConfig`s (the `--batch` input
//! format) through the OBSERVED runner and writes one JSON row per Hunter side:
//! whether it won, the traps it threw and whom they froze, and per-frame
//! quantities of its kite — melee contact, its own healer's sight of it, its
//! shot uptime, how far it stands from the enemy healer. With `TD_TRACE` set it
//! also counts the Hunter's trap-dip movement events from the decision trace.
//!
//! ```bash
//! TD_IN=sweep.jsonl TD_OUT=rows.jsonl TD_LOGS=/tmp/logs TD_JOBS=4 \
//!   cargo test --release --test hunter_trap_dip_measure -- --ignored --nocapture
//! ```
//!
//! The observer is read-only (`observed_run_does_not_perturb_outcomes`), so a
//! row's winner is the batch runner's for the same config.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, EYE_HEIGHT};
use arenasim::CharacterClass;
use bevy::prelude::{Entity, Vec3};
use serde::Serialize;

/// The countdown before the gates; `sim_time` includes it.
const COUNTDOWN_SECS: f32 = 10.0;
const FRAME: f32 = 1.0 / 60.0;
/// An enemy melee or pet this close to the Hunter, targeting it, is striking.
const CONTACT_YARDS: f32 = 5.0;
/// The Hunter's healer's reach.
const HEAL_RANGE: f32 = 40.0;
/// Auto Shot's band.
const SHOT_MIN: f32 = 8.0;
const SHOT_MAX: f32 = 35.0;

#[derive(Serialize, Default)]
struct HunterRow {
    label: String,
    map: String,
    profile: String,
    team: u8,
    comp: String,
    enemy: String,
    seed: u64,
    /// "win" / "loss" / "draw", from the Hunter's side.
    result: String,
    duration: f32,
    hunter_died: bool,
    hunter_death: Option<f32>,
    /// A living non-pet teammate of the Hunter's died.
    partner_died: bool,
    damage_taken: f32,
    damage_dealt: f32,
    alive_secs: f32,
    /// Seconds an enemy melee or pet targeting the Hunter stood within
    /// `CONTACT_YARDS` of it.
    melee_contact_secs: f32,
    /// Seconds the Hunter's own living healer could not heal it: out of sight
    /// or beyond `HEAL_RANGE`. Zero with no healer.
    own_healer_blind_secs: f32,
    /// Seconds with the Hunter's living target in Auto Shot's band and sight.
    shot_secs: f32,
    /// Seconds hidden from every living enemy (no sight line).
    hidden_secs: f32,
    /// Sum of the Hunter's distance to the nearest living enemy healer, over
    /// the frames one lived; divide by `enemy_healer_secs`.
    enemy_healer_dist_sum: f32,
    enemy_healer_secs: f32,
    /// Sum of distance to the Hunter's own living healer; divide by
    /// `own_healer_secs`.
    own_healer_dist_sum: f32,
    own_healer_secs: f32,
    /// Freezing Traps this Hunter threw.
    freezing_thrown: u32,
    /// Freezing Trap triggers on an enemy healer / on anyone else.
    healer_freezes: u32,
    other_freezes: u32,
    frost_thrown: u32,
    /// Damage the Hunter took, by the attacking unit's class (a pet's damage
    /// counts as its owner's): a melee class (Warrior, Rogue, Paladin), a
    /// healer that is not one (Priest, Shaman, Druid), or a ranged DPS
    /// (Mage, Warlock, Hunter). From the log's `[DMG]` lines.
    damage_from_melee: f32,
    damage_from_healer: f32,
    damage_from_ranged: f32,
    /// Trace counts (only with `TD_TRACE`): trap-dip movement events, and
    /// Freezing Traps chosen.
    dip_enter: u32,
    dip_complete: u32,
    dip_abort: u32,
    trap_chosen: u32,
}

fn log_name(team: u8, slot: u8, class: &str) -> String {
    format!("Team {team} {class} #{}", slot + 1)
}

fn flat_eye(v: Vec3) -> Vec3 {
    Vec3::new(v.x, EYE_HEIGHT, v.z)
}

fn planar(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

#[derive(Default)]
struct Acc {
    team: u8,
    slot: u8,
    alive: bool,
    death: Option<f32>,
    alive_secs: f32,
    melee_contact_secs: f32,
    own_healer_blind_secs: f32,
    shot_secs: f32,
    hidden_secs: f32,
    enemy_healer_dist_sum: f32,
    enemy_healer_secs: f32,
    own_healer_dist_sum: f32,
    own_healer_secs: f32,
}

fn run_one(
    cfg: HeadlessMatchConfig,
    log_path: String,
    trace_path: Option<String>,
) -> Vec<HunterRow> {
    let mut cfg = cfg;
    cfg.output_path = Some(log_path.clone());
    let label = cfg.label.clone().unwrap_or_default();
    let map = cfg.map.clone();
    let profile = cfg.ai_profile.clone().unwrap_or_else(|| "Legacy".into());
    let (team1, team2) = (cfg.team1.join("+"), cfg.team2.join("+"));
    let seed = cfg.random_seed.unwrap_or(0);
    let geometry = load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(HeadlessMatchConfig::parse_map(&map).expect("a known map"));
    let obstacles = geometry.volumes.clone();
    let sees = |a: Vec3, b: Vec3| has_line_of_sight(&obstacles, flat_eye(a), flat_eye(b));

    let mut hunters: BTreeMap<Entity, Acc> = BTreeMap::new();
    let mut partner_died = [false; 3];
    let trace = trace_path.clone().map(|p| TraceConfig {
        output_path: p.into(),
    });

    let result = run_headless_match_observed(cfg, false, trace, |frame| {
        if !frame.gates_open {
            return;
        }
        let t = frame.sim_time - COUNTDOWN_SECS;
        for (e, c) in frame.combatants.iter() {
            if !c.is_pet && c.class != CharacterClass::Hunter && !c.alive {
                partner_died[c.team as usize] = true;
            }
            if c.is_pet || c.class != CharacterClass::Hunter {
                continue;
            }
            let acc = hunters.entry(*e).or_insert_with(|| Acc {
                team: c.team,
                slot: c.slot,
                alive: true,
                ..Default::default()
            });
            if !c.alive {
                if acc.alive {
                    acc.death = Some(t);
                }
                acc.alive = false;
                continue;
            }
            acc.alive_secs += FRAME;
            let me = c.position;
            let enemies: Vec<_> = frame
                .combatants
                .iter()
                .filter(|(_, o)| o.team != c.team && o.alive)
                .collect();
            if enemies.iter().any(|(_, o)| {
                (o.is_pet || o.class.is_melee())
                    && o.target == Some(*e)
                    && planar(o.position, me) <= CONTACT_YARDS
            }) {
                acc.melee_contact_secs += FRAME;
            }
            if !enemies
                .iter()
                .filter(|(_, o)| !o.is_pet && !o.stealthed)
                .any(|(_, o)| sees(o.position, me))
            {
                acc.hidden_secs += FRAME;
            }
            if let Some(d) = enemies
                .iter()
                .filter(|(_, o)| !o.is_pet && o.class.is_healer())
                .map(|(_, o)| planar(o.position, me))
                .min_by(|a, b| a.total_cmp(b))
            {
                acc.enemy_healer_dist_sum += d;
                acc.enemy_healer_secs += FRAME;
            }
            if let Some(h) = frame
                .combatants
                .values()
                .find(|o| o.team == c.team && o.alive && !o.is_pet && o.class.is_healer())
            {
                let d = planar(h.position, me);
                acc.own_healer_dist_sum += d;
                acc.own_healer_secs += FRAME;
                if d > HEAL_RANGE || !sees(h.position, me) {
                    acc.own_healer_blind_secs += FRAME;
                }
            }
            if let Some(tgt) = c
                .target
                .and_then(|t| frame.combatants.get(&t))
                .filter(|o| o.alive)
            {
                let d = planar(tgt.position, me);
                if (SHOT_MIN..=SHOT_MAX).contains(&d) && sees(tgt.position, me) {
                    acc.shot_secs += FRAME;
                }
            }
        }
    })
    .expect("match runs");

    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    let _ = std::fs::remove_file(&log_path);
    let trace_lines: Vec<serde_json::Value> = trace_path
        .as_ref()
        .and_then(|p| {
            let text = std::fs::read_to_string(p).ok();
            let _ = std::fs::remove_file(p);
            text
        })
        .map(|text| {
            text.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default();

    hunters
        .into_values()
        .map(|acc| {
            let (comp, enemy) = if acc.team == 1 {
                (team1.clone(), team2.clone())
            } else {
                (team2.clone(), team1.clone())
            };
            let side = if acc.team == 1 {
                &result.team1_combatants
            } else {
                &result.team2_combatants
            };
            let me = side.iter().find(|r| r.class_name == "Hunter");
            let name = log_name(acc.team, acc.slot, "Hunter");
            let mut row = HunterRow {
                label: label.clone(),
                map: map.clone(),
                profile: profile.clone(),
                team: acc.team,
                comp,
                enemy,
                seed,
                result: match result.winner {
                    Some(w) if w == acc.team => "win".into(),
                    Some(_) => "loss".into(),
                    None => "draw".into(),
                },
                duration: result.match_time,
                hunter_died: acc.death.is_some(),
                hunter_death: acc.death,
                partner_died: partner_died[acc.team as usize],
                damage_taken: me.map_or(0.0, |r| r.damage_taken),
                damage_dealt: me.map_or(0.0, |r| r.damage_dealt),
                alive_secs: acc.alive_secs,
                melee_contact_secs: acc.melee_contact_secs,
                own_healer_blind_secs: acc.own_healer_blind_secs,
                shot_secs: acc.shot_secs,
                hidden_secs: acc.hidden_secs,
                enemy_healer_dist_sum: acc.enemy_healer_dist_sum,
                enemy_healer_secs: acc.enemy_healer_secs,
                own_healer_dist_sum: acc.own_healer_dist_sum,
                own_healer_secs: acc.own_healer_secs,
                ..Default::default()
            };
            for line in log.lines() {
                if let Some(i) = line.find("] [CAST] ") {
                    let body = &line[i + 9..];
                    if body.starts_with(&format!("{name} ")) {
                        if body.contains("Freezing Trap") {
                            row.freezing_thrown += 1;
                        } else if body.contains("Frost Trap") {
                            row.frost_thrown += 1;
                        }
                    }
                } else if let Some(i) = line.find("] [DMG] ") {
                    let body = &line[i + 8..];
                    let victim = format!(" {name} for ");
                    if let Some(at) = body.find(&victim) {
                        let amount: f32 = body[at + victim.len()..]
                            .split_whitespace()
                            .next()
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0.0);
                        match body.split_whitespace().nth(2).unwrap_or("?") {
                            "Warrior" | "Rogue" | "Paladin" => row.damage_from_melee += amount,
                            "Priest" | "Shaman" | "Druid" => row.damage_from_healer += amount,
                            _ => row.damage_from_ranged += amount,
                        }
                    }
                } else if let Some(i) = line.find("[TRAP] ") {
                    let body = &line[i + 7..];
                    if let Some(rest) =
                        body.strip_prefix(&format!("{name}'s Freezing Trap triggers on "))
                    {
                        let victim = rest.split_whitespace().nth(2).unwrap_or("?");
                        if matches!(victim, "Priest" | "Paladin" | "Shaman" | "Druid") {
                            row.healer_freezes += 1;
                        } else {
                            row.other_freezes += 1;
                        }
                    }
                }
            }
            for ev in &trace_lines {
                let actor = &ev["actor"];
                if actor["team"].as_u64() != Some(acc.team as u64)
                    || actor["class"].as_str() != Some("Hunter")
                {
                    continue;
                }
                match ev["kind"].as_str() {
                    Some("movement_decision") => match ev["trigger"].as_str() {
                        Some("DipEnter") => row.dip_enter += 1,
                        Some("DipComplete") => row.dip_complete += 1,
                        Some("DipAbort") => row.dip_abort += 1,
                        _ => {}
                    },
                    Some("ability_decision")
                        if ev["outcome"]["ability"].as_str() == Some("FreezingTrap") =>
                    {
                        row.trap_chosen += 1;
                    }
                    _ => {}
                }
            }
            row
        })
        .collect()
}

#[test]
#[ignore = "a measurement a human reads; run with TD_IN / TD_OUT / TD_LOGS set"]
fn hunter_trap_dip_measure() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("TD_IN").expect("TD_IN: the JSONL of match configs");
    let output = std::env::var("TD_OUT").expect("TD_OUT: where the rows go");
    let logs = std::env::var("TD_LOGS").expect("TD_LOGS: a scratch directory for match logs");
    let traced = std::env::var("TD_TRACE").is_ok();
    let jobs: usize = std::env::var("TD_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    std::fs::create_dir_all(&logs).unwrap();

    let configs: Vec<HeadlessMatchConfig> =
        std::io::BufReader::new(std::fs::File::open(&input).expect("open TD_IN"))
            .lines()
            .map(|l| l.unwrap())
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("a HeadlessMatchConfig"))
            .collect();
    let total = configs.len();
    let configs = Arc::new(configs);
    let cursor = Arc::new(AtomicUsize::new(0));
    let rows: Arc<Mutex<Vec<Option<Vec<String>>>>> = Arc::new(Mutex::new(vec![None; total]));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (configs, cursor, rows, logs) = (
                Arc::clone(&configs),
                Arc::clone(&cursor),
                Arc::clone(&rows),
                logs.clone(),
            );
            scope.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let out = run_one(
                    configs[i].clone(),
                    format!("{logs}/match_{i:06}.txt"),
                    traced.then(|| format!("{logs}/match_{i:06}_trace.jsonl")),
                );
                rows.lock().unwrap()[i] = Some(
                    out.iter()
                        .map(|r| serde_json::to_string(r).unwrap())
                        .collect(),
                );
                if (i + 1) % 500 == 0 {
                    eprintln!("{} / {}", i + 1, total);
                }
            });
        }
    });

    let mut out = std::io::BufWriter::new(std::fs::File::create(&output).unwrap());
    for row in rows.lock().unwrap().iter() {
        for line in row.as_ref().expect("every match ran") {
            writeln!(out, "{line}").unwrap();
        }
    }
    println!("{total} matches -> {output}");
}
