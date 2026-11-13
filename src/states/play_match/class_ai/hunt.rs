//! The endgame Hunt play (AS-394, AS-404): a team that outnumbers a lone
//! enemy healer hiding at a pillar closes on it from both sides of the
//! pillar.
//!
//! A healer left alone hides at a pillar (its cover denial, its team solve
//! or its pillar mode put it there), and a lone chaser cannot out-angle a
//! pillar hugger: closing on it shrinks the arc it must stay out of, and it
//! turns round the pillar faster in angle than the chaser can
//! (`docs/design/2026-10-25-ranged-dps-vs-hidden-targets.md` §3.2). Two
//! hunters closing from opposite sides leave it nowhere to hide.
//!
//! **The situation** ([`classify`], `TeamSituation::hunt`). `TeamPlan` only
//! (a team that runs [`PlayId::Hunt`]), with the gates open, at a pillar (a
//! map with none has nothing to hunt round). It begins when all of:
//!
//! 1. **lone healer**: the enemy's only living non-pet unit is a healer;
//! 2. **outnumbered**: the team has two or more living non-pet units;
//! 3. **at a pillar**: the healer stands within [`HUNT_ENTER`] of a pillar's
//!    skin;
//! 4. **hidden**: one of the team's units cannot see it.
//!
//! It HOLDS while the healer stays within [`HUNT_LEAVE`] of that pillar's
//! skin, or stands in the pillar's shadow from any living unit of the team
//! ([`in_shadow`]: AS-206's widened shadow, the one shadow definition) — it
//! is still using the pillar as cover. It ends once the healer is out in the
//! open, or when it reaches another pillar (a new hunt there). The shadow's
//! [`formation::SIGHT_MARGIN`] widening is the release hysteresis: a healer
//! must stand that far clear of every teammate's sight line round the pillar
//! before the hunt lets go. The window names the healer, the pillar and the
//! AXIS (the healer's bearing round the pillar's centre at onset, which only
//! assigns the sides). Every change is a `situation_change` named `hunt`.
//!
//! **Who hunts** — exactly two units, or none: one viewer cannot out-angle a
//! hugger. In this order:
//!
//! | role | in the hunt |
//! |---|---|
//! | ranged DPS (Mage, Hunter, Warlock) | first |
//! | healer (the Priest: [`HUNT_EXCEPTIONS`] names the others) | the second when only one ranged DPS hunts, and only while no teammate is dying |
//! | melee DPS | no: its pursuit already walks round the pillar onto the healer, and a melee on it sees it |
//!
//! The two units are the first two that qualify in Entity order (ranged DPS
//! before the healer), not the two nearest the pillar.
//!
//! **Sides** ([`Station::slot`]). At onset each unit takes a side of the
//! pillar: the two candidate points at the axis ±90° ([`stations`]) are
//! paired with the two units by the shorter total walk ROUND the pillar
//! ([`walk_round`], AS-403), so two units on one side fan out, each to the
//! side on its own hand. A unit keeps its side for the whole hunt. Sides are
//! measured from the healer's LIVE bearing round the pillar, so they move
//! with it.
//!
//! **The pincer** (per unit, every classification):
//!
//! - A unit that SEES the healer stands where it is and casts. The sight
//!   latch ([`Station::sighted`]) is a Schmitt pair on one shadow: it sets
//!   once the unit sees the healer clear of the pillar's widened shadow, and
//!   clears only once it loses plain sight.
//! - A BLIND unit walks to the nearest spot on its own side that sees the
//!   healer clear of the widened shadow ([`pincer_point`]): 4-14yd off the
//!   skin, within [`HUNT_REACH`]. As the healer moves round, that spot moves
//!   with it, so the two close in from both sides.
//! - Until a unit first sees the healer (its onset walk), it takes the
//!   shorter way round to its side's onset point — the far station a unit
//!   walked to before AS-404 — and stops to cast if it gets sight on the way
//!   ([`Station::onset_round_back`]). After that a blind unit on the wrong
//!   side always goes round the BACK of the pillar to its own
//!   ([`round_the_back`]), never across the healer's face — the rule that
//!   stops both hunters trailing the healer round one side.
//! - With no spot on its side that sees the healer (a wall), the unit walks
//!   for sight on its ordinary movement.
//!
//! So no hunter is ever parked without sight, and none walks out of sight
//! onto a fixed point.
//!
//! **The healer and its team's sight** (AS-385 D2: sight of an OWED duty
//! comes before hiding). A healer hunting on the far side cannot see a DPS
//! on the near one: the pillar stands between them. It hunts only while no
//! living teammate is dying (below `urgency_hp_threshold`, the health at
//! which the medic chase and the team solve already walk a healer back into
//! sight), and once one is, not again until every one is back to
//! [`SIGHT_YIELD_HP`] — the healer tops them up first. While it is owed, the
//! healer is on its ordinary solve, and with only one ranged DPS the team
//! hunts with nobody.
//!
//! **The play** ([`HuntStage`], `situation::Play::Hunt`) is a MOVEMENT play,
//! like Pillar: what the unit casts is its ordinary chain, and the Burst
//! step still leads it inside a kill window (the Burst stage applies
//! whatever the unit's play). A hunting unit walks to its point
//! ([`walk_to_station`]) — a kiter in place of its ENGAGE seek and chase, a
//! Warlock in place of its pursuit, a healer in place of its formation or
//! cover. A hunting DPS ignores its sight yield: the team's hunt leads, as
//! the team's stance does. The play stands down while the healer is locked
//! (the team's `enemy_healer_locked` is on it): a locked healer cannot run
//! round its pillar, and the Chain play below it extends the lock.
//!
//! **The target focus** (AS-389) needs no rule here: with one enemy left
//! there is nobody else in reach to move the focus to.

