//! Combat system
//!
//! Provides the combat log for tracking combat events during matches.
//! The actual combat logic is implemented in `states/play_match.rs`.

use bevy::prelude::*;

pub mod log;

pub use log::{CombatLog, CombatLogEntry, CombatLogEventType, MatchMetadata, CombatantMetadata, PositionData};

/// Plugin for the combat system.
/// 
/// Currently only initializes the `CombatLog` resource.
/// All combat logic is in `states/play_match.rs`.
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatLog>();
    }
}
