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
//!
//! ## Plays
//!
//! A [`Play`] is what a unit does about the situation: a named reordering of
//! its own priority chain ([`TeamSituation::play_for`]).
//!
//! - **`Burst`** — a kill window is open, so a DPS puts its BURST step at the
//!   head of its chain: its hard hitters on the window's subject
//!   (`class_ai::burst`), each cast only if it lands before the window's
//!   `until` (the cast gate's fit, `class_ai::cast_guard`). A NEW behaviour,
//!   gated under `AiProfile::TeamPlan` ([`SituationConfig::burst`]) like Peel;
//!   withheld for measurement the same way, and not run by the classes
//!   [`BURST_EXCEPTIONS`] names.
//! - **`Peel`** — a teammate is in danger, so a unit other than that teammate
//!   puts its PEEL step at the head of its chain: crowd control on one of the
//!   teammate's [`danger_attackers`](TeamSituation::danger_attackers)
//!   (`class_ai::peel`). A NEW behaviour, so it is gated under
//!   `AiProfile::TeamPlan` ([`SituationConfig::peel`]), and graduates or is
//!   deleted at the next milestone sweep (the architecture's §4.5).
//!   [`WithheldPlays`] denies it to one team — the paired control its probe
//!   and its head-to-head measure it against.
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
//! | pressured | `danger_radius`, `threat_intent_radius` | the healers' PRESSURED trigger |
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
    /// or still closing to its preferred range. A distant caster holding
    /// position does not pressure, and neither does a melee targeting from
    /// across the arena.
    pub pressured: bool,
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
    /// Whether this team runs the gated [`Play::Peel`]
    /// ([`SituationConfig::peel`]).
    pub peel_play: bool,
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
    /// Whether this team runs the gated [`Play::Burst`]
    /// ([`SituationConfig::burst`]).
    pub burst_play: bool,
    /// The classes of this team that do NOT burst ([`BURST_EXCEPTIONS`], or
    /// the measurement override [`WithheldPlays::burst_exceptions`]).
    pub burst_exceptions: ClassSet,
    /// The trace's `sim_time` this was classified at: the clock a window's
    /// `until` is on, which a cast is fitted against.
    pub now: f32,
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
    /// `until` — heads the chain. Gated under `AiProfile::TeamPlan`.
    Burst,
    /// `ally` is in danger: the PEEL step — crowd control on one of its
    /// [`danger_attackers`](TeamSituation::danger_attackers) — heads the
    /// chain. When it does not fire, the chain below it runs as
    /// [`TeamSituation::fallback_play`] orders it. Gated under
    /// `AiProfile::TeamPlan`.
    Peel { ally: Entity },
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

    /// How much of the way to [`MatchPhase::Attrition`] is left: 1 at the
    /// gates, falling linearly to 0 at [`ATTRITION_ONSET_SECS`], and 0 after.
    pub fn attrition_remaining(&self) -> f32 {
        (1.0 - self.since_gates / ATTRITION_ONSET_SECS).clamp(0.0, 1.0)
    }

    /// The play the unit `me`, playing `role`, runs in this situation.
    ///
    /// [`Play::Peel`] comes first, for every role: a teammate in danger is
    /// answered before a burst is spent. Only a team that runs it
    /// ([`Self::peel_play`]) peels, only for a teammate with an attacker to
    /// peel, and never the teammate in danger itself — its own defence is its
    /// chain's. Otherwise [`Self::fallback_play`].
    pub fn play_for(&self, role: Role, me: Entity) -> Play {
        match self.ally_in_danger {
            Some(w) if self.peel_play && w.subject != me && !self.danger_attackers.is_empty() => {
                Play::Peel { ally: w.subject }
            }
            _ => self.fallback_play(role),
        }
    }

    /// The play `role` runs when it does not peel: the order of the chain
    /// below a peel step that did not fire. A DPS on a team that runs it
    /// ([`Self::burst_play`]) bursts while a kill window is open. Exhaustive
    /// over [`Role`], so a new role is a compile error here until it answers.
    ///
    /// Per role, not per class: a class [`Self::burst_exceptions`] names is
    /// answered by [`Self::bursts`], which every reader of the play asks
    /// (`CombatContext::play`, `CombatContext::burst_window`).
    pub fn fallback_play(&self, role: Role) -> Play {
        match role {
            Role::MeleeDps | Role::RangedDps if self.burst_play && self.kill_window.is_some() => {
                Play::Burst
            }
            Role::MeleeDps | Role::RangedDps | Role::Healer => Play::Default,
        }
    }

    /// Whether a unit of `class` runs the Burst play this team runs: not one
    /// of [`Self::burst_exceptions`].
    pub fn bursts(&self, class: CharacterClass) -> bool {
        self.burst_play && !self.burst_exceptions.contains(class)
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

/// A DPS class the Burst play is NOT used for, and the measurement that put it
/// here. The ruling (2026-10-12): a play that measures negative in a context
/// is not used there; each exception is named here, with its finding, and is
/// re-checked at each milestone sweep by running the head-to-head with the
/// exceptions lifted (`"burst_exceptions": []` in a headless config, read into
/// [`WithheldPlays::burst_exceptions`]).
#[derive(Clone, Copy, Debug)]
pub struct BurstException {
    pub class: CharacterClass,
    /// The measurement that put it here: the findings note and its row.
    pub finding: &'static str,
}

/// The classes the Burst play is not used for (see [`BurstException`]).
pub const BURST_EXCEPTIONS: &[BurstException] = &[];

/// [`BURST_EXCEPTIONS`] as a set.
pub fn burst_exception_classes() -> ClassSet {
    ClassSet::of(&BURST_EXCEPTIONS.iter().map(|e| e.class).collect::<Vec<_>>())
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
    /// Whether the team runs the gated [`Play::Peel`]. Off until
    /// [`Self::under`] puts the team under `TeamPlan`, so a context classified
    /// with the bare thresholds (a test's) never peels.
    pub peel: bool,
    /// Whether the team runs the gated [`Play::Burst`] — off until
    /// [`Self::under`] puts it under `TeamPlan`, as [`Self::peel`] is.
    pub burst: bool,
    /// The gated plays each team is denied ([`WithheldPlays`]), applied by
    /// [`Self::for_team`].
    pub withheld: WithheldPlays,
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
            peel: false,
            burst: false,
            withheld: WithheldPlays::default(),
        }
    }

    /// The thresholds a team under `profile` is classified with. Tempo's
    /// hysteresis band is a behaviour change, so it is gated: `TeamPlan` keeps
    /// the configured release, and `Legacy` releases at the onset — no band,
    /// which is exactly the unlatched `>=` test it has always run. The Peel
    /// and Burst plays are gated the same way: `TeamPlan` runs them, `Legacy`
    /// never does. Every other threshold is the same under both.
    pub fn under(self, profile: AiProfile) -> Self {
        match profile {
            AiProfile::TeamPlan => Self {
                peel: true,
                burst: true,
                ..self
            },
            AiProfile::Legacy => Self {
                press_release_margin: self.press_advantage_margin,
                peel: false,
                burst: false,
                ..self
            },
        }
    }

    /// The configuration team `team`, under `profile`, is classified with:
    /// [`Self::under`], less the plays [`Self::withheld`] denies the team.
    pub fn for_team(self, team: u8, profile: AiProfile) -> Self {
        let cfg = self.under(profile);
        let denied = self.withheld.for_team(team);
        Self {
            peel: cfg.peel && !denied.denies(&PEEL),
            burst: cfg.burst && !denied.denies(&Play::Burst),
            ..cfg
        }
    }

    /// The classes that do not burst: [`BURST_EXCEPTIONS`], unless the
    /// measurement override [`WithheldPlays::burst_exceptions`] names others.
    pub fn burst_exceptions(&self) -> ClassSet {
        self.withheld
            .burst_exceptions
            .unwrap_or_else(burst_exception_classes)
    }
}

