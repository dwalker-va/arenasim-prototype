//! Render interpolation: every unit and missile the sim moves is DRAWN between
//! its last two sim ticks, so motion and turning are continuous at any display
//! rate.
//!
//! The sim steps at 60Hz in `FixedUpdate`; the display renders at whatever the
//! monitor delivers. Drawn at its raw `Transform`, a walking unit stood still on
//! frames with no tick and jumped a whole tick's travel on frames with one — at
//! 120Hz, every other frame — while the camera and the HUD each followed it
//! their own way; and a turning unit held its facing for a frame and then
//! snapped, its weapons with it. This is Bevy's standard fixed-timestep
//! pattern: the drawn pose is `lerp` (translation) and `slerp` (rotation)
//! between the previous tick and the latest one, by the overstep fraction.
//!
//! **The sim's `Transform` stays authoritative.** The interpolated pose is
//! written into `Transform` only for the render half of the frame — from the
//! end of the fixed loop through `Update` and `PostUpdate` — and the tick's own
//! translation and rotation are restored before the next fixed loop runs, bit
//! for bit. Nothing in `FixedUpdate` can observe them. Everything drawn in
//! between — the model and everything parented to it, the camera's follow
//! target, effects that follow a unit's `Transform`, and the HUD (placed from
//! post-propagation `GlobalTransform`s) — agrees on one pose. Graphical-only:
//! headless never registers these systems.
//!
//! **Discontinuities do not smear.** A newly spawned entity starts with both
//! ends at its spawn pose. A tick that moves an entity farther than any mover
//! can travel in one tick (`MAX_TICK_TRAVEL`) is a teleport and is drawn there
//! directly, facing included. A tick that turns an entity farther than any
//! continuous turn can (`MAX_TICK_TURN`) is a facing snap and is drawn at the
//! new facing directly, rather than swung through the in-between angles. And a
//! render-frame system that moves or turns an entity itself (outside the sim)
//! keeps its change: the restore applies it to the sim's own value, not to the
//! blended one, and restarts the segment from there.
//!
//! Registered for `PlayMatch` only: the Animation Sandbox stages and dashes its
//! units from render-frame systems, and is drawn at the raw sim poses.

use crate::states::play_match::components::*;
use bevy::prelude::*;

/// Farther than any continuous mover travels in one 60Hz tick, so a longer
/// tick is a teleport. The fastest are missiles at 50 yd/s (0.83 yd per tick),
/// Disengage at 30 yd/s and Charge at 4x run speed (both about 0.5).
const MAX_TICK_TRAVEL: f32 = 3.0;

/// Farther than any continuous turn goes in one 60Hz tick, so a larger turn is
/// a facing snap (radians; 45 degrees).
///
/// The sim faces a unit along its travel or at its target, so a continuous
/// turn is bounded by how fast that bearing can sweep. The fastest is a
/// Charge — 4x a 5 yd/s run — ending at melee range (2.5 yd) on a target
/// strafing at the fastest run speed (6 yd/s): (20 + 6) / 2.5 = 10.4 rad/s,
/// about 10 degrees a tick. Rounding a pillar is slower. Measured per tick
/// across four matches (1v1 and 2v2, BasicArena, TwinPillars and
/// PillaredArena), the continuous tail tops out near 25 degrees. The sim's
/// facing SNAPS — fear or Polymorph starting, a posture machine committing a
/// new compass direction, a kiter reversing — are mostly 45 to 180 degrees,
/// and those are drawn at the new facing directly. The rest are single
/// 22.5-degree compass steps, drawn as a turn within one tick. Below the
/// threshold, slerp always takes the shorter arc, so no turn it draws is ever
/// the long way round.
const MAX_TICK_TURN: f32 = std::f32::consts::FRAC_PI_4;

/// Whatever the sim moves on the fixed tick: combatants (pets included),
/// spell/shot projectiles and launched traps.
type SimMoved = Or<(
    With<Combatant>,
    With<Projectile>,
    With<TrapLaunchProjectile>,
)>;

/// `FixedFirst`: this tick's starting pose is the segment's earlier end.
pub fn begin_render_interpolation_tick(
    mut query: Query<(&Transform, &mut RenderInterpolation), SimMoved>,
) {
    for (transform, mut interpolation) in query.iter_mut() {
        interpolation.previous = transform.translation;
        interpolation.previous_rotation = transform.rotation;
    }
}

