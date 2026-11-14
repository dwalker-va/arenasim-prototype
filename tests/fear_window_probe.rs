//! The Warlock's own-Fear window, measured (AS-495): what team 1's Warlock
//! does while its own Fear holds its target. A measurement, not an assertion:
//! it plays a bank of headless configs and writes one CSV row per match.
//!
//! For every run of frames in which the Warlock's own Fear holds the enemy
//! that was its target when the Fear landed (a WINDOW, chained Fears
//! included), it counts:
//!
//! - **window seconds**, and **idle seconds** in them: the Warlock's
//!   `no_action` decision frames (off its global cooldown, not casting, and
//!   choosing nothing — a held global cooldown included), and of those the
//!   frames held for the window's end (`hold` seconds);
//! - **Fears landing on its own target by diminishing returns** (100 / 50 /
//!   25 / immune);
//! - **damage landing in the window's last second**: the Warlock's direct
//!   hits on the window's enemy landing within the last second before the
//!   window ends (or on the frame that ends it), and the hits landing in the
//!   window before that last second;
//! - the **gap** from the window's end to the Warlock's first direct hit on
//!   that enemy (capped at 15s, a window followed by no hit counts 15s).
//!
//! ```text
//! FEAR_PROBE_BANK=bank.jsonl FEAR_PROBE_OUT=out.csv \
//!   cargo test --release --test fear_window_probe -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, FrameObservation, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::components::AuraType;
use bevy::prelude::Entity;

const WORKERS: usize = 4;
const FRAME: f32 = 1.0 / 60.0;
const GAP_CAP: f32 = 15.0;

/// One frame, as much of it as the probe reads.
struct Slim {
    t: f32,
    gates_open: bool,
    /// The Warlock's target.
    target: Option<Entity>,
    /// Enemies (non-pet) carrying a Fear aura this frame.
    feared: Vec<Entity>,
}

#[derive(Default, Debug)]
struct Row {
    windows: u32,
    window_secs: f32,
    idle_secs: f32,
    hold_secs: f32,
    land_100: u32,
    land_50: u32,
    land_25: u32,
    land_immune: u32,
    last_sec_hits: u32,
    last_sec_dmg: f32,
    windows_hit_last_sec: u32,
    early_hits: u32,
    gap_sum: f32,
}

fn log_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line[open..].find("s]")? + open;
    line[open + 1..close].trim().parse().ok()
}

fn name_of(team: u8, class: CharacterClass, slot: u8) -> String {
    format!("Team {team} {} #{}", class.name(), slot + 1)
}

