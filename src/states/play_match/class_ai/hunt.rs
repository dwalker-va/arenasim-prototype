//! The endgame Hunt play (AS-394): a team that outnumbers a lone enemy healer
//! hiding at a pillar surrounds the pillar from two sides.
//!
//! A healer left alone hides at a pillar (its cover denial, its team solve
//! or its pillar mode put it there), and a lone chaser cannot out-angle a
//! pillar hugger: closing on it shrinks the arc it must stay out of, and it
//! turns round the pillar faster in angle than the chaser can
//! (`docs/design/2026-10-25-ranged-dps-vs-hidden-targets.md` §3.2). Two
//! viewers on opposite sides of the pillar leave it nowhere to hide.
//!
//! **The situation** ([`classify`], `TeamSituation::hunt`). `TeamPlan` only
//! (a team that runs [`PlayId::Hunt`]), with the gates open, at a pillar (a
//! map with none has nothing to hunt round). All of:
//!
//! 1. **lone healer**: the enemy's only living non-pet unit is a healer;
//! 2. **outnumbered**: the team has two or more living non-pet units;
//! 3. **at a pillar**: the healer stands within [`HUNT_ENTER`] of a pillar's
//!    skin (held until it is beyond [`HUNT_LEAVE`]);
//! 4. **hidden**, at onset only: one of the team's units cannot see it.
//!
//! The window names the healer, the pillar and the hunt's AXIS: the healer's
//! bearing round the pillar's centre at onset. It carries over while the
//! healer stays at that pillar, so the stations do not chase the healer round
//! the pillar; a healer that leaves for another pillar starts a new hunt
//! there. Every change is a `situation_change` named `hunt`.
//!
//! **The stations** ([`HuntWindow::stations`]): two points on a ring
//! [`HUNT_STANDOFF`] beyond the pillar's skin, at the axis plus and minus
//! half of [`HUNT_SPLIT`] (180°), so between them every bearing a hugger can
//! take is in sight of one ([`tests::two_stations_leave_a_hugger_nowhere_to_hide`]).
//! A station the arena wall or another pillar takes is pulled in toward the
//! pillar, then turned toward the axis ([`station_point`]).
//!
//! **Who takes a station** — exactly two units, or none: one viewer cannot
//! out-angle a hugger. In this order:
//!
//! | role | in the hunt |
//! |---|---|
//! | ranged DPS (Mage, Hunter, Warlock) | a station, first |
//! | healer (the Priest: [`HUNT_EXCEPTIONS`] names the others) | the second station when only one ranged DPS takes one, and only while no teammate is dying |
//! | melee DPS | no station: its pursuit already walks round the pillar onto the healer, and a melee on it sees it |
//!
//! The two units are the first two that qualify in Entity order (ranged DPS
//! before the healer), not the two nearest the pillar: in a 3v3 with two
//! ranged DPS the stations go to the lower-numbered pair. Each unit keeps the
//! station it took for as long as the hunt lasts; the first assignment of
//! the two to the two stations is the shorter total walk.
//!
//! **Holding** (`hold = !arrived || covered`, per unit). A unit holds its
//! station until it first reaches it; after that it holds only while one of
//! the two stationed units sees the healer. So a unit that has arrived gives
//! its station up as soon as nobody sees the healer — even while its partner
//! is still walking to the other one (the partner keeps walking: it has not
//! arrived) — and goes back to its ordinary walk for sight, taking the
//! station back once one of them sees the healer again. In the seed-723
//! endgame (Mage+Priest against a lone Priest, Nagrand) the Mage, on its
//! station, gives it up at 64.9s while our Priest is feared short of its own.
//! A station never parks a unit blind with nobody shooting.
//!
//! **The healer and its team's sight** (AS-385 D2: sight of an OWED duty
//! comes before hiding). A healer on the far station cannot see a DPS on the
//! near one: the pillar stands between them. It takes the station only while
//! no living teammate is dying (below `urgency_hp_threshold`, the health at
//! which the medic chase and the team solve already walk a healer back into
//! sight), and once one is, not again until every one is back to
//! [`SIGHT_YIELD_HP`] — the healer tops them up first. While it is owed, the
//! healer is on its ordinary solve and takes no station, and with only one
//! ranged DPS the team takes none.
//!
//! **The play** ([`HuntStage`], `situation::Play::Hunt`) is a MOVEMENT play,
//! like Pillar: what the unit casts is its ordinary chain, and
//! the Burst step still leads it inside a kill window (the Burst stage
//! applies whatever the unit's play). A stationed unit walks to its station
//! and holds it ([`walk_to_station`]) — a kiter in place of its ENGAGE seek
//! and chase, a Warlock in place of its pursuit, a healer in place of its
//! formation or cover. A stationed DPS ignores its sight yield while it
//! walks: the team's hunt leads, as the team's stance does. The play stands
//! down while the healer is locked (the team's `enemy_healer_locked` is on
//! it): a locked healer cannot run round its pillar, and the Chain play below
//! it extends the lock.
//!
//! **The target focus** (AS-389) needs no rule here: with one enemy left
//! there is nobody else in reach to move the focus to, and a stationed DPS
//! that cannot see the healer is the station the other one covers.

