//! The shared body hold (`body_hold.rs`): the one rule every effect uses to
//! draw a piece in front of the body it sits on.
//!
//! Bodies are `AlphaMode::Blend` and write no depth, and Bevy sorts
//! transparent draws by each mesh's translation, so a piece whose translation
//! is deeper than the body's would be painted over. The hold slides such a
//! piece along its own view ray to just in front of the body's nearest point
//! and scales it by the same ratio. These probes hold it to that, from every
//! camera the client allows, for a combatant and a pet, against the RENDERED
//! body (`VisualBody`) rather than the sim root, on a body that runs, bobs and
//! is lifted, at 8, 16 and 33 ms frames.
//!
//! Every expected value is derived here from the body's shape — a combatant
//! is `Capsule3d::new(0.5, 1.5)` centred on its body; a pet is
//! `Capsule3d::new(0.35, 0.6)` hung 0.45 below its root, held against the
//! sphere bounding it — never from the module's constants. Each effect keeps
//! its own burial probe beside these.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{Combatant, Pet, PetType, VisualBody};
use arenasim::states::play_match::{body_capsule, body_held_point, BodyHold, HoldDisjoint};
use arenasim::CharacterClass;

/// The clearance in front of the body's nearest point.
const CLEARANCE: f32 = 0.05;
/// A pet's body hangs this far below its root.
const PET_DROP: f32 = 0.45;
/// The sphere bounding a pet's laid-down capsule.
const PET_BOUND: f32 = 0.35 + 0.3;

/// The client camera's orbit (`camera.rs`): polar angle from the vertical in
/// [0.1, 1.5] (0.675, 38.7 degrees, the default), zoom in [20, 220].
fn orbit(target: Vec3, polar: f32, zoom: f32, yaw: f32) -> Transform {
    let eye = target
        + Vec3::new(
            zoom * polar.sin() * yaw.sin(),
            zoom * polar.cos(),
            zoom * polar.sin() * yaw.cos(),
        );
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}

/// The view depth of the nearest point of a body whose centre is `centre`.
fn body_nearest(cam: &Transform, centre: Vec3, pet: bool) -> f32 {
    let f = cam.forward().as_vec3();
    let depth = |p: Vec3| (p - cam.translation).dot(f);
    if pet {
        depth(centre) - PET_BOUND
    } else {
        depth(centre - Vec3::Y * 0.75).min(depth(centre + Vec3::Y * 0.75)) - 0.5
    }
}

/// On a 16:9 screen through the client's 45 degree vertical field of view.
fn on_screen(cam: &Transform, p: Vec3) -> bool {
    let local = cam.rotation.inverse() * (p - cam.translation);
    let depth = -local.z;
    let half_v = 22.5f32.to_radians().tan();
    depth > 0.1 && local.y.abs() <= depth * half_v && local.x.abs() <= depth * half_v * 16.0 / 9.0
}

