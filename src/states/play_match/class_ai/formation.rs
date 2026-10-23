//! The FREE formation point's sight constraint (AS-206), shared by every healer
//! posture machine that stands on a formation point (the Priest's, and the
//! caster-healer machine the Shaman and Druid share).
//!
//! The formation point is arithmetic on positions — the engaged-ally centroid
//! offset behind the line, clamped into wand range and the arena, projected onto
//! standable ground — and none of that arithmetic knows about sight. On a pillar
//! map it can land in a pillar's shadow from the very teammates the healer is
//! there to heal, and a FREE healer then idles blind: the medic chase walks it
//! round only for a teammate below `urgency_hp_threshold`, so a teammate at 60%
//! behind the pillar never draws it out, and its heals sit refused `LosBlocked`.
//!
//! **The rule:** the formation point keeps sight of every living non-pet
//! teammate when some point near it does; when none does, of the most injured
//! one (the anchor). It is a CONSTRAINT with a nearest-feasible repair — the
//! same shape as `nearest_standable` — not a scored term: the formation point is
//! one goal, not a choice among scored steps. A point in no teammate's shadow
//! (every point on an obstacle-free map) comes back bit-identical.
//!
//! **Where the repair looks.** Not a lattice round the point: a lattice moves
//! with its centre, so the winner hops between cells as the fight drifts
//! (`team_solve::candidates_for` explains the pathology). The candidates are
//! anchored to the geometry that hides the teammate: an obstacle's shadow from a
//! teammate is the wedge between the two tangent rays from the teammate past the
//! obstacle's footprint disc, and a point in that wedge — widened by
//! [`SIGHT_MARGIN`] — has one exit per side, the point moved out until it stands
//! the margin beyond that side's ray ([`shadow_exits`]).
//!
//! **How far, and which.** Only a shadow cast by an obstacle near the point
//! counts (a teammate close behind a distant pillar casts one whose edges sweep
//! across the point far faster than the teammate moves). Distances are measured
//! from the raw point, not its standable projection (which jumps between a
//! pillar's faces). An exit is taken in full to [`REPAIR_FULL`] from the raw
//! point, part of the way to [`REPAIR_LIMIT`], and not at all beyond. The exit
//! nearest the one last chosen counts [`KEEP_MARGIN`] nearer, so the point
//! keeps its side of a shadow, and one of two teammates' shadows behind a
//! pillar, until another exit is better by the band or its own loses the
//! anchor.
//!
//! **What still steps.** Each exit is continuous in the raw point across every
//! edge of its region — the side edges by construction, the shadow's near end
//! and the near-obstacle limit by fading the move out to nothing over
//! [`EDGE_FADE`] — so the repair starts and lets go where the point stands. A
//! change of exit (the kept one lost the anchor, or another became better by
//! the band) still moves the point the whole gap between the two exits at
//! once. That step is left a step: the two exits stand either side of a
//! teammate's shadow (or of two teammates' shadows behind one pillar), so any
//! continuous path between them crosses ground out of that teammate's sight or
//! detours round the pillar's near side, and easing the goal across blind
//! ground was ruled out (AS-206). With the heading continuous they are rare:
//! about six per 100 Warrior+Mage+Priest v Rogue+Warlock+Druid matches on
//! Nagrand move the goal over 5yd, much as before AS-370.
//!
//! **The heading (AS-370).** The raw point stands `formation_offset` off the
//! engaged-ally centroid along "behind the line", away from the enemy nearest
//! it. That direction flips whenever the enemy crosses the centroid, and the
//! point flipped with it — 5-16yd in a frame, the bulk of the formation goal's
//! jumps. [`turn_limited`] turns the heading from the last one no faster than
//! the healer can walk round the centroid, so the point swings round instead;
//! and [`walk_is_stale`] re-issues a walk that a moving point has left behind.
//!
//! **Profile.** `TeamPlan` only (the AI-profile rule: every new AI behaviour
//! lands gated). The callers ask [`sight_constrained`] and [`continuous`];
//! `Legacy` keeps the unconstrained point and stays byte-identical.

use bevy::prelude::*;

use crate::states::play_match::combat_core::clamp_to_arena;
use crate::states::play_match::components::{FormationHeading, MovementDirective, MovementGoal};
use crate::states::play_match::map_geometry::{
    has_line_of_sight, nearest_standable, ObstacleVolume, EYE_HEIGHT,
};

use super::cc_reach::{CcField, CcThreat, Disc};
use super::{CombatContext, CombatantInfo};

/// How far past the edge of a teammate's shadow the repaired point stands.
///
/// It exceeds the formation dead zone (1.5yd, `formation_deadzone`), within
/// which no walk is issued, so a healer parked anywhere inside the dead zone of
/// a repaired point is still out of the shadow. The edge it is measured from is
/// the tangent to the obstacle's footprint DISC — the circumscribed circle for a
/// Nagrand prism — which is itself already outside the true silhouette.
pub const SIGHT_MARGIN: f32 = 2.5;

/// How much better (nearer, in [`sighted_formation_point`]'s ranking) another
/// exit must be before the repaired formation point leaves the one it was
/// last repaired to.
///
/// The hysteresis band. Two exits of one shadow sit either side of its centre
/// line, so a point near the line has two about equally near, and the exits of
/// two teammates' shadows behind one pillar lie apart in the same way; switching
/// between them is a jump of the gap. With the band the point crosses about half
/// of it before it switches. 6yd is three commit-window walks at base speed,
/// wide enough that the fight's drift does not carry the point back and forth,
/// narrow enough that a point well past the line takes the shorter way out.
pub const KEEP_MARGIN: f32 = 6.0;

/// The kept exit is the candidate within this distance of where the point was
/// last repaired to: an exit moves with the point and the teammate, a few
/// tenths of a yard a frame near the pillar.
pub const KEEP_RADIUS: f32 = 3.0;

/// How much nearer, in yards, a candidate counts for each teammate it sees
/// beyond the first (the anchor first: a candidate that sees the anchor always
/// beats one that does not).
///
/// Sight of a second teammate is worth having, but not at any distance: as an
/// outright priority it overrode the band, and a 3v3 healer flipped ~15yd
/// across a shadow the moment the far exit saw two teammates. At 3yd — half the
/// band — one more teammate in sight takes the point to a new exit only when
/// that exit is decidedly the nearer.
pub const TEAMMATE_YARDS: f32 = 3.0;

/// Up to this distance from the raw formation point, an exit is taken in full.
///
/// The defect the repair exists for is a point a few yards inside a shadow cast
/// across a nearby pillar, whose nearest exit is about the shadow's half-width
/// plus `SIGHT_MARGIN` away — 7-9yd on Nagrand's pillars at the distances
/// healers stand, a little more from a raw point deep inside a footprint.
pub const REPAIR_FULL: f32 = 10.0;

/// Beyond this distance from the raw formation point, an exit is not taken at
/// all; between [`REPAIR_FULL`] and here the point is moved only part of the
/// way, in proportion. So the repair never sends the point far from where the
/// healer would otherwise stand, and fades out rather than snapping as an exit
/// recedes.
///
/// Exits much farther than a near pillar's belong to a teammate standing close
/// behind a pillar far from the healer, whose shadow is wide where the healer
/// is: following them sent the point tens of yards across the arena, and their
/// exits swing fast as the teammate moves — fast enough to outrun the band (a
/// shadow's edges are rays from the teammate, so an exit's speed grows with its
/// distance). Within this limit they cannot.
pub const REPAIR_LIMIT: f32 = 16.0;

/// The last stretch, in yards, over which an exit's move out fades to nothing
/// toward the shadow's near end and toward the near-obstacle limit
/// ([`shadow_exits`]), so neither edge is a step.
pub const EDGE_FADE: f32 = 3.0;

/// How much of the way to an exit `dist` yards from the raw point the repair
/// moves the point: all of it to [`REPAIR_FULL`], none of it from
/// [`REPAIR_LIMIT`], linear between.
pub fn repair_weight(dist: f32) -> f32 {
    ((REPAIR_LIMIT - dist) / (REPAIR_LIMIT - REPAIR_FULL)).clamp(0.0, 1.0)
}

/// The formation point's heading off the engaged-ally centroid, turned from
/// the last one toward `target` by at most `rate` radians a second (AS-370).
///
/// `target` is the "behind the line" direction as the formation machine
/// derives it: away from the enemy nearest the centroid, blended toward the
/// arena centre. That direction is discontinuous wherever the nearest enemy
/// crosses the centroid — a melee fighting at its teammate's feet swaps sides
/// of it constantly — and each crossing turned it half way round, so the point
/// `formation_offset` out along it jumped up to twice that, 5-16yd in one
/// frame, under both AI profiles. Turned at a bounded rate, the point swings
/// round the centroid instead, and a target that flickers back and forth turns
/// it by no more than the flicker lasts.
///
/// The callers pass `rate` as the healer's walking speed over
/// `formation_offset`, so the point never circles the centroid faster than the
/// healer can follow it round. The rate is per second of sim time between
/// evaluations, not per evaluation, so a healer that was casting (and not
/// evaluated) finds the point turned by what the cast lasted.
///
/// A heading within reach of `target` takes `target` exactly, so a formation
/// whose direction never turns faster than `rate` is the unconstrained one.
/// With nothing remembered (the first evaluation, or after
/// [`FormationMemory::forget`](crate::states::play_match::FormationMemory::forget))
/// the heading is `target`. A zero `target` (enemy and centroid at the arena
/// centre) keeps the last heading.
pub fn turn_limited(
    target: Vec2,
    heading: &mut Option<FormationHeading>,
    now: f32,
    rate: f32,
) -> Vec2 {
    let dir = match *heading {
        None => target,
        Some(last) if target == Vec2::ZERO => last.dir,
        Some(last) => {
            let reach = rate * (now - last.at).max(0.0);
            let angle = last.dir.angle_to(target);
            if angle.abs() <= reach {
                target
            } else {
                Vec2::from_angle(reach.copysign(angle))
                    .rotate(last.dir)
                    .normalize_or(last.dir)
            }
        }
    };
    if dir != Vec2::ZERO {
        *heading = Some(FormationHeading { dir, at: now });
    }
    dir
}

