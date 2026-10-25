//! Healer pillar modes: hug the pillar, or play the open (AS-373), as the
//! healer's half of the team's pillar stance (AS-385, `class_ai::pillar_stance`).
//!
//! A healer stands in one of two relations to a pillar. It **hugs** it —
//! stays loosely strapped to the pillar and follows its teammates round it, so
//! a change of side is a short walk round the pillar's skin — or it plays the
//! **open**: away from the pillar, following the teammate in sight, nearer the
//! enemy and exposed to its crowd control. The design and the measurements
//! behind every number here are `docs/design/2026-10-19-healer-pillar-modes.md`
//! and, for the stance, `docs/design/2026-10-24-team-sight-cohesion.md`.
//!
//! **The mode key** ([`update`]). A healer hugs only while its TEAM holds a
//! pillar stance and it runs the Pillar play (`TeamSituation::pillar_stance`,
//! `CombatContext::pillar_stance`; design D3: no healer pillars alone), on the
//! stance's pillar, and only while that pillar's ring has ground that sees
//! every living teammate within heal range and that no crowd control a pillar
//! defends against covers ([`team_arc`] over [`team_threats`]), in a run wide
//! enough to stand the sight margin inside both its edges ([`holds_margin`]):
//! a hug is worth its exposure only where the pillar hides the healer from
//! what the hug is for, and a sliver of sight is a grazing line, not a hug.
//! The stance's own keys — the threat, the fight at the pillar, the arc —
//! are classified once for the team (`pillar_stance::classify`), from the
//! same two functions, so the stance and its healer agree on the arc.
//!
//! Otherwise the healer plays the open. The key does not strobe: the stance
//! carries its own Schmitt pair, release and dwell, each mode is held at least
//! [`MODE_DWELL`], and the hugged pillar changes only when the stance's does.
//! An arc that no longer holds the margin ends a hug at once: it is
//! feasibility, not preference. Every change is traced as a
//! `PillarModeEnter` / `PillarModeLeave` movement decision.
//!
//! **What hug does.** The goal is an ANGLE on a ring round the pillar, at
//! `ρ = disc radius + MOVER_RADIUS + COVER_STANDOFF` (8yd on Nagrand, 4.5 on
//! TwinPillars), not a free point. Within the ring's team arc the target
//! angle is chosen in the order ([`target_angle`]):
//!
//! 1. sight of the anchor with a margin: at least [`HUG_SIGHT_MARGIN`] yards
//!    of arc, and the arc the anchor's own motion round the pillar sweeps in
//!    [`HUG_LEAD_SECS`], inside the edge that motion sweeps toward the healer
//!    (both edges, the plain margin, while the anchor stands still), so the
//!    healer sits where the teammate is going rather than where it was;
//! 2. out of the reach of the crowd control ready to land on the healer;
//! 3. hidden from the enemy casters;
//! 4. nearest the current angle, each other teammate in sight counting
//!    [`TEAMMATE_YARDS`](super::formation::TEAMMATE_YARDS) nearer.
//!
//! The angle turns toward its target no faster than the healer walks the ring
//! (`v / ρ`, AS-370's rate) and only through the arc, so a change of side is a
//! walk round the near side, in sight throughout. In FREE the ring point
//! replaces the formation point; in PRESSURED the `OccupyCover` solve keeps
//! its own candidates and gains the arc's ring points, a soft leash to the
//! ring and the margin (`team_solve::HugRing`).
//!
//! **Who.** `TeamPlan` only (`CombatContext::pillar_modes`); never a healer
//! [`HUG_EXCEPTIONS`] names — the Paladin, whose FREE is melee pursuit and
//! which neither mode touches; never while a medic chase, a dispel walk,
//! a hunt station, ESCAPE or a dip holds the healer's legs — those run instead of
//! the FREE and PRESSURED ticks that read the mode, and leave it as it was.

