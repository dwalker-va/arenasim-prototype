//! Render interpolation: every unit and missile the sim moves is DRAWN between
//! its last two sim ticks, so motion is continuous at any display rate.
//!
//! The sim steps at 60Hz in `FixedUpdate`; the display renders at whatever the
//! monitor delivers. Drawn at its raw `Transform`, a walking unit stood still on
//! frames with no tick and jumped a whole tick's travel on frames with one — at
//! 120Hz, every other frame — while the camera and the HUD each followed it
//! their own way. This is Bevy's standard fixed-timestep pattern: the drawn
//! position is `lerp(previous tick, latest tick, overstep fraction)`.
//!
//! **The sim's `Transform` stays authoritative.** The interpolated translation
//! is written into `Transform` only for the render half of the frame — from
//! the end of the fixed loop through `Update` and `PostUpdate` — and the tick's
//! own value is restored before the next fixed loop runs, bit for bit. Nothing
//! in `FixedUpdate` can observe it. Everything drawn in between — the model and
//! everything parented to it, the camera's follow target, effects that follow a
//! unit's `Transform`, and the HUD (placed from post-propagation
//! `GlobalTransform`s) — agrees on one position. Graphical-only: headless never
//! registers these systems.
//!
//! **Discontinuities do not smear.** A newly spawned entity starts with both
//! ends at its spawn point. A tick that moves an entity farther than any mover
//! can travel in one tick (`MAX_TICK_TRAVEL`) is a teleport and is drawn there
//! directly. And a render-frame system that moves an entity itself (outside the
//! sim) keeps its move: the restore applies it to the sim's own value, not to
//! the blended one, and restarts the segment from there.
//!
//! Registered for `PlayMatch` only: the Animation Sandbox stages and dashes its
//! units from render-frame systems, and is drawn at the raw sim positions.

use crate::states::play_match::components::*;
use bevy::prelude::*;

/// Farther than any continuous mover travels in one 60Hz tick, so a longer
/// tick is a teleport. The fastest are missiles at 50 yd/s (0.83 yd per tick),
/// Disengage at 30 yd/s and Charge at 4x run speed (both about 0.5).
const MAX_TICK_TRAVEL: f32 = 3.0;

/// Whatever the sim moves on the fixed tick: combatants (pets included),
/// spell/shot projectiles and launched traps.
type SimMoved = Or<(
    With<Combatant>,
    With<Projectile>,
    With<TrapLaunchProjectile>,
)>;

/// `FixedFirst`: this tick's starting translation is the segment's earlier end.
pub fn begin_render_interpolation_tick(
    mut query: Query<(&Transform, &mut RenderInterpolation), SimMoved>,
) {
    for (transform, mut interpolation) in query.iter_mut() {
        interpolation.previous = transform.translation;
    }
}

/// `FixedLast`: the tick's result is the segment's later end. Entities the sim
/// spawned are enrolled here, with both ends at the spawn point.
pub fn end_render_interpolation_tick(
    mut commands: Commands,
    mut query: Query<(Entity, &Transform, Option<&mut RenderInterpolation>), SimMoved>,
) {
    for (entity, transform, interpolation) in query.iter_mut() {
        let current = transform.translation;
        match interpolation {
            Some(mut interpolation) => {
                interpolation.current = current;
                if interpolation.previous.distance(current) > MAX_TICK_TRAVEL {
                    interpolation.previous = current;
                }
            }
            None => {
                commands.entity(entity).insert(RenderInterpolation {
                    previous: current,
                    current,
                    drawn: None,
                });
            }
        }
    }
}

/// After the fixed loop: draw every enrolled entity at its interpolated
/// translation for the rest of the frame.
pub fn apply_render_interpolation(
    fixed_time: Res<Time<Fixed>>,
    mut query: Query<(&mut Transform, &mut RenderInterpolation)>,
) {
    let alpha = fixed_time.overstep_fraction().clamp(0.0, 1.0);
    for (mut transform, mut interpolation) in query.iter_mut() {
        let drawn = interpolation.previous.lerp(interpolation.current, alpha);
        transform.translation = drawn;
        interpolation.drawn = Some(drawn);
    }
}

/// Before the fixed loop: hand the sim back its own translation.
pub fn restore_sim_translation(mut query: Query<(&mut Transform, &mut RenderInterpolation)>) {
    for (mut transform, mut interpolation) in query.iter_mut() {
        let Some(drawn) = interpolation.drawn.take() else {
            continue;
        };
        // Something other than this module may have moved the entity during
        // the render half of the frame. Its MOVE is kept, applied to the sim's
        // own value — never the blended position it was applied on top of —
        // and the segment restarts from the result.
        if transform.translation == drawn {
            transform.translation = interpolation.current;
        } else {
            let kept = interpolation.current + (transform.translation - drawn);
            transform.translation = kept;
            interpolation.previous = kept;
            interpolation.current = kept;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn restore(transform: Vec3, drawn: Vec3, current: Vec3) -> (Vec3, RenderInterpolation) {
        let mut world = World::new();
        let entity = world
            .spawn((
                Transform::from_translation(transform),
                RenderInterpolation {
                    previous: Vec3::ZERO,
                    current,
                    drawn: Some(drawn),
                },
            ))
            .id();
        world.run_system_once(restore_sim_translation).unwrap();
        (
            world.get::<Transform>(entity).unwrap().translation,
            *world.get::<RenderInterpolation>(entity).unwrap(),
        )
    }

    #[test]
    fn restore_hands_the_sim_back_its_own_translation() {
        let current = Vec3::new(1.0, 0.75, -2.0);
        let (translation, interpolation) = restore(
            Vec3::new(0.5, 0.75, -1.0),
            Vec3::new(0.5, 0.75, -1.0),
            current,
        );
        assert_eq!(translation, current);
        assert_eq!(interpolation.drawn, None);
    }

    #[test]
    fn a_render_frame_move_is_kept_on_the_sim_value_not_the_blended_one() {
        // A render-frame system lifted the drawn unit by 2 yards. The sim gets
        // its own x/z back with the lift on top — never the blended x/z.
        let current = Vec3::new(1.0, 0.75, -2.0);
        let drawn = Vec3::new(0.5, 0.75, -1.0);
        let (translation, interpolation) = restore(drawn + Vec3::Y * 2.0, drawn, current);
        assert_eq!(translation, current + Vec3::Y * 2.0);
        assert_eq!(interpolation.previous, translation);
        assert_eq!(interpolation.current, translation);
    }
}
