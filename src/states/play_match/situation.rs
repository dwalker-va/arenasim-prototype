//! Situations: the team-level read model the class AI decides from.
//!
//! A *situation* is a named, discrete fact about the fight that more than one
//! decision cares about — "the enemy healer is locked", "we are pressing". The
//! class AI used to recompute each one ad hoc at every site that read it; here
//! it is classified ONCE, from the same snapshot every unit decides from,
//! named in the decision trace, and handed to the class AI through
//! [`CombatContext::situation`](super::class_ai::CombatContext::situation).
//! The design is `docs/design/proposals/2026-10-11-ai-architecture.md` §4.2.
//!
//! It models these situations:
//!
//! - **`enemy_healer_locked`** — the enemy healer cannot cast a heal: it is
//!   under a CAST-PREVENTING crowd control ([`locks_healer`]). Its
//!   [`Window`] names the healer, when the lock began and when the longest
//!   such CC on it runs out.
//! - **`tempo`** — whether the team is [`Tempo::Pressing`] its health lead,
//!   [`Tempo::Trailing`], or [`Tempo::Even`]. It is latched with onset and
//!   release thresholds: a team starts pressing (or trailing) when its lead
//!   (or deficit) reaches `press_advantage_margin`, and keeps that tempo until
//!   it falls below `press_release_margin`, so a lead wobbling around the
//!   onset does not flip it with every hit and heal. The band is a behaviour
//!   change, so it applies only to a team under `AiProfile::TeamPlan`
//!   ([`SituationConfig::under`]); a `Legacy` team releases at the onset, as
//!   it always has.
//! - **`phase`** — where the match is in its life: [`MatchPhase::Prematch`]
//!   behind the gates, [`MatchPhase::Engaged`], then
//!   [`MatchPhase::Attrition`] once arena dampening reaches half
//!   ([`ATTRITION_ONSET_SECS`]). The clock under it, `since_gates`, rides
//!   along: the Druid's mana governor spends its reserve down toward the
//!   Attrition onset ([`TeamSituation::attrition_remaining`]). The design's
//!   pre-contact *Opener* phase is not modelled: nothing reads it yet, and
//!   contact is the `TeamPlan` layer's to define.
//! - **`ally_in_danger`** — a teammate both DYING (below
//!   `urgency_hp_threshold`) and PRESSURED (an enemy it can see is on it, near
//!   enough that intent matters). Its [`Window`] names the most injured such
//!   teammate. It is debounced at both ends. A teammate pressured only from
//!   the edge of the pressure test (within `danger_edge_margin` of a radius or
//!   a closing enemy's preferred range) opens a window once it has been in
//!   danger for `danger_dwell`; one pressured firmly opens it at once. A window
//!   ends once its teammate has been out of danger for `danger_release` (or
//!   dies). So an enemy crossing a pressure radius for a frame neither opens a
//!   window nor splits one in two, and a Warrior on a dying teammate is
//!   answered on the first frame. Nothing outside
//!   the `TeamPlan`-gated Peel play reads the window, so the debounce applies
//!   under both profiles. The two facts it joins are kept per teammate in
//!   [`TeamSituation::allies`], because the healers' rules read them apart:
//!   "a teammate is dying" (cover denial, the dispel walk's dying-first rule)
//!   and "I am pressured" (the posture machine's PRESSURED trigger). The
//!   enemies doing the pressuring ride along as
//!   [`TeamSituation::danger_attackers`]: the attackers a peel answers.
//! - **`kill_window`** — an enemy is LOW (below `kill_hp_threshold`, held
//!   until it is back above `kill_hp_release`) and nothing on its side can
//!   heal it: every living enemy healer is locked ([`locks_healer`]), out of
//!   mana (below `kill_oom_mana`), or cannot reach it (beyond `heal_range`
//!   or out of sight). A team with no living healer can heal nothing. Its
//!   [`Window`] names the most injured such enemy; its `until` is when the
//!   first healer that is only LOCKED comes free — `None` when no healer's
//!   return is known (out of mana, out of reach, or none at all).
//! - **`hunt`** — the enemy is down to a lone healer hiding at a pillar,
//!   and the team still has two or more units (AS-394, `class_ai::hunt`):
//!   `TeamPlan` only, on a team that runs the Hunt play. Its
//!   [`HuntWindow`](super::class_ai::hunt::HuntWindow) names the healer, the
//!   pillar, the axis (the healer's bearing at onset) and the two stations
//!   half a turn apart round it, with who holds each. Schmitt pair on the
//!   healer's distance from the pillar's skin; the onset needs the healer
//!   hidden from one of the team.
//! - **`pillar_stance`** — the team takes a pillar together (AS-385,
//!   `class_ai::pillar_stance`): `TeamPlan` only, on a team that runs the
//!   Pillar play, on a map with obstacles. It enters for `AvoidCc` when a
//!   crowd control a pillar defends against is ready for a teammate, the
//!   fight stands at a pillar, the pillar's ring has a TEAM arc (ground from
//!   which the healer sees every teammate, out of that crowd control's
//!   reach), and no kill is on. Its [`StanceWindow`] names the healer, the
//!   pillar, the side and the reason. Schmitt pair, release and dwell as the
//!   AS-373 hug's; the arc and the kill are feasibility and end it at once.
//! - **`lock_ending`** — of the visible enemies the team holds
//!   ([`locks_healer`]'s set: a stun, a fear or horror, a polymorph, an
//!   incapacitate, a cyclone), the one whose hold ends SOONEST, the healer
//!   on a tie. Its `until` is when the longest hold on that enemy runs out.
//!   The lock the Chain play hands off from; nothing else reads it, and it
//!   is traced only under that play (`situation.lock_ending`).
//! - **`sight_yield`** — per DPS–healer pair, which of the two must not break
//!   the pair's line of sight ([`SightPair`], AS-386; the design is
//!   `docs/design/2026-10-24-team-sight-cohesion.md` §3.1). The first rule
//!   that holds decides ([`YieldRule`]): a unit that cannot move leads; a
//!   pressured healer leads; the team's pillar stance leads; a hurt DPS
//!   (below [`SIGHT_YIELD_HP`], held until [`SIGHT_YIELD_RELEASE_HP`])
//!   yields; otherwise the DPS leads and the healer follows. A yield, once
//!   set, holds [`SIGHT_YIELD_DWELL`] unless rule 1 ends it. `TeamPlan` only
//!   ([`SituationConfig::sight_yield`]); a DPS's movement reads it (the kiter
//!   scorer's anchor mask, `CombatContext::sight_yield_healer`, and the
//!   pursuit's held tangent side, `combat_core::movement`).
//! - **`target_focus`** — the enemy the team's ranged DPS shoot, moved off a
//!   target one of them has not seen for `target_focus::FOCUS_HIDDEN_SECS`
//!   onto the nearest enemy that DPS can reach and see (AS-389,
//!   `class_ai::target_focus`). A focus holds `FOCUS_DWELL`; it returns to
//!   its old target only by the same rule. `TeamPlan` only
//!   ([`SituationConfig::target_focus`]), on a map with obstacles; target
//!   acquisition reads it (`combat_ai::acquire_targets`), one frame on.
//!
//! ## Plays
//!
//! A [`Play`] is what a unit does about the situation: a named reordering of
//! its own priority chain ([`TeamSituation::play_for`]). Each play is a STAGE
//! of one pipeline, `class_ai::plays::PLAY_ORDER` — Peel, Hunt, Pillar,
//! Chain, Burst — and a unit runs the first stage its team runs, the
//! situation calls, and the unit admits; a stage that does not apply passes
//! it to the next (the walk is `class_ai::plays`'s). Every play runs only
//! under `AiProfile::TeamPlan` ([`SituationConfig::under`]); `Legacy` runs
//! none. A NEW play is GATED until a milestone sweep graduates or deletes it
//! (the architecture's §4.5): [`WithheldPlays`] can deny a gated play to one
//! team — the paired control its probe and its head-to-head measure it
//! against. A GRADUATED play ([`PlayId::is_gated`] false; Peel, Hunt, Chain
//! and Burst, at the 0.8 sweep) is permanent under `TeamPlan` and cannot be
//! withheld. Every play's exceptions table can still be replaced for one
//! team, which is how a milestone sweep re-checks an exception.
//!
//! - **`Peel`** — a teammate is in danger, so a unit other than that teammate
//!   puts its PEEL step at the head of its chain: crowd control on one of the
//!   teammate's [`danger_attackers`](TeamSituation::danger_attackers)
//!   (`class_ai::peel`), except in the contexts `peel::PEEL_GATES` names.
//! - **`Hunt`** — the team hunts a lone enemy healer at a pillar and the
//!   unit holds one of the hunt's two stations, so it walks to its station
//!   and holds it (`class_ai::hunt`). A MOVEMENT play: the chain is the
//!   ordinary one. Not called while the hunted healer is locked.
//! - **`Pillar`** — the team holds a `pillar_stance`, so its healer hugs the
//!   stance's pillar on the team arc (`class_ai::pillar_mode`) and its kiting
//!   ranged DPS bend toward the stance's side (`class_ai::pillar_stance`). A
//!   MOVEMENT play: the chain is the ordinary one. A melee is not called: its
//!   half of the stance is the yield (AS-386), read from the situation.
//! - **`Chain`** — a lock the team holds is about to end (`lock_ending`
//!   within [`CHAIN_HORIZON_SECS`]), so a unit with a hand-off puts its
//!   CHAIN step between a peel and a burst: its own crowd control on the
//!   locked enemy, landing in the lock's last moments, or its global
//!   cooldown held for that moment (`class_ai::chain`). A healer does not
//!   chain while a teammate of its own is dying. Not run by the classes
//!   `chain::CHAIN_EXCEPTIONS` names.
//! - **`Burst`** — a kill window is open, so a DPS puts its BURST step at the
//!   head of its chain: its hard hitters on the window's subject
//!   (`class_ai::burst`), each cast only if it lands before the window's
//!   `until` (the cast gate's fit, `class_ai::cast_guard`). Not run by the
//!   classes `burst::BURST_EXCEPTIONS` names.
//!
//! ## Thresholds
//!
//! Every threshold is `movement.ron`'s `shared:` block, read into
//! [`SituationConfig`] — the values the ported sites read before they were
//! ported, so the port changed none of them:
//!
//! | fact | threshold | sites that read it |
//! |---|---|---|
//! | dying | `health_pct < urgency_hp_threshold` | healer cover denial (`teammate_needs_saving`), the dispel walk's dying-first rule (`another_teammate_dying`) |
//! | pressured | `danger_radius`, `threat_intent_radius`; an enemy healer never closing (`TeamPlan` only) | the healers' PRESSURED trigger (its hold) |
//! | pressure onset | the same, a closing threat `danger_edge_margin` inside both its edges | the healers' PRESSURED onset |
//! | tempo | onset `press_advantage_margin`, release `press_release_margin` (`TeamPlan` only) | healer cover denial, the Warrior tempo reset |
//! | danger window | dwell `danger_dwell` at a `danger_edge_margin` edge, release `danger_release` | the Peel play (`TeamPlan` only) |
//! | kill window | low below `kill_hp_threshold`, released above `kill_hp_release`; a healer out of mana below `kill_oom_mana`, out of reach beyond `heal_range` | the Burst play (`TeamPlan` only) |
//! | Attrition | dampening at 50% | the Druid's mana governor |
//!
//! Neighbours that look alike were deliberately NOT folded in, because doing
//! so would change behaviour: `LOW_HP_THRESHOLD` (a constant, also 0.5 today,
//! behind the root-dispel bar and the Paladin's while-CC Divine Shield) would
//! start following a retune of `urgency_hp_threshold`; the Druid's own
//! `DRUID_EMERGENCY_HP` (0.45) and `DRUID_URGENT_HP` (0.6) are different
//! numbers; and the Priest's `attack_prevented_by_cc` leaves Cyclone out of
//! the set [`locks_healer`] names. Unifying any of them is a behaviour change
//! with its own card.
//!
//! ## Where it is classified, and why there
//!
//! [`update_situations`] runs inside `decide_abilities`, on the snapshot that
//! function builds, and again after every same-frame instant CC that snapshot
//! reflects — not as a Phase 1 system. Both are load-bearing for byte-identity:
//! `process_aura_breaks` removes damage-broken CC at the START of Phase 2,
//! after any Phase 1 system could have read the auras, and an instant CC landed
//! by an earlier unit in the dispatch (a Kidney Shot on the healer) is visible
//! to every later unit in the same frame. A classification taken anywhere else
//! would disagree with what the decision actually saw.
//!
//! A context built anywhere else (`CombatContext::new` alone: a pet's, a
//! test's) carries NO situation, and reading one from it panics: it has no
//! frame clock and no `movement.ron` thresholds, so its answer would be
//! silently wrong. A test classifies one explicitly
//! (`CombatContext::classified`).
//!
//! ## Two properties that keep it safe
//!
//! - **Pure and RNG-free.** [`classify`] only reads. Adding the layer changes
//!   no output until a site reads it.
//! - **Built from what the team perceives.** The enemy healer is chosen with
//!   the [`stealth_visible`] rule every unit's view applies. Only a Rogue can
//!   stealth, so a healer is visible to its whole enemy team or to none of it,
//!   and one classification serves every member. A teammate's PRESSURE is
//!   judged from that teammate's own view (its own Shadow Sight included),
//!   because an attacker one teammate sees another may not.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::ai_profile::AiProfile;
use super::class_ai::combat_snapshot::CombatSnapshot;
use super::class_ai::pillar_stance::{self, StanceMemory, StanceWindow};
use super::class_ai::plays;
use super::class_ai::target_focus::{self, Focus, TargetSight};
use super::class_ai::{pressing_when_ahead, stealth_visible, team_hp_sums, CombatantInfo};
use super::components::{Aura, AuraType};
use super::constants::{DAMPENING_RAMP_SECS, DAMPENING_START_SECS};
use super::decision_trace::{DecisionTrace, SituationChangeKind, SituationName};
use super::map_geometry::{has_line_of_sight, ObstacleVolume};
use super::movement_config::MovementConfig;
use crate::states::match_config::CharacterClass;

/// When the match enters [`MatchPhase::Attrition`], in seconds after the gates:
/// the moment arena dampening reaches 50%. Heals land at half value or less
/// from here on, so mana saved past it buys little.
pub const ATTRITION_ONSET_SECS: f32 = DAMPENING_START_SECS + DAMPENING_RAMP_SECS * 0.5;

/// Where the match is in its life. Serializes snake_case (`"attrition"`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchPhase {
    /// The gates are closed.
    #[default]
    Prematch,
    /// The gates are open, and dampening has not reached half.
    Engaged,
    /// Dampening has reached half ([`ATTRITION_ONSET_SECS`]).
    Attrition,
}

/// The clocks a classification reads: the trace's `sim_time` for window
/// times, and the arena's gate clock for the phase.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MatchClock {
    /// The decision trace's `sim_time` (window `since` / `until`).
    pub now: f32,
    pub gates_opened: bool,
    /// `ArenaDampening::time_since_gates` (0 until the gates open).
    pub since_gates: f32,
}

impl MatchClock {
    /// `secs` after the gates opened, with the trace clock at the same value
    /// — the clock a scene or a unit test decides at.
    pub fn after_gates(secs: f32) -> Self {
        Self {
            now: secs,
            gates_opened: true,
            since_gates: secs,
        }
    }
}

/// Whether a team is ahead on health, behind, or level — latched between
/// `press_advantage_margin` (onset) and `press_release_margin` (release; see
/// [`SituationConfig::under`]). Serializes snake_case (`"pressing"`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tempo {
    #[default]
    Even,
    /// Own team's summed health fraction reached the onset lead, and has
    /// not fallen below the release lead since.
    Pressing,
    /// The enemy's did.
    Trailing,
}

/// One live window of a situation: who it is about, when it began, and when it
/// is known to end. Times are on the decision trace's `sim_time` clock, so a
/// window joins directly against the events recorded inside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    /// The unit the situation is about (the locked healer; the teammate in
    /// danger).
    pub subject: Entity,
    /// When this window opened. Carried across frames while the subject stays
    /// the same, so it is the ONSET, not the latest frame.
    pub since: f32,
    /// When it is due to end, if that is known. A lock's is the longest
    /// remaining locking CC on the healer, and moves later when a new CC
    /// extends it; a teammate's danger has none.
    pub until: Option<f32>,
}

/// One living, non-pet teammate's standing this frame — the two facts
/// `ally_in_danger` joins, kept apart because rules read them apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AllyStanding {
    pub entity: Entity,
    /// Below `urgency_hp_threshold`.
    pub dying: bool,
    /// An enemy it can see targets it from within `danger_radius` — or from
    /// within `threat_intent_radius` when that enemy is a melee class, a pet,
    /// or a ranged enemy still closing to its preferred range. A distant
    /// caster holding position does not pressure, and neither does a melee
    /// targeting from across the arena. Under `TeamPlan` an enemy healer is
    /// never closing ([`closes`]): it pressures from `danger_radius` only.
    pub pressured: bool,
    /// Pressured with the CLOSING branch given width ([`pressure_onset`]): a
    /// caster or ranged enemy counts as closing only `danger_edge_margin`
    /// inside both of that branch's edges — its own preferred range and
    /// `threat_intent_radius`. The ONSET of a healer's PRESSURED posture reads
    /// this and its hold reads [`Self::pressured`]. The branch is 2yd wide for
    /// a 28yd caster, so a caster pursuing a healer to its preferred range
    /// used to flip the posture on for a frame, the hold then backed the
    /// healer off for `pressured_hold`, and its formation point walked it
    /// straight back in: a ~3s bob across the caster's 30yd ring (AS-249).
    pub pressure_onset: bool,
}