use bevy::prelude::*;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::clamp_to_arena;
use crate::states::play_match::components::{HealerPosture, PillarMode, RingAngle};
use crate::states::play_match::decision_trace::{
    DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::map_geometry::{
    has_line_of_sight, nearest_standable, ObstacleVolume, EYE_HEIGHT, MOVER_RADIUS,
};
use crate::states::play_match::movement_config::SharedMovementConfig;
use crate::states::play_match::team_solve::{is_ranged_threat, COVER_STANDOFF, HEAL_REACH_MARGIN};

use super::cc_reach::{CcField, Disc};
use super::formation::TEAMMATE_YARDS;
use super::healer_postures::start_movement_event_with_target;
use super::CombatContext;

/// The fight — the healer's teammates' centroid, or its anchor — within this
/// many yards of a pillar's skin is at it: the pillar stance's entry edge.
pub const HUG_ENTER: f32 = 12.0;
/// A pillar stance lets go once the fight stands farther than this from its
/// pillar's skin: the Schmitt pair's exit edge.
pub const HUG_LEAVE: f32 = 20.0;
/// Seconds the stance's crowd-control key must be false before it ends the
/// stance.
pub const HUG_RELEASE: f32 = 3.0;
/// The least time, in seconds, a mode (or a hugged pillar) is held before the
/// preference keys may change it. As `COVER_HOLD_SECS`.
pub const MODE_DWELL: f32 = 2.0;
/// Yards nearer another pillar's skin the fight must stand before a stance
/// moves to that pillar.
pub const PILLAR_SWITCH: f32 = 8.0;
/// How far outside the ring, in yards, a hugging healer still counts as
/// strapped to it: the PRESSURED solve's soft leash starts here.
pub const HUG_SLACK: f32 = 3.0;
/// Yards of ring arc the target angle keeps inside the edge of the sighted
/// arc that the anchor's motion sweeps toward it.
pub const HUG_SIGHT_MARGIN: f32 = 3.0;
/// The readiness horizon, in seconds, of the crowd control the stance reads:
/// wider than the positioning horizon (`cc_ready_horizon`), because the mode
/// is a stance, not a step.
pub const CC_MODE_HORIZON: f32 = 5.0;
/// The healers that never hug, each with the reason. The Paladin (design
/// D5) is untouched by both modes: its FREE is melee pursuit and never builds
/// a formation point, so it would hug only in PRESSURED.
pub const HUG_EXCEPTIONS: &[(CharacterClass, &str)] = &[(
    CharacterClass::Paladin,
    "design D5: its FREE is melee pursuit, so it would hug only in PRESSURED",
)];

/// Ring samples: one every 5 degrees, 0.7yd of arc on Nagrand.
pub const RING_SAMPLES: usize = 72;
/// Yards a second of the anchor's motion round the pillar below which it
/// counts as standing still (the margin then holds off both edges).
const STILL: f32 = 0.5;

fn eye(p: Vec2) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.y)
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// Radians per ring sample.
pub fn ring_step() -> f32 {
    std::f32::consts::TAU / RING_SAMPLES as f32
}

/// The angle of ring sample `i`.
pub fn sample_angle(i: usize) -> f32 {
    i as f32 * ring_step()
}

/// The ring sample nearest angle `theta`.
pub fn sample_of(theta: f32) -> usize {
    let i = (theta.rem_euclid(std::f32::consts::TAU) / ring_step()).round() as usize;
    i % RING_SAMPLES
}

/// `theta` wrapped into `(-π, π]`.
fn wrap(theta: f32) -> f32 {
    let t = theta.rem_euclid(std::f32::consts::TAU);
    if t > std::f32::consts::PI {
        t - std::f32::consts::TAU
    } else {
        t
    }
}

/// A pillar's standing ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    /// The pillar's index in the map's obstacles.
    pub pillar: usize,
    pub center: Vec2,
    /// The pillar's footprint disc radius.
    pub disc: f32,
    /// The ring's radius: the disc, a body, and the cover standoff.
    pub rho: f32,
}

impl Ring {
    /// The ring point at angle `theta`.
    pub fn point(&self, theta: f32) -> Vec2 {
        self.center + Vec2::from_angle(theta) * self.rho
    }

    /// How far `p` stands from the pillar's skin (its footprint disc).
    pub fn skin(&self, p: Vec2) -> f32 {
        p.distance(self.center) - self.disc
    }

    /// The angle of `p` about the pillar's centre.
    pub fn bearing(&self, p: Vec2) -> f32 {
        (p - self.center).to_angle()
    }
}

/// The ring of obstacle `index`, when it is a pillar (a cylinder or a prism):
/// a box is a wall or a platform, with no ring to walk.
pub fn ring_of(obstacles: &[ObstacleVolume], index: usize) -> Option<Ring> {
    let volume = obstacles.get(index)?;
    if matches!(volume, ObstacleVolume::Aabb { .. }) {
        return None;
    }
    let (center, disc) = volume.footprint_disc();
    Some(Ring {
        pillar: index,
        center,
        disc,
        rho: disc + MOVER_RADIUS + COVER_STANDOFF,
    })
}

/// Every pillar's ring, in obstacle order.
pub fn rings(obstacles: &[ObstacleVolume]) -> Vec<Ring> {
    (0..obstacles.len())
        .filter_map(|i| ring_of(obstacles, i))
        .collect()
}

/// Which ring samples stand where the healer can heal `anchor` from: inside
/// the arena, on standable ground, within `heal_reach` of it, and in its
/// sight. For one teammate the sighted samples form one connected arc (a
/// circle round the pillar meets the teammate's shadow in one arc); another
/// pillar or a wall can cut more.
pub fn feasible_arc(
    ctx: &CombatContext,
    ring: &Ring,
    anchor: Vec2,
    heal_reach: f32,
    y: f32,
) -> Vec<bool> {
    (0..RING_SAMPLES)
        .map(|i| {
            let p = ring.point(sample_angle(i));
            ctx.bounds.contains(Vec3::new(p.x, 0.0, p.y))
                && nearest_standable(ctx.obstacles, p, y).is_some_and(|q| q.distance(p) < 1e-3)
                && p.distance(anchor) <= heal_reach
                && has_line_of_sight(ctx.obstacles, eye(p), eye(anchor))
        })
        .collect()
}

