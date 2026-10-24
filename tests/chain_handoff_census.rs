//! Chain hand-off census — a MEASUREMENT a human reads, not an assertion
//! (AS-391).
//!
//! The Pillar play sits ahead of Chain in `plays::PLAY_ORDER`, so while a
//! team holds a pillar stance a unit with a hand-off plays Pillar and its
//! hand-off is no longer at the head of its chain. AS-385 counted the
//! decision ticks that moved; this counts the HAND-OFFS. It runs a JSONL of
//! ordinary `HeadlessMatchConfig`s (the `--batch` input format) through the
//! observed runner with the decision trace on, and writes one JSON row per
//! match, scored for the MEASURED team (the one whose `team{N}_ai_profile` is
//! `TeamPlan`; team 1 when both or neither are):
//!
//! - **windows** — the Chain play's windows, read off the measured team's own
//!   decisions (`situation.lock_ending`: an enemy it holds, and the lock's
//!   `until`), one per distinct (subject, `until`). Each ends in exactly one
//!   outcome:
//!   - `in_lead` — a hand-off (`chain::chain_tools`, any class) LANDED on the
//!     subject in the lock's last `chain::HANDOFF_LEAD` (combat-log
//!     application, two frames' tolerance either side);
//!   - `early` — one landed before that, while the window was open;
//!   - `broken` — the subject was free before the lead began (damage, a
//!     dispel, a trinket): nothing could be timed onto it;
//!   - `extended` — the subject was still held after `until` by something
//!     that is not a hand-off (a second lock already on it, a trap);
//!   - `late` — a hand-off landed within 1.5s after `until`;
//!   - `void` — the subject died, or the match ended, before `until`;
//!   - `freed` — the lock ran out and nothing followed. A freed window is
//!     MISSED when a teammate had a hand-off READY at its moment (the moment
//!     it would have had to begin: `until` less its cast time, less half the
//!     lead): alive, not held, not silenced, the mana, off cooldown, in range
//!     and in sight of the subject, the subject not immune (Divine Shield, a
//!     Cyclone) nor diminished to immunity in the tool's category, and — for
//!     a hand-off that breaks on damage or shields its subject — the team
//!     neither targeting the subject nor holding a damage over time on it
//!     (`chain::suits`), and — for a healer — no teammate below half health
//!     (the play does not call a healer then). Each missed window is
//!     attributed to the PLAY its ready unit was deciding under in the global
//!     cooldown before that moment: `pillar`, `chain` or `other`.
//! - **lockout** — what the measured team inflicted on the other, per frame
//!   (`cc_lifecycle_probe`'s metrics): denied, healer-locked and
//!   counterplay-free seconds, and the longest lockouts.
//! - **chain steps** — the Chain play's own choices (`play_step == "chain"`
//!   with a cast) and its held global cooldowns.
//!
//! Readiness is reconstructed from the frames, the trace and the combat log,
//! not from the AI's own gates, so it is an approximation (it does not see a
//! school lockout, a cast in progress, or the hold rule). It is the SAME
//! approximation in every arm, which is what a paired comparison needs.
//!
//! ```bash
//! CHC_IN=bank.jsonl CHC_OUT=rows.jsonl CHC_TMP=/tmp/chc CHC_JOBS=8 \
//!   cargo test --release --test chain_handoff_census -- --ignored --nocapture
//! ```
//! Aggregation: `docs/design/balance/sweeps/2026-10-24-as391-chain-score.py`.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::chain::{chain_tools, HANDOFF_LEAD};
use arenasim::states::play_match::class_ai::{grants_damage_immunity, holds};
use arenasim::states::play_match::components::{AuraType, DRCategory};
use arenasim::states::play_match::constants::{DR_RESET_TIMER, GCD};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::{ArenaMap, CharacterClass};
use bevy::prelude::{Vec2, Vec3};
use serde::Serialize;

const FRAME: f32 = 1.0 / 60.0;
/// Timing tolerance on a landing, either side: two frames, plus the combat
/// log's 10ms stamp resolution.
const TOL: f32 = 2.0 * FRAME + 0.01;
/// How long after `until` a hand-off still counts as `late`.
const LATE: f32 = 1.5;
/// `urgency_hp_threshold`: a healer does not chain while a teammate is below it.
const DYING: f32 = 0.5;

/// A hand-off tool's facts, read once from the config.
#[derive(Clone)]
struct Tool {
    ability: AbilityType,
    name: String,
    class: CharacterClass,
    cast: f32,
    range: f32,
    mana: f32,
    cooldown: f32,
    dr: Option<DRCategory>,
    /// Breaks on damage, or makes its subject immune to the team's damage.
    spoils: bool,
}

