use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::druid_heals::{body_point, BODY_CENTRE_ABOVE_FLOOR};
use super::school_impact::IMPACT_PET_BODY_Y;
use crate::states::play_match::components::{Combatant, Pet, VisualBody};

// ==============================================================================
// The body hold — the one rule that draws an effect in front of its body
// ==============================================================================
//
// Combatant bodies are `AlphaMode::Blend` meshes, which write no depth. What
// decides which of two transparent draws covers the other is the transparent
// pass's sort, by each mesh's TRANSLATION (bevy_pbr material.rs: the key is
// the view-space depth of the mesh's translation, with no depth bias on
// either material). So a particle whose translation is deeper than the body's
// centre is drawn first and the body paints over it. The benches paint every
// particle over the body, so roughly half of any burst at a chest would
// vanish.
//
// The hold: a piece deeper than the body's nearest point (the nearer end of
// the body's swept segment, less its radius and [`BODY_HOLD_CLEARANCE`]) is
// slid along its own view ray to that depth and scaled by the same ratio.
// Holding is a homothety about the eye, so the piece covers exactly the
// pixels it would have, and it sorts — and, against an opaque body, depth
// tests — in front of the whole body. A piece already nearer stays put.
//
// Three things make a hold right, and this module owns all three:
//
// 1. **The body.** [`body_capsule`]: a combatant's upright capsule
//    (`Capsule3d::new(0.5, 1.5)` in `play_match/mod.rs`); a pet's capsule
//    lies along its facing, so it is held against the sphere bounding it.
// 2. **The RENDERED body.** The drawn body is the unit's `VisualBody` child,
//    which the walk and fear bobs, the swing step, the victory bounce and the
//    Cyclone lift move off the root ([`rendered_root`]). [`BodyHold`] reads
//    it live, so a hold is never a frame stale and never measured from the
//    sim root.
// 3. **When.** A hold must run after every writer of a body's transform —
//    the gaits in `Update`, `apply_cyclone_lift` in `PostUpdate` — and of the
//    camera's (`update_camera_position`, in `Update`), and before transform
//    propagation, so the held pose is the one drawn this frame. Every system
//    that holds is in [`BodyPlacement`], which `states/mod.rs` orders exactly
//    there (`tests/body_placement_order.rs` pins the membership and the
//    order).
//
// The camera is read as its `Transform`, not its `GlobalTransform`: before
// propagation a `GlobalTransform` is LAST frame's, and the camera is a root
// entity (spawned, never parented), so its `Transform` is its world pose as
// `update_camera_position` left it this frame. [`BodyHold::view`] asserts the
// camera has no parent in debug builds.

/// The combatant body's radius: `Capsule3d::new(0.5, 1.5)`. A glow quad
/// billboarded on the spine axis renders at or BEHIND the capsule's front
/// surface out to this radius, so any wrap layer must clear it with margin or
/// the body occludes it — how the third Healing Wave build (0.50/0.38/0.28
/// radii) lost its green glow (`healing_wave_glow_wraps_outside_the_body`).
pub const COMBATANT_BODY_RADIUS: f32 = 0.5;
/// Half the combatant capsule's swept segment: its 1.5 yd segment runs ±0.75
/// about the body's centre.
pub const COMBATANT_BODY_HALF_SEGMENT: f32 = 0.75;
/// The pet body: `Capsule3d::new(0.35, 0.6)`, centred `IMPACT_PET_BODY_Y`
/// below the pet's sim transform and laid along its facing by
/// `apply_pet_mesh_tilt`. Held against its bounding sphere — the capsule's
/// radius plus its half segment, about its centre — which contains the body
/// whichever way the pet faces.
pub const PET_BODY_BOUND: f32 = 0.35 + 0.3;
/// How far in front of the body's nearest point a held piece stands.
pub const BODY_HOLD_CLEARANCE: f32 = 0.05;

/// A body as a swept segment `a`-`b` of `radius`: every point within `radius`
/// of the segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyCapsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
}

impl BodyCapsule {
    /// The view depth of the body's nearest point, for a camera at `eye`
    /// looking along `forward`. Depth is linear along the segment, so its
    /// minimum is at an end; a sphere's is its centre less its radius.
    pub fn nearest_depth(&self, eye: Vec3, forward: Vec3) -> f32 {
        (self.a - eye).dot(forward).min((self.b - eye).dot(forward)) - self.radius
    }
}

/// The body of a unit whose rendered body stands as a unit at `root` would at
/// rest (see [`rendered_root`]). A combatant's is its upright capsule; a
/// pet's, whose capsule lies along its facing, is the sphere bounding it
/// (both ends at its centre).
pub fn body_capsule(root: Vec3, is_pet: bool) -> BodyCapsule {
    let (centre, half, radius) = if is_pet {
        (root + Vec3::Y * IMPACT_PET_BODY_Y, 0.0, PET_BODY_BOUND)
    } else {
        (root, COMBATANT_BODY_HALF_SEGMENT, COMBATANT_BODY_RADIUS)
    };
    BodyCapsule {
        a: centre - Vec3::Y * half,
        b: centre + Vec3::Y * half,
        radius,
    }
}

/// Where a piece truly at `at` is drawn, for a camera at `eye` looking along
/// `forward`, and the factor its size is scaled by.
///
/// A piece already nearer than the body's nearest point less
/// [`BODY_HOLD_CLEARANCE`] stays put. One deeper slides along its own view
/// ray (so it holds its screen position) to that depth, and is scaled by the
/// same ratio (so it holds its screen size). A body at or behind the eye
/// holds nothing.
pub fn body_held_point(at: Vec3, body: BodyCapsule, eye: Vec3, forward: Vec3) -> (Vec3, f32) {
    let depth = (at - eye).dot(forward);
    let nearest = body.nearest_depth(eye, forward) - BODY_HOLD_CLEARANCE;
    if depth <= nearest || nearest <= 0.0 {
        return (at, 1.0);
    }
    let k = nearest / depth;
    (eye + (at - eye) * k, k)
}

