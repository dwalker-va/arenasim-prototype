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
//! [`SIGHT_MARGIN`] — has one exit per side, the point moved straight out until
//! it stands the margin beyond that side's ray ([`shadow_exits`]).
//!
//! **Why it does not jump.** Every source of a step was found in a match and
//! removed:
//! - a point on the widened edge is its own exit, so the repair starts and lets
//!   go where the point stands;
//! - the choice of exit has hysteresis: the exit nearest where the point was
//!   last repaired to is kept until another is better by [`KEEP_MARGIN`] or it
//!   loses the anchor, so a point drifting over a shadow's centre line keeps
//!   its side, and one in two teammates' shadows behind the same pillar keeps
//!   to one of them;
//! - only a shadow cast by an obstacle near the point counts: a teammate close
//!   behind a distant pillar casts one whose edges sweep across the point far
//!   faster than the teammate moves;
//! - distances are measured from the raw point, not its standable projection,
//!   which jumps between a pillar's faces;
//! - the repair is bounded and fades: an exit is taken in full to
//!   [`REPAIR_FULL`] from the raw point, part of the way to [`REPAIR_LIMIT`],
//!   and not at all beyond — the far exits of a teammate close behind a distant
//!   pillar swing too fast for any band, and sent the point across the arena.
//!
//! The remaining steps are a change of exit when the kept one loses sight of
//! the anchor, and the raw point's own jumps, which the unconstrained machine
//! makes too.
//!
//! **Profile.** `TeamPlan` only (the AI-profile rule: every new AI behaviour
//! lands gated). The callers ask [`sight_constrained`]; `Legacy` keeps the
//! unconstrained point and stays byte-identical.

use bevy::prelude::*;

use crate::states::play_match::combat_core::clamp_to_arena;
use crate::states::play_match::map_geometry::{
    has_line_of_sight, nearest_standable, ObstacleVolume, EYE_HEIGHT,
};

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

/// How much of the way to an exit `dist` yards from the raw point the repair
/// moves the point: all of it to [`REPAIR_FULL`], none of it from
/// [`REPAIR_LIMIT`], linear between.
pub fn repair_weight(dist: f32) -> f32 {
    ((REPAIR_LIMIT - dist) / (REPAIR_LIMIT - REPAIR_FULL)).clamp(0.0, 1.0)
}