/// `FixedLast`: the tick's result is the segment's later end. Entities the sim
/// spawned are enrolled here, with both ends at the spawn pose.
pub fn end_render_interpolation_tick(
    mut commands: Commands,
    mut query: Query<(Entity, &Transform, Option<&mut RenderInterpolation>), SimMoved>,
) {
    for (entity, transform, interpolation) in query.iter_mut() {
        let current = transform.translation;
        let current_rotation = transform.rotation;
        match interpolation {
            Some(mut interpolation) => {
                interpolation.current = current;
                interpolation.current_rotation = current_rotation;
                if interpolation.previous.distance(current) > MAX_TICK_TRAVEL {
                    interpolation.previous = current;
                    interpolation.previous_rotation = current_rotation;
                }
                if interpolation
                    .previous_rotation
                    .angle_between(current_rotation)
                    > MAX_TICK_TURN
                {
                    interpolation.previous_rotation = current_rotation;
                }
            }
            None => {
                commands.entity(entity).insert(RenderInterpolation {
                    previous: current,
                    current,
                    previous_rotation: current_rotation,
                    current_rotation,
                    drawn: None,
                });
            }
        }
    }
}

/// After the fixed loop: draw every enrolled entity at its interpolated pose
/// for the rest of the frame.
pub fn apply_render_interpolation(
    fixed_time: Res<Time<Fixed>>,
    mut query: Query<(&mut Transform, &mut RenderInterpolation)>,
) {
    let alpha = fixed_time.overstep_fraction().clamp(0.0, 1.0);
    for (mut transform, mut interpolation) in query.iter_mut() {
        let translation = interpolation.previous.lerp(interpolation.current, alpha);
        // A unit that did not turn this tick is drawn at exactly its facing.
        let rotation = if interpolation.previous_rotation == interpolation.current_rotation {
            interpolation.current_rotation
        } else {
            interpolation
                .previous_rotation
                .slerp(interpolation.current_rotation, alpha)
        };
        transform.translation = translation;
        transform.rotation = rotation;
        interpolation.drawn = Some((translation, rotation));
    }
}

