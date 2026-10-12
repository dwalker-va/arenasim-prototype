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
//!   teammate. The two facts it joins are kept per teammate in
//!   [`TeamSituation::allies`], because the healers' rules read them apart:
//!   "a teammate is dying" (cover denial, the dispel walk's dying-first rule)
//!   and "I am pressured" (the posture machine's PRESSURED trigger).
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
use super::movement_config::MovementConfig;

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
    /// Every living non-pet teammate (the deciding unit included), in entity
    /// order.
    pub allies: SmallVec<[AllyStanding; 3]>,
}

/// What a unit does about a situation: a named reordering of its own priority
/// chain, nothing more. The class AI keeps its `try_*` steps and gates; a play
/// only says which order they run in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Play {
    /// The class's ordinary priority order.
    Default,
    /// The enemy healer cannot answer: spend the hard-hitting step first.
    Burst,
}

/// The part a unit plays for its team, as far as choosing a play goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    MeleeDps,
    RangedDps,
    Healer,
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

    /// The play `role` runs in this situation. Exhaustive over [`Role`], so a
    /// new role is a compile error here until it answers.
    pub fn play_for(&self, role: Role) -> Play {
        match role {
            Role::MeleeDps | Role::RangedDps if self.enemy_healer_locked.is_some() => Play::Burst,
            Role::MeleeDps | Role::RangedDps | Role::Healer => Play::Default,
        }
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
        }
    }

    /// The thresholds a team under `profile` is classified with. Tempo's
    /// hysteresis band is a behaviour change, so it is gated: `TeamPlan` keeps
    /// the configured release, and `Legacy` releases at the onset — no band,
    /// which is exactly the unlatched `>=` test it has always run. Every other
    /// threshold is the same under both.
    pub fn under(self, profile: AiProfile) -> Self {
        match profile {
            AiProfile::TeamPlan => self,
            AiProfile::Legacy => Self {
                press_release_margin: self.press_advantage_margin,
                ..self
            },
        }
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
        team,
        prev,
        clock,
        cfg,
    )
}

/// [`classify`] over the two maps it reads, so a context built without a
/// snapshot (`CombatContext::classified`) can classify its own roster.
pub fn classify_roster(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
    prev: &TeamSituation,
    clock: &MatchClock,
    cfg: &SituationConfig,
) -> TeamSituation {
    let allies = allies(roster, auras, team, cfg);
    TeamSituation {
        phase: phase(clock),
        since_gates: clock.since_gates,
        tempo: tempo(roster, team, prev.tempo, cfg),
        enemy_healer_locked: enemy_healer_locked(roster, auras, team, prev, clock.now),
        ally_in_danger: ally_in_danger(roster, &allies, prev, clock.now),
        allies,
    }
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
    let subject_has_shadow_sight = has_shadow_sight(auras, subject.entity);
    roster
        .values()
        .filter(|c| {
            c.team != subject.team
                && c.is_alive
                && c.target == Some(subject.entity)
                && stealth_visible(
                    c.stealthed,
                    subject_has_shadow_sight,
                    has_shadow_sight(auras, c.entity),
                )
        })
        .any(|threat| {
            let distance = subject.position.distance(threat.position);
            distance <= cfg.danger_radius
                || (distance <= cfg.threat_intent_radius
                    && (threat.is_pet || threat.class.is_melee() || closing(threat, subject)))
        })
}

/// `threat`, targeting `subject`, is still beyond its own preferred range of
/// it: coming in, not holding position.
fn closing(threat: &CombatantInfo, subject: &CombatantInfo) -> bool {
    let preferred = match threat.pet_type {
        Some(pet_type) => pet_type.preferred_range(),
        None => threat.class.preferred_range(),
    };
    threat.distance_to(subject.position) > preferred
}

/// The most injured teammate both dying and pressured, the first in entity
/// order on a tie. Its onset carries over while the subject stays the same.
fn ally_in_danger(
    roster: &BTreeMap<Entity, CombatantInfo>,
    allies: &[AllyStanding],
    prev: &TeamSituation,
    now: f32,
) -> Option<Window> {
    let mut worst: Option<(Entity, f32)> = None;
    for ally in allies.iter().filter(|a| a.dying && a.pressured) {
        let hp = roster.get(&ally.entity)?.health_pct();
        if worst.is_none_or(|(_, best)| hp < best) {
            worst = Some((ally.entity, hp));
        }
    }
    let (subject, _) = worst?;
    let since = prev
        .ally_in_danger
        .filter(|w| w.subject == subject)
        .map_or(now, |w| w.since);
    Some(Window {
        subject,
        since,
        until: None,
    })
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
/// is classified under its own AI profile, the snapshot's
/// ([`SituationConfig::under`]).
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
        let cfg = cfg.under(snapshot.ai_profile.for_team(team));
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
        assert_eq!(teamplan, SituationConfig::default());
        assert_eq!(legacy.press_release_margin, legacy.press_advantage_margin);
        assert_eq!(
            legacy,
            SituationConfig {
                press_release_margin: teamplan.press_advantage_margin,
                ..teamplan
            },
            "only the release differs"
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

    #[test]
    fn only_melee_and_ranged_dps_burst() {
        let locked = TeamSituation {
            enemy_healer_locked: Some(Window {
                subject: Entity::from_raw(1),
                since: 0.0,
                until: None,
            }),
            ..Default::default()
        };
        assert_eq!(locked.play_for(Role::MeleeDps), Play::Burst);
        assert_eq!(locked.play_for(Role::RangedDps), Play::Burst);
        assert_eq!(locked.play_for(Role::Healer), Play::Default);
        assert_eq!(
            TeamSituation::default().play_for(Role::MeleeDps),
            Play::Default
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
        // The Rogue pressured too, and lower: a new window on it.
        place(&mut f, healer, 1.0, 0.0, Some(rogue));
        let moved = classify_now(&f, &later, 32.0);
        let window = moved.ally_in_danger.unwrap();
        assert_eq!((window.subject, window.since), (rogue, 32.0));
        // Healthy again: no window, though still pressured.
        f.roster.get_mut(&warrior).unwrap().current_health = 100.0;
        f.roster.get_mut(&rogue).unwrap().current_health = 100.0;
        assert_eq!(classify_now(&f, &moved, 33.0).ally_in_danger, None);
    }

    #[test]
    fn the_phase_follows_the_gates_and_the_attrition_onset() {
        let f = fixture();
        let at = |clock: MatchClock| {
            classify_roster(
                &f.roster,
                &f.auras,
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