/// The gated plays one team is denied: one flag per gated [`Play`] variant,
/// read and written only through the exhaustive matches in [`Self::denies`]
/// and [`Self::withhold`]. Deleting a gated play is therefore a compile error
/// here until its flag goes with it, and gating a new one is a compile error
/// until it answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaySet {
    peel: bool,
    burst: bool,
}

impl PlaySet {
    /// The set that denies exactly `plays`.
    pub fn of(plays: &[Play]) -> Self {
        let mut set = Self::default();
        for play in plays {
            set.withhold(play);
        }
        set
    }

    /// Deny `play` (a no-op for an ungated one: it cannot be withheld).
    pub fn withhold(&mut self, play: &Play) {
        match play {
            Play::Peel { .. } => self.peel = true,
            Play::Burst => self.burst = true,
            Play::Default => {}
        }
    }

    /// Whether `play` is denied. An ungated play never is.
    pub fn denies(&self, play: &Play) -> bool {
        match play {
            Play::Peel { .. } => self.peel,
            Play::Burst => self.burst,
            Play::Default => false,
        }
    }
}

/// `Play::Peel` as a KIND, for the sets above (a peel's ally does not enter
/// whether it is withheld).
pub const PEEL: Play = Play::Peel {
    ally: Entity::PLACEHOLDER,
};

