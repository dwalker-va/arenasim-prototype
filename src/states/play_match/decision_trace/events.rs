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
use crate::states::play_match::class_ai::pillar_stance::{StanceReason, StanceWindow};
use crate::states::play_match::class_ai::target_focus::Focus;
use crate::states::play_match::components::{AuraType, DRCategory};
use crate::states::play_match::situation::{
    MatchPhase, Play, TeamSituation, Tempo, Window, YieldRule,
};

/// One AI decision event. Serializes to a single JSONL line.
#[derive(Serialize, Clone, Debug)]
pub struct DecisionEvent {
    pub frame: u64,
    pub sim_time: f32,
    pub seed: u64,
    /// Which AI implementation produced this decision ("Legacy" / "TeamPlan").
    ///
    /// Carried per-event, like `seed`, so a single JSONL line grepped out of
    /// context is still interpretable — without it, two traces from the same seed
    /// under different profiles are indistinguishable, and any conclusion drawn
    /// from a mixed corpus is unattributable.
    pub ai_profile: &'static str,
    pub kind: EventKind,
    /// The unit that decided. Every kind carries one except
    /// `situation_change`, which is about a TEAM (its `team` field) and omits
    /// it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<ActorView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetView>,
    /// The situation the deciding unit's team was in (`situation.rs`), on
    /// every `ability_decision` a class AI makes. Absent on the other kinds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub situation: Option<SituationTag>,
    #[serde(flatten)]
    pub payload: EventPayload,
}

/// The compact `situation` field of an `ability_decision`: the match phase and
/// the team's tempo, always, and the `enemy_healer_locked`, `ally_in_danger`
/// and `kill_window` windows while one is open.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SituationTag {
    pub phase: MatchPhase,
    pub tempo: Tempo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enemy_healer_locked: Option<WindowTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ally_in_danger: Option<WindowTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kill_window: Option<WindowTag>,
    /// The lock a chain hands off from (`TeamSituation::chain_window`):
    /// present only while the team runs the Chain play and the lock ends
    /// within its horizon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_ending: Option<WindowTag>,
    /// The pillar the team takes together (`TeamSituation::pillar_stance`,
    /// AS-385): present only while the team holds a stance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pillar_stance: Option<StanceTag>,
    /// The team's target focus (`TeamSituation::target_focus`, AS-389):
    /// present only while one is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_focus: Option<FocusTag>,
    /// The deciding unit's play, when it is not `Default`
    /// (`situation::Play`): `{"name": "burst"}` or
    /// `{"name": "peel", "ally": <entity id>}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play: Option<PlayTag>,
    /// The play's own step produced the choice — `"peel"` or `"burst"` when
    /// the decision's chosen ability came from that play's step at the head of
    /// the chain, not from the chain below it. Absent otherwise: a non-default play whose
    /// step did not fire recorded what its fallback chain chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_step: Option<PlayStep>,
}

/// A non-default `situation::Play`, as the trace records it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum PlayTag {
    Burst,
    Peel { ally: u32 },
    Hunt,
    Pillar,
    Chain { subject: u32 },
}

impl PlayTag {
    /// The tag for `play`; `None` for `Play::Default`.
    pub fn of(play: Play) -> Option<Self> {
        match play {
            Play::Default => None,
            Play::Burst => Some(PlayTag::Burst),
            Play::Peel { ally } => Some(PlayTag::Peel { ally: ally.index() }),
            Play::Hunt => Some(PlayTag::Hunt),
            Play::Pillar => Some(PlayTag::Pillar),
            Play::Chain { subject } => Some(PlayTag::Chain {
                subject: subject.index(),
            }),
        }
    }
}

/// A play's own step, when it produced the choice (`SituationTag::play_step`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayStep {
    /// The Peel play's crowd control on an attacker of the teammate in danger.
    Peel,
    /// The Burst play's hard hitter on the kill window's subject.
    Burst,
    /// The Chain play's hand-off: crowd control on the locked enemy (any
    /// enemy the team holds), landing in its lock's last moments.
    Chain,
}

/// A situation window as the trace records it: the subject's entity id and
/// the window's onset and expected end on the `sim_time` clock.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct WindowTag {
    pub subject: u32,
    pub since: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<f32>,
}

impl From<&Window> for WindowTag {
    fn from(window: &Window) -> Self {
        Self {
            subject: window.subject.index(),
            since: window.since,
            until: window.until,
        }
    }
}

/// A team's pillar stance as the trace records it: the healer whose ring it is
/// judged from, the pillar's index, the side (a ring angle, radians) and the
/// reason, and when it began on the `sim_time` clock.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct StanceTag {
    pub healer: u32,
    pub pillar: usize,
    pub side: f32,
    pub reason: StanceReason,
    pub since: f32,
}

