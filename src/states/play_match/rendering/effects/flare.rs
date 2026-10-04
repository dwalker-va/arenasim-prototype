use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

use super::hunter_shots::{draw, ClientEmitter, EmitterAssets, EmitterSprite};
use super::spell_bolts::{soft_dot_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Flare (Hunter) — a burning flare on the ground, lighting a pool round it
// ==============================================================================
//
// From the Classic Era client data (build 1.15.9.69547). Flare is spell 1543,
// visual 112198; its persistent area model is `flare_state_base.m2` (166196),
// whose one sequence runs 30 000 ms — exactly the Flare's duration. Every track
// below is keyed to that clock, so the visual is a function of the gameplay
// Flare's own burn (`FlareZone::duration_remaining`), not of a timer:
//
// - **The pool.** Two flat 13.9 yd additive glow quads in orange
//   `(0.992, 0.467, 0.0)`. Their texture weights ARE the pulse: quad A swings
//   0.05 <-> 0.10 every 100 ms for the whole life (a 5 Hz shimmer); quad B eases
//   0.10 -> 0.05 over the first 15.167 s, then swings 0.05 <-> 0.10 every 167 ms
//   (3 Hz), so the two beat against each other and a dying flare sputters
//   harder. Both fall to zero over the last half second. The two quads share
//   one size and one orange, so one quad carrying their summed weight draws
//   them both.
// - **The flare itself lies on the ground** (0.2 yd up) — `flare.blp` sprites,
//   0.667 yd, 10/s living 0.4 s, white-gold -> orange -> gold, twinkling at
//   30 Hz with their size jumping between 0.4x and 1.1x. That twinkle is the
//   flicker. The rate steps to 20/s at 29.0 s and 5/s at 29.5 s: a last
//   flare-up, then out. Drawn here as four camera-facing sprites cycling that
//   ramp a quarter-life apart — the client's four live particles — whose
//   twinkle is a deterministic hash held for one 1/30 s step.
// - **Sparks.** 25/s at the start falling linearly to 0 at 29 s, launched into
//   the upper hemisphere at 1.67 yd/s under 1.39 yd/s² gravity, 1 s life,
//   red -> orange -> pale yellow. The flare spits less as it burns down.
// - **Burn-out smoke** (20/s, 29.33-29.97 s, 0.11 yd dark additive puffs) is
//   not drawn: it is invisible at the match camera.
//
// Two layers are ours, not the client's. The RIM is a thin ring at the
// gameplay radius: the client's soft pool fades out well inside 10 yd, and a
// Rogue that walks past the rim unrevealed must read as a missed guess, not a
// bug. The HALO is a soft glow behind the flare so the flare reads at the
// match camera's 60 yd.
//
// The bench's knobs (AS-222 Flare Bench, docs/design/benches/
// 2026-10-04-flare-bench.html) sit on top of the transcription as named
// constants; the bench ports `flare_clock`, `flare_pool_weight`,
// `flare_core_rate` and `flare_twinkle` verbatim.
//
// **The rig is detached from the gameplay Flare.** `FlareZone` is a sim entity
// (`flare_system` reads it every tick), so nothing here inserts on it: the rig
// is its own entity, reads the zone's burn each frame, and goes the frame the
// zone does — on expiry, at match end, or when the sandbox clears it.
//
// Graphical-only: registered in `states/mod.rs` only, scattered by a
// deterministic hash (never `game_rng`), writing no sim state — headless stays
// byte-identical.

// ── The bench's knobs (AS-222 Flare Bench) ─────────────────────────────────

/// Pool alpha per unit of the client's summed texture weight (0.10-0.20).
pub const FLARE_POOL_GAIN: f32 = 4.0;
/// Pool half-width as a fraction of the gameplay radius.
pub const FLARE_POOL_RADIUS_SCALE: f32 = 1.0;
/// Pool texture alpha is `(1 - r²)^FALLOFF` across its radius.
pub const FLARE_POOL_FALLOFF: f32 = 1.6;
/// The rim at the gameplay radius.
pub const FLARE_RIM_ALPHA: f32 = 0.22;
/// The burning flare's sprite diameter, yards (client 0.667).
pub const FLARE_CORE_DIAMETER: f32 = 1.6;
/// Height of the flare's sprites above the floor, yards (client 0.2).
pub const FLARE_CORE_HEIGHT: f32 = 0.45;
/// The core's twinkle: a new size every 1/HZ seconds, between MIN and MAX of
/// its diameter (client 30 Hz, 0.4-1.1).
pub const FLARE_TWINKLE_HZ: f32 = 30.0;
pub const FLARE_TWINKLE_MIN: f32 = 0.4;
pub const FLARE_TWINKLE_MAX: f32 = 1.1;
/// The halo behind the flare (ours).
pub const FLARE_HALO_DIAMETER: f32 = 3.0;
pub const FLARE_HALO_ALPHA: f32 = 0.45;
/// Multipliers over the client's spark rate and size.
pub const FLARE_SPARK_RATE: f32 = 1.0;
pub const FLARE_SPARK_SIZE: f32 = 2.0;

// ── The transcription ──────────────────────────────────────────────────────

/// Length of `flare_state_base.m2`'s sequence, seconds.
pub const FLARE_CLIENT_SEQ_SECS: f32 = 30.0;
/// The pool's orange (the model's colour tracks).
const FLARE_RGB: (f32, f32, f32) = (0.992, 0.467, 0.0);
/// Overlapping core sprites, and the life each cycles through (10/s x 0.4 s).
pub const FLARE_CORE_SPRITES: usize = 4;
const CORE_LIFE: f32 = 0.4;
const CORE_COLOR: [[f32; 3]; 3] = [
    [255.0, 255.0, 192.0],
    [255.0, 95.0, 20.0],
    [253.0, 220.0, 40.0],
];
const CORE_ALPHA: [f32; 3] = [1.0, 1.0, 0.0];
const CORE_MID: f32 = 0.75;
/// How far a core sprite is scattered from the centre each cycle, yards.
const CORE_SCATTER: f32 = 0.12;
/// The core's spin, rad/s (client 0.5).
const CORE_SPIN: f32 = 0.5;
/// During the last flare-up (rate x2) the sprites also burn a little larger.
const CORE_FLAREUP_SIZE: f32 = 1.25;
/// The halo's tint, a warmer gold than the pool.
const HALO_RGB: (f32, f32, f32) = (1.0, 0.667, 0.275);

/// Emissive gains — the repo's 2-4x glow convention. `unlit` stays false on
/// every material here: Bevy's unlit branch discards emissive outright.
const POOL_GLOW: f32 = 1.5;
const RIM_GLOW: f32 = 1.5;
const CORE_GLOW: f32 = 4.0;
const HALO_GLOW: f32 = 2.5;

/// The pool and rim sit a hair above the floor so they never z-fight it.
const POOL_LIFT: f32 = 0.03;
const RIM_LIFT: f32 = 0.035;
const RIM_WIDTH: f32 = 0.15;
/// Where the sparks are born, above the flare's foot (client 0.08).
const SPARK_HEIGHT: f32 = 0.08;
/// Generated pool texture, pixels on a side.
const POOL_PX: u32 = 128;

/// The client's spark rate: 25/s at the start, linearly to 0 at 29 s.
const SPARK_TRACK: &[(f32, f32)] = &[(0.0, 25.0), (29.0, 0.0)];

/// `flare_state_base.m2` emitter 1, the sparks.
pub const FLARE_SPARKS: ClientEmitter = ClientEmitter {
    name: "flare_state_base/p1 sparks",
    sprite: EmitterSprite::Flare,
    additive: true,
    rate: 0.0,
    window: ClientEmitter::tracked(SPARK_TRACK),
    life: 1.0,
    speed: 1.667,
    gravity: 1.389,
    area: 0.014,
    column: None,
    rise: false,
    color: [[250, 5, 0], [255, 120, 0], [255, 252, 176]],
    alpha: [1.0, 1.0, 1.0],
    // Sized up for the match camera; its throw and gravity stay the client's.
    size: [0.139 * FLARE_SPARK_SIZE, 0.028 * FLARE_SPARK_SIZE, 0.0],
    mid: 0.8,
    track: Some(SPARK_TRACK),
};

// ── The tracks, as pure functions of the clocks ────────────────────────────

/// Seconds into the client sequence for a Flare with `remaining` of `total`
/// seconds left to burn.
pub fn flare_clock(remaining: f32, total: f32) -> f32 {
    if total <= 0.0 {
        return FLARE_CLIENT_SEQ_SECS;
    }
    (1.0 - remaining / total).clamp(0.0, 1.0) * FLARE_CLIENT_SEQ_SECS
}

/// 0 at a whole number, 1 halfway between.
fn tri(x: f32) -> f32 {
    1.0 - (2.0 * x.rem_euclid(1.0) - 1.0).abs()
}

/// Piecewise-linear lookup over `(t, v)` keys, held at both ends.
fn keyed(keys: &[(f32, f32)], t: f32) -> f32 {
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for pair in keys.windows(2) {
        let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
        if t <= t1 {
            return v0 + (v1 - v0) * (t - t0) / (t1 - t0);
        }
    }
    keys[keys.len() - 1].1
}

/// The pool's two texture weights, summed, `clock` seconds into the client
/// sequence and `age` seconds into the visual's life. The life envelope reads
/// `clock`; the pulses run on `age`, so they keep moving while a decided
/// match holds the Flare's burn still.
pub fn flare_pool_weight(clock: f32, age: f32) -> f32 {
    let a = if clock >= 29.5 {
        keyed(&[(29.5, 0.10), (29.6, 0.03), (30.0, 0.0)], clock)
    } else {
        0.05 + 0.05 * tri(age / 0.2)
    };
    let b = if clock >= 29.5 {
        keyed(&[(29.5, 0.05), (29.667, 0.07), (30.0, 0.0)], clock)
    } else if clock < 15.167 {
        0.10 - 0.05 * (clock / 15.167)
    } else {
        0.05 + 0.05 * tri(age * 3.0)
    };
    a + b
}

/// The core emitter's stepped rate track, relative to its steady 10/s.
pub fn flare_core_rate(clock: f32) -> f32 {
    if clock < 29.0 {
        1.0
    } else if clock < 29.5 {
        2.0
    } else if clock < FLARE_CLIENT_SEQ_SECS {
        0.5
    } else {
        0.0
    }
}

/// A sprite's size factor `age` seconds in: a deterministic draw held for one
/// `1 / FLARE_TWINKLE_HZ` step, between the twinkle's min and max.
pub fn flare_twinkle(seed: u32, age: f32) -> f32 {
    let step = (age.max(0.0) * FLARE_TWINKLE_HZ) as u32;
    FLARE_TWINKLE_MIN + (FLARE_TWINKLE_MAX - FLARE_TWINKLE_MIN) * draw(seed, step)
}

/// The pool and rim fade over the client's last half second.
fn end_fade(clock: f32) -> f32 {
    ((FLARE_CLIENT_SEQ_SECS - clock) / 0.5).clamp(0.0, 1.0)
}

fn ramp3(keys: [f32; 3], mid: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < mid {
        keys[0] + (keys[1] - keys[0]) * (t / mid)
    } else {
        keys[1] + (keys[2] - keys[1]) * ((t - mid) / (1.0 - mid))
    }
}

/// One core sprite's colour and alpha, `phase` (0..1) through its life.
fn core_ramp(phase: f32) -> (Color, f32) {
    let channel = |c: usize| {
        ramp3(
            [CORE_COLOR[0][c], CORE_COLOR[1][c], CORE_COLOR[2][c]],
            CORE_MID,
            phase,
        ) / 255.0
    };
    (
        Color::srgb(channel(0), channel(1), channel(2)),
        ramp3(CORE_ALPHA, CORE_MID, phase),
    )
}

fn glow(color: Color, gain: f32) -> LinearRgba {
    let c = color.to_linear();
    LinearRgba::rgb(c.red * gain, c.green * gain, c.blue * gain)
}

/// The pool's soft disc: white, `(1 - r²)^FLARE_POOL_FALLOFF` in alpha.
fn flare_pool_texture() -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = POOL_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r2 = dx * dx + dy * dy;
            let a = (1.0 - r2).max(0.0).powf(FLARE_POOL_FALLOFF);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

// ── Components ─────────────────────────────────────────────────────────────

/// The drawn Flare: a top-level entity beside its gameplay [`FlareZone`],
/// never on it. It lives exactly as long as the zone.
#[derive(Component)]
pub struct FlareRig {
    /// The gameplay Flare this rig draws. Once it is gone the rig goes.
    pub flare: Entity,
    /// The Flare's whole burn, read off the zone the frame it was lit.
    pub total: f32,
    /// Seconds since the rig was built (the pulses' clock).
    pub age: f32,
    /// The client clock last frame, so sparks are integrated over the frame.
    pub clock: f32,
    pub pool: Entity,
    pub rim: Entity,
    pub halo: Entity,
    pub cores: [Entity; FLARE_CORE_SPRITES],
    /// This rig's own materials: each is rewritten every frame.
    pub pool_material: Handle<StandardMaterial>,
    pub rim_material: Handle<StandardMaterial>,
    pub halo_material: Handle<StandardMaterial>,
    pub core_materials: [Handle<StandardMaterial>; FLARE_CORE_SPRITES],
    /// Deterministic particle seed (the zone's entity index).
    pub seed: u32,
    /// Sparks emitted so far, the running part of every spark seed.
    pub emitted: u32,
    /// Fractional sparks owed.
    pub carry: f32,
    pub quad: Handle<Mesh>,
    pub spark_palette: Arc<[Handle<StandardMaterial>]>,
}

/// A child of a [`FlareRig`]: which piece it is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlarePart {
    /// The lit pool on the floor.
    Pool,
    /// The ring at the gameplay radius.
    Rim,
    /// The soft glow behind the burning flare.
    Halo,
    /// One of the burning flare's twinkling sprites.
    Core(usize),
}

