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
use super::map_geometry::{position_blocked, resolve_movement, steer_toward_goal, ObstacleVolume};
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

/// How far apart neighbouring winners stand once they have met: close enough
/// to read as one group, clear of each other's bodies (a mover is
/// `MOVER_RADIUS` wide, so bodies touch at `2 * MOVER_RADIUS`).
pub const CONVERGE_SPACING: f32 = 2.5;

/// A winning pet stops this close to its owner (the distance a targetless pet
/// heels at during a match).
pub const PET_HEEL_DISTANCE: f32 = 3.0;

/// How far from its ring slot a spot may be moved to find clear ground when
/// the slot is inside a pillar, and the rings of candidates tried on the way.
const SPOT_SEARCH_RADIUS: f32 = 8.0;
const SPOT_SEARCH_STEP: f32 = 0.5;
const SPOT_SEARCH_DIRECTIONS: usize = 16;

/// Where one winner is going during the celebration, and what it faces once
/// it gets there.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CelebrationMarch {
    pub goal: Vec3,
    pub face: Vec3,
    /// Set once the winner has arrived — or can get no closer, pressed
    /// against a pillar or the arena edge. From then on it only faces `face`.
    pub settled: bool,
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
        volumes: &[ObstacleVolume],
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
                let from: Vec<Vec2> = winners.iter().map(|(_, p)| Vec2::new(p.x, p.z)).collect();
                let spots = ring_spots(Vec2::new(meet.x, meet.z), &from, volumes, meet.y);
                winners
                    .iter()
                    .zip(spots)
                    .map(|(&(entity, at), spot)| {
                        let goal = Vec3::new(spot.x, at.y, spot.y);
                        (
                            entity,
                            CelebrationMarch {
                                goal,
                                face: meet,
                                settled: false,
                            },
                        )
                    })
                    .collect()
            }
        }
    }
}

/// One distinct spot per winner, evenly spaced on a ring around `meet`
/// `CONVERGE_SPACING` apart, each winner given the slot on the side it
/// approaches from. A slot inside a pillar moves to the nearest clear ground
/// that keeps its spacing from the others.
///
/// Slots keep the winners' angular ORDER around the meeting point, and the
/// ring is rotated to the best fit of their approach angles, so nobody has to
/// cross the group to reach its place.
pub fn ring_spots(meet: Vec2, from: &[Vec2], volumes: &[ObstacleVolume], y: f32) -> Vec<Vec2> {
    let n = from.len();
    if n == 0 {
        return Vec::new();
    }
    let angle = |p: Vec2| {
        let d = p - meet;
        if d.length_squared() < 1e-8 {
            0.0
        } else {
            d.y.atan2(d.x)
        }
    };
    // Winners in angular order around the meeting point (ties by index).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        angle(from[a])
            .partial_cmp(&angle(from[b]))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let step = std::f32::consts::TAU / n as f32;
    // A ring whose neighbouring slots are CONVERGE_SPACING apart.
    let radius = if n == 1 {
        0.0
    } else {
        CONVERGE_SPACING / (2.0 * (step / 2.0).sin())
    };
    // Rotate the ring to the circular mean of each winner's angle less its
    // slot's offset: the rotation that best matches slots to approaches.
    let pull: Vec2 = order
        .iter()
        .enumerate()
        .map(|(k, &i)| Vec2::from_angle(angle(from[i]) - k as f32 * step))
        .sum();
    let base = if pull.length_squared() < 1e-8 {
        0.0
    } else {
        pull.y.atan2(pull.x)
    };
    let mut spots = vec![Vec2::ZERO; n];
    let mut placed: Vec<Vec2> = Vec::with_capacity(n);
    for (k, &i) in order.iter().enumerate() {
        let slot = meet + Vec2::from_angle(base + k as f32 * step) * radius;
        let spot = clear_spot(slot, &placed, volumes, y);
        placed.push(spot);
        spots[i] = spot;
    }
    spots
}

/// `slot` if a winner can stand there clear of every pillar and of the spots
/// already `placed`; otherwise the nearest such point, searched outward in
/// rings. Falls back to `slot` if nothing within `SPOT_SEARCH_RADIUS` is clear
/// — the winner then walks as close as it can and settles there.
fn clear_spot(slot: Vec2, placed: &[Vec2], volumes: &[ObstacleVolume], y: f32) -> Vec2 {
    let fits = |p: Vec2| {
        !position_blocked(volumes, Vec3::new(p.x, y, p.y))
            && placed
                .iter()
                .all(|q| q.distance(p) >= CONVERGE_SPACING - 1e-3)
    };
    if fits(slot) {
        return slot;
    }
    let rings = (SPOT_SEARCH_RADIUS / SPOT_SEARCH_STEP) as usize;
    for ring in 1..=rings {
        let r = ring as f32 * SPOT_SEARCH_STEP;
        for d in 0..SPOT_SEARCH_DIRECTIONS {
            let a = d as f32 * std::f32::consts::TAU / SPOT_SEARCH_DIRECTIONS as f32;
            let p = slot + Vec2::from_angle(a) * r;
            if fits(p) {
                return p;
            }
        }
    }
    slot
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
                settled: true,
            });
        commands.entity(entity).insert(march);
    }
}

