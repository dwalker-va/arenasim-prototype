//! Probes for the Disengage wind trail (Hunter backward leap).
//!
//! AS-20: the old trail was one elongated wind-streak cylinder parked at the
//! LEAP ORIGIN — the same origin-parked idiom the Charge trail abandoned in
//! AS-14, with the same defect (a static pipe behind the mover). The port
//! lays a SEQUENCE of elements along the actual leap path: thin wind slivers
//! at body height plus tiny spark motes (the white-blue additive vocabulary
//! of Classic Disengage's own kit model, `spells/blink_impact_chest.m2`),
//! all fading per element, with a launch flash at the jump point. These pin
//! the construction's WORLD geometry — positions on the path, vertical
//! placement, orientation, scatter, fade — via `GlobalTransform` after a
//! full frame (transform propagation included), not bookkeeping fields.
//!
//! Runs on `MinimalPlugins` + `TransformPlugin` + `AssetPlugin` — no window,
//! no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    DisengageSparkMote, DisengageTrailEmitter, DisengageWindStreak, DisengagingState,
};
use arenasim::states::play_match::{spawn_disengage_trail, update_and_cleanup_disengage_trails};

const TICK: Duration = Duration::from_millis(50);
/// Disengage leaps at `DISENGAGE_SPEED` (30 yd/s); one 50ms tick covers 1.5 yd.
const LEAP_STEP_PER_TICK: f32 = 1.5;
/// The combatant sim transform rides at world y = 1.0 (capsule centre over
/// the y = 0 arena floor) — the Hunter's REAL height during the leap, which
/// `move_to_target` never changes (the leap direction is horizontal).
const SIM_Y: f32 = 1.0;

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::transform::TransformPlugin,
        bevy::asset::AssetPlugin::default(),
    ));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app.add_systems(
        Update,
        (spawn_disengage_trail, update_and_cleanup_disengage_trails).chain(),
    );
    app
}

/// Drive the leap the way `move_to_target` would: advance the leaper along
/// its `DisengagingState` direction by `LEAP_STEP_PER_TICK` each tick and
/// let the trail system observe the live transform.
fn run_leap(app: &mut App, leaper: Entity, from: Vec3, dir: Vec3, ticks: usize) {
    let dir = dir.normalize();
    for step in 1..=ticks {
        let pos = from + dir * (LEAP_STEP_PER_TICK * step as f32);
        app.world_mut()
            .entity_mut(leaper)
            .get_mut::<Transform>()
            .unwrap()
            .translation = pos;
        app.update();
    }
}

fn spawn_leaper(app: &mut App, from: Vec3, dir: Vec3) -> Entity {
    app.world_mut()
        .spawn((
            Transform::from_translation(from),
            DisengagingState {
                direction: dir.normalize(),
                distance_remaining: 15.0,
            },
        ))
        .id()
}

/// World poses of all wind slivers, read off `GlobalTransform` (populated by
/// the transform propagation that ran in the same frame).
fn slivers(app: &mut App) -> Vec<GlobalTransform> {
    let mut q = app
        .world_mut()
        .query::<(&DisengageWindStreak, &GlobalTransform)>();
    q.iter(app.world()).map(|(_, t)| *t).collect()
}

fn motes(app: &mut App) -> Vec<GlobalTransform> {
    let mut q = app
        .world_mut()
        .query::<(&DisengageSparkMote, &GlobalTransform)>();
    q.iter(app.world()).map(|(_, t)| *t).collect()
}