use bevy::prelude::*;
use smallvec::SmallVec;

use super::combat_snapshot::CombatSnapshot;
use super::formation;
use super::pillar_mode::{rings, Ring};
use super::plays::{own_class, PlayStage};
use super::{CombatContext, CombatantInfo};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::ability_book::AbilityBook;
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

/// A hunt holds while its healer stands within this of the pillar's skin
/// (the release half of [`HUNT_ENTER`]'s Schmitt pair), and beyond it while
/// the healer is still in the pillar's shadow from one of the team
/// ([`in_shadow`]).
pub const HUNT_LEAVE: f32 = 10.0;

/// The farthest off its pillar's skin a healer can stand and still be held
/// by the pillar's shadow from a teammate: beyond [`HUNT_LEAVE`], a hunt
/// holds while the pillar hides the healer from one of the team, out to
/// here. Farther out the healer is in the open, whoever the pillar hides it
/// from.
pub const HUNT_SHADOW_LEAVE: f32 = 16.0;

/// The farthest a pincer spot stands off the pillar's skin ([`pincer_point`]),
/// and where the two onset side points stand ([`stations`]). From 20yd off a
/// Nagrand pillar's centre a hugger is in sight across about 95° either side
/// of the viewer's bearing (design §3.2), and a hugger on the far side stays
/// within 30yd, every ranged unit's casting reach.
pub const HUNT_STANDOFF: f32 = 14.0;

/// The nearest a pincer spot stands off the skin, and the nearest an onset
/// side point is pulled in toward the pillar when the wall or another pillar
/// takes the ring at [`HUNT_STANDOFF`].
pub const HUNT_MIN_STANDOFF: f32 = 4.0;

/// The angle between the two onset side points round the pillar's centre,
/// which assign the two units their sides ([`stations`], [`assign`]): half a
/// turn, so each side is one half of the pillar. Two viewers on those points
/// leave a hugger nowhere to hide, where 160° leaves a sliver of the far
/// side out of both points' sight
/// ([`tests::two_stations_leave_a_hugger_nowhere_to_hide`]).
pub const HUNT_SPLIT: f32 = std::f32::consts::PI;

/// A walking unit within this many yards of its point is on it.
pub const STATION_ARRIVED: f32 = 1.5;

/// How far a station may turn toward the axis when its own bearing is
/// taken, in steps of [`STATION_TURN_STEP`].
const STATION_MAX_TURN: f32 = 40.0 * std::f32::consts::PI / 180.0;
const STATION_TURN_STEP: f32 = 10.0 * std::f32::consts::PI / 180.0;
/// The step a taken station is pulled in by.
const STATION_PULL_STEP: f32 = 2.0;

