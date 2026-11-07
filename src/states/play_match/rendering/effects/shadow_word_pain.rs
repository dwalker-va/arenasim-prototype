use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;

use super::druid_heals::{body_point, drag_factor, keyed, keyed_color, window_on};
use super::hunter_shots::draw;
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use super::spell_bolts::{soft_band_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Shadow Word: Pain — the client's dark, violet-edged hit on the victim's chest
// ==============================================================================
//
// From the Classic Era client data (build 1.15.9.69547; TBC 2.5.6 uses the same
// model), joined in `docs/design/2026-10-17-baseline-kit-client-data.md` §3:
// Shadow Word: Pain's impact (kit 304) plays `shadowwordpain_chest.m2` on the
// victim's chest attachment, and the spell has NO sustained state and no tick
// visual — the same finding as Moonfire and Curse of Agony. So the landing is
// the whole visual and the aura icon carries the DoT
// (`DotStateVisual::for_dot` stays `None` for it).
//
// The model's one 900 ms sequence: violet streaks sucked INWARD for 0.17 s,
// then at 0.23 s a burst of black smoke, streaking black blobs, violet and
// magenta streaks, dark streaks and blue -> violet stars, and late dark sparks
// at 0.7 s. The smoke has the longest tail, so the hit trails off by ~2.3 s.
//
// The emitter table below is the signed-off AS-333 Baseline Kit Visuals Bench
// (version 1792202136-e9a0, "Shadow Word: Pain" tab) layer for layer, at its
// defaults: all seven layers on, every multiplier 1.0. Its emitter model is
// the AS-160 bench's (`druid_heals.rs` ports the same `Emitter`), plus the two
// things this model needs that the Druid's do not: alpha-blended layers (M2
// blend 2, which DARKEN the sand rather than add light) and an inward launch.
//
// Spawning rides the sim's `AuraLanding` marker (`apply_pending_auras`, on a
// fresh application or a refresh). Scatter is a deterministic hash (never
// `game_rng`), nothing here writes sim state, and every system is registered
// in `states/mod.rs` only — headless stays byte-identical.

// ── Bench sign-off constants (AS-333, version 1792202136-e9a0) ───────────────

// Shadow Word: Pain landing: shadowwordpain_chest.m2 (kit 304) on the victim's chest. AS-333 bench.
pub const SWP_LANDING_SECS: f32 = 0.90; // smoke tail to ~2.34 s
pub const SWP_INWARD_STREAKS: bool = true;
pub const SWP_SMOKE: bool = true;
pub const SWP_BLOBS: bool = true;
pub const SWP_VIOLET_STREAKS: bool = true;
pub const SWP_DARK_STREAKS: bool = true;
pub const SWP_STARS: bool = true;
pub const SWP_LATE_SPARKS: bool = true;
pub const SWP_SIZE_MUL: f32 = 1.00;
pub const SWP_DENSITY_MUL: f32 = 1.00;
// DoT state: none (DotStateVisual::for_dot stays None for "Shadow Word: Pain").

/// The model's own sequence length; [`SWP_LANDING_SECS`] over it is the
/// bench's duration knob, which stretches the windows and the lives.
const SWP_MODEL_SECS: f32 = 0.9;

/// The bench's chest anchor, above the floor (`ANCH.chest`).
pub const SWP_CHEST_ABOVE_FLOOR: f32 = 1.45;

// ── Rendering constants ──────────────────────────────────────────────────────

/// Emissive gain of an additive particle over its colour — the house's 2-4x
/// glow convention (`hunter_shots.rs` and `druid_heals.rs` use the same).
const PARTICLE_GLOW: f32 = 2.5;
/// Steps in a particle's colour/alpha palette. The bench's tracks key at 0.4,
/// 0.5, 0.6 and 0.7 of a life; sixteen steps resolve them.
const PALETTE_STEPS: usize = 16;
/// A streak is drawn only above this speed, yd/s (the bench's `sp > 0.2`).
const STREAK_MIN_SPEED: f32 = 0.2;
/// The bench draws the streak at 0.85 of the sprite's alpha.
const STREAK_ALPHA: f32 = 0.85;
/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 64;

// ── The emitter model (the bench's `Emitter`) ────────────────────────────────

/// What a particle's sprite is drawn with — the bench's sprite `kind`s.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PainSprite {
    /// `glow`: a soft round light, falling to 0.45 at mid-radius.
    Glow,
    /// `smoke`: a softer, flatter puff (`toonsmoke16`).
    Smoke,
    /// `blob`: a hard-edged disc at 0.7 of the sprite radius (`blobs_black`).
    Blob,
    /// `star`: a four-point star with a hot centre (`star5a`, `flare`).
    Star,
}

