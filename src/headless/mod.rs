//! Headless mode for agentic testing
//!
//! This module provides functionality to run arena matches without any graphical
//! output, suitable for automated testing and AI agent integration.
//!
//! ## Usage
//!
//! ```bash
//! # Run a headless match
//! cargo run --release -- --headless match_config.json
//! ```
//!
//! ## JSON Configuration
//!
//! ```json
//! {
//!   "team1": ["Warrior", "Priest"],
//!   "team2": ["Mage", "Rogue"],
//!   "map": "BasicArena",
//!   "max_duration_secs": 120
//! }
//! ```

pub mod config;
pub mod runner;

pub use config::HeadlessMatchConfig;
pub use runner::run_headless_match;