/// One hunter's place in the pincer: its side, and the point it walks to or
/// stands on this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    pub unit: Entity,
    /// Its side, kept for the hunt: 0 the arc counter-clockwise of the
    /// healer's live bearing round the pillar, 1 the arc clockwise of it.
    pub slot: u8,
    /// Where the unit goes: where it stands while [`Self::sighted`],
    /// otherwise its [`pincer_point`] (or the next point of its walk round
    /// the back of the pillar).
    pub point: Vec2,
    /// Whether the unit runs the play this frame: `false` only when no spot
    /// on its side sees the healer, and it walks for sight on its ordinary
    /// movement instead.
    pub hold: bool,
    /// The sight latch: set once the unit sees the healer clear of the
    /// pillar's widened shadow ([`in_shadow`]), cleared once it loses plain
    /// sight. While set the unit stands and casts.
    pub sighted: bool,
    /// Whether the unit has been [`Self::sighted`] since it took its side:
    /// its onset walk is over, and from now on it goes round the back of the
    /// pillar to reach its side.
    pub onset_walk_done: bool,
    /// Whether the unit's onset walk goes round the back: the shorter walk
    /// from where it took its side to that side's onset point ([`stations`])
    /// passes behind the pillar, not across the healer's face. Fixed when
    /// the unit takes its side.
    pub onset_round_back: bool,
    /// When the unit's current walk point was chosen (the decision trace's
    /// `sim_time` clock): a walk point is kept at least [`REAIM_SECS`].
    pub aimed_at: f32,
    /// Whether [`Self::point`] is a carrot of a walk round the back.
    pub round_back: bool,
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

