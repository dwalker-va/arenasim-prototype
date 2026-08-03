//! Command-line interface for ArenaSim
//!
//! Supports both graphical (default) and headless modes.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Default `--ai-profile`. Shared with `main` so the "flag ignored outside
/// --matrix" warning cannot drift from the clap default.
pub const DEFAULT_AI_PROFILE: &str = "Legacy";

/// AI decision trace output mode.
///
/// `off` — no trace emitted.
/// `on` — minimal trace (actor + target + reason codes).
///
/// A richer verbose mode (full aura lists, visible enemy state) is a future
/// addition; when it lands it will be a new variant — `verbose` is NOT
/// accepted today so scripts depending on it fail loudly at clap parse time
/// rather than silently receiving the minimal payload.
///
/// Default per mode: single-match runs default to `off`; matrix runs default
/// to `on` so every cell's trace is already on disk when you find an anomaly.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
#[clap(rename_all = "kebab-case")]
pub enum TraceMode {
    #[default]
    Off,
    On,
}

impl TraceMode {
    pub fn is_enabled(self) -> bool {
        matches!(self, TraceMode::On)
    }
}

/// Arena combat autobattler simulator
#[derive(Parser, Debug)]
#[command(name = "arenasim")]
#[command(about = "Arena combat autobattler simulator")]
#[command(version)]
pub struct Args {
    /// Run in headless mode with the specified JSON config file
    #[arg(long, value_name = "CONFIG_FILE")]
    pub headless: Option<PathBuf>,

    /// Output path for match log (headless mode only)
    #[arg(long, value_name = "OUTPUT_PATH")]
    pub output: Option<PathBuf>,

    /// Maximum match duration in seconds (headless mode only, overrides config file)
    #[arg(long)]
    pub max_duration: Option<f32>,

    /// Run all 7×7 class matchups N times each, emit a winrate heatmap
    /// (CSV + Markdown) to match_logs/matrix_<timestamp>.{csv,md}.
    /// Per-match `.txt` logs are suppressed unless --save-logs is also passed.
    #[arg(long, value_name = "N")]
    pub matrix: Option<u32>,

    /// Run a batch of matches from a JSONL file (one HeadlessMatchConfig per
    /// line) in parallel across all cores, writing one CSV row per match to
    /// --out. The fast path for 2v2/3v3/strategy-var sweeps.
    #[arg(long, value_name = "JSONL_FILE")]
    pub batch: Option<PathBuf>,

    /// Output CSV path for --batch mode (default: match_logs/batch_<timestamp>.csv).
    #[arg(long, value_name = "CSV_FILE")]
    pub out: Option<PathBuf>,

    /// Worker thread count for --batch mode (default: cores - 2).
    #[arg(long, value_name = "N")]
    pub jobs: Option<usize>,

    /// Base RNG seed for matrix mode. Each match gets seed = base + run_index,
    /// so the same --seed-base reproduces the same matrix exactly. Default: 0.
    #[arg(long, value_name = "SEED", default_value_t = 0)]
    pub seed_base: u64,

    /// Arena map for matrix mode. Defaults to "BasicArena" (the historical
    /// matrix behavior). "PillaredArena" runs every cell with line-of-sight
    /// obstacles active. "TestVerticality" is a test-only asset and is
    /// rejected here. CSV columns are unchanged regardless of map.
    #[arg(long, value_name = "MAP", default_value = "BasicArena")]
    pub matrix_map: String,

    /// AI implementation for matrix mode ONLY — `--headless` / `--batch` read
    /// `ai_profile` from their JSON config instead, and `main` warns if this flag
    /// is set without `--matrix`. "Legacy" (default — what every recorded baseline
    /// is calibrated against) or "TeamPlan". Because matches are deterministic,
    /// running the same `--seed-base` under both profiles gives a PAIRED comparison
    /// in which the AI is the only variable — far more sensitive than comparing two
    /// independent sweeps. The profile is recorded in the CSV header and in the
    /// output filename so runs cannot be confused.
    #[arg(long, value_name = "PROFILE", default_value = DEFAULT_AI_PROFILE)]
    pub ai_profile: String,

    /// Watch a headless config play out in the graphical client, seed and all.
    ///
    /// Takes the same JSON `--headless` accepts. Comps, map, AI profile and
    /// `random_seed` are all honoured, and the client boots straight into the
    /// match rather than the menu — so a bug found in a sweep can be WATCHED
    /// rather than inferred from a log. Without a `random_seed` in the file the
    /// match is fresh, and its chosen seed is still recorded in the report.
    #[arg(long, value_name = "CONFIG")]
    pub replay: Option<std::path::PathBuf>,

    /// In matrix mode, also write each individual match's `.txt` log file.
    /// Off by default to avoid 49 × N files in match_logs/.
    #[arg(long)]
    pub save_logs: bool,

    /// AI decision trace mode. `off` = no trace; `on` = minimal trace
    /// (actor + target + reason codes). Default: `off` for single match,
    /// `on` for `--matrix`.
    #[arg(long, value_name = "MODE", value_enum)]
    pub trace_mode: Option<TraceMode>,
}

pub fn parse_args() -> Args {
    Args::parse()
}
