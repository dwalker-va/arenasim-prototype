//! Playing a staged match (tier 3, `arenasim::headless::stage`) and reading it
//! back: every frame's observation, the decision trace and the combat log.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{
    run_headless_match_observed, run_headless_match_staged, FrameObservation, HeadlessMatchConfig,
    MatchResult, Stage,
};
use arenasim::states::play_match::decision_trace::DecisionTrace;

/// Where a stage landed, on each of the match's clocks.
#[derive(Clone, Copy, Debug)]
pub struct Landed {
    /// The app's clock, countdown included: a frame's `sim_time` and the
    /// combat log's stamps.
    pub app: f32,
    /// The combat clock: the trace's `sim_time` (and the AI's `now`).
    pub combat: f32,
    /// The trace's `frame` counter on the frame the stage landed on.
    pub trace_frame: u64,
}

impl Landed {
    /// Read off the world as a stage lands.
    pub fn now(world: &World) -> Self {
        let trace = world.resource::<DecisionTrace>();
        Landed {
            app: world.resource::<Time>().elapsed_secs(),
            combat: trace.current_sim_time,
            trace_frame: trace.current_frame,
        }
    }
}

/// One played match, everything a probe reads from it.
pub struct Played {
    /// Every frame's observation, the countdown's included.
    pub frames: Vec<FrameObservation>,
    /// The decision trace, one parsed event per line.
    pub trace: Vec<serde_json::Value>,
    /// The decision trace as written.
    pub trace_raw: String,
    /// The combat log as saved (`.txt` report).
    pub log: String,
    pub result: MatchResult,
    /// Where the stage landed (`None` for a seeded match).
    pub landed: Option<Landed>,
}

impl Played {
    /// Where the stage landed. Panics for a seeded match, or a stage that
    /// never came due.
    pub fn landed(&self) -> Landed {
        self.landed.expect("the stage landed")
    }

    /// The trace events from the frame the stage landed on.
    pub fn staged_trace(&self) -> impl Iterator<Item = &serde_json::Value> + '_ {
        let from = self.landed().trace_frame;
        self.trace
            .iter()
            .filter(move |e| e["frame"].as_u64().is_some_and(|f| f >= from))
    }

    /// The frames from the one the stage landed on.
    pub fn staged_frames(&self) -> impl Iterator<Item = &FrameObservation> + '_ {
        let from = self.landed().app;
        self.frames.iter().filter(move |f| f.sim_time >= from)
    }
}

/// Play `config` with `stage` landed on it.
pub fn play(config: HeadlessMatchConfig, stage: Stage) -> Played {
    play_with(config, Some(stage))
}

/// Play `config` as its seed plays it, through the plain observed runner (no
/// stage, no staging system).
pub fn play_seeded(config: HeadlessMatchConfig) -> Played {
    play_with(config, None)
}

fn play_with(config: HeadlessMatchConfig, stage: Option<Stage>) -> Played {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let trace_path = dir.path().join("trace.jsonl");
    let log_path = dir.path().join("match.txt");
    let config = HeadlessMatchConfig {
        output_path: Some(log_path.to_string_lossy().into_owned()),
        ..config
    };
    let landed = Arc::new(Mutex::new(None));
    let mut frames = Vec::new();
    let trace = Some(TraceConfig {
        output_path: trace_path.clone(),
    });
    let observe = |frame: &FrameObservation| frames.push(frame.clone());
    let result = match stage {
        Some(stage) => {
            let mark = landed.clone();
            let stage = stage.world(move |w| *mark.lock().unwrap() = Some(Landed::now(w)));
            run_headless_match_staged(config, false, trace, stage, observe)
        }
        None => run_headless_match_observed(config, false, trace, observe),
    }
    .expect("the match plays");
    let trace_raw = std::fs::read_to_string(&trace_path).expect("the trace");
    let landed = *landed.lock().unwrap();
    Played {
        frames,
        trace: trace_raw
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect(),
        trace_raw,
        log: std::fs::read_to_string(&log_path).expect("the combat log"),
        result,
        landed,
    }
}

/// The decision events of `class` (a pet's events name its owner's class).
pub fn of_class<'a>(
    events: impl Iterator<Item = &'a serde_json::Value> + 'a,
    class: &'a str,
) -> impl Iterator<Item = &'a serde_json::Value> + 'a {
    events.filter(move |e| e["actor"]["class"] == class)
}

/// The sim time of a trace event (the combat clock).
pub fn sim_time(e: &serde_json::Value) -> f32 {
    e["sim_time"].as_f64().expect("sim_time") as f32
}

/// The entity id the trace gives the combatant in `slot` of `team`.
pub fn entity_id(played: &Played, team: u8, slot: u8) -> u64 {
    played.frames[0]
        .combatants
        .iter()
        .find(|(_, c)| c.team == team && c.slot == slot && !c.is_pet)
        .map(|(e, _)| e.index() as u64)
        .unwrap_or_else(|| panic!("no combatant in slot {slot} of team {team}"))
}
