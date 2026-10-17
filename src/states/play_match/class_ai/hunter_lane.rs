//! Where the Hunter's lane Freezing Trap lands when cover hides the lane.
//!
//! The lane trap ([`fallback_trap`](super::hunter::fallback_trap)) is thrown
//! at the midpoint toward the enemy healer, where the enemy runs. Every throw
//! needs sight of its landing (a user ruling), so on a map whose pillars stand
//! where combat converges (TwinPillars) that midpoint is often behind a
//! pillar, and the throw is held. Pulling the landing back along the line of
//! the throw, short of the pillar, recovered nothing (AS-281): the value was
//! in landing where the enemy walks.
//!
//! So this picks the landing off the enemy's own route instead. An enemy
//! pursuing its target round a pillar walks the route its movement steers —
//! [`steer_toward_goal`]'s tangent arc — and the route leaves the pillar's
//! shadow at a point the Hunter can see. The landing is the first such point
//! the enemy reaches after the trap has armed ([`seen_route_landing`]): seen,
//! in range, on the route. Who springs it is then predicted along each enemy's
//! own route ([`route_springer`]), and the throw is decided on that enemy as
//! every lane throw is.
//!
//! A no-op with no obstacles: there no landing is hidden and nothing here is
//! asked.

use bevy::prelude::*;

use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::play_match::combat_core::clamp_to_arena;
use crate::states::play_match::components::AuraType;
use crate::states::play_match::constants::{
    TRAP_ARM_DELAY, TRAP_LAUNCH_SPEED, TRAP_TRIGGER_RADIUS,
};
use crate::states::play_match::map_geometry::{
    has_line_of_sight, resolve_movement, steer_toward_goal, ObstacleVolume,
};

use super::hunter_dip::{first_inside, trap_armed_after, TRAP_SPRING_WINDOW};
use super::CombatContext;

/// Sampling step of a predicted route, in seconds — the same step the
/// straight-line springer prediction samples at.
pub const ROUTE_STEP: f32 = 0.05;

/// How closely an enemy's heading must agree with the first step of its
/// steered route (the cosine of the angle between them) for the route to be
/// taken as where it is going. A pursuer rounding a pillar faces the way it
/// last stepped, which is the route; a unit whose heading disagrees is moving
/// for some other reason (a healer's posture, a kiter's run) and is not
/// predicted to walk the route.
pub const ROUTE_HEADING_AGREEMENT: f32 = 0.866;

/// Where a mover starting at `start`, walking at `speed` toward `goal` and
/// stopping once within `stop` of it, stands at each `step` seconds from now,
/// `steps + 1` samples from now on. It walks as pursuit walks
/// (`move_to_target`): toward the tangent point of a pillar between it and its
/// goal ([`steer_toward_goal`]), straight at the goal otherwise, each step
/// resolved against the obstacles and clamped to the arena. Planar distances;
/// the mover keeps its height.
pub fn steered_track(
    obstacles: &[ObstacleVolume],
    bounds: &ArenaBounds,
    start: Vec3,
    goal: Vec3,
    stop: f32,
    speed: f32,
    step: f32,
    steps: usize,
) -> Vec<Vec3> {
    let mut track = Vec::with_capacity(steps + 1);
    let mut pos = start;
    track.push(pos);
    for _ in 0..steps {
        let to_goal = Vec3::new(goal.x - pos.x, 0.0, goal.z - pos.z);
        if to_goal.length() > stop {
            let dir = steer_toward_goal(
                obstacles,
                Vec2::new(pos.x, pos.z),
                Vec2::new(goal.x, goal.z),
                pos.y,
            )
            .map(|s| Vec3::new(s.x, 0.0, s.y))
            .unwrap_or_else(|| to_goal.normalize_or_zero());
            if dir != Vec3::ZERO {
                let proposed = pos + dir * speed * step;
                pos = clamp_to_arena(bounds, resolve_movement(obstacles, pos, proposed));
            }
        }
        track.push(pos);
    }
    track
}