impl From<&StanceWindow> for StanceTag {
    fn from(s: &StanceWindow) -> Self {
        Self {
            healer: s.healer.index(),
            pillar: s.pillar,
            side: s.side,
            reason: s.reason,
            since: s.since,
        }
    }
}

/// A team's target focus as the trace records it: the enemy it shoots, the
/// target it moved off, and when.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct FocusTag {
    pub subject: u32,
    pub from: u32,
    pub since: f32,
}

impl From<&Focus> for FocusTag {
    fn from(f: &Focus) -> Self {
        Self {
            subject: f.subject.index(),
            from: f.from.index(),
            since: f.since,
        }
    }
}

impl From<&TeamSituation> for SituationTag {
    fn from(situation: &TeamSituation) -> Self {
        Self {
            phase: situation.phase,
            tempo: situation.tempo,
            enemy_healer_locked: situation.enemy_healer_locked.as_ref().map(WindowTag::from),
            ally_in_danger: situation.ally_in_danger.as_ref().map(WindowTag::from),
            kill_window: situation.kill_window.as_ref().map(WindowTag::from),
            lock_ending: situation.chain_window().as_ref().map(WindowTag::from),
            pillar_stance: situation.pillar_stance.as_ref().map(StanceTag::from),
            target_focus: situation.target_focus.as_ref().map(FocusTag::from),
            play: None,
            play_step: None,
        }
    }
}

/// Which situation a `situation_change` event is about.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SituationName {
    EnemyHealerLocked,
    AllyInDanger,
    KillWindow,
    /// A team takes a pillar together (`TeamSituation::pillar_stance`,
    /// AS-385). Its `subject` is the stance's healer; `pillar` and `reason`
    /// ride along.
    PillarStance,
    /// A team hunts a lone enemy healer at a pillar (`TeamSituation::hunt`,
    /// AS-394). Its `subject` is the healer; `pillar` rides along.
    Hunt,
    Tempo,
    Phase,
    /// A DPS began, ended or changed the rule of its healer-sight yield
    /// (`TeamSituation::sight_yield`, AS-386).
    SightYield,
    /// The team's ranged DPS moved their focus off a target they could not
    /// see (`TeamSituation::target_focus`, AS-389). Its `subject` is the new
    /// focus, `from` the target it moved off.
    TargetFocus,
}

