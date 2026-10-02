//! A stealthed Rogue plays around a lit enemy Flare (AS-185).
//!
//! A Flare is a visible light on the ground, so a stealthed Rogue can see it.
//! Whether it finds the Rogue is decided by geometry alone — where the Hunter
//! put it and which way the Rogue walks — with no roll (`traps::flare_system`
//! reveals every stealthed enemy inside the radius, nothing outside it). This
//! module is the walking half:
//!
//! - **Skirt.** When a way to its target stays out of the light, the Rogue
//!   walks it: around the light's edge ([`FLARE_SKIRT_MARGIN`] clear of it),
//!   and — when the target itself stands near the light — to the side of the
//!   target away from it.
//! - **Wait.** When no way in stays dark (the target stands so deep in the
//!   light that every spot in reach of it is lit), the Rogue holds
//!   [`FLARE_WAIT_BUFFER`] yards outside the light, on its line to the target,
//!   for the light to burn out or the target to walk out of it.
//! - **Commit.** It goes in anyway when waiting costs more than being found.
//!   That is exactly two things, both read while it holds:
//!   1. a teammate (not a pet) falls below half health
//!      ([`LOW_HP_THRESHOLD`]) — its partner is losing the fight without it;
//!   2. the light over its approach is relit — a Flare that closes the way in
//!      appears with more burn left than any it was waiting on. The Hunter is
//!      keeping the approach lit, and a wait cannot outlast a light that is
//!      renewed faster than it burns out (Flare's cooldown is half its
//!      duration).
//!   Once committed the Rogue pursues its target plainly until its stealth
//!   ends or no Flare is lit.
//!
//! No teleports and no new movement machinery: the pre-pass issues ordinary
//! `MovementGoal::Point` directives, which `move_to_target` walks with the
//! same pillar steering as every other point walk, and every point it walks to
//! lies inside the arena (a light against a wall is skirted on its open side).
//! With no enemy Flare lit it does nothing at all, so every match without a
//! Hunter facing a Rogue is unchanged.
//!
//! **Where it plays.** It acts only while the Rogue is still stealthed beside
//! a lit Flare. `hunter::flare_plan` lights the Flare for when a Rogue running
//! straight from its gate reaches it, so on an open map the light lands on the
//! Rogue and finds it the next frame. Where the Rogue's walk is slower than
//! that straight run — rounding TwinPillars' pillars — it arrives after the
//! light and this rule decides whether it is found.
//!
//! Traced: one `movement_decision` event per announced change
//! (`FlareSkirt` / `FlareWait` / `FlareCommit`), and a `[FLARE]` combat-log line
//! beside each.

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::LOW_HP_THRESHOLD;
use crate::states::play_match::decision_trace::{
    DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::utils::combat_log_id_for;

use super::healer_postures::start_movement_event_with_target;
use super::CombatContext;

/// Yards the Rogue keeps between itself and a Flare's light while walking
/// round it — room for the step it takes each frame and for a target that
/// moves under it.
pub const FLARE_SKIRT_MARGIN: f32 = 1.5;

/// Yards outside the light the spot beside a target must be. Smaller than the
/// skirt margin: this is where the Rogue stands to open, so it only has to be
/// dark, not roomy.
pub const FLARE_EDGE_MARGIN: f32 = 0.5;

/// Yards outside the light a waiting Rogue holds — out of it with room to
/// spare, near enough to move the moment it goes out.
pub const FLARE_WAIT_BUFFER: f32 = 4.0;

/// A relit Flare is recognised by its burn jumping by more than this (seconds)
/// between frames — far above a frame's decay, far below a fresh Flare's 30s.
const RELIGHT_EPS: f32 = 0.5;

/// How far past a tangent point a skirt waypoint is placed, so the walk never
/// arrives at it and stalls.
const WAYPOINT_LEAD: f32 = 1.0;

/// How far round the skirt ring (yards of arc) a Rogue on or inside it aims
/// at each frame.
const SKIRT_ARC_AHEAD: f32 = 2.0;

/// How long a Flare directive lives. The pre-pass re-issues it every frame the
/// Rogue decides; this only bounds a stale one.
const DIRECTIVE_TTL: f32 = 0.5;

/// A lit enemy Flare as the Rogue sees it, on the ground plane (`Vec2.y` is
/// world Z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LitFlare {
    pub center: Vec2,
    pub radius: f32,
    /// Seconds of burn left.
    pub remaining: f32,
}

/// What [`plan_flare_path`] decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlarePath {
    /// No lit Flare touches the way in: plain pursuit.
    Clear,
    /// Walk to `waypoint`: round a Flare's light, or to the dark side of a
    /// target standing near one.
    Skirt { waypoint: Vec2 },
    /// No way in stays dark: hold at `at`. `covering_remaining` is the longest
    /// burn left among the Flares that close the way.
    Wait { at: Vec2, covering_remaining: f32 },
}