/// How a particle launches.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PainLaunch {
    /// Outward from the chest in a direction on the full sphere, at the
    /// emitter's speed.
    Burst,
    /// Born on a shell about the chest and travelling IN to it (the bench's
    /// `inward`): `radius` is the shell (each particle at 0.6-1.0 of it), and
    /// `flat` stretches the shell vertically — the model's 1.11 x 1.56 area.
    Inward { radius: f32, flat: f32 },
}

/// A particle rate: one value, or a track keyed in milliseconds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PainRate {
    Flat(f32),
    Track(&'static [(f32, f32)]),
}

/// One emitter, as the bench's layer record carries it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PainEmitter {
    /// Unique key; the palette cache is keyed on it.
    pub name: &'static str,
    pub sprite: PainSprite,
    /// M2 blend 4 (additive) when `true`; blend 2 (alpha), which darkens what
    /// is behind it, when `false`.
    pub additive: bool,
    pub launch: PainLaunch,
    /// Launch speed, yd/s (its magnitude, for an inward launch).
    pub speed: f32,
    pub life: f32,
    pub rate: PainRate,
    /// On/off keys in milliseconds (`windowOn`).
    pub window: &'static [(f32, bool)],
    /// Colour, alpha and scale tracks over the particle's normalised life.
    pub color: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// Sprite RADIUS, yards.
    pub scale: &'static [(f32, f32)],
    pub drag: f32,
    /// Tail length factor; `0.0` draws no streak.
    pub tail: f32,
    /// Sprite spin, revolutions per second.
    pub spin: f32,
}

impl PainEmitter {
    /// Particles per second at `ms` into the (unstretched) sequence.
    pub fn rate_at(&self, ms: f32) -> f32 {
        match self.rate {
            PainRate::Flat(r) => r,
            PainRate::Track(keys) => keyed(keys, ms),
        }
    }

    /// Whether the emitter is emitting at `ms` into the (unstretched)
    /// sequence.
    pub fn emitting_at(&self, ms: f32) -> bool {
        window_on(Some(self.window), ms)
    }
}

const BLACK: [u8; 3] = [0, 0, 0];
const OPENS_AT_BURST: f32 = 233.0;