fn measure(config: HeadlessMatchConfig) -> (Option<u8>, Row) {
    let dir = tempfile::tempdir().expect("scratch");
    let trace_path = dir.path().join("trace.jsonl");
    let log_path = dir.path().join("match.txt");
    let config = HeadlessMatchConfig {
        output_path: Some(log_path.to_string_lossy().into_owned()),
        ..config
    };
    let mut frames: Vec<Slim> = Vec::new();
    let mut roster: BTreeMap<Entity, (u8, CharacterClass, u8, bool)> = BTreeMap::new();
    let mut warlock: Option<Entity> = None;
    let result = run_headless_match_observed(
        config,
        false,
        Some(TraceConfig {
            output_path: trace_path.clone(),
        }),
        |f: &FrameObservation| {
            if roster.is_empty() {
                for (e, c) in &f.combatants {
                    roster.insert(*e, (c.team, c.class, c.slot, c.is_pet));
                }
                warlock = f
                    .combatants
                    .iter()
                    .find(|(_, c)| c.team == 1 && !c.is_pet && c.class == CharacterClass::Warlock)
                    .map(|(e, _)| *e);
            }
            let me = warlock.and_then(|w| f.combatants.get(&w));
            frames.push(Slim {
                t: f.sim_time,
                gates_open: f.gates_open,
                target: me.filter(|c| c.alive).and_then(|c| c.target),
                feared: f
                    .combatants
                    .iter()
                    .filter(|(_, c)| {
                        c.team == 2
                            && !c.is_pet
                            && c.alive
                            && c.aura_types.contains(&AuraType::Fear)
                    })
                    .map(|(e, _)| *e)
                    .collect(),
            });
        },
    )
    .expect("the match plays");
    let mut row = Row::default();
    let Some(w) = warlock else {
        return (result.winner, row);
    };
    let (_, _, w_slot, _) = roster[&w];
    let w_name = name_of(1, CharacterClass::Warlock, w_slot);
    let log = std::fs::read_to_string(&log_path).expect("log");
    let trace = std::fs::read_to_string(&trace_path).expect("trace");

    // Trace clock -> app clock: the trace counts from the gates.
    let gates_at = frames
        .iter()
        .find(|f| f.gates_open)
        .map(|f| f.t)
        .unwrap_or(10.0);

    // The Warlock's idle frames (app clock), and those held for the window.
    let w_id = w.index() as u64;
    let mut idle: Vec<(f32, bool)> = Vec::new();
    for line in trace.lines() {
        if !line.contains("\"kind\":\"ability_decision\"") || !line.contains("no_action") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["actor"]["entity_id"].as_u64() != Some(w_id) {
            continue;
        }
        let t = v["sim_time"].as_f64().unwrap_or(0.0) as f32 + gates_at;
        idle.push((t, line.contains("Fear window: holding")));
    }

    // Own Fear landings: (app time, victim name, DR percent or None = immune).
    let lands_prefix = format!("{w_name}'s Fear lands on ");
    let mut landings: Vec<(f32, String)> = Vec::new();
    let mut hits: Vec<(f32, String, f32)> = Vec::new();
    let hit_prefix = format!("{w_name}'s ");
    for line in log.lines() {
        let Some(t) = log_time(line) else { continue };
        if let Some(rest) = line.split(&lands_prefix).nth(1) {
            let victim = rest.split(" (").next().unwrap_or("").to_string();
            landings.push((t, victim));
            continue;
        }
        if line.contains("Fear IMMUNE on ") {
            // Attributed to the Warlock when it was its cast: a Fear begun
            // by it on that enemy within the last 2s.
            let victim = line
                .split("IMMUNE on ")
                .nth(1)
                .and_then(|r| r.split(" (").next())
                .unwrap_or("")
                .to_string();
            if log.lines().any(|l| {
                l.contains(&format!("{w_name} begins casting Fear on {victim}"))
                    && log_time(l).is_some_and(|b| t - b <= 2.0 && t >= b)
            }) {
                row.land_immune += 1;
            }
            continue;
        }
        if let Some(rest) = line.split(&hit_prefix).nth(1) {
            if let Some((_, after)) = rest.split_once(" hits ") {
                if let Some((victim, dmg)) = after.split_once(" for ") {
                    let dmg: f32 = dmg
                        .split_whitespace()
                        .next()
                        .and_then(|d| d.parse().ok())
                        .unwrap_or(0.0);
                    hits.push((t, victim.to_string(), dmg));
                }
            }
        }
    }
    // DR level of each own landing: the "[CC] Fear on X (Ns, DR: P%)" line
    // that follows it.
    let lines: Vec<&str> = log.lines().collect();
    for (t, victim) in &landings {
        let tag = format!("[CC] Fear on {victim} (");
        let dr = lines.iter().find_map(|l| {
            let lt = log_time(l)?;
            if lt < *t - 0.001 || lt > *t + 0.1 || !l.contains(&tag) {
                return None;
            }
            l.split("DR: ")
                .nth(1)?
                .split('%')
                .next()?
                .parse::<u32>()
                .ok()
        });
        match dr {
            Some(100) => row.land_100 += 1,
            Some(50) => row.land_50 += 1,
            Some(25) => row.land_25 += 1,
            _ => {}
        }
    }

    // Windows: runs of frames in which an enemy is feared, begun by an own
    // landing on the enemy that was the Warlock's target then.
    let names: BTreeMap<Entity, String> = roster
        .iter()
        .map(|(e, (team, class, slot, _))| (*e, name_of(*team, *class, *slot)))
        .collect();
    let enemies: Vec<Entity> = roster
        .iter()
        .filter(|(_, (team, _, _, pet))| *team == 2 && !pet)
        .map(|(e, _)| *e)
        .collect();
    for enemy in enemies {
        let name = &names[&enemy];
        let mut i = 0;
        while i < frames.len() {
            if !frames[i].feared.contains(&enemy) {
                i += 1;
                continue;
            }
            let start = i;
            while i < frames.len() && frames[i].feared.contains(&enemy) {
                i += 1;
            }
            let (t0, t1) = (frames[start].t, frames[i.min(frames.len() - 1)].t);
            let own = landings
                .iter()
                .any(|(lt, v)| v == name && (lt - t0).abs() <= 0.1);
            let was_target = frames[start.saturating_sub(1)].target == Some(enemy)
                || frames[start].target == Some(enemy);
            if !own || !was_target {
                continue;
            }
            row.windows += 1;
            row.window_secs += t1 - t0;
            for (t, held) in &idle {
                if *t >= t0 && *t < t1 {
                    row.idle_secs += FRAME;
                    if *held {
                        row.hold_secs += FRAME;
                    }
                }
            }
            let mut hit_last = false;
            for (t, victim, dmg) in &hits {
                if victim != name {
                    continue;
                }
                if *t >= t1 - 1.0 && *t <= t1 + FRAME {
                    row.last_sec_hits += 1;
                    row.last_sec_dmg += dmg;
                    hit_last = true;
                } else if *t >= t0 && *t < t1 - 1.0 {
                    row.early_hits += 1;
                }
            }
            if hit_last {
                row.windows_hit_last_sec += 1;
            }
            let gap = hits
                .iter()
                .filter(|(t, v, _)| v == name && *t >= t1 - 1.0)
                .map(|(t, _, _)| (t - t1).max(0.0))
                .fold(GAP_CAP, f32::min);
            row.gap_sum += gap;
        }
    }
    (result.winner, row)
}