/// Everything one team's units know about the situation this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamSituation {
    pub phase: MatchPhase,
    /// Seconds since the gates opened (0 before): the clock under `phase`.
    pub since_gates: f32,
    pub tempo: Tempo,
    pub enemy_healer_locked: Option<Window>,
    /// The most injured teammate that is both dying and pressured.
    pub ally_in_danger: Option<Window>,
    /// The enemies pressuring `ally_in_danger`'s subject — the ones that make
    /// it pressured ([`AllyStanding::pressured`]), judged from its own view —
    /// nearest to it first (entity order on a tie). Empty while no teammate is
    /// in danger.
    pub danger_attackers: SmallVec<[Entity; 4]>,
    /// The plays this team runs ([`SituationConfig::runs`]): under
    /// `AiProfile::TeamPlan`, every graduated play and every gated one not
    /// withheld from it; under `Legacy`, none.
    pub runs: PlaySet,
    /// Each play's exceptions for this team: the stage's shipped table
    /// (`PlayStage::exceptions`), unless a measurement override replaces it
    /// ([`WithheldPlays::team1_exceptions`]).
    pub exceptions: PlayExceptions,
    /// Every living non-pet teammate (the deciding unit included), in entity
    /// order.
    pub allies: SmallVec<[AllyStanding; 3]>,
    /// Each teammate dying and pressured right now, in entity order, with
    /// when it became so: the raw fact under `ally_in_danger`, before the
    /// window's dwell and release ([`SituationConfig::danger_dwell`]).
    pub in_danger: SmallVec<[(Entity, f32); 3]>,
    /// When the open `ally_in_danger` window's teammate stopped being in
    /// danger, while the window's release runs; `None` while it is in
    /// danger, or no window is open.
    pub danger_cleared_at: Option<f32>,
    /// Whether this is a classification at all. Only the default is not: no
    /// classification has run, so nothing says when a fact began (see
    /// [`ally_in_danger`]).
    pub classified: bool,
    /// The most injured enemy that is low and that no enemy healer can heal
    /// (the module docs' `kill_window`).
    pub kill_window: Option<Window>,
    /// The trace's `sim_time` this was classified at: the clock a window's
    /// `until` is on, which a cast is fitted against.
    pub now: f32,
    /// The enemy whose hold by this team ends soonest — the lock a chain
    /// hands off from (the module docs' `lock_ending`).
    pub lock_ending: Option<Window>,
    /// Whether this team's healers play the pillar modes (AS-373,
    /// [`SituationConfig::pillar_modes`]).
    pub pillar_modes: bool,
    /// Each DPS–healer pair's sight yield: which of the two must not break
    /// the pair's line of sight (the module docs' `sight_yield`). Empty
    /// unless the team runs it ([`SituationConfig::sight_yield`]).
    pub sight_yield: SmallVec<[SightPair; 2]>,
    /// The lone enemy healer the team hunts round its pillar, and who holds
    /// which station (the module docs' `hunt`). Classified only by
    /// [`update_situations`], which has the map's bounds its stations need.
    pub hunt: Option<super::class_ai::hunt::HuntWindow>,
    /// The pillar the team takes together, while it holds one (the module
    /// docs' `pillar_stance`). Classified only by [`update_situations`],
    /// which has the ability data its crowd-control key reads.
    pub pillar_stance: Option<StanceWindow>,
    /// What the stance's keys remember from one classification to the next.
    pub stance_memory: StanceMemory,
    /// The enemy the team's ranged DPS shoot, once one of them has been blind
    /// to its own target for `target_focus::FOCUS_HIDDEN_SECS` (the module
    /// docs' `target_focus`). `None` unless the team runs it
    /// ([`SituationConfig::target_focus`]).
    pub target_focus: Option<Focus>,
    /// Each ranged DPS's sight of its own target, with when it went blind:
    /// the clock under `target_focus`.
    pub target_sight: SmallVec<[TargetSight; 3]>,
}

/// What a unit does about a situation: a named reordering of its own priority
/// chain, nothing more. The class AI keeps its `try_*` steps and gates; a play
/// only says which order they run in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Play {
    /// The class's ordinary priority order.
    Default,
    /// A kill window is open: the BURST step — the class's hard hitters on
    /// the window's subject, each only if it lands before the window's
    /// `until` — heads the chain. Under `AiProfile::TeamPlan` (graduated at
    /// the 0.8 sweep).
    Burst,
    /// `ally` is in danger: the PEEL step — crowd control on one of its
    /// [`danger_attackers`](TeamSituation::danger_attackers) — heads the
    /// chain. When it does not fire, the class's own chain runs below it,
    /// led by a burst step when the Burst stage applies to the unit
    /// (`CombatContext::burst_window`); never by a chain step, which runs
    /// only under the Chain play (`CombatContext::chain_window`). Under
    /// `AiProfile::TeamPlan` (graduated at the 0.8 sweep).
    Peel { ally: Entity },
    /// The team hunts a lone enemy healer hiding at a pillar
    /// ([`hunt`](TeamSituation::hunt)), and the unit holds one of the hunt's
    /// two stations round that pillar (`class_ai::hunt`). A MOVEMENT play —
    /// the chain is the ordinary one. Under `AiProfile::TeamPlan` (graduated
    /// at the 0.8 sweep).
    Hunt,
    /// The team holds a [`pillar_stance`](TeamSituation::pillar_stance): the
    /// healer hugs the stance's pillar on the team arc, and a kiting ranged
    /// DPS bends toward the stance's side (`class_ai::pillar_stance`). A
    /// MOVEMENT play — the chain is the ordinary one. Gated under
    /// `AiProfile::TeamPlan`.
    Pillar,
    /// The team's lock on the enemy `subject` ([`TeamSituation::lock_ending`])
    /// ends within [`CHAIN_HORIZON_SECS`]: the CHAIN step — a hand-off of the
    /// class's own crowd control onto the subject, landing in the lock's last
    /// moments
    /// (`class_ai::chain`) — heads the chain. When it does not fire, or
    /// holds the global cooldown for a hand-off still to come, the class's
    /// own chain runs below it, led by a burst step when the Burst stage
    /// applies to the unit (`CombatContext::burst_window`). Under
    /// `AiProfile::TeamPlan` (graduated at the 0.8 sweep).
    Chain { subject: Entity },
}

impl Play {
    /// The play this is; `None` for [`Play::Default`], which is no stage.
    pub fn id(&self) -> Option<PlayId> {
        match self {
            Play::Default => None,
            Play::Burst => Some(PlayId::Burst),
            Play::Peel { .. } => Some(PlayId::Peel),
            Play::Hunt => Some(PlayId::Hunt),
            Play::Pillar => Some(PlayId::Pillar),
            Play::Chain { .. } => Some(PlayId::Chain),
        }
    }
}

/// A play's identity: a [`Play`] without its payload. Withholding a gated
/// play, overriding its exceptions and placing it in the pipeline
/// (`class_ai::plays::PLAY_ORDER`) are all keyed by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlayId {
    Peel,
    Hunt,
    Pillar,
    Chain,
    Burst,
}

impl PlayId {
    /// Every play, in declaration order.
    pub const ALL: [PlayId; 5] = [
        PlayId::Peel,
        PlayId::Hunt,
        PlayId::Pillar,
        PlayId::Chain,
        PlayId::Burst,
    ];

    /// How many plays there are.
    pub const COUNT: usize = Self::ALL.len();

    fn index(self) -> usize {
        self as usize
    }

    /// Whether the play is still GATED: on probation under `TeamPlan` until a
    /// milestone sweep graduates or deletes it, and so the one kind of play
    /// [`WithheldPlays`] may deny a team. A GRADUATED play is permanent under
    /// `TeamPlan`: nothing withholds it (the user's ruling on what graduating
    /// means, 0.8 sweep, 2026-10-25). Exhaustive, so a new play answers here.
    pub const fn is_gated(self) -> bool {
        match self {
            // AS-387; kept gated at the 0.8 sweep (its sign flips by map,
            // 2026-10-25-0.8-milestone-sweep.md §3.5).
            PlayId::Pillar => true,
            // Graduated at the 0.8 sweep (AS-395's rulings, applied by AS-398).
            PlayId::Peel | PlayId::Hunt | PlayId::Chain | PlayId::Burst => false,
        }
    }
}

/// The part a unit plays for its team, as far as choosing a play goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    MeleeDps,
    RangedDps,
    Healer,
}

impl Role {
    /// The role `class` plays: a healer first (the Paladin is one), then melee
    /// or ranged.
    pub fn of(class: crate::states::match_config::CharacterClass) -> Self {
        if class.is_healer() {
            Role::Healer
        } else if class.is_melee() {
            Role::MeleeDps
        } else {
            Role::RangedDps
        }
    }
}

impl TeamSituation {
    /// Teammates below `urgency_hp_threshold`, in entity order.
    pub fn dying_allies(&self) -> impl Iterator<Item = Entity> + '_ {
        self.allies.iter().filter(|a| a.dying).map(|a| a.entity)
    }

    /// Whether teammate `ally` is pressured ([`AllyStanding::pressured`]).
    pub fn is_pressured(&self, ally: Entity) -> bool {
        self.allies.iter().any(|a| a.entity == ally && a.pressured)
    }

    /// Whether teammate `ally` meets the ONSET of pressure
    /// ([`AllyStanding::pressure_onset`]): the entry half of the healers'
    /// PRESSURED hysteresis.
    pub fn is_pressure_onset(&self, ally: Entity) -> bool {
        self.allies
            .iter()
            .any(|a| a.entity == ally && a.pressure_onset)
    }

    /// How much of the way to [`MatchPhase::Attrition`] is left: 1 at the
    /// gates, falling linearly to 0 at [`ATTRITION_ONSET_SECS`], and 0 after.
    pub fn attrition_remaining(&self) -> f32 {
        (1.0 - self.since_gates / ATTRITION_ONSET_SECS).clamp(0.0, 1.0)
    }

    /// The play the unit `me`, playing `role`, runs in this situation, as
    /// its TEAM sees it: the first stage of the pipeline
    /// (`class_ai::plays::PLAY_ORDER` — Peel, Hunt, Pillar, Chain, Burst) that
    /// the team runs ([`Self::runs`]) and that the situation calls for `me`;
    /// else [`Play::Default`]. The unit's own admission — its tools and each
    /// stage's exceptions — is `CombatContext::play`'s, over the same walk.
    pub fn play_for(&self, role: Role, me: Entity) -> Play {
        plays::walk(self, role, me, None, |_, _| true)
    }

    /// The play the unit `me`, playing `role`, runs when the stage `id` and
    /// every stage before it do not apply: the same walk, from the stage
    /// after `id`, as the team sees it. Test-only: it pins the walk's
    /// composition from the team's side; the AI reads `CombatContext::play`.
    #[cfg(test)]
    pub fn play_after(&self, id: PlayId, role: Role, me: Entity) -> Play {
        plays::walk(self, role, me, Some(id), |_, _| true)
    }

    /// The lock a chain hands off from: [`Self::lock_ending`], on a team that
    /// runs [`Play::Chain`] ([`Self::runs`]), while its `until` is no more
    /// than [`CHAIN_HORIZON_SECS`] away.
    pub fn chain_window(&self) -> Option<Window> {
        self.lock_ending.filter(|w| {
            self.runs.contains(PlayId::Chain)
                && w.until
                    .is_some_and(|until| until - self.now <= CHAIN_HORIZON_SECS)
        })
    }
}

/// A set of classes, as a bit per [`CharacterClass::all`] entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ClassSet(u16);

impl ClassSet {
    /// The set of exactly `classes`.
    pub fn of(classes: &[CharacterClass]) -> Self {
        Self(classes.iter().fold(0, |bits, c| bits | Self::bit(*c)))
    }

    fn bit(class: CharacterClass) -> u16 {
        let index = CharacterClass::all()
            .iter()
            .position(|c| *c == class)
            .expect("every class is in CharacterClass::all");
        1 << index
    }

    pub fn contains(&self, class: CharacterClass) -> bool {
        self.0 & Self::bit(class) != 0
    }

    /// Its classes, in [`CharacterClass::all`] order.
    pub fn classes(&self) -> impl Iterator<Item = CharacterClass> + '_ {
        CharacterClass::all()
            .iter()
            .copied()
            .filter(|c| self.contains(*c))
    }
}

/// How long before a lock the team holds ends the Chain play runs
/// ([`TeamSituation::chain_window`]). It covers the slowest hand-off in the
/// kit — a 1.5s cast slowed by Curse of Tongues, 2.25s — landing in the
/// lock's last moments, plus the global cooldown held for it
/// (`class_ai::chain`). Earlier than this nothing can be handed off yet, so
/// the unit runs its fallback play.
pub const CHAIN_HORIZON_SECS: f32 = 4.0;

/// The key an exceptions table is looked up under: the class of the unit
/// deciding, and for a play switched off by CONTEXT (the Peel play, by the
/// teammate it peels for) the class of that context. A play switched off by
/// FORMAT (the Peel play's 3v3 gates) is keyed by the deciding class and its
/// team's size instead. Every play's table is a shape of it, so one override
/// type serves them all ([`ExceptionOverrides`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExceptionKey {
    pub class: CharacterClass,
    pub context: Option<CharacterClass>,
    /// The deciding unit's team size, for a key that switches a play off in
    /// one format (`in_format`); `None` otherwise.
    pub team_size: Option<u8>,
}

/// The team sizes a format key can name ([`ExceptionKey::in_format`]).
pub const FORMAT_SIZES: std::ops::RangeInclusive<u8> = 2..=3;

impl ExceptionKey {
    /// `class`, in any context.
    pub fn class(class: CharacterClass) -> Self {
        Self {
            class,
            context: None,
            team_size: None,
        }
    }

    /// `class`, in the context of `context`.
    pub fn in_context(class: CharacterClass, context: CharacterClass) -> Self {
        Self {
            class,
            context: Some(context),
            team_size: None,
        }
    }

    /// `class`, on a team of `team_size` (2 or 3: [`FORMAT_SIZES`]), in any
    /// context. Panics on another size.
    pub fn in_format(class: CharacterClass, team_size: u8) -> Self {
        assert!(
            FORMAT_SIZES.contains(&team_size),
            "a format key names a team of 2 or 3, not {team_size}"
        );
        Self {
            class,
            context: None,
            team_size: Some(team_size),
        }
    }

    fn bit(self) -> u128 {
        let index = |c: CharacterClass| {
            CharacterClass::all()
                .iter()
                .position(|x| *x == c)
                .expect("every class is in CharacterClass::all")
        };
        let n = CharacterClass::all().len();
        let stride = n + 1;
        let slot = match self.team_size {
            // After every (class, context) slot: one run of classes per size.
            Some(size) => {
                n * stride + (size - FORMAT_SIZES.start()) as usize * n + index(self.class)
            }
            None => index(self.class) * stride + self.context.map_or(0, |c| index(c) + 1),
        };
        assert!(slot < 128, "ExceptionSet holds {slot} keys: widen it");
        1 << slot
    }

    /// Parse a config's exception entry: `"Mage"`, `"Mage/Hunter"` (a Mage,
    /// peeling for a Hunter) or `"Mage@3v3"` (a Mage on a team of three),
    /// with `parse_class` naming each class.
    pub fn parse(
        entry: &str,
        parse_class: impl Fn(&str) -> Result<CharacterClass, String>,
    ) -> Result<Self, String> {
        if let Some((class, format)) = entry.split_once('@') {
            let size = match format.trim() {
                "2v2" => 2,
                "3v3" => 3,
                other => {
                    return Err(format!(
                        "Exception '{entry}': unknown format '{other}' (2v2, 3v3)"
                    ))
                }
            };
            return Ok(Self::in_format(parse_class(class.trim())?, size));
        }
        match entry.split_once('/') {
            Some((class, context)) => Ok(Self::in_context(
                parse_class(class.trim())?,
                parse_class(context.trim())?,
            )),
            None => Ok(Self::class(parse_class(entry.trim())?)),
        }
    }
}

/// A set of [`ExceptionKey`]s: one play's exceptions table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ExceptionSet(u128);

impl ExceptionSet {
    /// The set of exactly `keys`.
    pub fn of(keys: &[ExceptionKey]) -> Self {
        Self(keys.iter().fold(0, |bits, k| bits | k.bit()))
    }

    pub fn contains(&self, key: ExceptionKey) -> bool {
        self.0 & key.bit() != 0
    }
}

impl From<ClassSet> for ExceptionSet {
    /// Each class of `classes`, in any context.
    fn from(classes: ClassSet) -> Self {
        Self::of(
            &classes
                .classes()
                .map(ExceptionKey::class)
                .collect::<Vec<_>>(),
        )
    }
}

/// Every gated play's exceptions, as one team runs them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayExceptions([ExceptionSet; PlayId::COUNT]);

impl PlayExceptions {
    /// `id`'s exceptions.
    pub fn of(&self, id: PlayId) -> ExceptionSet {
        self.0[id.index()]
    }

    /// Each play's shipped table (`PlayStage::exceptions`), less what
    /// `overrides` replaces.
    pub fn resolve(overrides: &ExceptionOverrides) -> Self {
        Self(PlayId::ALL.map(|id| {
            overrides
                .get(id)
                .unwrap_or_else(|| plays::stage(id).exceptions())
        }))
    }
}

/// A measurement's replacement exceptions tables, per gated play: `None`
/// keeps the shipped table, `Some(set)` runs `set` in its place, and
/// `Some(ExceptionSet::default())` LIFTS every exception — how a milestone
/// sweep re-checks one against the same side with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExceptionOverrides([Option<ExceptionSet>; PlayId::COUNT]);

impl ExceptionOverrides {
    /// `id`'s replacement table, if any.
    pub fn get(&self, id: PlayId) -> Option<ExceptionSet> {
        self.0[id.index()]
    }

    /// Run `set` in place of `id`'s shipped table.
    pub fn set(&mut self, id: PlayId, set: ExceptionSet) {
        self.0[id.index()] = Some(set);
    }

    /// Lift every exception of `id`.
    pub fn lift(&mut self, id: PlayId) {
        self.set(id, ExceptionSet::default());
    }
}

/// The situation of both teams. Team `t` lives at index `t - 1`.
///
/// Inserted per match beside `TeamPlans`; `decide_abilities` keeps it current
/// (see the module docs for why there). Read it from another system and you
/// get the previous frame's classification.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Situations {
    pub teams: [TeamSituation; 2],
}

impl Situations {
    /// Team `team`'s situation; the default (nothing going on) for a team id
    /// outside 1..=2, matching `TeamPlans::for_team`.
    pub fn for_team(&self, team: u8) -> TeamSituation {
        team.checked_sub(1)
            .and_then(|i| self.teams.get(i as usize))
            .cloned()
            .unwrap_or_default()
    }
}

