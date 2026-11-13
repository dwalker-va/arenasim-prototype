//! Per-frame tracks of one match, for reading the Hunt play's endgame by eye
//! (AS-404) — a MEASUREMENT a human reads, not an assertion.
//!
//! Plays the match described at `TRACK_IN` through the observed runner and
//! writes one CSV row per living non-pet combatant per frame after the gates
//! to `TRACK_OUT`: the app clock (the combat log's), the unit, its position,
//! health and mana, its live `Point` goal, whether it sees each living enemy
//! (the cast's own sight test), the cast or channel in progress and its
//! auras. `TRACE_OUT`, when set, gets the
//! decision trace.
//!
//! `TRACK_IN` is either a `--headless` config (played as its seed plays it),
//! or `{"config": <config>, "units": [...]}`: a staged match, each unit
//! `{"team", "slot", "x", "z", "hp", "mana"}` placed on the frame the gates
//! open — an endgame's onset reconstructed as a 2v1.
//!
//! ```bash
//! TRACK_IN=cfg.json TRACK_OUT=tracks.csv \
//!   cargo test --release --test hunt_tracks -- --ignored --nocapture
//! ```

use std::io::Write;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{
    run_headless_match_observed, run_headless_match_staged, FrameObservation, HeadlessMatchConfig,
    Stage, Unit,
};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::has_line_of_sight;
use arenasim::ArenaMap;

fn map_of(name: &str) -> ArenaMap {
    match name {
        "PillaredArena" => ArenaMap::PillaredArena,
        "TwinPillars" => ArenaMap::TwinPillars,
        "BasicArena" => ArenaMap::BasicArena,
        other => panic!("unmeasured map {other}"),
    }
}

#[derive(serde::Deserialize)]
struct Placed {
    team: u8,
    slot: u8,
    x: f32,
    z: f32,
    hp: f32,
    mana: f32,
}

#[derive(serde::Deserialize)]
struct Staged {
    config: HeadlessMatchConfig,
    units: Vec<Placed>,
}

#[test]
#[ignore = "a measurement a human reads; run with TRACK_IN / TRACK_OUT set"]
fn hunt_tracks() {
    let input = std::env::var("TRACK_IN").expect("TRACK_IN: a headless config");
    let output = std::env::var("TRACK_OUT").expect("TRACK_OUT: where the CSV goes");
    let trace = std::env::var("TRACE_OUT").ok().map(|p| TraceConfig {
        output_path: p.into(),
    });
    let text = std::fs::read_to_string(&input).expect("read TRACK_IN");
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let (config, stage) = if value.get("config").is_some() {
        let staged: Staged = serde_json::from_value(value).expect("a staged spec");
        let stage = staged.units.iter().fold(Stage::new(), |s, u| {
            let who = Unit::new(u.team, u.slot);
            s.place(who, u.x, u.z).health(who, u.hp).mana(who, u.mana)
        });
        (staged.config, Some(stage))
    } else {
        (
            serde_json::from_value::<HeadlessMatchConfig>(value).expect("a HeadlessMatchConfig"),
            None,
        )
    };
    let obstacles = load_map_geometry_config()
        .expect("maps.ron")
        .active_for(map_of(&config.map))
        .volumes
        .clone();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&output).unwrap());
    writeln!(
        out,
        "t,team,slot,class,x,z,hp,mana,goal_x,goal_z,sees,casting,auras"
    )
    .unwrap();
    let observe = |f: &FrameObservation| {
        if !f.gates_open {
            return;
        }
        let living: Vec<_> = f
            .combatants
            .values()
            .filter(|c| c.alive && !c.is_pet)
            .collect();
        for c in &living {
            let sees: Vec<String> = living
                .iter()
                .filter(|e| e.team != c.team)
                .filter(|e| has_line_of_sight(&obstacles, c.position, e.position))
                .map(|e| format!("{}{}", e.team, e.slot))
                .collect();
            let (gx, gz) = c.point_goal.map_or((String::new(), String::new()), |g| {
                (format!("{:.2}", g.x), format!("{:.2}", g.z))
            });
            writeln!(
                out,
                "{:.3},{},{},{:?},{:.2},{:.2},{:.3},{:.3},{},{},{},{},{}",
                f.sim_time,
                c.team,
                c.slot,
                c.class,
                c.position.x,
                c.position.z,
                c.current_health / c.max_health.max(1.0),
                c.current_mana / c.max_mana.max(1.0),
                gx,
                gz,
                sees.join(" "),
                c.casting
                    .or(c.channeling)
                    .map_or(String::new(), |a| format!("{a:?}")),
                c.aura_types
                    .iter()
                    .map(|a| format!("{a:?}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .unwrap();
        }
    };
    let result = match stage {
        Some(stage) => run_headless_match_staged(config, true, trace, stage, observe),
        None => run_headless_match_observed(config, true, trace, observe),
    }
    .expect("the match plays");
    println!(
        "winner {:?} at {:.2}s -> {output}",
        result.winner, result.match_time
    );
}