/// Start the heading from the healer's bearing off the centroid when the last
/// hug has just ended (`FormationMemory::seed_from_healer`, AS-373), so the
/// open point takes over where the healer stands and turns from there rather
/// than jumping to the "behind the line" side. A no-op otherwise.
pub fn seed_heading(
    memory: &mut crate::states::play_match::components::FormationMemory,
    centroid: Vec3,
    my_pos: Vec3,
    now: f32,
) {
    if !memory.seed_from_healer {
        return;
    }
    memory.seed_from_healer = false;
    let dir = (xz(my_pos) - xz(centroid)).normalize_or_zero();
    if memory.heading.is_none() && dir != Vec2::ZERO {
        memory.heading = Some(FormationHeading { dir, at: now });
    }
}

/// Whether the formation point moves continuously under this unit's AI
/// profile (AS-370): its heading turns at a bounded rate ([`turn_limited`]) and
/// a walk gone stale is re-issued ([`walk_is_stale`]). `TeamPlan` only, like
/// the sight repair ([`sight_constrained`]); `Legacy` keeps the point as it
/// was and stays byte-identical.
///
/// Under it, too, a healer parked at its formation point keeps its `Point`
/// directive alive (AS-362) instead of letting it lapse. A lapsed directive
/// handed the legs to target pursuit until the free tick re-issued the walk,
/// so a parked healer stepped toward its target and back every directive TTL:
/// 0.7yd each way, about a heading reversal a second by AS-249's metric. A
/// healer kept FREE longer (out of a closing caster's reach, so not pressured
/// by it) showed it most.
pub fn continuous(ctx: &CombatContext) -> bool {
    ctx.ai_profile.is_team_plan()
}

/// Whether the healer's live walk (`directive`) aims at a point more than
/// `threshold` (the machine's `formation_shift_threshold`) from the formation
/// point it would issue now, `point`.
///
/// The free tick re-commits when the point has moved past the threshold since
/// it last committed — but not while the healer stands within the dead zone of
/// the new point, and a walk kept alive by a refresh aims at the point as it
/// stood then. So a healer standing at a moving formation point could be left
/// walking toward a point well away from it (AS-370). This asks the walk
/// itself. A directive that is not a `Point` walk, or none, is not stale.
pub fn walk_is_stale(directive: Option<&MovementDirective>, point: Vec2, threshold: f32) -> bool {
    directive.is_some_and(|d| match d.goal {
        MovementGoal::Point(p) => xz(p).distance(point) > threshold,
        _ => false,
    })
}

/// Whether the formation point is held to sight of the teammates under this
/// unit's AI profile: `TeamPlan` only. With the pillar modes on
/// ([`open_constrained`]) the HEADING is held first, and this repair still runs
/// on the point it gives: a no-op wherever some heading kept the point out of
/// every near shadow, a repair only in `open_heading`'s anchor-only and
/// unconstrained fallbacks.
pub fn sight_constrained(ctx: &CombatContext) -> bool {
    ctx.ai_profile.is_team_plan()
}

/// Whether the open mode's sight constraint holds the formation HEADING
/// (AS-373, [`open_heading`]): `TeamPlan` with the pillar modes on, on a map
/// with obstacles. It holds the heading ahead of the point's sight repair
/// ([`sighted_formation_point`]), which still runs after it; with the modes off
/// a `TeamPlan` healer has the repair alone.
pub fn open_constrained(ctx: &CombatContext) -> bool {
    ctx.pillar_modes() && !ctx.obstacles.is_empty()
}

/// Heading samples round the centroid the open constraint is judged on: one
/// every 2 degrees, about a quarter yard of the point's travel at the
/// formation offset.
pub const HEADING_SAMPLES: usize = 180;

/// The headings, as sample indices, whose point would stand in a teammate's
/// shadow: per sample, a bit per teammate (bit 0 the anchor) whose widened
/// shadow near its obstacle holds the point ([`shadow_exits`] non-empty —
/// AS-206's wedge and near-obstacle limit).
fn heading_shadows(
    ctx: &CombatContext,
    allies: &[&CombatantInfo],
    point_at: &impl Fn(Vec2) -> Vec2,
) -> Vec<u8> {
    let step = std::f32::consts::TAU / HEADING_SAMPLES as f32;
    (0..HEADING_SAMPLES)
        .map(|i| {
            let p = point_at(Vec2::from_angle(i as f32 * step));
            let mut mask = 0u8;
            for (k, ally) in allies.iter().enumerate().take(8) {
                let viewer = xz(ally.position);
                // A shadow the repair would not have left (its nearest exit
                // `REPAIR_LIMIT` or more away) is not forbidden either:
                // turning round it would carry the point as far.
                if ctx.obstacles.iter().any(|o| {
                    shadow_exits(viewer, o, p, SIGHT_MARGIN)
                        .iter()
                        .any(|(_, e)| e.distance(p) < REPAIR_LIMIT)
                }) {
                    mask |= 1 << k;
                }
            }
            mask
        })
        .collect()
}

/// The open mode's formation heading (AS-373, `TeamPlan` with the pillar modes
/// on): [`turn_limited`] with the sight constraint moved from the point to the
/// heading.
///
/// `point_at` maps a heading to the raw formation point it gives (the offset
/// off the centroid, the wand and burn pulls, the arena clamp). A heading
/// whose point stands in a near teammate's widened shadow is FORBIDDEN — the
/// same wedge and near-obstacle limit as AS-206's repair ([`shadow_exits`]),
/// every teammate's while some heading avoids them all, else the anchor's
/// alone, else none (the unconstrained [`turn_limited`]). Then:
///
/// - a remembered heading the forbidden set has grown over is pushed to the
///   nearest allowed one: the interval's edge moves with the geometry, so the
///   heading is pushed, not flipped;
/// - an allowed target is reached the shorter way round when that way is
///   allowed, else the OTHER way round the centroid — through the open, in
///   sight at every step — else the heading goes as far toward it as it can
///   and holds at the boundary;
/// - a forbidden target holds the heading at the boundary of the forbidden
///   interval it lies in, on the side the heading is already on.
///
/// The turn is limited to `rate` radians a second, as [`turn_limited`]'s. With
/// nothing forbidden it IS [`turn_limited`], bit for bit. `allow_through` is
/// a mutation switch: `true` lets the heading turn through the forbidden set.
#[allow(clippy::too_many_arguments)]
pub fn open_heading(
    ctx: &CombatContext,
    healer: Entity,
    target: Vec2,
    heading: &mut Option<FormationHeading>,
    now: f32,
    rate: f32,
    point_at: impl Fn(Vec2) -> Vec2,
    allow_through: bool,
) -> Vec2 {
    let allies = teammates(ctx, healer);
    let shadows = heading_shadows(ctx, &allies, &point_at);
    let mut allowed: Vec<bool> = shadows.iter().map(|m| *m == 0).collect();
    if !allowed.iter().any(|a| *a) {
        allowed = shadows.iter().map(|m| m & 1 == 0).collect();
    }
    let a0 = heading.map(|h| h.dir).unwrap_or(target);
    let n = HEADING_SAMPLES;
    let step = std::f32::consts::TAU / n as f32;
    let index =
        |angle: f32| ((angle.rem_euclid(std::f32::consts::TAU) / step).round() as usize) % n;
    if allow_through
        || allowed.iter().all(|a| *a)
        || !allowed.iter().any(|a| *a)
        || a0 == Vec2::ZERO
    {
        return turn_limited(target, heading, now, rate);
    }
    let ok = |i: usize| allowed[i % n];

    // Push a heading the forbidden set has grown over to the nearest allowed
    // sample (the nearer side; the side of increasing angle on a tie).
    let mut from = a0.to_angle();
    let i0 = index(from);
    if !ok(i0) {
        let pushed = (1..n).find_map(|k| {
            if ok(i0 + k) {
                Some((i0 + k) % n)
            } else if ok(i0 + n - k) {
                Some((i0 + n - k) % n)
            } else {
                None
            }
        });
        if let Some(p) = pushed {
            from = p as f32 * step;
        }
    }
    let i_from = index(from);
    // How many samples one may walk from `i_from` in `sense` before the next
    // is forbidden (n when the whole circle is allowed that way).
    let reach_in = |sense: i32| -> usize {
        (1..n)
            .take_while(|&k| ok(((i_from as i32 + sense * k as i32).rem_euclid(n as i32)) as usize))
            .count()
    };
    let wrap = |a: f32| {
        let t = a.rem_euclid(std::f32::consts::TAU);
        if t > std::f32::consts::PI {
            t - std::f32::consts::TAU
        } else {
            t
        }
    };
    let to = if target == Vec2::ZERO {
        from
    } else {
        target.to_angle()
    };
    let i_to = index(to);
    let short = wrap(to - from);
    let sense_short: i32 = if short >= 0.0 { 1 } else { -1 };
    let steps = |delta: f32| (delta.abs() / step).round() as usize;
    let delta: f32 = if ok(i_to) {
        if steps(short) <= reach_in(sense_short) {
            short
        } else {
            let long = short - sense_short as f32 * std::f32::consts::TAU;
            if steps(long) <= reach_in(-sense_short) {
                long
            } else {
                // Held at the boundary on the short side.
                sense_short as f32 * reach_in(sense_short) as f32 * step
                    + wrap(i_from as f32 * step - from)
            }
        }
    } else {
        // The target lies in a forbidden interval: the boundary of the
        // allowed run the heading is in that borders it, the nearer one when
        // both do (the side the heading is already on).
        let borders = |sense: i32| -> Option<usize> {
            let r = reach_in(sense);
            // Past the boundary, through forbidden samples only, to the target?
            let mut k = r + 1;
            while k < n {
                let i = ((i_from as i32 + sense * k as i32).rem_euclid(n as i32)) as usize;
                if i == i_to {
                    return Some(r);
                }
                if ok(i) {
                    return None;
                }
                k += 1;
            }
            None
        };
        let pick = match (borders(1), borders(-1)) {
            (Some(a), Some(b)) if a <= b => Some((1, a)),
            (Some(_), Some(b)) => Some((-1, b)),
            (Some(a), None) => Some((1, a)),
            (None, Some(b)) => Some((-1, b)),
            (None, None) => None,
        };
        match pick {
            Some((sense, r)) => sense as f32 * r as f32 * step + wrap(i_from as f32 * step - from),
            None => 0.0,
        }
    };
    let dir = match *heading {
        None => Vec2::from_angle(from + delta),
        Some(last) => {
            let reach = rate * (now - last.at).max(0.0);
            Vec2::from_angle(from + delta.clamp(-reach, reach))
        }
    };
    *heading = Some(FormationHeading { dir, at: now });
    dir
}