use bevy::prelude::*;
use smallvec::SmallVec;

use super::combat_snapshot::CombatSnapshot;
use super::pillar_mode::{rings, Ring};
use super::plays::{own_class, PlayStage};
use super::{CombatContext, CombatantInfo};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::play_match::components::{MovementDirective, MovementGoal};
use crate::states::play_match::decision_trace::{
    DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture, SituationChangeKind,
};
use crate::states::play_match::map_geometry::{
    has_line_of_sight, nearest_standable, ObstacleVolume, MOVER_RADIUS,
};
use crate::states::play_match::situation::{
    ExceptionKey, ExceptionSet, MatchClock, Play, PlayId, Role, SituationConfig, TeamSituation,
    SIGHT_YIELD_HP,
};

/// A lone healer within this many yards of a pillar's skin is at it: the
/// hunt may begin. `pillar_mode`'s cover ring stands 1.5yd off the skin, and
/// the team solve's cover spots stand up to a few yards out.
pub const HUNT_ENTER: f32 = 6.0;

/// A hunt ends once its healer stands farther than this from the pillar's
/// skin: the release half of [`HUNT_ENTER`]'s Schmitt pair.
pub const HUNT_LEAVE: f32 = 10.0;

/// How far beyond the pillar's skin the stations stand. From 20yd off a
/// Nagrand pillar's centre a hugger is in sight across about 95° either side
/// of the viewer's bearing (design §3.2), so two stations half a turn apart
/// overlap; and a hugger on the far side of either stays within 30yd, every
/// ranged unit's casting reach.
pub const HUNT_STANDOFF: f32 = 14.0;

/// The nearest a station is pulled in toward the pillar when the wall or
/// another pillar takes the ring at [`HUNT_STANDOFF`].
pub const HUNT_MIN_STANDOFF: f32 = 4.0;

/// The angle between the two stations, round the pillar's centre: half a
/// turn. The design's "about 160° or more" is the least that works from
/// 30yd; at the shipped standoff 160° leaves a sliver of the far side out of
/// both stations' sight, and 180° leaves none
/// ([`tests::two_stations_leave_a_hugger_nowhere_to_hide`]).
pub const HUNT_SPLIT: f32 = std::f32::consts::PI;

/// A stationed unit within this many yards of its station is on it.
pub const STATION_ARRIVED: f32 = 1.5;

/// How far a station may turn toward the axis when its own bearing is
/// taken, in steps of [`STATION_TURN_STEP`].
const STATION_MAX_TURN: f32 = 40.0 * std::f32::consts::PI / 180.0;
const STATION_TURN_STEP: f32 = 10.0 * std::f32::consts::PI / 180.0;
/// The step a taken station is pulled in by.
const STATION_PULL_STEP: f32 = 2.0;