/// The thresholds the classifier reads — all from `movement.ron`'s `shared:`
/// block (the module docs' threshold table).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SituationConfig {
    /// `shared.press_advantage_margin` in `movement.ron`: tempo's ONSET, the
    /// team-HP-fraction lead at which a team starts pressing (and the deficit
    /// at which it starts trailing).
    pub press_advantage_margin: f32,
    /// `shared.press_release_margin`: tempo's RELEASE, the lead (deficit)
    /// below which a pressing (trailing) team stops. Equal to the onset under
    /// `Legacy` ([`Self::under`]).
    pub press_release_margin: f32,
    /// `shared.urgency_hp_threshold`: a teammate below it is dying.
    pub urgency_hp_threshold: f32,
    /// `shared.danger_radius`: any enemy targeting a teammate from this close
    /// pressures it.
    pub danger_radius: f32,
    /// `shared.threat_intent_radius`: a melee, pet or closing enemy targeting
    /// a teammate from this close pressures it.
    pub threat_intent_radius: f32,
    /// `shared.danger_dwell`: seconds a teammate stays dying and pressured
    /// before an `ally_in_danger` window opens on it.
    pub danger_dwell: f32,
    /// `shared.danger_release`: seconds a window's teammate stays out of
    /// danger before the window ends.
    pub danger_release: f32,
    /// `shared.danger_edge_margin`: yards inside every pressure boundary at
    /// which a threat pressures FIRMLY, skipping the dwell
    /// ([`firmly_pressured`]).
    pub danger_edge_margin: f32,
    /// `shared.kill_hp_threshold`: an enemy below this health fraction is
    /// low, and can open a kill window.
    pub kill_hp_threshold: f32,
    /// `shared.kill_hp_release`: a kill window's subject stays low until it
    /// is back above this fraction.
    pub kill_hp_release: f32,
    /// `shared.kill_oom_mana`: an enemy healer with less mana than this
    /// cannot heal.
    pub kill_oom_mana: f32,
    /// `shared.heal_range`: an enemy healer farther than this from a unit
    /// cannot heal it.
    pub heal_range: f32,
    /// Whether an enemy HEALER counts as closing ([`closes`]). `false` under
    /// `TeamPlan` and with the bare thresholds: a healer's position is its
    /// posture machine's, not a walk to its target, so it pressures only from
    /// `danger_radius`. `true` under `Legacy` ([`Self::under`]), the rule it
    /// has always run.
    pub healers_close: bool,
    /// The plays the team runs. None until [`Self::under`] puts the
    /// team under `TeamPlan`, so a context classified with the bare
    /// thresholds (a test's) runs no play.
    pub runs: PlaySet,
    /// The team's replacement exceptions tables, if a measurement set any;
    /// set by [`Self::for_team`] from [`Self::withheld`].
    pub exceptions: ExceptionOverrides,
    /// The gated plays each team is denied, and its exceptions overrides
    /// ([`WithheldPlays`]), applied by [`Self::for_team`].
    pub withheld: WithheldPlays,
    /// Whether the team's healers play the pillar modes (AS-373,
    /// `class_ai::pillar_mode`): `true` under `TeamPlan` ([`Self::under`])
    /// unless a measurement turns them off ([`WithheldPlays::no_pillar_modes`]).
    pub pillar_modes: bool,
    /// Whether the team classifies its `sight_yield` (AS-386): `true` under
    /// `TeamPlan` ([`Self::under`]) unless a measurement turns it off
    /// ([`WithheldPlays::no_sight_yield`]).
    pub sight_yield: bool,
    /// Whether the yield's rule 1 counts the healer's cast in progress as
    /// "cannot move" (AS-386): `true`, except in the measurement variant that
    /// turns it off ([`WithheldPlays::yield_cast_moves`]).
    pub yield_counts_healer_cast: bool,
    /// `shared.cc_reach_margin`: the margin of the crowd-control discs the
    /// pillar stance's team arc keeps out of.
    pub cc_reach_margin: f32,
    /// Whether the team classifies its `target_focus` (AS-389): `true` under
    /// `TeamPlan` ([`Self::under`]) unless a measurement turns it off
    /// ([`WithheldPlays::no_target_focus`]).
    pub target_focus: bool,
}

impl Default for SituationConfig {
    fn default() -> Self {
        Self::from_movement(&MovementConfig::default())
    }
}

impl SituationConfig {
    pub fn from_movement(config: &MovementConfig) -> Self {
        Self {
            press_advantage_margin: config.shared.press_advantage_margin,
            press_release_margin: config.shared.press_release_margin,
            urgency_hp_threshold: config.shared.urgency_hp_threshold,
            danger_radius: config.shared.danger_radius,
            threat_intent_radius: config.shared.threat_intent_radius,
            danger_dwell: config.shared.danger_dwell,
            danger_release: config.shared.danger_release,
            danger_edge_margin: config.shared.danger_edge_margin,
            kill_hp_threshold: config.shared.kill_hp_threshold,
            kill_hp_release: config.shared.kill_hp_release,
            kill_oom_mana: config.shared.kill_oom_mana,
            heal_range: config.shared.heal_range,
            healers_close: false,
            runs: PlaySet::default(),
            exceptions: ExceptionOverrides::default(),
            withheld: WithheldPlays::default(),
            pillar_modes: false,
            sight_yield: false,
            yield_counts_healer_cast: true,
            cc_reach_margin: config.shared.cc_reach_margin,
            target_focus: false,
        }
    }

    /// The thresholds a team under `profile` is classified with. Tempo's
    /// hysteresis band is a behaviour change, so it is gated: `TeamPlan` keeps
    /// the configured release, and `Legacy` releases at the onset — no band,
    /// which is exactly the unlatched `>=` test it has always run. The plays
    /// are gated the same way: `TeamPlan` runs every one, `Legacy` none. So is
    /// the closing rule ([`closes`]): only under `Legacy` does an enemy healer
    /// still count as closing. Every other threshold is the same under both.
    pub fn under(self, profile: AiProfile) -> Self {
        match profile {
            AiProfile::TeamPlan => Self {
                runs: PlaySet::all(),
                pillar_modes: true,
                sight_yield: true,
                target_focus: true,
                ..self
            },
            AiProfile::Legacy => Self {
                press_release_margin: self.press_advantage_margin,
                healers_close: true,
                runs: PlaySet::default(),
                pillar_modes: false,
                sight_yield: false,
                target_focus: false,
                ..self
            },
        }
    }

    /// The configuration team `team`, under `profile`, is classified with:
    /// [`Self::under`], less the gated plays [`Self::withheld`] denies the
    /// team, and with the exceptions overrides it sets for the team.
    ///
    /// Panics if [`Self::withheld`] names a graduated play for the team: a
    /// graduated play cannot be withheld ([`PlayId::is_gated`]), and a
    /// headless config that asks is refused when it is read
    /// ([`WithheldPlays::parse_set`]).
    pub fn for_team(self, team: u8, profile: AiProfile) -> Self {
        let cfg = self.under(profile);
        let withheld = self.withheld.for_team(team);
        if let Some(id) = withheld.ids().find(|id| !id.is_gated()) {
            panic!("{id:?} graduated and cannot be withheld (team {team})");
        }
        Self {
            runs: cfg.runs.without(withheld),
            exceptions: self.withheld.exceptions_for_team(team),
            pillar_modes: cfg.pillar_modes && !self.withheld.no_pillar_modes(team),
            sight_yield: cfg.sight_yield && !self.withheld.no_sight_yield(team),
            target_focus: cfg.target_focus && !self.withheld.no_target_focus(team),
            yield_counts_healer_cast: cfg.yield_counts_healer_cast
                && !self.withheld.yield_cast_moves(team),
            ..cfg
        }
    }
}

/// A set of plays, a bit per [`PlayId`]: the plays a team runs, or the gated
/// plays withheld from it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaySet(u8);

impl PlaySet {
    /// The set of exactly `plays` ([`Play::Default`] is no stage, and adds
    /// nothing).
    pub fn of(plays: &[Play]) -> Self {
        let mut set = Self::default();
        for play in plays {
            set.withhold(play);
        }
        set
    }

    /// Every play.
    pub fn all() -> Self {
        let mut set = Self::default();
        for id in PlayId::ALL {
            set.insert(id);
        }
        set
    }

    /// Add `play` (a no-op for [`Play::Default`], which is no stage).
    pub fn withhold(&mut self, play: &Play) {
        if let Some(id) = play.id() {
            self.insert(id);
        }
    }

    pub fn insert(&mut self, id: PlayId) {
        self.0 |= 1 << id.index();
    }

    pub fn contains(&self, id: PlayId) -> bool {
        self.0 & (1 << id.index()) != 0
    }

    /// Whether `play` is in the set. [`Play::Default`] never is.
    pub fn denies(&self, play: &Play) -> bool {
        play.id().is_some_and(|id| self.contains(id))
    }

    /// This set less `other`'s plays.
    pub fn without(self, other: PlaySet) -> Self {
        Self(self.0 & !other.0)
    }

    /// Its plays, in [`PlayId::ALL`] order.
    pub fn ids(&self) -> impl Iterator<Item = PlayId> + '_ {
        PlayId::ALL.into_iter().filter(|id| self.contains(*id))
    }
}

/// `Play::Peel` as a KIND, for the sets above (a peel's ally does not enter
/// whether it is withheld).
pub const PEEL: Play = Play::Peel {
    ally: Entity::PLACEHOLDER,
};

/// `Play::Chain` as a KIND, for the sets above (a chain's subject does not
/// enter whether it is withheld).
pub const CHAIN: Play = Play::Chain {
    subject: Entity::PLACEHOLDER,
};

/// The measurement controls on the plays, per team, keyed by play id:
///
/// - **Withheld** ([`Self::team1`], [`Self::team2`]): the GATED plays each
///   team is DENIED ([`PlayId::is_gated`]). A team under `TeamPlan` with a
///   play withheld is the paired control the play is measured against — in a
///   tier-4 probe (the same seed bank with the play off) and in a head-to-head
///   (one side with it, one without, on one seed). A graduated play cannot be
///   withheld ([`SituationConfig::for_team`] panics on one).
/// - **Exceptions** ([`Self::team1_exceptions`], [`Self::team2_exceptions`]):
///   any play's exceptions table replaced for a team. An empty table LIFTS
///   every exception, so the play runs even where it measured negative: how
///   a milestone sweep re-checks an exception, the same side with it and
///   without, on one seed.
///
/// A MEASUREMENT CONTROL, not a behaviour switch: nothing in the game sets it,
/// but it is reachable from a headless match's JSON (`team1_withheld_plays`,
/// `team1_exceptions` and team 2's) and so from `--replay`, which honours the
/// same config. The default withholds and overrides nothing. The withheld sets
/// live only as long as a play stays gated: when the last gated play
/// graduates or is deleted (§4.5), they and their config fields are deleted
/// too. The exceptions overrides stay as long as any play keeps a table.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WithheldPlays {
    pub team1: PlaySet,
    pub team2: PlaySet,
    /// Team 1's replacement exceptions tables.
    pub team1_exceptions: ExceptionOverrides,
    /// Team 2's replacement exceptions tables.
    pub team2_exceptions: ExceptionOverrides,
    /// Team 1 plays without the healer pillar modes (AS-373,
    /// `class_ai::pillar_mode`): a measurement control, as the withheld plays
    /// are (`team1_pillar_modes: false` in a headless config).
    pub team1_no_pillar_modes: bool,
    /// Team 2's, as [`Self::team1_no_pillar_modes`].
    pub team2_no_pillar_modes: bool,
    /// Team 1 plays without the sight yield (AS-386, `sight_yield`): a
    /// measurement control, as the pillar modes are (`team1_sight_yield:
    /// false` in a headless config).
    pub team1_no_sight_yield: bool,
    /// Team 2's, as [`Self::team1_no_sight_yield`].
    pub team2_no_sight_yield: bool,
    /// Team 1's sight yield does NOT count its healer's cast in progress as
    /// "cannot move" in rule 1 (AS-385 package measurement, the variant arm:
    /// `team1_yield_healer_cast: false` in a headless config). A measurement
    /// control only; the shipped rule counts the cast.
    pub team1_yield_cast_moves: bool,
    /// Team 2's, as [`Self::team1_yield_cast_moves`].
    pub team2_yield_cast_moves: bool,
    /// Team 1 plays without the target focus (AS-389, `target_focus`): a
    /// measurement control, as the sight yield is (`team1_target_focus:
    /// false` in a headless config).
    pub team1_no_target_focus: bool,
    /// Team 2's, as [`Self::team1_no_target_focus`].
    pub team2_no_target_focus: bool,
}

impl WithheldPlays {
    /// Whether team `team`'s yield leaves its healer's cast out of rule 1;
    /// team 1's for a team id outside 1..=2.
    pub fn yield_cast_moves(&self, team: u8) -> bool {
        if team == 2 {
            self.team2_yield_cast_moves
        } else {
            self.team1_yield_cast_moves
        }
    }

    /// Whether team `team` plays without the target focus; team 1's for a
    /// team id outside 1..=2.
    pub fn no_target_focus(&self, team: u8) -> bool {
        if team == 2 {
            self.team2_no_target_focus
        } else {
            self.team1_no_target_focus
        }
    }

    /// Whether team `team` plays without the sight yield; team 1's for a team
    /// id outside 1..=2.
    pub fn no_sight_yield(&self, team: u8) -> bool {
        if team == 2 {
            self.team2_no_sight_yield
        } else {
            self.team1_no_sight_yield
        }
    }

    /// Whether team `team` plays without the healer pillar modes; team 1's
    /// for a team id outside 1..=2.
    pub fn no_pillar_modes(&self, team: u8) -> bool {
        if team == 2 {
            self.team2_no_pillar_modes
        } else {
            self.team1_no_pillar_modes
        }
    }

    /// Team `team`'s withheld plays; team 1's for a team id outside 1..=2,
    /// matching `AiProfiles::for_team`.
    pub fn for_team(&self, team: u8) -> PlaySet {
        if team == 2 {
            self.team2
        } else {
            self.team1
        }
    }

    /// Team `team`'s exceptions overrides; team 1's for a team id outside
    /// 1..=2.
    pub fn exceptions_for_team(&self, team: u8) -> ExceptionOverrides {
        if team == 2 {
            self.team2_exceptions
        } else {
            self.team1_exceptions
        }
    }

    /// Team `team`'s exceptions overrides, to set; team 1's for a team id
    /// outside 1..=2.
    pub fn exceptions_mut(&mut self, team: u8) -> &mut ExceptionOverrides {
        if team == 2 {
            &mut self.team2_exceptions
        } else {
            &mut self.team1_exceptions
        }
    }

    /// Parse a config's list of plays to withhold (each stage's config key,
    /// `class_ai::plays::parse_play`) into the set it names. Only a GATED
    /// play may be withheld ([`PlayId::is_gated`]): a graduated one is an
    /// error, which names the exceptions override that is left.
    pub fn parse_set(names: &[String]) -> Result<PlaySet, String> {
        let mut set = PlaySet::default();
        for name in names {
            let id = plays::parse_play(name)?;
            if !id.is_gated() {
                return Err(format!(
                    "{name} graduated at the 0.8 milestone sweep and cannot be withheld; \
                     its exceptions can still be replaced (team1_exceptions / team2_exceptions)"
                ));
            }
            set.insert(id);
        }
        Ok(set)
    }

    /// Parse a config's exceptions table for the play `id`: each entry a
    /// class (`"Mage"`) or, for a play switched off by context, a class in
    /// a context (`"Mage/Hunter"`) — the shape the play's stage keys its
    /// exceptions by (`PlayStage::exceptions_have_context`).
    pub fn parse_exceptions(
        id: PlayId,
        entries: &[String],
        parse_class: impl Fn(&str) -> Result<CharacterClass, String>,
    ) -> Result<ExceptionSet, String> {
        let stage = plays::stage(id);
        let mut keys = Vec::with_capacity(entries.len());
        for entry in entries {
            let key = ExceptionKey::parse(entry, &parse_class)?;
            let fits = if key.team_size.is_some() {
                stage.exceptions_by_format()
            } else {
                key.context.is_some() == stage.exceptions_have_context()
            };
            if !fits {
                return Err(format!(
                    "Exception '{entry}' for {}: its exceptions are {}{}",
                    stage.config_key(),
                    if stage.exceptions_have_context() {
                        "a class in a context, as \"Mage/Hunter\""
                    } else {
                        "a class, as \"Mage\""
                    },
                    if stage.exceptions_by_format() {
                        ", or a class in a format, as \"Mage@3v3\""
                    } else {
                        ""
                    }
                ));
            }
            keys.push(key);
        }
        Ok(ExceptionSet::of(&keys))
    }
}

/// Crowd control that stops a healer casting a heal: Stun, Fear, Polymorph,
/// Incapacitate, Cyclone. NOT Root: a rooted healer heals freely, so a root
/// must not open a burst window. It is the set the hard-CC choosers ask of a
/// target ([`holds`](super::class_ai::holds)): a healer that is locked is one
/// no hard CC is spent on.
pub fn locks_healer(effect: AuraType) -> bool {
    super::class_ai::holds(effect)
}

/// Classify `team`'s situation from `snapshot`. Pure: the snapshot, the
/// previous classification (for each window's onset), the clocks, and the
/// thresholds.
pub fn classify(
    snapshot: &CombatSnapshot,
    team: u8,
    prev: &TeamSituation,
    clock: &MatchClock,
    cfg: &SituationConfig,
) -> TeamSituation {
    classify_roster(
        &snapshot.combatants,
        &snapshot.active_auras,
        &snapshot.obstacles,
        team,
        prev,
        clock,
        cfg,
    )
}

/// [`classify`] short of `sight_yield`, which reads the pillar stance
/// (rule 3, [`stance_leads_sight`]): [`update_situations`] classifies the
/// stance on this, then the yield ([`yield_sight`]).
fn classify_before_yield(
    snapshot: &CombatSnapshot,
    team: u8,
    prev: &TeamSituation,
    clock: &MatchClock,
    cfg: &SituationConfig,
) -> TeamSituation {
    classify_roster_before_yield(
        &snapshot.combatants,
        &snapshot.active_auras,
        &snapshot.obstacles,
        team,
        prev,
        clock,
        cfg,
    )
}