/// A cheap hash in [0, 1), for scatter.
fn draw(seed: u32, k: u32) -> f32 {
    let s = seed
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(k.wrapping_mul(0x85EB_CA6B))
        .wrapping_mul(747_796_405)
        .wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// A point within 1.6 yd of the chest of a body centred at `centre`: where
/// the held effects stand — shells round the chest, rings rising from the
/// feet, streaks falling from the head.
fn scatter(centre: Vec3, seed: u32) -> Vec3 {
    let d = Vec3::new(
        draw(seed, 1) * 2.0 - 1.0,
        draw(seed, 2) * 2.0 - 1.0,
        draw(seed, 3) * 2.0 - 1.0,
    );
    centre + Vec3::Y * 0.45 + d.normalize_or_zero() * (0.2 + 1.4 * draw(seed, 4))
}

/// From every camera the client allows — every polar pitch, every zoom, four
/// yaws — for bodies over a ±40 yd grid, a combatant's and a pet's: every
/// piece on screen is drawn on its own view ray, scaled by its depth ratio
/// (0 < k <= 1), and sorts in front of the whole body by at least the
/// clearance; a piece already that far in front is left where it is.
#[test]
fn every_piece_is_drawn_in_front_of_the_body_from_every_client_camera() {
    let (mut checked, mut held) = (0usize, 0usize);
    for polar in [0.1f32, 0.3, 0.675, 1.0, 1.25, 1.5] {
        for zoom in [20.0f32, 60.0, 220.0] {
            for yaw in [0.0f32, 1.0, 2.5, 4.0] {
                let cam = orbit(Vec3::new(0.0, 1.0, 0.0), polar, zoom, yaw);
                let (eye, forward) = (cam.translation, cam.forward().as_vec3());
                let depth = |p: Vec3| (p - eye).dot(forward);
                for gx in -2..=2 {
                    for gz in -2..=2 {
                        for pet in [false, true] {
                            let root = Vec3::new(gx as f32 * 20.0, 1.0, gz as f32 * 20.0);
                            let centre = if pet { root - Vec3::Y * PET_DROP } else { root };
                            let nearest = body_nearest(&cam, centre, pet);
                            let body = body_capsule(root, pet);
                            for seed in 0..24u32 {
                                let at = scatter(centre, seed);
                                if !on_screen(&cam, at) || nearest <= 1.0 {
                                    continue;
                                }
                                let (drawn, k) = body_held_point(at, body, eye, forward);
                                let label = format!(
                                    "polar {polar} zoom {zoom} yaw {yaw} pet {pet} at {at}"
                                );
                                assert!(k > 0.0 && k <= 1.0, "{label}: k {k}");
                                assert!(
                                    depth(drawn) <= nearest - CLEARANCE + 1e-3,
                                    "{label}: drawn at {} behind the body's {nearest}",
                                    depth(drawn)
                                );
                                assert!(depth(drawn) < depth(centre), "{label}: sorts behind");
                                if depth(at) <= nearest - CLEARANCE {
                                    assert_eq!((drawn, k), (at, 1.0), "{label}: moved");
                                } else {
                                    held += 1;
                                    // On its own ray, at its depth ratio.
                                    assert!(
                                        (drawn - eye).normalize().dot((at - eye).normalize())
                                            > 1.0 - 1e-6,
                                        "{label}: off its view ray"
                                    );
                                    assert!(
                                        (k - depth(drawn) / depth(at)).abs() < 1e-4,
                                        "{label}: k {k} is not the depth ratio"
                                    );
                                }
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        checked > 50_000 && held > 15_000,
        "{held} held of {checked}"
    );
}

/// A small move of the piece, the body or the eye moves the held point by
/// about as much: nothing jumps as a body runs under a turning camera.
#[test]
fn the_hold_is_continuous() {
    let mut checked = 0;
    for polar in [0.1f32, 0.675, 1.5] {
        for yaw in [0.0f32, 2.5] {
            let cam = orbit(Vec3::new(3.0, 1.0, -2.0), polar, 40.0, yaw);
            let (eye, forward) = (cam.translation, cam.forward().as_vec3());
            for pet in [false, true] {
                let root = Vec3::new(3.0, 1.0, -2.0);
                for seed in 0..64u32 {
                    let at = scatter(root, seed);
                    let (base, _) = body_held_point(at, body_capsule(root, pet), eye, forward);
                    let nudge = Vec3::new(0.01, -0.007, 0.004);
                    for (a, r, e) in [
                        (at + nudge, root, eye),
                        (at, root + nudge, eye),
                        (at, root, eye + nudge),
                    ] {
                        let (moved, _) = body_held_point(a, body_capsule(r, pet), e, forward);
                        assert!(
                            moved.distance(base) < 0.02,
                            "polar {polar} yaw {yaw} pet {pet}: a 0.013 yd nudge moved it {}",
                            moved.distance(base)
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 3 * 2 * 2 * 64 * 3);
}

// ── Through `BodyHold`, on a live body ──────────────────────────────────────

/// A piece held in front of `owner`'s rendered body by [`hold_pieces`]:
/// `at` is where it truly is.
#[derive(Component)]
struct Piece {
    owner: Entity,
    at: Vec3,
}

/// The probe's placement: every piece held in front of its owner's rendered
/// body, scaled by the hold's ratio — the shape of every hold in the client.
fn hold_pieces(hold: BodyHold, mut pieces: Query<(&Piece, &mut Transform), HoldDisjoint>) {
    let view = hold.view();
    for (piece, mut transform) in pieces.iter_mut() {
        let Some(body) = hold.body(piece.owner) else {
            continue;
        };
        let (drawn, k) = match view {
            Some(v) => v.held(piece.at, body.capsule()),
            None => (piece.at, 1.0),
        };
        transform.translation = drawn;
        transform.scale = Vec3::splat(k);
    }
}

struct Probe {
    app: App,
    unit: Entity,
    body: Entity,
    pet: bool,
}

impl Probe {
    /// A unit at `root` with its drawn body as its client spawns it: a
    /// combatant's at its root, a pet's hung at its rest height.
    fn new(frame: Duration, pet: bool, root: Vec3) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::transform::TransformPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app.add_systems(
            PostUpdate,
            hold_pieces.before(bevy::transform::TransformSystem::TransformPropagate),
        );
        let rest_y = if pet { -PET_DROP } else { 0.0 };
        let mut unit = app.world_mut().spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::from_translation(root),
        ));
        if pet {
            unit.insert(Pet {
                owner: Entity::PLACEHOLDER,
                pet_type: PetType::Spider,
            });
        }
        let unit = unit.id();
        let body = app
            .world_mut()
            .spawn((VisualBody { rest_y }, Transform::from_xyz(0.0, rest_y, 0.0)))
            .id();
        app.world_mut().entity_mut(unit).add_child(body);
        app.update();
        Probe {
            app,
            unit,
            body,
            pet,
        }
    }

    fn camera(&mut self, cam: Transform) {
        self.app.world_mut().spawn((Camera3d::default(), cam));
    }

    /// Where the drawn body's centre is: the unit's root, turned and moved
    /// by the body's offset from its rest pose — read straight off the
    /// entities, as the renderer will draw them.
    fn drawn_centre(&self) -> Vec3 {
        let world = self.app.world();
        let unit = world.get::<Transform>(self.unit).unwrap();
        let local = world.get::<Transform>(self.body).unwrap().translation;
        unit.transform_point(local)
    }
}

/// A body that runs past the camera at up to 9 yd/s — faster than any unit
/// runs, so a hold a frame behind it (0.08-0.15 yd a frame) would leave its
/// pieces in its far half — bobbing on its gait and lifted a yard as a
/// Cyclone lifts one, at 8, 16 and 33 ms frames: every frame, every piece is
/// drawn on its own ray in front of the body where it is DRAWN that frame.
#[test]
fn the_hold_follows_the_rendered_body_as_it_runs_bobs_and_lifts() {
    let mut held = 0;
    let mut checked = 0;
    for frame_ms in [8u64, 16, 33] {
        let frame = Duration::from_millis(frame_ms);
        for pet in [false, true] {
            for (polar, zoom, yaw) in [
                (0.1f32, 20.0f32, 0.0f32),
                (0.675, 60.0, 1.0),
                (1.5, 20.0, 2.5),
            ] {
                let start = Vec3::new(2.0, 1.0, -3.0);
                let mut p = Probe::new(frame, pet, start);
                let cam = orbit(start, polar, zoom, yaw);
                p.camera(cam);
                let pieces: Vec<Entity> = (0..24u32)
                    .map(|seed| {
                        let at = scatter(start, seed);
                        p.app
                            .world_mut()
                            .spawn((Piece { owner: p.unit, at }, Transform::from_translation(at)))
                            .id()
                    })
                    .collect();
                let toward = (cam.translation - start).with_y(0.0).normalize_or(Vec3::X);
                let across = Vec3::new(-toward.z, 0.0, toward.x);
                let rest = if pet { -PET_DROP } else { 0.0 };
                let mut t = 0.0f32;
                for _ in 0..(600 / frame_ms) {
                    t += frame_ms as f32 / 1000.0;
                    let root =
                        start + toward * 3.0 * (t * 2.5).sin() + across * 2.0 * (t * 2.3).sin();
                    let bob = 0.1 * (t * 9.0).sin() + if t > 0.3 { 1.0 } else { 0.0 };
                    {
                        let world = p.app.world_mut();
                        world.get_mut::<Transform>(p.unit).unwrap().translation = root;
                        world.get_mut::<Transform>(p.body).unwrap().translation.y = rest + bob;
                    }
                    // The pieces ride the body, as every effect's rig does.
                    for (seed, piece) in pieces.iter().enumerate() {
                        let at = scatter(root + Vec3::Y * bob, seed as u32);
                        p.app.world_mut().get_mut::<Piece>(*piece).unwrap().at = at;
                    }
                    p.app.update();
                    let centre = p.drawn_centre();
                    let nearest = body_nearest(&cam, centre, p.pet);
                    let (eye, forward) = (cam.translation, cam.forward().as_vec3());
                    let depth = |q: Vec3| (q - eye).dot(forward);
                    for piece in &pieces {
                        let world = p.app.world();
                        let truly = world.get::<Piece>(*piece).unwrap().at;
                        let g = world.get::<GlobalTransform>(*piece).unwrap();
                        let drawn = g.translation();
                        let label = format!("{frame_ms} ms, pet {pet}, polar {polar}, t {t}");
                        assert!(
                            depth(drawn) <= nearest - CLEARANCE + 1e-3,
                            "{label}: drawn at depth {} behind the drawn body's {nearest}",
                            depth(drawn)
                        );
                        assert!(
                            (drawn - eye).normalize().dot((truly - eye).normalize()) > 1.0 - 1e-6,
                            "{label}: off its view ray"
                        );
                        assert!(
                            (g.scale().x - depth(drawn) / depth(truly)).abs() < 1e-4,
                            "{label}: scale {} is not the depth ratio",
                            g.scale().x
                        );
                        held += usize::from(drawn.distance(truly) > 1e-3);
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 8_000 && held > 2_000, "{held} held of {checked}");
}

/// A pet is held against its own small body, hung below its root — not a
/// combatant's capsule about the root: from above, a piece a yard in front of
/// the pet's centre clears the pet, where a combatant's capsule, reaching
/// 1.25 yd up from its root, would cover it.
#[test]
fn a_pet_is_held_against_its_own_body() {
    let root = Vec3::new(0.0, 0.75, 0.0);
    let cam = orbit(root, 0.1, 20.0, 0.0);
    let (eye, forward) = (cam.translation, cam.forward().as_vec3());
    let centre = root - Vec3::Y * PET_DROP;
    let at = centre - forward;
    assert_eq!(
        body_held_point(at, body_capsule(root, true), eye, forward),
        (at, 1.0)
    );
    let (drawn, k) = body_held_point(at, body_capsule(root, false), eye, forward);
    assert!(
        k < 1.0 && drawn != at,
        "a combatant's capsule would hold it"
    );
}

/// With no camera nothing is held, and a body at or behind the eye holds
/// nothing either.
#[test]
fn no_camera_and_no_body_in_front_hold_nothing() {
    let mut p = Probe::new(Duration::from_millis(16), false, Vec3::new(1.0, 1.0, 1.0));
    let at = Vec3::new(1.0, 1.4, 0.0);
    let piece = p
        .app
        .world_mut()
        .spawn((Piece { owner: p.unit, at }, Transform::from_translation(at)))
        .id();
    p.app.update();
    let g = p.app.world().get::<GlobalTransform>(piece).unwrap();
    assert_eq!((g.translation(), g.scale().x), (at, 1.0));

    let eye = Vec3::new(0.0, 1.0, 0.0);
    let forward = Vec3::Z;
    let behind = body_capsule(Vec3::new(0.0, 1.0, -1.0), false);
    let at = Vec3::new(0.0, 1.0, 5.0);
    assert_eq!(body_held_point(at, behind, eye, forward), (at, 1.0));
}
