//! Probes for the charge trail (Warrior Charge + Boar Charge).
//!
//! Round 2 of AS-14: the round-1 trail was one solid cylinder parked at the
//! dash origin — it read as a pipe behind the Warrior. The redesign lays a
//! SEQUENCE of elements along the actual dash path (the Classic source is a
//! red chest ribbon plus base dust — see
//! `design-docs/2026-09-06-charge-client-data.md`): thin vertical streak
//! segments at chest height and dust puffs at ground level, all fading over
//! their lifetimes. These pin the construction's WORLD geometry (positions on
//! the path, vertical placement, orientation, fade) — not bookkeeping.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    ChargeDustPuff, ChargeStreakSegment, ChargeTrailEmitter, ChargingState, Pet, PetType,
    VisualBody,
};
use arenasim::states::play_match::{spawn_charge_trail, update_and_cleanup_charge_trails};

const TICK: Duration = Duration::from_millis(50);
/// Charge moves at base speed x4 (~28 yd/s); one 50ms tick covers ~1.4 yd.
const DASH_STEP_PER_TICK: f32 = 1.4;

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app.add_systems(
        Update,
        (spawn_charge_trail, update_and_cleanup_charge_trails).chain(),
    );
    app
}

/// Drive the dash the way the movement system would: advance the charger
/// toward `to` by `DASH_STEP_PER_TICK` each tick and let the trail system
/// observe the live transform.
fn run_dash(app: &mut App, charger: Entity, from: Vec3, to: Vec3, ticks: usize) {
    let dir = (to - from).normalize();
    for step in 1..=ticks {
        let pos = from + dir * (DASH_STEP_PER_TICK * step as f32);
        app.world_mut()
            .entity_mut(charger)
            .get_mut::<Transform>()
            .unwrap()
            .translation = pos;
        app.update();
    }
}

fn streaks(app: &mut App) -> Vec<Transform> {
    let mut q = app.world_mut().query::<(&ChargeStreakSegment, &Transform)>();
    q.iter(app.world()).map(|(_, t)| *t).collect()
}

fn puffs(app: &mut App) -> Vec<Transform> {
    let mut q = app.world_mut().query::<(&ChargeDustPuff, &Transform)>();
    q.iter(app.world()).map(|(_, t)| *t).collect()
}

#[test]
fn the_trail_is_laid_along_the_dash_path_not_parked_at_the_origin() {
    let mut app = harness();
    let from = Vec3::new(0.0, 1.0, 0.0);
    let to = Vec3::new(12.0, 1.0, 0.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    let charger = app
        .world_mut()
        .spawn((Transform::from_translation(from), ChargingState { target }))
        .id();
    run_dash(&mut app, charger, from, to, 5);

    // 7 yd traveled at 0.55 yd spacing: a dozen segments, DISTRIBUTED.
    let found = streaks(&mut app);
    assert!(
        found.len() >= 6,
        "expected a sequence of streak segments along the dash (>= 6 after 7 yd), found {}",
        found.len()
    );
    let xs: Vec<f32> = found.iter().map(|t| t.translation.x).collect();
    let span = xs.iter().cloned().fold(f32::MIN, f32::max)
        - xs.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        span >= 4.0,
        "streak segments span {span:.2} yd of a 7 yd dash — the trail is parked, not laid along the path"
    );
    // Every segment sits ON the path (the dash ran along X at z = 0).
    for t in &found {
        assert!(
            t.translation.z.abs() < 0.3,
            "streak segment at {:?} is off the dash line",
            t.translation
        );
    }
}