/// The first point of `track` (sampled every `step` seconds from now) a trap
/// thrown from `my_pos` can be landed on to catch the walker: within `range`,
/// in the Hunter's sight, and reached by the walker no sooner than the trap
/// arms there and no later than [`TRAP_SPRING_WINDOW`] after. The earliest
/// such point is the one that leans least on the prediction: on a route round
/// a pillar it is where the route leaves the pillar's shadow, or a little
/// further on when the walker would get there before the trap had armed.
pub fn seen_route_landing(
    obstacles: &[ObstacleVolume],
    my_pos: Vec3,
    range: f32,
    track: &[Vec3],
    step: f32,
) -> Option<Vec3> {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    track.iter().enumerate().find_map(|(k, &at)| {
        let reached = k as f32 * step;
        let armed = trap_armed_after(my_pos, at);
        (reached >= armed
            && reached <= armed + TRAP_SPRING_WINDOW
            && flat(my_pos).distance(flat(at)) <= range
            && has_line_of_sight(obstacles, my_pos, at))
        .then_some(at)
    })
}

/// How far ahead any route is predicted: the longest a trap within `range`
/// takes to arm, then the spring window.
fn route_horizon(range: f32) -> f32 {
    range / TRAP_LAUNCH_SPEED + TRAP_ARM_DELAY + TRAP_SPRING_WINDOW
}

/// The route `enemy` is predicted to walk round cover, sampled every
/// [`ROUTE_STEP`] out to `horizon`: its steered pursuit of its own target
/// ([`steered_track`]), stopping at its preferred range of it. `None` — no
/// route to predict — when it is not free to walk (casting, rooted or held),
/// its target is not in view, it is already within its preferred range, the
/// straight line to its target is clear (nothing to round), or its heading
/// does not follow the route's first step ([`ROUTE_HEADING_AGREEMENT`]).
pub fn enemy_route(ctx: &CombatContext, enemy: Entity, horizon: f32) -> Option<Vec<Vec3>> {
    let info = ctx.combatants.get(&enemy).filter(|e| e.is_alive)?;
    let heading = Vec3::new(info.velocity.x, 0.0, info.velocity.z);
    let speed = heading.length() * ctx.movement_slow_multiplier(enemy);
    if speed <= 0.0 {
        return None;
    }
    let held = ctx.active_auras.get(&enemy).is_some_and(|auras| {
        auras
            .iter()
            .any(|a| a.effect_type == AuraType::Root || super::is_hard_cc(a.effect_type))
    });
    if held {
        return None;
    }
    let goal = ctx.combatants.get(&info.target?)?.position;
    let stop = match info.pet_type {
        Some(pet_type) => pet_type.preferred_range(),
        None => info.class.preferred_range(),
    };
    let start = info.position;
    if Vec3::new(goal.x - start.x, 0.0, goal.z - start.z).length() <= stop {
        return None;
    }
    let first = steer_toward_goal(
        ctx.obstacles,
        Vec2::new(start.x, start.z),
        Vec2::new(goal.x, goal.z),
        start.y,
    )?;
    if first.length_squared() <= 0.0
        || Vec2::new(heading.x, heading.z)
            .normalize_or_zero()
            .dot(first.normalize_or_zero())
            < ROUTE_HEADING_AGREEMENT
    {
        return None;
    }
    let steps = (horizon / ROUTE_STEP).ceil() as usize;
    Some(steered_track(
        ctx.obstacles,
        &ctx.bounds,
        start,
        goal,
        stop,
        speed,
        ROUTE_STEP,
        steps,
    ))
}

