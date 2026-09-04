//! Probes for the dispel treatment (`rendering/effects/dispel_ribbon.rs`):
//! the rippling ribbon that coils around the dispelled unit and climbs it.
//!
//! World-space geometry, as always: the coil must be OUTSIDE the body (the
//! first build coiled at 0.35 inside a 0.5 capsule and could only be seen on
//! Purge), on the unit for its whole life (an earlier build floated it 0.65yd
//! above the head), rolling a fold, ignited at birth, playing out from the
//! bottom with sparks off the fixed top, and cleaned up.

use std::time::Duration;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{Combatant, DispelRibbon, DispelSpark};
use arenasim::states::play_match::{
    cleanup_expired_dispel_ribbons, ribbon_climb, ribbon_consumed, ribbon_fold, ribbon_fold_amp,
    ribbon_fold_centre, ribbon_height, ribbon_ignition, ribbon_origin, ribbon_positions,
    ribbon_radii, ribbon_starts_at_base, ribbon_top_local, spawn_dispel_ribbon_visuals,
    update_dispel_ribbons, update_dispel_sparks,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
/// What `process_dispels` spawns the ribbon with.
const RIBBON_LIFE: f32 = 1.5;
/// The capsule spans this much either side of the transform.
const HALF_HEIGHT: f32 = 1.25;

struct Harness {
    app: App,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.add_systems(
            Update,
            (
                spawn_dispel_ribbon_visuals,
                update_dispel_ribbons,
                update_dispel_sparks,
                cleanup_expired_dispel_ribbons,
            )
                .chain(),
        );
        Harness { app }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn spawn_victim(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
            ))
            .id()
    }

    /// A successful dispel on `victim`, as `process_dispels` spawns it.
    fn dispel(&mut self, victim: Entity, class: CharacterClass) -> Entity {
        self.app
            .world_mut()
            .spawn(DispelRibbon {
                target: victim,
                caster_class: class,
                lifetime: RIBBON_LIFE,
                initial_lifetime: RIBBON_LIFE,
                spin: 0.0,
            })
            .id()
    }

    fn global<T: Component>(&mut self) -> Vec<(Entity, GlobalTransform)> {
        let mut q = self
            .app
            .world_mut()
            .query_filtered::<(Entity, &GlobalTransform), With<T>>();
        q.iter(self.app.world()).map(|(e, g)| (e, *g)).collect()
    }

    fn ribbon_bottom(&mut self) -> f32 {
        self.global::<DispelRibbon>()[0].1.translation().y
    }

    /// The ribbon's current vertex positions, in the rig's frame.
    fn ribbon_vertices(&mut self) -> Vec<[f32; 3]> {
        let handle = {
            let mut q = self.app.world_mut().query::<(&DispelRibbon, &Mesh3d)>();
            q.iter(self.app.world()).next().expect("a ribbon mesh").1 .0.clone()
        };
        let meshes = self.app.world().resource::<Assets<Mesh>>();
        match meshes
            .get(&handle)
            .expect("mesh")
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("positions")
        {
            VertexAttributeValues::Float32x3(v) => v.clone(),
            _ => panic!("positions are Float32x3"),
        }
    }

    /// World height of the strip's bottom and top rings (the rig spins about
    /// Y, so local Y is world Y plus the rig's translation).
    fn strip_span(&mut self) -> (f32, f32) {
        let rig_y = self.ribbon_bottom();
        let verts = self.ribbon_vertices();
        let bottom = verts[0][1].min(verts[1][1]);
        let top = verts[verts.len() - 1][1].max(verts[verts.len() - 2][1]);
        (rig_y + bottom, rig_y + top)
    }

    fn sparks(&mut self) -> Vec<(Vec3, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&DispelSpark, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(s, g)| (g.translation(), s.velocity))
            .collect()
    }

    fn ribbon_emissive(&mut self) -> f32 {
        let handle = {
            let mut q = self
                .app
                .world_mut()
                .query::<(&DispelRibbon, &MeshMaterial3d<StandardMaterial>)>();
            q.iter(self.app.world()).next().expect("a ribbon material").1 .0.clone()
        };
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        let e = materials.get(&handle).expect("material").emissive;
        e.red + e.green + e.blue
    }
}

// ── the coil is outside the body ───────────────────────────────────────────