/// Where a stealthed Rogue at `from` walks to reach `target` without entering
/// any of `flares` — `reach` is how close to the target it must stand.
///
/// Pure geometry, deterministic (flares walked in slice order, a fixed side on
/// an exact tie):
///
/// 1. **Where it must stand.** The target itself, unless a light reaches
///    within `reach` + [`FLARE_SKIRT_MARGIN`] of it: then the spot `reach` from
///    the target directly away from that light's centre, which must lie
///    [`FLARE_EDGE_MARGIN`] outside every light and inside the arena. None
///    does → [`FlarePath::Wait`].
/// 2. **How it gets there.** Straight, unless the line dips into a light's
///    skirt ring (never judged deeper than either end of the line already is):
///    then toward the tangent of the first such ring, on the side that makes
///    more progress — unless the way round that side leaves the arena (a
///    light against a wall), when it takes the other side. Both sides walled
///    off → [`FlarePath::Wait`].
///
/// Every point it returns lies inside `bounds`.
pub fn plan_flare_path(
    from: Vec2,
    target: Vec2,
    reach: f32,
    flares: &[LitFlare],
    bounds: &ArenaBounds,
) -> FlarePath {
    let inside = |p: Vec2| bounds.contains(Vec3::new(p.x, 0.0, p.y));
    let covering: Vec<&LitFlare> = flares
        .iter()
        .filter(|f| target.distance(f.center) < f.radius + FLARE_SKIRT_MARGIN + reach)
        .collect();

    let goal = if covering.is_empty() {
        None
    } else {
        let dark = |p: Vec2| {
            inside(p)
                && flares
                    .iter()
                    .all(|f| p.distance(f.center) >= f.radius + FLARE_EDGE_MARGIN)
        };
        let spot = covering.iter().find_map(|f| {
            let away = (target - f.center)
                .try_normalize()
                .or_else(|| (from - target).try_normalize())
                .unwrap_or(Vec2::X);
            let spot = target + away * reach;
            dark(spot).then_some(spot)
        });
        match spot {
            Some(spot) => Some(spot),
            None => return wait(from, target, &covering, bounds),
        }
    };

    let dest = goal.unwrap_or(target);
    match first_blocking(from, dest, flares) {
        Some(flare) => match skirt_waypoint(from, dest, flare, &inside) {
            Some(waypoint) => FlarePath::Skirt { waypoint },
            None => wait(from, target, &[flare], bounds),
        },
        None => match goal {
            Some(spot) => FlarePath::Skirt { waypoint: spot },
            None => FlarePath::Clear,
        },
    }
}

/// The skirt ring a line from `from` to `to` must stay outside for `f`: the
/// light plus [`FLARE_SKIRT_MARGIN`], but never deeper than either end already
/// is, so a walk that starts or ends near the light is judged by whether it
/// goes deeper, not by where it already stands.
fn guard_radius(from: Vec2, to: Vec2, f: &LitFlare) -> f32 {
    (f.radius + FLARE_SKIRT_MARGIN)
        .min(from.distance(f.center))
        .min(to.distance(f.center))
}

/// Entry parameter of the segment `from → to` into the circle, if it crosses
/// the circle's interior (a tangent graze is clear).
fn segment_entry(from: Vec2, to: Vec2, center: Vec2, radius: f32) -> Option<f32> {
    let d = to - from;
    let f = from - center;
    let a = d.dot(d);
    if a <= 1e-9 {
        return None;
    }
    let b = 2.0 * f.dot(d);
    let c = f.dot(f) - radius * radius;
    let disc = b * b - 4.0 * a * c;
    if disc <= 1e-6 {
        return None;
    }
    let sq = disc.sqrt();
    let t0 = (-b - sq) / (2.0 * a);
    let t1 = (-b + sq) / (2.0 * a);
    // Both ends lie on or outside the guard ring by construction, so a real
    // crossing has its chord strictly inside [0, 1].
    if t1 <= 1e-4 || t0 >= 1.0 - 1e-4 {
        return None;
    }
    Some(t0.max(0.0))
}

/// The first light whose skirt ring the line `from → to` dips into.
fn first_blocking(from: Vec2, to: Vec2, flares: &[LitFlare]) -> Option<&LitFlare> {
    let mut best: Option<(f32, &LitFlare)> = None;
    for f in flares {
        let g = guard_radius(from, to, f);
        if let Some(t) = segment_entry(from, to, f.center, g - 1e-3) {
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, f));
            }
        }
    }
    best.map(|(_, f)| f)
}

