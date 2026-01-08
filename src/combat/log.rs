//! Combat logging
//!
//! Records all combat events for display and post-match analysis.
//!
//! The CombatLog is the **definitive source of truth** for match statistics.
//! The Results scene uses this data to build WoW Details-style breakdowns.
//!
//! ## Structured Data
//! Each log entry contains optional structured data for machine-readable queries:
//! - `DamageEvent`: source, target, ability, amount, was_killing_blow
//! - `HealingEvent`: source, target, ability, amount
//! - `CrowdControlEvent`: source, target, cc_type, duration
//! - `DeathEvent`: victim, killer (optional)

use bevy::prelude::*;
use std::collections::HashMap;

/// Unique identifier for a combatant in the combat log
/// Format: "Team {team} {class}" e.g. "Team 1 Warrior"
pub type CombatantId = String;

/// A single entry in the combat log
#[derive(Debug, Clone)]
pub struct CombatLogEntry {
    /// Timestamp in match time (seconds since match start)
    pub timestamp: f32,
    /// The type of event
    pub event_type: CombatLogEventType,
    /// Human-readable description of the event
    pub message: String,
    /// Optional position data for debugging (where the event occurred)
    pub position_data: Option<PositionData>,
    /// Optional structured data for machine-readable queries
    pub structured_data: Option<StructuredEventData>,
}

/// Structured event data for machine-readable queries and aggregation
#[derive(Debug, Clone)]
pub enum StructuredEventData {
    /// Damage dealt from one combatant to another
    Damage {
        source: CombatantId,
        target: CombatantId,
        ability: String,
        amount: f32,
        is_killing_blow: bool,
    },
    /// Healing done from one combatant to another (or self)
    Healing {
        source: CombatantId,
        target: CombatantId,
        ability: String,
        amount: f32,
    },
    /// Crowd control applied
    CrowdControl {
        source: CombatantId,
        target: CombatantId,
        cc_type: String,
        duration_secs: f32,
    },
    /// Combatant death
    Death {
        victim: CombatantId,
        killer: Option<CombatantId>,
    },
}

/// Position data for debugging combat events
#[derive(Debug, Clone)]
pub struct PositionData {
    /// Entity IDs involved in the event (source, target)
    pub entities: Vec<String>,
    /// Positions of entities at the time of the event
    pub positions: Vec<(f32, f32, f32)>, // (x, y, z)
    /// Distance between entities (if applicable)
    pub distance: Option<f32>,
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
    /// Friendly buff applied (like Power Word: Fortitude)
    Buff,
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

