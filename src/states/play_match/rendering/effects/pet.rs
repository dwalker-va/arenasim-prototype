use bevy::prelude::*;
use crate::states::play_match::components::*;

// ==============================================================================
// Pet Mesh Tilt (Quadruped Orientation)
// ==============================================================================

/// Reconstructs pet rotation as Y-facing * X-tilt so the capsule mesh lies
/// horizontal like a four-legged creature. Uses Euler decomposition to
/// extract the Y-facing angle regardless of whether the tilt is already
/// baked into the current rotation or the movement system just set a fresh
/// Y-only rotation this frame.
pub fn apply_pet_mesh_tilt(
    pets: Query<&Children, With<Pet>>,
    mut bodies: Query<&mut Transform, With<VisualBody>>,
) {
    let tilt = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    for children in pets.iter() {
        for child in children.iter() {
            let Ok(mut body_transform) = bodies.get_mut(child) else {
                continue;
            };
            // The parent already carries the Y-facing the sim wrote, and the
            // child's rotation composes on top of it, so the tilt is now a plain
            // local rotation instead of a decompose-and-reapply. Assigning it
            // unconditionally preserves the old behaviour of overriding a dying
            // pet's fall rotation on the next frame.
            body_transform.rotation = tilt;
        }
    }
}

