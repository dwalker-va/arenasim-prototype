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
    pub(super) pet_type: Option<std::borrow::Cow<'static, str>>,
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

    /// Commit the event. Implicit emission gate: no-ops when no candidates were
    /// pushed AND no explicit outcome was set. Use `finish_no_action(reason)` to
    /// emit a NoAction event from a top-level short-circuit (e.g., target immune)
    /// that bypasses the gate.
    pub fn finish(mut self) {
        if self.candidates.is_empty() {
            return;
        }
        // Take `chosen` so the destructuring move doesn't partially consume
        // `self` (which `emit` still needs).
        let outcome = match self.chosen.take() {
            Some(action) => AbilityOutcome::ActionTaken {
                ability: action.ability,
                target_id: action.target_id,
                was_instant: action.was_instant,
            },
            None => AbilityOutcome::NoAction {
                primary_reason: NoActionReason::AllCandidatesRejected,
            },
        };
        self.emit(outcome);
    }

    /// Commit with an explicit no-action reason. Unlike `finish`, this always
    /// emits — including when no candidates have been pushed. Use it for
    /// top-level short-circuits (e.g., `TargetImmune`, `SelfIncapacitated`)
    /// where the AI made a decision NOT to consider any abilities. Without
    /// this path, those events would be silently swallowed by the
    /// candidates-empty gate, defeating diagnostic value for skip cases.
    pub fn finish_no_action(self, primary_reason: NoActionReason) {
        let outcome = AbilityOutcome::NoAction { primary_reason };
        self.emit(outcome);
    }

    /// Shared serialization path for `finish` and `finish_no_action`. Selects
    /// `EventPayload::Pet` when this is a pet_decision event with owner/type
    /// set, else `EventPayload::Ability`.
    fn emit(self, outcome: AbilityOutcome) {
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
/// (chosen + rejected with reasons) and finishes with the new target and
/// cc_target. The event payload carries both the primary-target transition
/// and the cc_target transition so downstream consumers can distinguish
/// "Rogue switched kill targets" from "Mage switched its Polymorph mark".
pub struct TargetEventBuilder<'a> {
    pub(super) trace: &'a mut DecisionTrace,
    pub(super) actor: ActorView,
    pub(super) previous_target: Option<u32>,
    pub(super) previous_cc_target: Option<u32>,
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

    pub fn finish(self, new_target: Option<Entity>, new_cc_target: Option<Entity>) {
        let new_target_id = new_target.map(|e| e.index());
        let new_cc_target_id = new_cc_target.map(|e| e.index());
        let changed = self.previous_target != new_target_id;
        let cc_changed = self.previous_cc_target != new_cc_target_id;

        // Skip emission when nothing meaningful changed and there are no
        // candidates to record. This filters out idle ticks where target
        // acquisition runs but the state is stable.
        if !changed
            && !cc_changed
            && self.candidates.is_empty()
            && new_target_id.is_none()
            && self.previous_target.is_none()
        {
            return;
        }

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
                previous_cc_target: self.previous_cc_target,
                new_cc_target: new_cc_target_id,
                cc_changed,
                candidates: self.candidates,
            },
        };
        self.trace.pending_events.push(event);
    }
}