#[test]
#[ignore = "a measurement: plays a whole bank"]
fn fear_window_probe() {
    let bank = std::env::var("FEAR_PROBE_BANK").expect("FEAR_PROBE_BANK");
    let out = std::env::var("FEAR_PROBE_OUT").expect("FEAR_PROBE_OUT");
    let jobs: Vec<(String, HeadlessMatchConfig)> = std::fs::read_to_string(&bank)
        .expect("bank")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).expect("json");
            let label = v["label"].as_str().unwrap_or("").to_string();
            v.as_object_mut().unwrap().remove("label");
            let seed = v["random_seed"].as_u64().unwrap_or(0);
            (
                format!("{label},{seed}"),
                serde_json::from_value(v).expect("config"),
            )
        })
        .collect();
    let queue = Arc::new(Mutex::new(jobs.into_iter().enumerate().collect::<Vec<_>>()));
    let rows = Arc::new(Mutex::new(Vec::new()));
    let handles: Vec<_> = (0..WORKERS)
        .map(|_| {
            let queue = queue.clone();
            let rows = rows.clone();
            std::thread::spawn(move || loop {
                let Some((i, (key, config))) = queue.lock().unwrap().pop() else {
                    break;
                };
                let (winner, r) = measure(config);
                let line = format!(
                    "{key},{},{},{:.3},{:.3},{:.3},{},{},{},{},{},{:.1},{},{},{:.3}",
                    winner.map_or("draw".to_string(), |w| format!("team{w}")),
                    r.windows,
                    r.window_secs,
                    r.idle_secs,
                    r.hold_secs,
                    r.land_100,
                    r.land_50,
                    r.land_25,
                    r.land_immune,
                    r.last_sec_hits,
                    r.last_sec_dmg,
                    r.windows_hit_last_sec,
                    r.early_hits,
                    r.gap_sum,
                );
                rows.lock().unwrap().push((i, line));
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker");
    }
    let mut rows = Arc::try_unwrap(rows).unwrap().into_inner().unwrap();
    rows.sort();
    let mut text = String::from(
        "label,seed,winner,windows,window_secs,idle_secs,hold_secs,land_100,land_50,land_25,\
         land_immune,last_sec_hits,last_sec_dmg,windows_hit_last_sec,early_hits,gap_sum\n",
    );
    for (_, line) in rows {
        text.push_str(&line);
        text.push('\n');
    }
    std::fs::write(&out, text).expect("write");
}
