//! The team pillar stance and the Pillar play (AS-385 card B, AS-387).
//!
//! Pillaring is a TEAM stance, never one unit alone (the design's rule 3,
//! `docs/design/2026-10-24-team-sight-cohesion.md` §3.3). The team decides to
//! take a pillar and does it together:
//!
//! - **The situation** ([`classify`], `TeamSituation::pillar_stance`), once
//!   per team per classification, from the stance healer's own view of the
//!   snapshot. `TeamPlan` only (a team that runs [`PlayId::Pillar`]), on a map
//!   with obstacles, with the gates open, on a team whose healer can hug (not
//!   one `pillar_mode::HUG_EXCEPTIONS` names).
//! - **The play** ([`PillarStage`], `situation::Play::Pillar`), in
//!   `plays::PLAY_ORDER` after Hunt and before Chain. A MOVEMENT play, like
//!   Hunt: what the unit casts is its ordinary chain. Still GATED after the
//!   0.8 sweep (`PlayId::is_gated`): `team{1,2}_withheld_plays` can deny it.
//!
//! **Entering for `AvoidCc`** — all of ([`StanceKeys`]):
//!
//! 1. **threat**: a crowd control a pillar defends against is ready for a
//!    living teammate (`pillar_mode::team_threats`: AS-362's `CcField` read
//!    for the team — sight-needing, cast from beyond `MELEE_CC_REACH`, by a
//!    non-pet, ready within `CC_MODE_HORIZON`, the teammate not immune). A
//!    Rogue's stun or a Psychic Scream never qualifies;
//! 2. **at_pillar**: the fight stands at a pillar — the centroid of the
//!    healer's living teammates, or its anchor (the most injured), within
//!    `HUG_ENTER` of the pillar's skin;
//! 3. **arc**: the pillar's ring has a TEAM arc (`pillar_mode::team_arc`):
//!    ground from which the healer sees every living teammate within heal
//!    range, that none of those crowd controls covers, in a run that holds
//!    the sight margin (`pillar_mode::holds_margin`);
//! 4. **no_kill**: the team is not pressing for a kill — no `kill_window`
//!    (Burst wins), and not `Pressing` with the enemy healer locked.
//!
//! **Leaving.** The stance is held at least `MODE_DWELL`, and re-entered only
//! a dwell after it ends. The threat key releases only after `HUG_RELEASE`
//! false; the fight's distance is a Schmitt pair (`HUG_ENTER` in, `HUG_LEAVE`
//! out); the pillar is sticky (`PILLAR_SWITCH`). The arc and the kill are
//! FEASIBILITY: a team arc that stops holding the margin — a teammate the
//! healer cannot reach in sight from any spot on it, dying or not (design D2:
//! sight of an owed duty comes before hiding) — or a kill window ends it at
//! once. Every change is a `situation_change` named `pillar_stance`.
//!
//! **What each role does** while its team holds the stance:
//!
//! | role | in the stance |
//! |---|---|
//! | healer | AS-373's hug on the team arc (`pillar_mode::update`, design D3: it hugs ONLY under the stance) |
//! | ranged DPS (the kiters: Mage, Hunter) | its kiter scorer gains a pull toward the stance's side ([`side_point`], the `stance_pull` weight) |
//! | melee DPS | not called: its half is the sight yield's rule 3, "the stance leads" (AS-386), read from the situation |
//!
//! **The seam with AS-386.** AS-386 classifies `sight_yield`, whose rule 3
//! ("the team stance leads") reads `TeamSituation::pillar_stance`:
//! `situation::update_situations` classifies the stance first and the yield
//! last, so a DPS yields in the same classification its team takes the
//! stance. The yield's kiter mask is AS-386's; this card adds only the pull
//! to the side.
//!
//! **`Stall`** (design D4) is not built: [`StanceReason::Stall`] and its comp
//! table [`STALL_COMPS`] are stubbed, shipped empty, for AS-390.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::combat_snapshot::CombatSnapshot;
use super::pillar_mode::{
    holds_margin, ring_of, rings, runs, sample_angle, stance_members, team_arc, team_threats, Ring,
    HUG_ENTER, HUG_EXCEPTIONS, HUG_LEAVE, HUG_RELEASE, MODE_DWELL, PILLAR_SWITCH, RING_SAMPLES,
};
use super::plays::{own_class, PlayStage};
use super::CombatContext;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::decision_trace::SituationChangeKind;
use crate::states::play_match::situation::{
    ExceptionKey, ExceptionSet, MatchClock, Play, PlayId, Role, SituationConfig, TeamSituation,
    Tempo,
};
use crate::states::play_match::team_solve::HEAL_REACH_MARGIN;

