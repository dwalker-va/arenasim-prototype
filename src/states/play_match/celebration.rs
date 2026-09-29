//! Victory choreography: what the winners do during the celebration.
//!
//! Once a match is decided the sim is frozen (`combat_sim_runs` in
//! `src/states/mod.rs`), so nothing moves the winners unless this module does.
//! Moving them is safe: the outcome, the Results rows and the saved report
//! were all captured at the deciding tick. Graphical-only — headless never
//! registers these systems — and it never touches `GameRng`.
//!
//! Choreographies are a closed set, [`CelebrationChoreography`], picked at the
//! decision (`check_match_end`) and carried on `VictoryCelebration`. Each is
//! two pure pieces the systems below drive: a PLAN (where every winner is
//! going, from where they stood at the decision) and a STEP (how a winner
//! travels toward it each tick). A new pattern is a new variant with its own
//! `plan`, and a `step` arm if walking in a straight line is not enough.
//!
//! The winner bounce itself stays in `update_victory_celebration`, which
//! bounces each winner's `VisualBody` on the frame clock wherever the
//! choreography has walked it.
//!
//! Runs on the FIXED tick, so a choreography is a function of the tick count
//! and plays out identically at any display rate; render interpolation draws
//! it smoothly between ticks like any other movement.

use super::combat_core::clamp_to_arena;
use super::components::*;
use super::map_config::ActiveMapGeometry;
use super::map_geometry::{position_blocked, resolve_movement, steer_toward_goal};
use super::utils::is_incapacitated;
use bevy::prelude::*;

/// How the winners celebrate. One pattern today; more are expected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CelebrationChoreography {
    /// The winners run together from wherever the match left them and bounce
    /// side by side — teammates meeting up after the fight. A lone winner
    /// bounces where it stands.
    #[default]
    ConvergeAndBounce,
}

/// Distance each converging winner stops from the meeting point, so a pair
/// ends up side by side rather than stacked on one spot.
pub const CONVERGE_RING_RADIUS: f32 = 1.25;

/// A winning pet stops this close to its owner (the distance a targetless pet
/// heels at during a match).
pub const PET_HEEL_DISTANCE: f32 = 3.0;

/// Where one winner is going during the celebration, and what it faces once
/// it gets there.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CelebrationMarch {
    pub goal: Vec3,
    pub face: Vec3,
}

impl CelebrationChoreography {
    /// Every mover's destination, from where each stood at the decision.
    ///
    /// `winners` are the living winners free to move (in slot order);
    /// `volumes` is the map's obstacle set, so no plan sends anyone into a
    /// pillar.
    pub fn plan(
        self,
        winners: &[(Entity, Vec3)],
        volumes: &[super::map_geometry::ObstacleVolume],
    ) -> Vec<(Entity, CelebrationMarch)> {
        match self {
            Self::ConvergeAndBounce => {
                if winners.len() < 2 {
                    return Vec::new();
                }
                // Meet at the team's centroid — unless that is inside a
                // pillar, when the others come to the first winner instead.
                let centroid = winners.iter().map(|(_, p)| *p).sum::<Vec3>() / winners.len() as f32;
                let meet = if position_blocked(volumes, centroid) {
                    winners[0].1
                } else {
                    centroid
                };
                winners
                    .iter()
                    .map(|&(entity, from)| {
                        // Each stops on the side it came from, so no two
                        // winners cross paths or end on one spot.
                        let away = Vec3::new(from.x - meet.x, 0.0, from.z - meet.z);
                        let offset = away.normalize_or_zero() * CONVERGE_RING_RADIUS;
                        let goal = Vec3::new(meet.x + offset.x, from.y, meet.z + offset.z);
                        (entity, CelebrationMarch { goal, face: meet })
                    })
                    .collect()
            }
        }
    }
}

/// Plan the choreography once, the first tick the winners are marked.
///
/// `Celebrating` is inserted by `check_match_end` on exactly the living
/// winners (pets excluded), so it is the roster. Incapacitated winners — a
/// feared or polymorphed unit at the decision — hold their spot and bounce
/// there; the others meet without them.
pub fn plan_celebration(
    mut commands: Commands,
    celebration: Option<Res<VictoryCelebration>>,
    map_geometry: Res<ActiveMapGeometry>,
    winners: Query<
        (Entity, &Combatant, &Transform, Option<&ActiveAuras>),
        (With<Celebrating>, Without<CelebrationMarch>),
    >,
    planned: Query<(), (With<Celebrating>, With<CelebrationMarch>)>,
) {
    let Some(celebration) = celebration else {
        return;
    };
    if winners.is_empty() || !planned.is_empty() {
        return;
    }
    let mut movers: Vec<(u8, Entity, Vec3)> = winners
        .iter()
        .filter(|(_, _, _, auras)| !is_incapacitated(*auras))
        .map(|(entity, c, t, _)| (c.slot, entity, t.translation))
        .collect();
    movers.sort_by_key(|(slot, ..)| *slot);
    let movers: Vec<(Entity, Vec3)> = movers.into_iter().map(|(_, e, p)| (e, p)).collect();
    let plan = celebration
        .choreography
        .plan(&movers, &map_geometry.volumes);
    // Mark every winner planned — including those the plan leaves in place —
    // so this runs once. A winner that stays gets a march to where it stands.
    for (entity, _, transform, _) in winners.iter() {
        let march = plan
            .iter()
            .find(|(e, _)| *e == entity)
            .map(|(_, m)| *m)
            .unwrap_or(CelebrationMarch {
                goal: transform.translation,
                face: transform.translation + transform.forward().as_vec3(),
            });
        commands.entity(entity).insert(march);
    }
}