/// How well a spot sees the teammates: the anchor first, then how many.
/// Ordered so a greater value is better sight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sight {
    /// The anchor (the most injured teammate) is in sight.
    pub anchor: bool,
    /// How many teammates are in sight, the anchor included.
    pub seen: usize,
}

fn eye(p: Vec2) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.y)
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// The teammates the formation point answers to: every living non-pet ally but
/// the healer itself, the anchor (the most injured; the lowest entity on a tie)
/// first.
pub(super) fn teammates<'c>(ctx: &'c CombatContext, healer: Entity) -> Vec<&'c CombatantInfo> {
    let mut allies: Vec<&CombatantInfo> = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != healer)
        .collect();
    // Stable sort over BTree (entity) order: ties keep the lowest entity first.
    allies.sort_by(|a, b| a.health_pct().total_cmp(&b.health_pct()));
    allies
}

fn sight_from(obstacles: &[ObstacleVolume], spot: Vec2, allies: &[&CombatantInfo]) -> Sight {
    let mut sight = Sight {
        anchor: false,
        seen: 0,
    };
    for (i, a) in allies.iter().enumerate() {
        if has_line_of_sight(obstacles, eye(spot), eye(xz(a.position))) {
            sight.seen += 1;
            if i == 0 {
                sight.anchor = true;
            }
        }
    }
    sight
}

/// How well `spot` sees the healer's teammates (anchor, then count).
pub fn sight_of_teammates(ctx: &CombatContext, healer: Entity, spot: Vec2) -> Sight {
    sight_from(ctx.obstacles, spot, &teammates(ctx, healer))
}

/// The exits from `obstacle`'s shadow from `viewer` for a point `raw` standing
/// in it, each with the side (`1` or `-1`) of the tangent ray it lies beyond.
///
/// The shadow is the wedge between the two tangent rays from `viewer` past the
/// obstacle's footprint disc, beyond the tangent points, widened by `margin`
/// on each side. A point in that widened wedge has one exit per side: the point
/// moved out until it stands `margin` beyond that side's ray. Empty when `raw`
/// is not in the widened wedge, or the viewer stands at the disc's centre (no
/// bearing to take a shadow along).
///
/// Empty, too, when `raw` stands farther from the obstacle than the viewer
/// does and beyond [`REPAIR_FULL`] of its footprint: a teammate close behind a
/// distant pillar. Its shadow's edges are rays from the teammate, so where the
/// point stands they sweep at the teammate's speed times the ratio of the two
/// distances — 1.5yd a frame when the teammate stepped round a pillar 60yd away
/// — and an exit that fast cannot be followed. Near the pillar, or nearer it
/// than the teammate, the ratio is small and the edge moves about as fast as
/// the teammate does.
///
/// **Every exit is continuous in `raw` across every edge of the region it is
/// defined on.** A point exactly `margin` beyond a side's ray is its own exit
/// on that side. Toward the shadow's near end (the tangent points) and toward
/// the near-obstacle limit, the move out is scaled down to nothing over the
/// last [`EDGE_FADE`]. So as the point drifts into the region the repair starts
/// from where it is, and as it drifts out the repair lets go where it is.
pub fn shadow_exits(
    viewer: Vec2,
    obstacle: &ObstacleVolume,
    raw: Vec2,
    margin: f32,
) -> Vec<(i8, Vec2)> {
    let (center, radius) = obstacle.footprint_disc();
    let to_center = center - viewer;
    let dist = to_center.length();
    if dist <= f32::EPSILON {
        return Vec::new();
    }
    let along = to_center / dist;
    // A viewer flush against a prism's face stands inside its circumradius: its
    // shadow is all but a half-plane, so cap the half-angle just short of 90°.
    let half = (radius / dist).min(0.98).asin();
    // How far inside the near-obstacle limit (no farther from the obstacle
    // than the teammate is, or within `REPAIR_FULL` of its footprint).
    let inside_limit = dist.max(radius + REPAIR_FULL) - raw.distance(center);
    let rel = raw - viewer;
    // How far beyond the tangent points, measured along the centre line.
    let beyond_near_end = rel.dot(along) - dist * half.cos() * half.cos();
    if inside_limit <= 0.0 || beyond_near_end <= 0.0 {
        return Vec::new();
    }
    let fade = (inside_limit.min(beyond_near_end) / EDGE_FADE).min(1.0);
    let sides = [1_i8, -1].map(|side| {
        let sign = f32::from(side);
        let ray = Vec2::from_angle(sign * half).rotate(along);
        // Outward: a further quarter turn away from the centre line.
        let outward = Vec2::from_angle(sign * std::f32::consts::FRAC_PI_2).rotate(ray);
        (side, outward, rel.dot(outward))
    });
    if sides.iter().any(|&(_, _, out)| out >= margin) {
        return Vec::new();
    }
    sides
        .into_iter()
        .map(|(side, outward, out)| (side, raw + outward * (margin - out) * fade))
        .collect()
}

/// The formation point held to sight of the healer's teammates (AS-206).
///
/// `raw` is the formation point as its machine computed it, clamped into the
/// arena but NOT yet projected onto standable ground; the healer would stand at
/// its projection (`healer_postures::standable_formation_point`). When `raw`
/// stands in no teammate's widened shadow near its obstacle ([`shadow_exits`])
/// — always, on an obstacle-free map — that projection is returned unchanged,
/// exactly what the unconstrained machine returns, and `kept` is cleared.
/// Otherwise the point moves toward the best exit that sees the teammates no
/// worse than the projection does, by [`repair_weight`] of the way, and `kept`
/// records that exit; with no such exit within [`REPAIR_LIMIT`] of `raw`, the
/// projection stands and `kept` is cleared. A point moved only part of the way
/// may not see: past [`REPAIR_FULL`] the repair trades sight for staying near
/// the formation.
///
/// Distances are measured from `raw`, not from its projection. A raw point
/// inside a pillar's footprint moves continuously, but its projection jumps
/// from face to face as it crosses the footprint's medial line, and measuring
/// from the projection carried those jumps into the repair.
///
/// "Best": an exit that sees the anchor beats one that does not; among the
/// rest the nearest wins, each teammate seen beyond the first counting
/// [`TEAMMATE_YARDS`] nearer, and the exit nearest the one last chosen (`kept`,
/// carried on the healer's posture state; within [`KEEP_RADIUS`] of it)
/// counting [`KEEP_MARGIN`] nearer. So the point keeps to its exit — over a
/// shadow's centre line, or past another teammate's exit behind the same
/// pillar — until another is better by the band, or its own loses the anchor.
/// A change of exit moves the point by the gap between the two in one
/// evaluation.
pub fn sighted_formation_point(
    ctx: &CombatContext,
    healer: Entity,
    raw: Vec3,
    kept: &mut Option<Vec2>,
) -> Vec3 {
    let point = super::healer_postures::standable_formation_point(ctx, raw);
    if ctx.obstacles.is_empty() {
        return point;
    }
    let allies = teammates(ctx, healer);
    let origin = xz(point);
    let from = xz(raw);
    let mut origin_sight: Option<Sight> = None;
    // (spot, distance from raw, sight) of every exit that repairs the point.
    let mut candidates: Vec<(Vec2, f32, Sight)> = Vec::new();
    for ally in &allies {
        let ally_xz = xz(ally.position);
        for obstacle in ctx.obstacles {
            for (_, exit) in shadow_exits(ally_xz, obstacle, from, SIGHT_MARGIN) {
                let clamped = clamp_to_arena(&ctx.bounds, Vec3::new(exit.x, point.y, exit.y));
                let Some(spot) = nearest_standable(ctx.obstacles, xz(clamped), point.y) else {
                    continue;
                };
                let dist = spot.distance(from);
                if dist >= REPAIR_LIMIT {
                    continue;
                }
                let sight = sight_from(ctx.obstacles, spot, &allies);
                // An exit that sees worse than the point itself is no repair.
                let at_origin =
                    *origin_sight.get_or_insert_with(|| sight_from(ctx.obstacles, origin, &allies));
                if sight >= at_origin {
                    candidates.push((spot, dist, sight));
                }
            }
        }
    }
    // The kept exit: the candidate nearest the exit last chosen, if within
    // `KEEP_RADIUS` of it.
    let kept_index = kept.and_then(|last| {
        candidates
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.0.distance(last)))
            .filter(|&(_, d)| d <= KEEP_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    });
    let ranked = |i: usize| {
        let (_, dist, sight) = candidates[i];
        let band = if kept_index == Some(i) {
            KEEP_MARGIN
        } else {
            0.0
        };
        // Each teammate seen beyond the first counts as `TEAMMATE_YARDS`
        // nearer: seeing one more teammate is a trade against distance, not an
        // override of the band.
        dist - band - TEAMMATE_YARDS * sight.seen.saturating_sub(1) as f32
    };
    let best = (0..candidates.len()).min_by(|&a, &b| {
        // The anchor first (`true` sorts before `false` here), then the rank.
        candidates[b]
            .2
            .anchor
            .cmp(&candidates[a].2.anchor)
            .then(ranked(a).total_cmp(&ranked(b)))
    });
    *kept = best.map(|i| candidates[i].0);
    let Some(i) = best else {
        return point;
    };
    let (spot, dist, _) = candidates[i];
    let w = repair_weight(dist);
    if w >= 1.0 {
        return Vec3::new(spot.x, point.y, spot.y);
    }
    let partial = origin + (spot - origin) * w;
    let partial = nearest_standable(ctx.obstacles, partial, point.y).unwrap_or(partial);
    Vec3::new(partial.x, point.y, partial.y)
}

/// Whether the formation point is held out of enemy crowd-control reach under
/// this unit's AI profile (AS-362): `TeamPlan` only, like the sight repair.
pub fn reach_constrained(ctx: &CombatContext) -> bool {
    ctx.ai_profile.is_team_plan()
}

