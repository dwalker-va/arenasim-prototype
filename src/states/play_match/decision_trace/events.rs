//! Decision Trace Event Types
//!
//! Serializable event types emitted by the AI decision trace. Each event records
//! one AI decision (an ability pick, a target acquisition, or a pet decision)
//! along with the candidate set the AI considered and a typed rejection reason
//! for each candidate that lost.
//!
//! ## Wire format (JSON Lines)
//!
//! One event per line, serialized via `serde_json`. Two serde details that
//! matter for downstream `jq` consumers:
//!
//! - **`EventPayload` is flattened.** Fields like `candidates`, `outcome`,
//!   `previous_target`, `new_target`, `pet_type`, `owner` appear at the top
//!   level of the event object, NOT under a `payload` key. So write
//!   `.outcome.action_taken.ability`, not `.payload.outcome.action_taken.ability`.
//! - **Reason variants use mixed serialization shapes.** Unit variants like
//!   `TargetImmune` serialize as the bare string `"TargetImmune"`. Struct
//!   variants like `OutOfRange { distance, max }` serialize as
//!   `{"OutOfRange": {"distance": 35.0, "max": 12.0}}` — a single-key object
//!   whose key names the variant. Use the `if type == "object" then keys[0]
//!   else . end` jq idiom (or see CLAUDE.md → "Diagnose AI behaviour with the
//!   decision trace") to extract the variant name uniformly. The mixed shape
//!   is the serde default for externally-tagged enums and is preserved here
//!   intentionally — switching to a fully-tagged form would break every jq
//!   query already written against the schema.
//!
//! ## Truncated last line on abort/SIGKILL
//!
//! The writer uses a buffered `BufWriter` for performance. Normal exit (via
//! `close_writer` or `Drop`) flushes the buffer cleanly. But an `abort()`,
//! `SIGKILL`, OOM kill, or hard test-runner timeout SKIPS Drop — leaving the
//! buffer in memory. The trace file's last line is then truncated mid-JSON.
//!
//! Trace consumers should tolerate this: prefer `jq -c '. // empty'` (skip
//! parse errors) or pipe through `head -n -1` when reading a trace from a
//! match that may not have ended cleanly. CI / scripted consumers should
//! treat trailing partial lines as an expected failure mode, not corruption.
//!
//! ## Entity-id stability
//!
//! `actor.entity_id`, `target.entity_id`, `pet_owner`, and the entity_id fields
//! on `TargetCandidate` come from `Bevy::Entity::index()` — generation bits are
//! stripped. Combatant and pet entities are stable across a single match (pets
//! are zeroed in place on death, not despawned). However traps and projectiles
//! ARE despawned mid-match, returning their slots to Bevy's recycler. In
//! practice this is harmless because traps/projectiles never appear in
//! `actor.entity_id` / `target.entity_id` / `pet_owner` — only in fields the
//! trace doesn't expose. If you need across-match entity tracking, use
//! `(actor.team, actor.slot)` as the logical key.

use std::borrow::Cow;

use serde::Serialize;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::{AuraType, DRCategory};

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
        // Primary (kill) target before / after this acquisition tick.
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_target: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        new_target: Option<u32>,
        changed: bool,
        // CC target (the "polymorph this guy" slot) — independent of primary
        // target. Emission fires when either target OR cc_target changed;
        // these fields tell consumers which.
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_cc_target: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        new_cc_target: Option<u32>,
        cc_changed: bool,
        candidates: Vec<TargetCandidate>,
    },
    Pet {
        owner: u32,
        // Cow<'static, str> (not String) so callers can pass a &'static str
        // for the pet type name without allocating per pet per tick. The pet
        // type is always known at compile time (Felhunter / Spider / Boar / Bird).
        pet_type: Cow<'static, str>,
        candidates: Vec<AbilityCandidate>,
        outcome: AbilityOutcome,
        /// When `Some(entity_id)`, this pet decision was dispatched by the
        /// pet's owner (e.g., Hunter AI commanded Spider Web). `None` for
        /// autonomous pet decisions. Serialized only when present so
        /// pre-existing audit recipes that don't filter on this field stay
        /// backward-compatible.
        #[serde(skip_serializing_if = "Option::is_none")]
        dispatched_by: Option<u32>,
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
/// Note `OutOfRange` and `WithinDeadZone` are distinct variants — a jq filter
/// looking only at `OutOfRange` will miss dead-zone rejections (Hunter Aimed
/// Shot below min-range, Warrior Charge below CHARGE_MIN_RANGE). Match both
/// when doing range analysis.
///
/// Adding a new variant requires updating `tests/decision_trace_audit.rs::expected_reasons`
/// (audit test fails the build otherwise) and ensuring at least one reference
/// match exercises the new variant.
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
    DRImmune { category: DRCategory },
    FriendlyBreakableCC,
    SelfIncapacitated,
    Rooted,
    LowerPriorityThanChosen { chosen: AbilityType },
    AlreadyApplied,
    NoValidTarget,
    PreconditionUnmet { note: String },
    /// Pet is below the Heel HP threshold (25%). Emitted from pet_ai_system
    /// when the pet retreats to the owner's flank and suppresses ability
    /// execution. Hunter-dispatched PetCommands targeting this pet are
    /// despawned without execution under the same flag.
    LowHealthHeel,
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
