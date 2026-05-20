//! AI Decision Trace — Phase 1
//!
//! Structured JSONL trace of AI decisions. See
//! `docs/plans/2026-05-18-001-feat-ai-decision-trace-plan.md` for the full design.
//!
//! ## Usage from class AI
//!
//! ```ignore
//! // Inside decide_<class>_action, after the GCD short-circuit:
//! let mut builder = decision_trace.start_ability_decision(actor_view, target_view);
//! // ... call try_* helpers passing &mut builder ...
//! // Each helper calls builder.reject(ability, reason) at predicate gates
//! // The winning branch calls builder.choose(ability, target, was_instant)
//! builder.finish(); // commits the event; no-ops if no candidates pushed
//! ```
//!
//! Tracing is disabled by default — `DecisionTrace.writer` is `None` until the
//! headless runner installs a writer at match start (wired in U10). Builder
//! calls are still cheap (push to an in-memory Vec) and the flush system
//! discards the vec each frame when no writer is attached.

pub mod builder;
pub mod events;
pub mod writer;

use bevy::prelude::*;

pub use builder::{DecisionEventBuilder, TargetEventBuilder};
pub use events::{
    AbilityCandidate, AbilityOutcome, ActorView, CandidateStatus, DecisionEvent, EventKind,
    NoActionReason, RejectionReason, ResourceKind, TargetCandidate, TargetRejectionReason,
    TargetView,
};
pub use writer::{flush_decision_trace_system, TraceWriter};

use crate::states::match_config::CharacterClass;
use crate::states::play_match::class_ai::CombatantInfo;

/// Resource that owns the in-flight trace state for one match. Holds a buffer of
/// `pending_events` that the flush system drains each frame, plus an optional
/// `TraceWriter` for persisting to disk.
#[derive(Resource, Default)]
pub struct DecisionTrace {
    pub current_frame: u64,
    pub current_sim_time: f32,
    pub seed: u64,
    pub pending_events: Vec<DecisionEvent>,
    pub writer: Option<TraceWriter>,
}

impl DecisionTrace {
    /// Start an `ability_decision` event. Caller fills the builder via `reject`
    /// / `choose` and calls `finish`/`finish_no_action`.
    pub fn start_ability_decision(
        &mut self,
        actor: ActorView,
        target: Option<TargetView>,
    ) -> DecisionEventBuilder<'_> {
        DecisionEventBuilder {
            trace: self,
            kind: EventKind::AbilityDecision,
            actor,
            target,
            candidates: Vec::new(),
            chosen: None,
            pet_owner: None,
            pet_type: None,
        }
    }

    /// Start a `pet_decision` event. Carries `owner` and `pet_type` fields in
    /// addition to the ability-decision payload.
    pub fn start_pet_decision(
        &mut self,
        actor: ActorView,
        target: Option<TargetView>,
        owner: Entity,
        pet_type: &'static str,
    ) -> DecisionEventBuilder<'_> {
        DecisionEventBuilder {
            trace: self,
            kind: EventKind::PetDecision,
            actor,
            target,
            candidates: Vec::new(),
            chosen: None,
            pet_owner: Some(owner.index()),
            pet_type: Some(pet_type.to_string()),
        }
    }

    /// Start a `target_acquisition` event.
    pub fn start_target_acquisition(
        &mut self,
        actor: ActorView,
        previous_target: Option<Entity>,
    ) -> TargetEventBuilder<'_> {
        TargetEventBuilder {
            trace: self,
            actor,
            previous_target: previous_target.map(|e| e.index()),
            candidates: Vec::new(),
        }
    }

    /// Replace the writer (e.g., when starting a new match). Drops any prior
    /// writer, which flushes its buffer.
    pub fn install_writer(&mut self, writer: TraceWriter) {
        self.writer = Some(writer);
    }

    /// Detach and drop the writer (drained on drop). Used between matches in
    /// matrix mode.
    pub fn close_writer(&mut self) {
        // Drain any in-flight events into the writer before dropping it so the
        // last frame's events aren't lost.
        if let Some(writer) = self.writer.as_mut() {
            if !self.pending_events.is_empty() {
                let events = std::mem::take(&mut self.pending_events);
                let _ = writer.flush_events(events);
            }
        }
        self.writer = None;
        self.current_frame = 0;
        self.current_sim_time = 0.0;
    }
}

// ============================================================================
// Helpers for building ActorView / TargetView from runtime state
// ============================================================================

impl ActorView {
    /// Build an `ActorView` from a `CombatantInfo` snapshot (used by class AI).
    pub fn from_info(info: &CombatantInfo) -> Self {
        Self {
            entity_id: info.entity.index(),
            team: info.team,
            slot: info.slot,
            class: info.class,
            hp_pct: info.health_pct(),
            mana_pct: info.mana_pct(),
            position: [info.position.x, info.position.y, info.position.z],
        }
    }

    /// Build from raw fields when no `CombatantInfo` is at hand.
    pub fn from_raw(
        entity: Entity,
        team: u8,
        slot: u8,
        class: CharacterClass,
        hp_pct: f32,
        mana_pct: f32,
        position: Vec3,
    ) -> Self {
        Self {
            entity_id: entity.index(),
            team,
            slot,
            class,
            hp_pct,
            mana_pct,
            position: [position.x, position.y, position.z],
        }
    }
}

impl TargetView {
    pub fn from_info(info: &CombatantInfo, observer_pos: Vec3) -> Self {
        Self {
            entity_id: info.entity.index(),
            class: info.class,
            hp_pct: info.health_pct(),
            distance: observer_pos.distance(info.position),
        }
    }
}