/// [`classify`] over what it reads — the roster, its auras and the map's
/// obstacles — so a context built without a snapshot
/// (`CombatContext::classified`) can classify its own roster. It classifies
/// no pillar stance (that needs the enemies' kits, which only
/// [`update_situations`] has), so its `sight_yield` sees none.
pub fn classify_roster(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    team: u8,
    prev: &TeamSituation,
    clock: &MatchClock,
    cfg: &SituationConfig,
) -> TeamSituation {
    let mut situation =
        classify_roster_before_yield(roster, auras, obstacles, team, prev, clock, cfg);
    yield_sight(&mut situation, roster, auras, team, prev, clock.now, cfg);
    situation
}

/// The team's `sight_yield`, classified last onto `situation`: its rule 3
/// reads the situation's pillar stance ([`stance_leads_sight`]), and its
/// rules 1-2 the allies and the teammate in danger classified before it.
fn yield_sight(
    situation: &mut TeamSituation,
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
    prev: &TeamSituation,
    now: f32,
    cfg: &SituationConfig,
) {
    situation.sight_yield = classify_sight_yield(
        roster,
        auras,
        &situation.allies,
        situation.ally_in_danger,
        stance_leads_sight(situation),
        team,
        prev,
        now,
        cfg,
    );
}

/// Everything [`classify_roster`] classifies but `sight_yield`.
fn classify_roster_before_yield(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    team: u8,
    prev: &TeamSituation,
    clock: &MatchClock,
    cfg: &SituationConfig,
) -> TeamSituation {
    let allies = allies(roster, auras, team, cfg);
    let danger = ally_in_danger(roster, auras, &allies, prev, clock.now, cfg);
    let ally_in_danger = danger.window;
    let danger_attackers = ally_in_danger
        .and_then(|w| roster.get(&w.subject))
        .map(|subject| pressuring(roster, auras, subject, cfg))
        .unwrap_or_default();
    let kill_window = kill_window(roster, auras, obstacles, team, prev, clock.now, cfg);
    let (target_focus, target_sight) = target_focus::classify(
        roster,
        auras,
        obstacles,
        team,
        prev.target_focus,
        &prev.target_sight,
        kill_window.map(|w| w.subject),
        clock.now,
        cfg.target_focus,
    );
    TeamSituation {
        phase: phase(clock),
        since_gates: clock.since_gates,
        tempo: tempo(roster, team, prev.tempo, cfg),
        enemy_healer_locked: enemy_healer_locked(roster, auras, team, prev, clock.now),
        ally_in_danger,
        danger_attackers,
        runs: cfg.runs,
        pillar_modes: cfg.pillar_modes,
        pillar_stance: None,
        stance_memory: StanceMemory::default(),
        hunt: None,
        exceptions: PlayExceptions::resolve(&cfg.exceptions),
        allies,
        in_danger: danger.in_danger,
        danger_cleared_at: danger.cleared_at,
        classified: true,
        kill_window,
        target_focus,
        target_sight,
        now: clock.now,
        lock_ending: lock_ending(roster, auras, team, prev, clock.now),
        sight_yield: SmallVec::new(),
    }
}

/// When `healer` (an enemy healer, alive) can next heal `subject`:
/// `Some(Some(t))` — it is only LOCKED, and the longest locking CC on it runs
/// out at `t`; `Some(None)` — it cannot, and when it can again is not known
/// (out of mana, out of reach, out of sight); `None` — it can heal now.
fn healer_returns_at(
    healer: &CombatantInfo,
    subject: &CombatantInfo,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    now: f32,
    cfg: &SituationConfig,
) -> Option<Option<f32>> {
    let out_of_mana = healer.current_mana < cfg.kill_oom_mana;
    let out_of_reach = healer.entity != subject.entity
        && (healer.position.distance(subject.position) > cfg.heal_range
            || !has_line_of_sight(obstacles, healer.position, subject.position));
    if out_of_mana || out_of_reach {
        return Some(None);
    }
    auras
        .get(&healer.entity)?
        .iter()
        .filter(|a| locks_healer(a.effect_type))
        .map(|a| a.duration)
        .reduce(f32::max)
        .map(|remaining| Some(now + remaining))
}

/// The `kill_window`: the most injured enemy of `team` (entity order on a
/// tie) that is LOW and that no living enemy healer can heal — see the module
/// docs. Low is `kill_hp_threshold` to open, and `kill_hp_release` to hold an
/// open window's subject. The subject is one the team can SEE (a stealthed
/// enemy is no kill window), and not a pet.
///
/// `until` is the earliest time a healer that is only locked comes free; a
/// healer whose return is not known adds no deadline, and with none known
/// the window has no `until`. The onset carries while the subject holds.
fn kill_window(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    team: u8,
    prev: &TeamSituation,
    now: f32,
    cfg: &SituationConfig,
) -> Option<Window> {
    let open = prev.kill_window;
    let healers: SmallVec<[&CombatantInfo; 2]> = roster
        .values()
        .filter(|i| i.team != team && i.is_alive && !i.is_pet && i.class.is_healer())
        .collect();
    let mut best: Option<(f32, Window)> = None;
    for subject in roster.values().filter(|i| {
        i.team != team
            && i.is_alive
            && !i.is_pet
            && stealth_visible(i.stealthed, false, has_shadow_sight(auras, i.entity))
    }) {
        let is_open = open.is_some_and(|w| w.subject == subject.entity);
        let bar = if is_open {
            cfg.kill_hp_release
        } else {
            cfg.kill_hp_threshold
        };
        let hp = subject.health_pct();
        if hp >= bar {
            continue;
        }
        let mut until: Option<f32> = None;
        let mut healable = false;
        for healer in &healers {
            match healer_returns_at(healer, subject, auras, obstacles, now, cfg) {
                None => healable = true,
                Some(Some(t)) => until = Some(until.map_or(t, |u: f32| u.min(t))),
                Some(None) => {}
            }
        }
        if healable {
            continue;
        }
        if best.is_none_or(|(best_hp, _)| hp < best_hp) {
            let since = open
                .filter(|w| w.subject == subject.entity)
                .map_or(now, |w| w.since);
            best = Some((
                hp,
                Window {
                    subject: subject.entity,
                    since,
                    until,
                },
            ));
        }
    }
    best.map(|(_, w)| w)
}

fn phase(clock: &MatchClock) -> MatchPhase {
    if !clock.gates_opened {
        MatchPhase::Prematch
    } else if clock.since_gates >= ATTRITION_ONSET_SECS {
        MatchPhase::Attrition
    } else {
        MatchPhase::Engaged
    }
}

fn has_shadow_sight(auras: &BTreeMap<Entity, Vec<Aura>>, entity: Entity) -> bool {
    auras
        .get(&entity)
        .is_some_and(|list| list.iter().any(|a| a.effect_type == AuraType::ShadowSight))
}

/// Every living non-pet member of `team`, in entity order, with whether it is
/// dying and whether it is pressured.
fn allies(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
    cfg: &SituationConfig,
) -> SmallVec<[AllyStanding; 3]> {
    roster
        .values()
        .filter(|info| info.team == team && info.is_alive && !info.is_pet)
        .map(|info| AllyStanding {
            entity: info.entity,
            dying: info.health_pct() < cfg.urgency_hp_threshold,
            pressured: pressured(roster, auras, info, cfg),
            pressure_onset: pressure_onset(roster, auras, info, cfg),
        })
        .collect()
}

/// [`AllyStanding::pressured`], judged from `subject`'s own view: the enemies
/// [`stealth_visible`] lets IT see, its own Shadow Sight included. Enemy pets
/// count as threats.
fn pressured(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    subject: &CombatantInfo,
    cfg: &SituationConfig,
) -> bool {
    pressure_threats(roster, auras, subject, cfg)
        .next()
        .is_some()
}

/// The enemies that make `subject` [pressured](AllyStanding::pressured),
/// nearest to it first (entity order on a tie): the attackers a peel answers.
fn pressuring(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    subject: &CombatantInfo,
    cfg: &SituationConfig,
) -> SmallVec<[Entity; 4]> {
    let mut threats: SmallVec<[(f32, Entity); 4]> = pressure_threats(roster, auras, subject, cfg)
        .map(|threat| (subject.position.distance(threat.position), threat.entity))
        .collect();
    // Stable, over entity order: a tie keeps entity order.
    threats.sort_by(|a, b| a.0.total_cmp(&b.0));
    threats.into_iter().map(|(_, entity)| entity).collect()
}

/// [`pressured`]'s rule, as the threats that satisfy it, in entity order.
fn pressure_threats<'r>(
    roster: &'r BTreeMap<Entity, CombatantInfo>,
    auras: &'r BTreeMap<Entity, Vec<Aura>>,
    subject: &'r CombatantInfo,
    cfg: &'r SituationConfig,
) -> impl Iterator<Item = &'r CombatantInfo> + 'r {
    let subject_has_shadow_sight = has_shadow_sight(auras, subject.entity);
    roster
        .values()
        .filter(move |c| {
            c.team != subject.team
                && c.is_alive
                && c.target == Some(subject.entity)
                && stealth_visible(
                    c.stealthed,
                    subject_has_shadow_sight,
                    has_shadow_sight(auras, c.entity),
                )
        })
        .filter(move |threat| {
            let distance = subject.position.distance(threat.position);
            distance <= cfg.danger_radius
                || (distance <= cfg.threat_intent_radius
                    && (threat.is_pet
                        || threat.class.is_melee()
                        || (closes(threat, cfg) && closing(threat, subject))))
        })
}

/// `threat`, targeting `subject`, is still beyond its own preferred range of
/// it: coming in, not holding position. Counts only for a threat that
/// [`closes`].
fn closing(threat: &CombatantInfo, subject: &CombatantInfo) -> bool {
    threat.distance_to(subject.position) > preferred_range(threat)
}

/// Whether `threat` (a ranged enemy, not a pet) can pressure by CLOSING: it
/// moves by pursuing its target to its preferred range, so being beyond that
/// range of its target means it is walking in to attack from it. A ranged
/// DPS does (the Warlock, the Hunter; the Mage's 38yd preferred range lies
/// beyond `threat_intent_radius`). An enemy HEALER does not: it moves by its
/// posture machine — formation, cover, its own teammates — so its distance to
/// the unit it targets says nothing about where it is heading, and it holds
/// in the band between its 28yd preferred range and the 30yd intent radius
/// as often as it passes through. It pressures from `danger_radius` like any
/// enemy (and a Paladin, a melee class, by the melee branch). Gated by
/// [`SituationConfig::healers_close`]: `Legacy` still counts the healer.
fn closes(threat: &CombatantInfo, cfg: &SituationConfig) -> bool {
    cfg.healers_close || !threat.class.is_healer()
}

fn preferred_range(threat: &CombatantInfo) -> f32 {
    match threat.pet_type {
        Some(pet_type) => pet_type.preferred_range(),
        None => threat.class.preferred_range(),
    }
}

/// `subject` is pressured by a threat that is not at the EDGE of the pressure
/// test: one that would still pressure it `danger_edge_margin` further
/// inside every boundary it passes — `danger_radius`, `threat_intent_radius`
/// and, for a closing threat, its own preferred range. An enemy sitting on a
/// boundary flips the test frame by frame; one well inside it does not.
/// Read only by the `ally_in_danger` window's dwell: the pressure test itself
/// is unchanged.
fn firmly_pressured(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    subject: &CombatantInfo,
    cfg: &SituationConfig,
) -> bool {
    let margin = cfg.danger_edge_margin;
    pressure_threats(roster, auras, subject, cfg).any(|threat| {
        let distance = subject.position.distance(threat.position);
        distance <= cfg.danger_radius - margin
            || (distance <= cfg.threat_intent_radius - margin
                && (threat.is_pet
                    || threat.class.is_melee()
                    || threat.distance_to(subject.position) > preferred_range(threat) + margin))
    })
}

/// [`AllyStanding::pressure_onset`]: [`pressured`], except that a threat
/// pressuring `subject` only by CLOSING must close `danger_edge_margin` inside
/// that branch's two edges — beyond its preferred range by the margin, and
/// within `threat_intent_radius` less it. The proximity branch and the
/// melee-or-pet branch are unchanged, so a melee or pet onsets exactly where
/// it always has.
fn pressure_onset(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    subject: &CombatantInfo,
    cfg: &SituationConfig,
) -> bool {
    let margin = cfg.danger_edge_margin;
    pressure_threats(roster, auras, subject, cfg).any(|threat| {
        let distance = subject.position.distance(threat.position);
        distance <= cfg.danger_radius
            || (distance <= cfg.threat_intent_radius && (threat.is_pet || threat.class.is_melee()))
            || (distance <= cfg.threat_intent_radius - margin
                && threat.distance_to(subject.position) > preferred_range(threat) + margin)
    })
}

/// [`ally_in_danger`]'s answer: the window, and the history the next
/// classification reads ([`TeamSituation::in_danger`],
/// [`TeamSituation::danger_cleared_at`]).
struct Danger {
    window: Option<Window>,
    in_danger: SmallVec<[(Entity, f32); 3]>,
    cleared_at: Option<f32>,
}

/// The `ally_in_danger` window: the most injured teammate both dying and
/// pressured, the first in entity order on a tie — debounced at both ends.
///
/// - **Dwell, at an edge only.** A teammate pressured FIRMLY
///   ([`firmly_pressured`]: an enemy well inside the pressure test, such as a
///   Warrior on it) opens its window on the first frame it is dying. One
///   pressured only from the edge of the test opens its window once it has
///   been dying and pressured for [`SituationConfig::danger_dwell`], so an
///   enemy passing a boundary for a frame (a healer stepping across
///   `threat_intent_radius` or `danger_radius`, or to its preferred range)
///   opens nothing.
/// - **Release.** An open window ends only once its teammate has been out of
///   danger for [`SituationConfig::danger_release`], so a one-frame heal or
///   step out of a radius neither ends it nor opens a new one when it comes
///   back. It ends at once when the teammate dies, and gives way at once to
///   another teammate whose own window could open — its dwell counted from
///   when ITS danger began, not from when the first window ended.
///
/// The window's onset carries over while its teammate stays the same, the
/// release included. With no history (`prev` is the default: nothing has
/// been classified), a teammate already in danger is taken to have been so
/// for the dwell — nothing says when it began. A live match classifies from
/// its first frame behind the gates, where nobody is dying, so only a
/// context classified once (a scene's, a test's) ever starts that way.
fn ally_in_danger(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    allies: &[AllyStanding],
    prev: &TeamSituation,
    now: f32,
    cfg: &SituationConfig,
) -> Danger {
    let open = prev.ally_in_danger;
    let in_danger: SmallVec<[(Entity, f32); 3]> = allies
        .iter()
        .filter(|a| a.dying && a.pressured)
        .map(|a| {
            let since = if prev.classified {
                prev.in_danger
                    .iter()
                    .find(|(e, _)| *e == a.entity)
                    .map_or(now, |(_, since)| *since)
            } else {
                // No history: in danger since before anything was seen.
                f32::NEG_INFINITY
            };
            (a.entity, since)
        })
        .collect();
    let mut worst: Option<(Entity, f32)> = None;
    for &(entity, since) in &in_danger {
        let Some(info) = roster.get(&entity) else {
            continue;
        };
        let is_open = open.is_some_and(|w| w.subject == entity);
        if !is_open && now - since < cfg.danger_dwell && !firmly_pressured(roster, auras, info, cfg)
        {
            continue;
        }
        let hp = info.health_pct();
        if worst.is_none_or(|(_, best)| hp < best) {
            worst = Some((entity, hp));
        }
    }
    if let Some((subject, _)) = worst {
        let since = open
            .filter(|w| w.subject == subject)
            .map_or(now, |w| w.since);
        return Danger {
            window: Some(Window {
                subject,
                since,
                until: None,
            }),
            in_danger,
            cleared_at: None,
        };
    }
    // Nobody in danger for the dwell: an open window holds through its
    // release, while its teammate lives.
    let held = open
        .filter(|w| allies.iter().any(|a| a.entity == w.subject))
        .and_then(|w| {
            let cleared_at = prev.danger_cleared_at.unwrap_or(now);
            (now - cleared_at < cfg.danger_release).then_some((w, cleared_at))
        });
    Danger {
        window: held.map(|(w, _)| w),
        in_danger,
        cleared_at: held.map(|(_, cleared_at)| cleared_at),
    }
}

/// The enemy healer `team` would shut down: the first living, non-pet enemy
/// healer in entity order that the team can see. Entity order, and only the
/// FIRST healer, because that is the one every reader of this fact has always
/// meant (`CombatContext::enemy_healer`); a second enemy healer is not
/// consulted.
pub(crate) fn enemy_healer(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
) -> Option<Entity> {
    roster
        .values()
        .find(|info| {
            info.team != team
                && info.is_alive
                && !info.is_pet
                && info.class.is_healer()
                // The observer's own Shadow Sight is per-unit and cannot enter
                // a team classification; it never matters, because only a
                // Rogue stealths (see the module docs).
                && stealth_visible(info.stealthed, false, has_shadow_sight(auras, info.entity))
        })
        .map(|info| info.entity)
}

/// The `lock_ending` window: of the visible, living enemies (not pets) of
/// `team` that its team holds ([`locks_healer`]: the hard crowd control that
/// stops a unit acting), the one whose hold ends SOONEST — `until` is when the
/// longest such crowd control on it runs out. A tie goes to a healer, then to
/// entity order. The onset carries while the subject holds.
fn lock_ending(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
    prev: &TeamSituation,
    now: f32,
) -> Option<Window> {
    let mut best: Option<(f32, bool, Entity)> = None;
    for info in roster.values().filter(|i| {
        i.team != team
            && i.is_alive
            && !i.is_pet
            && stealth_visible(i.stealthed, false, has_shadow_sight(auras, i.entity))
    }) {
        let Some(remaining) = auras.get(&info.entity).and_then(|list| {
            list.iter()
                .filter(|a| locks_healer(a.effect_type))
                .map(|a| a.duration)
                .reduce(f32::max)
        }) else {
            continue;
        };
        let healer = info.class.is_healer();
        let better = best.is_none_or(|(r, h, _)| remaining < r || (remaining == r && healer && !h));
        if better {
            best = Some((remaining, healer, info.entity));
        }
    }
    best.map(|(remaining, _, subject)| Window {
        subject,
        since: prev
            .lock_ending
            .filter(|w| w.subject == subject)
            .map_or(now, |w| w.since),
        until: Some(now + remaining),
    })
}

