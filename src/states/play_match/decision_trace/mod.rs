//! AI Decision Trace
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
//! // The winning branch spends the cast gate's clearance (`cast_gate`):
//! // builder.choose(cleared, target, was_instant)
//! builder.finish(); // commits the event; no-ops if no candidates pushed
//! ```
//!
//! Tracing is disabled by default — `DecisionTrace.writer` is `None` until the
//! headless runner installs a writer at match start. Builder
//! calls are still cheap (push to an in-memory Vec) and the flush system
//! discards the vec each frame when no writer is attached.

pub mod builder;
pub mod events;
pub mod writer;

use bevy::prelude::*;

pub use builder::{
    DecisionEventBuilder, DispatchEventBuilder, MovementEventBuilder, TargetEventBuilder,
};
pub use events::{
    AbilityCandidate, AbilityOutcome, ActorView, CandidateStatus, DecisionEvent, EventKind,
    EventPayload, MovementGoalKind, MovementTrigger, NoActionReason, PlayStep, PlayTag, Posture,
    RejectionReason, ResourceKind, SituationChangeKind, SituationName, SituationTag, TapHold,
    TargetCandidate, TargetRejectionReason, TargetView, WindowTag,
};
pub use writer::{flush_decision_trace_system, TraceWriter};

use crate::states::match_config::CharacterClass;
use crate::states::play_match::class_ai::CombatantInfo;
use crate::states::play_match::situation::{MatchPhase, SightPair, Tempo, Window};