fn tools(defs: &AbilityDefinitions) -> Vec<Tool> {
    let mut out = Vec::new();
    for class in CharacterClass::all() {
        for &ability in chain_tools(*class) {
            let d = defs.get_unchecked(&ability);
            let aura = d.applies_aura.as_ref().expect("a hand-off applies an aura");
            out.push(Tool {
                ability,
                name: d.name.clone(),
                class: *class,
                cast: d.cast_time,
                range: d.range,
                mana: d.mana_cost,
                cooldown: d.cooldown,
                dr: aura
                    .dr_category
                    .or_else(|| DRCategory::from_aura_type(&aura.aura_type)),
                spoils: aura.break_on_damage >= 0.0 || grants_damage_immunity(aura.aura_type),
            });
        }
    }
    out
}

/// One combatant on one frame, as much as the census reads.
#[derive(Clone, Copy)]
struct U {
    pos: Vec2,
    team: u8,
    slot: u8,
    class: CharacterClass,
    pet: bool,
    alive: bool,
    mana: f32,
    hp: f32,
    held: bool,
    silenced: bool,
    immune: bool,
    dot: bool,
    target: Option<u32>,
}

#[derive(Serialize, Default, Clone)]
struct Lockout {
    denied: f32,
    healer_locked: f32,
    counterplay_free: f32,
    longest_lockout: f32,
    longest_healer_lockout: f32,
}

#[derive(Serialize, Default, Clone)]
struct Windows {
    total: u32,
    in_lead: u32,
    early: u32,
    broken: u32,
    extended: u32,
    late: u32,
    void: u32,
    freed: u32,
    /// Freed windows with a ready teammate, by the ready unit's play.
    missed_pillar: u32,
    missed_chain: u32,
    missed_other: u32,
    /// Windows during which a unit holding a hand-off decided under Pillar.
    pillar_exposed: u32,
    /// Hand-offs in the lead, by the landing tool's class.
    in_lead_by_class: BTreeMap<String, u32>,
    /// Missed windows, by the ready unit's class.
    missed_by_class: BTreeMap<String, u32>,
    /// Freed windows with no ready teammate, by the first reason the first
    /// tool holder was not ready.
    not_ready: BTreeMap<String, u32>,
}

#[derive(Serialize, Default)]
struct Row {
    label: String,
    map: String,
    seed: u64,
    measured: u8,
    team: String,
    opponent: String,
    winner: u8,
    won: bool,
    duration: f32,
    chain_casts: u32,
    chain_holds: u32,
    /// Hand-off landings on any enemy of the measured team (all timings).
    handoff_landings: u32,
    lockout: Lockout,
    windows: Windows,
}

struct Landing {
    t: f32,
    subject: (u8, u8),
    tool: usize,
}