/// What happened to the situation. Adding a variant requires updating
/// `tests/decision_trace_audit.rs::EXPECTED_SITUATION_CHANGES`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SituationChangeKind {
    /// A window opened — or moved to a new subject, which is a new window.
    Onset,
    /// A window closed.
    End,
    /// A new CC pushed an open window's `until` later.
    Extended,
    /// The team's tempo or the match phase moved (`tempo` / `previous_tempo`,
    /// or `phase` / `previous_phase`, carry both ends), or a DPS still
    /// yielding its healer's sight does so under a new `rule`
    /// (`sight_yield`).
    Shift,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AbilityDecision,
    TargetAcquisition,
    PetDecision,
    MovementDecision,
    /// A team's situation changed (`situation.rs`). Transitions only.
    SituationChange,
    /// A melee attack was dodged or parried (`combat_core::avoidance`). The
    /// `actor` is the attacker, the `target` the defender.
    MeleeAvoided,
    /// The match's header: the first line of a trace whose match has a talent
    /// build, carrying every slot's build. No actor. A match with no talents
    /// writes none, so its trace is the trace it always was.
    MatchHeader,
    /// A wearer's AI considered an on-use item (`class_ai::on_use`): used it,
    /// or held it with a reason. Emitted only for a wearer of one.
    ItemDecision,
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
        /// Optional LoS-only subset of `masked`: bit `i` set when candidate `i`
        /// was eliminated specifically by the obstacle mask (`MASK_LOS` — a step
        /// that walks into a wall). A strict subset of `masked`, surfaced
        /// separately so consumers can attribute obstacle-driven divergence
        /// without decoding the combined mask. Emitted only when nonzero, so
        /// obstacle-free (BasicArena) traces never carry it and stay
        /// byte-identical to pre-LoS traces.
        #[serde(skip_serializing_if = "Option::is_none")]
        los_masked: Option<u16>,
    },
    /// One `situation_change` event: a team's situation changed. Disambiguated
    /// by its REQUIRED `name` + `change` fields, which no other variant has,
    /// while it lacks the `candidates` / `posture` the others require.
    SituationChange {
        team: u8,
        name: SituationName,
        change: SituationChangeKind,
        /// The window's subject (`enemy_healer_locked`: the healer;
        /// `ally_in_danger`: the teammate; `kill_window`: the enemy).
        #[serde(skip_serializing_if = "Option::is_none")]
        subject: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        since: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        until: Option<f32>,
        /// `tempo` shifts: the tempo after the change, and before it.
        #[serde(skip_serializing_if = "Option::is_none")]
        tempo: Option<Tempo>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_tempo: Option<Tempo>,
        /// `phase` shifts: the phase after the change, and before it.
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<MatchPhase>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_phase: Option<MatchPhase>,
        /// `sight_yield` changes: the pair's healer (its DPS is `subject`)
        /// and the rule that now leads the pair.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        healer: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<YieldRule>,
        /// `pillar_stance` changes: the stance's pillar and reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pillar: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<StanceReason>,
        /// `target_focus` changes: the target the focus moved off.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<u32>,
    },
    /// One `melee_avoided` event: the attack the defender avoided (named as
    /// the combat log names a landed one) and how. Disambiguated by its
    /// REQUIRED `avoided` field, which no other variant has, while it lacks
    /// the `candidates` / `posture` / `name` the others require.
    MeleeAvoided {
        attack: String,
        avoided: crate::states::play_match::combat_core::Avoidance,
    },
    /// The `match_header` line: each team's builds, one per slot, each a
    /// talent-to-rank map (`{}` for a slot with none). Disambiguated by its
    /// REQUIRED `team1_builds` / `team2_builds`, which no other variant has.
    Header {
        team1_builds: Vec<crate::states::play_match::talents::Build>,
        team2_builds: Vec<crate::states::play_match::talents::Build>,
    },
    /// One `item_decision` event: the on-use item considered, whether it was
    /// used (`chosen`) or held (`rejected`, with its reason). Disambiguated by
    /// its REQUIRED `item` field, which no other variant has, while it lacks
    /// the `candidates` / `posture` / `name` / `avoided` / builds the others
    /// require.
    Item {
        item: crate::states::play_match::equipment::ItemId,
        status: CandidateStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<RejectionReason>,
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
    /// DPS-kiter ENGAGE → KITE: the entry condition opened (Mage: a melee enemy
    /// carries the Mage's own root/slow; Hunter: a melee-DPS threat is within
    /// closing range).
    KiteEnter,
    /// DPS-kiter KITE → ENGAGE: the sustain condition lapsed and the hysteresis
    /// hold has elapsed (Mage: no Mage-owned root/slow on a visible enemy;
    /// Hunter: no melee-DPS threat within the sustain radius).
    KiteExit,
    /// DPS-kiter ENGAGE seek-LoS: the kiter is idle in shot range of the
    /// kill target but OCCLUDED from it, so it ran the scorer (los_seek term) to
    /// reposition to a sighted angle instead of stalling behind cover. Emitted
    /// within the ENGAGE posture (no transition), so no `previous_posture`.
    /// Obstacle maps only.
    SeekLos,
    /// Melee tempo reset (Warrior): a CC'd melee with its gap closer down
    /// and out of melee range fell back toward its healer for a bounded window
    /// instead of face-chasing. Emitted on the activation edge only.
    MeleeReset,
    /// Healer dispel walk (Paladin, Priest): a teammate is held in crowd
    /// control the healer's dispel removes at the urgent bar, beyond the
    /// dispel's range or out of sight, so the healer walks toward it (a `point`
    /// goal, the ally in the target view) until the rotation's urgent dispel
    /// can reach. Emitted within the posture (no transition), once per commit
    /// window.
    DispelChase,
    /// The `Legacy` dispel walk reached its teammate with the dispel still
    /// owed (the GCD still running): the healer holds the point it reached (a
    /// `point` goal, the teammate in the target view) until the dispel lands or
    /// stops being owed, instead of handing movement back to a posture that can
    /// step it back out of range. Emitted once, on reaching.
    DispelHold,
    /// Stealthed Rogue (AS-185): a lit enemy Flare lies across its way to its
    /// target and a way round stays out of the light, so it walks that way (a
    /// `point` goal). Announced once per Flare, not per re-steer.
    FlareSkirt,
    /// Stealthed Rogue: every way to its target is lit, so it holds outside
    /// the light (a `point` goal) for the Flare to burn out.
    FlareWait,
    /// Stealthed Rogue: it was holding outside a Flare and goes in anyway —
    /// a teammate fell below half health, or the Flare was relit over its
    /// approach (an `entity` goal: plain pursuit of its target).
    FlareCommit,
    /// → ESCAPE (Druid): the healer shifted into Travel Form, which broke its
    /// roots and snares; it runs from every melee or pet CHASER inside the
    /// intent radius (the form outruns legs, not spells) for as long as it
    /// stays shifted, re-committing its direction each commit window (traced
    /// as `CommitExpired`).
    ShiftEscape,
    /// `TeamPlan`'s Hunt play (AS-394): a unit walks to its station round
    /// the pillar a lone enemy healer hides at (a `point` goal). Fires when
    /// the play takes the unit's movement and whenever its station moves.
    Hunt,
    /// `TeamPlan` (AS-362): the healer's goal was chosen to take it out of
    /// the reach of an enemy crowd control ready to land on it — the team
    /// solve's `C_CC_REACH` (PRESSURED), the Paladin's band-hold stepping to
    /// a disc's edge, or the FREE formation point's reach repair. Fires once
    /// as each such walk begins, not per re-solve.
    OutOfCcReach,
    /// `TeamPlan` (AS-373, `class_ai::pillar_mode`): the healer starts to HUG
    /// a pillar — its team holds a pillar stance on it (AS-385,
    /// `situation_change` `pillar_stance`) and the pillar's ring has ground
    /// that sees every teammate in heal range out of the crowd control the
    /// stance avoids — or moves its hug to the stance's new pillar. The scorer
    /// terms name the pillar (`pillar`) and the two keys (`key_stance`,
    /// `key_arc`) as they stood.
    PillarModeEnter,
    /// `TeamPlan` (AS-373): the healer stops hugging and plays the open. The
    /// scorer terms name the pillar it left and the keys as they stood.
    PillarModeLeave,
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
    OutOfRange {
        distance: f32,
        max: f32,
    },
    WithinDeadZone {
        distance: f32,
        min: f32,
    },
    OnCooldown {
        remaining: f32,
    },
    InsufficientMana {
        have: f32,
        need: f32,
    },
    InsufficientResource {
        resource: ResourceKind,
        have: f32,
        need: f32,
    },
    SilencedOrLocked {
        school: SpellSchool,
    },
    TargetImmune,
    TargetAlreadyCCd {
        cc_type: AuraType,
    },
    DRImmune {
        category: DRCategory,
    },
    FriendlyBreakableCC,
    SelfIncapacitated,
    Rooted,
    LowerPriorityThanChosen {
        chosen: AbilityType,
    },
    AlreadyApplied,
    NoValidTarget,
    PreconditionUnmet {
        note: String,
    },
    /// Pet is below the Heel HP threshold (25%). Emitted from pet_ai_system
    /// when the pet retreats to the owner's flank and suppresses ability
    /// execution. Hunter-dispatched PetCommands targeting this pet are
    /// despawned without execution under the same flag.
    LowHealthHeel,
    /// A targeted cast (hostile or friendly) was rejected because an obstacle
    /// volume blocks the straight line from the caster to the target. Checked
    /// at cast start in `pre_cast_ok`, AFTER `can_cast_config`, so range / mana
    /// / stealth take precedence (an out-of-range target reports `OutOfRange`,
    /// not this). Adding this variant requires updating
    /// `tests/decision_trace_audit.rs::EXPECTED_REJECTION_REASONS`.
    LosBlocked,
    /// An interrupt (Kick, Pummel, Wind Shear, Spell Lock) passed over an
    /// enemy mid-cast because the cast is `uninterruptible` (Aimed Shot):
    /// no interrupt takes it, so the interrupt is not spent on it.
    Uninterruptible {
        /// The cast it passed over.
        ability: AbilityType,
    },
    /// Under the Burst play, a cast on the kill window's subject that would
    /// land after the window's `until` (the cast gate's fit,
    /// `cast_guard::fits_window`): its cast time, its flight and — for damage
    /// over time alone — its first tick, from now.
    OutlastsWindow {
        /// When it would land, on the trace's `sim_time` clock.
        lands_at: f32,
        /// When the window ends.
        until: f32,
    },
    /// Under the Chain play, a hand-off that would not land in the lock's
    /// last moments (`class_ai::chain`): before `until - HANDOFF_LEAD`, which
    /// spends the new lock alongside the old one, or after `until`, which
    /// leaves the subject free in between.
    MissesHandoff {
        /// When it would land, on the trace's `sim_time` clock.
        lands_at: f32,
        /// When the lock it would chain onto ends.
        until: f32,
    },
    /// Life Tap held (`class_ai::warlock::life_tap_hold`): the Warlock does not
    /// need the mana, or the tap would leave it dying.
    TapHeld {
        hold: TapHold,
    },
}

/// Why a Warlock did not Life Tap ([`RejectionReason::TapHeld`]), in the
/// order the gates are asked.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum TapHold {
    /// Not mana-starved: `mana` is at or above `starved_below`.
    NotStarved { mana: f32, starved_below: f32 },
    /// The tap would leave the Warlock dying: `health_after` below `floor`.
    DeathRange { health_after: f32, floor: f32 },
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
