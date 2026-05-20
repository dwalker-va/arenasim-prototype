//! Decision Trace Event Types
//!
//! Serializable event types emitted by the AI decision trace. Each event records
//! one AI decision (an ability pick, a target acquisition, or a pet decision)
//! along with the candidate set the AI considered and a typed rejection reason
//! for each candidate that lost.
//!
//! Event schema is JSON Lines (JSONL) — one event per line, serialized via serde_json.
//! Reason variants carry structured payloads so `jq` filters work directly:
//! `jq '.candidates[].reason.OutOfRange.distance > 50' < trace.jsonl`.
//!
//! Initial variant set is finalized in U13 (predicate enumeration pass). New variants
//! must be added to `tests/decision_trace_audit.rs::expected_reasons` to pass the audit.

use serde::Serialize;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::AuraType;

/// One AI decision event. Serializes to a single JSONL line.
#[derive(Serialize, Clone, Debug)]
pub struct DecisionEvent {
    pub frame: u64,
    pub sim_time: f32,
    pub seed: u64,
    pub kind: EventKind,
    pub actor: ActorView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetView>,
    #[serde(flatten)]
    pub payload: EventPayload,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AbilityDecision,
    TargetAcquisition,
    PetDecision,
}

/// Payload varies by `kind`. Flattened into the parent event JSON object.
#[derive(Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum EventPayload {
    Ability {
        candidates: Vec<AbilityCandidate>,
        outcome: AbilityOutcome,
    },
    Target {
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_target: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        new_target: Option<u32>,
        changed: bool,
        candidates: Vec<TargetCandidate>,
    },
    Pet {
        owner: u32,
        pet_type: String,
        candidates: Vec<AbilityCandidate>,
        outcome: AbilityOutcome,
    },
}

#[derive(Serialize, Clone, Debug)]
pub struct ActorView {
    pub entity_id: u32,
    pub team: u8,
    pub slot: u8,
    pub class: CharacterClass,
    pub hp_pct: f32,
    pub mana_pct: f32,
    pub position: [f32; 3],
}

#[derive(Serialize, Clone, Debug)]
pub struct TargetView {
    pub entity_id: u32,
    pub class: CharacterClass,
    pub hp_pct: f32,
    pub distance: f32,
}

#[derive(Serialize, Clone, Debug)]
pub struct AbilityCandidate {
    pub ability: AbilityType,
    pub status: CandidateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<RejectionReason>,
}

#[derive(Serialize, Clone, Debug)]
pub struct TargetCandidate {
    pub entity_id: u32,
    pub class: CharacterClass,
    pub score: i32,
    pub status: CandidateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<TargetRejectionReason>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Chosen,
    Rejected,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum AbilityOutcome {
    #[serde(rename = "action_taken")]
    ActionTaken {
        ability: AbilityType,
        #[serde(skip_serializing_if = "Option::is_none")]
        target_id: Option<u32>,
        was_instant: bool,
    },
    #[serde(rename = "no_action")]
    NoAction { primary_reason: NoActionReason },
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoActionReason {
    AllCandidatesRejected,
    SelfIncapacitated,
    OnGlobalCooldown,
    NoValidTarget,
    TargetImmune,
}

/// Closed set of reasons an ability can be rejected. Variants carry structured
/// payloads so the rejection is greppable with `jq` AND retains the numeric
/// context the predicate observed.
///
/// Adding a new variant requires updating `tests/decision_trace_audit.rs::expected_reasons`
/// (audit test fails the build otherwise) and ensuring at least one reference match
/// in U12 exercises the new variant.
#[derive(Serialize, Clone, Debug)]
pub enum RejectionReason {
    OutOfRange { distance: f32, max: f32 },
    WithinDeadZone { distance: f32, min: f32 },
    OnCooldown { remaining: f32 },
    InsufficientMana { have: f32, need: f32 },
    InsufficientResource { resource: ResourceKind, have: f32, need: f32 },
    SilencedOrLocked { school: SpellSchool },
    TargetImmune,
    TargetAlreadyCCd { cc_type: AuraType },
    DRImmune { category: String },
    FriendlyBreakableCC,
    SelfIncapacitated,
    Rooted,
    LowerPriorityThanChosen { chosen: AbilityType },
    AlreadyApplied,
    NoValidTarget,
    PreconditionUnmet { note: String },
}

#[derive(Serialize, Clone, Debug)]
pub enum TargetRejectionReason {
    OutOfRange { distance: f32, max: f32 },
    Stealthed,
    Dead,
    Immune,
    CCd { cc_type: AuraType },
    LowerScoreThanChosen { score: i32, chosen_score: i32 },
    KillTargetOverride,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Rage,
    Energy,
    ComboPoints,
    Mana,
}
