//! Command-line interface for ArenaSim
//!
//! Supports both graphical (default) and headless modes.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// AI decision trace output mode.
///
/// `off` — no trace emitted.
/// `on` — minimal trace (actor + target + reason codes).
/// `verbose` — currently behaves identically to `on`. The flag is plumbed
/// end-to-end (TraceWriter.verbose is set, propagated through TraceConfig)
/// but the richer payload — full aura lists on actor + target plus all
/// visible enemies — is deferred to Phase 2 follow-up work. Passing this
/// today is harmless but emits the same JSONL as `on`.
///
/// Default per mode: single-match runs default to `off`; matrix runs default
/// to `on` so every cell's trace is already on disk when you find an anomaly.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
#[clap(rename_all = "kebab-case")]
pub enum TraceMode {
    #[default]
    Off,
    On,
    Verbose,
}

impl TraceMode {
    pub fn is_enabled(self) -> bool {
        matches!(self, TraceMode::On | TraceMode::Verbose)
    }

    pub fn is_verbose(self) -> bool {
        matches!(self, TraceMode::Verbose)
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

    /// Base RNG seed for matrix mode. Each match gets seed = base + run_index,
    /// so the same --seed-base reproduces the same matrix exactly. Default: 0.
    #[arg(long, value_name = "SEED", default_value_t = 0)]
    pub seed_base: u64,

    /// In matrix mode, also write each individual match's `.txt` log file.
    /// Off by default to avoid 49 × N files in match_logs/.
    #[arg(long)]
    pub save_logs: bool,

    /// AI decision trace mode. `off` = no trace; `on` = minimal trace.
    /// `verbose` is accepted but currently emits the same payload as `on` —
    /// richer payload (full aura lists, visible enemy state) is deferred to
    /// Phase 2. Default: `off` for single match, `on` for `--matrix`.
    #[arg(long, value_name = "MODE", value_enum)]
    pub trace_mode: Option<TraceMode>,
}

pub fn parse_args() -> Args {
    Args::parse()
}
