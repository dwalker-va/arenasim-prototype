use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::TAU;

use super::hunter_shots::{draw, ClientEmitter, EmitterAssets, EmitterSprite};
use super::school_impact::impact_origin;
use super::spell_bolts::soft_dot_texture;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ==============================================================================
// Life Tap — `lifetap.m2` landing on the Warlock's own chest
// ==============================================================================
//
// From the Classic client data (Era 1.15.9.69547, byte-identical in TBC
// 2.5.6.69795), joined in docs/design/2026-10-17-baseline-kit-client-data.md
// §1: visual 1225's impact kit 1167 attaches `spells/lifetap.m2` at the
// caster's chest (attachment 34). Life Tap is self-cast, so it plays on the
// Warlock. One 3334 ms sequence, no sustained state:
//
// - **Blue glow** — two spherical-billboard quads, 1.84 x 2.38 and
//   1.66 x 2.14 yd, `genericglow64`, additive, constant `#5165FF`. Alpha
//   0 -> 1 at 1334 ms -> 0 at 3334 ms; both scale-pulse
//   0.67 -> 0.99 -> 0.72 -> 0.84 -> 0.73 on the model's 833 ms global loop.
// - **P3 converging motes** — 834-2167 ms, 50/s, 1.0 s life, born 0.7-1.4 yd
//   out and travelling INWARD at 2.22 yd/s, red -> blue. Additive. The
//   health-into-mana identity.
// - **P2 cyan twinkles** — star sprites gated on and off from 1.0 s, 0.2 s
//   life, in a 0.56 x 1.11 yd column. Additive.
// - **P0 green puff** — 0-334 ms, 100/s, 1.0 s life, 1.94 yd/s under heavy
//   drag: it bursts out and stalls. Alpha-blended.
// - **P1 green sparks** — 34-334 ms, 100/s, 1.0 s life, 1.39 yd/s.
//   Alpha-blended.
//
// The values are the AS-333 Baseline Kit Visuals Bench's Life Tap tab at its
// defaults, which the user signed off (version 1792202136-e9a0): every layer
// on, every multiplier 1.0, the client's green kept (the AS-333 ruling:
// client-faithful). The cast side is the Warlock's shipped shadow hands and is
// not touched here.
//
// Graphical-only: spawned by `consume_instant_ability_signals` off the
// `InstantAbilityFired` the Warlock AI spawns at its Life Tap, scattered by a
// deterministic hash (never `game_rng`), writing no sim state, registered in
// `states/mod.rs` only — headless stays byte-identical. The particles are
// world-space client particles, aged and turned by `animate_client_particles`
// and `billboard_hunter_shots`, so a tap while running leaves its motes
// behind the way the client's do.

// ── The bench's constants (AS-333 Baseline Kit Visuals Bench, Life Tap tab,
//    signed off at version 1792202136-e9a0) ──────────────────────────────────

/// The whole landing, seconds (client sequence 3334 ms).
pub const LIFE_TAP_SECS: f32 = 3.334;
/// The blue glow: `#5165FF`, 1.84 x 2.38 yd, peak at 40%, 833 ms throb.
pub const LIFE_TAP_GLOW: bool = true;
/// The converging motes: inward 2.22 yd/s, red -> blue.
pub const LIFE_TAP_MOTES: bool = true;
pub const LIFE_TAP_TWINKLES: bool = true;
pub const LIFE_TAP_GREEN_PUFF: bool = true;
pub const LIFE_TAP_GREEN_SPARKS: bool = true;
/// Multipliers over the client (1.0 = client). `SIZE` scales every size;
/// `SPREAD` scales speeds, spawn areas and the glow; `DENSITY` scales rates;
/// `OPACITY` scales every alpha.
pub const LIFE_TAP_SIZE_MUL: f32 = 1.0;
pub const LIFE_TAP_SPREAD_MUL: f32 = 1.0;
pub const LIFE_TAP_DENSITY_MUL: f32 = 1.0;
pub const LIFE_TAP_OPACITY_MUL: f32 = 1.0;