/// `shadowwordpain_chest.m2`'s emitters as the bench's seven layers.
pub const SWP_EMITTERS: [PainEmitter; 7] = [
    // P4 gradient64flipb: violet streaks sucked in for the first 167 ms.
    PainEmitter {
        name: "shadowwordpain_chest/P4 inward violet streaks",
        sprite: PainSprite::Glow,
        additive: true,
        launch: PainLaunch::Inward {
            radius: 0.9,
            flat: 1.4,
        },
        speed: 4.58,
        life: 0.375,
        rate: PainRate::Flat(104.0),
        window: &[(0.0, true), (167.0, false)],
        color: &[(0.0, [177, 42, 240]), (1.0, [255, 0, 216])],
        alpha: &[(0.0, 0.0), (0.4, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.01), (0.5, 0.05), (1.0, 0.01)],
        drag: 0.0,
        tail: 3.0,
        spin: 0.0,
    },
    // P0 + P9 toonsmoke16: the black smoke, the longest tail (1.44 s lives).
    PainEmitter {
        name: "shadowwordpain_chest/P0+P9 black smoke",
        sprite: PainSprite::Smoke,
        additive: false,
        launch: PainLaunch::Burst,
        speed: 1.1,
        life: 1.44,
        rate: PainRate::Track(&[(233.0, 100.0), (733.0, 60.0)]),
        window: &[(0.0, false), (OPENS_AT_BURST, true), (900.0, false)],
        color: &[(0.0, BLACK), (0.5, [84, 84, 83]), (1.0, [192, 192, 190])],
        alpha: &[(0.0, 0.85), (0.6, 0.5), (1.0, 0.0)],
        scale: &[(0.0, 0.17), (0.5, 0.26), (1.0, 0.4)],
        drag: 2.0,
        tail: 0.0,
        spin: -0.8,
    },
    // P1 + P2 blobs_black: streaking black blobs.
    PainEmitter {
        name: "shadowwordpain_chest/P1+P2 black blobs",
        sprite: PainSprite::Blob,
        additive: false,
        launch: PainLaunch::Burst,
        speed: 1.3,
        life: 1.0,
        rate: PainRate::Flat(80.0),
        window: &[(0.0, false), (OPENS_AT_BURST, true), (433.0, false)],
        color: &[(0.0, [4, 0, 84]), (0.5, BLACK), (1.0, [4, 0, 84])],
        alpha: &[(0.0, 1.0), (0.7, 0.8), (1.0, 0.0)],
        scale: &[(0.0, 0.02), (0.5, 0.16), (1.0, 0.17)],
        drag: 0.0,
        tail: 10.0,
        spin: 0.0,
    },
    // P7 + P8 gradient64flipb: the violet / magenta streak burst.
    PainEmitter {
        name: "shadowwordpain_chest/P7+P8 violet streaks",
        sprite: PainSprite::Glow,
        additive: true,
        launch: PainLaunch::Burst,
        speed: 2.27,
        life: 0.62,
        rate: PainRate::Flat(95.0),
        window: &[(0.0, false), (OPENS_AT_BURST, true), (467.0, false)],
        color: &[
            (0.0, [214, 170, 255]),
            (0.5, [255, 35, 252]),
            (1.0, [60, 0, 120]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.02), (0.5, 0.11), (1.0, 0.02)],
        drag: 0.0,
        tail: 4.0,
        spin: 0.0,
    },
    // P6 gradient64flipb: black streaks at alpha <= 0.6.
    PainEmitter {
        name: "shadowwordpain_chest/P6 dark streaks",
        sprite: PainSprite::Glow,
        additive: false,
        launch: PainLaunch::Burst,
        speed: 2.09,
        life: 1.03,
        rate: PainRate::Flat(39.0),
        window: &[(0.0, false), (OPENS_AT_BURST, true), (400.0, false)],
        color: &[(0.0, BLACK), (1.0, BLACK)],
        alpha: &[(0.0, 0.6), (1.0, 0.0)],
        scale: &[(0.0, 0.01), (0.5, 0.14), (1.0, 0.03)],
        drag: 0.0,
        tail: 4.0,
        spin: 0.0,
    },
    // P5 star5a: blue -> violet stars.
    PainEmitter {
        name: "shadowwordpain_chest/P5 stars",
        sprite: PainSprite::Star,
        additive: true,
        launch: PainLaunch::Burst,
        speed: 1.88,
        life: 1.0,
        rate: PainRate::Flat(48.0),
        window: &[(0.0, false), (OPENS_AT_BURST, true), (500.0, false)],
        color: &[
            (0.0, [44, 136, 240]),
            (0.5, [102, 35, 225]),
            (1.0, [90, 90, 120]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.05), (0.5, 0.11), (1.0, 0.04)],
        drag: 0.0,
        tail: 10.0,
        spin: -1.1,
    },
    // P3 flare: late dark sparks, indigo -> black.
    PainEmitter {
        name: "shadowwordpain_chest/P3 late dark sparks",
        sprite: PainSprite::Star,
        additive: false,
        launch: PainLaunch::Burst,
        speed: 1.11,
        life: 1.0,
        rate: PainRate::Flat(50.0),
        window: &[(0.0, false), (700.0, true), (900.0, false)],
        color: &[(0.0, [40, 20, 110]), (1.0, BLACK)],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.04), (0.5, 0.09), (1.0, 0.04)],
        drag: 0.0,
        tail: 10.0,
        spin: 0.0,
    },
];

/// Which of [`SWP_EMITTERS`] are kept layers, in table order.
pub const SWP_LAYERS: [bool; 7] = [
    SWP_INWARD_STREAKS,
    SWP_SMOKE,
    SWP_BLOBS,
    SWP_VIOLET_STREAKS,
    SWP_DARK_STREAKS,
    SWP_STARS,
    SWP_LATE_SPARKS,
];

/// The bench's duration knob: how far the landing is stretched over the
/// model's 0.9 s.
pub fn swp_time_scale() -> f32 {
    SWP_LANDING_SECS / SWP_MODEL_SECS
}