/// Why a team holds its pillar stance. Serializes snake_case (`"avoid_cc"`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StanceReason {
    /// To hide the team from a crowd control the pillar defends against.
    AvoidCc,
    /// To stall for health over time, for a comp that gains from downtime.
    /// Not classified yet (design D4): AS-390.
    Stall,
}

/// One live pillar stance: which pillar the team takes, on which side, and
/// why. Times are on the decision trace's `sim_time` clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StanceWindow {
    /// The healer whose ring the team arc is judged from.
    pub healer: Entity,
    /// The pillar's index in the map's obstacles.
    pub pillar: usize,
    /// The ring angle (radians about the pillar's centre) at the middle of
    /// the team arc's run nearest the side last held: where the team stands.
    pub side: f32,
    pub reason: StanceReason,
    /// When the stance began on this pillar.
    pub since: f32,
}

/// What the stance's keys remember from one classification to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StanceMemory {
    /// When the threat key last went false (`None` while it holds): the
    /// stance releases on it only after `HUG_RELEASE`.
    pub cc_lost_at: Option<f32>,
    /// When the stance last began or ended: the dwell runs from it.
    pub changed_at: Option<f32>,
}

/// The four `AvoidCc` keys as they stood at a classification.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StanceKeys {
    pub threat: bool,
    pub at_pillar: bool,
    pub arc: bool,
    pub no_kill: bool,
}

/// A comp the `Stall` stance would be called for, and the measurement that
/// put it there (design §2.4). AS-390 adds the stall's own gates (tempo,
/// health, dampening, the enemy's damage over time) before any entry lands.
#[derive(Clone, Copy, Debug)]
pub struct StallComp {
    pub classes: &'static [CharacterClass],
    pub finding: &'static str,
}

/// The comps a `Stall` stance is called for. Shipped empty (design D4).
pub const STALL_COMPS: &[StallComp] = &[];

/// A class the Pillar play is NOT used for, and the measurement that put it
/// here: the same ruling, and the same re-check at each milestone sweep, as
/// `burst::BurstException`.
#[derive(Clone, Copy, Debug)]
pub struct PillarException {
    pub class: CharacterClass,
    pub finding: &'static str,
}

/// The classes the Pillar play is not used for (see [`PillarException`]).
pub const PILLAR_EXCEPTIONS: &[PillarException] = &[];

/// Whether `class` runs the kiter scorer (the ENGAGE/KITE machine), the one
/// executor of a ranged DPS's half of the stance.
pub fn is_kiter(class: CharacterClass) -> bool {
    matches!(class, CharacterClass::Mage | CharacterClass::Hunter)
}

/// The Pillar play, as a stage of the pipeline (`plays::PLAY_ORDER`).
pub struct PillarStage;

impl PlayStage for PillarStage {
    fn id(&self) -> PlayId {
        PlayId::Pillar
    }

    fn config_key(&self) -> &'static str {
        "Pillar"
    }

    /// The healer and the ranged DPS of a team that holds a pillar stance. A
    /// melee is not called: its half of the stance is the sight yield (AS-386,
    /// rule 3), read from the situation, so the stance never takes a melee off
    /// a Chain or a Burst.
    fn calls(&self, situation: &TeamSituation, role: Role, _me: Entity) -> Option<Play> {
        (situation.pillar_stance.is_some() && role != Role::MeleeDps).then_some(Play::Pillar)
    }

    /// A unit with an executor for its half: a healer that hugs (not one
    /// `pillar_mode::HUG_EXCEPTIONS` names), or a kiter ([`is_kiter`]).
    fn admits(&self, ctx: &CombatContext, _play: &Play) -> bool {
        own_class(ctx).is_some_and(|class| {
            if class.is_healer() {
                !HUG_EXCEPTIONS.iter().any(|(c, _)| *c == class)
            } else {
                is_kiter(class)
            }
        })
    }

    fn exception_key(&self, ctx: &CombatContext, _play: &Play) -> Option<ExceptionKey> {
        own_class(ctx).map(ExceptionKey::class)
    }

    fn exceptions_have_context(&self) -> bool {
        false
    }

    fn exceptions(&self) -> ExceptionSet {
        ExceptionSet::of(
            &PILLAR_EXCEPTIONS
                .iter()
                .map(|e| ExceptionKey::class(e.class))
                .collect::<Vec<_>>(),
        )
    }
}

