//! Probes for the Shaman totem template (`rendering/effects/totems.rs`): the
//! carved post, the orbiting ribbons, the top flame, the birth rise and the
//! death tilt with its dust and smoke.
//!
//! Everything is asserted in WORLD space, off `GlobalTransform` and the meshes
//! themselves — where the post's top is, where a ribbon's head is and how its
//! angle about the post advances, where flame and dust are born — never off
//! the rig's stored clocks.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window, no
//! GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform` never
//! propagates and every world position reads as the origin.

use std::f32::consts::TAU;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::components::{
    AuraType, ClientParticle, Totem, TotemElement, TotemPart, TotemRig,
};
use arenasim::states::play_match::{
    animate_client_particles, cleanup_totem_visuals, spawn_totem_visuals, update_totem_visuals,
    TOTEM_BIRTH_SECS, TOTEM_DEATH_SECS, TOTEM_DEATH_TILT_DEG, TOTEM_FLAME_HEIGHT_FRAC,
    TOTEM_FLAME_RATE, TOTEM_POST_HEIGHT, TOTEM_RIBBON_ALPHA, TOTEM_RIBBON_COUNT,
    TOTEM_RIBBON_ORBIT_HEIGHT_FRAC, TOTEM_RIBBON_ORBIT_RADIUS, TOTEM_RIBBON_REV_PER_SEC,
};

/// 20ms: 50 frames to a revolution.
const TICK: Duration = Duration::from_millis(20);
const DT: f32 = 0.02;

fn app() -> App {
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
            spawn_totem_visuals,
            update_totem_visuals,
            cleanup_totem_visuals,
            animate_client_particles,
        )
            .chain(),
    );
    // Prime `Time` so the first measured frame has a real delta.
    app.update();
    app
}

fn drop_totem(app: &mut App, element: TotemElement, at: Vec3) -> Entity {
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut()
        .spawn((
            Transform::from_translation(at),
            Totem {
                owner_team: 1,
                owner,
                element,
                radius: 20.0,
                duration_remaining: 30.0,
                aura_type: AuraType::SpellPowerIncrease,
                magnitude: 10.0,
                spell_school: SpellSchool::Nature,
            },
        ))
        .id()
}

fn run(app: &mut App, secs: f32) {
    for _ in 0..(secs / DT).round() as usize {
        app.update();
    }
}

fn rig_of(app: &mut App, totem: Entity) -> Option<TotemRig> {
    let mut q = app.world_mut().query::<&TotemRig>();
    q.iter(app.world())
        .find(|r| r.totem == totem)
        .map(|r| TotemRig {
            totem: r.totem,
            element: r.element,
            age: r.age,
            death_age: r.death_age,
            body: r.body,
            post: r.post,
            rune: r.rune,
            orbit: r.orbit,
            ribbon_material: r.ribbon_material.clone(),
            seed: r.seed,
            emitted: r.emitted,
            carry: r.carry,
            quad: r.quad.clone(),
            palettes: r.palettes.clone(),
        })
}

fn mesh_positions(app: &App, entity: Entity) -> Vec<Vec3> {
    let handle = app.world().get::<Mesh3d>(entity).unwrap().0.clone();
    let meshes = app.world().resource::<Assets<Mesh>>();
    match meshes
        .get(&handle)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
    {
        Some(VertexAttributeValues::Float32x3(p)) => {
            p.iter().map(|&v| Vec3::from_array(v)).collect()
        }
        _ => panic!("mesh without positions"),
    }
}

/// The post's apex in the world: its mesh's highest vertex (the knob's point,
/// on the axis) through its `GlobalTransform`. Picked in the mesh's own frame,
/// so a leaning post still reports its apex and not a wing tip that the lean
/// has lifted higher.
fn post_top(app: &mut App, totem: Entity) -> Vec3 {
    let post = rig_of(app, totem).expect("a rig").post;
    let global = *app.world().get::<GlobalTransform>(post).unwrap();
    let apex = mesh_positions(app, post)
        .into_iter()
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    global.transform_point(apex)
}