/// The enemy that would spring a trap landing at `landing`, and when — as
/// [`predicted_trap_springer`](super::hunter_dip::predicted_trap_springer)
/// predicts it, except that an enemy with a route round cover (`routes`) is
/// followed along that route rather than straight along its heading. The
/// first enemy inside the trigger radius in `[armed, armed + window]` springs
/// it (ties: nearest the landing, then entity order).
pub fn route_springer(
    ctx: &CombatContext,
    my_team: u8,
    my_pos: Vec3,
    landing: Vec3,
    routes: &std::collections::BTreeMap<Entity, Vec<Vec3>>,
) -> Option<(Entity, f32)> {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let armed = trap_armed_after(my_pos, landing);
    let at = flat(landing);
    let mut best: Option<(f32, f32, Entity)> = None;
    for enemy in ctx
        .combatants
        .values()
        .filter(|e| e.team != my_team && e.is_alive)
    {
        let inside = match routes.get(&enemy.entity) {
            Some(track) => first_on_track(track, at, armed, TRAP_SPRING_WINDOW),
            None => first_inside(ctx, enemy, at, armed, TRAP_SPRING_WINDOW),
        };
        if let Some(t) = inside {
            let key = (t, flat(enemy.position).distance(at), enemy.entity);
            if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                best = Some(key);
            }
        }
    }
    best.map(|(t, _, e)| (e, t))
}

/// The first moment in `[from, from + window]` at which a walker following
/// `track` (sampled every [`ROUTE_STEP`]) is inside the trigger radius of a
/// trap at `landing` (planar). Past the end of the track it stands at its last
/// sample.
fn first_on_track(track: &[Vec3], landing: Vec3, from: f32, window: f32) -> Option<f32> {
    let last = track.len().checked_sub(1)?;
    let first = (from / ROUTE_STEP).ceil() as usize;
    let end = ((from + window) / ROUTE_STEP).floor() as usize;
    (first..=end).find_map(|k| {
        let p = track[k.min(last)];
        (Vec3::new(p.x, 0.0, p.z).distance(landing) <= TRAP_TRIGGER_RADIUS)
            .then_some(k as f32 * ROUTE_STEP)
    })
}

