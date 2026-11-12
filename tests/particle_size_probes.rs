//! A client particle is drawn at the size its source keyed, in the unit the
//! source keyed it in (AS-452).
//!
//! Benches key a particle's size two ways: as the sprite's RADIUS (the M2
//! scale, as the Druid and Baseline Kit benches draw it) or as its full width
//! (the Hunter Shot and Interrupt Landing benches). A particle is a unit quad,
//! so its world scale is its DIAMETER. Two cards transcribed radius keys into a
//! diameter field and drew every particle at half the bench's size; a size is
//! now built as `ParticleSize::radii` or `ParticleSize::diameters`, and these
//! probes play one particle of each through the real spawn and animation
//! systems and measure it in world space.

use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{ClientParticle, ParticleFacing, ParticleSize};
use arenasim::states::play_match::{
    animate_client_particles, spawn_client_particle, ClientEmitter, EmitterSprite,
};

const TICK: Duration = Duration::from_millis(16);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::transform::TransformPlugin,
    ));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app.add_systems(Update, animate_client_particles);
    app
}

/// A still particle of `size`, living long enough to be measured mid-life.
fn emitter(name: &'static str, size: ParticleSize) -> ClientEmitter {
    ClientEmitter {
        name,
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 1.0,
        window: None,
        life: 2.0,
        speed: 0.0,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[255, 255, 255]; 3],
        alpha: [1.0; 3],
        size,
        mid: 0.5,
        track: None,
    }
}

/// Spawns one particle of `e` through the production spawner.
fn spawn(app: &mut App, e: &ClientEmitter) -> Entity {
    let world = app.world_mut();
    let quad = world
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1.0, 1.0));
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let palette: Arc<[Handle<StandardMaterial>]> = Arc::from(vec![material; 8]);
    let mut commands = world.commands();
    let id = spawn_client_particle(&mut commands, &quad, e, &palette, Vec3::ZERO, 7, false);
    world.flush();
    id
}

/// A linear ramp over birth, midlife and death keys, at `t` of the life.
fn ramp3(keys: [f32; 3], t: f32) -> f32 {
    if t < 0.5 {
        keys[0] + (keys[1] - keys[0]) * (t / 0.5)
    } else {
        keys[1] + (keys[2] - keys[1]) * ((t - 0.5) / 0.5)
    }
}

/// The particle's world-space width after `frames` frames of animation, and
/// how far through its life it is.
fn world_diameter(app: &mut App, particle: Entity, frames: usize) -> (f32, f32) {
    for _ in 0..frames {
        app.update();
    }
    let world = app.world();
    let p = world.get::<ClientParticle>(particle).expect("still alive");
    assert!(
        p.age > 0.0 && p.age < p.life,
        "measured mid-life: {}",
        p.age
    );
    let width = world
        .get::<GlobalTransform>(particle)
        .expect("placed")
        .compute_transform()
        .scale
        .x;
    (width, p.age / p.life)
}

/// The scale a particle has on the frame it is spawned, before anything has
/// animated it.
fn born_width(app: &App, particle: Entity) -> f32 {
    app.world().get::<Transform>(particle).unwrap().scale.x
}

/// Birth, midlife and death keys that all differ, so a birth or an animated
/// width read from the wrong key shows.
const KEYS: [f32; 3] = [0.3, 0.5, 0.1];

/// A bench that keys a RADIUS: the particle is drawn twice as wide as the key,
/// on the frame it is born and once the animation owns it.
#[test]
fn a_radius_key_is_drawn_at_twice_the_key() {
    let mut app = app();
    let e = emitter("probe/radius", ParticleSize::radii(KEYS));
    let particle = spawn(&mut app, &e);
    let born = born_width(&app, particle);
    assert!((born - 2.0 * KEYS[0]).abs() < 1e-6, "born at {born}");
    for frames in [10, 50, 40] {
        let (width, t) = world_diameter(&mut app, particle, frames);
        let want = 2.0 * ramp3(KEYS, t);
        assert!((width - want).abs() < 1e-5, "{width} at {t:.2}, not {want}");
    }
}

/// The control: a bench that keys the full width is drawn at the key, so the
/// pair above is not passing on a scale that doubles everything.
#[test]
fn a_diameter_key_is_drawn_at_the_key() {
    let mut app = app();
    let e = emitter("probe/diameter", ParticleSize::diameters(KEYS));
    let particle = spawn(&mut app, &e);
    let born = born_width(&app, particle);
    assert!((born - KEYS[0]).abs() < 1e-6, "born at {born}");
    for frames in [10, 50, 40] {
        let (width, t) = world_diameter(&mut app, particle, frames);
        let want = ramp3(KEYS, t);
        assert!((width - want).abs() < 1e-5, "{width} at {t:.2}, not {want}");
    }
}

/// Every spawner places a particle without a scale and relies on the particle
/// itself to take its birth size the moment it is spawned. A particle spawned
/// at any other scale is born at its first key.
#[test]
fn a_particle_spawned_at_any_scale_is_born_at_its_first_key() {
    let mut app = app();
    let size = ParticleSize::radii(KEYS);
    let particle = app
        .world_mut()
        .spawn((
            ClientParticle {
                age: 0.0,
                life: 2.0,
                velocity: Vec3::ZERO,
                gravity: 0.0,
                size,
                mid: 0.5,
                palette: Arc::from(Vec::<Handle<StandardMaterial>>::new()),
                step: 0,
                facing: ParticleFacing::Camera,
                owned: false,
            },
            Transform::from_scale(Vec3::splat(3.0)),
        ))
        .id();
    let born = born_width(&app, particle);
    assert!((born - 2.0 * KEYS[0]).abs() < 1e-6, "born at {born}");
}

/// A model scale (`ClientEmitter::scaled`) multiplies the drawn width and keeps
/// the unit the keys were written in.
#[test]
fn a_scaled_emitter_keeps_its_unit() {
    const RADIUS: f32 = 0.25;
    let mut app = app();
    let e = emitter("probe/scaled", ParticleSize::radii([RADIUS; 3])).scaled(0.8, 1.0, 1.0);
    let particle = spawn(&mut app, &e);
    let (width, _) = world_diameter(&mut app, particle, 30);
    assert!(
        (width - 2.0 * RADIUS * 0.8).abs() < 1e-6,
        "drawn at {width}"
    );
}

/// The ramp is read in the same unit: a radius ramp is drawn at twice its keys
/// at birth, midlife and death.
#[test]
fn the_ramp_keys_share_the_unit() {
    let size = ParticleSize::radii([0.1, 0.4, 0.05]);
    assert_eq!(size.diameter_keys(), [0.2, 0.8, 0.1]);
    assert_eq!(
        ParticleSize::diameters([0.1, 0.4, 0.05]).diameter_keys(),
        [0.1, 0.4, 0.05]
    );
    assert_eq!(size.scaled(2.0).diameter_keys(), [0.4, 1.6, 0.2]);
}