/// The camera, as a hold and a billboard need it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoldView {
    pub eye: Vec3,
    pub forward: Vec3,
    pub rotation: Quat,
}

impl HoldView {
    /// The view of a camera whose world pose is `camera`.
    pub fn of(camera: &Transform) -> Self {
        HoldView {
            eye: camera.translation,
            forward: camera.forward().as_vec3(),
            rotation: camera.rotation,
        }
    }

    /// [`body_held_point`] for this camera.
    pub fn held(&self, at: Vec3, body: BodyCapsule) -> (Vec3, f32) {
        body_held_point(at, body, self.eye, self.forward)
    }
}

/// [`body_held_point`] for the camera if there is one; with none, the piece is
/// drawn where it is.
pub fn held_in(view: Option<HoldView>, at: Vec3, body: BodyCapsule) -> (Vec3, f32) {
    match view {
        Some(v) => v.held(at, body),
        None => (at, 1.0),
    }
}

/// Where a unit at `unit` would stand for its RENDERED body to be where it is
/// now: its root, moved by however far its `VisualBody` has left the body's
/// rest pose (`rest_y`) — the walk bob, the fear bob, the swing step, the
/// Cyclone lift. Every body point measured from the root (`body_point`,
/// [`body_capsule`]) is then measured from the body the player sees. With no
/// body (a unit built without one), the root itself.
pub fn rendered_root(unit: &Transform, body: Option<(&Transform, &VisualBody)>) -> Vec3 {
    match body {
        Some((local, body)) => unit.transform_point(local.translation - Vec3::Y * body.rest_y),
        None => unit.translation,
    }
}

/// A unit's rendered body, read this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderedBody {
    /// [`rendered_root`]: the root a unit at rest would have for its body to
    /// stand where it is drawn.
    pub root: Vec3,
    /// The unit's own facing (its root's rotation).
    pub rotation: Quat,
    pub is_pet: bool,
}

impl RenderedBody {
    /// The body a hold measures.
    pub fn capsule(&self) -> BodyCapsule {
        body_capsule(self.root, self.is_pet)
    }

    /// The point `height` yards above the drawn body's feet, on a pet scaled
    /// to its stature (`body_point`).
    pub fn point(&self, height: f32) -> Vec3 {
        body_point(height, self.root, self.is_pet)
    }

    /// The centre of the drawn body.
    pub fn centre(&self) -> Vec3 {
        self.point(BODY_CENTRE_ABOVE_FLOOR)
    }
}

/// Everything a hold reads: the camera and every unit's rendered body.
///
/// A system that takes it and writes a `Transform` keeps that query off
/// combatants, bodies and cameras with [`HoldDisjoint`]. It reads the
/// `Camera3d` component itself as well as filtering on it, so a system that
/// holds is visible in the schedule as one that reads both a camera and a
/// `VisualBody` (`tests/body_placement_order.rs`).
#[derive(SystemParam)]
pub struct BodyHold<'w, 's> {
    camera: Query<
        'w,
        's,
        (&'static Transform, &'static Camera3d, Has<ChildOf>),
        (Without<Combatant>, Without<VisualBody>),
    >,
    units: Query<
        'w,
        's,
        (&'static Transform, Option<&'static Children>, Has<Pet>),
        (With<Combatant>, Without<Camera3d>, Without<VisualBody>),
    >,
    bodies: Query<
        'w,
        's,
        (&'static Transform, &'static VisualBody),
        (Without<Combatant>, Without<Camera3d>),
    >,
}

/// The filter a `Transform`-writing query beside a [`BodyHold`] carries.
pub type HoldDisjoint = (Without<Combatant>, Without<VisualBody>, Without<Camera3d>);

impl BodyHold<'_, '_> {
    /// The camera this frame, if there is one.
    pub fn view(&self) -> Option<HoldView> {
        let (camera, _, parented) = self.camera.iter().next()?;
        debug_assert!(
            !parented,
            "the hold reads the camera's Transform as its world pose; a parented camera needs its propagated pose"
        );
        Some(HoldView::of(camera))
    }

    /// `unit`'s rendered body this frame, or `None` when it is gone.
    pub fn body(&self, unit: Entity) -> Option<RenderedBody> {
        let (transform, children, is_pet) = self.units.get(unit).ok()?;
        let body = children.and_then(|kids| kids.iter().find_map(|k| self.bodies.get(k).ok()));
        Some(RenderedBody {
            root: rendered_root(transform, body),
            rotation: transform.rotation,
            is_pet,
        })
    }
}

/// Every system that holds a piece in front of a body, or otherwise stands on
/// a unit's rendered body: `PostUpdate`, after the Cyclone lift and before
/// transform propagation (module docs, point 3).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BodyPlacement;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_already_in_front_of_the_body_stays_put() {
        let body = body_capsule(Vec3::ZERO, false);
        let eye = Vec3::new(0.0, 10.0, 10.0);
        let forward = (Vec3::ZERO - eye).normalize();
        let near = eye + forward * 5.0;
        assert_eq!(body_held_point(near, body, eye, forward), (near, 1.0));
    }

    #[test]
    fn a_pet_is_held_against_the_sphere_round_its_hanging_body() {
        let body = body_capsule(Vec3::new(1.0, 0.75, 2.0), true);
        assert_eq!(body.a, body.b);
        assert_eq!(body.a, Vec3::new(1.0, 0.75 + IMPACT_PET_BODY_Y, 2.0));
        assert_eq!(body.radius, PET_BODY_BOUND);
    }
}