/// Whether the formation point is held to sight of the teammates under this
/// unit's AI profile: `TeamPlan` only.
pub fn sight_constrained(ctx: &CombatContext) -> bool {
    ctx.ai_profile.is_team_plan()
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
fn teammates<'c>(ctx: &'c CombatContext, healer: Entity) -> Vec<&'c CombatantInfo> {
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
/// itself moved straight out until it stands `margin` beyond that side's ray.
/// Empty when `raw` is not in the widened wedge, or the viewer stands at the
/// disc's centre (no bearing to take a shadow along).
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
/// **The exit is continuous in `raw` across the wedge's widened edge:** a point
/// exactly `margin` beyond a ray is its own exit on that side, so as the point
/// drifts into the shadow the repair starts from where it is, and as it drifts
/// out the repair lets go where it is — no step at either end.
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
    // Only a shadow the point stands near the obstacle in: no farther from it
    // than the teammate is, or within `REPAIR_FULL` of its footprint.
    if raw.distance(center) > dist.max(radius + REPAIR_FULL) {
        return Vec::new();
    }
    let rel = raw - viewer;
    // Beyond the tangent points, measured along the centre line.
    if rel.dot(along) < dist * half.cos() * half.cos() {
        return Vec::new();
    }
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
        .map(|(side, outward, out)| (side, raw + outward * (margin - out)))
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
/// [`TEAMMATE_YARDS`] nearer, and the exit nearest where the point was last
/// repaired to (`kept`, carried on the healer's posture state; within
/// [`KEEP_RADIUS`] of it) counting [`KEEP_MARGIN`] nearer. So the point keeps to
/// its exit — over a shadow's centre line, or past another teammate's exit
/// behind the same pillar — until another is better by the band, or its own
/// loses the anchor.
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
    // The kept exit: the candidate nearest where the point was last repaired
    // to, if within `KEEP_RADIUS` of it.
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
        fixture_after(0, teammates)
    }

    /// [`fixture`], with `spare` entities spawned first, so its units are
    /// different entities from another fixture's in the same places.
    fn fixture_after(spare: usize, teammates: &[(f32, f32, f32)]) -> Fixture {
        let mut world = World::new();
        for _ in 0..spare {
            world.spawn_empty();
        }
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

        /// One repair from a healer with no kept exit.
        fn repair(&self, obstacles: &[ObstacleVolume], point: Vec2) -> Vec2 {
            self.repair_from(obstacles, point, &mut None)
        }

        /// One repair, reading and updating the `kept` exit as the healer's
        /// posture state carries it frame to frame.
        fn repair_from(
            &self,
            obstacles: &[ObstacleVolume],
            point: Vec2,
            kept: &mut Option<Vec2>,
        ) -> Vec2 {
            xz(sighted_formation_point(
                &self.ctx(obstacles),
                self.healer,
                Vec3::new(point.x, 1.0, point.y),
                kept,
            ))
        }
    }

    fn sees(obstacles: &[ObstacleVolume], a: Vec2, b: Vec2) -> bool {
        has_line_of_sight(obstacles, eye(a), eye(b))
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
        assert_eq!(f.repair(&pillar(), open), open);
        // And every point, on an obstacle-free map.
        assert_eq!(f.repair(&[], BLIND), BLIND);
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

    /// The repair slides with the fight: a small move of the point moves the
    /// repaired point a small distance, never a hop to the shadow's other side.
    #[test]
    fn the_repaired_point_slides_continuously() {
        let f = fixture(&[MAGE]);
        let mut last = f.repair(&pillar(), Vec2::new(0.0, 0.5));
        for step in 1..20 {
            let point = Vec2::new(-(step as f32) * 0.1, 0.5 - step as f32 * 0.02);
            let now = f.repair(&pillar(), point);
            assert!(
                now.distance(last) < 0.5,
                "hop {last} -> {now} at step {step}"
            );
            last = now;
        }
    }

    /// The largest step between consecutive repairs of `points`, carrying the
    /// kept exit from one to the next when `remember`.
    fn largest_step(f: &Fixture, points: &[Vec2], remember: bool) -> (f32, Vec<Vec2>) {
        let mut kept = None;
        let repaired: Vec<Vec2> = points
            .iter()
            .map(|&p| {
                if !remember {
                    kept = None;
                }
                f.repair_from(&pillar(), p, &mut kept)
            })
            .collect();
        let step = repaired
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .fold(0.0, f32::max);
        (step, repaired)
    }

    /// The point drifting over the shadow's centre line (z = 0, the line from
    /// the Mage through the pillar) in 0.05yd steps, inside the hysteresis band.
    fn across_the_centre_line() -> Vec<Vec2> {
        (0..=80)
            .map(|i| Vec2::new(0.0, 2.0 - i as f32 * 0.05))
            .collect()
    }

    /// The point keeps its side as it drifts over the centre line: no repair
    /// jumps more than a few yards. The pair: with no exit kept (each
    /// repair from scratch), the same drift flips the point across the whole
    /// shadow at the centre line.
    #[test]
    fn the_point_keeps_its_side_across_the_centre_line() {
        let f = fixture(&[MAGE]);
        let points = across_the_centre_line();
        let (kept, repaired) = largest_step(&f, &points, true);
        assert!(kept < 2.0, "a repair jumped {kept}yd: {repaired:?}");
        assert!(
            repaired.iter().all(|p| p.y > 0.0),
            "the point left its side"
        );
        let (forgetful, _) = largest_step(&f, &points, false);
        assert!(
            forgetful > 8.0,
            "without a kept exit the drift should flip the point across the shadow \
             (largest step {forgetful}yd) — the scene no longer crosses the centre line"
        );
    }

    /// The band is a band, not a lock: a point carried well past the centre
    /// line changes side once — when the other exit is nearer by `KEEP_MARGIN`
    /// — and stays changed.
    #[test]
    fn the_point_changes_side_once_well_past_the_centre_line() {
        let f = fixture(&[MAGE]);
        let points: Vec<Vec2> = (0..=85)
            .map(|i| Vec2::new(0.0, 0.5 - i as f32 * 0.05))
            .collect();
        let (_, repaired) = largest_step(&f, &points, true);
        let switches = repaired
            .windows(2)
            .filter(|w| w[0].y * w[1].y < 0.0)
            .count();
        assert_eq!(switches, 1, "{repaired:?}");
        let at = repaired
            .windows(2)
            .position(|w| w[0].y * w[1].y < 0.0)
            .unwrap();
        let crossed = -points[at + 1].y;
        assert!(
            crossed > KEEP_MARGIN / 2.0 - 0.5 && crossed < KEEP_MARGIN / 2.0 + 0.5,
            "switched {crossed}yd past the centre line"
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
        let two = vec![
            pillar()[0],
            ObstacleVolume::Cylinder {
                center_xz: Vec2::new(0.0, 4.0),
                radius: 2.0,
                base_y: 0.0,
                height: 10.0,
            },
        ];
        assert!(!sees(&two, BLIND, warrior));
        let fixed = f.repair(&two, BLIND);
        let sight = sight_of_teammates(&f.ctx(&two), f.healer, fixed);
        assert!(sight.anchor, "{fixed}: {sight:?}");
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
    /// centre line, 0.05yd a step: the repair takes over from where the point
    /// stands, with no step at the shadow's edge — and lets go the same way on
    /// the way back out.
    #[test]
    fn the_repair_starts_and_ends_where_the_point_stands() {
        let f = fixture(&[MAGE]);
        let inward: Vec<Vec2> = (0..=160)
            .map(|i| Vec2::new(0.0, 8.0 - i as f32 * 0.05))
            .collect();
        let mut both = inward.clone();
        both.extend(inward.iter().rev());
        let (step, repaired) = largest_step(&f, &both, true);
        assert!(step < 0.5, "a repair stepped {step}yd: {repaired:?}");
        // Not vacuous: the far end is blind and repaired; the near end is in
        // sight and left alone.
        assert!(!sees(&pillar(), both[160], MAGE_XZ));
        assert!(repaired[160].distance(both[160]) > SIGHT_MARGIN);
        assert_eq!(repaired[0], both[0]);
        assert_eq!(repaired[both.len() - 1], both[0]);
    }

    /// A Mage close behind the pillar casts a wide shadow: a point 12yd behind
    /// the pillar is repaired only part of the way (its nearest exit lies
    /// between `REPAIR_FULL` and `REPAIR_LIMIT`), and one 19yd behind not at
    /// all — while a point just behind the pillar is repaired in full.
    #[test]
    fn a_far_exit_is_taken_part_of_the_way_or_not_at_all() {
        // The pillar at (8, 0) r2; the Mage at (11, 0), 3yd from its centre.
        let f = fixture(&[(11.0, 0.0, 0.7)]);
        let mage = Vec2::new(11.0, 0.0);
        let mut kept = None;
        let near = Vec2::new(5.0, 0.0);
        let fixed = f.repair_from(&pillar(), near, &mut kept);
        assert!(
            sees(&pillar(), fixed, mage),
            "near: repaired in full ({fixed})"
        );
        assert!(fixed.distance(near) < REPAIR_FULL);

        let mid = Vec2::new(-4.0, 0.0);
        let partial = f.repair_from(&pillar(), mid, &mut None);
        let moved = partial.distance(mid);
        let exit = shadow_exits(mage, &pillar()[0], mid, SIGHT_MARGIN)
            .into_iter()
            .map(|(_, e)| e.distance(mid))
            .fold(f32::MAX, f32::min);
        assert!(exit > REPAIR_FULL && exit < REPAIR_LIMIT, "exit {exit}yd");
        assert!(
            moved > 1.0 && moved < exit - 0.5,
            "mid: moved part of the way ({moved}yd of {exit}yd, to {partial})"
        );

        let far = Vec2::new(-11.0, 0.0);
        assert!(!sees(&pillar(), far, mage));
        let mut kept = None;
        assert_eq!(
            f.repair_from(&pillar(), far, &mut kept),
            far,
            "far: left alone"
        );
        assert_eq!(kept, None, "far: no exit kept");
    }

    /// A point back in sight forgets its side: the next repair takes the
    /// nearer exit, where a kept exit would have held it on the far one.
    #[test]
    fn a_point_in_sight_forgets_its_kept_exit() {
        let f = fixture(&[MAGE]);
        let mut kept = None;
        // Repaired to the +z side from just +z of the centre line.
        let first = f.repair_from(&pillar(), Vec2::new(0.0, 0.5), &mut kept);
        assert!(first.y > 0.0);
        // Back in open ground.
        f.repair_from(&pillar(), Vec2::new(0.0, 9.0), &mut kept);
        // Into the shadow again just -z of the centre line: the -z exit is
        // nearer, and nothing holds the point on +z.
        let again = f.repair_from(&pillar(), Vec2::new(0.0, -1.5), &mut kept);
        assert!(again.y < 0.0, "held on the side it had left: {again}");
        // The control: without the open-ground frame, the band holds +z.
        let mut held = None;
        f.repair_from(&pillar(), Vec2::new(0.0, 0.5), &mut held);
        let kept = f.repair_from(&pillar(), Vec2::new(0.0, -1.5), &mut held);
        assert!(kept.y > 0.0, "the band should hold +z: {kept}");
    }

    /// The band belongs to the exit nearest where the point was last repaired
    /// to, and only within `KEEP_RADIUS` of it: a kept spot far from every
    /// exit holds nothing.
    #[test]
    fn the_band_is_for_the_exit_the_point_was_at() {
        let f = fixture(&[MAGE]);
        let mut kept = None;
        let first = f.repair_from(&pillar(), Vec2::new(0.0, 0.5), &mut kept);
        assert_eq!(kept, Some(first));
        // Held: the +z exit is the one the point was at.
        let held = f.repair_from(&pillar(), Vec2::new(0.0, -1.5), &mut kept);
        assert!(held.y > 0.0, "held: {held}");
        // A kept spot 4yd off the +z exit (beyond `KEEP_RADIUS`) holds nothing.
        let mut stray = Some(first + Vec2::new(-4.0, 0.0));
        let free = f.repair_from(&pillar(), Vec2::new(0.0, -1.5), &mut stray);
        assert!(free.y < 0.0, "a stray kept spot held the point: {free}");
    }

    /// With a second teammate behind the pillar the point stands in two
    /// shadows and has an exit from each; drifting over them, it keeps to the
    /// exit it was at. The control: with no kept exit, the same drift jumps
    /// between exits.
    #[test]
    fn the_point_keeps_its_exit_with_two_teammates_behind_the_pillar() {
        // The Mage behind the pillar on the x axis; a healthy Warrior just
        // behind it, a yard off the axis.
        let f = fixture(&[MAGE, (10.0, 1.0, 0.9)]);
        let points: Vec<Vec2> = (0..=120)
            .map(|i| Vec2::new(0.0, 3.0 - i as f32 * 0.05))
            .collect();
        let (kept_step, kept) = largest_step(&f, &points, true);
        let (free_step, _) = largest_step(&f, &points, false);
        assert!(
            kept_step < 2.0,
            "a kept repair jumped {kept_step}yd: {kept:?}"
        );
        assert!(
            free_step > 6.0,
            "with no kept exit the drift should change exits (largest step {free_step}yd)"
        );
    }

    /// Only a shadow the point stands near the obstacle in counts: a teammate
    /// close behind the pillar (the Mage 3yd from its centre) casts one whose
    /// edges sweep fast where the point stands far behind. A point 13yd from the
    /// pillar's centre — farther than the Mage and beyond `REPAIR_FULL` of the
    /// footprint — is left alone though blind and its exit within
    /// `REPAIR_LIMIT`; one 11yd from it is repaired.
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
}