#[test]
fn streak_segments_leave_visible_gaps_and_fade_as_a_comet_tail() {
    let mut app = harness();
    let from = Vec3::new(0.0, 1.0, 0.0);
    let to = Vec3::new(12.0, 1.0, 0.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    let charger = app
        .world_mut()
        .spawn((Transform::from_translation(from), ChargingState { target }))
        .id();
    run_dash(&mut app, charger, from, to, 5);
    // Let the tail age two frames past the last emission (the charger holds
    // still, so nothing new spawns) before reading the fade gradient.
    app.update();
    app.update();

    // Measure each segment's WORLD extent along the path off its real mesh
    // (not a stored length field): AABB half-extent on the long axis, scaled
    // by the transform. The dash runs along world X here.
    let segs: Vec<(Transform, Handle<Mesh>)> = {
        let mut q = app
            .world_mut()
            .query::<(&ChargeStreakSegment, &Transform, &Mesh3d)>();
        q.iter(app.world())
            .map(|(_, t, m)| (*t, m.0.clone()))
            .collect()
    };
    assert!(segs.len() >= 6, "guard: the probe must not go vacuous");
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mut extents: Vec<(f32, f32)> = segs
        .iter()
        .map(|(t, h)| {
            let aabb = meshes.get(h).unwrap().compute_aabb().unwrap();
            (t.translation.x, aabb.half_extents.x * t.scale.x)
        })
        .collect();
    extents.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in extents.windows(2) {
        let (c0, h0) = w[0];
        let (c1, h1) = w[1];
        let gap = (c1 - c0) - (h0 + h1);
        assert!(
            gap > 0.05,
            "adjacent streak segments overlap: centers {c0:.2} and {c1:.2} with half-lengths \
             {h0:.2} + {h1:.2} leave a gap of {gap:.2} yd — the trail reads as one solid slab, \
             not a sequence of discrete streaks"
        );
    }

    // Comet tail: by mid-dash the oldest surviving segment must be clearly
    // dissolving (well under half the newest one's glow), not holding near
    // full alpha as a wall.
    let by_age: Vec<(f32, Handle<StandardMaterial>)> = {
        let mut q = app
            .world_mut()
            .query::<(&ChargeStreakSegment, &MeshMaterial3d<StandardMaterial>)>();
        let mut v: Vec<(f32, Handle<StandardMaterial>)> = q
            .iter(app.world())
            .map(|(s, m)| (s.initial_lifetime - s.lifetime, m.0.clone()))
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    };
    let mats = app.world().resource::<Assets<StandardMaterial>>();
    let newest = mats.get(&by_age.first().unwrap().1).unwrap().base_color.alpha();
    let oldest = mats.get(&by_age.last().unwrap().1).unwrap().base_color.alpha();
    assert!(
        oldest < 0.5 * newest,
        "the oldest streak segment (alpha {oldest:.3}) must be clearly dissolving while the \
         newest (alpha {newest:.3}) is fresh — a comet tail, not a wall"
    );
}

#[test]
fn streak_segments_align_with_travel_and_sit_at_chest_height() {
    let mut app = harness();
    // Diagonal dash so the orientation assertion can't pass by identity.
    let from = Vec3::new(0.0, 1.0, 0.0);
    let to = Vec3::new(8.0, 1.0, 6.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    let charger = app
        .world_mut()
        .spawn((Transform::from_translation(from), ChargingState { target }))
        .id();
    run_dash(&mut app, charger, from, to, 4);

    let found = streaks(&mut app);
    assert!(!found.is_empty(), "guard: the probe must not go vacuous");
    let dir = (to - from).normalize();
    for t in &found {
        // The segment's long axis is local X, yawed onto the travel heading.
        let axis = t.rotation * Vec3::X;
        let off = axis.angle_between(dir).min(axis.angle_between(-dir));
        assert!(
            off < 0.05,
            "a streak segment lies {off:.2} rad off the travel direction {dir:?}"
        );
        // The height axis must stay world-vertical (a rolled ribbon reads as
        // a floor stripe).
        let up = t.rotation * Vec3::Y;
        assert!(
            up.angle_between(Vec3::Y) < 0.05,
            "a streak segment's height axis is tilted {:.2} rad off vertical",
            up.angle_between(Vec3::Y)
        );
        // Chest height: above the ground band, below head height.
        assert!(
            t.translation.y > 0.8 && t.translation.y < 2.0,
            "streak segment at y = {:.2}, not at chest height",
            t.translation.y
        );
    }
}

#[test]
fn dust_puffs_sit_at_ground_level_along_the_path() {
    let mut app = harness();
    let from = Vec3::new(0.0, 1.0, 0.0);
    let to = Vec3::new(10.0, 1.0, 0.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    let charger = app
        .world_mut()
        .spawn((Transform::from_translation(from), ChargingState { target }))
        .id();
    run_dash(&mut app, charger, from, to, 4);

    // Each emission point kicks up a CLUSTER of small particles, not one big
    // pearl: at least 3 puffs per streak segment emitted.
    let n_streaks = streaks(&mut app).len();
    let found = puffs(&mut app);
    assert!(
        found.len() >= 3 * n_streaks,
        "expected a cluster (3-5 particles) per emission point ({} streaks), found only {} puffs",
        n_streaks,
        found.len()
    );
    for t in &found {
        // The charger's transform rides at y = 1.0 (capsule center); the dust
        // must drop to the floor, not trail at body height — and stay ABOVE
        // the floor, not clip under it (the round-3 bound), even as the
        // particles drift upward over their lives.
        assert!(
            t.translation.y > 0.0 && t.translation.y < 0.8,
            "dust puff at y = {:.2} — dust belongs at ground level, above the floor",
            t.translation.y
        );
    }
    // Positional jitter: the dash ran along X at z = 0, so a scatter-free
    // construction leaves every particle exactly on the line (the pearls).
    let max_off = found
        .iter()
        .map(|t| t.translation.z.abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_off > 0.1,
        "every dust particle sits exactly on the dash line (max |z| = {max_off:.3}) — \
         no cluster scatter"
    );
    // Varied small sizes, measured off the REAL meshes: world radius = mesh
    // AABB half-extent x transform scale (particles grow as they age, so the
    // bound is generous upward but still far under the old 0.16-radius pearl
    // at full body scale).
    let sized: Vec<(Handle<Mesh>, f32)> = {
        let mut q = app.world_mut().query::<(&ChargeDustPuff, &Transform, &Mesh3d)>();
        q.iter(app.world())
            .map(|(_, t, m)| (m.0.clone(), t.scale.x))
            .collect()
    };
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mut radii: Vec<f32> = sized
        .iter()
        .map(|(h, s)| meshes.get(h).unwrap().compute_aabb().unwrap().half_extents.x * s)
        .collect();
    for r in &radii {
        assert!(
            (0.03..=0.4).contains(r),
            "dust particle world radius {r:.3} outside the small-puff band"
        );
    }
    radii.sort_by(f32::total_cmp);
    let spread = radii.last().unwrap() - radii.first().unwrap();
    assert!(
        spread > 0.02,
        "dust particles are uniform (radius spread {spread:.3}) — pearls on a string, \
         not varied puffs"
    );
}

#[test]
fn elements_fade_over_life_and_despawn_without_leaking() {
    let mut app = harness();
    let from = Vec3::new(0.0, 1.0, 0.0);
    let to = Vec3::new(10.0, 1.0, 0.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    let charger = app
        .world_mut()
        .spawn((Transform::from_translation(from), ChargingState { target }))
        .id();
    run_dash(&mut app, charger, from, to, 3);
    assert!(
        !streaks(&mut app).is_empty() && !puffs(&mut app).is_empty(),
        "guard: the probe must not go vacuous"
    );

    // Fade is real material change, not bookkeeping: a young element glows
    // brighter than an old one.
    {
        let mut q = app
            .world_mut()
            .query::<(&ChargeStreakSegment, &MeshMaterial3d<StandardMaterial>)>();
        let mut by_age: Vec<(f32, Handle<StandardMaterial>)> = q
            .iter(app.world())
            .map(|(s, m)| (s.lifetime, m.0.clone()))
            .collect();
        by_age.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (oldest, youngest) = (by_age.first().unwrap(), by_age.last().unwrap());
        assert!(
            oldest.0 < youngest.0,
            "guard: the dash must have produced segments of different ages"
        );
        let mats = app.world().resource::<Assets<StandardMaterial>>();
        let old_glow = mats.get(&oldest.1).unwrap().emissive.red;
        let young_glow = mats.get(&youngest.1).unwrap().emissive.red;
        assert!(
            old_glow < young_glow,
            "an older segment ({old_glow:.2}) must glow dimmer than a younger one ({young_glow:.2})"
        );
    }

    // Dash ends: the emitter disarms and every element fades out and despawns.
    app.world_mut().entity_mut(charger).remove::<ChargingState>();
    for _ in 0..24 {
        app.update();
    }
    assert_eq!(streaks(&mut app).len(), 0, "streak segments must not leak");
    assert_eq!(puffs(&mut app).len(), 0, "dust puffs must not leak");
    assert!(
        app.world().entity(charger).get::<ChargeTrailEmitter>().is_none(),
        "the emitter must disarm when the dash ends"
    );
}

#[test]
fn a_charging_pet_gets_the_same_trail_scaled_to_its_body() {
    let mut app = harness();
    // The REAL geometry from `spawn_pet` (`play_match/mod.rs`): the sim entity
    // sits at `owner_position + 0.75` (so world y 1.75 beside a combatant at
    // 1.0) while the `VisualBody` child carries `rest_y = 0.3 - 1.75` and the
    // capsule actually renders at world 0.3 (crown 0.95). Anchoring the ribbon
    // off the sim y instead of the body would float it ~0.7yd above the Boar's
    // head.
    const PET_SIM_Y: f32 = 1.75;
    const PET_MESH_Y: f32 = 0.3;
    let from = Vec3::new(2.0, PET_SIM_Y, 1.0);
    let to = Vec3::new(-6.0, PET_SIM_Y, 4.0);
    let target = app.world_mut().spawn(Transform::from_translation(to)).id();
    let owner = app.world_mut().spawn(Transform::default()).id();
    let body = app
        .world_mut()
        .spawn((
            VisualBody { rest_y: PET_MESH_Y - PET_SIM_Y },
            Transform::from_xyz(0.0, PET_MESH_Y - PET_SIM_Y, 0.0),
        ))
        .id();
    let boar = app
        .world_mut()
        .spawn((
            Transform::from_translation(from),
            ChargingState { target },
            Pet {
                owner,
                pet_type: PetType::Boar,
            },
        ))
        .id();
    app.world_mut().entity_mut(boar).add_child(body);
    run_dash(&mut app, boar, from, to, 4);

    let found = streaks(&mut app);
    assert!(
        found.len() >= 6,
        "the Boar's dash must lay a streak sequence too, found {}",
        found.len()
    );
    for t in &found {
        // The band must sit on the RENDERED body (centre 0.3, crown 0.95) —
        // scaled chest anchor above the body centre, well under the sim y.
        assert!(
            t.translation.y > 0.3 && t.translation.y < 0.9,
            "a Boar streak segment at y = {:.2} — anchored off the sim transform, not the rendered body",
            t.translation.y
        );
    }
    assert!(
        !puffs(&mut app).is_empty(),
        "the Boar's dash must kick up dust too"
    );
}