/// The landing's whole life: its emission plus its longest particle, so the
/// rig never retires while a particle it owes is still unborn.
pub fn swp_one_shot_life() -> f32 {
    let longest = SWP_EMITTERS.iter().map(|e| e.life).fold(0.0_f32, f32::max);
    SWP_LANDING_SECS + longest * swp_time_scale()
}

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// Where a particle of `e` is born and how it launches, for a victim at
/// `translation` (the bench's `spawn`, at spread 1.0).
pub fn swp_particle_launch(
    e: &PainEmitter,
    translation: Vec3,
    is_pet: bool,
    seed: u32,
) -> (Vec3, Vec3) {
    let s = stature(is_pet);
    let chest = body_point(SWP_CHEST_ABOVE_FLOOR, translation, is_pet);
    match e.launch {
        PainLaunch::Inward { radius, flat } => {
            // The bench's shell: a cube direction, stretched vertically by
            // `flat`, normalised; the particle is born 0.6-1.0 of the radius
            // out and travels straight in.
            let d = Vec3::new(
                draw(seed, 1) * 2.0 - 1.0,
                (draw(seed, 2) * 2.0 - 1.0) * flat,
                draw(seed, 3) * 2.0 - 1.0,
            )
            .try_normalize()
            .unwrap_or(Vec3::Y);
            let r = radius * s * (0.6 + 0.4 * draw(seed, 4));
            (chest + d * r, -d * e.speed * s)
        }
        PainLaunch::Burst => {
            // A uniform direction on the sphere.
            let u = draw(seed, 1) * 2.0 - 1.0;
            let az = draw(seed, 2) * TAU;
            let r = (1.0 - u * u).max(0.0).sqrt();
            let dir = Vec3::new(r * az.cos(), u, r * az.sin());
            (chest, dir * e.speed * s)
        }
    }
}

/// Length of a particle's streak behind it, yards (the bench's
/// `min(tail*0.12, speed*0.06) * size`), or `None` when it draws none.
pub fn swp_streak_length(e: &PainEmitter, speed: f32, size: f32) -> Option<f32> {
    (e.tail > 0.0 && speed > STREAK_MIN_SPEED).then(|| (e.tail * 0.12).min(speed * 0.06) * size)
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// Linear alpha stops over the unit disc's radius (the bench's radial
/// gradients).
fn radial(stops: &[(f32, f32)], r: f32) -> f32 {
    keyed(stops, r)
}

/// Sprites and palettes, built once per system, lazily.
pub struct PainAssets {
    quad: Handle<Mesh>,
    glow: Handle<Image>,
    smoke: Handle<Image>,
    blob: Handle<Image>,
    star: Handle<Image>,
    band: Handle<Image>,
    /// `(sprite palette, streak palette)` per emitter name.
    palettes: HashMap<
        &'static str,
        (
            Arc<[Handle<StandardMaterial>]>,
            Arc<[Handle<StandardMaterial>]>,
        ),
    >,
}

impl PainAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            glow: images.add(disc_texture(SPRITE_PX, |r, _| {
                radial(&[(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)], r)
            })),
            smoke: images.add(disc_texture(SPRITE_PX, |r, _| {
                radial(&[(0.0, 0.85), (0.45, 0.5), (1.0, 0.0)], r)
            })),
            // A hard disc, a pixel's antialiasing at its rim.
            blob: images.add(disc_texture(SPRITE_PX, |r, _| {
                ((0.7 - r) * SPRITE_PX as f32 * 0.5 + 0.5).clamp(0.0, 1.0)
            })),
            star: images.add(star_flash_texture()),
            band: images.add(soft_band_texture()),
            palettes: HashMap::new(),
        }
    }

    fn palettes(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        e: &PainEmitter,
    ) -> (
        Arc<[Handle<StandardMaterial>]>,
        Arc<[Handle<StandardMaterial>]>,
    ) {
        if let Some(p) = self.palettes.get(e.name) {
            return p.clone();
        }
        let texture = match e.sprite {
            PainSprite::Glow => self.glow.clone(),
            PainSprite::Smoke => self.smoke.clone(),
            PainSprite::Blob => self.blob.clone(),
            PainSprite::Star => self.star.clone(),
        };
        let build =
            |materials: &mut Assets<StandardMaterial>, texture: &Handle<Image>, gain: f32| {
                (0..PALETTE_STEPS)
                    .map(|i| {
                        let t = (i as f32 + 0.5) / PALETTE_STEPS as f32;
                        let color = keyed_color(e.color, t);
                        let alpha = (keyed(e.alpha, t) * gain).clamp(0.0, 1.0);
                        materials.add(particle_material(e.additive, color, alpha, texture))
                    })
                    .collect::<Arc<[_]>>()
            };
        let sprite = build(materials, &texture, 1.0);
        let streak = build(materials, &self.band.clone(), STREAK_ALPHA);
        self.palettes
            .insert(e.name, (sprite.clone(), streak.clone()));
        (sprite, streak)
    }
}