/// The next point of the walk round `f`'s skirt ring, on the side that makes
/// more progress toward `dest` (the first-computed side on an exact tie), or
/// the other side when the way round the better one leaves the arena — `None`
/// when both do. A side is open when its next point and the ring's widest
/// point on that side (square to the line to `dest`) are both inside.
///
/// - **Outside the ring:** a point along the tangent to it, past the tangent
///   point.
/// - **On or inside it** (a light lit beside the Rogue): a point on the ring a
///   short arc ahead. A tangent to the circle through the Rogue's own position
///   would not do: every step along it leans a hair inward, and re-aimed each
///   frame it spirals into the light.
fn skirt_waypoint(
    from: Vec2,
    dest: Vec2,
    f: &LitFlare,
    inside: &impl Fn(Vec2) -> bool,
) -> Option<Vec2> {
    let ring = f.radius + FLARE_SKIRT_MARGIN;
    let to_center = f.center - from;
    let dist = to_center.length();
    let goal_dir = (dest - from).normalize_or_zero();
    // (waypoint, the point whose arena membership decides the side)
    let sides: [(Vec2, Vec2); 2] = if dist > ring {
        let dn = to_center / dist;
        let alpha = (ring / dist).clamp(-1.0, 1.0).asin();
        let (sin_a, cos_a) = alpha.sin_cos();
        let tangent_len = (dist * dist - ring * ring).max(0.0).sqrt();
        [
            Vec2::new(dn.x * cos_a - dn.y * sin_a, dn.x * sin_a + dn.y * cos_a),
            Vec2::new(dn.x * cos_a + dn.y * sin_a, -dn.x * sin_a + dn.y * cos_a),
        ]
        .map(|dir| (from + dir * (tangent_len + WAYPOINT_LEAD), from + dir * tangent_len))
    } else {
        let out = (-to_center)
            .try_normalize()
            .unwrap_or(Vec2::new(-goal_dir.y, goal_dir.x));
        let step = SKIRT_ARC_AHEAD / ring;
        [step, -step].map(|a| {
            let point = f.center + Vec2::from_angle(a).rotate(out) * ring;
            (point, point)
        })
    };
    let progress = |w: Vec2| (w - from).normalize_or_zero().dot(goal_dir);
    let [first, second] = sides;
    let (better, worse) = if progress(first.0) + 1e-4 >= progress(second.0) {
        (first, second)
    } else {
        (second, first)
    };
    // The ring's widest point on a side: square to the line to `dest`, on the
    // side the walk leans to.
    let square = Vec2::new(-goal_dir.y, goal_dir.x);
    let open = |(waypoint, probe): (Vec2, Vec2)| {
        let widest = f.center + square * square.dot(waypoint - from).signum() * ring;
        inside(probe) && inside(widest)
    };
    [better, worse]
        .into_iter()
        .find(|side| open(*side))
        .map(|(waypoint, _)| waypoint)
}

/// Where to hold when no way in is dark: [`FLARE_WAIT_BUFFER`] outside the
/// covering light nearest the Rogue — where its line to the target crosses
/// that ring, or straight out from the centre when it is already inside it —
/// brought back inside the arena when that lands beyond a wall.
fn wait(from: Vec2, target: Vec2, covering: &[&LitFlare], bounds: &ArenaBounds) -> FlarePath {
    let covering_remaining = covering
        .iter()
        .map(|f| f.remaining)
        .fold(0.0_f32, f32::max);
    let nearest = covering
        .iter()
        .min_by(|a, b| from.distance(a.center).total_cmp(&from.distance(b.center)))
        .expect("wait is only planned for a covered target");
    let hold = nearest.radius + FLARE_WAIT_BUFFER;
    let at = if from.distance(nearest.center) > hold {
        segment_entry(from, target, nearest.center, hold)
            .map(|t| from + (target - from) * t)
            .unwrap_or(from)
    } else {
        nearest.center
            + (from - nearest.center)
                .try_normalize()
                .unwrap_or((from - target).normalize_or(Vec2::X))
                * hold
    };
    let at = bounds.clamp(Vec3::new(at.x, 0.0, at.y));
    let at = Vec2::new(at.x, at.z);
    FlarePath::Wait {
        at,
        covering_remaining,
    }
}

/// Why a waiting Rogue goes in, or `None` to keep waiting. See the module
/// docs for the rule.
pub fn commit_reason(
    teammate_low: bool,
    was_waiting: bool,
    last_covering_remaining: f32,
    covering_remaining: f32,
) -> Option<&'static str> {
    if teammate_low {
        Some("a teammate is below half health")
    } else if was_waiting && covering_remaining > last_covering_remaining + RELIGHT_EPS {
        Some("the Flare was relit over its approach")
    } else {
        None
    }
}

