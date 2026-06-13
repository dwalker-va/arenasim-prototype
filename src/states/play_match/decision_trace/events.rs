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
//! - **`movement_decision` events flatten the same way.** `posture`,
//!   `previous_posture`, `trigger`, `goal_kind`, `chosen_direction`,
//!   `position`, `scorer_terms`, and `masked` are top-level keys. `posture` /
//!   `goal_kind` serialize snake_case (`"pressured"`, `"direction"`);
//!   `trigger` variants are unit-only and serialize as bare PascalCase
//!   strings (`"PressuredEnter"`) — same convention as unit
//!   `RejectionReason` variants, so `jq -r .trigger` needs no object
//!   unwrapping. `previous_posture` is present only on posture transitions;
//!   `scorer_terms` and `masked` are present only when the position scorer
//!   ran. `masked` is a u16 candidate bitmask (`0xFFFF` = all-masked frame).
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
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
    MovementDecision,
}

/// Payload varies by `kind`. Flattened into the parent event JSON object.
///
/// Untagged: serde distinguishes variants structurally, trying them in
/// declaration order on deserialize (declaration order has NO effect on
/// serialization). `Pet` is declared before `Ability` because Pet's shape is
/// a strict superset of Ability's (`candidates` + `outcome` plus
/// `owner`/`pet_type`) — superset-first ordering makes deserialization pick
/// the right variant. `Movement` sits last and is disambiguated by its
/// REQUIRED `posture` field (no other variant has one) while lacking the
/// `candidates` field every other variant requires — keep both properties
/// intact when evolving the shapes.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum EventPayload {
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
    /// One `movement_decision` event. Emitted by healer posture AI on posture
    /// transitions and committed direction changes ONLY — never per-tick
    /// (emitters land in the healer-movement plan's U6-U8; until then this
    /// variant exists but no events carry it).
    Movement {
        /// Posture in effect AFTER this decision. REQUIRED — this field is
        /// the structural discriminator that keeps the untagged payload
        /// unambiguous against the Ability/Pet shapes.
        posture: Posture,
        /// Posture before the decision. Present only on posture transitions;
        /// omitted for within-posture committed direction changes.
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_posture: Option<Posture>,
        /// Why the decision fired (closed enum — see `MovementTrigger`).
        trigger: MovementTrigger,
        /// Shape of the movement goal the directive carries.
        goal_kind: MovementGoalKind,
        /// Unit XZ direction chosen by the position scorer, when the goal is
        /// directional. Omitted for point/entity goals.
        #[serde(skip_serializing_if = "Option::is_none")]
        chosen_direction: Option<[f32; 2]>,
        /// Actor world position at decision time. Duplicated from
        /// `actor.position` so coarse trace-side movement KPIs (path
        /// sketches, separation estimates) can read one field.
        position: [f32; 3],
        /// Optional per-term score breakdown from the position scorer
        /// (term name → score for the winning candidate). BTreeMap so
        /// serialization order is deterministic (trace byte-identity at a
        /// fixed seed). Cow keys: term names are compile-time constants;
        /// avoid per-event allocation.
        #[serde(skip_serializing_if = "Option::is_none")]
        scorer_terms: Option<BTreeMap<Cow<'static, str>, f32>>,
        /// Optional bitmask over the 16 compass candidates: bit `i` set when
        /// candidate `i` was eliminated by the hard-constraint mask pass
        /// (boundary or ally-anchor). `Some(0xFFFF)` marks an all-masked frame
        /// — the only legitimate source of Part A behavior divergence from the
        /// pre-mask penalty scheme, so R6 byte-identity attribution greps this
        /// field. Present only when the scorer ran.
        #[serde(skip_serializing_if = "Option::is_none")]
        masked: Option<u16>,
    },
}

/// Movement posture: the healer FREE/PRESSURED/ESCAPE/DIP machine plus the
/// Mage ENGAGE/KITE machine. Serializes snake_case (`"free"`, `"kite"`, ...)
/// to match the `kind`/`status` convention.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    Free,
    Pressured,
    Escape,
    Dip,
    Engage,
    Kite,
}