/// The connected runs of `true` in a circular sample list, each as its first
/// sample and its length, in angle order. One run of every sample when all
/// are `true`.
pub fn runs(feasible: &[bool]) -> Vec<(usize, usize)> {
    let n = feasible.len();
    if feasible.iter().all(|f| *f) {
        return vec![(0, n)];
    }
    let mut out = Vec::new();
    for i in 0..n {
        if feasible[i] && !feasible[(i + n - 1) % n] {
            let mut len = 0;
            while feasible[(i + len) % n] {
                len += 1;
            }
            out.push((i, len));
        }
    }
    out
}

/// Where along `run` (as a fractional step count from its first sample) the
/// angle `theta` lies, if it lies within it.
fn position_in(run: (usize, usize), theta: f32) -> Option<f32> {
    let (start, len) = run;
    if len == RING_SAMPLES {
        return Some((theta.rem_euclid(std::f32::consts::TAU) - sample_angle(start)) / ring_step());
    }
    let from_start = (theta - sample_angle(start)).rem_euclid(std::f32::consts::TAU) / ring_step();
    // Half a sample of slack either side: a sample stands for its neighbourhood.
    if from_start <= (len - 1) as f32 + 0.5 {
        Some(from_start)
    } else if from_start >= RING_SAMPLES as f32 - 0.5 {
        Some(from_start - RING_SAMPLES as f32)
    } else {
        None
    }
}

/// The run `theta` lies in, or else the run whose nearer end is angularly
/// nearest it.
fn run_for(runs: &[(usize, usize)], theta: f32) -> Option<(usize, usize)> {
    if let Some(r) = runs.iter().find(|r| position_in(**r, theta).is_some()) {
        return Some(*r);
    }
    runs.iter()
        .copied()
        .min_by(|a, b| end_gap(*a, theta).total_cmp(&end_gap(*b, theta)))
}

/// The angular gap from `theta` to the nearer end of `run`.
fn end_gap(run: (usize, usize), theta: f32) -> f32 {
    let (start, len) = run;
    let first = sample_angle(start);
    let last = sample_angle((start + len - 1) % RING_SAMPLES);
    wrap(first - theta).abs().min(wrap(last - theta).abs())
}

/// How the anchor moves round the pillar: its sense (`1` with the angle, `-1`
/// against it, `0` standing still — below [`STILL`]) and its angular speed
/// about the pillar's centre, in radians a second.
pub fn anchor_spin(ring: &Ring, anchor: Vec2, velocity: Vec2) -> (i8, f32) {
    let rel = anchor - ring.center;
    let len = rel.length();
    if len <= f32::EPSILON {
        return (0, 0.0);
    }
    let tangential = rel.perp_dot(velocity) / len;
    let sense = if tangential > STILL {
        1
    } else if tangential < -STILL {
        -1
    } else {
        0
    };
    (sense, (tangential / len).abs())
}

/// Seconds of the anchor's motion round the pillar the margin leads by, on
/// top of [`HUG_SIGHT_MARGIN`]: the sighted arc turns with the anchor, at the
/// anchor's own angular speed, so its trailing edge sweeps the ring that much
/// faster than the anchor walks — a Warrior at the skin turns faster than a
/// healer on the ring can. One commit window: the PRESSURED solve re-solves no
/// oftener, so the healer must stand clear of where the edge will be by then.
pub const HUG_LEAD_SECS: f32 = 1.5;

/// How many samples the sight margin spans on `ring` for an anchor turning
/// round it at `spin` radians a second: [`HUG_SIGHT_MARGIN`] of arc, and the
/// arc the edge sweeps in [`HUG_LEAD_SECS`].
pub fn margin_samples(ring: &Ring, spin: f32) -> usize {
    ((HUG_SIGHT_MARGIN / ring.rho + spin * HUG_LEAD_SECS) / ring_step()).ceil() as usize
}

/// Whether `arc` has a run wide enough to stand [`HUG_SIGHT_MARGIN`] inside
/// both its edges: the arc key. A sliver of sighted ring puts the healer on a
/// grazing line, in sight one step and out the next, so it is no hug.
pub fn holds_margin(ring: &Ring, arc: &[bool]) -> bool {
    let k = margin_samples(ring, 0.0);
    runs(arc).iter().any(|r| r.1 > 2 * k)
}

/// Which samples of the ring are in the CORE of `run`: the run less the
/// margin off the edge the anchor's motion (`sense`) sweeps toward the healer
/// — the trailing edge, the low end for `1` — or off both edges while it
/// stands still. A whole ring has no edge, so all of it is core. A run too
/// short for its margin keeps its middle sample.
pub fn core_of(
    ring: &Ring,
    run: (usize, usize),
    (sense, spin): (i8, f32),
    margin: bool,
) -> Vec<bool> {
    let (start, len) = run;
    let mut core = vec![false; RING_SAMPLES];
    if len == RING_SAMPLES {
        return vec![true; RING_SAMPLES];
    }
    let k = if margin {
        margin_samples(ring, spin)
    } else {
        0
    };
    let (lo, hi) = match sense {
        1 => (k, len),
        -1 => (0, len.saturating_sub(k)),
        _ => (k, len.saturating_sub(k)),
    };
    if lo < hi {
        for j in lo..hi {
            core[(start + j) % RING_SAMPLES] = true;
        }
    } else {
        core[(start + len / 2) % RING_SAMPLES] = true;
    }
    core
}