fn enemy_healer_locked(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
    prev: &TeamSituation,
    now: f32,
) -> Option<Window> {
    let healer = enemy_healer(roster, auras, team)?;
    let remaining = auras
        .get(&healer)?
        .iter()
        .filter(|a| locks_healer(a.effect_type))
        .map(|a| a.duration)
        .reduce(f32::max)?;
    let since = prev
        .enemy_healer_locked
        .filter(|w| w.subject == healer)
        .map_or(now, |w| w.since);
    Some(Window {
        subject: healer,
        since,
        until: Some(now + remaining),
    })
}

/// `team`'s tempo, from its team-HP-fraction advantage (own alive non-pet
/// health fractions minus everyone else's, the sum
/// `CombatContext::team_hp_advantage` takes) and its tempo last frame.
///
/// A lead (deficit) at the onset presses (trails) whatever came before. Short
/// of the onset, the previous tempo holds while the lead (deficit) is still at
/// the release; otherwise the team is even. With the release at the onset —
/// `Legacy` — the hold never fires, and this is the plain [`pressing_when_ahead`]
/// test from each side.
fn tempo(
    roster: &BTreeMap<Entity, CombatantInfo>,
    team: u8,
    prev: Tempo,
    cfg: &SituationConfig,
) -> Tempo {
    let sums = team_hp_sums(roster);
    let own = sums.get(&team).copied().unwrap_or(0.0);
    let enemy: f32 = sums
        .iter()
        .filter(|(t, _)| **t != team)
        .map(|(_, sum)| *sum)
        .sum();
    let advantage = own - enemy;
    latch_tempo(
        advantage,
        prev,
        cfg.press_advantage_margin,
        cfg.press_release_margin,
    )
}

/// [`tempo`]'s latch on a bare advantage: pure, for the band's unit tests.
fn latch_tempo(advantage: f32, prev: Tempo, onset: f32, release: f32) -> Tempo {
    if pressing_when_ahead(advantage, onset) {
        Tempo::Pressing
    } else if pressing_when_ahead(-advantage, onset) {
        Tempo::Trailing
    } else if prev == Tempo::Pressing && pressing_when_ahead(advantage, release) {
        Tempo::Pressing
    } else if prev == Tempo::Trailing && pressing_when_ahead(-advantage, release) {
        Tempo::Trailing
    } else {
        Tempo::Even
    }
}

/// A lock's `until` moves only when a new CC lands, but recomputing it each
/// frame as `now + remaining` wobbles in the last bits. A move of less than
/// this is the same `until`.
const UNTIL_EPSILON: f32 = 0.05;

/// Re-classify both teams from `snapshot`, store the result in `situations`,
/// and record every transition as a `situation_change` trace event. Each team
/// is classified under its own AI profile, the snapshot's, less the plays
/// `cfg.withheld` denies it ([`SituationConfig::for_team`]).
///
/// `decide_abilities` calls it once on the snapshot it builds, and again after
/// each same-frame instant CC that snapshot reflects (see the module docs).
/// The pillar stance is classified here, after the rest, from the stance
/// healer's own view of the snapshot (`pillar_stance::classify`): its
/// crowd-control key reads each enemy's kit from its own ability book.
pub fn update_situations(
    situations: &mut Situations,
    snapshot: &CombatSnapshot,
    clock: &MatchClock,
    cfg: &SituationConfig,
    trace: &mut DecisionTrace,
) {
    for (index, team) in [1u8, 2].into_iter().enumerate() {
        let prev = std::mem::take(&mut situations.teams[index]);
        let cfg = cfg.for_team(team, snapshot.ai_profile.for_team(team));
        let mut next = classify_before_yield(snapshot, team, &prev, clock, &cfg);
        next.hunt = super::class_ai::hunt::classify(snapshot, team, &prev, &next, clock, &cfg);
        let (stance, memory) = pillar_stance::classify(snapshot, team, &prev, &next, clock, &cfg);
        next.pillar_stance = stance;
        next.stance_memory = memory;
        yield_sight(
            &mut next,
            &snapshot.combatants,
            &snapshot.active_auras,
            team,
            &prev,
            clock.now,
            &cfg,
        );
        record_transitions(team, &prev, &next, trace);
        situations.teams[index] = next;
    }
}

/// Emit one `situation_change` event per situation that changed between
/// `prev` and `next`. Transitions only: a frame on which nothing changed
/// records nothing.
fn record_transitions(
    team: u8,
    prev: &TeamSituation,
    next: &TeamSituation,
    trace: &mut DecisionTrace,
) {
    if prev.phase != next.phase {
        trace.record_phase_change(team, prev.phase, next.phase);
    }
    if prev.tempo != next.tempo {
        trace.record_tempo_change(team, prev.tempo, next.tempo);
    }
    if let Some((kind, window)) = window_change(prev.ally_in_danger, next.ally_in_danger) {
        trace.record_window_change(team, SituationName::AllyInDanger, kind, &window);
    }
    if let Some((kind, window)) = window_change(prev.enemy_healer_locked, next.enemy_healer_locked)
    {
        trace.record_window_change(team, SituationName::EnemyHealerLocked, kind, &window);
    }
    if let Some((kind, window)) = window_change(prev.kill_window, next.kill_window) {
        trace.record_window_change(team, SituationName::KillWindow, kind, &window);
    }
    record_sight_yield_changes(team, prev, next, trace);
    if let Some((kind, hunt)) =
        super::class_ai::hunt::change(prev.hunt.as_ref(), next.hunt.as_ref())
    {
        trace.record_hunt_change(team, kind, &hunt);
    }
    if let Some((kind, stance)) = pillar_stance::change(prev.pillar_stance, next.pillar_stance) {
        trace.record_stance_change(team, kind, &stance);
    }
    if let Some((kind, focus)) = target_focus::change(prev.target_focus, next.target_focus) {
        trace.record_focus_change(team, kind, &focus);
    }
}

/// How a window moved between two classifications, if it did: opened, closed,
/// moved to a new subject (a new window), or had its `until` pushed later.
fn window_change(
    prev: Option<Window>,
    next: Option<Window>,
) -> Option<(SituationChangeKind, Window)> {
    match (prev, next) {
        (None, None) => None,
        (None, Some(w)) => Some((SituationChangeKind::Onset, w)),
        (Some(w), None) => Some((SituationChangeKind::End, w)),
        (Some(old), Some(new)) if old.subject != new.subject => {
            Some((SituationChangeKind::Onset, new))
        }
        (Some(old), Some(new)) => match (old.until, new.until) {
            (Some(a), Some(b)) if b > a + UNTIL_EPSILON => {
                Some((SituationChangeKind::Extended, new))
            }
            _ => None,
        },
    }
}

// ---------------------------------------------------------------------------
// sight_yield (AS-386): who keeps a DPS–healer pair in sight of each other.
// ---------------------------------------------------------------------------

/// A DPS below this health fraction yields its healer's sight (the design's
/// `SIGHT_YIELD_HP`, rule 4).
pub const SIGHT_YIELD_HP: f32 = 0.8;

/// A DPS yielding for its health keeps yielding until it is back above this
/// fraction: the release half of [`SIGHT_YIELD_HP`]'s Schmitt pair.
pub const SIGHT_YIELD_RELEASE_HP: f32 = 0.9;

/// Seconds a DPS's yield holds once set, whatever rule set it, unless rule 1
/// (a unit that cannot move) ends it sooner — the pillar modes' `MODE_DWELL`.
pub const SIGHT_YIELD_DWELL: f32 = 2.0;

/// Why a DPS–healer pair is led the way it is: the first rule of the yield
/// rule that held (the design's §3.1). Serializes snake_case.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum YieldRule {
    /// Rule 1: the healer cannot move — hard crowd control, a root, or a cast
    /// in progress — so it cannot follow, and the DPS yields.
    HealerCannotMove,
    /// Rule 1: the DPS cannot move — hard crowd control or a root — so the
    /// healer follows it. A DPS's own cast does not count: it plants the DPS
    /// where it already stands, and flipping the pair's leader for every cast
    /// would only strobe the trace.
    DpsCannotMove,
    /// Rule 2: the healer is pressured, or is the teammate in danger: its DPS
    /// yields (walking round a pillar away from a trained healer is the
    /// opposite of a peel).
    HealerPressured,
    /// Rule 3: the team holds a pillar stance, and every DPS yields to it
    /// ([`stance_leads_sight`]).
    Stance,
    /// Rule 4: the DPS is hurt (below [`SIGHT_YIELD_HP`], held until
    /// [`SIGHT_YIELD_RELEASE_HP`]): it yields. (The teammate in danger is
    /// always hurt: it is dying, below `urgency_hp_threshold`.)
    DpsHurt,
    /// Rule 5: otherwise the DPS leads and the healer follows.
    DpsLeads,
}

impl YieldRule {
    /// Whether, under this rule, the DPS must not break its healer's sight.
    pub fn dps_yields(self) -> bool {
        match self {
            YieldRule::HealerCannotMove
            | YieldRule::HealerPressured
            | YieldRule::Stance
            | YieldRule::DpsHurt => true,
            YieldRule::DpsCannotMove | YieldRule::DpsLeads => false,
        }
    }

    /// Rule 1, which alone may cut a held yield short.
    fn cannot_move(self) -> bool {
        matches!(self, YieldRule::HealerCannotMove | YieldRule::DpsCannotMove)
    }
}

/// One DPS–healer pair's sight yield: whether the DPS must keep its healer
/// in sight, by which rule, and since when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SightPair {
    /// The DPS: a living non-pet teammate that is not a healer.
    pub dps: Entity,
    /// Its healer: the team's first living non-pet healer, in entity order —
    /// the healer the kiter leash already follows.
    pub healer: Entity,
    pub rule: YieldRule,
    /// When the DPS last began yielding (or leading): carried while
    /// [`YieldRule::dps_yields`] gives the same answer, so a yield's dwell is
    /// counted from its onset.
    pub since: f32,
    /// The DPS's health latch ([`SIGHT_YIELD_HP`] / [`SIGHT_YIELD_RELEASE_HP`]).
    pub hurt: bool,
}

impl SightPair {
    /// Whether the DPS must not break its healer's sight.
    pub fn dps_yields(&self) -> bool {
        self.rule.dps_yields()
    }
}

impl TeamSituation {
    /// The pair whose DPS is `dps`, while that DPS yields its healer's sight.
    pub fn sight_yield_of(&self, dps: Entity) -> Option<&SightPair> {
        self.sight_yield
            .iter()
            .find(|p| p.dps == dps && p.dps_yields())
    }
}

/// Rule 3's input: whether the team holds a pillar stance
/// ([`TeamSituation::pillar_stance`], AS-387) — the stance leads, so every
/// DPS keeps its healer in sight. Read off the situation being classified,
/// so the yield follows the stance in the same classification.
fn stance_leads_sight(situation: &TeamSituation) -> bool {
    situation.pillar_stance.is_some()
}

/// Whether `info` cannot move this frame: hard crowd control
/// ([`locks_healer`]'s set) or a root — and, when `cast_counts`, a cast in
/// progress.
fn cannot_move(
    info: &CombatantInfo,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    cast_counts: bool,
) -> bool {
    (cast_counts && info.casting_ability.is_some())
        || auras.get(&info.entity).is_some_and(|list| {
            list.iter()
                .any(|a| locks_healer(a.effect_type) || a.effect_type == AuraType::Root)
        })
}

/// The `sight_yield` classification: one [`SightPair`] per living non-pet
/// DPS on `team`, with the team's first living non-pet healer. Empty when
/// the team does not run it ([`SituationConfig::sight_yield`]) or has no
/// healer. `stance` is rule 3's input ([`stance_leads_sight`]).
#[allow(clippy::too_many_arguments)]
fn classify_sight_yield(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    allies: &[AllyStanding],
    ally_in_danger: Option<Window>,
    stance: bool,
    team: u8,
    prev: &TeamSituation,
    now: f32,
    cfg: &SituationConfig,
) -> SmallVec<[SightPair; 2]> {
    if !cfg.sight_yield {
        return SmallVec::new();
    }
    let living = || {
        roster
            .values()
            .filter(move |i| i.team == team && i.is_alive && !i.is_pet)
    };
    let Some(healer) = living().find(|i| i.class.is_healer()) else {
        return SmallVec::new();
    };
    let in_danger = |e: Entity| ally_in_danger.is_some_and(|w| w.subject == e);
    let healer_pressured = allies
        .iter()
        .any(|a| a.entity == healer.entity && a.pressured)
        || in_danger(healer.entity);
    living()
        .filter(|d| !d.class.is_healer())
        .map(|dps| {
            let held = prev
                .sight_yield
                .iter()
                .find(|p| p.dps == dps.entity && p.healer == healer.entity);
            let hp = dps.health_pct();
            let hurt = hp < SIGHT_YIELD_HP
                || (held.is_some_and(|p| p.hurt) && hp < SIGHT_YIELD_RELEASE_HP);
            let raw = if cannot_move(healer, auras, cfg.yield_counts_healer_cast) {
                YieldRule::HealerCannotMove
            } else if cannot_move(dps, auras, false) {
                YieldRule::DpsCannotMove
            } else if healer_pressured {
                YieldRule::HealerPressured
            } else if stance {
                YieldRule::Stance
            } else if hurt {
                YieldRule::DpsHurt
            } else {
                YieldRule::DpsLeads
            };
            // The dwell: a yield, once set, holds SIGHT_YIELD_DWELL unless
            // rule 1 ends it.
            let rule = match held {
                Some(p)
                    if p.dps_yields()
                        && !raw.dps_yields()
                        && !raw.cannot_move()
                        && now - p.since < SIGHT_YIELD_DWELL =>
                {
                    p.rule
                }
                _ => raw,
            };
            let since = held
                .filter(|p| p.dps_yields() == rule.dps_yields())
                .map_or(now, |p| p.since);
            SightPair {
                dps: dps.entity,
                healer: healer.entity,
                rule,
                since,
                hurt,
            }
        })
        .collect()
}