/// One unit's station: the point it holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    pub unit: Entity,
    /// Which side of the axis: 0 (the axis plus half the split) or 1.
    pub slot: u8,
    pub point: Vec2,
    /// Whether the unit holds its station this frame: `!arrived || covered`.
    /// Until it first reaches the station it holds it; after that, only
    /// while one of the two stationed units sees the healer. An arrived unit
    /// gives its station up whenever nobody sees the healer, whether or not
    /// its partner has reached its own, and walks for sight on its ordinary
    /// movement until one of them sees it again — so a station never parks a
    /// unit blind with nobody shooting.
    pub hold: bool,
    /// Whether the unit has reached this station since it took it (within
    /// [`STATION_ARRIVED`]), latched.
    pub arrived: bool,
}

/// One live hunt: the healer, its pillar, the axis and who holds which
/// station. Times are on the decision trace's `sim_time` clock.
#[derive(Clone, Debug, PartialEq)]
pub struct HuntWindow {
    /// The lone enemy healer.
    pub subject: Entity,
    /// The pillar's index in the map's obstacles.
    pub pillar: usize,
    /// The healer's bearing round the pillar's centre at onset (radians).
    pub axis: f32,
    /// When the hunt began on this pillar.
    pub since: f32,
    /// Whether a teammate is owed the healer's sight (the module docs'
    /// Schmitt pair): while it is, the healer takes no station.
    pub owed: bool,
    pub stations: SmallVec<[Station; 2]>,
}

impl HuntWindow {
    /// `unit`'s station, if it holds one.
    pub fn station_of(&self, unit: Entity) -> Option<Vec2> {
        self.stations
            .iter()
            .find(|s| s.unit == unit && s.hold)
            .map(|s| s.point)
    }
}

/// A class the Hunt play is NOT used for, and the measurement that put it
/// here: the same ruling, and the same re-check at each milestone sweep, as
/// `burst::BurstException`.
#[derive(Clone, Copy, Debug)]
pub struct HuntException {
    pub class: CharacterClass,
    pub finding: &'static str,
}

/// The classes the Hunt play is not used for (see [`HuntException`]).
/// All three are healers with no attack that costs no mana (the Priest has
/// its wand): a station takes the healer's sight off its partner, and once
/// the partner is out of mana or out of sight nobody hurts the lone healer —
/// the endgame lengthens and draws.
pub const HUNT_EXCEPTIONS: &[HuntException] = &[
    HuntException {
        class: CharacterClass::Paladin,
        finding: "AS-394 pilot (seeds 701-702, uniform): endgames with a stationed Paladin \
                  +3.2s to +64.4s by comp, draws 3 -> 25 (ranged-dps-vs-hidden-targets §9)",
    },
    HuntException {
        class: CharacterClass::Druid,
        finding: "AS-394 pilot (seeds 701-702, uniform): endgames with a stationed Druid +2.8s, \
                  Warlock+Druid +22.1s, draws 11 -> 30 (ranged-dps-vs-hidden-targets §9)",
    },
    HuntException {
        class: CharacterClass::Shaman,
        finding: "AS-394 seeds 711-714 (uniform, Paladin and Druid excepted): endgames with a \
                  stationed Shaman +1.2s, Mage+Shaman +5.6s, Hunter+Shaman +6.7s; every one of \
                  the 23 new draws had one, draws 12 -> 35 (ranged-dps-vs-hidden-targets §9)",
    },
];

/// Whether `class` can hold a station: a healer or a ranged DPS.
pub fn can_station(class: CharacterClass) -> bool {
    class.is_healer() || !class.is_melee()
}

/// The Hunt play, as a stage of the pipeline (`plays::PLAY_ORDER`).
pub struct HuntStage;

impl PlayStage for HuntStage {
    fn id(&self) -> PlayId {
        PlayId::Hunt
    }