/// Meshes, textures and the spark palette every Flare shares.
pub struct FlareAssets {
    quad: Handle<Mesh>,
    pool_texture: Handle<Image>,
    dot: Handle<Image>,
    star: Handle<Image>,
    spark_palette: Arc<[Handle<StandardMaterial>]>,
}

impl FlareAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        let mut emitters = EmitterAssets::build(meshes, images);
        Self {
            quad: emitters.quad(),
            pool_texture: images.add(flare_pool_texture()),
            dot: images.add(soft_dot_texture()),
            star: images.add(star_flash_texture()),
            spark_palette: emitters.palette(materials, &FLARE_SPARKS),
        }
    }
}

/// An additive textured material; `unlit` stays false so the emissive draws.
fn additive(texture: &Handle<Image>, color: Color, alpha: f32, gain: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color.with_alpha(alpha),
        base_color_texture: Some(texture.clone()),
        emissive: glow(color, gain),
        emissive_texture: Some(texture.clone()),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

// ── Systems ────────────────────────────────────────────────────────────────

/// Build the rig for a newly lit Flare. Registered ONLY in
/// `StatesPlugin::build`.
pub fn spawn_flare_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<FlareAssets>>,
    new_flares: Query<(Entity, &FlareZone, &Transform), Added<FlareZone>>,
) {
    if new_flares.is_empty() {
        return;
    }
    let assets =
        assets.get_or_insert_with(|| FlareAssets::build(&mut meshes, &mut materials, &mut images));
    let (r, g, b) = FLARE_RGB;
    let orange = Color::srgb(r, g, b);
    let (hr, hg, hb) = HALO_RGB;
    let gold = Color::srgb(hr, hg, hb);
    for (flare_entity, flare, transform) in new_flares.iter() {
        let pool_material = materials.add(additive(&assets.pool_texture, orange, 0.0, POOL_GLOW));
        let pool_size = 2.0 * flare.radius * FLARE_POOL_RADIUS_SCALE;
        let pool = commands
            .spawn((
                FlarePart::Pool,
                Mesh3d(meshes.add(Plane3d::default().mesh().size(pool_size, pool_size))),
                MeshMaterial3d(pool_material.clone()),
                Transform::from_xyz(0.0, POOL_LIFT, 0.0),
                NotShadowCaster,
            ))
            .id();

        let rim_material = materials.add(StandardMaterial {
            base_color: orange.with_alpha(0.0),
            emissive: glow(orange, RIM_GLOW),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        let rim = commands
            .spawn((
                FlarePart::Rim,
                // `Annulus` lies in XY facing +Z; a quarter turn back lays it
                // on the floor facing up.
                Mesh3d(meshes.add(Annulus::new(flare.radius - RIM_WIDTH, flare.radius))),
                MeshMaterial3d(rim_material.clone()),
                Transform::from_xyz(0.0, RIM_LIFT, 0.0)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                NotShadowCaster,
            ))
            .id();

        let halo_material = materials.add(additive(&assets.dot, gold, 0.0, HALO_GLOW));
        let halo = commands
            .spawn((
                FlarePart::Halo,
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(halo_material.clone()),
                Transform::from_xyz(0.0, FLARE_CORE_HEIGHT, 0.0)
                    .with_scale(Vec3::splat(FLARE_HALO_DIAMETER)),
                NotShadowCaster,
            ))
            .id();

        let core_materials: [Handle<StandardMaterial>; FLARE_CORE_SPRITES] =
            std::array::from_fn(|_| {
                materials.add(additive(&assets.star, Color::WHITE, 0.0, CORE_GLOW))
            });
        let cores: [Entity; FLARE_CORE_SPRITES] = std::array::from_fn(|k| {
            commands
                .spawn((
                    FlarePart::Core(k),
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(core_materials[k].clone()),
                    Transform::from_xyz(0.0, FLARE_CORE_HEIGHT, 0.0).with_scale(Vec3::splat(1e-4)),
                    NotShadowCaster,
                ))
                .id()
        });

        let mut children = vec![pool, rim, halo];
        children.extend_from_slice(&cores);
        commands
            .spawn((
                FlareRig {
                    flare: flare_entity,
                    total: flare.duration_remaining,
                    age: 0.0,
                    clock: 0.0,
                    pool,
                    rim,
                    halo,
                    cores,
                    pool_material,
                    rim_material,
                    halo_material,
                    core_materials,
                    seed: flare_entity.index(),
                    emitted: 0,
                    carry: 0.0,
                    quad: assets.quad.clone(),
                    spark_palette: assets.spark_palette.clone(),
                },
                Transform::from_translation(Vec3::new(
                    transform.translation.x,
                    0.0,
                    transform.translation.z,
                )),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .add_children(&children);
    }
}

/// Pulse the pool, twinkle the flare, spit its sparks — all read off the
/// gameplay Flare's burn — and take the rig down the frame the Flare is gone.
#[allow(clippy::type_complexity)]
pub fn update_flare_visuals(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    flares: Query<&FlareZone>,
    camera: Query<&Transform, (With<Camera3d>, Without<FlareRig>, Without<FlarePart>)>,
    mut rigs: Query<(Entity, &mut FlareRig, &Transform), Without<FlarePart>>,
    mut parts: Query<&mut Transform, (With<FlarePart>, Without<FlareRig>, Without<Camera3d>)>,
) {
    let dt = time.delta_secs();
    let facing = camera.iter().next().map(|c| c.rotation).unwrap_or_default();
    let (r, g, b) = FLARE_RGB;
    let orange = Color::srgb(r, g, b);
    for (rig_entity, mut rig, rig_transform) in rigs.iter_mut() {
        let Ok(flare) = flares.get(rig.flare) else {
            commands.entity(rig_entity).try_despawn();
            continue;
        };
        rig.age += dt;
        let age = rig.age;
        let previous = rig.clock;
        let clock = flare_clock(flare.duration_remaining, rig.total);
        rig.clock = clock;
        let fade = end_fade(clock);

        // The pool: the client's two pulsing weights, summed.
        if let Some(m) = materials.get_mut(&rig.pool_material) {
            let alpha = (flare_pool_weight(clock, age) * FLARE_POOL_GAIN).clamp(0.0, 1.0);
            m.base_color = orange.with_alpha(alpha);
        }
        if let Some(m) = materials.get_mut(&rig.rim_material) {
            m.base_color = orange.with_alpha(FLARE_RIM_ALPHA * fade);
        }

        // The burning flare.
        let rate = flare_core_rate(clock);
        let span = (FLARE_TWINKLE_MAX - FLARE_TWINKLE_MIN).max(1e-3);
        let flick = 0.75
            + 0.25 * (flare_twinkle(rig.seed.wrapping_add(99), age) - FLARE_TWINKLE_MIN) / span;
        if let Some(m) = materials.get_mut(&rig.halo_material) {
            m.base_color
                .set_alpha((FLARE_HALO_ALPHA * rate.min(1.5) * flick).clamp(0.0, 1.0));
        }
        if let Ok(mut halo) = parts.get_mut(rig.halo) {
            halo.rotation = facing;
        }
        for k in 0..FLARE_CORE_SPRITES {
            let cycle = age / CORE_LIFE + k as f32 / FLARE_CORE_SPRITES as f32;
            let n = cycle.floor();
            let phase = cycle - n;
            let s = rig
                .seed
                .wrapping_mul(31)
                .wrapping_add((k as u32).wrapping_mul(7919))
                .wrapping_add(n as u32);
            let (color, alpha) = core_ramp(phase);
            if let Some(m) = materials.get_mut(&rig.core_materials[k]) {
                m.base_color = color.with_alpha(alpha * rate.min(1.0));
                m.emissive = glow(color, CORE_GLOW);
            }
            let flareup = if rate > 1.0 { CORE_FLAREUP_SIZE } else { 1.0 };
            let size = FLARE_CORE_DIAMETER
                * flare_twinkle(rig.seed.wrapping_mul(131).wrapping_add(k as u32), age)
                * flareup;
            if let Ok(mut core) = parts.get_mut(rig.cores[k]) {
                core.translation = Vec3::new(
                    (draw(s, 1) - 0.5) * CORE_SCATTER,
                    FLARE_CORE_HEIGHT + (draw(s, 2) - 0.5) * CORE_SCATTER,
                    (draw(s, 3) - 0.5) * CORE_SCATTER,
                );
                core.rotation = facing * Quat::from_rotation_z(draw(s, 4) * TAU + age * CORE_SPIN);
                core.scale = Vec3::splat(if rate > 0.0 { size.max(1e-4) } else { 1e-4 });
            }
        }

        // Sparks, integrated over this frame's slice of the client clock.
        rig.carry += FLARE_SPARKS.count_between(previous, clock) * FLARE_SPARK_RATE;
        let foot = rig_transform.translation + Vec3::Y * SPARK_HEIGHT;
        while rig.carry >= 1.0 {
            rig.carry -= 1.0;
            let seed = rig
                .seed
                .wrapping_mul(2_654_435_761)
                .wrapping_add(rig.emitted);
            rig.emitted = rig.emitted.wrapping_add(1);
            let velocity = spark_launch(seed);
            let e = &FLARE_SPARKS;
            commands.spawn((
                ClientParticle {
                    age: 0.0,
                    life: e.life,
                    velocity,
                    gravity: e.gravity,
                    size: e.size,
                    mid: e.mid,
                    palette: rig.spark_palette.clone(),
                    step: 0,
                    facing: ParticleFacing::Camera,
                    owned: false,
                },
                Mesh3d(rig.quad.clone()),
                MeshMaterial3d(rig.spark_palette[0].clone()),
                Transform::from_translation(foot)
                    .with_rotation(facing)
                    .with_scale(Vec3::splat(e.size[0].max(1e-4))),
                NotShadowCaster,
                PlayMatchEntity,
            ));
        }
    }
}

/// A spark's launch: anywhere in the upper hemisphere (client vRange pi/2,
/// hRange 2pi), at the client's 1.667 yd/s +- 0.1.
fn spark_launch(seed: u32) -> Vec3 {
    let polar = draw(seed, 1) * FRAC_PI_2;
    let azimuth = draw(seed, 2) * TAU;
    let speed = FLARE_SPARKS.speed + (draw(seed, 3) - 0.5) * 0.2;
    Vec3::new(
        polar.sin() * azimuth.cos(),
        polar.cos(),
        polar.sin() * azimuth.sin(),
    ) * speed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_runs_the_client_sequence_over_the_flares_burn() {
        assert_eq!(flare_clock(30.0, 30.0), 0.0);
        assert!((flare_clock(15.0, 30.0) - 15.0).abs() < 1e-5);
        assert_eq!(flare_clock(0.0, 30.0), FLARE_CLIENT_SEQ_SECS);
        // A different burn is stretched onto the same sequence.
        assert!((flare_clock(10.0, 20.0) - 15.0).abs() < 1e-5);
    }

    /// The summed weight hits the client's keys: quad A 0.05 / 0.10 on the
    /// 100 ms grid, quad B 0.10 at the start, 0.05 at 15.167 s.
    #[test]
    fn the_pool_weight_lands_on_the_client_keys() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(flare_pool_weight(0.0, 0.0), 0.15));
        assert!(close(flare_pool_weight(0.0, 0.1), 0.20));
        assert!(close(flare_pool_weight(15.167, 0.0), 0.10));
        assert!(close(flare_pool_weight(29.5, 0.0), 0.15));
        assert!(close(flare_pool_weight(30.0, 0.0), 0.0));
        // Every weight in the burn is inside the two quads' joint range.
        for i in 0..3000 {
            let t = i as f32 / 100.0;
            let w = flare_pool_weight(t, t);
            assert!((0.0..=0.2001).contains(&w), "{t}: {w}");
        }
    }

    /// The pool pulses all its life: within any 0.2 s it swings by at least the
    /// client's 0.05.
    #[test]
    fn the_pool_pulses_throughout() {
        for start in [0.0f32, 5.0, 14.0, 16.0, 25.0, 29.0] {
            let samples: Vec<f32> = (0..=20)
                .map(|i| {
                    let t = start + i as f32 * 0.01;
                    flare_pool_weight(t, t)
                })
                .collect();
            let (lo, hi) = samples
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), &w| (lo.min(w), hi.max(w)));
            assert!(
                hi - lo >= 0.049,
                "at {start}s the pool swings only {}",
                hi - lo
            );
        }
    }

    #[test]
    fn the_core_flares_up_then_goes_out() {
        assert_eq!(flare_core_rate(0.0), 1.0);
        assert_eq!(flare_core_rate(28.99), 1.0);
        assert_eq!(flare_core_rate(29.2), 2.0);
        assert_eq!(flare_core_rate(29.7), 0.5);
        assert_eq!(flare_core_rate(30.0), 0.0);
    }

    #[test]
    fn the_twinkle_holds_a_step_and_stays_in_range() {
        let step = 1.0 / FLARE_TWINKLE_HZ;
        let mut distinct = 0;
        for i in 0..300u32 {
            let t = i as f32 * step + step * 0.5;
            let v = flare_twinkle(7, t);
            assert!((FLARE_TWINKLE_MIN..=FLARE_TWINKLE_MAX).contains(&v));
            // Held across its step.
            assert_eq!(v, flare_twinkle(7, t + step * 0.3));
            if (v - flare_twinkle(7, t + step)).abs() > 0.05 {
                distinct += 1;
            }
        }
        assert!(distinct > 200, "the twinkle barely moves: {distinct}/300");
    }

    #[test]
    fn sparks_follow_the_client_track() {
        let total = FLARE_SPARKS.count_between(0.0, FLARE_CLIENT_SEQ_SECS);
        // The integral of 25 -> 0 over 29 s.
        assert!((total - 25.0 * 29.0 / 2.0).abs() < 1e-2, "{total}");
        assert!(FLARE_SPARKS.count_between(29.0, 30.0).abs() < 1e-6);
        assert!(FLARE_SPARKS.count_between(0.0, 1.0) > FLARE_SPARKS.count_between(20.0, 21.0));
    }
}