// ── The client's values ────────────────────────────────────────────────────

/// The client's own sequence length, which [`LIFE_TAP_SECS`] stretches.
const LIFE_TAP_CLIENT_SECS: f32 = 3.334;
/// How far [`LIFE_TAP_SECS`] stretches the client's timings.
const TIME: f32 = LIFE_TAP_SECS / LIFE_TAP_CLIENT_SECS;

/// The glow's colour, `#5165FF`.
pub const LIFE_TAP_GLOW_RGB: [u8; 3] = [81, 101, 255];
/// The two glow quads, `(width, height)` in yards.
pub const LIFE_TAP_GLOW_QUADS: [Vec2; 2] = [Vec2::new(1.84, 2.38), Vec2::new(1.66, 2.14)];
/// The glow's alpha track, `(seconds, alpha)`.
const GLOW_ALPHA: [(f32, f32); 3] = [(0.0, 0.0), (1.334, 1.0), (3.334, 0.0)];
/// The scale pulse's loop, seconds (the model's global loop).
pub const LIFE_TAP_PULSE_LOOP_SECS: f32 = 0.833;
/// The scale pulse, keyed evenly across the loop.
pub const LIFE_TAP_PULSE: [f32; 5] = [0.67, 0.99, 0.72, 0.84, 0.73];
/// Emissive gain of the glow over its colour.
const GLOW_GAIN: f32 = 1.5;

/// Drag on the green puff (the M2's 8), as the bench integrates it: the
/// velocity loses `3 x drag` per second, exponentially.
const PUFF_DRAG: f32 = 8.0;
const DRAG_PER_SEC: f32 = 3.0 * PUFF_DRAG;
/// Twinkle spin, rad/s (the M2's -4: the bench's -0.64 turns a second).
const TWINKLE_SPIN: f32 = -0.64 * TAU;
/// The motes' birth shell, yards out from the chest (the client's 1.39 yd
/// area, from its half-extent to its whole).
pub const LIFE_TAP_MOTE_SHELL: (f32, f32) = (0.7, 1.4);

const RED: [u8; 3] = [255, 0, 0];
const MANA_BLUE: [u8; 3] = [63, 179, 255];
const CYAN: [u8; 3] = [0, 199, 255];
const PALE: [u8; 3] = [155, 216, 255];
const PUFF_GREEN: [u8; 3] = [67, 177, 0];
const PUFF_END: [u8; 3] = [30, 0, 0];
const SPARK_GREEN: [u8; 3] = [39, 165, 0];
const SPARK_END: [u8; 3] = [90, 10, 0];

/// The colour a two-key ramp shows `t` of the way along, as a middle key —
/// so a ramp keyed at a non-half `mid` stays a straight line.
const fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    const fn c(a: u8, b: u8, t: f32) -> u8 {
        (a as f32 + (b as f32 - a as f32) * t) as u8
    }
    [c(a[0], b[0], t), c(a[1], b[1], t), c(a[2], b[2], t)]
}

/// How a layer's particles are born.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Launch {
    /// From the emitter's spawn volume, in a uniformly random direction at the
    /// emitter's speed.
    Out,
    /// On a shell `(near, far)` yards out, travelling straight in at the
    /// emitter's speed.
    Inward { near: f32, far: f32 },
}

/// One of the model's four emitters: its ramps (as a [`ClientEmitter`], whose
/// `window` is ignored), the windows it emits in, and how it launches.
struct Layer {
    on: bool,
    emitter: ClientEmitter,
    windows: &'static [(f32, f32)],
    launch: Launch,
    /// Where the SIZE ramp's middle key sits (the colour ramp keeps
    /// `emitter.mid`).
    size_mid: f32,
    drag: f32,
    spin: f32,
}

const MOTE_WINDOWS: [(f32, f32); 1] = [(0.834, 2.167)];
const TWINKLE_WINDOWS: [(f32, f32); 4] =
    [(1.0, 1.167), (1.334, 1.5), (1.634, 1.834), (2.067, 2.234)];
