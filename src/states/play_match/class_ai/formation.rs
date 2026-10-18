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
//! one goal, not a choice among scored steps, and the repair moves it only as
//! far as sight needs, so everywhere sight already holds (every point on an
//! obstacle-free map) it comes back bit-identical.
//!
//! **Where the repair looks.** Not a lattice round the point: a lattice moves
//! with its centre, so the winner hops between cells as the fight drifts
//! (`team_solve::candidates_for` explains the pathology). The candidates are
//! anchored to the geometry that hides the teammate: for each obstacle between
//! the point and a teammate it cannot see, the obstacle's shadow from that
//! teammate is bounded by the two tangent rays from the teammate past the
//! obstacle's footprint disc, and the nearest point outside the shadow lies on
//! one of them. Each candidate is that projection pushed `margin` clear of the
//! ray, so it slides continuously as the teammate and the point move.
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

/// The two points just outside `obstacle`'s shadow from `viewer` nearest to
/// `spot`: `spot` projected onto each tangent ray from `viewer` past the
/// obstacle's footprint disc (never short of the tangent point), then pushed
/// `margin` outward, away from the shadow. Empty when the viewer stands at the
/// disc's centre (no bearing to take a shadow along).
pub fn shadow_exits(viewer: Vec2, obstacle: &ObstacleVolume, spot: Vec2, margin: f32) -> Vec<Vec2> {
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
    let tangent_len = dist * half.cos();
    [1.0_f32, -1.0]
        .into_iter()
        .map(|side| {
            let ray = Vec2::from_angle(side * half).rotate(along);
            let t = (spot - viewer).dot(ray).max(tangent_len);
            // Outward: rotate the ray a further quarter turn away from the centre line.
            let outward = Vec2::from_angle(side * std::f32::consts::FRAC_PI_2).rotate(ray);
            viewer + ray * t + outward * margin
        })
        .collect()
}

/// The formation point held to sight of the healer's teammates (AS-206).
///
/// `point` is the formation point as its machine computed it — already clamped
/// into the arena and projected onto standable ground. When it sees every
/// living non-pet teammate it comes back bit-identical (always, on an
/// obstacle-free map). Otherwise it moves to the nearest
/// [`shadow_exits`] candidate that sees strictly better ([`Sight`]: the anchor
/// first, then more teammates), clamped and projected the same way; with no
/// candidate better than the point, the point stands.
pub fn sighted_formation_point(ctx: &CombatContext, healer: Entity, point: Vec3) -> Vec3 {
    if ctx.obstacles.is_empty() {
        return point;
    }
    let allies = teammates(ctx, healer);
    let origin = xz(point);
    let mut best = (sight_from(ctx.obstacles, origin, &allies), 0.0_f32, origin);
    if best.0.seen == allies.len() {
        return point;
    }
    for ally in &allies {
        let ally_xz = xz(ally.position);
        if has_line_of_sight(ctx.obstacles, eye(origin), eye(ally_xz)) {
            continue;
        }
        for obstacle in ctx.obstacles {
            if has_line_of_sight(std::slice::from_ref(obstacle), eye(origin), eye(ally_xz)) {
                continue;
            }
            for exit in shadow_exits(ally_xz, obstacle, origin, SIGHT_MARGIN) {
                let clamped = clamp_to_arena(&ctx.bounds, Vec3::new(exit.x, point.y, exit.y));
                let Some(spot) = nearest_standable(ctx.obstacles, xz(clamped), point.y) else {
                    continue;
                };
                let sight = sight_from(ctx.obstacles, spot, &allies);
                let dist = spot.distance(origin);
                // The point itself is the incumbent at distance 0, so only a
                // candidate that sees strictly better can displace it; among
                // those, the nearest wins.
                if sight > best.0 || (sight == best.0 && dist < best.1) {
                    best = (sight, dist, spot);
                }
            }
        }
    }
    Vec3::new(best.2.x, point.y, best.2.y)
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

        fn repair(&self, obstacles: &[ObstacleVolume], point: Vec2) -> Vec2 {
            xz(sighted_formation_point(
                &self.ctx(obstacles),
                self.healer,
                Vec3::new(point.x, 1.0, point.y),
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
}