    fn config_key(&self) -> &'static str {
        "Hunt"
    }

    /// A unit that holds a station in its team's hunt, while the hunted
    /// healer is not locked.
    fn calls(&self, situation: &TeamSituation, _role: Role, me: Entity) -> Option<Play> {
        let hunt = situation.hunt.as_ref()?;
        let locked = situation
            .enemy_healer_locked
            .is_some_and(|w| w.subject == hunt.subject);
        (!locked && hunt.station_of(me).is_some()).then_some(Play::Hunt)
    }

    /// Every unit the stage calls: the classification gives a station only
    /// to a unit that can hold one ([`can_station`]), so the call is the
    /// admission.
    fn admits(&self, _ctx: &CombatContext, _play: &Play) -> bool {
        true
    }

    fn exception_key(&self, ctx: &CombatContext, _play: &Play) -> Option<ExceptionKey> {
        own_class(ctx).map(ExceptionKey::class)
    }

    fn exceptions_have_context(&self) -> bool {
        false
    }

    fn exceptions(&self) -> ExceptionSet {
        ExceptionSet::of(
            &HUNT_EXCEPTIONS
                .iter()
                .map(|e| ExceptionKey::class(e.class))
                .collect::<Vec<_>>(),
        )
    }
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// Whether `p` (XZ) is a spot a unit can stand on: inside the arena and
/// outside every obstacle's footprint.
fn standable(obstacles: &[ObstacleVolume], bounds: &ArenaBounds, p: Vec2, y: f32) -> bool {
    bounds.contains(Vec3::new(p.x, y, p.y))
        && nearest_standable(obstacles, p, y).is_some_and(|q| q.distance(p) < 1e-3)
}