/// Before the fixed loop: hand the sim back its own translation and rotation.
pub fn restore_sim_pose(mut query: Query<(&mut Transform, &mut RenderInterpolation)>) {
    for (mut transform, mut interpolation) in query.iter_mut() {
        let Some((drawn, drawn_rotation)) = interpolation.drawn.take() else {
            continue;
        };
        // Something other than this module may have moved or turned the
        // entity during the render half of the frame. Its change is kept,
        // applied to the sim's own value — never the blended pose it was
        // applied on top of — and that half of the segment restarts from the
        // result.
        if transform.translation == drawn {
            transform.translation = interpolation.current;
        } else {
            let kept = interpolation.current + (transform.translation - drawn);
            transform.translation = kept;
            interpolation.previous = kept;
            interpolation.current = kept;
        }
        if transform.rotation == drawn_rotation {
            transform.rotation = interpolation.current_rotation;
        } else {
            let turn = transform.rotation * drawn_rotation.inverse();
            let kept = (turn * interpolation.current_rotation).normalize();
            transform.rotation = kept;
            interpolation.previous_rotation = kept;
            interpolation.current_rotation = kept;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn restore(
        transform: Transform,
        drawn: (Vec3, Quat),
        current: (Vec3, Quat),
    ) -> (Transform, RenderInterpolation) {
        let mut world = World::new();
        let entity = world
            .spawn((
                transform,
                RenderInterpolation {
                    previous: Vec3::ZERO,
                    current: current.0,
                    previous_rotation: Quat::IDENTITY,
                    current_rotation: current.1,
                    drawn: Some(drawn),
                },
            ))
            .id();
        world.run_system_once(restore_sim_pose).unwrap();
        (
            *world.get::<Transform>(entity).unwrap(),
            *world.get::<RenderInterpolation>(entity).unwrap(),
        )
    }

    fn pose(translation: Vec3, rotation: Quat) -> Transform {
        Transform::from_translation(translation).with_rotation(rotation)
    }

    #[test]
    fn restore_hands_the_sim_back_its_own_pose() {
        let current = (Vec3::new(1.0, 0.75, -2.0), Quat::from_rotation_y(0.7));
        let drawn = (Vec3::new(0.5, 0.75, -1.0), Quat::from_rotation_y(0.4));
        let (transform, interpolation) = restore(pose(drawn.0, drawn.1), drawn, current);
        assert_eq!(transform.translation, current.0);
        assert_eq!(transform.rotation, current.1);
        assert_eq!(interpolation.drawn, None);
    }

    #[test]
    fn a_render_frame_move_is_kept_on_the_sim_value_not_the_blended_one() {
        // A render-frame system lifted the drawn unit by 2 yards. The sim gets
        // its own x/z back with the lift on top — never the blended x/z — and
        // its own facing, untouched.
        let current = (Vec3::new(1.0, 0.75, -2.0), Quat::from_rotation_y(0.7));
        let drawn = (Vec3::new(0.5, 0.75, -1.0), Quat::from_rotation_y(0.4));
        let (transform, interpolation) =
            restore(pose(drawn.0 + Vec3::Y * 2.0, drawn.1), drawn, current);
        assert_eq!(transform.translation, current.0 + Vec3::Y * 2.0);
        assert_eq!(interpolation.previous, transform.translation);
        assert_eq!(interpolation.current, transform.translation);
        assert_eq!(transform.rotation, current.1);
    }

    #[test]
    fn a_render_frame_turn_is_kept_on_the_sim_facing_not_the_blended_one() {
        // A render-frame system turned the drawn unit a further quarter turn.
        // The sim gets its own facing back with that quarter turn on top.
        let current = (Vec3::new(1.0, 0.75, -2.0), Quat::from_rotation_y(0.7));
        let drawn = (Vec3::new(0.5, 0.75, -1.0), Quat::from_rotation_y(0.4));
        let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let (transform, interpolation) = restore(pose(drawn.0, quarter * drawn.1), drawn, current);
        let expected = Quat::from_rotation_y(0.7 + std::f32::consts::FRAC_PI_2);
        assert!(
            transform.rotation.abs_diff_eq(expected, 1e-6)
                || transform.rotation.abs_diff_eq(-expected, 1e-6),
            "{:?}, expected {expected:?}",
            transform.rotation
        );
        assert_eq!(interpolation.previous_rotation, transform.rotation);
        assert_eq!(interpolation.current_rotation, transform.rotation);
        assert_eq!(transform.translation, current.0);
    }

    fn tick(previous: (Vec3, Quat), current: (Vec3, Quat)) -> RenderInterpolation {
        let mut world = World::new();
        let entity = world
            .spawn((
                Combatant::new(1, 0, crate::states::match_config::CharacterClass::Warrior),
                pose(previous.0, previous.1),
                RenderInterpolation {
                    previous: previous.0,
                    current: previous.0,
                    previous_rotation: previous.1,
                    current_rotation: previous.1,
                    drawn: None,
                },
            ))
            .id();
        world
            .run_system_once(begin_render_interpolation_tick)
            .unwrap();
        *world.get_mut::<Transform>(entity).unwrap() = pose(current.0, current.1);
        world
            .run_system_once(end_render_interpolation_tick)
            .unwrap();
        *world.get::<RenderInterpolation>(entity).unwrap()
    }

    #[test]
    fn a_continuous_turn_is_blended_and_a_facing_snap_is_not() {
        let at = Vec3::new(1.0, 0.75, -2.0);
        let facing = Quat::from_rotation_y(0.3);
        // The fastest continuous turn, and well past it — still blended.
        for degrees in [10.0_f32, 40.0] {
            let turned = Quat::from_rotation_y(0.3 + degrees.to_radians());
            let segment = tick((at, facing), (at, turned));
            assert_eq!(segment.previous_rotation, facing, "{degrees} degree turn");
            assert_eq!(segment.current_rotation, turned);
        }
        // A reversal is a snap: both ends at the new facing.
        let reversed = Quat::from_rotation_y(0.3 + std::f32::consts::PI);
        let segment = tick((at, facing), (at, reversed));
        assert_eq!(segment.previous_rotation, reversed);
        // So is the facing of a teleport, however small the turn.
        let turned = Quat::from_rotation_y(0.35);
        let segment = tick((at, facing), (at + Vec3::X * 10.0, turned));
        assert_eq!(segment.previous_rotation, turned);
    }
}