const PUFF_WINDOWS: [(f32, f32); 1] = [(0.0, 0.334)];
const SPARK_WINDOWS: [(f32, f32); 1] = [(0.034, 0.334)];

const fn alpha3(a: [f32; 3]) -> [f32; 3] {
    [
        a[0] * LIFE_TAP_OPACITY_MUL,
        a[1] * LIFE_TAP_OPACITY_MUL,
        a[2] * LIFE_TAP_OPACITY_MUL,
    ]
}

const fn size3(s: [f32; 3]) -> [f32; 3] {
    [
        s[0] * LIFE_TAP_SIZE_MUL,
        s[1] * LIFE_TAP_SIZE_MUL,
        s[2] * LIFE_TAP_SIZE_MUL,
    ]
}

/// The four layers at the signed-off knobs, in [`LifeTapGlow::carry`] order.
static LAYERS: [Layer; 4] = [
    // P3 `blue_glow2`: the converging motes.
    Layer {
        on: LIFE_TAP_MOTES,
        emitter: ClientEmitter {
            name: "lifetap/p3 converging motes",
            sprite: EmitterSprite::Glow,
            additive: true,
            rate: 50.0 * LIFE_TAP_DENSITY_MUL,
            window: None,
            life: 1.0 * TIME,
            speed: 2.22 * LIFE_TAP_SPREAD_MUL,
            gravity: 0.0,
            area: 0.0,
            column: None,
            rise: false,
            color: [RED, lerp_rgb(RED, MANA_BLUE, 0.3), MANA_BLUE],
            alpha: alpha3([0.2, 1.0, 0.9]),
            size: size3([0.03, 0.14, 0.28]),
            mid: 0.3,
            track: None,
        },
        windows: &MOTE_WINDOWS,
        launch: Launch::Inward {
            near: LIFE_TAP_MOTE_SHELL.0 * LIFE_TAP_SPREAD_MUL,
            far: LIFE_TAP_MOTE_SHELL.1 * LIFE_TAP_SPREAD_MUL,
        },
        size_mid: 0.5,
        drag: 0.0,
        spin: 0.0,
    },
    // P2 `star4`: the cyan twinkles.
    Layer {
        on: LIFE_TAP_TWINKLES,
        emitter: ClientEmitter {
            name: "lifetap/p2 cyan twinkles",
            sprite: EmitterSprite::Flare,
            additive: true,
            rate: 50.0 * LIFE_TAP_DENSITY_MUL,
            window: None,
            life: 0.2 * TIME,
            speed: 0.28 * LIFE_TAP_SPREAD_MUL,
            gravity: 0.0,
            area: 0.0,
            column: Some((0.56 * LIFE_TAP_SPREAD_MUL, 1.11 * LIFE_TAP_SPREAD_MUL)),
            rise: false,
            color: [CYAN, lerp_rgb(CYAN, PALE, 0.5), PALE],
            alpha: alpha3([1.0, 0.9, 0.8]),
            size: size3([0.014, 0.11, 0.014]),
            mid: 0.5,
            track: None,
        },
        windows: &TWINKLE_WINDOWS,
        launch: Launch::Out,
        size_mid: 0.5,
        drag: 0.0,
        spin: TWINKLE_SPIN,
    },
    // P0 `clouds8x8fade`: the green puff.
    Layer {
        on: LIFE_TAP_GREEN_PUFF,
        emitter: ClientEmitter {
            name: "lifetap/p0 green puff",
            sprite: EmitterSprite::Glow,
            additive: false,
            rate: 100.0 * LIFE_TAP_DENSITY_MUL,
            window: None,
            life: 1.0 * TIME,
            speed: 1.94 * LIFE_TAP_SPREAD_MUL,
            gravity: 0.0,
            area: 0.0,
            column: None,
            rise: false,
            color: [PUFF_GREEN, PUFF_GREEN, PUFF_END],
            alpha: alpha3([0.7, 0.45, 0.0]),
            size: size3([0.11, 0.44, 1.11]),
            mid: 0.5,
            track: None,
        },
        windows: &PUFF_WINDOWS,
        launch: Launch::Out,
        size_mid: 0.5,
        drag: DRAG_PER_SEC,
        spin: 0.0,
    },
    // P1 `flare`: the green sparks.
    Layer {
        on: LIFE_TAP_GREEN_SPARKS,
        emitter: ClientEmitter {
            name: "lifetap/p1 green sparks",
            sprite: EmitterSprite::Flare,
            additive: false,
            rate: 100.0 * LIFE_TAP_DENSITY_MUL,
            window: None,
            life: 1.0 * TIME,
            speed: 1.39 * LIFE_TAP_SPREAD_MUL,
            gravity: 0.0,
            area: 0.0,
            column: None,
            rise: false,
            color: [
                SPARK_GREEN,
                lerp_rgb(SPARK_GREEN, SPARK_END, 0.5),
                SPARK_END,
            ],
            alpha: alpha3([1.0, 0.5, 0.0]),
            size: size3([0.08, 0.06, 0.03]),
            mid: 0.5,
            track: None,
        },
        windows: &SPARK_WINDOWS,
        launch: Launch::Out,
        size_mid: 0.5,
        drag: 0.0,
        spin: 0.0,
    },
];