/// Yards of heal-range slack a spot needs before the reach repair STARTS: once
/// live, a spot inside heal reach is enough, but a repair that only just fits
/// would let go and take hold again as the teammates step. The feasibility
/// half of the repair's hysteresis; the disc's Schmitt pair is the other.
pub const REACH_SLACK: f32 = 1.0;

/// Yards past a disc's outer rim a repaired point stands, so it is still out
/// of the disc after the projection onto standable ground.
const RIM_CLEARANCE: f32 = 0.25;

/// Up to this distance from the formation point, the reach repair's spot is
/// taken in full. Wider than the sight repair's [`REPAIR_FULL`]: a caster's
/// reach is 30yd, and with the teammate fighting near the caster the nearest
/// spot out of it is 20-25yd from the formation point. A shorter limit left
/// the FREE point inside the reach the PRESSURED solve had just walked the
/// healer out of, and the healer walked back in until the closing caster
/// pressured it again — AS-249's bob, measured at three reversals in eight
/// seconds on `tests/healer_cc_avoidance.rs`'s strobe stage.
pub const REACH_REPAIR_FULL: f32 = 24.0;

/// Beyond this distance from the formation point the reach repair's spot is
/// not taken at all; between [`REACH_REPAIR_FULL`] and here, part of the way.
/// Its own bound, the sight repair's [`REPAIR_LIMIT`] reasoning in reach
/// terms: never carry the point across the arena.
pub const REACH_REPAIR_LIMIT: f32 = 30.0;

/// [`repair_weight`] on the reach repair's own bounds.
fn reach_repair_weight(dist: f32) -> f32 {
    ((REACH_REPAIR_LIMIT - dist) / (REACH_REPAIR_LIMIT - REACH_REPAIR_FULL)).clamp(0.0, 1.0)
}

/// Whether walking straight from `from` to `to` takes a mover deeper into the
/// disc `(center, radius)` than `from` already is: through it from outside, or
/// further in from inside. A walk that starts inside and only moves outward
/// does not.
fn walks_through(from: Vec2, to: Vec2, center: Vec2, radius: f32) -> bool {
    let step = to - from;
    let len2 = step.length_squared();
    if len2 <= f32::EPSILON {
        return false;
    }
    let t = ((center - from).dot(step) / len2).clamp(0.0, 1.0);
    t > 1e-3 && (from + step * t).distance(center) < radius - 1e-2
}

/// The points where the circles `(c1, r1)` and `(c2, r2)` cross — none, or
/// two (a tangent pair counts as two equal points).
fn circle_crossings(c1: Vec2, r1: f32, c2: Vec2, r2: f32) -> Vec<Vec2> {
    let d = c1.distance(c2);
    if d <= f32::EPSILON || d > r1 + r2 || d < (r1 - r2).abs() {
        return Vec::new();
    }
    let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - along * along).max(0.0).sqrt();
    let axis = (c2 - c1) / d;
    let mid = c1 + axis * along;
    let perp = axis.perp();
    vec![mid + perp * h, mid - perp * h]
}