/// A lane landing on an enemy's route round cover, for when the lane itself is
/// hidden: of every visible enemy with a route ([`enemy_route`]), the first
/// seen point on it ([`seen_route_landing`]), and of those the one sprung
/// soonest ([`route_springer`]; ties by entity order). Returns the landing
/// and the enemy predicted to spring it, or `None` when no enemy has a route
/// with a seen point on it that someone would spring.
pub fn route_lane(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    my_team: u8,
    my_pos: Vec3,
) -> Option<(Vec3, Entity)> {
    let range = abilities.get_unchecked(&AbilityType::FreezingTrap).range;
    let horizon = route_horizon(range);
    let routes: std::collections::BTreeMap<Entity, Vec<Vec3>> = ctx
        .combatants
        .values()
        .filter(|e| e.team != my_team && e.is_alive)
        .filter_map(|e| enemy_route(ctx, e.entity, horizon).map(|r| (e.entity, r)))
        .collect();
    let mut best: Option<(f32, Vec3, Entity)> = None;
    for track in routes.values() {
        let Some(landing) = seen_route_landing(ctx.obstacles, my_pos, range, track, ROUTE_STEP)
        else {
            continue;
        };
        let Some((springer, at)) = route_springer(ctx, my_team, my_pos, landing, &routes) else {
            continue;
        };
        if best.is_none_or(|(t, _, _)| at < t) {
            best = Some((at, landing, springer));
        }
    }
    best.map(|(_, landing, springer)| (landing, springer))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pillar(x: f32, z: f32, radius: f32) -> ObstacleVolume {
        ObstacleVolume::Cylinder {
            center_xz: Vec2::new(x, z),
            radius,
            base_y: 0.0,
            height: 10.0,
        }
    }

    fn bounds() -> ArenaBounds {
        ArenaBounds::default()
    }

    /// The card's geometry: the Hunter at the origin, a pillar 12yd out on +x,
    /// an enemy 24yd out walking at the Hunter round it at `speed`.
    fn walk_round_pillar(speed: f32) -> (Vec<ObstacleVolume>, Vec<Vec3>) {
        let obstacles = vec![pillar(12.0, 0.0, 2.5)];
        let track = steered_track(
            &obstacles,
            &bounds(),
            Vec3::new(24.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            2.0,
            speed,
            ROUTE_STEP,
            120,
        );
        (obstacles, track)
    }

    /// [`walk_round_pillar`] at a Warrior's base speed.
    fn route_round_pillar() -> (Vec<ObstacleVolume>, Vec<Vec3>) {
        walk_round_pillar(7.0)
    }

    #[test]
    fn a_steered_track_rounds_the_pillar_and_never_enters_it() {
        let (obstacles, track) = route_round_pillar();
        let blocked =
            |p: &Vec3| crate::states::play_match::map_geometry::position_blocked(&obstacles, *p);
        assert!(!track.iter().any(blocked), "the track enters the pillar");
        // It leaves the axis to round the pillar, and arrives at its stop.
        assert!(
            track.iter().any(|p| p.z.abs() > 2.5),
            "never rounded the pillar"
        );
        let end = *track.last().unwrap();
        assert!(
            Vec3::new(end.x, 0.0, end.z).length() <= 2.0 + 7.0 * ROUTE_STEP + 1e-3,
            "did not arrive: {end:?}"
        );
    }

    #[test]
    fn a_clear_track_is_a_straight_walk_that_stops_at_its_range() {
        let track = steered_track(
            &[],
            &bounds(),
            Vec3::new(20.0, 1.0, 0.0),
            Vec3::ZERO,
            10.0,
            5.0,
            ROUTE_STEP,
            100,
        );
        assert!(track.iter().all(|p| p.z.abs() < 1e-5));
        assert!((track[20].x - 15.0).abs() < 1e-3, "{:?}", track[20]);
        assert!(track.last().unwrap().x <= 10.0 + 1e-3);
        assert!(track.last().unwrap().x >= 10.0 - 5.0 * ROUTE_STEP);
    }

    /// The landing is on the route, in sight, in range, and reached by the
    /// walker after the trap has armed there — and it is the FIRST such
    /// point: every earlier route point is hidden, out of range, or reached
    /// before the trap would arm.
    #[test]
    fn the_landing_is_the_first_seen_point_reached_after_arming() {
        let (obstacles, track) = route_round_pillar();
        let me = Vec3::new(0.0, 1.0, 0.0);
        let landing =
            seen_route_landing(&obstacles, me, 30.0, &track, ROUTE_STEP).expect("a seen point");
        let k = track
            .iter()
            .position(|p| *p == landing)
            .expect("on the route");
        assert!(has_line_of_sight(&obstacles, me, landing));
        let reached = k as f32 * ROUTE_STEP;
        let armed = trap_armed_after(me, landing);
        assert!(reached >= armed && reached <= armed + TRAP_SPRING_WINDOW);
        for (j, p) in track[..k].iter().enumerate() {
            let early = (j as f32) * ROUTE_STEP < trap_armed_after(me, *p);
            assert!(
                early || !has_line_of_sight(&obstacles, me, *p),
                "an earlier point {p:?} was seen and reached after arming"
            );
        }
    }

    /// The pillar's shadow is what pushes the landing along: a slow walker is
    /// still behind the pillar when a trap would arm, so with the pillar's
    /// sight ignored it is caught there, hidden, and with sight asked it is
    /// caught later, where the route has left the shadow.
    #[test]
    fn the_shadow_moves_the_landing_further_along_the_route() {
        let (obstacles, track) = walk_round_pillar(3.5);
        let me = Vec3::new(0.0, 1.0, 0.0);
        let shadowed = seen_route_landing(&obstacles, me, 30.0, &track, ROUTE_STEP).unwrap();
        let open = seen_route_landing(&[], me, 30.0, &track, ROUTE_STEP).unwrap();
        let index = |p: Vec3| track.iter().position(|q| *q == p).unwrap();
        assert!(index(open) < index(shadowed), "{open:?} vs {shadowed:?}");
        assert!(!has_line_of_sight(&obstacles, me, open), "{open:?}");
        assert!(has_line_of_sight(&obstacles, me, shadowed), "{shadowed:?}");
    }

    /// Range is asked of the landing: a walker that does not come within it
    /// before the track ends has no landing, though the same walk carried on
    /// does.
    #[test]
    fn no_landing_out_of_range() {
        let (obstacles, track) = route_round_pillar();
        let me = Vec3::new(0.0, 1.0, 0.0);
        assert!(seen_route_landing(&obstacles, me, 8.0, &track, ROUTE_STEP).is_some());
        // 2.2s in, the walker is still more than 8yd out.
        let short = &track[..=44];
        assert!(short
            .iter()
            .all(|p| Vec3::new(p.x, 0.0, p.z).length() > 8.0));
        assert_eq!(
            seen_route_landing(&obstacles, me, 8.0, short, ROUTE_STEP),
            None
        );
        assert!(seen_route_landing(&obstacles, me, 30.0, short, ROUTE_STEP).is_some());
    }

    /// A walker that only comes into sight long after a trap would have armed
    /// — beyond the spring window — has no landing: a trap there would catch
    /// whoever wandered in later, a decision nobody made.
    #[test]
    fn no_landing_beyond_the_spring_window() {
        let obstacles = vec![pillar(10.0, 0.0, 2.0)];
        let me = Vec3::new(0.0, 1.0, 0.0);
        let hidden = Vec3::new(20.0, 1.0, 0.0);
        let seen = Vec3::new(10.0, 1.0, 6.0);
        assert!(!has_line_of_sight(&obstacles, me, hidden));
        assert!(has_line_of_sight(&obstacles, me, seen));
        let emerging_at = |k: usize| -> Vec<Vec3> {
            (0..=k + 20)
                .map(|j| if j < k { hidden } else { seen })
                .collect()
        };
        // Into sight at 2.5s: within the window of a trap arming there.
        assert_eq!(
            seen_route_landing(&obstacles, me, 30.0, &emerging_at(50), ROUTE_STEP),
            Some(seen)
        );
        // Into sight at 6s: the trap would have armed (~2.1s) and waited
        // past its window (3s) by then.
        assert_eq!(
            seen_route_landing(&obstacles, me, 30.0, &emerging_at(120), ROUTE_STEP),
            None
        );
    }

    /// A route the Hunter never sees has no landing.
    #[test]
    fn no_landing_without_a_seen_point() {
        let me = Vec3::new(0.0, 1.0, 0.0);
        // A wall of a pillar on the Hunter's own doorstep hides everything.
        let walled = vec![pillar(2.0, 0.0, 1.8)];
        let hidden: Vec<Vec3> = (0..120)
            .map(|k| Vec3::new(20.0 - k as f32 * 0.1, 1.0, 0.0))
            .collect();
        assert_eq!(
            seen_route_landing(&walled, me, 30.0, &hidden, ROUTE_STEP),
            None
        );
    }

    #[test]
    fn a_walker_is_inside_the_radius_from_when_it_reaches_it() {
        let track: Vec<Vec3> = (0..=100)
            .map(|k| Vec3::new(20.0 - k as f32 * 0.25, 0.0, 0.0))
            .collect();
        // At 5yd/s from x=20 it is within 5yd of x=10 from t=1.0 (k=20).
        let t = first_on_track(&track, Vec3::new(10.0, 0.0, 0.0), 0.0, 3.0).unwrap();
        assert!((t - 1.0).abs() < 1e-4, "{t}");
        // Not before the window opens, and never once it has closed.
        let t = first_on_track(&track, Vec3::new(10.0, 0.0, 0.0), 2.0, 3.0).unwrap();
        assert!((t - 2.0).abs() < 1e-4, "{t}");
        assert_eq!(
            first_on_track(&track, Vec3::new(30.0, 0.0, 0.0), 0.0, 3.0),
            None
        );
    }
}