/// The ring's state for one hug evaluation: everything the FREE goal and the
/// PRESSURED solve read.
#[derive(Clone, Debug, PartialEq)]
pub struct HugGoal {
    pub ring: Ring,
    /// The ring samples that see every teammate in heal range and that no
    /// crowd control a pillar defends against covers ([`team_arc`]).
    pub arc: Vec<bool>,
    /// The samples of the arc the healer may stand at: the run it is in, less
    /// the sight margin ([`core_of`]).
    pub core: Vec<bool>,
    /// The ring angle the goal stands at, turned toward [`Self::target`].
    pub theta: f32,
    /// The ring angle chosen within the core ([`target_angle`]).
    pub target: f32,
    /// The ring point at [`Self::theta`], on standable ground in the arena.
    pub goal: Vec3,
}

/// The inputs [`target_angle`] ranks the core by.
pub struct Ranking<'a> {
    pub field: &'a CcField,
    /// The enemy casters' positions ([`is_ranged_threat`]).
    pub casters: &'a [Vec2],
    /// The healer's other living teammates (the anchor excluded).
    pub others: &'a [Vec2],
}

/// The target ring angle within `core`, ranked as the module docs give: out
/// of crowd-control reach, then the fewest casters in sight, then nearest
/// `theta0` along `run` (each other teammate in sight counting
/// [`TEAMMATE_YARDS`] nearer). The first sample in angle order on a tie.
pub fn target_angle(
    obstacles: &[ObstacleVolume],
    ring: &Ring,
    run: (usize, usize),
    core: &[bool],
    theta0: f32,
    rank: &Ranking,
) -> f32 {
    let at = position_in(run, theta0);
    let mut best: Option<((bool, usize, f32), usize)> = None;
    for j in 0..run.1 {
        let i = (run.0 + j) % RING_SAMPLES;
        if !core[i] {
            continue;
        }
        let p = ring.point(sample_angle(i));
        let covered = rank
            .field
            .covering(obstacles, p, Disc::Outer)
            .next()
            .is_some();
        let seen_by = rank
            .casters
            .iter()
            .filter(|c| has_line_of_sight(obstacles, eye(p), eye(**c)))
            .count();
        let others = rank
            .others
            .iter()
            .filter(|o| has_line_of_sight(obstacles, eye(p), eye(**o)))
            .count();
        let travel = match at {
            Some(a) => (j as f32 - a).abs() * ring_step() * ring.rho,
            None => wrap(sample_angle(i) - theta0).abs() * ring.rho,
        };
        let key = (covered, seen_by, travel - TEAMMATE_YARDS * others as f32);
        if best.is_none_or(|(k, _)| key < k) {
            best = Some((key, i));
        }
    }
    best.map_or(theta0, |(_, i)| sample_angle(i))
}

/// The signed angle from `theta0` toward `target` the goal may turn through:
/// along `run` — through the sighted arc, never through the anchor's shadow —
/// when `theta0` lies in it (`through_arc`). A whole ring turns the shorter
/// way. From outside the run (blind
/// already) it takes the shorter way to the run's nearer end, back into sight.
pub fn turn_toward(run: (usize, usize), theta0: f32, target: f32, through_arc: bool) -> f32 {
    if !through_arc || run.1 == RING_SAMPLES {
        return wrap(target - theta0);
    }
    match position_in(run, theta0) {
        Some(at) => {
            let to = position_in(run, target).unwrap_or(at);
            (to - at) * ring_step()
        }
        None => {
            let first = sample_angle(run.0);
            let last = sample_angle((run.0 + run.1 - 1) % RING_SAMPLES);
            let (a, b) = (wrap(first - theta0), wrap(last - theta0));
            if a.abs() <= b.abs() {
                a
            } else {
                b
            }
        }
    }
}

/// Crowd control that reaches no farther than this is a melee's (a Kidney
/// Shot, a Cheap Shot): its caster walks round the pillar to land it, so the
/// pillar is no defence against it.
pub const MELEE_CC_REACH: f32 = 8.0;

/// The crowd control a pillar defends `healer` against, as a field: a crowd
/// control that needs its caster's sight, cast from beyond [`MELEE_CC_REACH`]
/// by a unit that is not a pet (a pet closes), ready within
/// [`CC_MODE_HORIZON`] and not one `healer` is immune to. Its discs are where
/// a hug would NOT be defended.
pub fn pillar_threats(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    healer: Entity,
    margin: f32,
) -> CcField {
    let mut field = CcField::for_healer(ctx, abilities, healer, CC_MODE_HORIZON, margin);
    field.threats.retain(|t| {
        t.needs_sight
            && t.reach > MELEE_CC_REACH
            && ctx.combatants.get(&t.source).is_some_and(|s| !s.is_pet)
    });
    field
}

