//! ArenaSim - Arena Combat Autobattler Prototype
//!
//! A prototype implementation of an autobattler where players configure teams
//! of combatants and watch them battle CPU vs CPU.
//!
//! This library exposes the core game modules for testing and reuse.

pub mod camera;
pub mod cli;
pub mod combat;
pub mod headless;
pub mod keybindings;
pub mod settings;
pub mod states;
pub mod ui;

// Re-export commonly used types
pub use combat::log::{CombatLog, CombatLogEventType};
pub use headless::HeadlessMatchConfig;
pub use states::match_config::{ArenaMap, CharacterClass, MatchConfig};
