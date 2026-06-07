//! Command-line interface for ArenaSim
//!
//! Supports both graphical (default) and headless modes.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

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
