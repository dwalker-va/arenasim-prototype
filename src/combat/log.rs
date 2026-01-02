//! Combat logging
//!
//! Records all combat events for display and post-match analysis.

use bevy::prelude::*;

/// A single entry in the combat log
#[derive(Debug, Clone)]
pub struct CombatLogEntry {
    /// Timestamp in match time (seconds since match start)
    pub timestamp: f32,
    /// The type of event
    pub event_type: CombatLogEventType,
    /// Human-readable description of the event
    pub message: String,
}

/// Types of combat log events for filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatLogEventType {
    /// Damage dealt
    Damage,
    /// Healing done
    Healing,
    /// Ability used
    AbilityUsed,
    /// Buff/debuff applied
    AuraApplied,
    /// Buff/debuff removed
    AuraRemoved,
    /// Crowd control applied
    CrowdControl,
    /// Combatant died
    Death,
    /// Match event (start, end, etc.)
    MatchEvent,
}

/// The combat log resource storing all events
#[derive(Resource, Default)]
pub struct CombatLog {
    /// All log entries in chronological order
    pub entries: Vec<CombatLogEntry>,
    /// Current match time
    pub match_time: f32,
}

impl CombatLog {
    /// Clear the log for a new match
    pub fn clear(&mut self) {
        self.entries.clear();
        self.match_time = 0.0;
    }

    /// Add a new entry to the log
    pub fn log(&mut self, event_type: CombatLogEventType, message: String) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type,
            message,
        });
    }

    /// Get entries filtered by event type
    pub fn filter_by_type(&self, event_type: CombatLogEventType) -> Vec<&CombatLogEntry> {
        self.entries
            .iter()
            .filter(|e| e.event_type == event_type)
            .collect()
    }

    /// Get only HP-changing events (damage and healing)
    pub fn hp_changes_only(&self) -> Vec<&CombatLogEntry> {
        self.entries
            .iter()
            .filter(|e| {
                matches!(
                    e.event_type,
                    CombatLogEventType::Damage | CombatLogEventType::Healing
                )
            })
            .collect()
    }

    /// Get the last N entries
    pub fn recent(&self, count: usize) -> Vec<&CombatLogEntry> {
        self.entries.iter().rev().take(count).rev().collect()
    }
}