/// The Rogue's Flare pre-pass: runs before its ability decision each frame
/// the gates are open. `flares` is every lit Flare with its owner's team;
/// only an enemy's can find this Rogue. `state` is the Rogue's
/// [`RogueFlareState`] if it has one; the return is the state it should hold
/// now — the caller stores it only when it changed, and inserts it only when
/// it is not the default ([`RogueFlareState::needs_insert`]), so a Rogue that
/// never meets a Flare never carries one and an idle frame touches nothing.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_rogue_flare(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    flares: &[(u8, LitFlare)],
    state: Option<RogueFlareState>,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> RogueFlareState {
    let mut state = state.unwrap_or_default();
    // Only our own Skirt/Wait directive is ours to clear.
    let release = |commands: &mut Commands, state: &RogueFlareState| {
        if matches!(state.mode, FlareApproach::Skirt | FlareApproach::Wait) {
            commands.entity(entity).remove::<MovementDirective>();
        }
    };

    let lit: Vec<LitFlare> = flares
        .iter()
        .filter(|(team, _)| *team != combatant.team)
        .map(|(_, f)| *f)
        .collect();
    let target = combatant
        .target
        .and_then(|t| ctx.combatants.get(&t).map(|info| (t, info.position)));

    let (Some((target_entity, target_pos)), true, false) =
        (target, combatant.stealthed, lit.is_empty())
    else {
        // Nothing to play around: hand movement back to plain pursuit.
        release(commands, &state);
        return RogueFlareState::default();
    };

    if state.mode == FlareApproach::Committed {
        return state;
    }

    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let plan = plan_flare_path(
        flat(my_pos),
        flat(target_pos),
        CharacterClass::Rogue.preferred_range(),
        &lit,
        &ctx.bounds,
    );

    let directive = |commands: &mut Commands, at: Vec2| {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(Vec3::new(at.x, my_pos.y, at.y)),
            expires: now + DIRECTIVE_TTL,
            committed_until: now,
        });
    };
    let mut say = |state: &mut RogueFlareState,
                        mode: FlareApproach,
                        trigger: MovementTrigger,
                        goal_kind: MovementGoalKind,
                        what: String| {
        if state.announced != mode {
            state.announced = mode;
            announce(
                combat_log,
                decision_trace,
                ctx,
                combatant,
                target_entity,
                my_pos,
                trigger,
                goal_kind,
                what,
            );
        }
    };

    match plan {
        FlarePath::Clear => {
            release(commands, &state);
            state.mode = FlareApproach::Clear;
        }
        FlarePath::Skirt { waypoint } => {
            directive(commands, waypoint);
            state.mode = FlareApproach::Skirt;
            say(
                &mut state,
                FlareApproach::Skirt,
                MovementTrigger::FlareSkirt,
                MovementGoalKind::Point,
                format!("skirts the Flare toward ({:.0}, {:.0})", waypoint.x, waypoint.y),
            );
        }
        FlarePath::Wait {
            at,
            covering_remaining,
        } => {
            // Teammates only: `alive_allies` leaves out pets, and a dying pet
            // is not worth the opener.
            let teammate_low = ctx
                .alive_allies()
                .into_iter()
                .any(|a| a.entity != entity && a.health_pct() < LOW_HP_THRESHOLD);
            let was_waiting = state.mode == FlareApproach::Wait;
            if let Some(reason) = commit_reason(
                teammate_low,
                was_waiting,
                state.covering_remaining,
                covering_remaining,
            ) {
                release(commands, &state);
                state.mode = FlareApproach::Committed;
                say(
                    &mut state,
                    FlareApproach::Committed,
                    MovementTrigger::FlareCommit,
                    MovementGoalKind::Entity,
                    format!("goes into the Flare: {reason}"),
                );
            } else {
                directive(commands, at);
                state.mode = FlareApproach::Wait;
                state.covering_remaining = covering_remaining;
                say(
                    &mut state,
                    FlareApproach::Wait,
                    MovementTrigger::FlareWait,
                    MovementGoalKind::Point,
                    format!("holds outside the Flare at ({:.0}, {:.0})", at.x, at.y),
                );
            }
        }
    }
    state
}