/// The station at `bearing` round `ring`'s pillar: [`HUNT_STANDOFF`] beyond
/// its skin, on standable ground, in sight of the pillar's face at that
/// bearing. A spot the wall or another pillar takes is pulled in toward the
/// pillar (down to [`HUNT_MIN_STANDOFF`]), then turned toward `axis` (up to
/// 40°). `None` when nothing qualifies.
pub fn station_point(
    obstacles: &[ObstacleVolume],
    bounds: &ArenaBounds,
    ring: &Ring,
    bearing: f32,
    axis: f32,
    y: f32,
) -> Option<Vec2> {
    let toward = {
        let d = (axis - bearing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        d.signum()
    };
    let turns = (STATION_MAX_TURN / STATION_TURN_STEP).round() as i32;
    for k in 0..=turns {
        let theta = bearing + toward * STATION_TURN_STEP * k as f32;
        let face = ring.center + Vec2::from_angle(theta) * (ring.disc + MOVER_RADIUS + 0.5);
        let mut standoff = HUNT_STANDOFF;
        while standoff >= HUNT_MIN_STANDOFF - 1e-3 {
            let p = ring.center + Vec2::from_angle(theta) * (ring.disc + standoff);
            if standable(obstacles, bounds, p, y)
                && has_line_of_sight(
                    obstacles,
                    Vec3::new(p.x, y, p.y),
                    Vec3::new(face.x, y, face.y),
                )
            {
                return Some(p);
            }
            standoff -= STATION_PULL_STEP;
        }
    }
    None
}

/// The two stations of a hunt on `ring` about `axis`, by slot.
pub fn stations(
    obstacles: &[ObstacleVolume],
    bounds: &ArenaBounds,
    ring: &Ring,
    axis: f32,
    y: f32,
) -> [Option<Vec2>; 2] {
    let half = HUNT_SPLIT * 0.5;
    [
        station_point(obstacles, bounds, ring, axis + half, axis, y),
        station_point(obstacles, bounds, ring, axis - half, axis, y),
    ]
}

/// Whether a teammate other than `healer` is owed its sight, given whether
/// one was ([`HuntWindow::owed`]): any living non-pet teammate below
/// `dying` (`urgency_hp_threshold`) makes it owed; it stays owed until every
/// one is at or above [`SIGHT_YIELD_HP`].
fn sight_owed(mates: &[&CombatantInfo], healer: Entity, was: bool, dying: f32) -> bool {
    let bar = if was { SIGHT_YIELD_HP } else { dying };
    mates
        .iter()
        .filter(|m| m.entity != healer)
        .any(|m| m.health_pct() < bar)
}

/// Assign `units` (in priority order, at most two) to the available `slots`:
/// a unit that held a slot in `prev` keeps it while it is available; the
/// rest take the free slots by least total walk (the first such pairing on a
/// tie).
fn assign(
    slots: [Option<Vec2>; 2],
    units: &[(Entity, Vec2)],
    prev: Option<&HuntWindow>,
) -> SmallVec<[Station; 2]> {
    let mut out: SmallVec<[Station; 2]> = SmallVec::new();
    let mut taken = [false; 2];
    let mut rest: SmallVec<[(Entity, Vec2); 2]> = SmallVec::new();
    for &(unit, at) in units {
        let kept = prev
            .and_then(|p| p.stations.iter().find(|s| s.unit == unit))
            .map(|s| s.slot as usize)
            .filter(|&slot| !taken[slot] && slots[slot].is_some());
        match kept {
            Some(slot) => {
                taken[slot] = true;
                out.push(Station {
                    unit,
                    slot: slot as u8,
                    point: slots[slot].unwrap(),
                    hold: false,
                    arrived: false,
                });
            }
            None => rest.push((unit, at)),
        }
    }
    let free: SmallVec<[usize; 2]> = (0..2)
        .filter(|&s| !taken[s] && slots[s].is_some())
        .collect();
    let walk = |unit: &(Entity, Vec2), slot: usize| unit.1.distance(slots[slot].unwrap());
    match (rest.len(), free.len()) {
        (0, _) | (_, 0) => {}
        (1, _) => {
            let slot = *free
                .iter()
                .min_by(|a, b| walk(&rest[0], **a).total_cmp(&walk(&rest[0], **b)))
                .unwrap();
            out.push(Station {
                unit: rest[0].0,
                slot: slot as u8,
                point: slots[slot].unwrap(),
                hold: false,
                arrived: false,
            });
        }
        (_, 1) => {
            let unit = rest
                .iter()
                .min_by(|a, b| walk(a, free[0]).total_cmp(&walk(b, free[0])))
                .unwrap();
            out.push(Station {
                unit: unit.0,
                slot: free[0] as u8,
                point: slots[free[0]].unwrap(),
                hold: false,
                arrived: false,
            });
        }
        _ => {
            let straight = walk(&rest[0], free[0]) + walk(&rest[1], free[1]);
            let crossed = walk(&rest[0], free[1]) + walk(&rest[1], free[0]);
            let order = if crossed < straight {
                [free[1], free[0]]
            } else {
                [free[0], free[1]]
            };
            for (unit, slot) in rest.iter().zip(order) {
                out.push(Station {
                    unit: unit.0,
                    slot: slot as u8,
                    point: slots[slot].unwrap(),
                    hold: false,
                    arrived: false,
                });
            }
        }
    }
    out.sort_by_key(|s| s.slot);
    out
}

/// Classify `team`'s hunt (the module docs' keys), given its previous
/// classification and the rest of this one (`next`: its exceptions). Pure
/// and RNG-free.
pub fn classify(
    snapshot: &CombatSnapshot,
    team: u8,
    prev: &TeamSituation,
    next: &TeamSituation,
    clock: &MatchClock,
    cfg: &SituationConfig,
) -> Option<HuntWindow> {
    if !cfg.runs.contains(PlayId::Hunt) || !clock.gates_opened {
        return None;
    }
    let roster = &snapshot.combatants;
    let mut enemies = roster
        .values()
        .filter(|c| c.team != team && c.is_alive && !c.is_pet);
    let healer = enemies.next()?;
    if enemies.next().is_some() || !healer.class.is_healer() {
        return None;
    }
    let mates: Vec<&CombatantInfo> = roster
        .values()
        .filter(|c| c.team == team && c.is_alive && !c.is_pet)
        .collect();
    if mates.len() < 2 {
        return None;
    }
    let all = rings(&snapshot.obstacles);
    let at = xz(healer.position);
    let held = prev.hunt.as_ref().and_then(|h| {
        all.iter()
            .find(|r| r.pillar == h.pillar && r.skin(at) <= HUNT_LEAVE)
            .map(|r| (h, *r))
    });
    let (ring, axis, since) = match held {
        Some((h, ring)) => (ring, h.axis, h.since),
        None => {
            let ring = all
                .iter()
                .copied()
                .min_by(|a, b| a.skin(at).total_cmp(&b.skin(at)))?;
            let hidden = mates
                .iter()
                .any(|m| !has_line_of_sight(&snapshot.obstacles, m.position, healer.position));
            if ring.skin(at) > HUNT_ENTER || !hidden {
                return None;
            }
            (ring, ring.bearing(at), clock.now)
        }
    };
    let prev_window = held.map(|(h, _)| h);
    let excepted = |class: CharacterClass| {
        next.exceptions
            .of(PlayId::Hunt)
            .contains(ExceptionKey::class(class))
    };
    let own_healer = mates.iter().find(|m| m.class.is_healer()).map(|m| m.entity);
    let owed = own_healer.is_some_and(|h| {
        sight_owed(
            &mates,
            h,
            prev_window.is_some_and(|w| w.owed),
            cfg.urgency_hp_threshold,
        )
    });
    let ranged = mates
        .iter()
        .filter(|m| !m.class.is_healer() && can_station(m.class) && !excepted(m.class));
    let healers = mates
        .iter()
        .filter(|m| m.class.is_healer() && !owed && !excepted(m.class));
    let units: SmallVec<[(Entity, Vec2); 2]> = ranged
        .chain(healers)
        .take(2)
        .map(|m| (m.entity, xz(m.position)))
        .collect();
    let y = healer.position.y;
    let slots = stations(&snapshot.obstacles, &snapshot.bounds, &ring, axis, y);
    // Two viewers or none: one alone cannot out-angle a hugger (§3.2) — one
    // unit to station, or one station the geometry leaves.
    let mut assigned = assign(slots, &units, prev_window);
    if assigned.len() < 2 {
        assigned.clear();
    }
    let position = |unit: Entity| roster.get(&unit).map_or(Vec3::ZERO, |c| c.position);
    let covered = assigned
        .iter()
        .any(|s| has_line_of_sight(&snapshot.obstacles, position(s.unit), healer.position));
    for station in assigned.iter_mut() {
        let was = prev_window
            .and_then(|w| w.stations.iter().find(|s| s.unit == station.unit))
            .is_some_and(|s| s.slot == station.slot && s.arrived);
        station.arrived =
            was || xz(position(station.unit)).distance(station.point) <= STATION_ARRIVED;
        station.hold = !station.arrived || covered;
    }
    Some(HuntWindow {
        subject: healer.entity,
        pillar: ring.pillar,
        axis,
        since,
        owed,
        stations: assigned,
    })
}

/// How the hunt moved between two classifications, if it did: began (or
/// moved to another pillar or healer, which is a new hunt), or ended.
pub fn change(
    prev: Option<&HuntWindow>,
    next: Option<&HuntWindow>,
) -> Option<(SituationChangeKind, HuntWindow)> {
    match (prev, next) {
        (None, None) => None,
        (None, Some(h)) => Some((SituationChangeKind::Onset, h.clone())),
        (Some(h), None) => Some((SituationChangeKind::End, h.clone())),
        (Some(a), Some(b)) if a.pillar != b.pillar || a.subject != b.subject => {
            Some((SituationChangeKind::Onset, b.clone()))
        }
        _ => None,
    }
}

impl CombatContext<'_> {
    /// This unit's hunt station, when it runs the Hunt play (its team hunts,
    /// the stage calls and admits it, and no earlier stage of the pipeline
    /// does). `None` on a context nobody classified.
    pub fn hunt_station(&self) -> Option<Vec2> {
        let hunt = self.situation.as_ref()?.hunt.as_ref()?;
        let role = Role::of(self.self_info()?.class);
        (self.play(role) == Play::Hunt)
            .then(|| hunt.station_of(self.self_entity))
            .flatten()
    }
}