impl Layer {
    /// Particles this layer owes between `t0` and `t1` seconds into the tap.
    fn count_between(&self, t0: f32, t1: f32) -> f32 {
        if !self.on {
            return 0.0;
        }
        self.windows
            .iter()
            .map(|&(s, e)| {
                ClientEmitter {
                    window: Some((s * TIME, e * TIME)),
                    ..self.emitter
                }
                .count_between(t0, t1)
            })
            .sum()
    }

    /// Where a particle is born relative to the chest, and how it launches.
    fn launch(&self, seed: u32) -> (Vec3, Vec3) {
        let e = &self.emitter;
        // A uniform direction on the sphere.
        let u = draw(seed, 4) * 2.0 - 1.0;
        let theta = draw(seed, 5) * TAU;
        let r = (1.0 - u * u).max(0.0).sqrt();
        let dir = Vec3::new(r * theta.cos(), u, r * theta.sin());
        match self.launch {
            Launch::Inward { near, far } => {
                let out = near + (far - near) * draw(seed, 1);
                (dir * out, -dir * e.speed)
            }
            Launch::Out => {
                let offset = match e.column {
                    Some((w, h)) => Vec3::new(
                        (draw(seed, 1) - 0.5) * w,
                        (draw(seed, 2) - 0.5) * h,
                        (draw(seed, 3) - 0.5) * w,
                    ),
                    None => Vec3::ZERO,
                };
                (offset, dir * e.speed)
            }
        }
    }
}

/// Whether an ability lands the Life Tap glow on its caster — the routing the
/// instant-ability router and the lands-silently audit share.
impl LifeTapGlow {
    pub fn plays_for(ability: AbilityType) -> bool {
        ability == AbilityType::LifeTap
    }
}

/// Start a Life Tap landing on `caster`. Called by the instant-ability router.
pub fn spawn_life_tap_glow(commands: &mut Commands, caster: Entity, caster_pos: Vec3) {
    commands.spawn((
        LifeTapGlow {
            caster,
            age: 0.0,
            carry: [0.0; 4],
            emitted: 0,
        },
        Transform::from_translation(life_tap_chest(caster_pos)),
        Visibility::default(),
        PlayMatchEntity,
    ));
}

/// Attachment 34: the Warlock's chest. A Warlock is never a pet.
pub fn life_tap_chest(translation: Vec3) -> Vec3 {
    impact_origin(ImpactAnchor::Chest, translation, false)
}

/// The glow's alpha `secs` into the tap.
pub fn life_tap_glow_alpha(secs: f32) -> f32 {
    let t = secs / TIME;
    let a = if t <= GLOW_ALPHA[0].0 {
        GLOW_ALPHA[0].1
    } else if t >= GLOW_ALPHA[2].0 {
        GLOW_ALPHA[2].1
    } else {
        let i = if t < GLOW_ALPHA[1].0 { 0 } else { 1 };
        let ((t0, a0), (t1, a1)) = (GLOW_ALPHA[i], GLOW_ALPHA[i + 1]);
        a0 + (a1 - a0) * (t - t0) / (t1 - t0)
    };
    a * LIFE_TAP_OPACITY_MUL
}

