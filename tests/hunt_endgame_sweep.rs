//! The lone-healer endgame — a MEASUREMENT a human reads, not an assertion
//! (AS-394).
//!
//! Runs a JSONL of ordinary `HeadlessMatchConfig`s (the `--batch` input
//! format) through the observed runner, trace off, and writes one JSON row
//! per match: its outcome, and its lone-healer endgame read off every frame —
//! one team reduced to a lone healer while the other still fields two or more
//! non-pet units. Per endgame:
//!
//! - `onset` and `secs`: when it began (seconds after the gates) and how long
//!   it ran, to the match's end;
//! - `hider_team`, `hider_class`;
//! - `seen`: the share of its frames in which a living non-pet unit of the
//!   outnumbering side had the healer in sight (the cast's own test, eye to
//!   eye), and `split`: the share in which two of them did from bearings at
//!   least 120° apart round the healer's nearest pillar — the pincer the Hunt
//!   play builds.
//!
//! The observer is read-only (`observed_run_does_not_perturb_outcomes`), so
//! a row's outcome is the `--batch` CSV's for the same config. Aggregation:
//! `docs/design/balance/sweeps/2026-10-25-as394-score.py`.
//!
//! ```bash
//! HUNT_IN=bank.jsonl HUNT_OUT=rows.jsonl HUNT_JOBS=8 \
//!   cargo test --release --test hunt_endgame_sweep -- --ignored --nocapture
//! ```

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arenasim::headless::{run_headless_match_observed, FrameObservation, HeadlessMatchConfig};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume};
use arenasim::ArenaMap;
use bevy::prelude::{Vec2, Vec3};

fn map_of(name: &str) -> ArenaMap {
    match name {
        "PillaredArena" => ArenaMap::PillaredArena,
        "TwinPillars" => ArenaMap::TwinPillars,
        "BasicArena" => ArenaMap::BasicArena,
        other => panic!("unmeasured map {other}"),
    }
}

#[derive(Default)]
struct Endgame {
    onset: Option<f32>,
    hider_team: u8,
    hider_class: String,
    frames: u32,
    seen: u32,
    split: u32,
}

fn bearing_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}

fn observe(eg: &mut Endgame, f: &FrameObservation, gate: f32, obstacles: &[ObstacleVolume]) {
    let living = |team: u8| {
        f.combatants
            .values()
            .filter(move |c| c.team == team && c.alive && !c.is_pet)
    };
    if eg.onset.is_none() {
        for team in [1u8, 2] {
            let mut alone = living(team);
            let (Some(last), None) = (alone.next(), alone.next()) else {
                continue;
            };
            if last.class.is_healer() && living(3 - team).count() >= 2 {
                eg.onset = Some(f.sim_time - gate);
                eg.hider_team = team;
                eg.hider_class = format!("{:?}", last.class);
            }
        }
    }
    if eg.onset.is_none() {
        return;
    }
    let Some(hider) = living(eg.hider_team).next() else {
        return;
    };
    eg.frames += 1;
    let pillar = obstacles.iter().map(|v| v.footprint_disc()).min_by(|a, b| {
        let h = Vec2::new(hider.position.x, hider.position.z);
        (a.0.distance(h) - a.1).total_cmp(&(b.0.distance(h) - b.1))
    });
    let seers: Vec<Vec3> = living(3 - eg.hider_team)
        .filter(|c| has_line_of_sight(obstacles, c.position, hider.position))
        .map(|c| c.position)
        .collect();
    if !seers.is_empty() {
        eg.seen += 1;
    }
    if let Some((centre, _)) = pillar {
        let bearings: Vec<f32> = living(3 - eg.hider_team)
            .map(|c| (Vec2::new(c.position.x, c.position.z) - centre).to_angle())
            .collect();
        let split = !seers.is_empty()
            && bearings.iter().enumerate().any(|(i, a)| {
                bearings[i + 1..]
                    .iter()
                    .any(|b| bearing_gap(*a, *b) >= 120f32.to_radians())
            });
        if split {
            eg.split += 1;
        }
    }
}

fn run_one(config: HeadlessMatchConfig) -> serde_json::Value {
    let geometry = load_map_geometry_config()
        .expect("maps.ron")
        .active_for(map_of(&config.map));
    let obstacles = geometry.volumes.clone();
    let mut gate: Option<f32> = None;
    let mut eg = Endgame::default();
    let label = config.label.clone().unwrap_or_default();
    let (team1, team2, seed, map) = (
        config.team1.join("+"),
        config.team2.join("+"),
        config.random_seed,
        config.map.clone(),
    );
    let result = run_headless_match_observed(config, true, None, |f: &FrameObservation| {
        if !f.gates_open {
            return;
        }
        let g = *gate.get_or_insert(f.sim_time);
        observe(&mut eg, f, g, &obstacles);
    })
    .expect("the match plays");
    let winner = match result.winner {
        Some(1) => "team1",
        Some(2) => "team2",
        _ => "draw",
    };
    let endgame = eg.onset.map(|onset| {
        let n = eg.frames.max(1) as f32;
        serde_json::json!({
            "onset": onset,
            "secs": (result.match_time - onset).max(0.0),
            "hider_team": eg.hider_team,
            "hider_class": eg.hider_class,
            "seen": eg.seen as f32 / n,
            "split": eg.split as f32 / n,
        })
    });
    serde_json::json!({
        "label": label,
        "map": map,
        "team1": team1,
        "team2": team2,
        "seed": seed,
        "winner": winner,
        "duration": result.match_time,
        "endgame": endgame,
    })
}

#[test]
#[ignore = "a measurement a human reads; run with HUNT_IN / HUNT_OUT set"]
fn hunt_endgame_sweep() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("HUNT_IN").expect("HUNT_IN: the JSONL of match configs");
    let output = std::env::var("HUNT_OUT").expect("HUNT_OUT: where the rows go");
    let jobs: usize = std::env::var("HUNT_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let configs: Vec<HeadlessMatchConfig> =
        std::io::BufReader::new(std::fs::File::open(&input).expect("open HUNT_IN"))
            .lines()
            .map(|l| l.unwrap())
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("a HeadlessMatchConfig"))
            .collect();
    let total = configs.len();
    let configs = Arc::new(configs);
    let cursor = Arc::new(AtomicUsize::new(0));
    let rows: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(vec![None; total]));
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (configs, cursor, rows) =
                (Arc::clone(&configs), Arc::clone(&cursor), Arc::clone(&rows));
            scope.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let row = run_one(configs[i].clone());
                rows.lock().unwrap()[i] = Some(row.to_string());
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