#[test]
fn the_trail_is_laid_along_the_leap_path_not_parked_at_the_origin() {
    let mut app = harness();
    let from = Vec3::new(0.0, SIM_Y, 0.0);
    let leaper = spawn_leaper(&mut app, from, Vec3::X);
    run_leap(&mut app, leaper, from, Vec3::X, 5);

    // 7.5 yd traveled at 0.55 yd spacing with 2-3 slivers per point: a
    // couple dozen slivers, DISTRIBUTED along the path.
    let found = slivers(&mut app);
    assert!(
        found.len() >= 12,
        "expected a sequence of wind slivers along the leap (>= 12 after 7.5 yd), found {}",
        found.len()
    );
    let xs: Vec<f32> = found.iter().map(|t| t.translation().x).collect();
    let span = xs.iter().cloned().fold(f32::MIN, f32::max)
        - xs.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        span >= 4.0,
        "wind slivers span {span:.2} yd of a 7.5 yd leap — the trail is parked, not laid along the path"
    );
    // Every sliver sits ON the path line (the leap ran along X at z = 0;
    // lateral jitter is bounded at 0.25 yd).
    for t in &found {
        assert!(
            t.translation().z.abs() < 0.35,
            "wind sliver at {:?} is off the leap line",
            t.translation()
        );
    }
}

#[test]
fn wind_slivers_align_with_travel_and_ride_the_body_band() {
    let mut app = harness();
    // Diagonal leap so the orientation assertion can't pass by identity.
    let dir = Vec3::new(-4.0, 0.0, 3.0).normalize();
    let from = Vec3::new(2.0, SIM_Y, -1.0);
    let leaper = spawn_leaper(&mut app, from, dir);
    run_leap(&mut app, leaper, from, dir, 4);

    let found = slivers(&mut app);
    assert!(found.len() >= 8, "guard: the probe must not go vacuous");
    for t in &found {
        // The sliver's long axis is local X, yawed onto the travel heading —
        // a speed-line pointing the way the Hunter flies.
        let axis = t.rotation() * Vec3::X;
        let off = axis.angle_between(dir).min(axis.angle_between(-dir));
        assert!(
            off < 0.05,
            "a wind sliver lies {off:.2} rad off the travel direction {dir:?}"
        );
        // Body band: the leap is an AIR move off a sim transform at y = 1.0
        // (chest anchor 0.55 above it, jitter ±0.4) — never a floor stripe,
        // never above head height.
        let y = t.translation().y;
        assert!(
            (1.0..=2.1).contains(&y),
            "wind sliver at y = {y:.2} — outside the leaping body band"
        );
    }
}

#[test]
fn the_launch_point_flashes_a_spark_burst() {
    let mut app = harness();
    let from = Vec3::new(3.0, SIM_Y, 2.0);
    let _leaper = spawn_leaper(&mut app, from, Vec3::Z);
    // One frame, no movement yet: the emitter arms and the launch flash
    // fires at the jump point (the `blink_impact_chest` one-shot analog).
    app.update();

    let found = motes(&mut app);
    assert!(
        found.len() >= 5,
        "expected a launch burst of spark motes (>= 5) at the jump point, found {}",
        found.len()
    );
    for t in &found {
        let d_xz = (t.translation().xz() - from.xz()).length();
        assert!(
            d_xz < 1.2,
            "a launch mote sits {d_xz:.2} yd from the jump point — the burst must flash AT the launch"
        );
        // Around the chest of the real body (sim y 1.0 + chest 0.55 ± 0.5
        // scatter), never on the arena floor (world y = 0).
        let y = t.translation().y;
        assert!(
            (0.9..=2.2).contains(&y),
            "launch mote at y = {y:.2} — off the leaping body"
        );
    }
}