/// The gameplay-side postures convert losslessly into the unified trace enum
/// (the wire format keeps all variants; gameplay splits healer vs DPS so the
/// two state machines can't share variants). Conversion lives here — events.rs
/// already depends on `components` — so the gameplay components stay free of
/// trace-schema concerns.
impl From<crate::states::play_match::components::Posture> for Posture {
    fn from(p: crate::states::play_match::components::Posture) -> Self {
        use crate::states::play_match::components::Posture as GamePosture;
        match p {
            GamePosture::Free => Posture::Free,
            GamePosture::Pressured => Posture::Pressured,
            GamePosture::Escape => Posture::Escape,
            GamePosture::Dip => Posture::Dip,
        }
    }
}

impl From<crate::states::play_match::components::DpsPosture> for Posture {
    fn from(p: crate::states::play_match::components::DpsPosture) -> Self {
        use crate::states::play_match::components::DpsPosture as GameDps;
        match p {
            GameDps::Engage => Posture::Engage,
            GameDps::Kite => Posture::Kite,
        }
    }
}

/// Closed set of causes for a `movement_decision` event. Unit-only variants
/// serialize as bare PascalCase strings (same convention as unit
/// `RejectionReason` variants), so `jq -r .trigger` works without the
/// object-unwrapping idiom.
///
/// Adding a new variant requires updating
/// `tests/decision_trace_audit.rs::EXPECTED_MOVEMENT_TRIGGERS` (the
/// surprise-only audit fails on any emitted variant missing from that list).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementTrigger {
    /// FREE/DIP → PRESSURED: targeted by a visible enemy AND a proximity /
    /// intent condition holds (danger radius, melee/pet threat, or closing).
    PressuredEnter,
    /// PRESSURED → FREE: the compound trigger no longer holds (post-hysteresis).
    PressuredExit,
    /// PRESSURED → ESCAPE: all proximate threats are movement-impaired
    /// (Root/Stun/Incapacitate — not Fear); window converted to separation.
    EscapeWindowOpen,
    /// ESCAPE → PRESSURED or FREE: window duration elapsed (or threats freed).
    EscapeWindowClosed,
    /// FREE → DIP (Paladin): HoJ ready, teammate stable, enemy healer reachable.
    DipEnter,
    /// DIP → FREE via an abort condition (teammate HP dive, target
    /// dead/immune/DR-immune, budget exceeded). Self-focus preemption is
    /// `PressuredEnter` (DIP → PRESSURED), not an abort.
    DipAbort,
    /// DIP → FREE: HoJ cast landed; returning to team.
    DipComplete,
    /// Committed direction window expired and re-evaluation chose a new
    /// direction within the SAME posture (no transition).
    CommitExpired,
    /// FREE formation goal moved enough to re-commit (engaged-ally centroid
    /// shifted) within the same posture.
    FormationShift,
    /// Mage ENGAGE → KITE: a melee-range threat now carries the Mage's own
    /// root/slow aura (the kiting window opened).
    KiteEnter,
    /// Mage KITE → ENGAGE: no visible enemy carries a Mage-owned root/slow
    /// aura and the hysteresis hold has elapsed (the kiting window closed).
    KiteExit,
}

/// Shape of the movement goal carried by the directive this decision issued.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MovementGoalKind {
    /// Scored unit direction (PRESSURED repositioning, ESCAPE separation).
    Direction,
    /// Fixed world point (FREE formation anchor).
    Point,
    /// Pursue an entity (DIP target chase).
    Entity,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AbilityCandidate {
    pub ability: AbilityType,
    pub status: CandidateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<RejectionReason>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TargetCandidate {
    pub entity_id: u32,
    pub class: CharacterClass,
    pub score: i32,
    pub status: CandidateStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<TargetRejectionReason>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Chosen,
    Rejected,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Serialize, Deserialize, Clone, Debug)]
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TargetRejectionReason {
    OutOfRange { distance: f32, max: f32 },
    Stealthed,
    Dead,
    Immune,
    CCd { cc_type: AuraType },
    LowerScoreThanChosen { score: i32, chosen_score: i32 },
    KillTargetOverride,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Rage,
    Energy,
    ComboPoints,
    Mana,
}