/// An additive layer is lit-emissive with `AlphaMode::Add` (`unlit` would
/// discard the emissive); an alpha layer is a flat, unlit colour with
/// `AlphaMode::Blend`, so its black smoke and blobs DARKEN the sand — the
/// client's blend 2, as `hunter_shots.rs` draws it.
fn particle_material(
    additive: bool,
    color: Color,
    alpha: f32,
    texture: &Handle<Image>,
) -> StandardMaterial {
    if additive {
        let c = color.to_linear();
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: Some(texture.clone()),
            emissive: LinearRgba::rgb(
                c.red * PARTICLE_GLOW,
                c.green * PARTICLE_GLOW,
                c.blue * PARTICLE_GLOW,
            ),
            emissive_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    }
}

fn palette_step(t: f32) -> usize {
    ((t.clamp(0.0, 1.0) * PALETTE_STEPS as f32) as usize).min(PALETTE_STEPS - 1)
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): a rig for every Shadow Word: Pain landing the sim
/// marked. Only this landing's markers are consumed; the Druid's are
/// `spawn_druid_effects`'.
pub fn spawn_shadow_pain_hits(
    mut commands: Commands,
    landings: Query<(Entity, &AuraLanding)>,
    victims: Query<(), With<Combatant>>,
) {
    for (marker, landing) in landings.iter() {
        if landing.kind != AuraLandingKind::ShadowWordPainHit {
            continue;
        }
        commands.entity(marker).despawn();
        if victims.get(landing.target).is_err() {
            continue;
        }
        commands.spawn((
            ShadowPainHit {
                target: landing.target,
                age: 0.0,
                carry: vec![0.0; SWP_EMITTERS.len()],
                emitted: 0,
            },
            PlayMatchEntity,
        ));
    }
}

