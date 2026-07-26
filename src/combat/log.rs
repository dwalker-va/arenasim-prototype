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

/// Unique identifier for a combatant in the combat log.
/// Format: "Team {team} {class} #{slot+1}" e.g. "Team 1 Warrior #1"
/// (pets use their type name and the owner's slot, e.g. "Team 1 Spider #2").
/// Built by `states::play_match::utils::combat_log_id` and its wrappers; the
/// 1-based slot suffix keeps same-class teammates distinct.
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
        is_crit: bool,
    },
    /// Healing done from one combatant to another (or self)
    Healing {
        source: CombatantId,
        target: CombatantId,
        ability: String,
        amount: f32,
        is_crit: bool,
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
    /// Ability cast initiated (for timeline visualization)
    AbilityCast {
        caster: CombatantId,
        ability: String,
        target: Option<CombatantId>,
        /// Whether this cast was interrupted before completing
        interrupted: bool,
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
    /// All combatants registered at match start (for timeline display)
    pub registered_combatants: Vec<CombatantId>,
}

impl CombatLog {
    /// Clear the log for a new match
    pub fn clear(&mut self) {
        self.entries.clear();
        self.match_time = 0.0;
        self.registered_combatants.clear();
    }

    /// Register a combatant at match start (for timeline display)
    pub fn register_combatant(&mut self, combatant_id: CombatantId) {
        if !self.registered_combatants.contains(&combatant_id) {
            self.registered_combatants.push(combatant_id);
        }
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
        is_crit: bool,
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
                is_crit,
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
        is_crit: bool,
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
                is_crit,
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

    /// Add a structured ability cast event (for timeline visualization)
    pub fn log_ability_cast(
        &mut self,
        caster: CombatantId,
        ability: String,
        target: Option<CombatantId>,
        message: String,
    ) {
        self.entries.push(CombatLogEntry {
            timestamp: self.match_time,
            event_type: CombatLogEventType::AbilityUsed,
            message,
            position_data: None,
            structured_data: Some(StructuredEventData::AbilityCast {
                caster,
                ability,
                target,
                interrupted: false,
            }),
        });
    }

    /// Flag the most recent `source -> target` damage entry as a killing blow.
    ///
    /// Channel ticks (Drain Life) log their damage in one pass and only learn
    /// the target died in a later application pass, so the tick's `Damage` event
    /// is written with `is_killing_blow: false`. Since `killing_blows` counts the
    /// `Damage` flag (not `Death` events), that kill would go uncredited. This
    /// back-patches the flag on the just-logged tick — the same shape as
    /// [`Self::mark_cast_interrupted`]. Idempotent; a no-op if no match exists.
    pub fn mark_last_damage_killing_blow(&mut self, source_id: &str, target_id: &str) {
        for entry in self.entries.iter_mut().rev() {
            if let Some(StructuredEventData::Damage { source, target, is_killing_blow, .. }) =
                &mut entry.structured_data
            {
                if source == source_id && target == target_id {
                    *is_killing_blow = true;
                    return;
                }
            }
        }
    }

    /// Mark the most recent ability cast by a combatant as interrupted
    pub fn mark_cast_interrupted(&mut self, caster_id: &str, ability_name: &str) {
        // Find the most recent matching ability cast and mark it interrupted
        for entry in self.entries.iter_mut().rev() {
            if let Some(StructuredEventData::AbilityCast { caster, ability, interrupted, .. }) = &mut entry.structured_data {
                if caster == caster_id && ability == ability_name {
                    *interrupted = true;
                    return;
                }
            }
        }
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

    /// Get all ability casts for a specific combatant (for timeline visualization)
    /// Returns Vec<(timestamp, ability_name, was_interrupted)> sorted by timestamp
    pub fn ability_casts_for(&self, combatant_id: &str) -> Vec<(f32, &str, bool)> {
        self.entries
            .iter()
            .filter_map(|e| {
                if let Some(StructuredEventData::AbilityCast { caster, ability, interrupted, .. }) = &e.structured_data {
                    if caster == combatant_id {
                        Some((e.timestamp, ability.as_str(), *interrupted))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    // =========================================================================
    // Aggregation Methods for Results Scene
    // =========================================================================

    /// Get total damage dealt by a combatant, broken down by ability.
    /// Returns HashMap<AbilityName, TotalDamage>. Thin wrapper over
    /// [`Self::damage_by_ability_including_pets`] with no pet links, so the two
    /// can never drift out of sync (a split like that was the original results
    /// bug).
    pub fn damage_by_ability(&self, combatant_id: &str) -> HashMap<String, f32> {
        self.damage_by_ability_including_pets(combatant_id, &HashMap::new())
    }

    /// Like [`Self::damage_by_ability`], but also folds in damage dealt by the
    /// combatant's pets. `pet_links` maps a pet's source id (e.g.
    /// `"Team 1 Spider #2"`) to `(owner_id, pet_display_name)` (e.g.
    /// `("Team 1 Hunter #2", "Spider")`); any log entry whose source maps to
    /// `owner_id` is credited to the owner under the label
    /// `"<pet_display_name>: <ability>"` (e.g. `"Spider: Auto Attack"`), keeping
    /// it distinct from the owner's own abilities. Entries sourced directly by
    /// `owner_id` are counted unchanged. With an empty `pet_links` this is a
    /// plain per-ability damage tally.
    pub fn damage_by_ability_including_pets(
        &self,
        owner_id: &str,
        pet_links: &HashMap<String, (String, String)>,
    ) -> HashMap<String, f32> {
        let mut result: HashMap<String, f32> = HashMap::new();

        for entry in &self.entries {
            if let Some(StructuredEventData::Damage { source, ability, amount, .. }) = &entry.structured_data {
                if source == owner_id {
                    *result.entry(ability.clone()).or_insert(0.0) += amount;
                } else if let Some((mapped_owner, pet_name)) = pet_links.get(source) {
                    if mapped_owner == owner_id {
                        *result.entry(format!("{pet_name}: {ability}")).or_insert(0.0) += amount;
                    }
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

    /// Get number of killing blows by a combatant. Thin wrapper over
    /// [`Self::killing_blows_including_pets`] with no pet links, so the plain
    /// and pet-aware counts can never drift apart.
    pub fn killing_blows(&self, combatant_id: &str) -> u32 {
        self.killing_blows_including_pets(combatant_id, &HashMap::new())
    }

    /// Like [`Self::killing_blows`], but also credits killing blows dealt by the
    /// combatant's pets to the owner. `pet_links` maps a pet's source id (e.g.
    /// `"Team 1 Spider #2"`) to `(owner_id, pet_display_name)` (e.g.
    /// `("Team 1 Hunter #2", "Spider")`) — the same map the Results screen uses
    /// for the damage breakdown. A killing blow whose source maps to `owner_id`
    /// counts toward the owner. With an empty `pet_links` this is a plain
    /// killing-blow tally. Mirrors [`Self::damage_by_ability_including_pets`] so
    /// the K column accounts for pet kills the way the DMG column folds in pet
    /// damage.
    pub fn killing_blows_including_pets(
        &self,
        owner_id: &str,
        pet_links: &HashMap<String, (String, String)>,
    ) -> u32 {
        let mut count = 0;

        for entry in &self.entries {
            if let Some(StructuredEventData::Damage { source, is_killing_blow: true, .. }) = &entry.structured_data {
                let credited = source == owner_id
                    || pet_links
                        .get(source)
                        .is_some_and(|(mapped_owner, _)| mapped_owner == owner_id);
                if credited {
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

    /// Get all unique combatant IDs (from registered list, or extracted from log entries)
    pub fn all_combatants(&self) -> Vec<String> {
        // Use registered combatants if available (preferred - ensures all columns show from start)
        if !self.registered_combatants.is_empty() {
            return self.registered_combatants.clone();
        }

        // Fallback: extract from log entries
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
                Some(StructuredEventData::AbilityCast { caster, target, .. }) => {
                    combatants.insert(caster.clone());
                    if let Some(t) = target {
                        combatants.insert(t.clone());
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
    /// If `output_path` is provided, saves to that exact path.
    /// Otherwise, generates a timestamped filename in match_logs/
    pub fn save_to_file(&self, match_metadata: &MatchMetadata, output_path: Option<&str>) -> std::io::Result<String> {
        use std::fs::{self, File};
        use std::io::Write;
        use std::time::{SystemTime, UNIX_EPOCH};

        let filename = if let Some(path) = output_path {
            // Use custom path - ensure parent directory exists
            if let Some(parent) = std::path::Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            path.to_string()
        } else {
            // Create logs directory if it doesn't exist
            fs::create_dir_all("match_logs")?;

            // Generate filename with timestamp
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            format!("match_logs/match_{}.txt", timestamp)
        };
        
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
        writeln!(file, "Seed: {}", match match_metadata.random_seed {
            Some(seed) => seed.to_string(),
            None => "<unseeded>".to_string(),
        })?;
        writeln!(file)?;
        
        // Write team compositions
        writeln!(file, "TEAM 1 COMPOSITION")?;
        writeln!(file, "{}", "-".repeat(80))?;
        for (i, combatant) in match_metadata.team1.iter().enumerate() {
            write_combatant_block(&mut file, i + 1, combatant)?;
        }
        writeln!(file)?;

        writeln!(file, "TEAM 2 COMPOSITION")?;
        writeln!(file, "{}", "-".repeat(80))?;
        for (i, combatant) in match_metadata.team2.iter().enumerate() {
            write_combatant_block(&mut file, i + 1, combatant)?;
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
    /// Seed used for deterministic RNG (None = unseeded entropy).
    /// Embedded in the log header so a saved match can be reproduced.
    pub random_seed: Option<u64>,
    pub team1: Vec<CombatantMetadata>,
    pub team2: Vec<CombatantMetadata>,
}

/// Write a single combatant's stat block (HP/mana, position, damage, mitigation) to the report.
fn write_combatant_block(
    file: &mut std::fs::File,
    slot_number: usize,
    combatant: &CombatantMetadata,
) -> std::io::Result<()> {
    use std::io::Write;

    writeln!(
        file,
        "  Slot {}: {} (HP: {:.0}/{:.0}, Mana: {:.0}/{:.0})",
        slot_number,
        combatant.class_name,
        combatant.final_health,
        combatant.max_health,
        combatant.final_mana,
        combatant.max_mana,
    )?;
    writeln!(
        file,
        "    Position: ({:.2}, {:.2}, {:.2})",
        combatant.final_position.0,
        combatant.final_position.1,
        combatant.final_position.2,
    )?;
    writeln!(
        file,
        "    Damage Dealt: {:.0}, Damage Taken: {:.0}",
        combatant.damage_dealt, combatant.damage_taken,
    )?;

    // Mitigated line: omit zero schools, skip line entirely if everything is zero.
    let school_labels = ["frost", "holy", "shadow", "arcane", "fire", "nature"];
    let any_resistance = combatant.damage_mitigated_by_resistance.iter().any(|v| *v > 0.0);
    if combatant.damage_mitigated_by_armor > 0.0 || any_resistance {
        let mut parts: Vec<String> = Vec::new();
        if combatant.damage_mitigated_by_armor > 0.0 {
            parts.push(format!("armor={:.0}", combatant.damage_mitigated_by_armor));
        }
        for (idx, label) in school_labels.iter().enumerate() {
            let value = combatant.damage_mitigated_by_resistance[idx];
            if value > 0.0 {
                parts.push(format!("{}={:.0}", label, value));
            }
        }
        writeln!(file, "    Mitigated: {}", parts.join(" "))?;
    }

    Ok(())
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
    /// Total physical damage prevented by armor over the match.
    pub damage_mitigated_by_armor: f32,
    /// Total magical damage prevented by spell resistance per school.
    /// Index mapping: Frost=0, Holy=1, Shadow=2, Arcane=3, Fire=4, Nature=5.
    pub damage_mitigated_by_resistance: [f32; 6],
    pub final_position: (f32, f32, f32),
}


#[cfg(test)]
mod pet_attribution_tests {
    use super::*;

    fn dmg(log: &mut CombatLog, source: &str, ability: &str, amount: f32) {
        log.log_damage(
            source.to_string(),
            "Team 2 Warrior #1".to_string(),
            ability.to_string(),
            amount,
            false,
            false,
            String::new(),
        );
    }

    fn killing_blow(log: &mut CombatLog, source: &str, ability: &str) {
        log.log_damage(
            source.to_string(),
            "Team 2 Warrior #1".to_string(),
            ability.to_string(),
            50.0,
            true,
            false,
            String::new(),
        );
    }

    #[test]
    fn folds_pet_damage_into_owner_with_labelled_abilities() {
        let mut log = CombatLog::default();
        dmg(&mut log, "Team 1 Hunter", "Aimed Shot", 100.0);
        dmg(&mut log, "Team 1 Hunter", "Auto Shot", 40.0);
        dmg(&mut log, "Team 1 Spider", "Auto Attack", 30.0);
        dmg(&mut log, "Team 1 Spider", "Auto Attack", 20.0);
        dmg(&mut log, "Team 1 Spider", "Web", 5.0);
        // Unrelated team-2 damage must never leak into team 1's breakdown.
        dmg(&mut log, "Team 2 Mage", "Frostbolt", 999.0);

        let mut links = HashMap::new();
        links.insert(
            "Team 1 Spider".to_string(),
            ("Team 1 Hunter".to_string(), "Spider".to_string()),
        );

        let breakdown = log.damage_by_ability_including_pets("Team 1 Hunter", &links);

        assert_eq!(breakdown.get("Aimed Shot"), Some(&100.0));
        assert_eq!(breakdown.get("Auto Shot"), Some(&40.0));
        // Same-ability pet hits accumulate under one labelled key.
        assert_eq!(breakdown.get("Spider: Auto Attack"), Some(&50.0));
        assert_eq!(breakdown.get("Spider: Web"), Some(&5.0));
        assert!(breakdown.get("Frostbolt").is_none());
        // Breakdown total now matches the owner's damage + pet damage.
        let total: f32 = breakdown.values().sum();
        assert_eq!(total, 195.0);
    }

    #[test]
    fn empty_links_matches_plain_damage_by_ability() {
        let mut log = CombatLog::default();
        dmg(&mut log, "Team 1 Hunter", "Aimed Shot", 100.0);
        dmg(&mut log, "Team 1 Spider", "Auto Attack", 30.0);

        let empty = HashMap::new();
        let with_pets = log.damage_by_ability_including_pets("Team 1 Hunter", &empty);
        let plain = log.damage_by_ability("Team 1 Hunter");
        assert_eq!(with_pets, plain);
        // Pet damage is absent without a link entry.
        assert!(with_pets.get("Spider: Auto Attack").is_none());
    }

    #[test]
    fn credits_pet_killing_blows_to_the_owner() {
        let mut log = CombatLog::default();
        killing_blow(&mut log, "Team 1 Hunter", "Aimed Shot"); // owner's own kill
        killing_blow(&mut log, "Team 1 Spider", "Auto Attack"); // pet's kill -> owner
        dmg(&mut log, "Team 1 Spider", "Auto Attack", 30.0); // non-lethal pet hit, ignored
        killing_blow(&mut log, "Team 2 Mage", "Frostbolt"); // enemy kill, must not leak

        let mut links = HashMap::new();
        links.insert(
            "Team 1 Spider".to_string(),
            ("Team 1 Hunter".to_string(), "Spider".to_string()),
        );

        // Owner is credited with its own kill + the pet's kill.
        assert_eq!(log.killing_blows_including_pets("Team 1 Hunter", &links), 2);
        // The plain method still only sees the owner's own kill.
        assert_eq!(log.killing_blows("Team 1 Hunter"), 1);
        // Enemy kills never leak into the owner's count.
        assert_eq!(log.killing_blows_including_pets("Team 2 Mage", &links), 1);
    }

    #[test]
    fn mark_last_damage_killing_blow_credits_the_channel_finish() {
        // Mirrors Drain Life: the tick's Damage lands with is_killing_blow=false,
        // then the application pass learns the target died and back-patches it.
        //
        // The ids here are hand-written (suffixed to be representative). This
        // test can only exercise the back-patch *mechanism* — flagging the most
        // recent source→target Damage entry — because `log.rs` sits below
        // `states::play_match::utils::{combatant_id, log_id_from_parts}` in the
        // module graph and cannot call the real id constructors. The production
        // guarantee that the tick's source id and the death-block back-patch id
        // are byte-identical is enforced at their construction sites
        // (`combat_core/casting.rs`, both pet-aware), exercised by the headless
        // integration path.
        let mut log = CombatLog::default();
        dmg(&mut log, "Team 1 Warlock #1", "Drain Life (tick)", 30.0);
        dmg(&mut log, "Team 1 Warlock #1", "Drain Life (tick)", 30.0); // the lethal one
        assert_eq!(log.killing_blows("Team 1 Warlock #1"), 0);

        // Back-patch the most recent Warlock -> Warrior damage entry.
        log.mark_last_damage_killing_blow("Team 1 Warlock #1", "Team 2 Warrior #1");
        assert_eq!(log.killing_blows("Team 1 Warlock #1"), 1);
        // Only the most-recent matching entry is flagged.
        let flagged = log.entries.iter().filter(|e| matches!(
            &e.structured_data,
            Some(StructuredEventData::Damage { is_killing_blow: true, .. })
        )).count();
        assert_eq!(flagged, 1);
    }

    #[test]
    fn empty_links_kills_match_plain_killing_blows() {
        let mut log = CombatLog::default();
        killing_blow(&mut log, "Team 1 Hunter", "Aimed Shot");
        killing_blow(&mut log, "Team 1 Spider", "Auto Attack");

        let empty = HashMap::new();
        // Without links the pet's kill is not credited — identical to the plain method.
        assert_eq!(
            log.killing_blows_including_pets("Team 1 Hunter", &empty),
            log.killing_blows("Team 1 Hunter"),
        );
        assert_eq!(log.killing_blows_including_pets("Team 1 Hunter", &empty), 1);
    }
}