/// Every vertex of the coil, at every moment of the fold, sits further from
/// the axis than the body's radius — or the ribbon is inside the capsule and
/// invisible, which is exactly how it shipped the first time.
#[test]
fn the_coil_wraps_outside_the_body() {
    let (radius, body) = ribbon_radii();
    assert!(radius > body, "coil radius {radius} must clear the body radius {body}");
    for age in [-1.0f32, 0.0, 0.1, 0.25, 0.4, 0.6, 0.9] {
        let verts = ribbon_positions(2.5, ribbon_height(), 0.26, radius, 96, age, 0.0);
        let nearest = verts
            .iter()
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(f32::INFINITY, f32::min);
        assert!(
            nearest > body + 0.02,
            "at age {age} a vertex swings to {nearest} from the axis, inside the {body} body"
        );
    }
}

/// For every dispeller class, the helix starts inside the capsule's height
/// range and never leaves the unit entirely: its bottom stays below the crown
/// for its whole life, and it ends higher than it began (it climbs).
#[test]
fn the_ribbon_climbs_the_body_and_never_floats_off_it() {
    for class in [
        CharacterClass::Priest,
        CharacterClass::Paladin,
        CharacterClass::Shaman,
        CharacterClass::Warlock,
    ] {
        let start = ribbon_origin(class, Vec3::ZERO, 1.0).y;
        let end = ribbon_origin(class, Vec3::ZERO, 0.0).y;
        assert!(
            start >= -HALF_HEIGHT - 1e-3 && start < HALF_HEIGHT,
            "{class:?}: helix bottom starts at {start}, off the capsule (-1.25..1.25)"
        );
        assert!(end > start + 0.4, "{class:?}: the ribbon should climb, {start} -> {end}");
        assert!(
            end < HALF_HEIGHT,
            "{class:?}: at the end the helix bottom ({end}) is above the crown — floating"
        );
        assert!(
            end + ribbon_height() > HALF_HEIGHT * 0.7,
            "{class:?}: the coil never reaches head height"
        );
    }
    // Friendly dispels rise from the FEET (BASE attachment); Purge from the chest.
    assert!(ribbon_starts_at_base(CharacterClass::Priest));
    assert!(ribbon_starts_at_base(CharacterClass::Paladin));
    assert!(ribbon_starts_at_base(CharacterClass::Warlock));
    assert!(!ribbon_starts_at_base(CharacterClass::Shaman));
    assert!(
        ribbon_origin(CharacterClass::Priest, Vec3::ZERO, 1.0).y
            < ribbon_origin(CharacterClass::Shaman, Vec3::ZERO, 1.0).y - 0.5,
        "a Purge starts visibly higher than a Dispel Magic"
    );
}

/// The rendered ribbon follows the pure origin — spawn, mid-life, and after
/// the victim moves.
#[test]
fn the_rendered_ribbon_tracks_its_origin_and_its_victim() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Priest);
    h.tick(1);
    let y0 = h.ribbon_bottom();
    assert!(
        (y0 - ribbon_origin(CharacterClass::Priest, Vec3::ZERO, 1.0).y).abs() < 0.05,
        "spawned at {y0}"
    );

    let moved = Vec3::new(3.0, 0.0, -2.0);
    h.app.world_mut().get_mut::<Transform>(victim).unwrap().translation = moved;
    h.tick(30);
    let at = h.global::<DispelRibbon>()[0].1.translation();
    assert!((at.x - moved.x).abs() < 1e-3 && (at.z - moved.z).abs() < 1e-3, "did not follow: {at:?}");
    assert!(at.y > y0 + 0.4, "should have climbed by half-life: {y0} -> {}", at.y);
    assert!(at.y < HALF_HEIGHT, "climbed off the body: {}", at.y);
}

// ── it ripples and ignites ─────────────────────────────────────────────────