/// Animate (graphical-only): emit each rig's particles from the victim's
/// chest, integrating the rate over the frame so a frame-rate change does not
/// change the density, and retire the rig once its last particle has died.
#[allow(clippy::too_many_arguments)]
pub fn animate_shadow_pain_hits(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<PainAssets>>,
    mut rigs: Query<(Entity, &mut ShadowPainHit)>,
    victims: Query<(&Transform, Option<&Pet>), With<Combatant>>,
) {
    let dt = time.delta_secs();
    let assets = assets.get_or_insert_with(|| PainAssets::build(&mut meshes, &mut images));
    let time_scale = swp_time_scale();

    for (rig_entity, mut rig) in rigs.iter_mut() {
        let Ok((victim, pet)) = victims.get(rig.target) else {
            commands.entity(rig_entity).despawn();
            continue;
        };
        if rig.age >= swp_one_shot_life() {
            commands.entity(rig_entity).despawn();
            continue;
        }
        let is_pet = pet.is_some();
        let at = victim.translation;
        rig.age += dt;
        let ms = rig.age * 1000.0 / time_scale;

        for (ei, e) in SWP_EMITTERS.iter().enumerate() {
            if !SWP_LAYERS[ei] || !e.emitting_at(ms) {
                continue;
            }
            rig.carry[ei] += e.rate_at(ms) * SWP_DENSITY_MUL * dt;
            if rig.carry[ei] < 1.0 {
                continue;
            }
            let (sprite_palette, streak_palette) = assets.palettes(&mut materials, e);
            while rig.carry[ei] >= 1.0 {
                rig.carry[ei] -= 1.0;
                let seed = rig_entity
                    .index()
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let (position, velocity) = swp_particle_launch(e, at, is_pet, seed);
                let mut parts = vec![commands
                    .spawn((
                        ShadowPainParticlePart::Sprite,
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(sprite_palette[0].clone()),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id()];
                if e.tail > 0.0 {
                    parts.push(
                        commands
                            .spawn((
                                ShadowPainParticlePart::Streak,
                                Mesh3d(assets.quad.clone()),
                                MeshMaterial3d(streak_palette[0].clone()),
                                Transform::from_scale(Vec3::splat(1e-4)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
                commands
                    .spawn((
                        ShadowPainParticle {
                            rig: rig_entity,
                            emitter: ei,
                            age: 0.0,
                            life: e.life * time_scale,
                            velocity,
                            angle: draw(seed, 6) * TAU,
                            stature: stature(is_pet),
                            sprite_palette: sprite_palette.clone(),
                            streak_palette: streak_palette.clone(),
                        },
                        Transform::from_translation(position),
                        Visibility::default(),
                        PlayMatchEntity,
                    ))
                    .add_children(&parts);
            }
        }
    }
}

/// Animate (graphical-only): age, drag, move and spin every particle, and
/// retire it at the end of its life.
pub fn animate_shadow_pain_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut ShadowPainParticle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut transform) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let e = SWP_EMITTERS[p.emitter];
        p.velocity *= drag_factor(e.drag, dt);
        transform.translation += p.velocity * dt;
        p.angle += e.spin * TAU * dt;
    }
}

/// Billboard (graphical-only): turn each particle's sprite to the camera,
/// sized and coloured along its life, and lay its streak back along its
/// flight as the camera sees it.
#[allow(clippy::type_complexity)]
pub fn billboard_shadow_pain_particles(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    particles: Query<(&ShadowPainParticle, &Children)>,
    mut parts: Query<
        (
            &ShadowPainParticlePart,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<ShadowPainParticle>,
    >,
) {
    let cam = camera
        .iter()
        .next()
        .map(|c| c.compute_transform().rotation)
        .unwrap_or(Quat::IDENTITY);
    for (p, children) in particles.iter() {
        let e = SWP_EMITTERS[p.emitter];
        let t = (p.age / p.life).clamp(0.0, 1.0);
        let step = palette_step(t);
        // The scale track is a RADIUS; the quad is a unit diameter.
        let radius = keyed(e.scale, t) * SWP_SIZE_MUL * p.stature;
        let speed = p.velocity.length();
        for child in children.iter() {
            let Ok((part, mut transform, mut material)) = parts.get_mut(child) else {
                continue;
            };
            match part {
                ShadowPainParticlePart::Sprite => {
                    transform.rotation = cam * Quat::from_rotation_z(p.angle);
                    transform.translation = Vec3::ZERO;
                    transform.scale = Vec3::splat((radius * 2.0).max(1e-4));
                    material.0 = p.sprite_palette[step].clone();
                }
                ShadowPainParticlePart::Streak => {
                    let Some(length) = swp_streak_length(&e, speed, SWP_SIZE_MUL * p.stature)
                    else {
                        transform.scale = Vec3::splat(1e-4);
                        continue;
                    };
                    // Roll the camera-facing quad until its long (local X)
                    // axis lies back along the flight as the camera sees it.
                    let back = -p.velocity / speed;
                    let (right, up) = (cam * Vec3::X, cam * Vec3::Y);
                    let roll = back.dot(up).atan2(back.dot(right));
                    transform.rotation = cam * Quat::from_rotation_z(roll);
                    transform.translation = back * (length * 0.5);
                    // The bench's line is 0.7 of the sprite's radius wide.
                    transform.scale = Vec3::new(length.max(1e-4), (radius * 0.7).max(1e-4), 1.0);
                    material.0 = p.streak_palette[step].clone();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emitter_has_a_unique_palette_key() {
        let mut names: Vec<&str> = SWP_EMITTERS.iter().map(|e| e.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn the_inward_streaks_open_the_hit_and_the_burst_follows_at_233_ms() {
        let opening: Vec<&str> = SWP_EMITTERS
            .iter()
            .filter(|e| e.emitting_at(100.0))
            .map(|e| e.name)
            .collect();
        assert_eq!(opening, ["shadowwordpain_chest/P4 inward violet streaks"]);
        let burst = SWP_EMITTERS
            .iter()
            .filter(|e| e.emitting_at(OPENS_AT_BURST))
            .count();
        assert_eq!(burst, 5, "smoke, blobs, violet, dark streaks and stars");
        let late: Vec<&str> = SWP_EMITTERS
            .iter()
            .filter(|e| e.emitting_at(800.0))
            .map(|e| e.name)
            .collect();
        assert_eq!(
            late,
            [
                "shadowwordpain_chest/P0+P9 black smoke",
                "shadowwordpain_chest/P3 late dark sparks"
            ]
        );
        assert!(SWP_EMITTERS.iter().all(|e| !e.emitting_at(900.0)));
    }

    #[test]
    fn the_smoke_trails_the_hit_off_by_about_two_point_three_seconds() {
        assert!((swp_one_shot_life() - 2.34).abs() < 1e-4);
    }
}