/// Whether a unit playing `role` can hold a station: a healer or a ranged
/// DPS (a Shadowform Priest is the second).
pub fn can_station(role: Role) -> bool {
    role != Role::MeleeDps
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

/// How far inside an attack's range band a hunter must stand before it
/// counts as in reach: the enter half of the reach Schmitt pair (it stays in
/// reach out to the band's own edge).
pub const REACH_MARGIN: f32 = 2.0;

/// The least time a blind hunter keeps the walk point it chose before it may
/// choose another: a healer on the run moves every point's sight, and
/// re-aiming each frame it does would turn the walk every few frames.
pub const REAIM_SECS: f32 = 0.5;

/// How long after a hunt ends a hunt that starts again on the same healer at
/// the same pillar is the same hunt: each unit keeps its side.
pub const HUNT_REMEMBER: f32 = 3.0;

/// A hunt that just ended, kept for [`HUNT_REMEMBER`] (`TeamSituation::hunt_memory`).
#[derive(Clone, Debug, PartialEq)]
pub struct HuntMemory {
    pub window: HuntWindow,
    /// When it ended, on the decision trace's `sim_time` clock.
    pub ended: f32,
}

/// The team's hunt memory after a classification: the hunt that just ended
/// (`prev` held one, `next` holds none), or the one it already kept while
/// that is younger than [`HUNT_REMEMBER`]; none while a hunt runs.
pub fn remember(
    prev: Option<&HuntWindow>,
    kept: Option<&HuntMemory>,
    next: Option<&HuntWindow>,
    now: f32,
) -> Option<HuntMemory> {
    match (prev, next) {
        (_, Some(_)) => None,
        (Some(w), None) => Some(HuntMemory {
            window: w.clone(),
            ended: now,
        }),
        (None, None) => kept.filter(|m| now - m.ended <= HUNT_REMEMBER).cloned(),
    }
}

/// Whether `unit` can attack from `distance` (yards): some damaging ability
/// in its own book, not locked by its per-match choices, whose range band
/// holds `distance` with `margin` to spare at both ends (a Hunter's shots
/// have a dead zone).
pub fn in_reach(unit: &CombatantInfo, distance: f32, margin: f32) -> bool {
    book_reaches(&unit.book, distance, margin)
}

/// [`in_reach`], asked of an ability book.
pub fn book_reaches(book: &AbilityBook, distance: f32, margin: f32) -> bool {
    book.iter().any(|(ability, config)| {
        config.is_damage()
            && !book.is_locked(*ability)
            && distance <= config.range - margin
            && config.min_range.is_none_or(|m| distance >= m + margin)
    })
}

/// Whether `pillar` alone stands between `a` and `b`.
fn pillar_blocks(obstacles: &[ObstacleVolume], pillar: usize, a: Vec3, b: Vec3) -> bool {
    obstacles
        .get(pillar)
        .is_some_and(|o| !has_line_of_sight(std::slice::from_ref(o), a, b))
}

/// The angle between two pincer candidates round the pillar.
const PINCER_STEP: f32 = 10.0 * std::f32::consts::PI / 180.0;

/// Whether `healer` (XZ) stands in `pillar`'s shadow as `viewer` sees it:
/// AS-206's widened shadow ([`formation::shadow_exits`] at
/// [`formation::SIGHT_MARGIN`]) — the wedge behind the pillar's footprint
/// from the viewer, widened by the margin, and only out to the near-obstacle
/// limit (no farther from the pillar than the viewer, or within
/// `REPAIR_FULL` of its footprint). One definition serves both the hunt's
/// hold (the healer is still using its pillar against one of the team) and
/// a walking hunter's stop (it sees the healer with the margin to spare).
pub fn in_shadow(obstacles: &[ObstacleVolume], pillar: usize, viewer: Vec2, healer: Vec2) -> bool {
    obstacles.get(pillar).is_some_and(|o| {
        !formation::shadow_exits(viewer, o, healer, formation::SIGHT_MARGIN).is_empty()
    })
}

/// Whether `p` stands on `side` (`1.0` counter-clockwise, `-1.0` clockwise)
/// of the healer's bearing round `ring`'s pillar, allowing the front itself.
fn on_side(ring: &Ring, p: Vec2, healer_bearing: f32, side: f32) -> bool {
    let d = (ring.bearing(p) - healer_bearing + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    d * side >= -PINCER_STEP * 0.5
}

/// How near the back of the pillar (the healer's antipode) a unit counts as
/// on either side: from there both ways round are about as long.
const BACK_TOLERANCE: f32 = 20.0 * std::f32::consts::PI / 180.0;

/// How far round the pillar a walk round the back aims at once: a carrot on
/// the unit's own circle, re-aimed as it walks.
const BACK_CARROT: f32 = 40.0 * std::f32::consts::PI / 180.0;

/// Whether the shorter walk round `ring`'s pillar from `from` to `to` (on
/// opposite sides of the healer's bearing) passes behind the pillar rather
/// than across the healer's face.
fn crosses_the_back(ring: &Ring, from: Vec2, to: Vec2, healer_bearing: f32) -> bool {
    let (a, b) = (
        signed_offset(ring, from, healer_bearing),
        signed_offset(ring, to, healer_bearing),
    );
    a.signum() != b.signum() && a.abs() + b.abs() > std::f32::consts::PI
}

/// `p`'s bearing round `ring`'s pillar less the healer's, in (-π, π].
fn signed_offset(ring: &Ring, p: Vec2, healer_bearing: f32) -> f32 {
    (ring.bearing(p) - healer_bearing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI
}

/// Whether a unit at `p` sees the healer at `healer` clear of the pillar's
/// widened shadow.
fn sees_clear(obstacles: &[ObstacleVolume], pillar: usize, p: Vec2, healer: Vec2, y: f32) -> bool {
    has_line_of_sight(
        obstacles,
        Vec3::new(p.x, y, p.y),
        Vec3::new(healer.x, y, healer.y),
    ) && !in_shadow(obstacles, pillar, p, healer)
}

/// The next point of a walk round the BACK of the pillar from `from`, which
/// stands on the side of the healer's bearing opposite `side`:
/// [`BACK_CARROT`] further away from the healer's face, on the unit's own
/// circle (held between [`HUNT_MIN_STANDOFF`] and [`HUNT_STANDOFF`] off the
/// skin), in 10° steps so the goal does not change every frame.
fn round_the_back(ring: &Ring, from: Vec2, side: f32) -> Vec2 {
    // Walking away from the face on the wrong side means turning by -side.
    let bearing = ring.bearing(from);
    let step = (bearing / PINCER_STEP).round() * PINCER_STEP - side * BACK_CARROT;
    let radius = from
        .distance(ring.center)
        .clamp(ring.disc + HUNT_MIN_STANDOFF, ring.disc + HUNT_STANDOFF);
    ring.center + Vec2::from_angle(step) * radius
}

/// The nearest spot, by the walk round the pillar ([`walk_round`]), on
/// `side` of the healer's live bearing from which a unit at `from` sees the
/// healer at `healer` clear of the pillar's widened shadow ([`in_shadow`]):
/// on standable ground, between [`HUNT_MIN_STANDOFF`] and [`HUNT_STANDOFF`]
/// off the skin, at a distance from the healer the unit can attack from
/// (`reach`). `None` when no spot on that side qualifies.
fn pincer_point(
    obstacles: &[ObstacleVolume],
    bounds: &ArenaBounds,
    ring: &Ring,
    from: Vec2,
    healer: Vec2,
    side: f32,
    y: f32,
    reach: impl Fn(f32) -> bool,
) -> Option<Vec2> {
    let h = ring.bearing(healer);
    let steps = (std::f32::consts::PI / PINCER_STEP).round() as i32;
    let mut best: Option<(f32, Vec2)> = None;
    for k in 1..=steps {
        let theta = h + side * PINCER_STEP * k as f32;
        let mut standoff = HUNT_STANDOFF;
        while standoff >= HUNT_MIN_STANDOFF - 1e-3 {
            let p = ring.center + Vec2::from_angle(theta) * (ring.disc + standoff);
            standoff -= STATION_PULL_STEP;
            if !reach(Vec3::new(p.x, y, p.y).distance(Vec3::new(healer.x, y, healer.y)))
                || !standable(obstacles, bounds, p, y)
                || !sees_clear(obstacles, ring.pillar, p, healer, y)
            {
                continue;
            }
            let walk = walk_round(ring, from, p);
            if best.is_none_or(|(w, _)| walk < w) {
                best = Some((walk, p));
            }
        }
    }
    best.map(|(_, p)| p)
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

/// How far a unit walks from `from` to `to` round `ring`'s pillar: the
/// straight line when it clears the pillar, otherwise the taut path —
/// tangent, arc of the footprint disc plus a body, tangent — the way the
/// tangent steering walks it, round the shorter side. A straight line
/// through the pillar undercounts a walk to the far side by the half-turn
/// it skips, and would send the unit beside one station round the pillar to
/// the other while its partner walks onto the first.
pub fn walk_round(ring: &Ring, from: Vec2, to: Vec2) -> f32 {
    let (a, b) = (from - ring.center, to - ring.center);
    let (da, db) = (a.length(), b.length());
    // A point on (or, by a hair, inside) the disc plus a body stands on the
    // path's arc itself.
    let r = (ring.disc + MOVER_RADIUS).min(da).min(db);
    if r <= 1e-4 {
        return from.distance(to);
    }
    let wrap = a.angle_to(b).abs() - (r / da).acos() - (r / db).acos();
    if wrap <= 0.0 {
        return from.distance(to);
    }
    (da * da - r * r).max(0.0).sqrt() + (db * db - r * r).max(0.0).sqrt() + r * wrap
}

/// Assign `units` (in priority order, at most two) to the available `slots`
/// of a hunt round `ring`: a unit that held a slot in `prev` keeps it while
/// it is available; the rest take the free slots by least total walk round
/// the pillar ([`walk_round`]; the first such pairing on a tie).
fn assign(
    ring: &Ring,
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
                    sighted: false,
                    onset_walk_done: false,
                    onset_round_back: false,
                    aimed_at: 0.0,
                    round_back: false,
                });
            }
            None => rest.push((unit, at)),
        }
    }
    let free: SmallVec<[usize; 2]> = (0..2)
        .filter(|&s| !taken[s] && slots[s].is_some())
        .collect();
    let walk = |unit: &(Entity, Vec2), slot: usize| walk_round(ring, unit.1, slots[slot].unwrap());
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
                sighted: false,
                onset_walk_done: false,
                onset_round_back: false,
                aimed_at: 0.0,
                round_back: false,
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
                sighted: false,
                onset_walk_done: false,
                onset_round_back: false,
                aimed_at: 0.0,
                round_back: false,
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
                    sighted: false,
                    onset_walk_done: false,
                    onset_round_back: false,
                    aimed_at: 0.0,
                    round_back: false,
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
    if enemies.next().is_some() || !healer.is_healer() {
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
    // A hunt holds while its healer is near its pillar, or while it still
    // stands in that pillar's shadow from one of the team (AS-404): it is
    // still using the pillar as cover. A healer that has reached ANOTHER
    // pillar is hunted there instead.
    let nearest = all
        .iter()
        .min_by(|a, b| a.skin(at).total_cmp(&b.skin(at)))
        .map(|r| r.pillar);
    let held = prev.hunt.as_ref().and_then(|h| {
        let ring = all.iter().find(|r| r.pillar == h.pillar)?;
        let near = ring.skin(at) <= HUNT_LEAVE;
        let elsewhere = nearest.is_some_and(|n| n != h.pillar)
            && all
                .iter()
                .any(|r| r.pillar != h.pillar && r.skin(at) <= HUNT_ENTER);
        let shadowed = !elsewhere
            && ring.skin(at) <= HUNT_SHADOW_LEAVE
            && mates.iter().any(|m| {
                in_shadow(&snapshot.obstacles, h.pillar, xz(m.position), at)
                    || pillar_blocks(&snapshot.obstacles, h.pillar, m.position, healer.position)
            });
        (near || shadowed).then_some((h, *ring))
    });
    // A hunt that ended a moment ago on this healer at this pillar is the
    // same hunt starting again: each unit keeps the side it had.
    let remembered = prev
        .hunt_memory
        .as_ref()
        .filter(|m| clock.now - m.ended <= HUNT_REMEMBER && m.window.subject == healer.entity)
        .map(|m| &m.window);
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
            match remembered.filter(|w| w.pillar == ring.pillar) {
                Some(w) => (ring, w.axis, clock.now),
                None => (ring, ring.bearing(at), clock.now),
            }
        }
    };
    let prev_window = held
        .map(|(h, _)| h)
        .or_else(|| remembered.filter(|w| w.pillar == ring.pillar));
    let excepted = |class: CharacterClass| {
        next.exceptions
            .of(PlayId::Hunt)
            .contains(ExceptionKey::class(class))
    };
    let own_healer = mates.iter().find(|m| m.is_healer()).map(|m| m.entity);
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
        .filter(|m| !m.is_healer() && can_station(m.role) && !excepted(m.class));
    let healers = mates
        .iter()
        .filter(|m| m.is_healer() && !owed && !excepted(m.class));
    let units: SmallVec<[(Entity, Vec2); 2]> = ranged
        .chain(healers)
        .take(2)
        .map(|m| (m.entity, xz(m.position)))
        .collect();
    let y = healer.position.y;
    let slots = stations(&snapshot.obstacles, &snapshot.bounds, &ring, axis, y);
    // Two viewers or none: one alone cannot out-angle a hugger (§3.2) — one
    // unit to station, or one station the geometry leaves.
    let mut assigned = assign(&ring, slots, &units, prev_window);
    if assigned.len() < 2 {
        assigned.clear();
    }
    let position = |unit: Entity| roster.get(&unit).map_or(Vec3::ZERO, |c| c.position);
    // The pincer (the module docs): stand while sighted; blind, walk to the
    // nearest spot on its own side that sees, or — its onset walk done, on the
    // wrong side — round the back of the pillar.
    let healer_bearing = ring.bearing(at);
    for station in assigned.iter_mut() {
        let me = position(station.unit);
        let before = prev_window
            .and_then(|w| w.stations.iter().find(|s| s.unit == station.unit))
            .filter(|s| s.slot == station.slot);
        // The onset walk's way round, fixed when the unit takes its side: the
        // shorter walk to its side's onset point (`assign` left it in
        // `point`), the one a far-station walker took before AS-404.
        station.onset_round_back = match before {
            Some(s) => s.onset_round_back,
            None => crosses_the_back(&ring, xz(me), station.point, healer_bearing),
        };
        let info = roster.get(&station.unit).expect("a hunter on the roster");
        let range = me.distance(healer.position);
        let sees = has_line_of_sight(&snapshot.obstacles, me, healer.position);
        let was_sighted = before.is_some_and(|s| s.sighted);
        // Sighted means in sight AND in reach: a Schmitt pair on each. It sets
        // on sight clear of the widened shadow within `REACH_MARGIN` of the
        // reach band's edges, and clears on losing plain sight or leaving the
        // band itself.
        let sighted = sees
            && if was_sighted {
                in_reach(info, range, 0.0)
            } else {
                in_reach(info, range, REACH_MARGIN)
                    && !in_shadow(&snapshot.obstacles, ring.pillar, xz(me), at)
            };
        station.onset_walk_done = before.is_some_and(|s| s.onset_walk_done) || sighted;
        let side = if station.slot == 0 { 1.0 } else { -1.0 };
        if sighted {
            station.sighted = true;
            station.hold = true;
            station.point = xz(me);
            continue;
        }
        station.sighted = false;
        station.hold = true;
        let offset = signed_offset(&ring, xz(me), healer_bearing);
        let wrong_side = offset * side < -PINCER_STEP * 0.5
            && offset.abs() < std::f32::consts::PI - BACK_TOLERANCE;
        if wrong_side && (station.onset_walk_done || station.onset_round_back) {
            // The carrot is kept until the unit is within half a step of it,
            // so the walk is re-aimed every [`BACK_CARROT`] / 2, not every
            // frame.
            let carrot = before
                .filter(|s| s.round_back)
                .map(|s| s.point)
                .filter(|&p| {
                    signed_offset(&ring, p, ring.bearing(xz(me))).abs() > BACK_CARROT * 0.5
                });
            station.point = carrot.unwrap_or_else(|| round_the_back(&ring, xz(me), side));
            station.round_back = true;
            continue;
        }
        // A walk point is chosen clear of the widened shadow and in reach, and
        // kept for at least `REAIM_SECS`, then for as long as it still sees
        // the healer at all within reach (the latch's Schmitt pair again),
        // so a healer on the move does not re-aim the walk every few frames.
        let kept = before
            .filter(|s| {
                !s.sighted
                    && !s.round_back
                    && s.hold
                    && on_side(&ring, s.point, healer_bearing, side)
            })
            .filter(|s| {
                let p3 = Vec3::new(s.point.x, y, s.point.y);
                clock.now - s.aimed_at < REAIM_SECS
                    || (has_line_of_sight(&snapshot.obstacles, p3, healer.position)
                        && in_reach(info, p3.distance(healer.position), 0.0))
            })
            .map(|s| (s.point, s.aimed_at));
        if let Some((p, at)) = kept {
            station.point = p;
            station.aimed_at = at;
            continue;
        }
        station.aimed_at = clock.now;
        match pincer_point(
            &snapshot.obstacles,
            &snapshot.bounds,
            &ring,
            xz(me),
            at,
            side,
            y,
            |d| in_reach(info, d, REACH_MARGIN),
        ) {
            Some(p) => station.point = p,
            None => station.hold = false,
        }
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
        let role = self.self_info()?.role;
        (self.play(role) == Play::Hunt)
            .then(|| hunt.station_of(self.self_entity))
            .flatten()
    }
}