/// One fold rolls up the strip from the held end: it lifts then pulls down,
/// its peak travels toward the top with age, it shrinks as it goes, and the
/// ends of the strip never move. A standing wave over the whole length would
/// fail every one of these — that was the tessellated first ripple.
#[test]
fn a_fold_rolls_up_the_ribbon() {
    let n = 200;
    let profile = |age: f32| -> Vec<f32> {
        (0..=n).map(|i| ribbon_fold(i as f32 / n as f32, age)).collect()
    };
    let peak_at = |p: &[f32]| -> (usize, f32) {
        p.iter()
            .enumerate()
            .map(|(i, v)| (i, v.abs()))
            .fold((0, 0.0), |best, cur| if cur.1 > best.1 { cur } else { best })
    };

    let early = profile(0.08);
    let mid = profile(0.25);
    let late = profile(0.45);
    let (i0, a0) = peak_at(&early);
    let (i1, a1) = peak_at(&mid);
    let (i2, a2) = peak_at(&late);
    assert!(i0 < i1 && i1 < i2, "the fold must travel up the strip: {i0} -> {i1} -> {i2}");
    assert!(a0 > a1 && a1 > a2, "the fold must shrink as it travels: {a0} -> {a1} -> {a2}");
    assert!(a0 > 0.12, "the first fold must be big enough to see: {a0}");
    assert!(
        (ribbon_fold_centre(0.25) - i1 as f32 / n as f32).abs() < 0.15,
        "the peak should sit near the fold centre"
    );

    // Up then down: a lifted lobe ahead of a pulled-down lobe, not a bump.
    let up = mid.iter().cloned().fold(f32::MIN, f32::max);
    let down = mid.iter().cloned().fold(f32::MAX, f32::min);
    assert!(up > 0.05 && down < -0.05, "the fold needs both lobes: {up} / {down}");
    let first_up = mid.iter().position(|v| *v > 0.03).unwrap();
    let first_down = mid.iter().position(|v| *v < -0.03).unwrap();
    assert!(first_up < first_down, "lift leads, pull follows");

    // The fold is LOCAL: most of the strip is still at any moment.
    let still = mid.iter().filter(|v| v.abs() < 0.01).count();
    assert!(still > n / 2, "a fold is a local event; {still} of {n} samples were still");

    // The held end and the free end stay put.
    for age in [0.05f32, 0.3, 0.6] {
        assert!(ribbon_fold(0.0, age).abs() < 1e-3);
        assert!(ribbon_fold(1.0, age).abs() < 1e-3);
    }

    // And the rendered strip actually moves between frames, up to the amplitude.
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Paladin);
    h.tick(2);
    let a = h.ribbon_vertices();
    h.tick(6);
    let b = h.ribbon_vertices();
    assert_eq!(a.len(), b.len(), "the fold must not change the vertex layout");
    let biggest = a
        .iter()
        .zip(b.iter())
        .map(|(p, q)| (p[1] - q[1]).abs())
        .fold(0.0f32, f32::max);
    assert!(biggest > 0.06, "the strip barely moved in 0.1s: {biggest}");
    assert!(biggest <= ribbon_fold_amp() * 1.6 + 1e-3);
}

/// The instant is marked by the ribbon itself: it ignites at birth and settles
/// within a fraction of its life, rather than a separate burst competing with
/// it.
#[test]
fn the_ribbon_ignites_then_settles() {
    assert!(ribbon_ignition(0.0) > 3.0);
    assert!(ribbon_ignition(0.6) < 1.4, "mostly settled by mid-life");
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Priest);
    h.tick(1);
    let birth = h.ribbon_emissive();
    h.tick(30);
    let settled = h.ribbon_emissive();
    assert!(
        birth > settled * 2.5,
        "the ribbon should flare at birth: {birth} vs {settled} at 0.5s"
    );
}

/// The ribbon must be OPAQUE. The body capsule is alpha-blended, and blended
/// meshes are sorted by distance without depth writes, so a blended ribbon
/// lost the draw-order fight and was painted over even where it was in front
/// of the body. It cannot fade, so it plays out instead.