/// Record a `sight_yield` `situation_change` for each pair whose DPS began
/// yielding (`onset`), stopped (`end`, its pair gone included), or yields
/// under a new rule (`shift`). A leading DPS whose leading rule changes is
/// not recorded: nothing it does changes.
fn record_sight_yield_changes(
    team: u8,
    prev: &TeamSituation,
    next: &TeamSituation,
    trace: &mut DecisionTrace,
) {
    for pair in &next.sight_yield {
        let before = prev
            .sight_yield
            .iter()
            .find(|p| p.dps == pair.dps && p.healer == pair.healer);
        let was = before.is_some_and(|p| p.dps_yields());
        let kind = match (was, pair.dps_yields()) {
            (false, true) => SituationChangeKind::Onset,
            (true, false) => SituationChangeKind::End,
            (true, true) if before.is_some_and(|p| p.rule != pair.rule) => {
                SituationChangeKind::Shift
            }
            _ => continue,
        };
        trace.record_sight_yield_change(team, kind, pair);
    }
    for gone in prev.sight_yield.iter().filter(|p| {
        p.dps_yields()
            && !next
                .sight_yield
                .iter()
                .any(|n| n.dps == p.dps && n.healer == p.healer)
    }) {
        trace.record_sight_yield_change(team, SituationChangeKind::End, gone);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;

    fn unit(entity: Entity, team: u8, class: CharacterClass, hp: f32) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: hp,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
            book: crate::states::play_match::ability_book::AbilityBook::shipped_for(class, None),
        }
    }

    fn cc(effect_type: AuraType, duration: f32) -> Aura {
        Aura {
            effect_type,
            duration,
            ..Default::default()
        }
    }

    struct Fixture {
        roster: BTreeMap<Entity, CombatantInfo>,
        auras: BTreeMap<Entity, Vec<Aura>>,
        warrior: Entity,
        rogue: Entity,
        healer: Entity,
        mage: Entity,
    }

    fn fixture() -> Fixture {
        let mut world = World::new();
        let warrior = world.spawn_empty().id();
        let rogue = world.spawn_empty().id();
        let healer = world.spawn_empty().id();
        let mage = world.spawn_empty().id();
        // Two a side at full health: level, so tempo starts Even.
        let roster = [
            unit(warrior, 1, CharacterClass::Warrior, 100.0),
            unit(rogue, 1, CharacterClass::Rogue, 100.0),
            unit(healer, 2, CharacterClass::Priest, 100.0),
            unit(mage, 2, CharacterClass::Mage, 100.0),
        ]
        .into_iter()
        .map(|u| (u.entity, u))
        .collect();
        Fixture {
            roster,
            auras: BTreeMap::new(),
            warrior,
            rogue,
            healer,
            mage,
        }
    }

    fn classify_now(f: &Fixture, prev: &TeamSituation, now: f32) -> TeamSituation {
        classify_roster(
            &f.roster,
            &f.auras,
            &[],
            1,
            prev,
            &MatchClock::after_gates(now),
            &SituationConfig::default(),
        )
    }

    /// `who` stands at `(x, 0, z)` and targets `whom`.
    fn place(f: &mut Fixture, who: Entity, x: f32, z: f32, whom: Option<Entity>) {
        let info = f.roster.get_mut(&who).unwrap();
        info.position = Vec3::new(x, 0.0, z);
        info.target = whom;
    }

    #[test]
    fn a_root_does_not_lock_the_healer_and_a_polymorph_does() {
        let mut f = fixture();
        f.auras.insert(f.healer, vec![cc(AuraType::Root, 4.0)]);
        assert_eq!(
            classify_now(&f, &TeamSituation::default(), 10.0).enemy_healer_locked,
            None
        );
        f.auras
            .get_mut(&f.healer)
            .unwrap()
            .push(cc(AuraType::Polymorph, 6.0));
        let window = classify_now(&f, &TeamSituation::default(), 10.0)
            .enemy_healer_locked
            .expect("a polymorphed healer is locked");
        assert_eq!(window.subject, f.healer);
        assert_eq!(window.since, 10.0);
        assert_eq!(window.until, Some(16.0), "until is the longest locking CC");
    }

    #[test]
    fn the_onset_survives_reclassification_and_resets_on_a_new_subject() {
        let mut f = fixture();
        f.auras.insert(f.healer, vec![cc(AuraType::Stun, 4.0)]);
        let first = classify_now(&f, &TeamSituation::default(), 10.0);
        let later = classify_now(&f, &first, 11.0);
        assert_eq!(later.enemy_healer_locked.unwrap().since, 10.0);
        let other = TeamSituation {
            enemy_healer_locked: Some(Window {
                subject: Entity::from_raw(999),
                since: 3.0,
                until: None,
            }),
            ..Default::default()
        };
        assert_eq!(
            classify_now(&f, &other, 11.0)
                .enemy_healer_locked
                .unwrap()
                .since,
            11.0
        );
    }

    #[test]
    fn tempo_presses_and_trails_at_the_margin_and_not_inside_it() {
        let mut f = fixture();
        let margin = SituationConfig::default().press_advantage_margin;
        let healer = f.healer;
        f.roster.get_mut(&healer).unwrap().current_health = 100.0 * (1.0 - margin);
        assert_eq!(
            classify_now(&f, &TeamSituation::default(), 0.0).tempo,
            Tempo::Pressing
        );
        let team2 = classify_roster(
            &f.roster,
            &f.auras,
            &[],
            2,
            &TeamSituation::default(),
            &MatchClock::after_gates(0.0),
            &SituationConfig::default(),
        );
        assert_eq!(team2.tempo, Tempo::Trailing);
        f.roster.get_mut(&healer).unwrap().current_health = 100.0 * (1.0 - margin / 2.0);
        assert_eq!(
            classify_now(&f, &TeamSituation::default(), 0.0).tempo,
            Tempo::Even
        );
    }

    #[test]
    fn tempo_latches_inside_the_band_and_releases_below_it() {
        let cfg = SituationConfig::default();
        let (onset, release) = (cfg.press_advantage_margin, cfg.press_release_margin);
        assert!(release < onset, "the shipped config has a band");
        let inside = (onset + release) / 2.0;
        // From each tempo, a lead inside the band keeps it...
        for prev in [Tempo::Pressing, Tempo::Even, Tempo::Trailing] {
            let expected = if prev == Tempo::Pressing {
                Tempo::Pressing
            } else {
                Tempo::Even
            };
            assert_eq!(latch_tempo(inside, prev, onset, release), expected);
        }
        assert_eq!(
            latch_tempo(-inside, Tempo::Trailing, onset, release),
            Tempo::Trailing
        );
        // ...at the release it still holds; below it, it lets go.
        assert_eq!(
            latch_tempo(release, Tempo::Pressing, onset, release),
            Tempo::Pressing
        );
        assert_eq!(
            latch_tempo(release - 1e-4, Tempo::Pressing, onset, release),
            Tempo::Even
        );
        assert_eq!(
            latch_tempo(-(release - 1e-4), Tempo::Trailing, onset, release),
            Tempo::Even
        );
        // The onset wins over whatever came before: a swing straight across.
        assert_eq!(
            latch_tempo(-onset, Tempo::Pressing, onset, release),
            Tempo::Trailing
        );
    }

    #[test]
    fn legacy_has_no_band() {
        let teamplan = SituationConfig::default().under(AiProfile::TeamPlan);
        let legacy = SituationConfig::default().under(AiProfile::Legacy);
        assert_eq!(
            teamplan,
            SituationConfig {
                runs: PlaySet::all(),
                pillar_modes: true,
                sight_yield: true,
                target_focus: true,
                ..SituationConfig::default()
            }
        );
        assert_eq!(legacy.press_release_margin, legacy.press_advantage_margin);
        assert_eq!(
            legacy,
            SituationConfig {
                press_release_margin: teamplan.press_advantage_margin,
                healers_close: true,
                runs: PlaySet::default(),
                pillar_modes: false,
                sight_yield: false,
                target_focus: false,
                ..teamplan
            },
            "only the release, the closing rule, the gated plays, the pillar modes, the \
             sight yield and the target focus differ"
        );
        // With no band the latch is the plain test from each side, whatever
        // the previous tempo.
        let m = legacy.press_advantage_margin;
        for advantage in [-0.5, -m, -m + 1e-4, 0.0, m - 1e-4, m, 0.5] {
            let plain = if pressing_when_ahead(advantage, m) {
                Tempo::Pressing
            } else if pressing_when_ahead(-advantage, m) {
                Tempo::Trailing
            } else {
                Tempo::Even
            };
            for prev in [Tempo::Pressing, Tempo::Even, Tempo::Trailing] {
                assert_eq!(
                    latch_tempo(advantage, prev, m, m),
                    plain,
                    "{advantage} {prev:?}"
                );
            }
        }
    }

    #[test]
    fn transitions_are_recorded_once_each() {
        let mut f = fixture();
        let mut trace = DecisionTrace::default();
        let quiet = classify_now(&f, &TeamSituation::default(), 5.0);
        f.auras.insert(f.healer, vec![cc(AuraType::Fear, 8.0)]);
        let locked = classify_now(&f, &quiet, 5.0);
        record_transitions(1, &quiet, &locked, &mut trace);
        record_transitions(1, &locked, &classify_now(&f, &locked, 5.0), &mut trace);
        record_transitions(1, &locked, &quiet, &mut trace);
        let changes: Vec<String> = trace
            .pending_events
            .iter()
            .map(|e| serde_json::to_value(e).unwrap()["change"].to_string())
            .collect();
        assert_eq!(changes, vec!["\"onset\"", "\"end\""]);
    }

    /// Burst answers a kill window, for a DPS on a team that runs the play.
    /// Each condition removed gives the Default play back: no window, a team
    /// that does not run it, a healer. A locked enemy healer alone — the
    /// situation the retired lock-window rules answered — is not a Burst.
    #[test]
    fn only_melee_and_ranged_dps_burst_and_only_into_a_kill_window() {
        let window = Window {
            subject: Entity::from_raw(1),
            since: 0.0,
            until: None,
        };
        let open = TeamSituation {
            kill_window: Some(window),
            runs: PlaySet::of(&[Play::Burst]),
            ..Default::default()
        };
        let me = Entity::from_raw(9);
        assert_eq!(open.play_for(Role::MeleeDps, me), Play::Burst);
        assert_eq!(open.play_for(Role::RangedDps, me), Play::Burst);
        assert_eq!(open.play_for(Role::Healer, me), Play::Default);
        let gated = TeamSituation {
            runs: PlaySet::default(),
            ..open.clone()
        };
        assert_eq!(gated.play_for(Role::MeleeDps, me), Play::Default);
        let closed = TeamSituation {
            kill_window: None,
            enemy_healer_locked: Some(window),
            ..open.clone()
        };
        assert_eq!(closed.play_for(Role::RangedDps, me), Play::Default);
    }

    /// A class [`BURST_EXCEPTIONS`] (or the measurement override) names does
    /// not burst; every other class of a team that runs the play does.
    #[test]
    fn a_burst_exception_names_a_class_that_does_not_burst() {
        let mut withheld = WithheldPlays::default();
        withheld.exceptions_mut(1).set(
            PlayId::Burst,
            ClassSet::of(&[CharacterClass::Hunter]).into(),
        );
        let cfg = SituationConfig {
            withheld,
            ..SituationConfig::default()
        }
        .for_team(1, AiProfile::TeamPlan);
        let f = fixture();
        let s = classify_roster(
            &f.roster,
            &f.auras,
            &[],
            1,
            &TeamSituation::default(),
            &MatchClock::after_gates(30.0),
            &cfg,
        );
        let burst = s.exceptions.of(PlayId::Burst);
        assert!(burst.contains(ExceptionKey::class(CharacterClass::Hunter)));
        assert!(!burst.contains(ExceptionKey::class(CharacterClass::Warrior)));
        // The shipped exceptions, unless overridden; an override for one team
        // only.
        assert_eq!(
            PlayExceptions::resolve(&ExceptionOverrides::default()).of(PlayId::Burst),
            plays::stage(PlayId::Burst).exceptions()
        );
        assert_eq!(
            SituationConfig {
                withheld,
                ..SituationConfig::default()
            }
            .for_team(2, AiProfile::TeamPlan)
            .exceptions,
            ExceptionOverrides::default()
        );
        let mut lifted = ExceptionOverrides::default();
        lifted.lift(PlayId::Burst);
        assert_eq!(
            PlayExceptions::resolve(&lifted).of(PlayId::Burst),
            ExceptionSet::default()
        );
    }

    /// Peel heads every role's play while a teammate is in danger with an
    /// attacker to peel — on a team that runs it, for every unit but that
    /// teammate — and beats a burst. Each condition removed gives the play
    /// back to the fallback.
    #[test]
    fn peel_answers_a_teammate_in_danger_and_only_on_a_team_that_runs_it() {
        let ally = Entity::from_raw(1);
        let attacker = Entity::from_raw(2);
        let me = Entity::from_raw(3);
        let danger = TeamSituation {
            ally_in_danger: Some(Window {
                subject: ally,
                since: 0.0,
                until: None,
            }),
            danger_attackers: [attacker].into_iter().collect(),
            kill_window: Some(Window {
                subject: Entity::from_raw(4),
                since: 0.0,
                until: None,
            }),
            runs: PlaySet::of(&[PEEL, Play::Burst]),
            ..Default::default()
        };
        for role in [Role::MeleeDps, Role::RangedDps, Role::Healer] {
            assert_eq!(danger.play_for(role, me), Play::Peel { ally }, "{role:?}");
            assert_eq!(
                danger.play_for(role, ally),
                danger.play_after(PlayId::Chain, role, ally),
                "the teammate in danger does not peel for itself"
            );
            let gated = TeamSituation {
                runs: PlaySet::of(&[Play::Burst]),
                ..danger.clone()
            };
            assert_eq!(
                gated.play_for(role, me),
                gated.play_after(PlayId::Chain, role, me)
            );
            let unattacked = TeamSituation {
                danger_attackers: SmallVec::new(),
                ..danger.clone()
            };
            assert_eq!(
                unattacked.play_for(role, me),
                unattacked.play_after(PlayId::Chain, role, me)
            );
        }
        assert_eq!(
            danger.play_after(PlayId::Chain, Role::MeleeDps, me),
            Play::Burst
        );
    }

    /// The danger's attackers are the enemies pressuring its subject, nearest
    /// first; a distant caster holding position is not one.
    #[test]
    fn danger_attackers_are_the_pressuring_enemies_nearest_first() {
        let mut f = fixture();
        let cfg = SituationConfig::default();
        let (warrior, healer, mage) = (f.warrior, f.healer, f.mage);
        f.roster.get_mut(&warrior).unwrap().current_health = 20.0;
        place(&mut f, warrior, 0.0, 0.0, None);
        // The Priest at 9yd (inside danger_radius), the Mage at 6yd.
        place(&mut f, healer, 9.0, 0.0, Some(warrior));
        place(&mut f, mage, 6.0, 0.0, Some(warrior));
        let s = classify_now(&f, &TeamSituation::default(), 30.0);
        assert_eq!(s.ally_in_danger.map(|w| w.subject), Some(f.warrior));
        assert_eq!(s.danger_attackers.as_slice(), &[f.mage, f.healer]);
        // The Mage backs off to its own range and holds: no longer pressure.
        place(&mut f, mage, cfg.danger_radius + 10.0, 0.0, Some(warrior));
        let s = classify_now(&f, &TeamSituation::default(), 30.0);
        assert_eq!(s.danger_attackers.as_slice(), &[f.healer]);
        // Nobody in danger: no attackers.
        f.roster.get_mut(&f.warrior).unwrap().current_health = 100.0;
        let s = classify_now(&f, &TeamSituation::default(), 30.0);
        assert!(s.danger_attackers.is_empty());
    }

    /// The plays graduated at the 0.8 sweep, by name; the rest are gated.
    /// A play that graduates (or is deleted) changes this list.
    const GRADUATED: [PlayId; 4] = [PlayId::Peel, PlayId::Hunt, PlayId::Chain, PlayId::Burst];

    /// A graduated play runs under `TeamPlan` for both teams, never under
    /// `Legacy`, and no config can withhold it; the gated Pillar can be
    /// withheld from one team only.
    #[test]
    fn graduated_plays_cannot_be_withheld_and_a_gated_one_can() {
        let gated: Vec<PlayId> = PlayId::ALL.into_iter().filter(|id| id.is_gated()).collect();
        assert_eq!(gated, vec![PlayId::Pillar]);
        let cfg = SituationConfig::default();
        for id in GRADUATED {
            assert!(!id.is_gated(), "{id:?}");
            for team in [1, 2] {
                assert!(cfg.for_team(team, AiProfile::TeamPlan).runs.contains(id));
                assert!(!cfg.for_team(team, AiProfile::Legacy).runs.contains(id));
            }
            let name = plays::stage(id).config_key().to_string();
            let refused = WithheldPlays::parse_set(std::slice::from_ref(&name)).unwrap_err();
            assert!(refused.contains("graduated"), "{refused}");
        }
        assert_eq!(
            WithheldPlays::parse_set(&["Pillar".to_string()]),
            Ok(PlaySet::of(&[Play::Pillar]))
        );
        assert!(WithheldPlays::parse_set(&["Default".to_string()]).is_err());
        assert!(WithheldPlays::parse_set(&["TakeCover".to_string()]).is_err());
        let withheld = SituationConfig {
            withheld: WithheldPlays {
                team2: PlaySet::of(&[Play::Pillar]),
                ..Default::default()
            },
            ..cfg
        };
        assert!(withheld
            .for_team(1, AiProfile::TeamPlan)
            .runs
            .contains(PlayId::Pillar));
        let team2 = withheld.for_team(2, AiProfile::TeamPlan).runs;
        assert!(!team2.contains(PlayId::Pillar));
        assert!(GRADUATED.iter().all(|id| team2.contains(*id)));
    }

    /// A graduated play withheld in code, past the config's refusal, is a
    /// panic, never a silent control.
    #[test]
    #[should_panic(expected = "graduated and cannot be withheld")]
    fn withholding_a_graduated_play_in_code_panics() {
        let cfg = SituationConfig {
            withheld: WithheldPlays {
                team1: PlaySet::of(&[Play::Burst]),
                ..Default::default()
            },
            ..SituationConfig::default()
        };
        let _ = cfg.for_team(1, AiProfile::TeamPlan);
    }

    // -----------------------------------------------------------------------
    // lock_ending and the Chain play
    // -----------------------------------------------------------------------

    /// The lock a chain hands off from is the enemy whose hold ends
    /// SOONEST; its `until` is the longest hold on that enemy; a tie goes to
    /// the healer; a root holds nothing; the onset carries while the subject
    /// holds.
    #[test]
    fn lock_ending_is_the_enemy_hold_that_ends_soonest() {
        let mut f = fixture();
        let none = classify_now(&f, &TeamSituation::default(), 30.0);
        assert_eq!(none.lock_ending, None);
        f.auras.insert(f.mage, vec![cc(AuraType::Root, 1.0)]);
        assert_eq!(classify_now(&f, &none, 30.0).lock_ending, None, "a root");
        f.auras.insert(
            f.mage,
            vec![cc(AuraType::Stun, 1.0), cc(AuraType::Polymorph, 2.0)],
        );
        f.auras.insert(f.healer, vec![cc(AuraType::Fear, 3.0)]);
        let s = classify_now(&f, &none, 30.0);
        assert_eq!(
            s.lock_ending,
            Some(Window {
                subject: f.mage,
                since: 30.0,
                until: Some(32.0),
            }),
            "the Mage's longest hold, 2s, ends before the Priest's 3s"
        );
        let later = classify_now(&f, &s, 31.0);
        assert_eq!(later.lock_ending.map(|w| w.since), Some(30.0));
        f.auras.insert(f.healer, vec![cc(AuraType::Fear, 2.0)]);
        // The healer AFTER the other enemy in entity order, so entity order
        // alone would pick the other one.
        f.roster.get_mut(&f.healer).unwrap().class = CharacterClass::Mage;
        f.roster.get_mut(&f.mage).unwrap().class = CharacterClass::Priest;
        assert_eq!(
            classify_now(&f, &none, 30.0).lock_ending.map(|w| w.subject),
            Some(f.mage),
            "a tie goes to the healer"
        );
        // Its own team's hold is no lock it hands off from.
        f.auras.clear();
        f.auras.insert(f.warrior, vec![cc(AuraType::Stun, 1.0)]);
        assert_eq!(classify_now(&f, &none, 30.0).lock_ending, None);
    }

    /// The Chain play heads the play of every role — a healer's only while no
    /// teammate of its own is dying — once the lock is within the horizon of
    /// its end, on a team that runs it, ahead of a burst and behind a peel.
    #[test]
    fn chain_hands_off_a_lock_ending_within_the_horizon() {
        let subject = Entity::from_raw(1);
        let me = Entity::from_raw(3);
        let lock = |until: f32| Window {
            subject,
            since: 0.0,
            until: Some(until),
        };
        let ending = TeamSituation {
            lock_ending: Some(lock(30.0 + CHAIN_HORIZON_SECS)),
            now: 30.0,
            runs: PlaySet::of(&[CHAIN, Play::Burst]),
            kill_window: Some(lock(31.0)),
            ..Default::default()
        };
        for role in [Role::MeleeDps, Role::RangedDps, Role::Healer] {
            assert_eq!(
                ending.play_for(role, me),
                Play::Chain { subject },
                "{role:?}"
            );
        }
        assert_eq!(
            ending.play_after(PlayId::Chain, Role::MeleeDps, me),
            Play::Burst
        );
        let far = TeamSituation {
            lock_ending: Some(lock(30.01 + CHAIN_HORIZON_SECS)),
            ..ending.clone()
        };
        assert_eq!(far.play_for(Role::RangedDps, me), Play::Burst);
        assert_eq!(far.play_for(Role::Healer, me), Play::Default);
        let gated = TeamSituation {
            runs: PlaySet::of(&[Play::Burst]),
            ..ending.clone()
        };
        assert_eq!(gated.play_for(Role::Healer, me), Play::Default);
        let dying = TeamSituation {
            allies: [AllyStanding {
                entity: me,
                dying: true,
                pressured: false,
                pressure_onset: false,
            }]
            .into_iter()
            .collect(),
            ..ending.clone()
        };
        assert_eq!(dying.play_for(Role::Healer, me), Play::Default);
        assert_eq!(dying.play_for(Role::MeleeDps, me), Play::Chain { subject });
        let danger = TeamSituation {
            ally_in_danger: Some(lock(0.0)),
            danger_attackers: [Entity::from_raw(2)].into_iter().collect(),
            runs: PlaySet::of(&[PEEL, CHAIN, Play::Burst]),
            ..ending.clone()
        };
        assert_eq!(
            danger.play_for(Role::MeleeDps, me),
            Play::Peel { ally: subject }
        );
    }

    // -----------------------------------------------------------------------
    // kill_window
    // -----------------------------------------------------------------------

    fn set_hp(f: &mut Fixture, who: Entity, hp: f32) {
        f.roster.get_mut(&who).unwrap().current_health = hp;
    }

    /// The Mage (team 2's DPS) low at 20%, its Priest 10yd off: the window
    /// opens on the Mage only when the Priest cannot heal it — and each way
    /// it cannot is its own pair.
    #[test]
    fn a_kill_window_is_a_low_enemy_its_healer_cannot_heal() {
        let mut f = fixture();
        let (healer, mage) = (f.healer, f.mage);
        set_hp(&mut f, mage, 20.0);
        place(&mut f, mage, 0.0, 0.0, None);
        place(&mut f, healer, 10.0, 0.0, None);
        let quiet = TeamSituation::default();
        // Free and in reach: no window.
        assert_eq!(classify_now(&f, &quiet, 30.0).kill_window, None);
        // Locked: a window until the lock ends.
        f.auras.insert(healer, vec![cc(AuraType::Polymorph, 4.0)]);
        let w = classify_now(&f, &quiet, 30.0)
            .kill_window
            .expect("a locked healer opens a kill window");
        assert_eq!((w.subject, w.since, w.until), (mage, 30.0, Some(34.0)));
        // A root does not lock: no window.
        f.auras.insert(healer, vec![cc(AuraType::Root, 4.0)]);
        assert_eq!(classify_now(&f, &quiet, 30.0).kill_window, None);
        f.auras.clear();
        // Out of mana: a window with no known end.
        let cfg = SituationConfig::default();
        f.roster.get_mut(&healer).unwrap().current_mana = cfg.kill_oom_mana - 1.0;
        let w = classify_now(&f, &quiet, 30.0).kill_window.expect("OOM");
        assert_eq!((w.subject, w.until), (mage, None));
        f.roster.get_mut(&healer).unwrap().current_mana = cfg.kill_oom_mana;
        assert_eq!(classify_now(&f, &quiet, 30.0).kill_window, None);
        // Out of heal range: a window; at the range, none.
        place(&mut f, healer, cfg.heal_range + 0.5, 0.0, None);
        assert!(classify_now(&f, &quiet, 30.0).kill_window.is_some());
        place(&mut f, healer, cfg.heal_range - 0.5, 0.0, None);
        assert_eq!(classify_now(&f, &quiet, 30.0).kill_window, None);
        // Out of sight behind a pillar: a window.
        let pillar = ObstacleVolume::Cylinder {
            center_xz: Vec2::new(5.0, 0.0),
            radius: 1.5,
            base_y: -1.0,
            height: 10.0,
        };
        place(&mut f, healer, 10.0, 0.0, None);
        let behind = classify_roster(
            &f.roster,
            &f.auras,
            &[pillar],
            1,
            &quiet,
            &MatchClock::after_gates(30.0),
            &cfg,
        );
        assert_eq!(behind.kill_window.map(|w| w.subject), Some(mage));
    }

    /// Low is below `kill_hp_threshold` to open, and an open window holds its
    /// subject until it is back above `kill_hp_release`, keeping its onset.
    #[test]
    fn the_kill_window_opens_below_its_threshold_and_releases_above_its_release() {
        let cfg = SituationConfig::default();
        assert!(cfg.kill_hp_release > cfg.kill_hp_threshold, "a band ships");
        let mut f = fixture();
        let (healer, mage) = (f.healer, f.mage);
        f.auras.insert(healer, vec![cc(AuraType::Stun, 6.0)]);
        set_hp(&mut f, mage, 100.0 * cfg.kill_hp_threshold);
        let quiet = classify_now(&f, &TeamSituation::default(), 30.0);
        assert_eq!(quiet.kill_window, None, "strictly below the threshold");
        set_hp(&mut f, mage, 100.0 * cfg.kill_hp_threshold - 1.0);
        let open = classify_now(&f, &quiet, 30.5);
        assert_eq!(open.kill_window.map(|w| w.since), Some(30.5));
        // Healed into the band: held, onset kept.
        set_hp(
            &mut f,
            mage,
            100.0 * (cfg.kill_hp_threshold + cfg.kill_hp_release) / 2.0,
        );
        let held = classify_now(&f, &open, 31.0);
        assert_eq!(
            held.kill_window.map(|w| (w.subject, w.since)),
            Some((mage, 30.5))
        );
        // ...but the same health with no window open opens nothing.
        assert_eq!(classify_now(&f, &quiet, 31.0).kill_window, None);
        // Above the release: closed.
        set_hp(&mut f, mage, 100.0 * cfg.kill_hp_release + 1.0);
        assert_eq!(classify_now(&f, &held, 31.5).kill_window, None);
    }

    /// The window names the MOST injured qualifying enemy, never a pet or an
    /// enemy the team cannot see; a healer that is itself low and locked is
    /// one (it cannot heal itself), and one that is free is not.
    #[test]
    fn the_kill_window_names_the_most_injured_enemy_it_can_see() {
        let cfg = SituationConfig::default();
        let mut f = fixture();
        let (healer, mage) = (f.healer, f.mage);
        set_hp(&mut f, mage, 30.0);
        set_hp(&mut f, healer, 10.0);
        let quiet = TeamSituation::default();
        // The Priest free: it heals itself and the Mage.
        assert_eq!(classify_now(&f, &quiet, 30.0).kill_window, None);
        f.auras.insert(healer, vec![cc(AuraType::Fear, 3.0)]);
        let w = classify_now(&f, &quiet, 30.0).kill_window.unwrap();
        assert_eq!((w.subject, w.until), (healer, Some(33.0)));
        // Stealthed, the Priest is not seen: the Mage instead.
        f.roster.get_mut(&healer).unwrap().stealthed = true;
        assert_eq!(
            classify_now(&f, &quiet, 30.0)
                .kill_window
                .map(|w| w.subject),
            Some(mage)
        );
        f.roster.get_mut(&healer).unwrap().stealthed = false;
        // A pet is never the subject.
        f.roster.get_mut(&healer).unwrap().is_pet = true;
        set_hp(&mut f, mage, 100.0);
        let s = classify_now(&f, &quiet, 30.0);
        assert_eq!(
            s.kill_window.map(|w| w.subject),
            None,
            "no non-pet enemy is low ({})",
            cfg.kill_hp_threshold
        );
    }

    /// A team with no living healer heals nothing: a low enemy is a kill
    /// window with no known end. A second healer that can still heal closes
    /// it; with both locked, `until` is the first to come free.
    #[test]
    fn every_enemy_healer_must_be_unable_and_until_is_the_first_to_return() {
        let mut f = fixture();
        let (healer, mage, rogue) = (f.healer, f.mage, f.rogue);
        set_hp(&mut f, mage, 20.0);
        f.roster.get_mut(&healer).unwrap().is_alive = false;
        let quiet = TeamSituation::default();
        let w = classify_now(&f, &quiet, 30.0).kill_window.unwrap();
        assert_eq!((w.subject, w.until), (mage, None));
        // Two enemy healers: the Priest (alive again) and the Rogue's slot
        // moved to team 2 as a Druid.
        f.roster.get_mut(&healer).unwrap().is_alive = true;
        {
            let druid = f.roster.get_mut(&rogue).unwrap();
            druid.team = 2;
            druid.class = CharacterClass::Druid;
        }
        f.auras.insert(healer, vec![cc(AuraType::Stun, 5.0)]);
        assert_eq!(
            classify_now(&f, &quiet, 30.0).kill_window,
            None,
            "the free Druid heals"
        );
        f.auras.insert(rogue, vec![cc(AuraType::Polymorph, 2.0)]);
        let w = classify_now(&f, &quiet, 30.0).kill_window.unwrap();
        assert_eq!(w.until, Some(32.0), "the Druid comes free first");
    }

    #[test]
    fn kill_window_transitions_are_recorded() {
        let mut f = fixture();
        let mut trace = DecisionTrace::default();
        let (healer, mage) = (f.healer, f.mage);
        let quiet = classify_now(&f, &TeamSituation::default(), 5.0);
        set_hp(&mut f, mage, 10.0);
        f.auras.insert(healer, vec![cc(AuraType::Stun, 3.0)]);
        let open = classify_now(&f, &quiet, 5.0);
        record_transitions(1, &quiet, &open, &mut trace);
        let names: Vec<(String, String)> = trace
            .pending_events
            .iter()
            .map(|e| {
                let v = serde_json::to_value(e).unwrap();
                (v["name"].to_string(), v["change"].to_string())
            })
            .filter(|(n, _)| n == "\"kill_window\"")
            .collect();
        assert_eq!(
            names,
            vec![("\"kill_window\"".to_string(), "\"onset\"".to_string())]
        );
    }

    #[test]
    fn dying_is_below_the_urgency_threshold_and_pets_and_the_dead_are_not_allies() {
        let mut f = fixture();
        let urgency = SituationConfig::default().urgency_hp_threshold;
        let (warrior, rogue) = (f.warrior, f.rogue);
        f.roster.get_mut(&warrior).unwrap().current_health = 100.0 * urgency;
        f.roster.get_mut(&rogue).unwrap().current_health = 100.0 * urgency - 1.0;
        let s = classify_now(&f, &TeamSituation::default(), 30.0);
        assert_eq!(
            s.dying_allies().collect::<Vec<_>>(),
            vec![rogue],
            "strictly below the threshold"
        );
        f.roster.get_mut(&rogue).unwrap().is_alive = false;
        let s = classify_now(&f, &TeamSituation::default(), 30.0);
        assert_eq!(s.allies.len(), 1, "a dead teammate is not an ally");
        f.roster.get_mut(&warrior).unwrap().is_pet = true;
        assert!(classify_now(&f, &TeamSituation::default(), 30.0)
            .allies
            .is_empty());
    }

    #[test]
    fn pressure_is_proximity_or_a_melee_pet_or_closing_threat_within_intent() {
        let mut f = fixture();
        let cfg = SituationConfig::default();
        let (warrior, healer, mage) = (f.warrior, f.healer, f.mage);
        // An enemy caster (a Warlock, whose preferred range sits between the
        // two radii) targets the Warrior from inside the danger radius.
        f.roster.get_mut(&mage).unwrap().class = CharacterClass::Warlock;
        place(&mut f, mage, cfg.danger_radius - 1.0, 0.0, Some(warrior));
        assert!(classify_now(&f, &TeamSituation::default(), 30.0).is_pressured(warrior));
        // From its own preferred range, beyond the danger radius but inside
        // the intent radius: a caster holding position does not pressure.
        let held = CharacterClass::Warlock.preferred_range();
        assert!(held > cfg.danger_radius && held < cfg.threat_intent_radius);
        place(&mut f, mage, held, 0.0, Some(warrior));
        assert!(!classify_now(&f, &TeamSituation::default(), 30.0).is_pressured(warrior));
        // ...but still closing to it does.
        place(&mut f, mage, held + 1.0, 0.0, Some(warrior));
        assert!(classify_now(&f, &TeamSituation::default(), 30.0).is_pressured(warrior));
        // A melee at the same range pressures; beyond the intent radius
        // nothing does.
        place(&mut f, mage, 0.0, 0.0, None);
        f.roster.get_mut(&healer).unwrap().class = CharacterClass::Paladin;
        place(&mut f, healer, held, 0.0, Some(warrior));
        assert!(classify_now(&f, &TeamSituation::default(), 30.0).is_pressured(warrior));
        place(
            &mut f,
            healer,
            cfg.threat_intent_radius + 1.0,
            0.0,
            Some(warrior),
        );
        assert!(!classify_now(&f, &TeamSituation::default(), 30.0).is_pressured(warrior));
    }

    #[test]
    fn pressure_is_judged_from_the_teammates_own_view() {
        let mut f = fixture();
        let (warrior, rogue, mage) = (f.warrior, f.rogue, f.mage);
        // A stealthed enemy on the Warrior: team 2's Mage, stealthed for the
        // test (the rule does not care which class stealths).
        place(&mut f, mage, 1.0, 0.0, Some(warrior));
        f.roster.get_mut(&mage).unwrap().stealthed = true;
        let unseen = classify_now(&f, &TeamSituation::default(), 30.0);
        assert!(!unseen.is_pressured(warrior), "an unseen attacker");
        // The Warrior's own Shadow Sight reveals it to the Warrior...
        f.auras
            .insert(warrior, vec![cc(AuraType::ShadowSight, 10.0)]);
        assert!(classify_now(&f, &TeamSituation::default(), 30.0).is_pressured(warrior));
        // ...a teammate's does not.
        f.auras.remove(&warrior);
        f.auras.insert(rogue, vec![cc(AuraType::ShadowSight, 10.0)]);
        assert!(!classify_now(&f, &TeamSituation::default(), 30.0).is_pressured(warrior));
    }

    /// The onset of pressure gives the closing branch width: a caster closing
    /// from inside `danger_edge_margin` of its preferred range, or from the
    /// last margin inside `threat_intent_radius`, holds the posture but does
    /// not start it. Proximity, melee and pets onset exactly where they
    /// pressure.
    #[test]
    fn pressure_onset_gives_the_closing_branch_width_and_nothing_else() {
        let mut f = fixture();
        let cfg = SituationConfig::default();
        let m = cfg.danger_edge_margin;
        let (warrior, healer, mage) = (f.warrior, f.healer, f.mage);
        let at = |f: &Fixture| {
            let s = classify_now(f, &TeamSituation::default(), 30.0);
            (s.is_pressured(warrior), s.is_pressure_onset(warrior))
        };
        // A Hunter, whose preferred range (25) leaves a closing band with a
        // middle (26..29), so all three of its parts can be shown.
        f.roster.get_mut(&mage).unwrap().class = CharacterClass::Hunter;
        let pref = CharacterClass::Hunter.preferred_range();
        place(&mut f, mage, pref + m * 0.5, 0.0, Some(warrior));
        assert_eq!(
            at(&f),
            (true, false),
            "closing, at the preferred-range edge"
        );
        place(&mut f, mage, pref + m + 1.0, 0.0, Some(warrior));
        assert_eq!(at(&f), (true, true), "closing, firmly inside both edges");
        place(
            &mut f,
            mage,
            cfg.threat_intent_radius - m * 0.5,
            0.0,
            Some(warrior),
        );
        assert_eq!(at(&f), (true, false), "closing, at the intent-radius edge");
        // A 28yd caster's band is 2yd wide: it never onsets by closing.
        f.roster.get_mut(&mage).unwrap().class = CharacterClass::Warlock;
        for d in [28.5, 29.0, 29.5, 30.0] {
            place(&mut f, mage, d, 0.0, Some(warrior));
            assert_eq!(at(&f), (true, false), "a 28yd caster closing at {d}");
        }
        // Inside the danger radius, at its very edge: onset unchanged.
        place(&mut f, mage, cfg.danger_radius, 0.0, Some(warrior));
        assert_eq!(at(&f), (true, true), "proximity");
        // A melee at the intent radius's very edge: onset unchanged.
        place(&mut f, mage, 0.0, 0.0, None);
        f.roster.get_mut(&healer).unwrap().class = CharacterClass::Rogue;
        place(&mut f, healer, cfg.threat_intent_radius, 0.0, Some(warrior));
        assert_eq!(at(&f), (true, true), "melee");
    }

    /// An enemy healer in its closing band — beyond its 28yd preferred range,
    /// inside the 30yd intent radius — is not closing under `TeamPlan`, and
    /// so pressures nothing; a Warlock in the same spot is, and does. Under
    /// `Legacy` the healer still counts, as it always has. Inside
    /// `danger_radius` the healer pressures under both.
    #[test]
    fn an_enemy_healer_never_closes_except_under_legacy() {
        let mut f = fixture();
        let warrior = f.warrior;
        let healer = f.healer;
        let at = |f: &Fixture, profile: AiProfile| {
            let s = classify_roster(
                &f.roster,
                &f.auras,
                &[],
                1,
                &TeamSituation::default(),
                &MatchClock::after_gates(30.0),
                &SituationConfig::default().under(profile),
            );
            (s.is_pressured(warrior), s.is_pressure_onset(warrior))
        };
        let band = CharacterClass::Priest.preferred_range() + 1.0;
        let cfg = SituationConfig::default();
        assert!(band < cfg.threat_intent_radius);
        for class in [
            CharacterClass::Priest,
            CharacterClass::Shaman,
            CharacterClass::Druid,
        ] {
            f.roster.get_mut(&healer).unwrap().class = class;
            place(&mut f, healer, band, 0.0, Some(warrior));
            assert_eq!(at(&f, AiProfile::TeamPlan), (false, false), "{class:?}");
            assert!(at(&f, AiProfile::Legacy).0, "{class:?} under Legacy");
            place(&mut f, healer, cfg.danger_radius, 0.0, Some(warrior));
            assert_eq!(
                at(&f, AiProfile::TeamPlan),
                (true, true),
                "{class:?} close in"
            );
        }
        // The control: a ranged DPS in the same spot is closing.
        f.roster.get_mut(&healer).unwrap().class = CharacterClass::Warlock;
        place(&mut f, healer, band, 0.0, Some(warrior));
        assert!(at(&f, AiProfile::TeamPlan).0, "a Warlock closing");
    }

    #[test]
    fn ally_in_danger_is_the_most_injured_teammate_both_dying_and_pressured() {
        let mut f = fixture();
        let (warrior, rogue, healer, mage) = (f.warrior, f.rogue, f.healer, f.mage);
        f.roster.get_mut(&warrior).unwrap().current_health = 30.0;
        f.roster.get_mut(&rogue).unwrap().current_health = 20.0;
        // Both dying; only the Warrior pressured.
        place(&mut f, mage, 1.0, 0.0, Some(warrior));
        let first = classify_now(&f, &TeamSituation::default(), 30.0);
        let window = first.ally_in_danger.expect("the Warrior is in danger");
        assert_eq!((window.subject, window.since), (warrior, 30.0));
        // The onset carries while the subject holds.
        let later = classify_now(&f, &first, 31.0);
        assert_eq!(later.ally_in_danger.unwrap().since, 30.0);
        // The Rogue firmly pressured too, and lower: a new window on it at
        // once.
        place(&mut f, healer, 1.0, 0.0, Some(rogue));
        let moved = classify_now(&f, &later, 32.0);
        let window = moved.ally_in_danger.unwrap();
        assert_eq!((window.subject, window.since), (rogue, 32.0));
        // Healthy again: no window once the release has run, though still
        // pressured.
        let release = SituationConfig::default().danger_release;
        f.roster.get_mut(&warrior).unwrap().current_health = 100.0;
        f.roster.get_mut(&rogue).unwrap().current_health = 100.0;
        let releasing = classify_now(&f, &moved, 33.0);
        assert_eq!(releasing.ally_in_danger.map(|w| w.subject), Some(rogue));
        assert_eq!(
            classify_now(&f, &releasing, 33.0 + release + 0.01).ally_in_danger,
            None
        );
    }

    const FRAME: f32 = 1.0 / 60.0;

    /// A frame clock over a fixture's classifications.
    struct Frames {
        now: f32,
        s: TeamSituation,
    }

    impl Frames {
        /// Classified history at 30s.
        fn new(f: &Fixture) -> Self {
            Self {
                now: 30.0,
                s: classify_now(f, &TeamSituation::default(), 30.0),
            }
        }

        /// One frame on: the team-1 window.
        fn step(&mut self, f: &Fixture) -> Option<Window> {
            self.now += FRAME;
            self.s = classify_now(f, &self.s, self.now);
            self.s.ally_in_danger
        }
    }

    /// The fixture's enemy Priest, made a Warlock, 29.5yd from `whom`,
    /// targeting it: still closing (beyond its 28yd preferred range) inside
    /// the 30yd intent radius, so it pressures — but from within
    /// `danger_edge_margin` of both boundaries, so only from the EDGE. (A
    /// Warlock and not the Priest: an enemy healer never closes,
    /// [`closes`].)
    fn edge_threat(f: &mut Fixture, whom: Entity) {
        let at = f.roster[&whom].position;
        let healer = f.healer;
        f.roster.get_mut(&healer).unwrap().class = CharacterClass::Warlock;
        place(f, healer, at.x + 29.5, at.z, Some(whom));
    }

    fn firm(f: &Fixture, whom: Entity) -> bool {
        firmly_pressured(
            &f.roster,
            &f.auras,
            &f.roster[&whom],
            &SituationConfig::default(),
        )
    }

    fn loose(f: &Fixture, whom: Entity) -> bool {
        pressured(
            &f.roster,
            &f.auras,
            &f.roster[&whom],
            &SituationConfig::default(),
        )
    }

    #[test]
    fn edge_and_firm_pressure_are_told_apart() {
        let cfg = SituationConfig::default();
        let mut f = fixture();
        let (warrior, healer, mage) = (f.warrior, f.healer, f.mage);
        edge_threat(&mut f, warrior);
        assert!(loose(&f, warrior) && !firm(&f, warrior));
        // A caster closing through the 2yd band between its 28yd preferred
        // range and the 30yd intent radius is never a margin inside both:
        // anywhere in it, the edge. Well inside the danger radius, firm.
        for d in [28.25, 29.0, 29.75] {
            place(&mut f, healer, d, 0.0, Some(warrior));
            assert!(loose(&f, warrior) && !firm(&f, warrior), "{d}yd");
        }
        place(
            &mut f,
            healer,
            cfg.danger_radius - cfg.danger_edge_margin - 0.5,
            0.0,
            Some(warrior),
        );
        assert!(firm(&f, warrior));
        // Inside the danger radius by less than the margin: the edge.
        place(&mut f, healer, cfg.danger_radius - 0.5, 0.0, Some(warrior));
        assert!(loose(&f, warrior) && !firm(&f, warrior));
        // A melee 1.5yd off is firm; one half a yard inside the intent
        // radius is at its edge.
        place(&mut f, healer, 0.0, 0.0, None);
        f.roster.get_mut(&mage).unwrap().class = CharacterClass::Rogue;
        place(&mut f, mage, 1.5, 0.0, Some(warrior));
        assert!(firm(&f, warrior));
        place(
            &mut f,
            mage,
            cfg.threat_intent_radius - 0.5,
            0.0,
            Some(warrior),
        );
        assert!(loose(&f, warrior) && !firm(&f, warrior));
    }

    /// A teammate firmly pressured opens its window on the first frame it is
    /// dying: the dwell never delays the answer to a Warrior on a dying ally.
    #[test]
    fn firm_pressure_opens_a_window_on_the_first_frame() {
        let mut f = fixture();
        let (warrior, mage) = (f.warrior, f.mage);
        let mut frames = Frames::new(&f);
        f.roster.get_mut(&mage).unwrap().class = CharacterClass::Warrior;
        place(&mut f, mage, 1.5, 0.0, Some(warrior));
        assert_eq!(frames.step(&f), None, "not dying yet");
        f.roster.get_mut(&warrior).unwrap().current_health = 20.0;
        let opened = frames.step(&f).expect("opened on the first dying frame");
        assert_eq!((opened.subject, opened.since), (warrior, frames.now));
    }

    /// The debounce, frame by frame at 60 Hz, on pressure from the edge: a
    /// danger shorter than the dwell opens nothing; a window survives a gap
    /// shorter than the release and keeps its onset; it ends once the release
    /// has run, and at once when its teammate dies.
    #[test]
    fn ally_in_danger_dwells_before_it_opens_and_releases_after_it_clears() {
        let cfg = SituationConfig::default();
        assert!(cfg.danger_dwell > 2.0 * FRAME && cfg.danger_release > 2.0 * FRAME);
        let mut f = fixture();
        let (warrior, healer) = (f.warrior, f.healer);
        f.roster.get_mut(&warrior).unwrap().current_health = 20.0;
        let mut frames = Frames::new(&f);
        let target = |f: &mut Fixture, whom: Option<Entity>| {
            f.roster.get_mut(&healer).unwrap().target = whom;
        };
        // One frame of edge pressure: nothing opens, then or after.
        edge_threat(&mut f, warrior);
        assert_eq!(frames.step(&f), None);
        target(&mut f, None);
        for _ in 0..30 {
            assert_eq!(frames.step(&f), None, "a one-frame danger opened a window");
        }
        // Edge pressure that holds opens a window once the dwell has run.
        target(&mut f, Some(warrior));
        let start = frames.now + FRAME;
        let opened = std::iter::repeat_with(|| frames.step(&f))
            .take(60)
            .find_map(|w| w)
            .expect("a held danger opens a window");
        assert!(opened.since - start >= cfg.danger_dwell - 1e-4);
        assert!(opened.since - start < cfg.danger_dwell + 2.0 * FRAME);
        // A one-frame step off it does not end it, nor re-open it.
        target(&mut f, None);
        assert_eq!(frames.step(&f), Some(opened));
        target(&mut f, Some(warrior));
        assert_eq!(frames.step(&f), Some(opened));
        // Neither does a one-frame heal over the threshold.
        f.roster.get_mut(&warrior).unwrap().current_health = 60.0;
        assert_eq!(frames.step(&f), Some(opened));
        f.roster.get_mut(&warrior).unwrap().current_health = 20.0;
        assert_eq!(frames.step(&f), Some(opened));
        // Healed for good: it holds through the release, then ends.
        f.roster.get_mut(&warrior).unwrap().current_health = 60.0;
        let cleared = frames.now + FRAME;
        let mut ended = None;
        for _ in 0..60 {
            if frames.step(&f).is_none() {
                ended = Some(frames.now);
                break;
            }
        }
        let ended = ended.expect("a cleared danger ends");
        assert!(ended - cleared >= cfg.danger_release - 1e-4);
        assert!(ended - cleared < cfg.danger_release + 2.0 * FRAME);
        // A teammate that dies ends its window at once.
        f.roster.get_mut(&warrior).unwrap().current_health = 20.0;
        for _ in 0..60 {
            frames.step(&f);
        }
        assert!(frames.s.ally_in_danger.is_some());
        f.roster.get_mut(&warrior).unwrap().is_alive = false;
        assert_eq!(frames.step(&f), None);
    }

    /// The takeover: A's window is open, and B — healthier, pressured only
    /// from the edge — has been in danger for the dwell behind it. When A
    /// heals over the threshold, or dies, the window names B on that same
    /// frame: B's dwell counts from when ITS danger began.
    #[test]
    fn a_teammate_already_past_its_dwell_takes_over_at_once() {
        for ending in ["healed", "died"] {
            let mut f = fixture();
            let (warrior, rogue, mage) = (f.warrior, f.rogue, f.mage);
            // A: the Warrior at 20%, a Mage on it at 1yd (firm).
            f.roster.get_mut(&warrior).unwrap().current_health = 20.0;
            place(&mut f, mage, 1.0, 0.0, Some(warrior));
            // B: the Rogue at 40%, 100yd off, the Priest at its edge.
            place(&mut f, rogue, 100.0, 0.0, None);
            f.roster.get_mut(&rogue).unwrap().current_health = 40.0;
            let mut frames = Frames::new(&f);
            edge_threat(&mut f, rogue);
            for _ in 0..30 {
                assert_eq!(frames.step(&f).map(|w| w.subject), Some(warrior));
            }
            assert!(
                frames.s.in_danger.iter().any(|(e, _)| *e == rogue),
                "B is in danger behind A's window"
            );
            match ending {
                "healed" => f.roster.get_mut(&warrior).unwrap().current_health = 100.0,
                _ => f.roster.get_mut(&warrior).unwrap().is_alive = false,
            }
            let w = frames.step(&f).expect("B takes over");
            assert_eq!(
                (w.subject, w.since),
                (rogue, frames.now),
                "A {ending}: B must take over on the same frame"
            );
        }
    }

    /// With the dwell and release at zero the window is the raw fact, frame
    /// for frame: the debounce adds nothing else.
    #[test]
    fn a_zero_debounce_is_the_raw_fact() {
        let cfg = SituationConfig {
            danger_dwell: 0.0,
            danger_release: 0.0,
            ..SituationConfig::default()
        };
        let mut f = fixture();
        let (warrior, healer) = (f.warrior, f.healer);
        f.roster.get_mut(&warrior).unwrap().current_health = 20.0;
        let at = |f: &Fixture, prev: &TeamSituation, now: f32| {
            classify_roster(
                &f.roster,
                &f.auras,
                &[],
                1,
                prev,
                &MatchClock::after_gates(now),
                &cfg,
            )
        };
        let quiet = at(&f, &TeamSituation::default(), 30.0);
        edge_threat(&mut f, warrior);
        let on = at(&f, &quiet, 30.01);
        assert_eq!(on.ally_in_danger.map(|w| w.since), Some(30.01));
        f.roster.get_mut(&healer).unwrap().target = None;
        assert_eq!(at(&f, &on, 30.02).ally_in_danger, None);
    }

    #[test]
    fn the_phase_follows_the_gates_and_the_attrition_onset() {
        let f = fixture();
        let at = |clock: MatchClock| {
            classify_roster(
                &f.roster,
                &f.auras,
                &[],
                1,
                &TeamSituation::default(),
                &clock,
                &SituationConfig::default(),
            )
        };
        assert_eq!(at(MatchClock::default()).phase, MatchPhase::Prematch);
        let engaged = at(MatchClock::after_gates(ATTRITION_ONSET_SECS - 0.1));
        assert_eq!(engaged.phase, MatchPhase::Engaged);
        assert!(engaged.attrition_remaining() > 0.0);
        let attrition = at(MatchClock::after_gates(ATTRITION_ONSET_SECS));
        assert_eq!(attrition.phase, MatchPhase::Attrition);
        assert_eq!(attrition.attrition_remaining(), 0.0);
        assert_eq!(at(MatchClock::after_gates(0.0)).attrition_remaining(), 1.0);
        assert_eq!(
            ATTRITION_ONSET_SECS,
            DAMPENING_START_SECS + DAMPENING_RAMP_SECS * 0.5,
            "Attrition begins as dampening reaches 50%"
        );
    }

    #[test]
    fn phase_and_danger_transitions_are_recorded() {
        let mut f = fixture();
        let mut trace = DecisionTrace::default();
        let quiet = TeamSituation::default();
        let (warrior, mage) = (f.warrior, f.mage);
        f.roster.get_mut(&warrior).unwrap().current_health = 10.0;
        place(&mut f, mage, 1.0, 0.0, Some(warrior));
        let danger = classify_now(&f, &quiet, 5.0);
        record_transitions(1, &quiet, &danger, &mut trace);
        let names: Vec<(String, String)> = trace
            .pending_events
            .iter()
            .map(|e| {
                let v = serde_json::to_value(e).unwrap();
                (v["name"].to_string(), v["change"].to_string())
            })
            .collect();
        assert_eq!(
            names,
            vec![
                ("\"phase\"".to_string(), "\"shift\"".to_string()),
                ("\"tempo\"".to_string(), "\"shift\"".to_string()),
                ("\"ally_in_danger\"".to_string(), "\"onset\"".to_string()),
            ]
        );
    }
}

