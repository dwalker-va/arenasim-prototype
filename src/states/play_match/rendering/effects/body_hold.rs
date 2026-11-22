use bevy::prelude::*;

use super::heal_impact::COMBATANT_BODY_RADIUS;
use super::school_impact::IMPACT_PET_BODY_Y;

// ==============================================================================
// The view-ray hold: an effect piece drawn in front of the body it sits on
// ==============================================================================
//
// A bench is a 2D canvas: it paints the bodies first and every effect over
// them. Here a body is an `AlphaMode::Blend` mesh, which writes no depth, so
// what decides the overlap is the transparent pass's sort by each mesh's
// translation: a piece deeper than the body's centre is drawn first and the
// body paints over it. A burst born at the chest would lose about half of
// itself behind its victim.
//
// The hold draws such a piece on its own view ray, slid toward the camera
// until it stands nearer than the body's nearest point, and scaled by the
// same ratio. Scaling a figure about the eye leaves its projection exactly
// where it was, so the piece covers the screen footprint its bench gives it —
// a camera-facing sprite and a flat piece alike — and sorts in front of the
// body. This is the rule AS-347 wrote for Siphon Life (`siphon_drawn_point`
// on its branch), here as a shared helper.

/// How far in front of the body's nearest point a held piece stands.
pub const BODY_HOLD_CLEARANCE: f32 = 0.05;

/// The combatant body: `Capsule3d::new(0.5, 1.5)` centred on its transform
/// (`play_match/mod.rs`), so its swept segment runs ±0.75 about the centre.
const COMBATANT_BODY_HALF_SEGMENT: f32 = 0.75;
/// The pet body: `Capsule3d::new(0.35, 0.6)`, centred `IMPACT_PET_BODY_Y`
/// below the pet's sim transform and laid along its facing. Held against its
/// bounding sphere — the capsule's radius plus its half-segment, about its
/// centre — which contains the body whichever way the pet faces.
const PET_BODY_BOUND: f32 = 0.35 + 0.3;

/// A body as a swept segment: its two ends and its radius, for a unit at
/// `translation`. A combatant's is its upright capsule; a pet's, whose
/// capsule lies along its facing, is the sphere bounding it (both ends at its
/// centre).
pub fn body_capsule(translation: Vec3, is_pet: bool) -> (Vec3, Vec3, f32) {
    let (centre, half, radius) = if is_pet {
        (
            translation + Vec3::Y * IMPACT_PET_BODY_Y,
            0.0,
            PET_BODY_BOUND,
        )
    } else {
        (
            translation,
            COMBATANT_BODY_HALF_SEGMENT,
            COMBATANT_BODY_RADIUS,
        )
    };
    (centre - Vec3::Y * half, centre + Vec3::Y * half, radius)
}

/// Where a piece truly at `at` is drawn, for a camera at `eye` looking along
/// `forward`, and the factor its quad is scaled by.
///
/// A piece already nearer than the body's nearest point (the nearer end of
/// its segment, less its radius and [`BODY_HOLD_CLEARANCE`]) stays put; one
/// deeper slides along its own view ray to that depth and is scaled by the
/// same ratio.
pub fn held_in_front(
    at: Vec3,
    capsule: (Vec3, Vec3, f32),
    eye: Vec3,
    forward: Vec3,
) -> (Vec3, f32) {
    let depth = (at - eye).dot(forward);
    let (a, b, radius) = capsule;
    let nearest = (a - eye).dot(forward).min((b - eye).dot(forward)) - radius - BODY_HOLD_CLEARANCE;
    if depth <= nearest || nearest <= 0.0 {
        return (at, 1.0);
    }
    let k = nearest / depth;
    (eye + (at - eye) * k, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_behind_the_body_is_pulled_in_front_on_its_own_ray() {
        let capsule = body_capsule(Vec3::new(0.0, 1.0, 0.0), false);
        let eye = Vec3::new(0.0, 20.0, 30.0);
        let forward = (Vec3::new(0.0, 1.0, 0.0) - eye).normalize();
        let behind = Vec3::new(0.2, 1.45, -0.4);
        let (drawn, k) = held_in_front(behind, capsule, eye, forward);
        assert!(k < 1.0);
        // On the same ray, k of the way out.
        assert!((drawn - (eye + (behind - eye) * k)).length() < 1e-5);
        let (a, b, r) = capsule;
        let nearest = (a - eye).dot(forward).min((b - eye).dot(forward)) - r;
        assert!((drawn - eye).dot(forward) < nearest);
        // A piece already in front is untouched.
        let near = eye + forward * 5.0;
        assert_eq!(held_in_front(near, capsule, eye, forward), (near, 1.0));
    }
}