/// Walk every winner one tick toward its planned spot, and every winning pet
/// toward its owner.
pub fn step_celebration(
    time: Res<Time>,
    celebration: Option<Res<VictoryCelebration>>,
    map_geometry: Res<ActiveMapGeometry>,
    mut marchers: Query<(Entity, &Combatant, &mut Transform, &CelebrationMarch), Without<Pet>>,
    mut pets: Query<(&Combatant, &mut Transform, &Pet), Without<CelebrationMarch>>,
) {
    let Some(celebration) = celebration else {
        return;
    };
    let dt = time.delta_secs();
    let geometry = &*map_geometry;
    match celebration.choreography {
        CelebrationChoreography::ConvergeAndBounce => {
            for (_, combatant, mut transform, march) in marchers.iter_mut() {
                let step = combatant.base_movement_speed * dt;
                if !walk_toward(&mut transform, march.goal, step, 0.0, geometry) {
                    face(&mut transform, march.face);
                }
            }
        }
    }
    // A winning pet heels to its owner wherever the owner goes — the same
    // follow it does in a match when it has no target.
    for (combatant, mut transform, pet) in pets.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        let Ok((_, _, owner, _)) = marchers.get(pet.owner) else {
            continue;
        };
        let owner_pos = owner.translation;
        let step = combatant.base_movement_speed * dt;
        walk_toward(&mut transform, owner_pos, step, PET_HEEL_DISTANCE, geometry);
    }
}

/// Move one step toward `goal`, stopping `stop_at` short of it. Returns
/// whether it moved.
fn walk_toward(
    transform: &mut Transform,
    goal: Vec3,
    step: f32,
    stop_at: f32,
    geometry: &ActiveMapGeometry,
) -> bool {
    let from = transform.translation;
    let to_goal = Vec2::new(goal.x - from.x, goal.z - from.z);
    let remaining = to_goal.length() - stop_at;
    if remaining <= 1e-3 {
        return false;
    }
    // Round any pillar in the way, as a pursuer does.
    let direction = steer_toward_goal(
        &geometry.volumes,
        Vec2::new(from.x, from.z),
        Vec2::new(goal.x, goal.z),
        from.y,
    )
    .unwrap_or_else(|| to_goal.normalize_or_zero());
    if direction == Vec2::ZERO {
        return false;
    }
    let travel = step.min(remaining);
    let proposed = from + Vec3::new(direction.x, 0.0, direction.y) * travel;
    transform.translation = clamp_to_arena(
        &geometry.bounds,
        resolve_movement(&geometry.volumes, from, proposed),
    );
    transform.rotation = Quat::from_rotation_y(direction.x.atan2(direction.y));
    true
}

fn face(transform: &mut Transform, point: Vec3) {
    let toward = Vec2::new(
        point.x - transform.translation.x,
        point.z - transform.translation.z,
    );
    if toward.length_squared() > 1e-6 {
        transform.rotation = Quat::from_rotation_y(toward.x.atan2(toward.y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(i: u32) -> Entity {
        Entity::from_raw(i)
    }

    #[test]
    fn a_lone_winner_stays_put() {
        let plan = CelebrationChoreography::ConvergeAndBounce.plan(&[(e(1), Vec3::X * 20.0)], &[]);
        assert!(plan.is_empty());
    }

    #[test]
    fn two_winners_meet_side_by_side_on_their_own_sides() {
        let a = Vec3::new(-30.0, 1.0, 0.0);
        let b = Vec3::new(30.0, 1.0, 10.0);
        let plan = CelebrationChoreography::ConvergeAndBounce.plan(&[(e(1), a), (e(2), b)], &[]);
        let goal = |i: u32| plan.iter().find(|(x, _)| *x == e(i)).unwrap().1.goal;
        let meet = (a + b) / 2.0;
        // Each ends CONVERGE_RING_RADIUS from the meeting point, on the side
        // it came from, at its own height.
        for (i, from) in [(1, a), (2, b)] {
            let g = goal(i);
            assert!(
                (Vec2::new(g.x - meet.x, g.z - meet.z).length() - CONVERGE_RING_RADIUS).abs()
                    < 1e-4
            );
            assert!((g - meet).dot(from - meet) > 0.0);
            assert_eq!(g.y, from.y);
        }
        let apart = goal(1).distance(goal(2));
        assert!((apart - 2.0 * CONVERGE_RING_RADIUS).abs() < 1e-3, "{apart}");
    }
}