/// The healer the stance is judged from: `team`'s first living non-pet healer
/// in entity order that hugs (not one `pillar_mode::HUG_EXCEPTIONS` names).
pub fn stance_healer(snapshot: &CombatSnapshot, team: u8) -> Option<Entity> {
    snapshot
        .combatants
        .values()
        .find(|c| {
            c.team == team
                && c.is_alive
                && !c.is_pet
                && c.class.is_healer()
                && !HUG_EXCEPTIONS.iter().any(|(e, _)| *e == c.class)
        })
        .map(|c| c.entity)
}

/// How far the fight stands from `ring`'s skin: the nearer of the healer's
/// teammates' centroid and its anchor (`mates[0]`, the most injured).
fn fight_skin(ring: &Ring, mates: &[Vec2]) -> f32 {
    let centroid = mates.iter().copied().sum::<Vec2>() / mates.len().max(1) as f32;
    ring.skin(centroid).min(ring.skin(mates[0]))
}

/// The middle of `arc`'s run nearest `near` (a ring angle), among the runs
/// that hold the margin: where the team stands on the pillar.
fn side_of(ring: &Ring, arc: &[bool], near: f32) -> Option<f32> {
    let k = super::pillar_mode::margin_samples(ring, 0.0);
    runs(arc)
        .into_iter()
        .filter(|r| r.1 > 2 * k)
        .map(|(start, len)| {
            if len == RING_SAMPLES {
                near
            } else {
                sample_angle(start) + (len - 1) as f32 * 0.5 * super::pillar_mode::ring_step()
            }
        })
        .min_by(|a, b| angle_gap(*a, near).total_cmp(&angle_gap(*b, near)))
}

fn angle_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}

/// The point on `stance`'s ring at its side: where a ranged DPS's
/// `stance_pull` draws it. `None` when the pillar is not in `obstacles`.
pub fn side_point(
    obstacles: &[crate::states::play_match::map_geometry::ObstacleVolume],
    stance: &StanceWindow,
) -> Option<Vec2> {
    ring_of(obstacles, stance.pillar).map(|r| r.point(stance.side))
}

/// Whether `team`'s comp is one [`STALL_COMPS`] names. Always `false` while
/// the table is empty (design D4); AS-390 adds the stall's own gates.
fn stall_called(snapshot: &CombatSnapshot, team: u8) -> bool {
    let own: Vec<CharacterClass> = snapshot
        .combatants
        .values()
        .filter(|c| c.team == team && !c.is_pet)
        .map(|c| c.class)
        .collect();
    let count =
        |set: &[CharacterClass], class: CharacterClass| set.iter().filter(|c| **c == class).count();
    STALL_COMPS.iter().any(|comp| {
        comp.classes.len() == own.len()
            && comp
                .classes
                .iter()
                .all(|c| count(comp.classes, *c) == count(&own, *c))
    })
}

