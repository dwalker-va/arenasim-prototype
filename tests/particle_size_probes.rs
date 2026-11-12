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

use arenasim::states::play_match::components::{ClientParticle, ParticleSize};
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

/// The particle's world-space width after `frames` frames of animation.
fn world_diameter(app: &mut App, particle: Entity, frames: usize) -> f32 {
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
    world
        .get::<GlobalTransform>(particle)
        .expect("placed")
        .compute_transform()
        .scale
        .x
}

/// A bench that keys a RADIUS: the particle is drawn twice as wide as the key,
/// at birth (the spawner's own scale) and once the animation owns it.
#[test]
fn a_radius_key_is_drawn_at_twice_the_key() {
    const RADIUS: f32 = 0.3;
    let mut app = app();
    let e = emitter("probe/radius", ParticleSize::radii([RADIUS; 3]));
    let particle = spawn(&mut app, &e);
    let born = app.world().get::<Transform>(particle).unwrap().scale.x;
    assert!((born - 2.0 * RADIUS).abs() < 1e-6, "born at {born}");
    let width = world_diameter(&mut app, particle, 30);
    assert!((width - 2.0 * RADIUS).abs() < 1e-6, "drawn at {width}");
}

/// The control: a bench that keys the full width is drawn at the key, so the
/// pair above is not passing on a scale that doubles everything.
#[test]
fn a_diameter_key_is_drawn_at_the_key() {
    const DIAMETER: f32 = 0.3;
    let mut app = app();
    let e = emitter("probe/diameter", ParticleSize::diameters([DIAMETER; 3]));
    let particle = spawn(&mut app, &e);
    let born = app.world().get::<Transform>(particle).unwrap().scale.x;
    assert!((born - DIAMETER).abs() < 1e-6, "born at {born}");
    let width = world_diameter(&mut app, particle, 30);
    assert!((width - DIAMETER).abs() < 1e-6, "drawn at {width}");
}

/// A model scale (`ClientEmitter::scaled`) multiplies the drawn width and keeps
/// the unit the keys were written in.
#[test]
fn a_scaled_emitter_keeps_its_unit() {
    const RADIUS: f32 = 0.25;
    let mut app = app();
    let e = emitter("probe/scaled", ParticleSize::radii([RADIUS; 3])).scaled(0.8, 1.0, 1.0);
    let particle = spawn(&mut app, &e);
    let width = world_diameter(&mut app, particle, 30);
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