/// One `[FLARE]` combat-log line and one `movement_decision` event, with the
/// Rogue's target in the event's target view.
#[allow(clippy::too_many_arguments)]
fn announce(
    combat_log: &mut CombatLog,
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    combatant: &Combatant,
    target: Entity,
    my_pos: Vec3,
    trigger: MovementTrigger,
    goal_kind: MovementGoalKind,
    what: String,
) {
    combat_log.log(
        CombatLogEventType::MatchEvent,
        format!("[FLARE] {} {}", combat_log_id_for(combatant, None), what),
    );
    if let Some(mut builder) = start_movement_event_with_target(decision_trace, ctx, target, my_pos)
    {
        builder.direction_change(TracePosture::Free, trigger, goal_kind);
        builder.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REACH: f32 = 2.0;

    fn flare(x: f32, z: f32) -> LitFlare {
        LitFlare {
            center: Vec2::new(x, z),
            radius: 10.0,
            remaining: 30.0,
        }
    }

    /// Walk the plan the way `move_to_target` walks a point directive (or
    /// straight at the target on `Clear`), and return every position visited
    /// and the final plan.
    fn walk(from: Vec2, target: Vec2, flares: &[LitFlare]) -> (Vec<Vec2>, FlarePath) {
        let mut pos = from;
        let mut path = vec![pos];
        let step = 0.1_f32;
        let mut plan = FlarePath::Clear;
        for _ in 0..2000 {
            plan = plan_flare_path(pos, target, REACH, flares, &ArenaBounds::default());
            let dest = match plan {
                FlarePath::Clear => {
                    if pos.distance(target) <= REACH {
                        break;
                    }
                    target
                }
                FlarePath::Skirt { waypoint } => waypoint,
                FlarePath::Wait { at, .. } => at,
            };
            let to = dest - pos;
            if to.length() <= 0.25 {
                break;
            }
            pos += to.normalize() * step.min(to.length());
            path.push(pos);
        }
        (path, plan)
    }

    fn min_clearance(path: &[Vec2], f: &LitFlare) -> f32 {
        path.iter()
            .map(|p| p.distance(f.center) - f.radius)
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn no_flare_is_plain_pursuit() {
        let plan = plan_flare_path(Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0), REACH, &[], &ArenaBounds::default());
        assert_eq!(plan, FlarePath::Clear);
    }

    #[test]
    fn a_flare_off_the_line_changes_nothing() {
        let f = [flare(0.0, 20.0)];
        let plan = plan_flare_path(Vec2::new(-30.0, 0.0), Vec2::new(30.0, 0.0), REACH, &f, &ArenaBounds::default());
        assert_eq!(plan, FlarePath::Clear);
    }

    /// A light across the middle of the walk, target well clear of it: the
    /// Rogue walks round and reaches the target without ever standing in it.
    #[test]
    fn a_flare_across_the_line_is_walked_round() {
        let f = [flare(0.0, 1.0)];
        let target = Vec2::new(30.0, 0.0);
        let (path, _) = walk(Vec2::new(-30.0, 0.0), target, &f);
        assert!(path.last().unwrap().distance(target) <= REACH + 0.3);
        assert!(min_clearance(&path, &f[0]) > 0.0);
        // It did go round: some point is well off the straight line.
        assert!(path.iter().any(|p| p.y.abs() > 8.0));
    }

    /// The target stands just inside the light's near edge: the spot beside it
    /// away from the centre is dark, so the Rogue reaches it unlit.
    #[test]
    fn a_target_at_the_edge_is_reached_from_its_dark_side() {
        let f = [flare(0.0, 0.0)];
        let target = Vec2::new(9.5, 0.0);
        let (path, plan) = walk(Vec2::new(-30.0, 3.0), target, &f);
        assert!(!matches!(plan, FlarePath::Wait { .. }));
        let end = *path.last().unwrap();
        assert!(end.distance(target) <= REACH + 0.3, "ended {end:?}");
        assert!(min_clearance(&path, &f[0]) > 0.0);
    }

    /// The Hunter's own placement: its ally 5yd inside the light. Every spot in
    /// reach is lit, so the Rogue holds outside it on its line in.
    #[test]
    fn a_target_deep_in_the_light_is_waited_out() {
        let f = [flare(0.0, 0.0)];
        let target = Vec2::new(5.0, 0.0);
        let (path, plan) = walk(Vec2::new(-34.0, 0.0), target, &f);
        let FlarePath::Wait { at, covering_remaining } = plan else {
            panic!("expected a wait, got {plan:?}");
        };
        assert_eq!(covering_remaining, 30.0);
        // 4yd outside the 10yd light, on its own side of it.
        assert!((at - Vec2::new(-14.0, 0.0)).length() < 1e-3, "{at:?}");
        assert!(min_clearance(&path, &f[0]) >= 3.7);
    }

    /// A Rogue already inside the wait ring backs straight out to it, never
    /// deeper.
    #[test]
    fn a_rogue_inside_the_wait_ring_backs_out() {
        let f = [flare(0.0, 0.0)];
        let plan = plan_flare_path(Vec2::new(-11.0, 0.0), Vec2::new(5.0, 0.0), REACH, &f, &ArenaBounds::default());
        let FlarePath::Wait { at, .. } = plan else {
            panic!("expected a wait, got {plan:?}");
        };
        assert!((at - Vec2::new(-14.0, 0.0)).length() < 1e-3);
    }

    /// Two lights, the spot beside the target dark of one but lit by the
    /// other: no way in, wait.
    #[test]
    fn a_spot_lit_by_a_second_flare_is_no_way_in() {
        let f = [flare(0.0, 0.0), flare(19.0, 0.0)];
        let plan = plan_flare_path(Vec2::new(-30.0, 0.0), Vec2::new(9.5, 0.0), REACH, &f, &ArenaBounds::default());
        assert!(matches!(plan, FlarePath::Wait { .. }), "{plan:?}");
    }

    /// Straight behind the centre the side is a fixed choice, and the walk
    /// does not flip-flop off it.
    #[test]
    fn a_dead_centre_approach_commits_to_one_side() {
        let f = [flare(0.0, 0.0)];
        let target = Vec2::new(30.0, 0.0);
        let (path, _) = walk(Vec2::new(-30.0, 0.0), target, &f);
        assert!(path.last().unwrap().distance(target) <= REACH + 0.3);
        let off: Vec<f32> = path.iter().map(|p| p.y).filter(|y| y.abs() > 0.5).collect();
        assert!(off.iter().all(|y| y.signum() == off[0].signum()));
    }

    #[test]
    fn commit_rule() {
        assert_eq!(commit_reason(false, true, 20.0, 19.98), None);
        assert_eq!(commit_reason(false, false, 0.0, 30.0), None);
        assert_eq!(
            commit_reason(false, true, 14.0, 30.0),
            Some("the Flare was relit over its approach")
        );
        assert_eq!(
            commit_reason(true, false, 0.0, 30.0),
            Some("a teammate is below half health")
        );
    }

    /// A light against the arena's +z wall (the default octagon's `half_z` is
    /// 21.5; this ring reaches 23.5): the walk goes round the open side even
    /// though the walled side is the shorter way.
    #[test]
    fn a_light_against_a_wall_is_skirted_on_the_open_side() {
        let f = [flare(0.0, 12.0)];
        let plan = plan_flare_path(
            Vec2::new(-30.0, 14.0),
            Vec2::new(30.0, 14.0),
            REACH,
            &f,
            &ArenaBounds::default(),
        );
        let FlarePath::Skirt { waypoint } = plan else {
            panic!("expected a skirt, got {plan:?}");
        };
        assert!(waypoint.y < 12.0, "went round the wall side: {waypoint:?}");
        let (path, _) = walk(Vec2::new(-30.0, 14.0), Vec2::new(30.0, 14.0), &f);
        assert!(min_clearance(&path, &f[0]) > 0.0);
        assert!(path
            .iter()
            .all(|p| ArenaBounds::default().contains(Vec3::new(p.x, 0.0, p.y))));
    }

    /// A light spanning the corridor between both walls: no way round, wait.
    #[test]
    fn a_light_walled_in_on_both_sides_is_waited_out() {
        let bounds = ArenaBounds::Octagon {
            half_x: 36.5,
            half_z: 9.0,
            corner_sum: 48.88,
        };
        let plan = plan_flare_path(
            Vec2::new(-30.0, 0.0),
            Vec2::new(30.0, 0.0),
            REACH,
            &[flare(0.0, 0.0)],
            &bounds,
        );
        let FlarePath::Wait { at, .. } = plan else {
            panic!("expected a wait, got {plan:?}");
        };
        assert!((at - Vec2::new(-14.0, 0.0)).length() < 1e-3, "{at:?}");
    }

    // ---- The call site: `evaluate_rogue_flare` through a CombatContext ----

    use crate::states::play_match::ai_profile::AiProfile;
    use crate::states::play_match::class_ai::CombatantInfo;
    use bevy::ecs::world::CommandQueue;
    use std::collections::BTreeMap;

    const ROGUE: Entity = Entity::from_raw(1);
    const HUNTER: Entity = Entity::from_raw(2);
    const ALLY: Entity = Entity::from_raw(3);

    /// One frame of the Rogue's pre-pass, stored the way the call site stores
    /// it.
    struct Frame {
        /// The Rogue's state after the frame (`None`: it carries none).
        state: Option<RogueFlareState>,
        /// The Rogue's directive after the frame.
        directive: Option<MovementDirective>,
        /// Movement triggers traced this frame.
        triggers: Vec<String>,
    }

    struct Scene {
        rogue_pos: Vec3,
        hunter_pos: Vec3,
        stealthed: bool,
        /// Health fraction of the Rogue's ally, and whether it is a pet.
        ally: Option<(f32, bool)>,
        flares: Vec<(u8, LitFlare)>,
        state: Option<RogueFlareState>,
        /// A directive someone else issued, standing on the Rogue.
        foreign_directive: bool,
    }

    impl Default for Scene {
        fn default() -> Self {
            Scene {
                rogue_pos: Vec3::new(-20.0, 0.0, 0.0),
                hunter_pos: Vec3::new(5.0, 0.0, 0.0),
                stealthed: true,
                ally: None,
                // Team 1's Flare, the Hunter 5yd inside it: every way in is lit.
                flares: vec![(1, flare(0.0, 0.0))],
                state: None,
                foreign_directive: false,
            }
        }
    }

    const FOREIGN: MovementDirective = MovementDirective {
        goal: MovementGoal::Point(Vec3::new(99.0, 0.0, 99.0)),
        expires: 50.0,
        committed_until: 0.0,
    };

    fn run(scene: Scene) -> Frame {
        let info = |entity, team, class, position, health: f32, is_pet| CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: health * 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet,
            casting_ability: None,
            pet_type: None,
            pet: None,
        };
        let mut roster: BTreeMap<Entity, CombatantInfo> = BTreeMap::new();
        roster.insert(
            ROGUE,
            CombatantInfo {
                stealthed: scene.stealthed,
                ..info(ROGUE, 2, CharacterClass::Rogue, scene.rogue_pos, 1.0, false)
            },
        );
        roster.insert(
            HUNTER,
            info(HUNTER, 1, CharacterClass::Hunter, scene.hunter_pos, 1.0, false),
        );
        if let Some((health, is_pet)) = scene.ally {
            let class = if is_pet {
                CharacterClass::Warlock
            } else {
                CharacterClass::Priest
            };
            roster.insert(
                ALLY,
                info(ALLY, 2, class, Vec3::new(-25.0, 0.0, 5.0), health, is_pet),
            );
        }
        let (auras, dr, cds) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
        let ctx = CombatContext::new(
            ROGUE,
            2,
            &roster,
            &auras,
            &dr,
            &cds,
            &[],
            ArenaBounds::default(),
            AiProfile::Legacy,
        );
        let mut rogue = Combatant::new(2, 0, CharacterClass::Rogue);
        rogue.stealthed = scene.stealthed;
        rogue.target = Some(HUNTER);

        let mut world = World::new();
        for _ in 0..4 {
            world.spawn_empty();
        }
        if scene.foreign_directive {
            world.entity_mut(ROGUE).insert(FOREIGN);
        }
        if let Some(state) = scene.state {
            world.entity_mut(ROGUE).insert(state);
        }
        let mut log = CombatLog::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let next = {
            let mut commands = Commands::new(&mut queue, &world);
            evaluate_rogue_flare(
                &mut commands,
                &mut log,
                ROGUE,
                &rogue,
                scene.rogue_pos,
                &ctx,
                &scene.flares,
                scene.state,
                10.0,
                &mut trace,
            )
        };
        queue.apply(&mut world);
        // The call site's store.
        match world.get_mut::<RogueFlareState>(ROGUE) {
            Some(mut state) => {
                state.set_if_neq(next);
            }
            None if RogueFlareState::needs_insert(None, &next) => {
                world.entity_mut(ROGUE).insert(next);
            }
            None => {}
        }
        let triggers = trace
            .pending_events
            .iter()
            .filter_map(|e| {
                let v = serde_json::to_value(e).unwrap();
                v["trigger"].as_str().map(str::to_string)
            })
            .collect();
        Frame {
            state: world.get::<RogueFlareState>(ROGUE).copied(),
            directive: world.get::<MovementDirective>(ROGUE).copied(),
            triggers,
        }
    }

    fn point(d: Option<MovementDirective>) -> Vec2 {
        match d.map(|d| d.goal) {
            Some(MovementGoal::Point(p)) => Vec2::new(p.x, p.z),
            other => panic!("expected a point directive, got {other:?}"),
        }
    }

    /// The light lies across the line, its centre to one side of it: the
    /// Rogue goes round the OTHER side, the short way — mirrored, so a fixed
    /// side fails one of the two.
    #[test]
    fn the_skirt_takes_the_shorter_side_through_the_call_site() {
        for side in [1.0_f32, -1.0] {
            let frame = run(Scene {
                rogue_pos: Vec3::new(-30.0, 0.0, 0.0),
                hunter_pos: Vec3::new(30.0, 0.0, -6.0 * side),
                flares: vec![(1, flare(0.0, side))],
                ..Default::default()
            });
            assert_eq!(frame.triggers, ["FlareSkirt"]);
            assert_eq!(frame.state.unwrap().mode, FlareApproach::Skirt);
            let at = point(frame.directive);
            assert!(at.y * side < -5.0, "side {side}: went the long way to {at:?}");
        }
    }

    /// Every way in is lit: hold 4yd outside the 10yd light on the Rogue's
    /// line in.
    #[test]
    fn a_covered_target_holds_four_yards_out_through_the_call_site() {
        let frame = run(Scene::default());
        assert_eq!(frame.triggers, ["FlareWait"]);
        assert_eq!(frame.state.unwrap().mode, FlareApproach::Wait);
        let at = point(frame.directive);
        assert!((at - Vec2::new(-14.0, 0.0)).length() < 1e-3, "{at:?}");
    }

    /// The half-health line, exercised where it is read: 49% goes in, 51%
    /// holds.
    #[test]
    fn a_teammate_under_half_health_sends_the_rogue_in() {
        let low = run(Scene {
            ally: Some((0.49, false)),
            ..Default::default()
        });
        assert_eq!(low.triggers, ["FlareCommit"]);
        assert_eq!(low.state.unwrap().mode, FlareApproach::Committed);
        assert!(low.directive.is_none(), "goes in by plain pursuit");

        let fine = run(Scene {
            ally: Some((0.51, false)),
            ..Default::default()
        });
        assert_eq!(fine.triggers, ["FlareWait"]);
        assert_eq!(fine.state.unwrap().mode, FlareApproach::Wait);
    }

    /// A dying pet is not a teammate losing the fight.
    #[test]
    fn a_dying_pet_does_not_send_the_rogue_in() {
        let frame = run(Scene {
            ally: Some((0.10, true)),
            ..Default::default()
        });
        assert_eq!(frame.triggers, ["FlareWait"]);
        assert_eq!(frame.state.unwrap().mode, FlareApproach::Wait);
    }

    /// A Flare relit over the approach while the Rogue holds sends it in; the
    /// same light burning down keeps it holding.
    #[test]
    fn a_relit_flare_sends_a_waiting_rogue_in() {
        let waiting = RogueFlareState {
            mode: FlareApproach::Wait,
            announced: FlareApproach::Wait,
            covering_remaining: 14.0,
        };
        let frame = run(Scene {
            state: Some(waiting),
            ..Default::default()
        });
        assert_eq!(frame.triggers, ["FlareCommit"]);
        assert_eq!(frame.state.unwrap().mode, FlareApproach::Committed);

        let frame = run(Scene {
            state: Some(waiting),
            flares: vec![(
                1,
                LitFlare {
                    remaining: 13.98,
                    ..flare(0.0, 0.0)
                },
            )],
            ..Default::default()
        });
        assert!(frame.triggers.is_empty(), "announced once already");
        assert_eq!(frame.state.unwrap().mode, FlareApproach::Wait);
        assert!((point(frame.directive) - Vec2::new(-14.0, 0.0)).length() < 1e-3);
    }

    /// Committed holds while the Rogue stays stealthed and the light burns —
    /// no directive, nothing traced — and clears when its stealth ends.
    #[test]
    fn committed_latches_until_stealth_ends() {
        let committed = RogueFlareState {
            mode: FlareApproach::Committed,
            announced: FlareApproach::Committed,
            covering_remaining: 30.0,
        };
        let held = run(Scene {
            state: Some(committed),
            ..Default::default()
        });
        assert_eq!(held.state, Some(committed));
        assert!(held.directive.is_none());
        assert!(held.triggers.is_empty());

        let revealed = run(Scene {
            state: Some(committed),
            stealthed: false,
            foreign_directive: true,
            ..Default::default()
        });
        assert_eq!(revealed.state, Some(RogueFlareState::default()));
        assert_eq!(revealed.directive.map(|d| d.goal), Some(FOREIGN.goal), "not ours to clear");
    }

    /// A frame with nothing to play around (no Flare, or only its own team's)
    /// inserts no state and leaves a directive someone else issued alone —
    /// with no state yet, and with an idle one.
    #[test]
    fn an_idle_frame_touches_nothing() {
        for state in [None, Some(RogueFlareState::default())] {
            for flares in [vec![], vec![(2, flare(0.0, 0.0))]] {
                let frame = run(Scene {
                    flares,
                    state,
                    foreign_directive: true,
                    ..Default::default()
                });
                assert_eq!(frame.state, state, "state touched");
                assert_eq!(frame.directive.map(|d| d.goal), Some(FOREIGN.goal), "directive touched");
                assert!(frame.triggers.is_empty());
            }
        }
    }

    /// Leaving a skirt hands back the directive it issued.
    #[test]
    fn leaving_a_skirt_clears_its_directive() {
        let skirting = RogueFlareState {
            mode: FlareApproach::Skirt,
            announced: FlareApproach::Skirt,
            covering_remaining: 0.0,
        };
        let frame = run(Scene {
            state: Some(skirting),
            stealthed: false,
            foreign_directive: true,
            ..Default::default()
        });
        assert!(frame.directive.is_none());
        assert_eq!(frame.state, Some(RogueFlareState::default()));
    }

    /// The light is lit beside the Rogue, its edge 0.3yd away and the target
    /// beyond it: the walk round never drifts into the light.
    #[test]
    fn a_light_lit_beside_the_rogue_is_skirted_without_drifting_in() {
        let f = [flare(0.0, 0.0)];
        let target = Vec2::new(-20.0, -3.0);
        let (path, _) = walk(Vec2::new(10.3, 0.0), target, &f);
        assert!(path.last().unwrap().distance(target) <= REACH + 0.3, "{:?}", path.last());
        assert!(min_clearance(&path, &f[0]) > 0.0, "{}", min_clearance(&path, &f[0]));
    }
}