/// [`feasible_arc`] less the samples the pillar does not defend: those inside
/// a disc of `threats` ([`pillar_threats`]) — in range of the caster and in its
/// sight. Hugging is worth its exposure only where the pillar hides the healer
/// from the crowd control the hug is for.
pub fn defended_arc(
    ctx: &CombatContext,
    ring: &Ring,
    anchor: Vec2,
    heal_reach: f32,
    y: f32,
    threats: &CcField,
) -> Vec<bool> {
    let mut arc = feasible_arc(ctx, ring, anchor, heal_reach, y);
    for (i, a) in arc.iter_mut().enumerate() {
        if *a {
            let p = ring.point(sample_angle(i));
            *a = threats
                .covering(ctx.obstacles, p, Disc::Outer)
                .next()
                .is_none();
        }
    }
    arc
}

/// The crowd control a pillar defends the TEAM against: [`pillar_threats`]
/// for each unit of `members`, as one field, each (caster, ability) once. The
/// pillar stance's threat key reads it, and its discs are what the team arc
/// ([`team_arc`]) keeps the healer out of. Built from one context, so the
/// stance and the healer that plays it read the same field.
pub fn team_threats(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    members: &[Entity],
    margin: f32,
) -> CcField {
    let mut field = CcField {
        threats: Vec::new(),
        margin,
    };
    for member in members {
        for t in pillar_threats(ctx, abilities, *member, margin).threats {
            if !field
                .threats
                .iter()
                .any(|x| x.source == t.source && x.ability == t.ability)
            {
                field.threats.push(t);
            }
        }
    }
    field
}

/// The TEAM arc of `ring`: the samples from which the healer sees every one of
/// `mates` within `heal_reach`, on standable ground inside the arena, and that
/// no disc of `threats` covers. [`defended_arc`] with "the anchor" widened to
/// every teammate: it is what makes the pillar stance a team position and not
/// a healer's hide (AS-385 §3.3). With one teammate it is that teammate's
/// defended arc.
pub fn team_arc(
    ctx: &CombatContext,
    ring: &Ring,
    mates: &[Vec2],
    heal_reach: f32,
    y: f32,
    threats: &CcField,
) -> Vec<bool> {
    (0..RING_SAMPLES)
        .map(|i| {
            let p = ring.point(sample_angle(i));
            !mates.is_empty()
                && ctx.bounds.contains(Vec3::new(p.x, 0.0, p.y))
                && nearest_standable(ctx.obstacles, p, y).is_some_and(|q| q.distance(p) < 1e-3)
                && mates.iter().all(|m| {
                    p.distance(*m) <= heal_reach
                        && has_line_of_sight(ctx.obstacles, eye(p), eye(*m))
                })
                && threats
                    .covering(ctx.obstacles, p, Disc::Outer)
                    .next()
                    .is_none()
        })
        .collect()
}

/// The units whose crowd control the stance reads: the healer and its living
/// non-pet teammates, in entity order.
pub fn stance_members(ctx: &CombatContext) -> Vec<Entity> {
    ctx.alive_allies().iter().map(|a| a.entity).collect()
}

/// The two keys as they stood at an evaluation, for the trace: the
/// preference key (the team holds a pillar stance this healer plays) and the
/// feasibility key (the stance pillar's team arc holds the margin).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Keys {
    pub stance: bool,
    pub arc: bool,
}