#[test]
fn spark_motes_scatter_along_the_path_with_varied_sizes() {
    let mut app = harness();
    let from = Vec3::new(0.0, SIM_Y, 0.0);
    let leaper = spawn_leaper(&mut app, from, Vec3::X);
    run_leap(&mut app, leaper, from, Vec3::X, 5);

    let found = motes(&mut app);
    // 13 emission points at 2-3 motes each plus the launch burst.
    assert!(
        found.len() >= 12,
        "expected spark motes scattered along the leap (>= 12), found {}",
        found.len()
    );
    // Motes reach down the path, not just the launch point.
    let max_x = found
        .iter()
        .map(|t| t.translation().x)
        .fold(f32::MIN, f32::max);
    assert!(
        max_x > 4.0,
        "the farthest spark mote sits at x = {max_x:.2} — motes are parked at the launch, not laid along the path"
    );
    // Positional scatter: a scatter-free construction leaves every mote
    // exactly on the line.
    let max_off = found
        .iter()
        .map(|t| t.translation().z.abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_off > 0.1,
        "every spark mote sits exactly on the leap line (max |z| = {max_off:.3}) — no scatter"
    );
    // Air move: nothing touches the floor plane (world y = 0); motes hang
    // around the body and drift upward.
    for t in &found {
        assert!(
            t.translation().y > 0.9,
            "spark mote at y = {:.2} — the leap trail is an air effect, nothing sits on the floor",
            t.translation().y
        );
    }
    // Tiny varied glints, measured off the REAL meshes: world radius = mesh
    // AABB half-extent x transform scale.
    let sized: Vec<(Handle<Mesh>, f32)> = {
        let mut q = app
            .world_mut()
            .query::<(&DisengageSparkMote, &GlobalTransform, &Mesh3d)>();
        q.iter(app.world())
            .map(|(_, t, m)| (m.0.clone(), t.scale().x))
            .collect()
    };
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mut radii: Vec<f32> = sized
        .iter()
        .map(|(h, s)| meshes.get(h).unwrap().compute_aabb().unwrap().half_extents.x * s)
        .collect();
    for r in &radii {
        assert!(
            (0.02..=0.12).contains(r),
            "spark mote world radius {r:.3} outside the tiny-glint band"
        );
    }
    radii.sort_by(f32::total_cmp);
    let spread = radii.last().unwrap() - radii.first().unwrap();
    assert!(
        spread > 0.01,
        "spark motes are uniform (radius spread {spread:.3}) — pearls, not glints"
    );
}

#[test]
fn elements_fade_over_life_and_despawn_without_leaking() {
    let mut app = harness();
    let from = Vec3::new(0.0, SIM_Y, 0.0);
    let leaper = spawn_leaper(&mut app, from, Vec3::X);
    run_leap(&mut app, leaper, from, Vec3::X, 4);
    assert!(
        !slivers(&mut app).is_empty() && !motes(&mut app).is_empty(),
        "guard: the probe must not go vacuous"
    );

    // Fade is real material change, not bookkeeping: a young sliver glows
    // brighter than an old one, in both alpha and emissive.
    {
        let mut q = app
            .world_mut()
            .query::<(&DisengageWindStreak, &MeshMaterial3d<StandardMaterial>)>();
        let mut by_age: Vec<(f32, Handle<StandardMaterial>)> = q
            .iter(app.world())
            .map(|(s, m)| (s.lifetime, m.0.clone()))
            .collect();
        by_age.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (oldest, youngest) = (by_age.first().unwrap(), by_age.last().unwrap());
        assert!(
            oldest.0 < youngest.0,
            "guard: the leap must have produced slivers of different ages"
        );
        let mats = app.world().resource::<Assets<StandardMaterial>>();
        let old_mat = mats.get(&oldest.1).unwrap();
        let young_mat = mats.get(&youngest.1).unwrap();
        assert!(
            old_mat.emissive.red < young_mat.emissive.red,
            "an older sliver ({:.2}) must glow dimmer than a younger one ({:.2})",
            old_mat.emissive.red,
            young_mat.emissive.red
        );
        assert!(
            old_mat.base_color.alpha() < young_mat.base_color.alpha(),
            "an older sliver's alpha ({:.3}) must sit below a younger one's ({:.3})",
            old_mat.base_color.alpha(),
            young_mat.base_color.alpha()
        );
    }

    // Leap ends (DisengagingState removed): the emitter disarms and every
    // element fades out and despawns.
    app.world_mut().entity_mut(leaper).remove::<DisengagingState>();
    for _ in 0..24 {
        app.update();
    }
    assert_eq!(slivers(&mut app).len(), 0, "wind slivers must not leak");
    assert_eq!(motes(&mut app).len(), 0, "spark motes must not leak");
    assert!(
        app.world()
            .entity(leaper)
            .get::<DisengageTrailEmitter>()
            .is_none(),
        "the emitter must disarm when the leap ends"
    );
}