/// The head of ribbon `k`: where its leading edge's two vertices meet the
/// orbit, in the world.
fn ribbon_head(app: &mut App, totem: Entity, k: usize) -> Vec3 {
    let orbit = rig_of(app, totem).expect("a rig").orbit;
    let global = *app.world().get::<GlobalTransform>(orbit).unwrap();
    let p = mesh_positions(app, orbit);
    let per = p.len() / {
        let element = rig_of(app, totem).unwrap().element;
        let slot = match element {
            TotemElement::Air => 0,
            TotemElement::Earth => 1,
            TotemElement::Fire => 2,
            TotemElement::Water => 3,
        };
        TOTEM_RIBBON_COUNT[slot] as usize
    };
    global.transform_point((p[k * per] + p[k * per + 1]) * 0.5)
}

/// Every live particle: world position, velocity, age, and whether it draws
/// alpha-blended (the smoke).
fn particles(app: &mut App) -> Vec<(Vec3, Vec3, f32, bool)> {
    let found: Vec<(Vec3, Vec3, f32, Handle<StandardMaterial>)> = {
        let mut q = app.world_mut().query::<(
            &ClientParticle,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        q.iter(app.world())
            .filter(|(p, _, _)| p.age < p.life)
            .map(|(p, g, m)| (g.translation(), p.velocity, p.age, m.0.clone()))
            .collect()
    };
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    found
        .into_iter()
        .map(|(at, v, age, m)| {
            let blend = matches!(materials.get(&m).unwrap().alpha_mode, AlphaMode::Blend);
            (at, v, age, blend)
        })
        .collect()
}

fn ribbon_alpha(app: &mut App, totem: Entity) -> f32 {
    let handle = rig_of(app, totem).unwrap().ribbon_material;
    app.world()
        .resource::<Assets<StandardMaterial>>()
        .get(&handle)
        .unwrap()
        .base_color
        .alpha()
}

fn count<T: Component>(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<Entity, With<T>>();
    q.iter(app.world()).count()
}

/// Angle about the post's axis at `centre`, in the orbit's sense (the one
/// `Quat::from_rotation_y` advances).
fn angle_about(centre: Vec3, p: Vec3) -> f32 {
    let d = p - centre;
    (-d.z).atan2(d.x)
}

// ── The post ───────────────────────────────────────────────────────────────

#[test]
fn the_post_rises_out_of_the_ground_and_stands_one_yard_tall() {
    let mut app = app();
    let at = Vec3::new(5.0, 0.0, -3.0);
    let totem = drop_totem(&mut app, TotemElement::Earth, at);
    run(&mut app, 0.2);
    let early = post_top(&mut app, totem);
    assert!(
        early.y < 0.8 * TOTEM_POST_HEIGHT,
        "0.2 s in the post is still rising, top at {early:?}"
    );
    run(&mut app, TOTEM_BIRTH_SECS);
    let top = post_top(&mut app, totem);
    assert!(
        (top.y - TOTEM_POST_HEIGHT).abs() < 1e-3,
        "the standing post's top is 1.0 yd up, got {top:?}"
    );
    assert!(
        (top.xz() - at.xz()).length() < 1e-3,
        "and it stands upright over the totem, top at {top:?}"
    );
    assert!((TOTEM_POST_HEIGHT - 1.0).abs() < 1e-6);
}

#[test]
fn the_birth_rise_is_monotonic_and_settles_upright() {
    let mut app = app();
    let totem = drop_totem(&mut app, TotemElement::Water, Vec3::ZERO);
    let mut last = f32::MIN;
    let mut max_lean = 0.0_f32;
    for _ in 0..(TOTEM_BIRTH_SECS / DT) as usize {
        app.update();
        let top = post_top(&mut app, totem);
        // (The wobble's lean costs at most 1 - cos 6° of height.)
        assert!(top.y >= last - 6e-3, "the post sank mid-rise");
        last = top.y;
        max_lean = max_lean.max(top.xz().length());
    }
    assert!(max_lean > 0.01, "the birth wobbles (max lean {max_lean})");
    run(&mut app, 0.1);
    assert!(post_top(&mut app, totem).xz().length() < 1e-3);
}

#[test]
fn the_rune_plate_faces_the_front_near_mid_height() {
    let mut app = app();
    let totem = drop_totem(&mut app, TotemElement::Fire, Vec3::ZERO);
    run(&mut app, TOTEM_BIRTH_SECS + 0.1);
    let rune = rig_of(&mut app, totem).unwrap().rune;
    let g = *app.world().get::<GlobalTransform>(rune).unwrap();
    let facing = g.rotation() * Vec3::Z;
    assert!(
        facing.dot(Vec3::Z) > 0.999,
        "the plate faces +Z: {facing:?}"
    );
    let at = g.translation();
    assert!((0.4..0.55).contains(&at.y), "plate at {at:?}");
    assert!(at.z > 0.2, "on the front face, not inside the post: {at:?}");
}

// ── The ribbons ────────────────────────────────────────────────────────────

#[test]
fn the_ribbons_orbit_the_post_once_a_second() {
    let mut app = app();
    let at = Vec3::new(-4.0, 0.0, 6.0);
    let totem = drop_totem(&mut app, TotemElement::Air, at);
    run(&mut app, TOTEM_BIRTH_SECS + 0.1);

    let step = TAU * TOTEM_RIBBON_REV_PER_SEC * DT;
    let start = ribbon_head(&mut app, totem, 0);
    let mut previous = start;
    let mut travelled = 0.0;
    for _ in 0..(1.0 / DT).round() as usize {
        app.update();
        let head = ribbon_head(&mut app, totem, 0);
        let radius = (head.xz() - at.xz()).length();
        assert!(
            (radius - TOTEM_RIBBON_ORBIT_RADIUS).abs() < 1e-3,
            "the head circles the post at {TOTEM_RIBBON_ORBIT_RADIUS} yd, got {radius}"
        );
        let height = TOTEM_POST_HEIGHT * TOTEM_RIBBON_ORBIT_HEIGHT_FRAC;
        assert!(
            (head.y - height).abs() < 1e-3,
            "at 47% height, got {}",
            head.y
        );
        let advance = (angle_about(at, head) - angle_about(at, previous)).rem_euclid(TAU);
        assert!(
            (advance - step).abs() < 1e-3,
            "one frame turns the head {step} rad about the post, got {advance}"
        );
        travelled += advance;
        previous = head;
    }
    assert!((travelled - TAU).abs() < 1e-2, "one revolution in a second");
    assert!(
        (previous - start).length() < 1e-3,
        "and back where it began"
    );
}

#[test]
fn fire_carries_three_ribbons_evenly_spaced_and_the_others_two() {
    for (element, n) in [
        (TotemElement::Air, 2),
        (TotemElement::Earth, 2),
        (TotemElement::Fire, 3),
        (TotemElement::Water, 2),
    ] {
        let mut app = app();
        let totem = drop_totem(&mut app, element, Vec3::ZERO);
        run(&mut app, TOTEM_BIRTH_SECS + 0.1);
        let heads: Vec<f32> = (0..n)
            .map(|k| angle_about(Vec3::ZERO, ribbon_head(&mut app, totem, k)))
            .collect();
        for k in 1..n {
            let gap = (heads[k] - heads[k - 1]).rem_euclid(TAU);
            assert!(
                (gap - TAU / n as f32).abs() < 1e-3,
                "{element:?}: heads {heads:?}"
            );
        }
    }
}

// ── The flame ──────────────────────────────────────────────────────────────

#[test]
fn the_flame_burns_at_the_top_and_rises() {
    let mut app = app();
    let at = Vec3::new(2.0, 0.0, 2.0);
    let _totem = drop_totem(&mut app, TotemElement::Water, at);
    run(&mut app, TOTEM_BIRTH_SECS + 0.1);
    // Count a clean two seconds of standing emission.
    let before = count::<ClientParticle>(&mut app);
    assert!(before > 0, "the flame burns through the stand");
    run(&mut app, 2.0);
    let fresh: Vec<_> = particles(&mut app)
        .into_iter()
        .filter(|(_, _, age, _)| *age <= DT + 1e-6)
        .collect();
    assert!(!fresh.is_empty(), "a flame lick is born every frame or two");
    for (p, v, _, blend) in &fresh {
        let born_at = TOTEM_POST_HEIGHT * TOTEM_FLAME_HEIGHT_FRAC;
        assert!(
            (p.y - born_at).abs() < 0.05,
            "born in the knob at {born_at} yd, got {p:?}"
        );
        assert!((p.xz() - at.xz()).length() < 0.15, "over the post: {p:?}");
        assert!(v.y > 0.5, "and rising: {v:?}");
        assert!(!blend, "the flame is additive");
    }
    // 35/s x 0.62 s life: ~22 alive at once.
    let alive = particles(&mut app).len() as f32;
    let expected = TOTEM_FLAME_RATE * 0.62;
    assert!(
        (alive - expected).abs() < 3.0,
        "{alive} alive, ~{expected} expected"
    );
}

// ── Death ──────────────────────────────────────────────────────────────────

#[test]
fn death_tilts_the_post_bursts_dust_and_smoke_and_retires_the_rig() {
    let mut app = app();
    let at = Vec3::new(1.0, 0.0, -1.0);
    let totem = drop_totem(&mut app, TotemElement::Fire, at);
    run(&mut app, TOTEM_BIRTH_SECS + 0.5);
    let post = rig_of(&mut app, totem).unwrap().post;

    // The sim despawns the totem (expiry or a recast); the rig plays on.
    app.world_mut().entity_mut(totem).despawn();
    app.update();
    assert!(
        rig_of(&mut app, totem).is_some(),
        "the death outlives the totem"
    );

    // Dust in its window: element-tinted additive motes at the base, kicked
    // outward along the ground.
    run(&mut app, 0.2);
    let dust: Vec<_> = particles(&mut app)
        .into_iter()
        .filter(|(p, v, age, blend)| !blend && p.y < 0.4 && v.xz().length() > 1.0 && *age > 0.1)
        .collect();
    assert!(
        dust.len() > 5,
        "a dust burst at the base, got {}",
        dust.len()
    );
    for (p, v, _, _) in &dust {
        let out = (p.xz() - at.xz()).normalize_or_zero();
        assert!(
            out.dot(v.xz().normalize_or_zero()) > 0.0,
            "moving away from the base"
        );
    }

    // The flame went out: nothing born near the top after the cutoff.
    let top_born = particles(&mut app)
        .into_iter()
        .filter(|(p, v, age, blend)| !blend && *age < 0.05 && p.y > 0.7 && v.xz().length() < 1.0)
        .count();
    assert_eq!(top_born, 0, "the top flame is out");

    // Smoke, alpha-blended, rising.
    run(&mut app, 0.4);
    let smoke: Vec<_> = particles(&mut app)
        .into_iter()
        .filter(|(_, _, _, blend)| *blend)
        .collect();
    assert!(
        smoke.len() > 5,
        "an alpha-blended smoke puff, got {}",
        smoke.len()
    );
    assert!(
        smoke.iter().all(|(_, v, _, _)| v.y > 0.0),
        "the smoke rises"
    );

    // The tilt: just before the rig retires, the top leans 18 degrees off
    // vertical about the base.
    run(&mut app, TOTEM_DEATH_SECS - 0.6 - DT);
    let top = post_top(&mut app, totem);
    let lean = (top.xz() - at.xz()).length().atan2(top.y);
    assert!(
        (lean.to_degrees() - TOTEM_DEATH_TILT_DEG).abs() < 1.5,
        "tilted {}°",
        lean.to_degrees()
    );

    // And then the rig is gone, parts and all.
    run(&mut app, 0.1);
    assert!(rig_of(&mut app, totem).is_none());
    assert!(
        app.world().get_entity(post).is_err(),
        "the post went with it"
    );
    assert_eq!(count::<TotemPart>(&mut app), 0);
}

#[test]
fn a_dying_totem_fades_only_its_own_ribbons() {
    let mut app = app();
    let air = drop_totem(&mut app, TotemElement::Air, Vec3::new(-2.0, 0.0, 0.0));
    let fire = drop_totem(&mut app, TotemElement::Fire, Vec3::new(2.0, 0.0, 0.0));
    run(&mut app, TOTEM_BIRTH_SECS + 0.1);
    app.world_mut().entity_mut(fire).despawn();
    run(&mut app, 0.7);
    assert!(
        ribbon_alpha(&mut app, fire) < 1e-3,
        "the dying ribbons faded out"
    );
    assert!(
        (ribbon_alpha(&mut app, air) - TOTEM_RIBBON_ALPHA).abs() < 1e-6,
        "the standing totem's did not"
    );
}

// ── The gameplay totem ─────────────────────────────────────────────────────

#[test]
fn the_gameplay_totem_is_never_touched() {
    let mut app = app();
    let at = Vec3::new(3.0, 0.0, 4.0);
    let totem = drop_totem(&mut app, TotemElement::Earth, at);
    for _ in 0..(3.0 / DT) as usize {
        app.update();
        let t = app.world().get::<Transform>(totem).unwrap();
        assert_eq!(*t, Transform::from_translation(at));
        let totem = app.world().get::<Totem>(totem).unwrap();
        assert_eq!(totem.duration_remaining, 30.0);
        assert_eq!(totem.radius, 20.0);
    }
    assert_eq!(count::<TotemRig>(&mut app), 1, "one rig per totem");
}