/// Evaluate the healer's pillar mode and, while it hugs, its ring goal.
///
/// Called from the FREE tick and the PRESSURED solve of every healer; a no-op
/// returning `None` unless the healer's team runs `TeamPlan` with the modes on
/// ([`CombatContext::pillar_modes`]), the map has obstacles, and
/// [`HUG_EXCEPTIONS`] does not name its class. The healer hugs only under its
/// team's pillar stance (`CombatContext::pillar_stance`), on the stance's
/// pillar. A change of mode forgets the formation point's memory; leaving a
/// hug seeds the open heading from where the healer stands
/// (`FormationMemory::seed_from_healer`), so the open point takes over there.
#[allow(clippy::too_many_arguments)]
pub fn update(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    entity: Entity,
    speed: f32,
    my_pos: Vec3,
    state: &mut HealerPosture,
    shared: &SharedMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    posture: TracePosture,
) -> Option<HugGoal> {
    if !ctx.pillar_modes()
        || ctx.obstacles.is_empty()
        || ctx
            .self_info()
            .is_some_and(|me| HUG_EXCEPTIONS.iter().any(|(c, _)| *c == me.class))
    {
        return None;
    }
    let mates = super::formation::teammates(ctx, entity);
    let anchor = mates.first().copied();
    let rings = rings(ctx.obstacles);
    let heal_reach = shared.heal_range - HEAL_REACH_MARGIN;
    let stance = ctx.pillar_stance();

    let hugged = match state.pillar.mode {
        PillarMode::Hug { pillar } => rings.iter().find(|r| r.pillar == pillar).copied(),
        PillarMode::Open => None,
    };
    let Some(anchor) = anchor else {
        if let Some(ring) = hugged {
            leave(
                state,
                now,
                ring,
                Keys::default(),
                decision_trace,
                ctx,
                None,
                my_pos,
                posture,
            );
        }
        return None;
    };

    // The candidate pillar is the stance's; with no stance, the hugged one,
    // held only through its dwell.
    let candidate = stance
        .and_then(|s| rings.iter().find(|r| r.pillar == s.pillar).copied())
        .or(hugged);
    let ring = candidate?;

    let threats = team_threats(ctx, abilities, &stance_members(ctx), shared.cc_reach_margin);
    let mate_points: Vec<Vec2> = mates.iter().map(|m| xz(m.position)).collect();
    let arc_of = |r: &Ring| team_arc(ctx, r, &mate_points, heal_reach, my_pos.y, &threats);
    let arc = arc_of(&ring);
    let keys = Keys {
        stance: stance.is_some(),
        arc: holds_margin(&ring, &arc),
    };
    let dwelt = state.pillar.since.is_none_or(|t| now - t >= MODE_DWELL);

    let (ring, arc) = match hugged {
        None => {
            if !(keys.stance && keys.arc && dwelt) {
                return None;
            }
            enter(
                state,
                now,
                ring,
                keys,
                decision_trace,
                ctx,
                anchor.entity,
                my_pos,
                posture,
            );
            (ring, arc)
        }
        // The stance has moved to another pillar, but the hug on this one is
        // still within its dwell: keep hugging this one.
        Some(h) if ring.pillar != h.pillar && !dwelt => {
            let arc = arc_of(&h);
            if !holds_margin(&h, &arc) {
                leave(
                    state,
                    now,
                    h,
                    keys,
                    decision_trace,
                    ctx,
                    Some(anchor.entity),
                    my_pos,
                    posture,
                );
                return None;
            }
            (h, arc)
        }
        Some(h) => {
            if !keys.arc || (dwelt && !keys.stance) {
                leave(
                    state,
                    now,
                    h,
                    keys,
                    decision_trace,
                    ctx,
                    Some(anchor.entity),
                    my_pos,
                    posture,
                );
                return None;
            }
            if ring.pillar != h.pillar {
                enter(
                    state,
                    now,
                    ring,
                    keys,
                    decision_trace,
                    ctx,
                    anchor.entity,
                    my_pos,
                    posture,
                );
            }
            (ring, arc)
        }
    };
    hug_goal(
        ctx, speed, my_pos, state, ring, arc, anchor, &mates, abilities, shared, now,
    )
}

/// Seconds over which the anchor's velocity is measured from its displacement.
const VELOCITY_WINDOW: f32 = 0.25;

/// The anchor's velocity on the ground plane, measured from where it was
/// sampled at least [`VELOCITY_WINDOW`] ago (`PillarMemory::anchor_seen`);
/// zero for a new anchor or a sample more than a second old. The snapshot's
/// own velocity is the unit's facing times its speed, which a unit standing
/// still and hitting keeps, so it would lead the margin round a pillar the
/// anchor is not walking round.
fn anchor_velocity(state: &mut HealerPosture, entity: Entity, pos: Vec2, now: f32) -> Vec2 {
    use crate::states::play_match::components::AnchorSample;
    let fresh = AnchorSample {
        entity,
        pos,
        at: now,
        velocity: Vec2::ZERO,
    };
    match state.pillar.anchor_seen {
        Some(s) if s.entity == entity && now - s.at < 1.0 => {
            if now - s.at >= VELOCITY_WINDOW {
                let velocity = (pos - s.pos) / (now - s.at);
                state.pillar.anchor_seen = Some(AnchorSample { velocity, ..fresh });
                velocity
            } else {
                s.velocity
            }
        }
        _ => {
            state.pillar.anchor_seen = Some(fresh);
            Vec2::ZERO
        }
    }
}

/// The ring goal for a hug of `ring`, whose sighted arc is `arc`.
#[allow(clippy::too_many_arguments)]
fn hug_goal(
    ctx: &CombatContext,
    speed: f32,
    my_pos: Vec3,
    state: &mut HealerPosture,
    ring: Ring,
    arc: Vec<bool>,
    anchor: &super::CombatantInfo,
    mates: &[&super::CombatantInfo],
    abilities: &AbilityDefinitions,
    shared: &SharedMovementConfig,
    now: f32,
) -> Option<HugGoal> {
    let me = xz(my_pos);
    let anchor_xz = xz(anchor.position);
    let all = runs(&arc);
    let last = state.pillar.angle;
    let theta0 = last.map_or_else(|| ring.bearing(me), |a| a.theta);
    let run = run_for(&all, theta0)?;
    let velocity = anchor_velocity(state, anchor.entity, anchor_xz, now);
    let spin = anchor_spin(&ring, anchor_xz, velocity);
    let core = core_of(&ring, run, spin, true);
    let field = CcField::for_healer(
        ctx,
        abilities,
        ctx.self_entity,
        shared.cc_ready_horizon,
        shared.cc_reach_margin,
    );
    let team = ctx.self_info().map_or(0, |s| s.team);
    let casters: Vec<Vec2> = ctx
        .combatants
        .values()
        .filter(|c| c.is_alive && c.team != team && is_ranged_threat(c))
        .map(|c| xz(c.position))
        .collect();
    let others: Vec<Vec2> = mates.iter().skip(1).map(|m| xz(m.position)).collect();
    let target = target_angle(
        ctx.obstacles,
        &ring,
        run,
        &core,
        theta0,
        &Ranking {
            field: &field,
            casters: &casters,
            others: &others,
        },
    );
    let rate = speed / ring.rho;
    let delta = turn_toward(run, theta0, target, true);
    let theta = match last {
        // The first evaluation of a hug starts at the healer's own bearing,
        // or the nearer end of the sighted arc when that bearing is not in it.
        None if position_in(run, theta0).is_some() => theta0,
        None => theta0 + delta,
        Some(a) => {
            let reach = rate * (now - a.at).max(0.0);
            theta0 + delta.clamp(-reach, reach)
        }
    };
    let theta = wrap(theta);
    state.pillar.angle = Some(RingAngle { theta, at: now });
    let p = ring.point(theta);
    let clamped = clamp_to_arena(&ctx.bounds, Vec3::new(p.x, my_pos.y, p.y));
    let spot = nearest_standable(ctx.obstacles, xz(clamped), my_pos.y).unwrap_or(xz(clamped));
    Some(HugGoal {
        ring,
        arc,
        core,
        theta,
        target,
        goal: Vec3::new(spot.x, my_pos.y, spot.y),
    })
}