fn run_one(cfg: HeadlessMatchConfig, tools: &[Tool], tmp: &str, i: usize) -> Row {
    let mut cfg = cfg;
    let log_path = format!("{tmp}/match_{i:06}.txt");
    cfg.output_path = Some(log_path.clone());
    let trace_path = std::path::PathBuf::from(format!("{tmp}/trace_{i:06}.jsonl"));
    let measured: u8 = match (&cfg.team1_ai_profile, &cfg.team2_ai_profile) {
        (Some(a), Some(b)) if a != b => {
            if a == "TeamPlan" {
                1
            } else {
                2
            }
        }
        _ => 1,
    };
    let victim = 3 - measured;
    let mut row = Row {
        label: cfg.label.clone().unwrap_or_default(),
        map: cfg.map.clone(),
        seed: cfg.random_seed.unwrap_or(0),
        measured,
        team: if measured == 1 {
            cfg.team1.join("+")
        } else {
            cfg.team2.join("+")
        },
        opponent: if measured == 1 {
            cfg.team2.join("+")
        } else {
            cfg.team1.join("+")
        },
        ..Default::default()
    };
    let arena: ArenaMap = *ArenaMap::all()
        .iter()
        .find(|m| format!("{m:?}") == cfg.map)
        .expect("a map name");
    let volumes: Vec<ObstacleVolume> = load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(arena)
        .volumes;
    let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);
    let sees = |a: Vec2, b: Vec2| volumes.is_empty() || has_line_of_sight(&volumes, eye(a), eye(b));

    let mut frames: Vec<(f32, bool, BTreeMap<u32, U>)> = Vec::new();
    let result = run_headless_match_observed(
        cfg,
        false,
        Some(TraceConfig {
            output_path: trace_path.clone(),
        }),
        |frame| {
            let units = frame
                .combatants
                .iter()
                .map(|(e, c)| {
                    (
                        e.index(),
                        U {
                            pos: Vec2::new(c.position.x, c.position.z),
                            team: c.team,
                            slot: c.slot,
                            class: c.class,
                            pet: c.is_pet,
                            alive: c.alive,
                            mana: c.current_mana,
                            hp: if c.max_health > 0.0 {
                                c.current_health / c.max_health
                            } else {
                                0.0
                            },
                            held: c.aura_types.iter().any(|a| holds(*a)),
                            silenced: c.aura_types.contains(&AuraType::Silence),
                            immune: c.aura_types.iter().any(|a| grants_damage_immunity(*a)),
                            dot: c.aura_types.contains(&AuraType::DamageOverTime),
                            target: c.target.map(|t| t.index()),
                        },
                    )
                })
                .collect();
            frames.push((frame.sim_time, frame.gates_open, units));
        },
    )
    .expect("match runs");
    row.winner = result.winner.unwrap_or(0);
    row.won = result.winner == Some(measured);
    row.duration = result.match_time;

    let Some(gate) = frames.iter().position(|f| f.1) else {
        return row;
    };
    let gate_app = frames[gate].0;
    let n = frames.len();
    let frame_at = |t: f32| -> usize { (gate + (t / FRAME).round().max(0.0) as usize).min(n - 1) };
    let end_t = (n - 1 - gate) as f32 * FRAME;

    // Lockout inflicted on the victim (`cc_lifecycle_probe`'s metrics).
    {
        let mut run = 0.0f32;
        let mut runs: BTreeMap<u32, f32> = BTreeMap::new();
        let lo = &mut row.lockout;
        for (_, open, units) in frames.iter() {
            if !open {
                continue;
            }
            let mut locked = 0;
            let mut healer = false;
            for (e, u) in units.iter().filter(|(_, u)| u.team == victim && !u.pet) {
                let r = runs.entry(*e).or_default();
                if u.alive && u.held {
                    locked += 1;
                    healer |= u.class.is_healer();
                    *r += FRAME;
                    lo.longest_lockout = lo.longest_lockout.max(*r);
                } else {
                    *r = 0.0;
                }
            }
            lo.denied += FRAME * locked as f32;
            if healer {
                lo.healer_locked += FRAME;
                run += FRAME;
                lo.longest_healer_lockout = lo.longest_healer_lockout.max(run);
            } else {
                run = 0.0;
            }
            if locked >= 2 && healer {
                lo.counterplay_free += FRAME;
            }
        }
    }

    // The trace: the measured team's windows, plays, casts.
    // (subject, since-first-seen, until)
    let mut windows: Vec<(u32, f32, f32)> = Vec::new();
    // per entity: (t, play name)
    let mut plays: BTreeMap<u32, Vec<(f32, String)>> = BTreeMap::new();
    // per entity: (t, ability) chosen
    let mut casts: BTreeMap<u32, Vec<(f32, AbilityType)>> = BTreeMap::new();
    if let Ok(f) = std::fs::File::open(&trace_path) {
        for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
            if !line.contains("\"ability_decision\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if v["actor"]["team"].as_u64() != Some(measured as u64) {
                continue;
            }
            let t = v["sim_time"].as_f64().unwrap_or(0.0) as f32;
            let id = v["actor"]["entity_id"].as_u64().unwrap_or(u64::MAX) as u32;
            let situation = &v["situation"];
            let play = situation["play"]["name"]
                .as_str()
                .unwrap_or("default")
                .to_string();
            if situation["play_step"] == "chain" {
                if v["outcome"]["ability"].is_string() {
                    row.chain_casts += 1;
                } else {
                    row.chain_holds += 1;
                }
            }
            if let Some(a) = v["outcome"]["ability"].as_str() {
                if let Ok(ability) =
                    serde_json::from_value::<AbilityType>(serde_json::Value::String(a.into()))
                {
                    casts.entry(id).or_default().push((t, ability));
                }
            }
            plays.entry(id).or_default().push((t, play));
            let le = &situation["lock_ending"];
            if let (Some(s), Some(u)) = (le["subject"].as_u64(), le["until"].as_f64()) {
                let (s, u) = (s as u32, u as f32);
                match windows.iter_mut().rev().find(|w| w.0 == s) {
                    Some(w) if (w.2 - u).abs() < 0.05 => {}
                    _ => windows.push((s, t, u)),
                }
            }
        }
    }
    let _ = std::fs::remove_file(&trace_path);

    // The combat log: every hand-off that landed on an enemy of the measured team.
    let mut landings: Vec<Landing> = Vec::new();
    // DR applications per (subject, category): times.
    let mut dr_apps: BTreeMap<((u8, u8), usize), Vec<f32>> = BTreeMap::new();
    let by_name: BTreeMap<&str, usize> = tools
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.as_str(), i))
        .collect();
    if let Ok(log) = std::fs::read_to_string(&log_path) {
        for line in log.lines() {
            let Some(rest) = line.split("] [CC] ").nth(1) else {
                continue;
            };
            if !line.contains(", DR: ") {
                continue;
            }
            let t_app: f32 = line
                .trim_start_matches('[')
                .split('s')
                .next()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(-1.0);
            let Some((ability, after)) = rest.split_once(" on Team ") else {
                continue;
            };
            let Some((who, tail)) = after.split_once(" (") else {
                continue;
            };
            let mut parts = who.split_whitespace();
            let team: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let Some(k) = who
                .rsplit_once('#')
                .and_then(|(_, k)| k.trim().parse::<u8>().ok())
            else {
                continue;
            };
            let subject = (team, k.saturating_sub(1));
            let dr_pct: i32 = tail
                .split("DR: ")
                .nth(1)
                .and_then(|s| s.trim_end_matches(')').trim_end_matches('%').parse().ok())
                .unwrap_or(0);
            let t = t_app - gate_app;
            if let Some(&ti) = by_name.get(ability) {
                if let Some(cat) = tools[ti].dr {
                    dr_apps.entry((subject, cat.index())).or_default().push(t);
                }
                if dr_pct > 0 && team == victim {
                    landings.push(Landing {
                        t,
                        subject,
                        tool: ti,
                    });
                }
            }
        }
    }
    let _ = std::fs::remove_file(&log_path);
    row.handoff_landings = landings.len() as u32;

    // DR level of (subject, cat) just before t: the engine's tracker replayed.
    let dr_immune = |subject: (u8, u8), cat: DRCategory, t: f32| -> bool {
        let Some(apps) = dr_apps.get(&(subject, cat.index())) else {
            return false;
        };
        let (mut level, mut last) = (0u8, f32::NEG_INFINITY);
        for &a in apps.iter().filter(|a| **a < t) {
            if a - last > DR_RESET_TIMER {
                level = 0;
            }
            if level < 3 {
                level += 1;
                last = a;
            }
        }
        t - last <= DR_RESET_TIMER && level >= 3
    };

    let slot_of = |e: u32| -> Option<(u8, u8)> { frames[gate].2.get(&e).map(|u| (u.team, u.slot)) };
    let held_at = |e: u32, t: f32| -> bool {
        frames[frame_at(t)]
            .2
            .get(&e)
            .is_some_and(|u| u.alive && u.held)
    };
    let alive_at =
        |e: u32, t: f32| -> bool { frames[frame_at(t)].2.get(&e).is_some_and(|u| u.alive) };

    // Units on the measured team holding a hand-off.
    let holders: Vec<(u32, CharacterClass)> = frames[gate]
        .2
        .iter()
        .filter(|(_, u)| u.team == measured && !u.pet && !chain_tools(u.class).is_empty())
        .map(|(e, u)| (*e, u.class))
        .collect();

    let w = &mut row.windows;
    for &(s, first, until) in &windows {
        w.total += 1;
        let Some(subject) = slot_of(s) else {
            continue;
        };
        if holders.iter().any(|(e, _)| {
            plays.get(e).is_some_and(|v| {
                v.iter()
                    .any(|(t, p)| *t >= first && *t <= until && p == "pillar")
            })
        }) {
            w.pillar_exposed += 1;
        }
        let on_subject = |lo: f32, hi: f32| {
            landings
                .iter()
                .find(|l| l.subject == subject && l.t >= lo && l.t <= hi)
        };
        if until + 0.1 > end_t || !alive_at(s, until + 0.1) {
            w.void += 1;
            continue;
        }
        if let Some(l) = on_subject(until - HANDOFF_LEAD - TOL, until + TOL) {
            w.in_lead += 1;
            *w.in_lead_by_class
                .entry(tools[l.tool].class.name().to_string())
                .or_default() += 1;
            continue;
        }
        if !held_at(s, until - HANDOFF_LEAD - 0.05) {
            w.broken += 1;
            continue;
        }
        if on_subject(first - TOL, until - HANDOFF_LEAD - TOL).is_some() {
            w.early += 1;
            continue;
        }
        if held_at(s, until + 0.1) {
            w.extended += 1;
            continue;
        }
        if on_subject(until + TOL, until + LATE).is_some() {
            w.late += 1;
            continue;
        }
        w.freed += 1;
        // Who was ready at their moment?
        let mut ready: Option<(u32, CharacterClass, f32)> = None;
        let mut first_reason: Option<&'static str> = None;
        'units: for &(e, class) in &holders {
            for tool in tools.iter().filter(|t| t.class == class) {
                let tau = until - tool.cast - HANDOFF_LEAD / 2.0;
                let fr = &frames[frame_at(tau)].2;
                let (Some(me), Some(sub)) = (fr.get(&e), fr.get(&s)) else {
                    continue;
                };
                let reason = if !me.alive {
                    Some("dead")
                } else if class.is_healer()
                    && fr
                        .values()
                        .any(|u| u.team == measured && !u.pet && u.alive && u.hp < DYING)
                {
                    Some("healer: teammate dying")
                } else if me.held {
                    Some("held")
                } else if me.silenced {
                    Some("silenced")
                } else if me.mana < tool.mana {
                    Some("mana")
                } else if casts.get(&e).is_some_and(|v| {
                    tool.cooldown > 0.0
                        && v.iter()
                            .any(|(t, a)| *a == tool.ability && *t < tau && tau - t < tool.cooldown)
                }) {
                    Some("cooldown")
                } else if me.pos.distance(sub.pos) > tool.range {
                    Some("range")
                } else if !sees(me.pos, sub.pos) {
                    Some("sight")
                } else if sub.immune {
                    Some("immune")
                } else if tool.dr.is_some_and(|c| dr_immune(subject, c, tau)) {
                    Some("dr")
                } else if tool.spoils
                    && (sub.dot
                        || fr
                            .values()
                            .any(|u| u.team == measured && u.alive && u.target == Some(s)))
                {
                    Some("spoils")
                } else {
                    None
                };
                match reason {
                    None => {
                        ready = Some((e, class, tau));
                        break 'units;
                    }
                    Some(r) => {
                        first_reason.get_or_insert(r);
                    }
                }
            }
        }
        match ready {
            Some((e, class, tau)) => {
                let seen: Vec<&str> = plays
                    .get(&e)
                    .map(|v| {
                        v.iter()
                            .filter(|(t, _)| *t >= tau - GCD && *t <= tau + 0.05)
                            .map(|(_, p)| p.as_str())
                            .collect()
                    })
                    .unwrap_or_default();
                if seen.contains(&"pillar") {
                    w.missed_pillar += 1;
                } else if seen.contains(&"chain") {
                    w.missed_chain += 1;
                } else {
                    w.missed_other += 1;
                }
                *w.missed_by_class
                    .entry(class.name().to_string())
                    .or_default() += 1;
            }
            None => {
                *w.not_ready
                    .entry(first_reason.unwrap_or("no holder").to_string())
                    .or_default() += 1;
            }
        }
    }
    row
}