/// Walk every winner one tick toward its planned spot, and every winning pet
/// toward its owner.
#[allow(clippy::type_complexity)]
pub fn step_celebration(
    time: Res<Time>,
    celebration: Option<Res<VictoryCelebration>>,
    map_geometry: Res<ActiveMapGeometry>,
    mut marchers: Query<(Entity, &Combatant, &mut Transform, &mut CelebrationMarch), Without<Pet>>,
    mut pets: Query<
        (&Combatant, &mut Transform, &Pet, Option<&ActiveAuras>),
        Without<CelebrationMarch>,
    >,
) {
    let Some(celebration) = celebration else {
        return;
    };
    let dt = time.delta_secs();
    let geometry = &*map_geometry;
    match celebration.choreography {
        CelebrationChoreography::ConvergeAndBounce => {
            for (_, combatant, mut transform, mut march) in marchers.iter_mut() {
                let step = combatant.base_movement_speed * dt;
                advance_march(&mut transform, &mut march, step, geometry);
            }
        }
    }
    // A winning pet heels to its owner wherever the owner goes — the same
    // follow it does in a match when it has no target — unless the pet itself
    // is feared or polymorphed, when it stays put like any incapacitated
    // winner.
    for (combatant, mut transform, pet, auras) in pets.iter_mut() {
        if !combatant.is_alive() || is_incapacitated(auras) {
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

/// One tick of a winner's march: a step toward its spot until it arrives or
/// can get no closer, then face the group for the rest of the celebration.
fn advance_march(
    transform: &mut Transform,
    march: &mut CelebrationMarch,
    step: f32,
    geometry: &ActiveMapGeometry,
) {
    if !march.settled {
        match walk_toward(transform, march.goal, step, 0.0, geometry) {
            Walk::Moved => return,
            Walk::Arrived | Walk::Stuck => march.settled = true,
        }
    }
    face(transform, march.face);
}

/// What one step of [`walk_toward`] did.
#[derive(Debug, PartialEq, Eq)]
enum Walk {
    /// Made progress toward the goal.
    Moved,
    /// Already there (within `stop_at`).
    Arrived,
    /// Could not make meaningful progress: pressed against a pillar or the
    /// arena edge. Its facing is left alone.
    Stuck,
}

/// A step that covers less than this fraction of its intended travel counts
/// as stuck.
const STUCK_FRACTION: f32 = 0.25;

/// Move one step toward `goal`, stopping `stop_at` short of it.
fn walk_toward(
    transform: &mut Transform,
    goal: Vec3,
    step: f32,
    stop_at: f32,
    geometry: &ActiveMapGeometry,
) -> Walk {
    let from = transform.translation;
    let to_goal = Vec2::new(goal.x - from.x, goal.z - from.z);
    let remaining = to_goal.length() - stop_at;
    if remaining <= 1e-3 {
        return Walk::Arrived;
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
        return Walk::Stuck;
    }
    let travel = step.min(remaining);
    let proposed = from + Vec3::new(direction.x, 0.0, direction.y) * travel;
    let landed = clamp_to_arena(
        &geometry.bounds,
        resolve_movement(&geometry.volumes, from, proposed),
    );
    let before = to_goal.length();
    let after = Vec2::new(goal.x - landed.x, goal.z - landed.z).length();
    transform.translation = landed;
    if before - after < STUCK_FRACTION * travel {
        return Walk::Stuck;
    }
    transform.rotation = Quat::from_rotation_y(direction.x.atan2(direction.y));
    Walk::Moved
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
    use super::super::arena_bounds::ArenaBounds;
    use super::super::map_geometry::MOVER_RADIUS;
    use super::*;

    fn e(i: u32) -> Entity {
        Entity::from_raw(i)
    }

    fn pillar(x: f32, z: f32, radius: f32) -> ObstacleVolume {
        ObstacleVolume::Cylinder {
            center_xz: Vec2::new(x, z),
            radius,
            base_y: 0.0,
            height: 10.0,
        }
    }

    fn plan(winners: &[Vec3], volumes: &[ObstacleVolume]) -> Vec<CelebrationMarch> {
        let winners: Vec<(Entity, Vec3)> = winners
            .iter()
            .enumerate()
            .map(|(i, p)| (e(i as u32), *p))
            .collect();
        let plan = CelebrationChoreography::ConvergeAndBounce.plan(&winners, volumes);
        winners
            .iter()
            .map(|(entity, _)| plan.iter().find(|(x, _)| x == entity).unwrap().1)
            .collect()
    }

    fn flat(v: Vec3) -> Vec2 {
        Vec2::new(v.x, v.z)
    }

    fn assert_spaced(marches: &[CelebrationMarch]) {
        for (i, a) in marches.iter().enumerate() {
            for b in &marches[i + 1..] {
                let d = flat(a.goal).distance(flat(b.goal));
                assert!(d >= CONVERGE_SPACING - 1e-3, "two spots {d:.3}yd apart");
                assert!(d >= 2.0 * MOVER_RADIUS);
            }
        }
    }

    #[test]
    fn a_lone_winner_stays_put() {
        let winners = [(e(1), Vec3::X * 20.0)];
        assert!(CelebrationChoreography::ConvergeAndBounce
            .plan(&winners, &[])
            .is_empty());
    }

    #[test]
    fn two_winners_meet_side_by_side_on_their_own_sides() {
        let a = Vec3::new(-30.0, 1.0, 0.0);
        let b = Vec3::new(30.0, 1.0, 10.0);
        let marches = plan(&[a, b], &[]);
        let meet = (a + b) / 2.0;
        for (march, from) in marches.iter().zip([a, b]) {
            assert!((flat(march.goal) - flat(meet)).dot(flat(from) - flat(meet)) > 0.0);
            assert_eq!(march.goal.y, from.y);
            assert_eq!(march.face, meet);
        }
        let apart = flat(marches[0].goal).distance(flat(marches[1].goal));
        assert!((apart - CONVERGE_SPACING).abs() < 1e-3, "{apart}");
    }

    /// Three winners closing from nearly the same direction each get their own
    /// spot. Each spot pointing back along its winner's approach would put
    /// two of them on top of each other.
    #[test]
    fn winners_from_one_side_get_distinct_spots() {
        let winners = [
            Vec3::new(-20.0, 1.0, 0.0),
            Vec3::new(-20.0, 1.0, 0.5),
            Vec3::new(30.0, 1.0, 0.2),
        ];
        let marches = plan(&winners, &[]);
        assert_spaced(&marches);
        // ...and the one from the far side still takes the far side.
        let meet = winners.iter().copied().sum::<Vec3>() / 3.0;
        assert!(marches[2].goal.x > meet.x);
    }

    /// A pillar at the origin: the meeting point (3.25, 0) is clear, but the
    /// slot on the near winner's side lands inside the pillar. It moves to
    /// clear ground and keeps its spacing.
    #[test]
    fn a_spot_inside_a_pillar_moves_to_clear_ground() {
        let volumes = [pillar(0.0, 0.0, 2.0)];
        let winners = [Vec3::new(-3.5, 1.0, 0.0), Vec3::new(10.0, 1.0, 0.0)];
        let meet = (winners[0] + winners[1]) / 2.0;
        assert!(!position_blocked(&volumes, meet));
        let slot = meet + Vec3::new(-CONVERGE_SPACING / 2.0, 0.0, 0.0);
        assert!(
            position_blocked(&volumes, slot),
            "the unmoved slot is in the pillar"
        );
        let marches = plan(&winners, &volumes);
        for march in &marches {
            assert!(!position_blocked(&volumes, march.goal), "{:?}", march.goal);
        }
        assert_spaced(&marches);
    }

    /// When the team's centroid is inside a pillar, the others come to the
    /// first winner instead.
    #[test]
    fn a_centroid_inside_a_pillar_meets_at_the_first_winner() {
        let volumes = [pillar(0.0, 0.0, 2.0)];
        let winners = [Vec3::new(-6.0, 1.0, 0.0), Vec3::new(6.0, 1.0, 0.0)];
        let marches = plan(&winners, &volumes);
        for march in &marches {
            assert_eq!(march.face, winners[0]);
            assert!(!position_blocked(&volumes, march.goal));
        }
        assert_spaced(&marches);
    }

    /// A winner whose spot it cannot reach — here, deliberately planted inside
    /// a pillar — walks as close as it can, settles, and faces the group
    /// instead of pressing against the pillar for the rest of the
    /// celebration.
    #[test]
    fn a_winner_that_cannot_reach_its_spot_settles_facing_the_group() {
        let geometry = ActiveMapGeometry {
            bounds: ArenaBounds::Octagon {
                half_x: 60.0,
                half_z: 60.0,
                corner_sum: 100.0,
            },
            volumes: vec![pillar(0.0, 0.0, 2.0)],
            cover_anchors: Vec::new(),
        };
        let face_at = Vec3::new(0.0, 1.0, 10.0);
        let mut march = CelebrationMarch {
            goal: Vec3::new(0.5, 1.0, 0.0),
            face: face_at,
            settled: false,
        };
        let mut transform = Transform::from_xyz(-8.0, 1.0, 0.0);
        let step = 7.0 / 60.0;
        for _ in 0..300 {
            advance_march(&mut transform, &mut march, step, &geometry);
            if march.settled {
                break;
            }
        }
        assert!(march.settled, "still walking at {}", transform.translation);
        let before = transform.translation;
        advance_march(&mut transform, &mut march, step, &geometry);
        assert_eq!(transform.translation, before, "a settled winner stays put");
        let facing = transform.forward().as_vec3();
        let toward = (face_at - transform.translation).normalize();
        // Bevy's forward is -Z of the rotation; the sim's facing convention
        // (`from_rotation_y(x.atan2(z))`) points +Z at the target.
        assert!(
            (-facing).dot(toward) > 0.99,
            "facing {facing:?}, group at {toward:?}"
        );
    }
}