/// Resource that owns the in-flight trace state for one match. Holds a buffer of
/// `pending_events` that the flush system drains each frame, plus an optional
/// `TraceWriter` for persisting to disk.
#[derive(Resource, Default)]
pub struct DecisionTrace {
    pub current_frame: u64,
    pub current_sim_time: f32,
    pub seed: u64,
    /// Stamped onto every event. Set at match setup from the `AiProfile` resource.
    pub ai_profile: &'static str,
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
            pet_dispatched_by: None,
            situation: None,
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
            // Cow::Borrowed avoids the per-tick allocation that
            // `.to_string()` would do for the always-static pet type name.
            pet_type: Some(std::borrow::Cow::Borrowed(pet_type)),
            pet_dispatched_by: None,
            situation: None,
        }
    }

    /// Start a `pet_decision` event for a hybrid-model dispatch (Hunter AI
    /// commanded the pet ability). Identical shape to `start_pet_decision` but
    /// sets `dispatched_by` so trace audits can attribute the decision to the
    /// commanding owner. Use the existing `start_pet_decision` for autonomous
    /// pet decisions where the pet AI chose the ability on its own.
    pub fn start_pet_dispatch_decision(
        &mut self,
        actor: ActorView,
        target: Option<TargetView>,
        owner: Entity,
        pet_type: &'static str,
        dispatched_by: Entity,
    ) -> DecisionEventBuilder<'_> {
        DecisionEventBuilder {
            trace: self,
            kind: EventKind::PetDecision,
            actor,
            target,
            candidates: Vec::new(),
            chosen: None,
            pet_owner: Some(owner.index()),
            pet_type: Some(std::borrow::Cow::Borrowed(pet_type)),
            pet_dispatched_by: Some(dispatched_by.index()),
            situation: None,
        }
    }

    /// Start the owner's side of a hybrid-model dispatch: the same event as
    /// [`Self::start_pet_dispatch_decision`], recorded through a
    /// [`DispatchEventBuilder`], which records an order rather than a cast.
    /// The pet's execution of the order is the cast, and is gated.
    pub fn start_pet_command_dispatch(
        &mut self,
        actor: ActorView,
        target: Option<TargetView>,
        owner: Entity,
        pet_type: &'static str,
        dispatched_by: Entity,
    ) -> DispatchEventBuilder<'_> {
        DispatchEventBuilder {
            inner: self.start_pet_dispatch_decision(actor, target, owner, pet_type, dispatched_by),
        }
    }

    /// Start a `movement_decision` event. Mirrors `start_pet_decision`
    /// semantics: the caller records a posture transition or committed
    /// direction change on the builder and commits with `.finish()`; a
    /// builder that records no decision emits nothing (emission gate — this
    /// is what keeps movement events transition-only, never per-tick).
    /// `target` is the movement goal's entity when one exists (e.g., the
    /// enemy healer a Paladin DIP pursues), `None` for direction/point goals.
    pub fn start_movement_decision(
        &mut self,
        actor: ActorView,
        target: Option<TargetView>,
    ) -> MovementEventBuilder<'_> {
        MovementEventBuilder {
            trace: self,
            actor,
            target,
            decision: None,
        }
    }

    /// Start a `target_acquisition` event. Pass the previous primary target
    /// and previous cc_target so the resulting event records both transitions
    /// (the AI may change cc_target without changing primary, and vice versa).
    pub fn start_target_acquisition(
        &mut self,
        actor: ActorView,
        previous_target: Option<Entity>,
        previous_cc_target: Option<Entity>,
    ) -> TargetEventBuilder<'_> {
        TargetEventBuilder {
            trace: self,
            actor,
            previous_target: previous_target.map(|e| e.index()),
            previous_cc_target: previous_cc_target.map(|e| e.index()),
            candidates: Vec::new(),
        }
    }

    /// Record a `situation_change` event for a window (`situation.rs`).
    pub fn record_window_change(
        &mut self,
        team: u8,
        name: SituationName,
        change: SituationChangeKind,
        window: &Window,
    ) {
        let tag = WindowTag::from(window);
        self.push_situation_change(EventPayload::SituationChange {
            team,
            name,
            change,
            subject: Some(tag.subject),
            since: Some(tag.since),
            until: tag.until,
            tempo: None,
            previous_tempo: None,
            phase: None,
            previous_phase: None,
            healer: None,
            rule: None,
            pillar: None,
            reason: None,
            from: None,
        });
    }

    /// Record a `situation_change` event for a team's pillar stance
    /// (`situation.rs`, AS-385): its healer as the subject, its onset, its
    /// pillar and its reason.
    pub fn record_stance_change(
        &mut self,
        team: u8,
        change: SituationChangeKind,
        stance: &crate::states::play_match::class_ai::pillar_stance::StanceWindow,
    ) {
        self.push_situation_change(EventPayload::SituationChange {
            team,
            name: SituationName::PillarStance,
            change,
            subject: Some(stance.healer.index()),
            since: Some(stance.since),
            until: None,
            tempo: None,
            previous_tempo: None,
            phase: None,
            previous_phase: None,
            healer: None,
            rule: None,
            pillar: Some(stance.pillar),
            reason: Some(stance.reason),
            from: None,
        });
    }

    /// Record a `situation_change` event for a team's hunt (AS-394): the
    /// lone enemy healer as the subject, its onset and its pillar.
    pub fn record_hunt_change(
        &mut self,
        team: u8,
        change: SituationChangeKind,
        hunt: &crate::states::play_match::class_ai::hunt::HuntWindow,
    ) {
        self.push_situation_change(EventPayload::SituationChange {
            team,
            name: SituationName::Hunt,
            change,
            subject: Some(hunt.subject.index()),
            since: Some(hunt.since),
            until: None,
            tempo: None,
            previous_tempo: None,
            phase: None,
            previous_phase: None,
            healer: None,
            rule: None,
            pillar: Some(hunt.pillar),
            reason: None,
            from: None,
        });
    }

    /// Record a `situation_change` event for a tempo shift.
    pub fn record_tempo_change(&mut self, team: u8, previous: Tempo, tempo: Tempo) {
        self.push_situation_change(EventPayload::SituationChange {
            team,
            name: SituationName::Tempo,
            change: SituationChangeKind::Shift,
            subject: None,
            since: None,
            until: None,
            tempo: Some(tempo),
            previous_tempo: Some(previous),
            phase: None,
            previous_phase: None,
            healer: None,
            rule: None,
            pillar: None,
            reason: None,
            from: None,
        });
    }

    /// Record a `situation_change` event for a match-phase shift.
    pub fn record_phase_change(&mut self, team: u8, previous: MatchPhase, phase: MatchPhase) {
        self.push_situation_change(EventPayload::SituationChange {
            team,
            name: SituationName::Phase,
            change: SituationChangeKind::Shift,
            subject: None,
            since: None,
            until: None,
            tempo: None,
            previous_tempo: None,
            phase: Some(phase),
            previous_phase: Some(previous),
            healer: None,
            rule: None,
            pillar: None,
            reason: None,
            from: None,
        });
    }

    /// Record a `situation_change` event for a DPS–healer pair's sight yield
    /// (AS-386): `subject` is the DPS, `healer` its healer, `rule` the rule
    /// that now leads the pair and `since` when its current answer began.
    pub fn record_sight_yield_change(
        &mut self,
        team: u8,
        change: SituationChangeKind,
        pair: &SightPair,
    ) {
        self.push_situation_change(EventPayload::SituationChange {
            team,
            name: SituationName::SightYield,
            change,
            subject: Some(pair.dps.index()),
            since: Some(pair.since),
            until: None,
            tempo: None,
            previous_tempo: None,
            phase: None,
            previous_phase: None,
            healer: Some(pair.healer.index()),
            rule: Some(pair.rule),
            pillar: None,
            reason: None,
            from: None,
        });
    }

    /// Record a `situation_change` event for the team's target focus
    /// (AS-389): `subject` the enemy its ranged DPS now shoot, `from` the
    /// target the focus moved off, `since` when it moved.
    pub fn record_focus_change(
        &mut self,
        team: u8,
        change: SituationChangeKind,
        focus: &crate::states::play_match::class_ai::target_focus::Focus,
    ) {
        self.push_situation_change(EventPayload::SituationChange {
            team,
            name: SituationName::TargetFocus,
            change,
            subject: Some(focus.subject.index()),
            since: Some(focus.since),
            until: None,
            tempo: None,
            previous_tempo: None,
            phase: None,
            previous_phase: None,
            healer: None,
            rule: None,
            pillar: None,
            reason: None,
            from: Some(focus.from.index()),
        });
    }

    fn push_situation_change(&mut self, payload: EventPayload) {
        self.pending_events.push(DecisionEvent {
            frame: self.current_frame,
            sim_time: self.current_sim_time,
            seed: self.seed,
            ai_profile: self.ai_profile,
            kind: EventKind::SituationChange,
            actor: None,
            target: None,
            situation: None,
            payload,
        });
    }

    /// Replace the writer (e.g., when starting a new match). Drops any prior
    /// writer, which flushes its buffer.
    pub fn install_writer(&mut self, writer: TraceWriter) {
        self.writer = Some(writer);
    }

    /// Detach and drop the writer (drained on drop). Used between matches in
    /// matrix mode.
    ///
    /// Returns `Ok(())` on successful drain + close, `Err(io::Error)` if the
    /// final flush failed. Callers should surface the error — at match end
    /// this is the authoritative last-frame write and silent failure leaves
    /// the trace file truncated without any signal.
    pub fn close_writer(&mut self) -> std::io::Result<()> {
        let mut result: std::io::Result<()> = Ok(());
        if let Some(writer) = self.writer.as_mut() {
            if !self.pending_events.is_empty() {
                let events = std::mem::take(&mut self.pending_events);
                result = writer.flush_events(events).map(|_count| ());
            }
        }
        self.writer = None;
        self.current_frame = 0;
        self.current_sim_time = 0.0;
        result
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