#[test]
#[ignore = "a measurement a human reads; run with CHC_IN / CHC_OUT / CHC_TMP set"]
fn chain_handoff_census() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("CHC_IN").expect("CHC_IN: the JSONL of match configs");
    let output = std::env::var("CHC_OUT").expect("CHC_OUT: where the rows go");
    let tmp = std::env::var("CHC_TMP").expect("CHC_TMP: a scratch directory");
    let jobs: usize = std::env::var("CHC_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    std::fs::create_dir_all(&tmp).unwrap();

    let configs: Vec<HeadlessMatchConfig> =
        std::io::BufReader::new(std::fs::File::open(&input).expect("open CHC_IN"))
            .lines()
            .map(|l| l.unwrap())
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("a HeadlessMatchConfig"))
            .collect();
    let total = configs.len();
    let configs = Arc::new(configs);
    let tools = Arc::new(tools(&AbilityDefinitions::default()));
    let cursor = Arc::new(AtomicUsize::new(0));
    let rows: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(vec![None; total]));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (configs, tools, cursor, rows, tmp) = (
                Arc::clone(&configs),
                Arc::clone(&tools),
                Arc::clone(&cursor),
                Arc::clone(&rows),
                tmp.clone(),
            );
            scope.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let out = run_one(configs[i].clone(), &tools, &tmp, i);
                rows.lock().unwrap()[i] = Some(serde_json::to_string(&out).unwrap());
                if (i + 1) % 500 == 0 {
                    eprintln!("{} / {}", i + 1, total);
                }
            });
        }
    });

    let mut out = std::io::BufWriter::new(std::fs::File::create(&output).unwrap());
    for row in rows.lock().unwrap().iter() {
        writeln!(out, "{}", row.as_ref().expect("every match ran")).unwrap();
    }
    println!("{total} matches -> {output}");
}
