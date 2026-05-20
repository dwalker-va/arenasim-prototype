//! Decision Trace Builders
//!
//! The builder API class AI uses to record one decision. Builders accumulate
//! candidates (rejected with reasons + the one chosen) and emit a single
//! `DecisionEvent` on `.finish()`.
//!
//! Emission gate: if no candidates were pushed before `.finish()`, no event is
//! emitted. This prevents per-frame noise from class AI functions that
//! short-circuit before evaluating any predicate (e.g., GCD check at the top).

use bevy::prelude::Entity;

use super::events::{
    AbilityCandidate, AbilityOutcome, ActorView, CandidateStatus, DecisionEvent, EventKind,
    EventPayload, NoActionReason, RejectionReason, TargetCandidate, TargetRejectionReason,
    TargetView,
};
use super::DecisionTrace;
use crate::states::play_match::abilities::AbilityType;

/// Builder for one `ability_decision` or `pet_decision` event.
///
/// Class AI calls `.reject(ability, reason)` at each predicate gate and
/// `.choose(ability, ...)` on the winning branch. Drop without `.finish()` to
/// discard.
pub struct DecisionEventBuilder<'a> {
    pub(super) trace: &'a mut DecisionTrace,
    pub(super) kind: EventKind,
    pub(super) actor: ActorView,
    pub(super) target: Option<TargetView>,
    pub(super) candidates: Vec<AbilityCandidate>,
    pub(super) chosen: Option<ChosenAction>,
    pub(super) pet_owner: Option<u32>,
    pub(super) pet_type: Option<String>,
}

pub(super) struct ChosenAction {
    pub ability: AbilityType,
    pub target_id: Option<u32>,
    pub was_instant: bool,
}

impl<'a> DecisionEventBuilder<'a> {
    /// Record an ability as considered-and-rejected with a typed reason.
    pub fn reject(&mut self, ability: AbilityType, reason: RejectionReason) {
        self.candidates.push(AbilityCandidate {
            ability,
            status: CandidateStatus::Rejected,
            reason: Some(reason),
        });
    }

    /// Record the chosen ability. Should be called at most once per builder; if
    /// called multiple times, the last call wins for the outcome but all
    /// chosens remain in the candidate list (caller should ensure single-choose
    /// to match the if-chain semantics).
    pub fn choose(&mut self, ability: AbilityType, target: Option<Entity>, was_instant: bool) {
        self.candidates.push(AbilityCandidate {
            ability,
            status: CandidateStatus::Chosen,
            reason: None,
        });
        self.chosen = Some(ChosenAction {
            ability,
            target_id: target.map(|e| e.index()),
            was_instant,
        });
    }

    /// Commit the event. No-ops when no candidates were pushed (emission gate).
    pub fn finish(self) {
        if self.candidates.is_empty() {
            return;
        }
        let outcome = match self.chosen {
            Some(action) => AbilityOutcome::ActionTaken {
                ability: action.ability,
                target_id: action.target_id,
                was_instant: action.was_instant,
            },
            None => AbilityOutcome::NoAction {
                primary_reason: NoActionReason::AllCandidatesRejected,
            },
        };
        let payload = match (self.kind, self.pet_owner, self.pet_type) {
            (EventKind::PetDecision, Some(owner), Some(pet_type)) => EventPayload::Pet {
                owner,
                pet_type,
                candidates: self.candidates,
                outcome,
            },
            _ => EventPayload::Ability {
                candidates: self.candidates,
                outcome,
            },
        };
        let event = DecisionEvent {
            frame: self.trace.current_frame,
            sim_time: self.trace.current_sim_time,
            seed: self.trace.seed,
            kind: self.kind,
            actor: self.actor,
            target: self.target,
            payload,
        };
        self.trace.pending_events.push(event);
    }

    /// Commit with an explicit no-action reason (e.g., NoValidTarget short-circuit).
    /// Still gated on candidates being non-empty — caller can use this when one or
    /// more rejections were recorded before the no-action exit.
    pub fn finish_no_action(self, primary_reason: NoActionReason) {
        if self.candidates.is_empty() {
            return;
        }
        let outcome = AbilityOutcome::NoAction { primary_reason };
        let payload = match (self.kind, self.pet_owner, self.pet_type) {
            (EventKind::PetDecision, Some(owner), Some(pet_type)) => EventPayload::Pet {
                owner,
                pet_type,
                candidates: self.candidates,
                outcome,
            },
            _ => EventPayload::Ability {
                candidates: self.candidates,
                outcome,
            },
        };
        let event = DecisionEvent {
            frame: self.trace.current_frame,
            sim_time: self.trace.current_sim_time,
            seed: self.trace.seed,
            kind: self.kind,
            actor: self.actor,
            target: self.target,
            payload,
        };
        self.trace.pending_events.push(event);
    }
}

/// Builder for one `target_acquisition` event. Caller pushes scored enemies
/// (chosen + rejected with reasons) and finishes with the new target.
pub struct TargetEventBuilder<'a> {
    pub(super) trace: &'a mut DecisionTrace,
    pub(super) actor: ActorView,
    pub(super) previous_target: Option<u32>,
    pub(super) candidates: Vec<TargetCandidate>,
}

impl<'a> TargetEventBuilder<'a> {
    pub fn score(
        &mut self,
        enemy: Entity,
        class: crate::states::match_config::CharacterClass,
        score: i32,
        status: CandidateStatus,
        reason: Option<TargetRejectionReason>,
    ) {
        self.candidates.push(TargetCandidate {
            entity_id: enemy.index(),
            class,
            score,
            status,
            reason,
        });
    }

    pub fn finish(self, new_target: Option<Entity>) {
        if self.candidates.is_empty() && new_target.is_none() && self.previous_target.is_none() {
            return;
        }
        let new_target_id = new_target.map(|e| e.index());
        let changed = self.previous_target != new_target_id;
        let event = DecisionEvent {
            frame: self.trace.current_frame,
            sim_time: self.trace.current_sim_time,
            seed: self.trace.seed,
            kind: EventKind::TargetAcquisition,
            actor: self.actor,
            target: None,
            payload: EventPayload::Target {
                previous_target: self.previous_target,
                new_target: new_target_id,
                changed,
                candidates: self.candidates,
            },
        };
        self.trace.pending_events.push(event);
    }
}