/// The glow's scale pulse `secs` into the tap, on the model's global loop.
pub fn life_tap_glow_pulse(secs: f32) -> f32 {
    let k = (secs.rem_euclid(LIFE_TAP_PULSE_LOOP_SECS) / LIFE_TAP_PULSE_LOOP_SECS)
        * (LIFE_TAP_PULSE.len() - 1) as f32;
    let i = (k as usize).min(LIFE_TAP_PULSE.len() - 2);
    let f = k - i as f32;
    LIFE_TAP_PULSE[i] + (LIFE_TAP_PULSE[i + 1] - LIFE_TAP_PULSE[i]) * f
}

/// Build the glow's two quads under each new landing: one additive material
/// per landing, since its alpha follows that landing's own clock.
pub fn spawn_life_tap_glow_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<Image>)>>,
    glows: Query<Entity, Added<LifeTapGlow>>,
) {
    if !LIFE_TAP_GLOW {
        return;
    }
    for entity in glows.iter() {
        let (quad, dot) = assets
            .get_or_insert_with(|| {
                (
                    meshes.add(Rectangle::new(1.0, 1.0)),
                    images.add(soft_dot_texture()),
                )
            })
            .clone();
        let color = Color::srgb_u8(
            LIFE_TAP_GLOW_RGB[0],
            LIFE_TAP_GLOW_RGB[1],
            LIFE_TAP_GLOW_RGB[2],
        );
        let c = color.to_linear();
        let material = materials.add(StandardMaterial {
            base_color: color.with_alpha(0.0),
            base_color_texture: Some(dot.clone()),
            emissive: LinearRgba::rgb(c.red * GLOW_GAIN, c.green * GLOW_GAIN, c.blue * GLOW_GAIN),
            emissive_texture: Some(dot),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        for size in LIFE_TAP_GLOW_QUADS {
            let child = commands
                .spawn((
                    LifeTapGlowQuad { size },
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_scale(Vec3::ZERO),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(entity).add_child(child);
        }
    }
}

/// Play every live Life Tap: follow the Warlock's chest, emit each layer over
/// its windows, size, fade and billboard the glow, and retire the landing at
/// [`LIFE_TAP_SECS`] (or as soon as its Warlock is gone).
pub fn update_life_tap_glows(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<EmitterAssets>>,
    mut glows: Query<(Entity, &mut LifeTapGlow, &mut Transform, Option<&Children>)>,
    mut quads: Query<
        (
            &LifeTapGlowQuad,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<LifeTapGlow>,
    >,
    casters: Query<
        &Transform,
        (
            With<Combatant>,
            Without<LifeTapGlow>,
            Without<LifeTapGlowQuad>,
        ),
    >,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<LifeTapGlow>,
            Without<LifeTapGlowQuad>,
            Without<Combatant>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let cam_rotation = camera.iter().next().map(|c| c.rotation);
    for (entity, mut glow, mut transform, children) in glows.iter_mut() {
        let Ok(caster) = casters.get(glow.caster) else {
            commands.entity(entity).despawn();
            continue;
        };
        let (from, to) = (glow.age, glow.age + dt);
        glow.age = to;
        if from >= LIFE_TAP_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        let chest = life_tap_chest(caster.translation);
        transform.translation = chest;

        // The particles: world-space, born at the chest as it is now.
        let assets = assets.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        for (i, layer) in LAYERS.iter().enumerate() {
            glow.carry[i] += layer.count_between(from, to);
            if glow.carry[i] < 1.0 {
                continue;
            }
            let palette = assets.palette(&mut materials, &layer.emitter);
            let quad = assets.quad();
            while glow.carry[i] >= 1.0 {
                glow.carry[i] -= 1.0;
                let seed = entity
                    .index()
                    .wrapping_add(glow.emitted.wrapping_mul(0x85EB_CA6B));
                glow.emitted = glow.emitted.wrapping_add(1);
                let (offset, velocity) = layer.launch(seed);
                let facing = if layer.spin != 0.0 {
                    ParticleFacing::Spin {
                        mrad_per_sec: (layer.spin * 1000.0) as i16,
                        start_mrad: (draw(seed, 6) * TAU * 1000.0) as i16,
                    }
                } else {
                    ParticleFacing::Camera
                };
                let e = &layer.emitter;
                let mut particle = commands.spawn((
                    ClientParticle {
                        age: 0.0,
                        life: e.life,
                        velocity,
                        gravity: 0.0,
                        size: e.size,
                        mid: layer.size_mid,
                        palette: palette.clone(),
                        step: 0,
                        facing,
                        owned: false,
                    },
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(palette[0].clone()),
                    Transform::from_translation(chest + offset)
                        .with_scale(Vec3::splat(e.size[0].max(1e-4))),
                    NotShadowCaster,
                    PlayMatchEntity,
                ));
                if layer.drag > 0.0 {
                    particle.insert(ParticleDrag {
                        per_sec: layer.drag,
                    });
                }
            }
        }

        // The glow: swell, throb, fade, facing the camera.
        let alpha = life_tap_glow_alpha(to);
        let pulse = life_tap_glow_pulse(to) * LIFE_TAP_SIZE_MUL * LIFE_TAP_SPREAD_MUL;
        for child in children.into_iter().flat_map(|c| c.iter()) {
            let Ok((quad, mut part, material)) = quads.get_mut(child) else {
                continue;
            };
            part.scale = (quad.size * pulse).extend(1.0);
            if let Some(rotation) = cam_rotation {
                part.rotation = transform.rotation.inverse() * rotation;
            }
            if let Some(material) = materials.get_mut(&material.0) {
                material.base_color.set_alpha(alpha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_layer_emits_its_client_count() {
        // Rate x window: 66.65 motes, 4 x ~8.4 twinkles, 33.4 puffs, 30 sparks.
        let counts: Vec<f32> = LAYERS
            .iter()
            .map(|l| l.count_between(0.0, LIFE_TAP_SECS))
            .collect();
        let want = [
            50.0 * 1.333,
            50.0 * (0.167 + 0.166 + 0.2 + 0.167),
            33.4,
            30.0,
        ];
        for (got, want) in counts.iter().zip(want) {
            assert!((got - want).abs() < 1e-2, "{counts:?}");
        }
    }

    #[test]
    fn the_glow_peaks_at_1334_ms_and_is_gone_at_the_end() {
        assert_eq!(life_tap_glow_alpha(0.0), 0.0);
        assert!((life_tap_glow_alpha(1.334) - 1.0).abs() < 1e-5);
        assert!((life_tap_glow_alpha(0.667) - 0.5).abs() < 1e-3);
        assert!(life_tap_glow_alpha(LIFE_TAP_SECS).abs() < 1e-5);
    }

    #[test]
    fn the_pulse_runs_its_five_keys_on_the_833_ms_loop() {
        let q = LIFE_TAP_PULSE_LOOP_SECS / 4.0;
        for (i, want) in LIFE_TAP_PULSE.iter().take(4).enumerate() {
            assert!((life_tap_glow_pulse(i as f32 * q) - want).abs() < 1e-4);
        }
        assert!((life_tap_glow_pulse(LIFE_TAP_PULSE_LOOP_SECS + q) - 0.99).abs() < 1e-4);
    }

    #[test]
    fn motes_are_born_on_the_shell_and_head_for_the_chest() {
        for seed in 0..200 {
            let (at, v) = LAYERS[0].launch(seed);
            let r = at.length();
            assert!((0.7 - 1e-4..=1.4 + 1e-4).contains(&r), "{r}");
            assert!((v.length() - 2.22).abs() < 1e-4);
            assert!(v.dot(at) < 0.0, "a mote must travel inward");
        }
    }
}