/// Walk `entity` to its hunt `station` and hold it: a `Point` directive on
/// the station, re-issued once its commit window has run or when it names
/// another point, and traced (`MovementTrigger::Hunt`) as each walk to a new
/// station begins. Returns whether the unit is still walking (beyond
/// [`STATION_ARRIVED`]).
#[allow(clippy::too_many_arguments)]
pub fn walk_to_station(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    station: Vec2,
    directive: Option<&MovementDirective>,
    posture: TracePosture,
    commit: f32,
    ttl: f32,
    now: f32,
    ctx: &CombatContext,
    decision_trace: &mut DecisionTrace,
) -> bool {
    let current = directive.and_then(|d| match d.goal {
        MovementGoal::Point(p) => Some((xz(p), d)),
        _ => None,
    });
    let same = current.is_some_and(|(p, _)| p.distance(station) < 1e-3);
    let live = current.is_some_and(|(_, d)| now < d.committed_until && now < d.expires);
    if !(same && live) {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(Vec3::new(station.x, my_pos.y, station.y)),
            expires: now + ttl.max(commit),
            committed_until: now + commit,
        });
        if !same {
            if let Some(info) = ctx.combatants.get(&entity) {
                let actor = crate::states::play_match::decision_trace::ActorView::from_info(info);
                let mut builder = decision_trace.start_movement_decision(actor, None);
                builder.direction_change(posture, MovementTrigger::Hunt, MovementGoalKind::Point);
                builder.finish();
            }
        }
    }
    xz(my_pos).distance(station) > STATION_ARRIVED
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::map_config::load_map_geometry_config;
    use crate::states::play_match::map_geometry::EYE_HEIGHT;
    use crate::ArenaMap;

    fn cylinder(x: f32, z: f32, radius: f32) -> ObstacleVolume {
        ObstacleVolume::Cylinder {
            center_xz: Vec2::new(x, z),
            radius,
            base_y: 0.0,
            height: 10.0,
        }
    }

    fn ring() -> Ring {
        rings(&[cylinder(0.0, 0.0, 6.0)])[0]
    }

    const NORTH: f32 = std::f32::consts::FRAC_PI_2;

    /// The trace's transitions: a hunt begins, moves to another pillar (a
    /// new hunt), or ends; one that holds its pillar changes nothing.
    #[test]
    fn a_hunt_changes_on_onset_a_new_pillar_and_its_end() {
        let window = |pillar: usize| HuntWindow {
            subject: Entity::from_raw(7),
            pillar,
            axis: 0.0,
            since: 1.0,
            owed: false,
            stations: SmallVec::new(),
        };
        let (a, b) = (window(0), window(1));
        assert_eq!(change(None, None), None);
        assert_eq!(
            change(None, Some(&a)).map(|c| c.0),
            Some(SituationChangeKind::Onset)
        );
        assert_eq!(
            change(Some(&a), None).map(|c| c.0),
            Some(SituationChangeKind::End)
        );
        assert_eq!(
            change(Some(&a), Some(&b)).map(|c| (c.0, c.1.pillar)),
            Some((SituationChangeKind::Onset, 1))
        );
        assert_eq!(change(Some(&a), Some(&a.clone())), None);
    }

    /// A free spot is [`HUNT_STANDOFF`] off the skin at its own bearing.
    #[test]
    fn a_free_station_stands_at_the_full_standoff() {
        let p = station_point(
            &[cylinder(0.0, 0.0, 6.0)],
            &ArenaBounds::default(),
            &ring(),
            NORTH,
            0.0,
            1.0,
        )
        .unwrap();
        assert!(p.distance(Vec2::new(0.0, 20.0)) < 1e-3, "{p}");
    }

    /// A wall over the far ring pulls the station in toward the pillar
    /// before it turns it.
    #[test]
    fn a_taken_station_is_pulled_in_first() {
        let wall = ObstacleVolume::Aabb {
            min: Vec3::new(-30.0, 0.0, 18.0),
            max: Vec3::new(30.0, 5.0, 23.0),
        };
        let obstacles = [cylinder(0.0, 0.0, 6.0), wall];
        let p = station_point(
            &obstacles,
            &ArenaBounds::default(),
            &ring(),
            NORTH,
            0.0,
            1.0,
        )
        .unwrap();
        assert!(p.distance(Vec2::new(0.0, 16.0)) < 1e-3, "{p}");
    }

    /// A column over every pulled-in spot turns the station, toward the
    /// axis (east here), by the least step that frees it with the pillar's
    /// face in sight: 20° clears the column's footprint, but the column
    /// still hides the face from there, so 30°.
    #[test]
    fn a_column_turns_the_station_toward_the_axis() {
        let column = ObstacleVolume::Aabb {
            min: Vec3::new(-3.0, 0.0, 8.0),
            max: Vec3::new(3.0, 5.0, 23.0),
        };
        let obstacles = [cylinder(0.0, 0.0, 6.0), column];
        let p = station_point(
            &obstacles,
            &ArenaBounds::default(),
            &ring(),
            NORTH,
            0.0,
            1.0,
        )
        .unwrap();
        let bearing = p.to_angle().to_degrees();
        assert!((bearing - 60.0).abs() < 0.1, "{p}: {bearing}");
        assert!((p.length() - 20.0).abs() < 1e-3, "at the full standoff");
    }

    /// A station that cannot see the face it guards is no station: a post
    /// between it and the pillar pulls it in past the post.
    #[test]
    fn a_station_sees_the_face_it_guards() {
        let obstacles = [cylinder(0.0, 0.0, 6.0), cylinder(0.0, 13.0, 1.0)];
        let p = station_point(
            &obstacles,
            &ArenaBounds::default(),
            &ring(),
            NORTH,
            0.0,
            1.0,
        )
        .unwrap();
        assert!(p.distance(Vec2::new(0.0, 10.0)) < 1e-3, "{p}");
    }

    /// The design's claim, measured on the shipped maps: with the two
    /// stations of a hunt about any axis, a healer hugging the pillar at
    /// any bearing is in sight of at least one of them. A healer at the
    /// cover ring (`pillar_mode`'s, 1.5yd off the skin) and one at the skin
    /// itself are both tried, at every 2°, for every pillar and every axis
    /// in 15° steps — wherever the wall leaves both stations their spot.
    /// The control: at 160° apart the far side is not covered.
    #[test]
    fn two_stations_leave_a_hugger_nowhere_to_hide() {
        let config = load_map_geometry_config().expect("maps.ron");
        let mut tried = 0;
        let mut blind_at_160 = 0;
        for map in [ArenaMap::PillaredArena, ArenaMap::TwinPillars] {
            let geometry = config.active_for(map);
            let obstacles = &geometry.volumes;
            for ring in rings(obstacles) {
                for a in 0..24 {
                    let axis = a as f32 * std::f32::consts::TAU / 24.0;
                    let both = |split: f32| {
                        let half = split * 0.5;
                        let p = |b: f32| {
                            let q = ring.center + Vec2::from_angle(b) * (ring.disc + HUNT_STANDOFF);
                            standable(obstacles, &geometry.bounds, q, EYE_HEIGHT).then_some(q)
                        };
                        p(axis + half).zip(p(axis - half))
                    };
                    let Some((s0, s1)) = both(HUNT_SPLIT) else {
                        continue;
                    };
                    let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);
                    let sees = |s: Vec2, h: Vec2| has_line_of_sight(obstacles, eye(s), eye(h));
                    for off in [MOVER_RADIUS + 0.05, MOVER_RADIUS + 1.5] {
                        for k in 0..180 {
                            let b = k as f32 * std::f32::consts::TAU / 180.0;
                            let h = ring.center + Vec2::from_angle(b) * (ring.disc + off);
                            if !standable(obstacles, &geometry.bounds, h, EYE_HEIGHT) {
                                continue;
                            }
                            tried += 1;
                            assert!(
                                sees(s0, h) || sees(s1, h),
                                "{map:?} pillar {} axis {axis} hugger at {b}",
                                ring.pillar
                            );
                            if let Some((n0, n1)) = both(160f32.to_radians()) {
                                if !sees(n0, h) && !sees(n1, h) {
                                    blind_at_160 += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(tried > 10_000, "the sweep ran: {tried}");
        assert!(blind_at_160 > 0, "the control: 160° leaves a blind sliver");
    }
}