    /// Add a new entry to the log (without structured data - for simple events)
    pub fn log(&mut self, event_type: CombatLogEventType, message: String) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type,
            message,
            position_data: None,
            structured_data: None,
        });
    }

    /// Add a new entry with position data for debugging
    pub fn log_with_position(
        &mut self,
        event_type: CombatLogEventType,
        message: String,
        position_data: PositionData,
    ) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type,
            message,
            position_data: Some(position_data),
            structured_data: None,
        });
    }

    /// Add a structured damage event
    pub fn log_damage(
        &mut self,
        source: CombatantId,
        target: CombatantId,
        ability: String,
        amount: f32,
        is_killing_blow: bool,
        message: String,
    ) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type: CombatLogEventType::Damage,
            message,
            position_data: None,
            structured_data: Some(StructuredEventData::Damage {
                source,
                target,
                ability,
                amount,
                is_killing_blow,
            }),
        });
    }

    /// Add a structured healing event
    pub fn log_healing(
        &mut self,
        source: CombatantId,
        target: CombatantId,
        ability: String,
        amount: f32,
        message: String,
    ) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type: CombatLogEventType::Healing,
            message,
            position_data: None,
            structured_data: Some(StructuredEventData::Healing {
                source,
                target,
                ability,
                amount,
            }),
        });
    }

    /// Add a structured crowd control event
    pub fn log_crowd_control(
        &mut self,
        source: CombatantId,
        target: CombatantId,
        cc_type: String,
        duration_secs: f32,
        message: String,
    ) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type: CombatLogEventType::CrowdControl,
            message,
            position_data: None,
            structured_data: Some(StructuredEventData::CrowdControl {
                source,
                target,
                cc_type,
                duration_secs,
            }),
        });
    }

    /// Add a structured death event
    pub fn log_death(
        &mut self,
        victim: CombatantId,
        killer: Option<CombatantId>,
        message: String,
    ) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type: CombatLogEventType::Death,
            message,
            position_data: None,
            structured_data: Some(StructuredEventData::Death { victim, killer }),
        });
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

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

    // =========================================================================
    // Aggregation Methods for Results Scene
    // =========================================================================

    /// Get total damage dealt by a combatant, broken down by ability
    /// Returns HashMap<AbilityName, TotalDamage>
    pub fn damage_by_ability(&self, combatant_id: &str) -> HashMap<String, f32> {
        let mut result: HashMap<String, f32> = HashMap::new();

        for entry in &self.entries {
            if let Some(StructuredEventData::Damage { source, ability, amount, .. }) = &entry.structured_data {
                if source == combatant_id {
                    *result.entry(ability.clone()).or_insert(0.0) += amount;
                }
            }
        }

        result
    }

    /// Get total healing done by a combatant, broken down by ability
    /// Returns HashMap<AbilityName, TotalHealing>
    pub fn healing_by_ability(&self, combatant_id: &str) -> HashMap<String, f32> {
        let mut result: HashMap<String, f32> = HashMap::new();

        for entry in &self.entries {
            if let Some(StructuredEventData::Healing { source, ability, amount, .. }) = &entry.structured_data {
                if source == combatant_id {
                    *result.entry(ability.clone()).or_insert(0.0) += amount;
                }
            }
        }

        result
    }

    /// Get total damage dealt by a combatant (sum of all abilities)
    pub fn total_damage_dealt(&self, combatant_id: &str) -> f32 {
        self.damage_by_ability(combatant_id).values().sum()
    }

    /// Get total healing done by a combatant (sum of all abilities)
    pub fn total_healing_done(&self, combatant_id: &str) -> f32 {
        self.healing_by_ability(combatant_id).values().sum()
    }

    /// Get total damage taken by a combatant
    pub fn total_damage_taken(&self, combatant_id: &str) -> f32 {
        let mut total = 0.0;

        for entry in &self.entries {
            if let Some(StructuredEventData::Damage { target, amount, .. }) = &entry.structured_data {
                if target == combatant_id {
                    total += amount;
                }
            }
        }

        total
    }

    /// Get number of killing blows by a combatant
    pub fn killing_blows(&self, combatant_id: &str) -> u32 {
        let mut count = 0;

        for entry in &self.entries {
            if let Some(StructuredEventData::Damage { source, is_killing_blow: true, .. }) = &entry.structured_data {
                if source == combatant_id {
                    count += 1;
                }
            }
        }

        count
    }

    /// Get total CC time done by a combatant (in seconds)
    pub fn cc_done_seconds(&self, combatant_id: &str) -> f32 {
        let mut total = 0.0;

        for entry in &self.entries {
            if let Some(StructuredEventData::CrowdControl { source, duration_secs, .. }) = &entry.structured_data {
                if source == combatant_id {
                    total += duration_secs;
                }
            }
        }

        total
    }

    /// Get total CC time received by a combatant (in seconds)
    pub fn cc_received_seconds(&self, combatant_id: &str) -> f32 {
        let mut total = 0.0;

        for entry in &self.entries {
            if let Some(StructuredEventData::CrowdControl { target, duration_secs, .. }) = &entry.structured_data {
                if target == combatant_id {
                    total += duration_secs;
                }
            }
        }

        total
    }

    /// Get all unique combatant IDs that appear in the log
    pub fn all_combatants(&self) -> Vec<String> {
        let mut combatants: std::collections::HashSet<String> = std::collections::HashSet::new();

        for entry in &self.entries {
            match &entry.structured_data {
                Some(StructuredEventData::Damage { source, target, .. }) => {
                    combatants.insert(source.clone());
                    combatants.insert(target.clone());
                }
                Some(StructuredEventData::Healing { source, target, .. }) => {
                    combatants.insert(source.clone());
                    combatants.insert(target.clone());
                }
                Some(StructuredEventData::CrowdControl { source, target, .. }) => {
                    combatants.insert(source.clone());
                    combatants.insert(target.clone());
                }
                Some(StructuredEventData::Death { victim, killer }) => {
                    combatants.insert(victim.clone());
                    if let Some(k) = killer {
                        combatants.insert(k.clone());
                    }
                }
                None => {}
            }
        }

        combatants.into_iter().collect()
    }

    /// Check if a combatant survived (no death event recorded for them)
    pub fn combatant_survived(&self, combatant_id: &str) -> bool {
        for entry in &self.entries {
            if let Some(StructuredEventData::Death { victim, .. }) = &entry.structured_data {
                if victim == combatant_id {
                    return false;
                }
            }
        }
        true
    }
    
    /// Save the combat log to a file with match metadata
    pub fn save_to_file(&self, match_metadata: &MatchMetadata) -> std::io::Result<String> {
        use std::fs::{self, File};
        use std::io::Write;
        use std::time::{SystemTime, UNIX_EPOCH};
        
        // Create logs directory if it doesn't exist
        fs::create_dir_all("match_logs")?;
        
        // Generate filename with timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let filename = format!("match_logs/match_{}.txt", timestamp);
        
        let mut file = File::create(&filename)?;
        
        // Write header
        writeln!(file, "{}", "=".repeat(80))?;
        writeln!(file, "ARENA MATCH REPORT")?;
        writeln!(file, "{}", "=".repeat(80))?;
        writeln!(file)?;
        
        // Write match metadata
        writeln!(file, "MATCH METADATA")?;
        writeln!(file, "{}", "-".repeat(80))?;
        writeln!(file, "Arena: {}", match_metadata.arena_name)?;
        writeln!(file, "Duration: {:.2}s", self.match_time)?;
        writeln!(file, "Winner: {}", match match_metadata.winner {
            None => "DRAW".to_string(),
            Some(1) => "Team 1".to_string(),
            Some(2) => "Team 2".to_string(),
            Some(n) => format!("Team {} (invalid)", n),
        })?;
        writeln!(file)?;
        
        // Write team compositions
        writeln!(file, "TEAM 1 COMPOSITION")?;
        writeln!(file, "{}", "-".repeat(80))?;
        for (i, combatant) in match_metadata.team1.iter().enumerate() {
            writeln!(file, "  Slot {}: {} (HP: {:.0}/{:.0}, Mana: {:.0}/{:.0})",
                i + 1,
                combatant.class_name,
                combatant.final_health,
                combatant.max_health,
                combatant.final_mana,
                combatant.max_mana,
            )?;
            writeln!(file, "    Position: ({:.2}, {:.2}, {:.2})",
                combatant.final_position.0,
                combatant.final_position.1,
                combatant.final_position.2,
            )?;
            writeln!(file, "    Damage Dealt: {:.0}, Damage Taken: {:.0}",
                combatant.damage_dealt,
                combatant.damage_taken,
            )?;
        }
        writeln!(file)?;
        
        writeln!(file, "TEAM 2 COMPOSITION")?;
        writeln!(file, "{}", "-".repeat(80))?;
        for (i, combatant) in match_metadata.team2.iter().enumerate() {
            writeln!(file, "  Slot {}: {} (HP: {:.0}/{:.0}, Mana: {:.0}/{:.0})",
                i + 1,
                combatant.class_name,
                combatant.final_health,
                combatant.max_health,
                combatant.final_mana,
                combatant.max_mana,
            )?;
            writeln!(file, "    Position: ({:.2}, {:.2}, {:.2})",
                combatant.final_position.0,
                combatant.final_position.1,
                combatant.final_position.2,
            )?;
            writeln!(file, "    Damage Dealt: {:.0}, Damage Taken: {:.0}",
                combatant.damage_dealt,
                combatant.damage_taken,
            )?;
        }
        writeln!(file)?;
        
        // Write combat log
        writeln!(file, "COMBAT LOG")?;
        writeln!(file, "{}", "=".repeat(80))?;
        writeln!(file)?;
        
        for entry in &self.entries {
            // Format timestamp
            let timestamp_str = format!("[{:>6.2}s]", entry.timestamp);
            
            // Event type indicator
            let type_str = match entry.event_type {
                CombatLogEventType::Damage => "[DMG]",
                CombatLogEventType::Healing => "[HEAL]",
                CombatLogEventType::AbilityUsed => "[CAST]",
                CombatLogEventType::AuraApplied => "[AURA+]",
                CombatLogEventType::AuraRemoved => "[AURA-]",
                CombatLogEventType::CrowdControl => "[CC]",
                CombatLogEventType::Buff => "[BUFF]",
                CombatLogEventType::Death => "[DEATH]",
                CombatLogEventType::MatchEvent => "[EVENT]",
            };
            
            // Write main log line
            writeln!(file, "{} {} {}", timestamp_str, type_str, entry.message)?;
            
            // Write position data if available
            if let Some(ref pos_data) = entry.position_data {
                writeln!(file, "    Entities: {}", pos_data.entities.join(", "))?;
                for (i, pos) in pos_data.positions.iter().enumerate() {
                    writeln!(file, "      {}: ({:.2}, {:.2}, {:.2})",
                        if i < pos_data.entities.len() { &pos_data.entities[i] } else { "?" },
                        pos.0, pos.1, pos.2
                    )?;
                }
                if let Some(distance) = pos_data.distance {
                    writeln!(file, "    Distance: {:.2} units", distance)?;
                }
            }
        }
        
        writeln!(file)?;
        writeln!(file, "{}", "=".repeat(80))?;
        writeln!(file, "END OF REPORT")?;
        writeln!(file, "{}", "=".repeat(80))?;
        
        Ok(filename)
    }
}

/// Match metadata for saving combat logs
#[derive(Debug, Clone)]
pub struct MatchMetadata {
    pub arena_name: String,
    pub winner: Option<u8>,
    pub team1: Vec<CombatantMetadata>,
    pub team2: Vec<CombatantMetadata>,
}

/// Combatant metadata for match logs
#[derive(Debug, Clone)]
pub struct CombatantMetadata {
    pub class_name: String,
    pub max_health: f32,
    pub final_health: f32,
    pub max_mana: f32,
    pub final_mana: f32,
    pub damage_dealt: f32,
    pub damage_taken: f32,
    pub final_position: (f32, f32, f32),
}