#[allow(clippy::too_many_arguments)]
fn enter(
    state: &mut HealerPosture,
    now: f32,
    ring: Ring,
    keys: Keys,
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    anchor: Entity,
    my_pos: Vec3,
    posture: TracePosture,
) {
    state.pillar.mode = PillarMode::Hug {
        pillar: ring.pillar,
    };
    state.pillar.since = Some(now);
    state.pillar.angle = None;
    state.formation.forget();
    trace(
        decision_trace,
        ctx,
        Some(anchor),
        my_pos,
        posture,
        MovementTrigger::PillarModeEnter,
        ring,
        keys,
    );
}

#[allow(clippy::too_many_arguments)]
fn leave(
    state: &mut HealerPosture,
    now: f32,
    ring: Ring,
    keys: Keys,
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    anchor: Option<Entity>,
    my_pos: Vec3,
    posture: TracePosture,
) {
    state.pillar.mode = PillarMode::Open;
    state.pillar.since = Some(now);
    state.pillar.angle = None;
    state.formation.forget();
    state.formation.seed_from_healer = true;
    trace(
        decision_trace,
        ctx,
        anchor,
        my_pos,
        posture,
        MovementTrigger::PillarModeLeave,
        ring,
        keys,
    );
}

#[allow(clippy::too_many_arguments)]
fn trace(
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    anchor: Option<Entity>,
    my_pos: Vec3,
    posture: TracePosture,
    trigger: MovementTrigger,
    ring: Ring,
    keys: Keys,
) {
    let builder = match anchor {
        Some(a) => start_movement_event_with_target(decision_trace, ctx, a, my_pos),
        None => super::healer_postures::start_movement_event(decision_trace, ctx),
    };
    if let Some(mut builder) = builder {
        builder.direction_change(posture, trigger, MovementGoalKind::Point);
        builder.scorer_term("pillar", ring.pillar as f32);
        builder.scorer_term("key_stance", f32::from(u8::from(keys.stance)));
        builder.scorer_term("key_arc", f32::from(u8::from(keys.arc)));
        builder.finish();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::states::match_config::CharacterClass::{Priest, Warrior};
    use crate::states::play_match::class_ai::reach_fixture::{ctx, roster, unit};
    use crate::states::play_match::Aura;

    /// A Nagrand-sized pillar at the origin: footprint disc radius 6, so the
    /// ring stands at 8yd.
    fn pillar() -> Vec<ObstacleVolume> {
        vec![ObstacleVolume::Cylinder {
            center_xz: Vec2::ZERO,
            radius: 6.0,
            base_y: 0.0,
            height: 10.0,
        }]
    }

    fn mask(set: &[usize]) -> Vec<bool> {
        let mut out = vec![false; RING_SAMPLES];
        for &i in set {
            out[i] = true;
        }
        out
    }

    #[test]
    fn runs_follow_the_circle_round() {
        assert_eq!(runs(&[true; RING_SAMPLES]), vec![(0, RING_SAMPLES)]);
        // A run across sample 0.
        let wrapped: Vec<usize> = (68..72).chain(0..5).collect();
        assert_eq!(runs(&mask(&wrapped)), vec![(68, 9)]);
        // Two runs, in angle order.
        assert_eq!(runs(&mask(&[3, 4, 10])), vec![(3, 2), (10, 1)]);
        assert!(runs(&[false; RING_SAMPLES]).is_empty());
    }

    /// One teammate behind the pillar's ring: the samples that see it form one
    /// connected arc, on its side of the pillar; its shadow side is not in it.
    #[test]
    fn the_sighted_arc_is_one_connected_run_on_the_anchors_side() {
        let obstacles = pillar();
        let mut world = World::new();
        let healer = world.spawn_empty().id();
        let mate = world.spawn_empty().id();
        let units = roster(vec![
            unit(healer, 1, Priest, Vec3::new(-10.0, 1.0, 0.0), 1.0),
            unit(mate, 1, Warrior, Vec3::new(0.0, 1.0, 12.0), 1.0),
        ]);
        let auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
        let c = ctx(healer, &units, &auras, &obstacles);
        let ring = ring_of(&obstacles, 0).unwrap();
        assert!((ring.rho - 8.0).abs() < 1e-4);
        let arc = feasible_arc(&c, &ring, Vec2::new(0.0, 12.0), 39.0, 1.0);
        let all = runs(&arc);
        assert_eq!(all.len(), 1, "{all:?}");
        // The anchor's own bearing (+z, a quarter turn) is in it; the far side
        // (-z) is its shadow and is not.
        assert!(arc[sample_of(std::f32::consts::FRAC_PI_2)]);
        assert!(!arc[sample_of(-std::f32::consts::FRAC_PI_2)]);
    }

    /// The margin comes off the edge the anchor's motion sweeps toward the
    /// healer: the low end of the run when the anchor moves with the angle,
    /// both ends while it stands still. The pair: with the margin off, the
    /// core is the whole run.
    #[test]
    fn the_core_keeps_the_margin_off_the_trailing_edge() {
        let ring = ring_of(&pillar(), 0).unwrap();
        let k = margin_samples(&ring, 0.0);
        assert!(
            k >= 4,
            "3yd on an 8yd ring is about 21 degrees: {k} samples"
        );
        let run = (10, 30);
        let core = core_of(&ring, run, (1, 0.0), true);
        assert!(!core[10] && !core[10 + k - 1] && core[10 + k] && core[39]);
        let core = core_of(&ring, run, (-1, 0.0), true);
        assert!(core[10] && core[39 - k] && !core[39 - k + 1]);
        let core = core_of(&ring, run, (0, 0.0), true);
        assert!(!core[10] && !core[39] && core[25]);
        // The mutation's pair: no margin, the whole run.
        let core = core_of(&ring, run, (1, 0.0), false);
        assert!((10..40).all(|i| core[i]));
        // A run shorter than its margin keeps its middle.
        let core = core_of(&ring, (10, 3), (1, 0.0), true);
        assert_eq!(core.iter().filter(|c| **c).count(), 1);
        assert!(core[11]);
    }

    /// The arc key wants a run wide enough to stand the margin inside both
    /// edges; a sliver no wider than the two margins is no hug.
    #[test]
    fn a_sliver_of_arc_holds_no_hug() {
        let ring = ring_of(&pillar(), 0).unwrap();
        let k = margin_samples(&ring, 0.0);
        let sliver: Vec<usize> = (10..10 + 2 * k).collect();
        assert!(!holds_margin(&ring, &mask(&sliver)));
        let wide: Vec<usize> = (10..11 + 2 * k).collect();
        assert!(holds_margin(&ring, &mask(&wide)));
    }

    /// Changing side turns through the sighted arc — the near side — never
    /// through the gap the anchor's shadow leaves. The pair: turned the shorter
    /// way, the same change crosses the gap.
    #[test]
    fn a_side_change_turns_through_the_arc() {
        // The arc runs from sample 10 to sample 59 (a 250-degree arc); the
        // shadow is samples 60..72 and 0..10.
        let run = (10, 50);
        let from = sample_angle(12);
        let to = sample_angle(57);
        let through = turn_toward(run, from, to, true);
        assert!(through > 0.0, "up the arc, the long way round: {through}");
        assert!((through - 45.0 * ring_step()).abs() < 1e-4);
        let short = turn_toward(run, from, to, false);
        assert!(short < 0.0, "the shorter way crosses the shadow: {short}");
        // From outside the arc, the shorter way back to its nearer end.
        let outside = sample_angle(2);
        let back = turn_toward(run, outside, to, true);
        assert!((back - 8.0 * ring_step()).abs() < 1e-4, "{back}");
    }

    /// The target sits nearest the current angle, all else equal — and a spot
    /// out of a ready crowd control's disc beats a nearer one in it.
    #[test]
    fn the_target_is_out_of_reach_before_it_is_near() {
        let obstacles = pillar();
        let ring = ring_of(&obstacles, 0).unwrap();
        let run = (0, RING_SAMPLES);
        let core = vec![true; RING_SAMPLES];
        let none = CcField::default();
        let quiet = Ranking {
            field: &none,
            casters: &[],
            others: &[],
        };
        let theta0 = sample_angle(20);
        assert_eq!(
            target_angle(&obstacles, &ring, run, &core, theta0, &quiet),
            theta0
        );
        // A disc that covers the healer's side of the ring (centred off +x,
        // reaching 14yd): the target leaves it.
        let field = CcField {
            threats: vec![super::super::cc_reach::CcThreat {
                source: Entity::from_raw(99),
                ability: crate::states::play_match::AbilityType::Fear,
                center: ring.point(theta0) * 2.0,
                reach: 14.0,
                needs_sight: false,
                weight: 8.0,
            }],
            margin: 0.0,
        };
        let ranked = Ranking {
            field: &field,
            casters: &[],
            others: &[],
        };
        let target = target_angle(&obstacles, &ring, run, &core, theta0, &ranked);
        assert!(
            field
                .covering(&obstacles, ring.point(target), Disc::Outer)
                .next()
                .is_none(),
            "{target}"
        );
    }
}