/// AS-386's `sight_yield`: what the scenes (`tests/sight_yield_scenes.rs`)
/// do not pin one rule at a time — rule 3 against its neighbours, through the
/// seam that reads the stance off the situation — and the trace's transitions.
#[cfg(test)]
mod sight_yield_tests {
    use super::*;
    use crate::states::match_config::CharacterClass;
    use crate::states::play_match::decision_trace::EventPayload;

    struct Team {
        roster: BTreeMap<Entity, CombatantInfo>,
        auras: BTreeMap<Entity, Vec<Aura>>,
        dps: Entity,
        healer: Entity,
    }

    fn info(entity: Entity, class: CharacterClass, hp: f32) -> CombatantInfo {
        CombatantInfo {
            entity,
            team: 1,
            slot: 0,
            class,
            current_health: hp,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
            book: crate::states::play_match::ability_book::AbilityBook::shipped_for(class, None),
        }
    }

    fn team(dps_hp: f32) -> Team {
        let mut world = World::new();
        let dps = world.spawn_empty().id();
        let healer = world.spawn_empty().id();
        Team {
            roster: [
                info(dps, CharacterClass::Warrior, dps_hp),
                info(healer, CharacterClass::Priest, 100.0),
            ]
            .into_iter()
            .map(|u| (u.entity, u))
            .collect(),
            auras: BTreeMap::new(),
            dps,
            healer,
        }
    }

