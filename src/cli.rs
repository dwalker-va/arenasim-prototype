//! Command-line interface for ArenaSim
//!
//! Supports both graphical (default) and headless modes.

use clap::Parser;
use std::path::PathBuf;

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

    /// Maximum match duration in seconds (headless mode only)
    #[arg(long, default_value = "300")]
    pub max_duration: f32,
}

pub fn parse_args() -> Args {
    Args::parse()
}