/// The formation point held out of the reach of the enemy crowd control that
/// can land on the healer (AS-362, `TeamPlan` only).
///
/// `point` is the formation point after the sight repair
/// ([`sighted_formation_point`]), on standable ground. The repair is a
/// CONSTRAINT with a nearest-feasible repair — the sight repair's shape, not a
/// push. A **feasible** spot is:
///
/// - out of every disc of `field` (the outer disc, [`Disc::Outer`]);
/// - within `heal_reach` of every teammate `point` was within `heal_reach` of
///   (the most injured teammate when it was within reach of none) — the heal
///   comes first, so a spot out of reach is no repair at all;
/// - seeing the teammates no worse than `point` does, so the reach repair
///   cannot undo the sight repair it runs on;
/// - standable, inside the arena.
///
/// The **candidates** are anchored to the discs that cover `point`: the rim
/// point nearest `point`, the rim point nearest the healer, where each rim
/// crosses each teammate's heal circle, and where two rims cross. The nearest
/// feasible one to `point` wins, the one nearest the spot last chosen (`kept`)
/// counting [`KEEP_MARGIN`] nearer. Each candidate slides continuously as the
/// caster and the teammates move, so the point does too; only a change of
/// candidate is a step, and the band covers that. Like the sight repair, the
/// spot is taken in full to [`REPAIR_FULL`] from `point`, part of the way to
/// [`REPAIR_LIMIT`], and not at all beyond ([`repair_weight`]): a disc the
/// formation sits deep inside is the PRESSURED solve's business, not a reason
/// to carry the formation across the arena.
///
/// **Hysteresis.** While no repair is live (`kept` is `None`), `point` stands
/// unless it is inside a disc's INNER radius ([`Disc::Inner`]), and a repair
/// starts only where a feasible spot has [`REACH_SLACK`] of heal range to
/// spare. While one is live, `point` stands only once it is out of every outer
/// disc. With no feasible spot (the teammates' heal circles lie wholly in
/// reach — a 3v3 with casters on both flanks) `point` stands: heal first.
///
/// An empty `field` — every map, every `Legacy` call, a healer no ready crowd
/// control reaches — returns `point` unchanged.
pub fn reach_repaired_point(
    ctx: &CombatContext,
    healer: Entity,
    my_pos: Vec3,
    point: Vec3,
    field: &CcField,
    heal_reach: f32,
    kept: &mut Option<Vec2>,
) -> Vec3 {
    let origin = xz(point);
    let live = kept.is_some();
    let judged = if live { Disc::Outer } else { Disc::Inner };
    let covering: Vec<&CcThreat> = field.covering(ctx.obstacles, origin, judged).collect();
    if covering.is_empty() {
        *kept = None;
        return point;
    }
    let allies = teammates(ctx, healer);
    let mut held: Vec<Vec2> = allies
        .iter()
        .map(|a| xz(a.position))
        .filter(|a| a.distance(origin) <= heal_reach)
        .collect();
    if held.is_empty() {
        if let Some(anchor) = allies.first() {
            held.push(xz(anchor.position));
        }
    }
    let reach = if live {
        heal_reach
    } else {
        heal_reach - REACH_SLACK
    };
    let origin_sight =
        (!ctx.obstacles.is_empty()).then(|| sight_from(ctx.obstacles, origin, &allies));

    let mut raw: Vec<Vec2> = Vec::new();
    let rims: Vec<(Vec2, f32)> = covering
        .iter()
        .map(|t| (t.center, field.radius(t, Disc::Outer) + RIM_CLEARANCE))
        .collect();
    let me = xz(my_pos);
    for &(center, rim) in &rims {
        for from in [origin, me] {
            let away = (from - center).normalize_or_zero();
            if away != Vec2::ZERO {
                raw.push(center + away * rim);
            }
        }
        for &ally in &held {
            raw.extend(circle_crossings(center, rim, ally, reach));
        }
    }
    for (i, &(c1, r1)) in rims.iter().enumerate() {
        for &(c2, r2) in &rims[i + 1..] {
            raw.extend(circle_crossings(c1, r1, c2, r2));
        }
    }

    let feasible: Vec<Vec2> = raw
        .into_iter()
        .filter_map(|c| {
            let clamped = clamp_to_arena(&ctx.bounds, Vec3::new(c.x, point.y, c.y));
            nearest_standable(ctx.obstacles, xz(clamped), point.y)
        })
        .filter(|&c| {
            field
                .covering(ctx.obstacles, c, Disc::Outer)
                .next()
                .is_none()
        })
        .filter(|&c| held.iter().all(|a| a.distance(c) <= reach + 1e-3))
        .filter(|&c| origin_sight.is_none_or(|at| sight_from(ctx.obstacles, c, &allies) >= at))
        .collect();
    let rank = |c: Vec2| {
        let band = if kept.is_some_and(|k| k.distance(c) <= KEEP_RADIUS) {
            KEEP_MARGIN
        } else {
            0.0
        };
        c.distance(origin) - band
    };
    // A spot the healer would have to walk through a disc to reach comes
    // after every spot it would not: the far rim of a caster standing between
    // the healer and its formation point is nearest the point, and walking to
    // it carries the healer through the reach it is avoiding — into the
    // closing caster's PRESSURED onset, back out, and in again.
    let through = |c: Vec2| {
        covering
            .iter()
            .any(|t| walks_through(me, c, t.center, field.radius(t, Disc::Outer)))
    };
    // The spot last chosen stands while it is still a repair — out of every
    // disc, in heal reach and sight of the teammates, within reach of the
    // point, and reached without walking through a disc — as a satisfied
    // solve stands still. Re-picking the nearest spot every evaluation made
    // the point follow the caster's every step: back in when it stepped back,
    // out when it stepped in, and the healer's heading reversed with it.
    if let Some(k) = *kept {
        let still = k.distance(origin) < REACH_REPAIR_FULL
            && field
                .covering(ctx.obstacles, k, Disc::Outer)
                .next()
                .is_none()
            && held.iter().all(|a| a.distance(k) <= heal_reach)
            && origin_sight.is_none_or(|at| sight_from(ctx.obstacles, k, &allies) >= at)
            && !through(k);
        if still {
            return Vec3::new(k.x, point.y, k.y);
        }
    }
    let best = feasible
        .into_iter()
        .filter(|c| c.distance(origin) < REACH_REPAIR_LIMIT)
        .min_by(|a, b| {
            through(*a)
                .cmp(&through(*b))
                .then(rank(*a).total_cmp(&rank(*b)))
        });
    // With no spot it can reach without walking through a disc, a healer
    // already standing out of reach — and in heal reach and sight of its
    // teammates — stays where it stands, as a satisfied solve does. Its own
    // spot is judged by the inner disc: the Schmitt pair the solve uses for
    // the spot a healer stands on. The goal is then where the HEALER stands,
    // however far that is from the formation point: this branch is not bound
    // by `REACH_REPAIR_LIMIT`, and taking it (or leaving it) can move the goal
    // further than the limit in one evaluation.
    let stays = best.is_none_or(through)
        && field
            .covering(ctx.obstacles, me, Disc::Inner)
            .next()
            .is_none()
        && held.iter().all(|a| a.distance(me) <= heal_reach)
        && origin_sight.is_none_or(|at| sight_from(ctx.obstacles, me, &allies) >= at);
    if stays {
        *kept = Some(me);
        return Vec3::new(me.x, point.y, me.y);
    }
    *kept = best;
    let Some(spot) = best else {
        return point;
    };
    // As far as the sight repair goes, and no further: in full to
    // `REPAIR_FULL`, fading to nothing at `REPAIR_LIMIT`, so the repair never
    // carries the point across the arena and lets go without a step as its
    // spot recedes.
    let w = reach_repair_weight(spot.distance(origin));
    let moved = origin + (spot - origin) * w;
    let moved = if w >= 1.0 {
        spot
    } else {
        nearest_standable(ctx.obstacles, moved, point.y).unwrap_or(moved)
    };
    Vec3::new(moved.x, point.y, moved.y)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::states::match_config::CharacterClass::{Druid, Mage, Warrior};
    use crate::states::play_match::ai_profile::AiProfile;
    use crate::states::play_match::class_ai::reach_fixture::{pillar, roster, unit};
    use crate::states::play_match::components::DRTracker;
    use crate::states::play_match::{AbilityType, Aura};

    static NO_DR: BTreeMap<Entity, DRTracker> = BTreeMap::new();
    static NO_CDS: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();
    static NO_AURAS: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();

    /// The healer, and its teammates at `(x, z, health)`.
    struct Fixture {
        healer: Entity,
        roster: BTreeMap<Entity, CombatantInfo>,
    }

    fn fixture(teammates: &[(f32, f32, f32)]) -> Fixture {
        let mut world = World::new();
        let healer = world.spawn_empty().id();
        let mut units = vec![unit(healer, 1, Druid, Vec3::new(-10.0, 1.0, 0.0), 1.0)];
        for (i, &(x, z, hp)) in teammates.iter().enumerate() {
            let class = if i == 0 { Mage } else { Warrior };
            units.push(unit(
                world.spawn_empty().id(),
                1,
                class,
                Vec3::new(x, 1.0, z),
                hp,
            ));
        }
        Fixture {
            healer,
            roster: roster(units),
        }
    }

    impl Fixture {
        fn ctx<'a>(&'a self, obstacles: &'a [ObstacleVolume]) -> CombatContext<'a> {
            CombatContext::new(
                self.healer,
                1,
                &self.roster,
                &NO_AURAS,
                &NO_DR,
                &NO_CDS,
                obstacles,
                Default::default(),
                AiProfile::TeamPlan,
            )
        }

        /// One evaluation, reading and updating the kept exit `state` as the
        /// healer's posture carries it from one evaluation to the next.
        fn repair_from(
            &self,
            obstacles: &[ObstacleVolume],
            point: Vec2,
            state: &mut Option<Vec2>,
        ) -> Vec2 {
            xz(sighted_formation_point(
                &self.ctx(obstacles),
                self.healer,
                Vec3::new(point.x, 1.0, point.y),
                state,
            ))
        }

        /// Evaluations at a standing `point` until the goal settles (the kept
        /// exit can change on the first).
        fn settle_from(
            &self,
            obstacles: &[ObstacleVolume],
            point: Vec2,
            state: &mut Option<Vec2>,
        ) -> Vec2 {
            let mut goal = self.repair_from(obstacles, point, state);
            for _ in 0..1000 {
                let next = self.repair_from(obstacles, point, state);
                if next == goal {
                    break;
                }
                goal = next;
            }
            goal
        }

        /// The settled goal for `point` from a healer with no kept exit.
        fn repair(&self, obstacles: &[ObstacleVolume], point: Vec2) -> Vec2 {
            self.settle_from(obstacles, point, &mut None)
        }
    }

    fn sees(obstacles: &[ObstacleVolume], a: Vec2, b: Vec2) -> bool {
        has_line_of_sight(obstacles, eye(a), eye(b))
    }

    fn cylinder(x: f32, z: f32, radius: f32) -> ObstacleVolume {
        ObstacleVolume::Cylinder {
            center_xz: Vec2::new(x, z),
            radius,
            base_y: 0.0,
            height: 10.0,
        }
    }

    /// The fixture pillar stands at (8, 0), radius 2: the Mage at (16, 0) is
    /// behind it from the origin.
    const MAGE: (f32, f32, f32) = (16.0, 0.0, 0.7);
    const MAGE_XZ: Vec2 = Vec2::new(16.0, 0.0);
    const BLIND: Vec2 = Vec2::ZERO;

    #[test]
    fn a_point_in_sight_comes_back_bit_identical() {
        let f = fixture(&[MAGE]);
        let open = Vec2::new(0.0, 9.0);
        assert!(sees(&pillar(), open, MAGE_XZ));
        let mut state = None;
        assert_eq!(f.repair_from(&pillar(), open, &mut state), open);
        assert_eq!(state, None);
        // And every point, on an obstacle-free map.
        assert_eq!(f.repair_from(&[], BLIND, &mut state), BLIND);
    }

    /// The pair to the above: the point behind the pillar is moved out of the
    /// Mage's shadow, and only about as far as sight needs.
    #[test]
    fn a_blind_point_moves_just_clear_of_the_shadow() {
        let f = fixture(&[MAGE]);
        assert!(!sees(&pillar(), BLIND, MAGE_XZ), "the control is blind");
        let fixed = f.repair(&pillar(), BLIND);
        assert!(sees(&pillar(), fixed, MAGE_XZ), "{fixed}");
        // The shadow's half-width 16yd behind the Mage is 16 tan(asin(2/8)),
        // about 4.1yd: the repair lands about a margin past it, no further.
        let half_width = 16.0 * (2.0_f32 / 8.0).asin().tan();
        let moved = fixed.distance(BLIND);
        assert!(
            moved > half_width && moved < half_width + SIGHT_MARGIN + 0.5,
            "moved {moved}"
        );
        // The whole formation dead zone round the repaired point sees the Mage,
        // so a healer parked anywhere the walk stops is not blind.
        for i in 0..16 {
            let p = fixed + Vec2::from_angle(i as f32 * std::f32::consts::TAU / 16.0) * 1.5;
            assert!(
                sees(&pillar(), p, MAGE_XZ),
                "{p} in the dead zone of {fixed}"
            );
        }
    }

    /// The goals for `points` evaluated in turn, the repair carried from one to
    /// the next when `remember`, else each settled from no kept exit; and
    /// the largest step between consecutive goals.
    fn largest_step(f: &Fixture, points: &[Vec2], remember: bool) -> (f32, Vec<Vec2>) {
        let mut state = None;
        let goals: Vec<Vec2> = points
            .iter()
            .map(|&p| {
                if remember {
                    f.repair_from(&pillar(), p, &mut state)
                } else {
                    f.repair(&pillar(), p)
                }
            })
            .collect();
        let step = goals
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .fold(0.0, f32::max);
        (step, goals)
    }

    /// Settled from no kept exit, a small move of the point moves the
    /// repaired goal a small distance.
    #[test]
    fn the_repaired_point_slides_continuously() {
        let f = fixture(&[MAGE]);
        let points: Vec<Vec2> = (0..20)
            .map(|i| Vec2::new(-(i as f32) * 0.1, 0.5 - i as f32 * 0.02))
            .collect();
        let (step, goals) = largest_step(&f, &points, false);
        assert!(step < 0.5, "hop of {step}yd: {goals:?}");
    }

    /// The point drifting over the shadow's centre line (z = 0, the line from
    /// the Mage through the pillar) in 0.05yd steps, inside the hysteresis band.
    fn across_the_centre_line() -> Vec<Vec2> {
        (0..=80)
            .map(|i| Vec2::new(0.0, 2.0 - i as f32 * 0.05))
            .collect()
    }

    /// The point keeps its side as it drifts over the centre line. The pair:
    /// settled with no exit kept, the same drift flips the goal across the
    /// whole shadow at the centre line.
    #[test]
    fn the_point_keeps_its_side_across_the_centre_line() {
        let f = fixture(&[MAGE]);
        let points = across_the_centre_line();
        let mut state = None;
        f.settle_from(&pillar(), points[0], &mut state);
        let mut goals = Vec::new();
        for &p in &points {
            goals.push(f.repair_from(&pillar(), p, &mut state));
        }
        let kept = goals
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .fold(0.0, f32::max);
        assert!(kept < 0.5, "a repair stepped {kept}yd: {goals:?}");
        assert!(goals.iter().all(|p| p.y > 0.0), "the point left its side");
        let (forgetful, _) = largest_step(&f, &points, false);
        assert!(
            forgetful > 8.0,
            "without a kept exit the drift should flip the point across the shadow \
             (largest step {forgetful}yd) — the scene no longer crosses the centre line"
        );
    }

    /// The band is a band, not a lock: a point carried well past the centre
    /// line changes exit once — when the other is nearer by `KEEP_MARGIN`,
    /// about half the band past the line — and stays changed.
    #[test]
    fn the_point_changes_exit_once_well_past_the_centre_line() {
        let f = fixture(&[MAGE]);
        let points: Vec<Vec2> = (0..=85)
            .map(|i| Vec2::new(0.0, 0.5 - i as f32 * 0.05))
            .collect();
        let mut kept = None;
        let mut goals = Vec::new();
        let mut exits = Vec::new();
        for &p in &points {
            goals.push(f.repair_from(&pillar(), p, &mut kept));
            exits.push(kept.expect("an exit kept"));
        }
        let changes: Vec<usize> = (1..exits.len())
            .filter(|&k| exits[k - 1].y * exits[k].y < 0.0)
            .collect();
        assert_eq!(changes.len(), 1, "{exits:?}");
        let crossed = -points[changes[0]].y;
        assert!(
            crossed > KEEP_MARGIN / 2.0 - 0.5 && crossed < KEEP_MARGIN / 2.0 + 0.5,
            "changed exit {crossed}yd past the centre line"
        );
        // The goal follows the exit across the shadow, from +z to -z.
        assert!(
            goals[0].y > 2.0 && goals[goals.len() - 1].y < -2.0,
            "{goals:?}"
        );
    }

    /// Sight of every teammate wins when some spot has it; when none does,
    /// the anchor (the most injured) is never given up for the others.
    #[test]
    fn the_anchor_comes_first_then_every_teammate() {
        // The Mage at 70% behind the pillar; a Warrior at 40% in sight.
        let warrior = Vec2::new(0.0, 16.0);
        let f = fixture(&[MAGE, (warrior.x, warrior.y, 0.4)]);
        let fixed = f.repair(&pillar(), BLIND);
        assert!(sees(&pillar(), fixed, MAGE_XZ) && sees(&pillar(), fixed, warrior));

        // A second pillar hides the Warrior too: the spots that clear the
        // Mage's shadow do not all clear the Warrior's, and the Warrior is
        // the anchor.
        let two = vec![pillar()[0], cylinder(0.0, 4.0, 2.0)];
        assert!(!sees(&two, BLIND, warrior));
        let fixed = f.repair(&two, BLIND);
        let sight = sight_of_teammates(&f.ctx(&two), f.healer, fixed);
        assert!(sight.anchor, "{fixed}: {sight:?}");
    }

    /// The open mode (AS-373): the heading turns round the centroid at the
    /// Mage, from the +z side of it to the -z side, and the shorter way
    /// crosses the Mage's shadow behind the pillar (headings near -x). Every
    /// heading on the way stands its point out of the shadow, and the turn
    /// arrives the other way round, through the open. The pair: allowed
    /// through, the same turn crosses the shadow.
    #[test]
    fn the_open_heading_turns_round_the_sighted_side() {
        use std::f32::consts::PI;
        let f = fixture(&[MAGE]);
        let obstacles = pillar();
        let ctx = f.ctx(&obstacles);
        let point_at = |d: Vec2| MAGE_XZ + d * 8.0;
        let in_shadow =
            |d: Vec2| !shadow_exits(MAGE_XZ, &obstacles[0], point_at(d), SIGHT_MARGIN).is_empty();
        let start = Vec2::from_angle(PI * 100.0 / 180.0);
        let target = Vec2::from_angle(PI * 260.0 / 180.0);
        assert!(
            in_shadow(Vec2::from_angle(PI)),
            "the scene puts the shorter way through the shadow"
        );
        for through in [false, true] {
            let mut heading = Some(FormationHeading {
                dir: start,
                at: 0.0,
            });
            let mut crossed = 0;
            let mut last = start;
            for k in 1..=80 {
                last = open_heading(
                    &ctx,
                    f.healer,
                    target,
                    &mut heading,
                    k as f32 * 0.1,
                    1.0,
                    point_at,
                    through,
                );
                if in_shadow(last) {
                    crossed += 1;
                }
            }
            if through {
                assert!(
                    crossed > 0,
                    "allowed through, the turn should cross the shadow"
                );
            } else {
                assert_eq!(crossed, 0, "a heading stood its point in the shadow");
                assert!(last.angle_to(target).abs() < 1e-3, "arrived at {last}");
            }
        }
    }

    /// With nothing forbidden the open heading IS `turn_limited`, bit for bit.
    #[test]
    fn the_open_heading_is_turn_limited_in_the_open() {
        let f = fixture(&[(0.0, 16.0, 0.7)]);
        let ctx = f.ctx(&[]);
        let mut a = Some(FormationHeading {
            dir: Vec2::X,
            at: 0.0,
        });
        let mut b = a;
        let target = Vec2::from_angle(2.5);
        for k in 1..=30 {
            let now = k as f32 * 0.05;
            let x = open_heading(&ctx, f.healer, target, &mut a, now, 1.3, |d| d * 8.0, false);
            let y = turn_limited(target, &mut b, now, 1.3);
            assert_eq!(x, y);
        }
    }

    #[test]
    fn legacy_is_not_constrained() {
        let f = fixture(&[MAGE]);
        let obstacles = pillar();
        let mut ctx = f.ctx(&obstacles);
        assert!(sight_constrained(&ctx));
        ctx.ai_profile = AiProfile::Legacy;
        assert!(!sight_constrained(&ctx));
    }

    /// The point drifting from open ground into the Mage's shadow and on to its
    /// centre line, 0.05yd a step, and back: the repair takes over from where
    /// the point stands, with no step at the shadow's side edge, and lets go
    /// the same way.
    #[test]
    fn the_repair_starts_and_ends_where_the_point_stands() {
        let f = fixture(&[MAGE]);
        let inward: Vec<Vec2> = (0..=160)
            .map(|i| Vec2::new(0.0, 8.0 - i as f32 * 0.05))
            .collect();
        let mut both = inward.clone();
        both.extend(inward.iter().rev());
        let (step, goals) = largest_step(&f, &both, true);
        assert!(step < 0.2, "a repair stepped {step}yd: {goals:?}");
        // Not vacuous: the far end is blind and repaired; the near end is in
        // sight and left alone.
        assert!(!sees(&pillar(), both[160], MAGE_XZ));
        assert!(goals[160].distance(both[160]) > SIGHT_MARGIN);
        assert_eq!(goals[0], both[0]);
        assert_eq!(goals[both.len() - 1], both[0]);
    }

    /// The largest step of each side's exit, and its last move out, as `raw`
    /// walks `points` (exits present from the first point on).
    fn exit_track(viewer: Vec2, obstacle: &ObstacleVolume, points: &[Vec2]) -> (f32, f32, f32) {
        let mut last: Option<Vec<(i8, Vec2)>> = None;
        let (mut step, mut first_out, mut last_out) = (0.0_f32, 0.0_f32, f32::MAX);
        for (k, &p) in points.iter().enumerate() {
            let exits = shadow_exits(viewer, obstacle, p, SIGHT_MARGIN);
            if exits.is_empty() {
                break;
            }
            let out = exits.iter().map(|e| e.1.distance(p)).fold(0.0, f32::max);
            if k == 0 {
                first_out = out;
            }
            last_out = out;
            if let Some(prev) = &last {
                for (a, b) in prev.iter().zip(&exits) {
                    step = step.max(a.1.distance(b.1));
                }
            }
            last = Some(exits);
        }
        (step, first_out, last_out)
    }

    /// Toward the shadow's near end (the tangent points), the exits fade into
    /// the point itself: walking `raw` up the shadow to where it ends, no exit
    /// steps, and the last ones before the end are the point.
    #[test]
    fn the_exits_fade_out_at_the_shadows_near_end() {
        // The Mage at (16, 0), the pillar at (8, 0) r2: the shadow's near end
        // is 7.5yd from the Mage along the centre line, at x = 8.5.
        let points: Vec<Vec2> = (0..=500)
            .map(|i| Vec2::new(4.0 + i as f32 * 0.01, 0.5))
            .collect();
        let (step, first_out, last_out) = exit_track(MAGE_XZ, &pillar()[0], &points);
        assert!(
            first_out > SIGHT_MARGIN,
            "the walk starts deep in the shadow"
        );
        assert!(step < 0.1, "an exit stepped {step}yd");
        assert!(
            last_out < 0.05,
            "the exits before the near end move {last_out}yd"
        );
    }

    /// Toward the near-obstacle limit the exits fade into the point too.
    #[test]
    fn the_exits_fade_out_at_the_near_obstacle_limit() {
        // The Mage 3yd from the pillar's centre: the limit is 12yd from the
        // centre, at x = -4.
        let mage = Vec2::new(11.0, 0.0);
        let points: Vec<Vec2> = (0..=500)
            .map(|i| Vec2::new(-0.5 - i as f32 * 0.01, 0.0))
            .collect();
        let (step, first_out, last_out) = exit_track(mage, &pillar()[0], &points);
        assert!(first_out > SIGHT_MARGIN);
        assert!(step < 0.1, "an exit stepped {step}yd");
        assert!(
            last_out < 0.05,
            "the exits before the limit move {last_out}yd"
        );
    }

    /// A Mage close behind the pillar casts a wide shadow: a point well behind
    /// the pillar is repaired only part of the way (its exit lies between
    /// `REPAIR_FULL` and `REPAIR_LIMIT`), one just behind it in full.
    #[test]
    fn a_far_exit_is_taken_part_of_the_way() {
        // The pillar at (8, 0) r2; the Mage at (11, 0), 3yd from its centre.
        let f = fixture(&[(11.0, 0.0, 0.7)]);
        let mage = Vec2::new(11.0, 0.0);
        let near = Vec2::new(5.0, 0.0);
        let fixed = f.repair(&pillar(), near);
        assert!(sees(&pillar(), fixed, mage), "near: in full ({fixed})");
        assert!(fixed.distance(near) < REPAIR_FULL);

        let mid = Vec2::new(-1.0, 0.0);
        let partial = f.repair(&pillar(), mid);
        let moved = partial.distance(mid);
        let exit = shadow_exits(mage, &pillar()[0], mid, SIGHT_MARGIN)
            .into_iter()
            .map(|(_, e)| e.distance(mid))
            .fold(f32::MAX, f32::min);
        assert!(exit > REPAIR_FULL && exit < REPAIR_LIMIT, "exit {exit}yd");
        assert!(
            moved > 1.0 && moved < exit - 0.3,
            "mid: moved part of the way ({moved}yd of {exit}yd, to {partial})"
        );
    }

    /// An exit `REPAIR_LIMIT` or more from the point is not taken: a Nagrand-
    /// sized pillar with the Mage close behind it, the point well back.
    #[test]
    fn an_exit_beyond_the_limit_is_not_taken() {
        let obstacles = vec![cylinder(8.0, 0.0, 6.0)];
        let mage = Vec2::new(14.5, 0.0);
        let f = fixture(&[(mage.x, mage.y, 0.7)]);
        let far = Vec2::new(-5.0, 0.0);
        assert!(!sees(&obstacles, far, mage));
        let exits = shadow_exits(mage, &obstacles[0], far, SIGHT_MARGIN);
        assert!(
            !exits.is_empty() && exits.iter().all(|e| e.1.distance(far) >= REPAIR_LIMIT),
            "{exits:?}"
        );
        let mut state = None;
        assert_eq!(
            f.repair_from(&obstacles, far, &mut state),
            far,
            "left alone"
        );
        assert_eq!(state, None, "no exit kept");
    }

    /// A point back in sight forgets its exit: the next repair takes the
    /// nearer exit, where a kept exit would have held it on the far one.
    #[test]
    fn a_point_in_sight_forgets_its_kept_exit() {
        let f = fixture(&[MAGE]);
        let mut state = None;
        let first = f.settle_from(&pillar(), Vec2::new(0.0, 0.5), &mut state);
        assert!(first.y > 0.0);
        // Back in open ground, for one evaluation: no exit is kept.
        f.repair_from(&pillar(), Vec2::new(0.0, 9.0), &mut state);
        assert_eq!(state, None);
        // Into the shadow again just -z of the centre line: the -z exit is
        // nearer, and nothing holds the point on +z.
        let again = f.settle_from(&pillar(), Vec2::new(0.0, -1.5), &mut state);
        assert!(again.y < 0.0, "held on the side it had left: {again}");
        // The control: without the open-ground evaluation, the band holds +z.
        let mut held = None;
        f.settle_from(&pillar(), Vec2::new(0.0, 0.5), &mut held);
        let kept = f.settle_from(&pillar(), Vec2::new(0.0, -1.5), &mut held);
        assert!(kept.y > 0.0, "the band should hold +z: {kept}");
    }

    /// The band belongs to the exit nearest the one last chosen, within
    /// `KEEP_RADIUS` of it: 2yd off still holds, 4yd off holds nothing.
    #[test]
    fn the_band_is_for_the_exit_last_chosen() {
        let f = fixture(&[MAGE]);
        let mut state = None;
        f.settle_from(&pillar(), Vec2::new(0.0, 0.5), &mut state);
        let exit = state.expect("an exit kept");
        let off = |dx: f32| Some(exit + Vec2::new(dx, 0.0));
        let within = f.settle_from(&pillar(), Vec2::new(0.0, -1.5), &mut off(-2.0));
        assert!(within.y > 0.0, "an exit 2yd off should hold: {within}");
        let beyond = f.settle_from(&pillar(), Vec2::new(0.0, -1.5), &mut off(-4.0));
        assert!(beyond.y < 0.0, "an exit 4yd off held the point: {beyond}");
    }

    /// With a second teammate behind the pillar the point stands in two
    /// shadows and has an exit from each; drifting over them, it keeps to its
    /// exit. The control: settled with no kept exit, the same drift jumps
    /// between exits.
    #[test]
    fn the_point_keeps_its_exit_with_two_teammates_behind_the_pillar() {
        // The Mage behind the pillar on the x axis; a healthy Warrior just
        // behind it, a yard off the axis.
        let f = fixture(&[MAGE, (10.0, 1.0, 0.9)]);
        let points: Vec<Vec2> = (0..=120)
            .map(|i| Vec2::new(0.0, 3.0 - i as f32 * 0.05))
            .collect();
        let mut state = None;
        f.settle_from(&pillar(), points[0], &mut state);
        let goals: Vec<Vec2> = points
            .iter()
            .map(|&p| f.repair_from(&pillar(), p, &mut state))
            .collect();
        let kept = goals
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .fold(0.0, f32::max);
        let (free, _) = largest_step(&f, &points, false);
        assert!(kept < 0.5, "a kept repair stepped {kept}yd: {goals:?}");
        assert!(
            free > 6.0,
            "with no kept exit the drift should change exits (largest step {free}yd)"
        );
    }

    /// Only a shadow the point stands near the obstacle in counts: a teammate
    /// close behind the pillar (the Mage 3yd from its centre) casts one whose
    /// edges sweep fast where the point stands far behind. A point 13yd from the
    /// pillar's centre — farther than the Mage and beyond `REPAIR_FULL` of the
    /// footprint — is left alone though blind; one 11yd from it is repaired.
    #[test]
    fn a_shadow_counts_only_near_its_obstacle() {
        let f = fixture(&[(11.0, 0.0, 0.7)]);
        let mage = Vec2::new(11.0, 0.0);
        let far = Vec2::new(-5.0, 0.0);
        assert!(!sees(&pillar(), far, mage), "the far point is blind");
        assert!(shadow_exits(mage, &pillar()[0], far, SIGHT_MARGIN).is_empty());
        assert_eq!(f.repair(&pillar(), far), far, "far: left alone");

        let near = Vec2::new(-3.0, 0.0);
        assert!(!sees(&pillar(), near, mage));
        assert!(
            f.repair(&pillar(), near).distance(near) > 1.0,
            "near: repaired"
        );
    }

    // ------------------------------------------------------------------
    // The heading turns at a bounded rate (AS-370)
    // ------------------------------------------------------------------

    const RATE: f32 = 0.625; // 5yd/s walk over the 8yd offset

    /// Evaluate `turn_limited` every frame for `secs`, toward `target(t)`,
    /// from `start`; returns every heading.
    fn turn_track(start: Vec2, secs: f32, target: impl Fn(f32) -> Vec2) -> Vec<Vec2> {
        let mut heading = Some(FormationHeading {
            dir: start,
            at: 0.0,
        });
        (1..=(secs * 60.0) as usize)
            .map(|k| {
                let t = k as f32 / 60.0;
                turn_limited(target(t), &mut heading, t, RATE)
            })
            .collect()
    }

    /// A target that turns no faster than the rate is taken exactly: the
    /// heading is the unconstrained direction whenever that direction is
    /// continuous.
    #[test]
    fn a_slow_target_is_taken_exactly() {
        let slow = |t: f32| Vec2::from_angle(0.5 * RATE * t);
        let track = turn_track(Vec2::X, 3.0, slow);
        for (k, h) in track.iter().enumerate() {
            assert_eq!(*h, slow((k + 1) as f32 / 60.0), "frame {k}");
        }
    }

    /// The defect: the nearest enemy crosses the centroid and the target flips
    /// half way round in one frame. The heading swings round at the rate — the
    /// point 8yd out along it moves at most the healer's walk a second, never
    /// 16yd at once — and arrives once the turn has had its time.
    #[test]
    fn a_flipped_target_is_swung_round_at_the_rate() {
        let track = turn_track(Vec2::X, 6.0, |_| -Vec2::X);
        let offset = 8.0;
        for pair in track.windows(2) {
            let step = (pair[1] - pair[0]).length() * offset;
            assert!(step <= RATE * offset / 60.0 + 1e-3, "stepped {step}yd");
        }
        let turned_by = |k: usize| track[k].angle_to(Vec2::X).abs();
        assert!(
            (turned_by(59) - RATE).abs() < 0.02,
            "after 1s the heading should have turned {RATE} rad, not {}",
            turned_by(59)
        );
        let arrive = (std::f32::consts::PI / RATE * 60.0).ceil() as usize;
        assert_eq!(track[arrive], -Vec2::X, "arrived by frame {arrive}");
        assert!(track.iter().all(|h| (h.length() - 1.0).abs() < 1e-4));
    }

    /// A target flickering across the centroid every frame (a melee at its
    /// teammate's feet) moves the heading by no more than one frame's turn
    /// either way: it does not wander off, and it cannot bob.
    #[test]
    fn a_flickering_target_holds_the_heading() {
        let flicker = |t: f32| {
            if ((t * 60.0).round() as i32) % 2 == 0 {
                Vec2::Y
            } else {
                -Vec2::Y
            }
        };
        let track = turn_track(Vec2::X, 3.0, flicker);
        for h in &track {
            assert!(
                h.angle_to(Vec2::X).abs() <= RATE / 60.0 + 1e-4,
                "the heading wandered to {h}"
            );
        }
    }

    /// The heading is per second of sim time: an evaluation after a gap (a
    /// cast) turns by what the gap lasted; with nothing remembered the target
    /// is taken at once; a zero target keeps the last heading.
    #[test]
    fn the_turn_is_per_second_and_starts_afresh() {
        let mut heading = Some(FormationHeading {
            dir: Vec2::X,
            at: 0.0,
        });
        let after_cast = turn_limited(Vec2::Y, &mut heading, 2.0, RATE);
        assert!((after_cast.angle_to(Vec2::X).abs() - 2.0 * RATE).abs() < 1e-4);
        assert!(after_cast.angle_to(Vec2::X).abs() <= std::f32::consts::FRAC_PI_2);

        assert_eq!(
            turn_limited(Vec2::ZERO, &mut heading, 2.5, RATE),
            after_cast
        );

        let mut fresh = None;
        assert_eq!(turn_limited(-Vec2::Y, &mut fresh, 2.5, RATE), -Vec2::Y);
        assert_eq!(fresh.map(|h| h.dir), Some(-Vec2::Y));
    }

    // -----------------------------------------------------------------------
    // The reach repair (AS-362)
    // -----------------------------------------------------------------------

    use crate::states::play_match::class_ai::cc_reach::CcThreat;

    /// A Fear from (`x`, 0) reaching 30yd, margin 3: inner 31.5, outer 33.
    fn fear_from(x: f32) -> CcField {
        CcField {
            threats: vec![CcThreat {
                source: Entity::from_raw(77),
                ability: AbilityType::Fear,
                center: Vec2::new(x, 0.0),
                reach: 30.0,
                needs_sight: true,
                weight: 8.0,
            }],
            margin: 3.0,
        }
    }

    /// The Warrior at the origin; heal reach 39.
    const WARRIOR_AT: (f32, f32, f32) = (0.0, 0.0, 1.0);
    const HEAL_REACH: f32 = 39.0;

    impl Fixture {
        fn reach_from(&self, field: &CcField, point: Vec2, kept: &mut Option<Vec2>) -> Vec2 {
            xz(reach_repaired_point(
                &self.ctx(&[]),
                self.healer,
                Vec3::new(-10.0, 1.0, 0.0),
                Vec3::new(point.x, 1.0, point.y),
                field,
                HEAL_REACH,
                kept,
            ))
        }
    }

    /// A point out of every disc — and every point, with nothing ready —
    /// comes back bit-identical.
    #[test]
    fn a_point_out_of_reach_comes_back_unchanged() {
        let f = fixture(&[WARRIOR_AT]);
        let mut kept = None;
        let point = Vec2::new(-8.0, 0.0);
        assert_eq!(f.reach_from(&CcField::default(), point, &mut kept), point);
        assert_eq!(f.reach_from(&fear_from(30.0), point, &mut kept), point);
        assert_eq!(kept, None);
    }

    /// The pair: the same point inside the Fear's disc moves just out of it,
    /// staying in heal reach of the Warrior.
    #[test]
    fn a_covered_point_moves_just_out_of_reach_and_keeps_heal_reach() {
        let f = fixture(&[WARRIOR_AT]);
        let mut kept = None;
        let point = Vec2::new(-8.0, 0.0);
        let fixed = f.reach_from(&fear_from(15.0), point, &mut kept);
        let caster = Vec2::new(15.0, 0.0);
        assert!(fixed.distance(caster) > 33.0, "{fixed}");
        assert!(
            fixed.distance(caster) < 33.5,
            "moved further than reach needs: {fixed}"
        );
        assert!(fixed.distance(Vec2::ZERO) <= HEAL_REACH, "{fixed}");
        assert_eq!(kept, Some(fixed));
    }

    /// Heal first: when every spot in heal reach of the Warrior is in reach of
    /// the Fear, the point stands.
    #[test]
    fn with_no_spot_in_heal_reach_out_of_reach_the_point_stands() {
        let f = fixture(&[WARRIOR_AT]);
        let mut kept = None;
        let point = Vec2::new(-8.0, 0.0);
        let mut field = fear_from(0.0);
        field.threats[0].reach = 80.0;
        assert_eq!(f.reach_from(&field, point, &mut kept), point);
        assert_eq!(kept, None);
    }

    /// The Schmitt pair: a point between the inner and the outer disc stands
    /// while no repair is live, and is moved out while one is.
    #[test]
    fn the_repair_starts_inside_the_inner_disc_and_lets_go_outside_the_outer() {
        let f = fixture(&[WARRIOR_AT]);
        // 32yd from the caster: between the discs.
        let point = Vec2::new(-8.0, 0.0);
        let field = fear_from(24.0);
        let mut idle = None;
        assert_eq!(f.reach_from(&field, point, &mut idle), point);
        let mut live = Some(Vec2::new(-8.5, 0.0));
        let moved = f.reach_from(&field, point, &mut live);
        assert!(moved.distance(Vec2::new(24.0, 0.0)) > 33.0, "{moved}");
    }

    /// Heal first: the rim point nearest the formation point stands beyond
    /// heal reach of the Warrior, so the repair takes the rim where it crosses
    /// heal reach instead.
    #[test]
    fn the_repair_keeps_heal_reach_of_the_teammates() {
        let f = fixture(&[WARRIOR_AT]);
        let mut field = fear_from(-28.0);
        field.threats[0].reach = 8.0;
        let point = Vec2::new(-34.0, 0.0);
        let mut kept = None;
        let fixed = f.reach_from(&field, point, &mut kept);
        assert!(fixed.distance(Vec2::new(-28.0, 0.0)) > 11.0, "{fixed}");
        assert!(
            fixed.distance(Vec2::ZERO) <= HEAL_REACH - REACH_SLACK + 1e-3,
            "{fixed} is out of heal reach"
        );
    }

    /// The repair composes with the sight repair: a rim spot the pillar hides
    /// the Mage from is no repair. The Mage at (16, 0) stands behind the
    /// fixture pillar at (8, 0) from the west; the point (0, 5) sees past the
    /// pillar's edge, and the only spots out of the disc above it lie in the
    /// pillar's shadow, so the point stands rather than go blind.
    #[test]
    fn the_repair_never_gives_up_sight_of_a_teammate() {
        let f = fixture(&[MAGE]);
        let mut field = fear_from(0.0);
        field.threats[0].center = Vec2::new(0.0, 10.0);
        field.threats[0].reach = 4.0;
        field.threats[0].needs_sight = false;
        let point = Vec2::new(0.0, 5.0);
        assert!(sees(&pillar(), point, MAGE_XZ));
        assert!(
            !sees(&pillar(), Vec2::new(0.0, 2.75), MAGE_XZ),
            "the rim is blind"
        );
        let mut kept = None;
        let fixed = xz(reach_repaired_point(
            &f.ctx(&pillar()),
            f.healer,
            Vec3::new(-10.0, 1.0, 0.0),
            Vec3::new(point.x, 1.0, point.y),
            &field,
            HEAL_REACH,
            &mut kept,
        ));
        assert!(sees(&pillar(), fixed, MAGE_XZ), "{fixed} lost the Mage");
    }

    /// The repair never carries the point across the arena: a disc whose
    /// nearest spot out lies beyond `REACH_REPAIR_LIMIT` of the point leaves
    /// the point where it stands.
    #[test]
    fn a_spot_beyond_the_repair_limit_is_not_taken() {
        let f = fixture(&[(-2.0, 5.0, 1.0)]);
        let point = Vec2::new(-2.0, 0.0);
        let mut kept = None;
        assert_eq!(f.reach_from(&fear_from(0.0), point, &mut kept), point);
        assert_eq!(kept, None);
    }

    /// The repair at `point` for a healer standing at `me`.
    fn repair_for(f: &Fixture, field: &CcField, point: Vec2, me: Vec2) -> Vec2 {
        xz(reach_repaired_point(
            &f.ctx(&[]),
            f.healer,
            Vec3::new(me.x, 1.0, me.y),
            Vec3::new(point.x, 1.0, point.y),
            field,
            HEAL_REACH,
            &mut None,
        ))
    }

    /// A spot the healer would walk through the disc to reach comes after one
    /// it would not: with a Scream-sized disc between the healer and its
    /// formation point, the near rim wins though the far rim is nearer the
    /// point.
    #[test]
    fn the_repair_does_not_walk_the_healer_through_the_disc() {
        let f = fixture(&[(12.0, 0.0, 1.0)]);
        let mut field = fear_from(0.0);
        field.threats[0].reach = 8.0;
        // A healer beyond heal reach of the Warrior, so it cannot simply stay.
        let fixed = repair_for(&f, &field, Vec2::new(2.0, 0.0), Vec2::new(-30.0, 0.0));
        assert!(fixed.x < 0.0, "sent through the disc to {fixed}");
        assert!(fixed.distance(Vec2::ZERO) > 11.0, "{fixed}");
    }

    /// With no spot it can reach without walking through the disc, a healer
    /// already out of reach and in heal reach of its teammate stays where it
    /// stands — the caster at its Warrior's feet, AS-249's standoff shape.
    #[test]
    fn a_healer_out_of_reach_stays_rather_than_walk_through_it() {
        let f = fixture(&[WARRIOR_AT]);
        let me = Vec2::new(-34.0, 0.0);
        let fixed = repair_for(&f, &fear_from(1.0), Vec2::new(6.0, 0.0), me);
        assert_eq!(fixed, me);
        // The control: the same healer inside the disc is not held there.
        let inside = Vec2::new(-20.0, 0.0);
        assert_ne!(
            repair_for(&f, &fear_from(1.0), Vec2::new(6.0, 0.0), inside),
            inside
        );
    }

    /// Feasibility's hysteresis: a repair STARTS only where a spot has
    /// `REACH_SLACK` of heal reach to spare, and a live one keeps a spot with
    /// less. With a 20yd heal reach the disc leaves a sliver 19.5yd from the
    /// Warrior: inside heal reach, not inside it less the slack.
    #[test]
    fn a_repair_starts_only_with_heal_reach_to_spare() {
        let f = fixture(&[WARRIOR_AT]);
        let mut field = fear_from(5.0);
        field.threats[0].reach = 21.25;
        let point = Vec2::new(-10.0, 0.0);
        let repair = |kept: &mut Option<Vec2>| {
            xz(reach_repaired_point(
                &f.ctx(&[]),
                f.healer,
                Vec3::new(-10.0, 1.0, 0.0),
                Vec3::new(point.x, 1.0, point.y),
                &field,
                20.0,
                kept,
            ))
        };
        let mut idle = None;
        assert_eq!(repair(&mut idle), point);
        assert_eq!(idle, None);
        let mut live = Some(Vec2::new(-19.0, 0.0));
        let moved = repair(&mut live);
        assert!(moved.distance(Vec2::new(5.0, 0.0)) > 24.25, "{moved}");
        assert!(moved.distance(Vec2::ZERO) <= 20.0 + 1e-3, "{moved}");
    }

    /// The repaired spot holds while it is still a repair: a caster stepping
    /// back does not pull the point back in after it, and one stepping in
    /// moves it out. (Following the caster both ways reversed the healer's
    /// heading with every step the caster took.)
    #[test]
    fn the_repaired_point_holds_while_the_caster_steps_back() {
        let f = fixture(&[WARRIOR_AT]);
        let point = Vec2::new(-8.0, 0.0);
        let mut kept = None;
        let first = f.reach_from(&fear_from(15.0), point, &mut kept);
        assert!(first.distance(Vec2::new(15.0, 0.0)) > 33.0, "{first}");
        // Back 2yd: the old spot is still out of reach, so it stands.
        assert_eq!(f.reach_from(&fear_from(17.0), point, &mut kept), first);
        // In 2yd from the start: the old spot is in reach again, so it moves.
        let moved = f.reach_from(&fear_from(13.0), point, &mut kept);
        assert!(moved.distance(Vec2::new(13.0, 0.0)) > 33.0, "{moved}");
        assert!(
            moved.x < first.x,
            "{moved} did not give ground from {first}"
        );
    }

    /// No strobe: a caster walking in, a step a frame, moves the repaired
    /// point a step a frame — never a jump — and the point stays out of reach.
    #[test]
    fn a_closing_caster_slides_the_point_without_a_jump() {
        let f = fixture(&[WARRIOR_AT]);
        let point = Vec2::new(-8.0, 0.0);
        let mut kept = None;
        let mut last: Option<Vec2> = None;
        let mut largest = 0.0f32;
        let mut x = 40.0;
        while x > 10.0 {
            let fixed = f.reach_from(&fear_from(x), point, &mut kept);
            if let Some(l) = last {
                largest = largest.max(l.distance(fixed));
            }
            // Out of reach while the rim is taken in full (beyond, the repair fades).
            if kept.is_some() && 33.25 - (x + 8.0) <= REACH_REPAIR_FULL {
                assert!(fixed.distance(Vec2::new(x, 0.0)) > 33.0, "{fixed} at {x}");
            }
            last = Some(fixed);
            x -= 0.1;
        }
        assert!(
            kept.is_some(),
            "the caster never reached the point: vacuous"
        );
        // The entry step: from inside the inner disc to the outer rim.
        assert!(largest < 2.0, "the point jumped {largest}yd");
    }
}