    fn cfg() -> SituationConfig {
        SituationConfig::default().for_team(1, AiProfile::TeamPlan)
    }

    /// The team's one pair, classified through [`yield_sight`] onto a
    /// situation that holds a pillar stance when `stance` — the seam
    /// `update_situations` runs, the stance read off the situation.
    fn yield_of(t: &Team, stance: bool, prev: &TeamSituation, now: f32) -> SightPair {
        let cfg = cfg();
        let mut situation = TeamSituation {
            allies: allies(&t.roster, &t.auras, 1, &cfg),
            pillar_stance: stance.then_some(StanceWindow {
                healer: t.healer,
                pillar: 0,
                side: 0.0,
                reason: pillar_stance::StanceReason::AvoidCc,
                since: now,
            }),
            ..Default::default()
        };
        yield_sight(&mut situation, &t.roster, &t.auras, 1, prev, now, &cfg);
        situation.sight_yield[0]
    }

    /// Rule 3: while the team holds a stance, a healthy DPS yields. The
    /// control: no stance, it leads. Rule 2 still comes first (a pressured
    /// healer's yield names rule 2), and rule 3 before rule 4 (a hurt DPS in
    /// a stance yields to the stance).
    #[test]
    fn the_stance_makes_a_healthy_dps_yield() {
        let t = team(100.0);
        let none = TeamSituation::default();
        assert_eq!(yield_of(&t, true, &none, 30.0).rule, YieldRule::Stance);
        assert_eq!(yield_of(&t, false, &none, 30.0).rule, YieldRule::DpsLeads);
        let hurt = team(50.0);
        assert_eq!(yield_of(&hurt, true, &none, 30.0).rule, YieldRule::Stance);
        let mut stunned = team(100.0);
        stunned.auras.insert(
            stunned.healer,
            vec![Aura {
                effect_type: AuraType::Stun,
                duration: 3.0,
                ..Default::default()
            }],
        );
        assert_eq!(
            yield_of(&stunned, true, &none, 30.0).rule,
            YieldRule::HealerCannotMove
        );
    }

    fn changes(
        prev: &TeamSituation,
        next: &TeamSituation,
    ) -> Vec<(SituationChangeKind, YieldRule)> {
        let mut trace = DecisionTrace::default();
        record_sight_yield_changes(1, prev, next, &mut trace);
        trace
            .pending_events
            .iter()
            .map(|e| match &e.payload {
                EventPayload::SituationChange {
                    name: SituationName::SightYield,
                    change,
                    rule: Some(rule),
                    ..
                } => (*change, *rule),
                other => panic!("not a sight_yield change: {other:?}"),
            })
            .collect()
    }

    fn situation(pairs: &[SightPair]) -> TeamSituation {
        TeamSituation {
            sight_yield: pairs.iter().copied().collect(),
            ..Default::default()
        }
    }

    /// Every change is traced, and only a change: a yield's onset, a rule
    /// shift while it yields, its end, and its pair gone with its DPS; a
    /// leading DPS whose leading rule changes, and a frame with nothing new,
    /// record nothing.
    #[test]
    fn each_change_is_traced_once() {
        let t = team(100.0);
        let pair = |rule| SightPair {
            dps: t.dps,
            healer: t.healer,
            rule,
            since: 30.0,
            hurt: false,
        };
        use SituationChangeKind::*;
        use YieldRule::*;
        let leads = situation(&[pair(DpsLeads)]);
        let hurt = situation(&[pair(DpsHurt)]);
        let pressed = situation(&[pair(HealerPressured)]);
        assert_eq!(changes(&leads, &hurt), vec![(Onset, DpsHurt)]);
        assert_eq!(changes(&hurt, &hurt), vec![]);
        assert_eq!(changes(&hurt, &pressed), vec![(Shift, HealerPressured)]);
        assert_eq!(changes(&pressed, &leads), vec![(End, DpsLeads)]);
        assert_eq!(changes(&leads, &situation(&[pair(DpsCannotMove)])), vec![]);
        assert_eq!(
            changes(&pressed, &situation(&[])),
            vec![(End, HealerPressured)]
        );
        assert_eq!(
            changes(&TeamSituation::default(), &hurt),
            vec![(Onset, DpsHurt)]
        );
    }
}