/// The gated plays ([`Play::Peel`], [`Play::Burst`]) each team is DENIED, for
/// measurement: a team under `TeamPlan` with a play withheld is the paired
/// control the play is measured against — in a tier-4 probe (the same seed
/// bank with the play off) and in a head-to-head (one side with it, one
/// without, on one seed).
///
/// A MEASUREMENT CONTROL, not a behaviour switch: nothing in the game sets it,
/// but it is reachable from a headless match's JSON
/// (`team1_withheld_plays` / `team2_withheld_plays`, `burst_exceptions`) and
/// so from `--replay`, which honours the same config. The default withholds
/// nothing. It lives only as long as the plays it names stay gated: when a
/// play graduates or is deleted (§4.5) its flag goes, and with the last gated
/// play this type and the config fields are deleted too.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WithheldPlays {
    pub team1: PlaySet,
    pub team2: PlaySet,
    /// Replaces [`BURST_EXCEPTIONS`] for both teams when set: how a milestone
    /// sweep re-checks an exception (`Some(ClassSet::default())` lifts them
    /// all). `None`, the default, keeps the shipped exceptions.
    pub burst_exceptions: Option<ClassSet>,
}

impl WithheldPlays {
    /// Team `team`'s withheld plays; team 1's for a team id outside 1..=2,
    /// matching `AiProfiles::for_team`.
    pub fn for_team(&self, team: u8) -> PlaySet {
        if team == 2 {
            self.team2
        } else {
            self.team1
        }
    }

    /// Parse a config's list of gated play names (`"Peel"`, `"Burst"`) into
    /// the set it denies.
    pub fn parse_set(names: &[String]) -> Result<PlaySet, String> {
        let mut set = PlaySet::default();
        for name in names {
            let play = match name.as_str() {
                "Peel" | "peel" => PEEL,
                "Burst" | "burst" => Play::Burst,
                other => {
                    return Err(format!(
                        "Unknown withheld play: '{other}'. Gated plays: Peel, Burst"
                    ))
                }
            };
            set.withhold(&play);
        }
        Ok(set)
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

/// [`classify`] over what it reads — the roster, its auras and the map's
/// obstacles — so a context built without a snapshot
/// (`CombatContext::classified`) can classify its own roster.
pub fn classify_roster(
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
    TeamSituation {
        phase: phase(clock),
        since_gates: clock.since_gates,
        tempo: tempo(roster, team, prev.tempo, cfg),
        enemy_healer_locked: enemy_healer_locked(roster, auras, team, prev, clock.now),
        ally_in_danger,
        danger_attackers,
        peel_play: cfg.peel,
        allies,
        in_danger: danger.in_danger,
        danger_cleared_at: danger.cleared_at,
        classified: true,
        kill_window: kill_window(roster, auras, obstacles, team, prev, clock.now, cfg),
        burst_play: cfg.burst,
        burst_exceptions: cfg.burst_exceptions(),
        now: clock.now,
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
                    && (threat.is_pet || threat.class.is_melee() || closing(threat, subject)))
        })
}