/// Classify `team`'s pillar stance (the module docs' keys), given its
/// previous classification and the rest of this one (`next`: its kill window,
/// tempo and enemy-healer lock). Pure and RNG-free.
pub fn classify(
    snapshot: &CombatSnapshot,
    team: u8,
    prev: &TeamSituation,
    next: &TeamSituation,
    clock: &MatchClock,
    cfg: &SituationConfig,
) -> (Option<StanceWindow>, StanceMemory) {
    if !cfg.runs.contains(PlayId::Pillar) || snapshot.obstacles.is_empty() || !clock.gates_opened {
        return (None, StanceMemory::default());
    }
    let now = clock.now;
    let mut memory = prev.stance_memory;
    let held = prev.pillar_stance;
    let end = |mut memory: StanceMemory| {
        if held.is_some() {
            memory.changed_at = Some(now);
        }
        memory.cc_lost_at = None;
        (None, memory)
    };
    let Some(healer) = stance_healer(snapshot, team) else {
        return end(memory);
    };
    let ctx = snapshot.context_for(healer);
    let mates = super::formation::teammates(&ctx, healer);
    if mates.is_empty() {
        return end(memory);
    }
    let mate_points: Vec<Vec2> = mates
        .iter()
        .map(|m| Vec2::new(m.position.x, m.position.z))
        .collect();
    let all = rings(ctx.obstacles);
    let held_ring = held.and_then(|h| all.iter().find(|r| r.pillar == h.pillar).copied());
    let nearest = all
        .iter()
        .copied()
        .min_by(|a, b| fight_skin(a, &mate_points).total_cmp(&fight_skin(b, &mate_points)));
    let dwelt = memory.changed_at.is_none_or(|t| now - t >= MODE_DWELL);
    let candidate = match (held_ring, nearest) {
        (Some(h), Some(n))
            if dwelt
                && fight_skin(&n, &mate_points) + PILLAR_SWITCH < fight_skin(&h, &mate_points) =>
        {
            n
        }
        (Some(h), _) => h,
        (None, Some(n)) => n,
        (None, None) => return end(memory),
    };

    let threats = team_threats(&ctx, &stance_members(&ctx), cfg.cc_reach_margin);
    let cc_now = !threats.is_empty();
    if cc_now {
        memory.cc_lost_at = None;
    } else if memory.cc_lost_at.is_none() {
        memory.cc_lost_at = Some(now);
    }
    let heal_reach = cfg.heal_range - HEAL_REACH_MARGIN;
    let y = ctx.self_info().map_or(0.0, |h| h.position.y);
    let arc = team_arc(&ctx, &candidate, &mate_points, heal_reach, y, &threats);
    let keys = StanceKeys {
        threat: if held.is_some() {
            cc_now || memory.cc_lost_at.is_some_and(|t| now - t < HUG_RELEASE)
        } else {
            cc_now
        },
        at_pillar: fight_skin(&candidate, &mate_points)
            <= if held.is_some() { HUG_LEAVE } else { HUG_ENTER },
        arc: holds_margin(&candidate, &arc),
        no_kill: next.kill_window.is_none()
            && !(next.tempo == Tempo::Pressing && next.enemy_healer_locked.is_some()),
    };

    let reason = if keys.threat && keys.at_pillar {
        Some(StanceReason::AvoidCc)
    } else if stall_called(snapshot, team) {
        Some(StanceReason::Stall)
    } else {
        None
    };
    let feasible = keys.arc && keys.no_kill;
    match held {
        None => {
            let Some(reason) = reason.filter(|_| feasible && dwelt) else {
                return (None, memory);
            };
            let healer_xz = ctx
                .self_info()
                .map_or(Vec2::ZERO, |h| Vec2::new(h.position.x, h.position.z));
            let Some(side) = side_of(&candidate, &arc, candidate.bearing(healer_xz)) else {
                return (None, memory);
            };
            memory.changed_at = Some(now);
            (
                Some(StanceWindow {
                    healer,
                    pillar: candidate.pillar,
                    side,
                    reason,
                    since: now,
                }),
                memory,
            )
        }
        Some(h) => {
            if !feasible || (dwelt && reason.is_none()) {
                return end(memory);
            }
            let near = if candidate.pillar == h.pillar {
                h.side
            } else {
                candidate.bearing(Vec2::new(
                    ctx.self_info().map_or(0.0, |i| i.position.x),
                    ctx.self_info().map_or(0.0, |i| i.position.z),
                ))
            };
            let Some(side) = side_of(&candidate, &arc, near) else {
                return end(memory);
            };
            let moved = candidate.pillar != h.pillar;
            if moved {
                memory.changed_at = Some(now);
            }
            (
                Some(StanceWindow {
                    healer,
                    pillar: candidate.pillar,
                    side,
                    reason: reason.unwrap_or(h.reason),
                    since: if moved || h.healer != healer {
                        now
                    } else {
                        h.since
                    },
                }),
                memory,
            )
        }
    }
}

/// How the stance moved between two classifications, if it did: began (or
/// moved to another pillar or healer, which is a new stance), or ended.
pub fn change(
    prev: Option<StanceWindow>,
    next: Option<StanceWindow>,
) -> Option<(SituationChangeKind, StanceWindow)> {
    match (prev, next) {
        (None, None) => None,
        (None, Some(s)) => Some((SituationChangeKind::Onset, s)),
        (Some(s), None) => Some((SituationChangeKind::End, s)),
        (Some(a), Some(b)) if a.pillar != b.pillar || a.healer != b.healer => {
            Some((SituationChangeKind::Onset, b))
        }
        _ => None,
    }
}

impl CombatContext<'_> {
    /// The team's pillar stance, when this unit runs the Pillar play (its
    /// team holds the stance, the stage calls and admits it, and no earlier
    /// stage of the pipeline does). `None` on a context nobody classified.
    pub fn pillar_stance(&self) -> Option<StanceWindow> {
        let stance = self.situation.as_ref()?.pillar_stance?;
        let role = Role::of(self.self_info()?.class);
        (self.play(role) == Play::Pillar).then_some(stance)
    }
}