/// The play-out: the climb completes, then the TOP end holds still in world
/// space while the BOTTOM end rises through the strip until nothing is left —
/// and sparks stream off the fixed top the whole time, and only then.
#[test]
fn the_ribbon_plays_out_from_the_bottom_while_sparks_leave_the_top() {
    // The two curves hand over exactly once.
    assert!(ribbon_climb(1.0) < 1e-6 && ribbon_consumed(1.0) < 1e-6);
    let handover = (0..=100).rev()
        .map(|i| i as f32 / 100.0)
        .find(|p| ribbon_climb(*p) >= 1.0 - 1e-6)
        .expect("the climb completes");
    assert!(ribbon_consumed(handover + 0.02) < 1e-6, "nothing is consumed before the climb ends");
    assert!(ribbon_consumed(handover - 0.05) > 0.0, "consumption starts as the climb ends");

    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Priest);
    // Climb phase: no sparks, both ends rising.
    h.tick(10);
    let (b0, t0) = h.strip_span();
    assert!(h.sparks().is_empty(), "no sparks while the ribbon is still climbing");
    h.tick(20);
    let (b1, t1) = h.strip_span();
    assert!(b1 > b0 + 0.2 && t1 > t0 + 0.2, "the whole strip climbs first: {b0}->{b1}, {t0}->{t1}");

    // Play-out: the top holds, the bottom keeps rising, the strip shortens.
    let frames_to_fix = ((RIBBON_LIFE * (1.0 - handover)) / TICK.as_secs_f32()).ceil() as u32 + 2;
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Priest);
    h.tick(frames_to_fix);
    let (bf, tf) = h.strip_span();
    let top_world = h.global::<DispelRibbon>()[0].1.translation()
        + h.global::<DispelRibbon>()[0].1.rotation() * ribbon_top_local();
    h.tick(12);
    let (bm, tm) = h.strip_span();
    assert!((tm - tf).abs() < 0.03, "the top end must hold once fixed: {tf} -> {tm}");
    assert!(bm > bf + 0.15, "the bottom end must keep rising: {bf} -> {bm}");
    assert!(tm - bm < tf - bf - 0.15, "the strip must be shortening");

    // Sparks come from the fixed top, and rise.
    let sparks = h.sparks();
    assert!(sparks.len() >= 5, "sparks should stream during the play-out, got {}", sparks.len());
    for (at, v) in &sparks {
        assert!(v.y > 0.5, "a spark must rise: {v:?}");
        let horizontal = Vec2::new(at.x - top_world.x, at.z - top_world.z).length();
        assert!(
            horizontal < 0.6 && at.y > top_world.y - 0.1 && at.y < top_world.y + 1.2,
            "a spark at {at:?} did not come from the top end {top_world:?}"
        );
    }

    // Near the end the strip has almost nothing left, then it is gone — and
    // the last sparks finish on their own.
    let frames_to_end = (RIBBON_LIFE / TICK.as_secs_f32()).ceil() as u32;
    h.tick(frames_to_end - frames_to_fix - 12 - 2);
    let (be, te) = h.strip_span();
    assert!(te - be < 0.2, "the strip should be nearly consumed: {be}..{te}");
    h.tick(6);
    assert!(h.global::<DispelRibbon>().is_empty(), "the ribbon should be gone");
    h.tick(45);
    assert!(h.sparks().is_empty(), "sparks should have finished after the ribbon");
}

#[test]
fn the_ribbon_writes_depth() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Priest);
    h.tick(1);
    let handle = {
        let mut q = h
            .app
            .world_mut()
            .query::<(&DispelRibbon, &MeshMaterial3d<StandardMaterial>)>();
        q.iter(h.app.world()).next().expect("a ribbon material").1 .0.clone()
    };
    let materials = h.app.world().resource::<Assets<StandardMaterial>>();
    let material = materials.get(&handle).expect("material");
    assert_eq!(material.alpha_mode, AlphaMode::Opaque);
    // And the ending exists in another form: the strip is consumed, not faded.
    assert!(ribbon_consumed(1.0) < 1e-6 && ribbon_consumed(0.6) < 1e-6, "whole while climbing");
    assert!(ribbon_consumed(0.2) > 0.4 && ribbon_consumed(0.2) < 0.7);
    assert!((ribbon_consumed(0.0) - 1.0).abs() < 1e-6, "gone by the end");
}

// ── hygiene ────────────────────────────────────────────────────────────────

/// The ribbon is the ONLY thing a dispel spawns now, and it casts no shadow.
#[test]
fn a_dispel_spawns_one_mesh_and_it_casts_no_shadow() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Priest);
    h.tick(2);
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<(Entity, Option<&NotShadowCaster>), With<Mesh3d>>();
    let mut n = 0;
    for (e, flag) in q.iter(h.app.world()) {
        n += 1;
        assert!(flag.is_some(), "{e:?} casts a shadow");
    }
    assert_eq!(n, 1, "the ribbon alone — no competing burst");
}

/// The ribbon retires after its life, and a victim that despawns mid-effect
/// must not panic anything.
#[test]
fn the_ribbon_expires_and_survives_losing_its_victim() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.dispel(victim, CharacterClass::Shaman);
    h.tick(2);
    assert_eq!(h.global::<DispelRibbon>().len(), 1);
    h.app.world_mut().despawn(victim);
    h.tick(150);
    assert!(h.global::<DispelRibbon>().is_empty(), "ribbon never expired");
    assert!(h.global::<DispelSpark>().is_empty(), "sparks never expired");
}