/// `threat`, targeting `subject`, is still beyond its own preferred range of
/// it: coming in, not holding position.
fn closing(threat: &CombatantInfo, subject: &CombatantInfo) -> bool {
    threat.distance_to(subject.position) > preferred_range(threat)
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
        let next = classify(snapshot, team, &prev, clock, &cfg);
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
                peel: true,
                burst: true,
                ..SituationConfig::default()
            }
        );
        assert_eq!(legacy.press_release_margin, legacy.press_advantage_margin);
        assert_eq!(
            legacy,
            SituationConfig {
                press_release_margin: teamplan.press_advantage_margin,
                peel: false,
                burst: false,
                ..teamplan
            },
            "only the release and the gated plays differ"
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
            burst_play: true,
            ..Default::default()
        };
        let me = Entity::from_raw(9);
        assert_eq!(open.play_for(Role::MeleeDps, me), Play::Burst);
        assert_eq!(open.play_for(Role::RangedDps, me), Play::Burst);
        assert_eq!(open.play_for(Role::Healer, me), Play::Default);
        let gated = TeamSituation {
            burst_play: false,
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
        let cfg = SituationConfig {
            withheld: WithheldPlays {
                burst_exceptions: Some(ClassSet::of(&[CharacterClass::Hunter])),
                ..Default::default()
            },
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
        assert!(!s.bursts(CharacterClass::Hunter));
        assert!(s.bursts(CharacterClass::Warrior));
        // The shipped exceptions, unless overridden.
        assert_eq!(
            SituationConfig::default().burst_exceptions(),
            burst_exception_classes()
        );
        let lifted = SituationConfig {
            withheld: WithheldPlays {
                burst_exceptions: Some(ClassSet::default()),
                ..Default::default()
            },
            ..SituationConfig::default()
        };
        assert_eq!(lifted.burst_exceptions(), ClassSet::default());
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
            peel_play: true,
            burst_play: true,
            ..Default::default()
        };
        for role in [Role::MeleeDps, Role::RangedDps, Role::Healer] {
            assert_eq!(danger.play_for(role, me), Play::Peel { ally }, "{role:?}");
            assert_eq!(
                danger.play_for(role, ally),
                danger.fallback_play(role),
                "the teammate in danger does not peel for itself"
            );
            let gated = TeamSituation {
                peel_play: false,
                ..danger.clone()
            };
            assert_eq!(gated.play_for(role, me), gated.fallback_play(role));
            let unattacked = TeamSituation {
                danger_attackers: SmallVec::new(),
                ..danger.clone()
            };
            assert_eq!(
                unattacked.play_for(role, me),
                unattacked.fallback_play(role)
            );
        }
        assert_eq!(danger.fallback_play(Role::MeleeDps), Play::Burst);
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

    #[test]
    fn withheld_plays_deny_peel_to_one_team_only() {
        let withheld = WithheldPlays {
            team2: PlaySet::of(&[PEEL]),
            ..Default::default()
        };
        let cfg = SituationConfig {
            withheld,
            ..SituationConfig::default()
        };
        assert!(cfg.for_team(1, AiProfile::TeamPlan).peel);
        assert!(!cfg.for_team(2, AiProfile::TeamPlan).peel);
        assert!(!cfg.for_team(1, AiProfile::Legacy).peel);
        assert_eq!(
            WithheldPlays::parse_set(&["Peel".to_string()]),
            Ok(PlaySet::of(&[PEEL]))
        );
        assert!(WithheldPlays::parse_set(&["Default".to_string()]).is_err());
    }

    #[test]
    fn withheld_plays_deny_burst_to_one_team_only() {
        let cfg = SituationConfig {
            withheld: WithheldPlays {
                team1: PlaySet::of(&[Play::Burst]),
                ..Default::default()
            },
            ..SituationConfig::default()
        };
        assert!(!cfg.for_team(1, AiProfile::TeamPlan).burst);
        assert!(cfg.for_team(1, AiProfile::TeamPlan).peel, "Peel stays");
        assert!(cfg.for_team(2, AiProfile::TeamPlan).burst);
        assert!(!cfg.for_team(2, AiProfile::Legacy).burst);
        assert_eq!(
            WithheldPlays::parse_set(&["Burst".to_string(), "Peel".to_string()]),
            Ok(PlaySet::of(&[Play::Burst, PEEL]))
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

    /// The fixture's enemy Priest 29.5yd from `whom`, targeting it: still
    /// closing (beyond its 28yd preferred range) inside the 30yd intent
    /// radius, so it pressures — but from within `danger_edge_margin` of both
    /// boundaries, so only from the EDGE.
    fn edge_threat(f: &mut Fixture, whom: Entity) {
        let at = f.roster[&whom].position;
        let healer = f.healer;
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