/// Walk `entity` to its hunt point `station` (or hold it: a sighted unit's
/// point is where it stands): a `Point` directive on it, re-issued once its
/// commit window has run or when it names another point, and traced
/// (`MovementTrigger::Hunt`) as each walk to a new point begins (a walk point
/// is kept until it loses the healer, and a carrot round the back for half a
/// step, so that is when the walk moves on). Returns whether the unit is
/// still walking (beyond [`STATION_ARRIVED`]).
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

    /// A walk that clears the pillar is the straight line; one through it is
    /// the taut path round its disc plus a body, the shorter way.
    #[test]
    fn a_walk_round_the_pillar_is_the_taut_path() {
        let ring = ring();
        let clear = walk_round(&ring, Vec2::new(-20.0, 10.0), Vec2::new(20.0, 10.0));
        assert!((clear - 40.0).abs() < 1e-4, "{clear}");
        // North to south through the centre: two tangents to the 6.5 circle
        // and the arc between their feet.
        let r: f32 = 6.0 + MOVER_RADIUS;
        let tangent = (20.0f32 * 20.0 - r * r).sqrt();
        let arc = r * (std::f32::consts::PI - 2.0 * (r / 20.0).acos());
        let round = walk_round(&ring, Vec2::new(0.0, 20.0), Vec2::new(0.0, -20.0));
        assert!((round - (2.0 * tangent + arc)).abs() < 1e-3, "{round}");
        assert!(round > 40.0 + 2.0, "longer than the line through: {round}");
    }

    /// Two units on one side of the pillar, one beside a station: it takes
    /// that station, and the other walks round to the far one. Measured
    /// through the pillar, the pairing crossed them (AS-403: Mage+Warlock
    /// against Rogue+Druid, Nagrand, seed 3) — the control below shows the
    /// straight lines still prefer that crossing, so the test is not won by
    /// the positions alone.
    #[test]
    fn units_on_one_side_take_the_station_on_their_own_hand() {
        let ring = ring();
        let at = |deg: f32, r: f32| Vec2::from_angle(deg.to_radians()) * r;
        // Station 0 west, station 1 east; the axis points north.
        let slots = [Some(at(180.0, 20.0)), Some(at(0.0, 20.0))];
        let mage = (Entity::from_raw(1), at(139.0, 9.2));
        let warlock = (Entity::from_raw(2), at(-171.0, 6.3));
        let assigned = assign(&ring, slots, &[mage, warlock], None);
        let slot_of = |e: Entity| assigned.iter().find(|s| s.unit == e).map(|s| s.slot);
        assert_eq!(slot_of(warlock.0), Some(0), "beside the west station");
        assert_eq!(slot_of(mage.0), Some(1), "round to the east one");
        // The control: through the pillar, the other pairing is shorter.
        let line = |u: (Entity, Vec2), s: usize| u.1.distance(slots[s].unwrap());
        assert!(line(mage, 0) + line(warlock, 1) < line(mage, 1) + line(warlock, 0));
    }

    /// The shadow is the wedge behind the pillar from the viewer, widened by
    /// the margin and bounded by the near-obstacle limit: a hider just
    /// outside the plain wedge is still in it (the release hysteresis), one
    /// beyond the margin is not, and one farther from the pillar than the
    /// viewer and beyond `REPAIR_FULL` is in the open.
    #[test]
    fn the_shadow_is_widened_by_the_margin_and_bounded() {
        let obstacles = [cylinder(0.0, 0.0, 6.0)];
        let viewer = Vec2::new(-30.0, 0.0);
        // The plain wedge's upper edge, 14yd beyond the centre.
        let half = (6.0f32 / 30.0).asin();
        let along = 44.0;
        let edge = viewer + Vec2::from_angle(half) * (along / half.cos());
        let outward = Vec2::from_angle(half + std::f32::consts::FRAC_PI_2);
        let sees = |p: Vec2| {
            has_line_of_sight(
                &obstacles,
                Vec3::new(viewer.x, 1.0, viewer.y),
                Vec3::new(p.x, 1.0, p.y),
            )
        };
        let inside = edge - outward * 1.0;
        let band = edge + outward * 1.0;
        let clear = edge + outward * (formation::SIGHT_MARGIN + 0.5);
        assert!(!sees(inside) && in_shadow(&obstacles, 0, viewer, inside));
        assert!(
            sees(band) && in_shadow(&obstacles, 0, viewer, band),
            "the band"
        );
        assert!(sees(clear) && !in_shadow(&obstacles, 0, viewer, clear));
        // Straight behind, but beyond both the viewer's distance and
        // `REPAIR_FULL` off the pillar: the open.
        assert!(in_shadow(&obstacles, 0, viewer, Vec2::new(14.0, 0.0)));
        assert!(!in_shadow(&obstacles, 0, viewer, Vec2::new(31.0, 0.0)));
    }

    /// A pincer spot stands on the unit's own side of the healer's bearing,
    /// sees the healer clear of the widened shadow, and is the nearest such
    /// spot by the walk round the pillar.
    #[test]
    fn a_pincer_spot_is_the_nearest_clear_sight_on_its_side() {
        let obstacles = [cylinder(0.0, 0.0, 6.0)];
        let ring = ring();
        let healer = Vec2::new(7.0, 0.0);
        let from = Vec2::new(-20.0, 3.0);
        let bounds = ArenaBounds::default();
        for side in [1.0, -1.0] {
            let p = pincer_point(&obstacles, &bounds, &ring, from, healer, side, 1.0, |d| {
                d <= 30.0
            })
            .expect("a spot");
            assert!(on_side(&ring, p, 0.0, side), "{side}: {p}");
            assert!(sees_clear(&obstacles, 0, p, healer, 1.0));
            assert!(p.distance(healer) <= 30.0);
            // Nothing on that side that sees is a shorter walk.
            for k in 1..=18 {
                let theta = side * (k as f32 * 10.0).to_radians();
                let q = ring.center + Vec2::from_angle(theta) * (ring.disc + HUNT_STANDOFF);
                if sees_clear(&obstacles, 0, q, healer, 1.0) {
                    assert!(walk_round(&ring, from, p) <= walk_round(&ring, from, q) + 1e-3);
                }
            }
        }
        // From the north the counter-clockwise spot is the nearer; the
        // clockwise one is round the pillar's back.
        let north = Vec2::new(0.0, 20.0);
        let ccw = pincer_point(&obstacles, &bounds, &ring, north, healer, 1.0, 1.0, |d| {
            d <= 30.0
        })
        .unwrap();
        let cw = pincer_point(&obstacles, &bounds, &ring, north, healer, -1.0, 1.0, |d| {
            d <= 30.0
        })
        .unwrap();
        assert!(walk_round(&ring, north, ccw) < walk_round(&ring, north, cw));
    }

    /// The walk round the back turns away from the healer's face: a unit
    /// clockwise of the healer bound for the counter-clockwise side turns
    /// clockwise, further from the face, on its own circle.
    #[test]
    fn the_walk_round_the_back_turns_away_from_the_face() {
        let ring = ring();
        // The healer east (0°); the unit at -60°, 15yd out, owed side +1.
        let from = Vec2::from_angle((-60f32).to_radians()) * 15.0;
        let next = round_the_back(&ring, from, 1.0);
        let off = signed_offset(&ring, next, 0.0).to_degrees();
        assert!(off < -60.0 - 30.0, "{off}: further from the face");
        assert!((next.length() - 15.0).abs() < 1e-3, "on its own circle");
        // The other side's unit turns the other way.
        let from = Vec2::from_angle(60f32.to_radians()) * 15.0;
        let off = signed_offset(&ring, round_the_back(&ring, from, -1.0), 0.0).to_degrees();
        assert!(off > 60.0 + 30.0, "{off}");
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
