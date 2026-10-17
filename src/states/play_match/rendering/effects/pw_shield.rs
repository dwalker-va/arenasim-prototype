//! Power Word: Shield — the client's state model on our rig (AS-344).
//!
//! The Classic and TBC clients draw the shield as `holydivineshield_state_base.m2`
//! (fdid 166342, kit 847, SpellVisual 784; research and measurements in
//! `docs/design/2026-10-16-pws-client-data.md`). It is not a sphere. It is three
//! flat, hollow, additive CARDS on one cylindrical-billboard bone, worn at the
//! feet, with no particles:
//!
//! - **Rim** (`genericglow2b1`): a gold outline with a hard inner edge;
//! - **Streak A / B** (`shockwave10d` / `10b`): soft radial streaks that flicker
//!   against each other about every 100 ms, which is the shimmer.
//!
//! Its three sequences are its whole lifecycle: a 200 ms delay and then a tall,
//! full-strength pop (birth, 700 ms); a faint shimmer with a slow jelly wobble
//! (loop, 633 ms); and a swell-and-fade (death, 1100 ms) that plays however the
//! shield ends, broken or expired. The loud moment is the arrival and the
//! sustained state is quiet, so this channel costs less on a busy ally than
//! the sphere it replaces for this spell.
//!
//! Every number below is the bench's signed-off value
//! (`docs/design/benches/2026-10-16-pws-bench.html`). The user kept all three
//! layers, the birth pop and the death swell, at the client's own values; the
//! bench's break burst was left out, so it does not exist here.
//!
//! **Anchor.** The client wears the model on the Base attachment, which rides
//! the animated body. The cards follow the unit's `VisualBody` (walk bob, fear
//! bob, swing step, Cyclone lift), not the sim transform. That is also what
//! keeps their draw order fixed: the capsule is `AlphaMode::Blend` with no depth
//! write, so Bevy orders the two by the view depth of their translations. Every
//! card centre sits above the body centre on the same vertical, so it sorts in
//! front at every camera pitch. Anchored on the sim transform instead, a fear
//! bob (0.18 + 0.04) lifts the body past the rim's 0.11–0.14 and the leg-crossing
//! arc flickers behind and in front of the legs
//! (`tests/pw_shield_visual_probes.rs::the_cards_sort_in_front_of_the_body_at_every_pitch`).
//! That is why `PWS_PUSH` stays at the client's 0.
//!
//! Detection is renderer-side, off `ActiveAuras`, like the aura band: headless
//! never registers any of this, so the sim is untouched by construction. The
//! caster side is the Holy two-hand launch heals already draw (kit 270), routed
//! through `InstantAbilityFired` because PW:S never has a `CastingState`
//! (`heal_cast.rs`, `spawn_instant_cast_hands`).

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use crate::states::play_match::components::*;

// --- Signed-off values (the bench's defaults) --------------------------------

/// Client units to our rig: `humanmale.m2` stands 2.034, our capsule 2.5.
pub const PWS_CLIENT_TO_RIG: f32 = 2.5 / 2.034;
/// × the client quad sizes on our rig.
pub const PWS_SIZE: f32 = 1.0;
/// × how far the bone scale swings from 1 (birth pop, wobble, death swell).
pub const PWS_SPREAD: f32 = 1.0;
/// Yards added to the feet anchor (client 0).
pub const PWS_LIFT: f32 = 0.0;
/// Yards toward the camera (client 0: the cards stand on the body's centre plane).
pub const PWS_PUSH: f32 = 0.0;
/// × the client rim weight in the loop and death.
pub const PWS_RIM_GAIN: f32 = 1.0;
/// × the client streak weights in the loop and death.
pub const PWS_STREAK_GAIN: f32 = 1.0;
/// × the 300 ms arrival spike (client weight 1.0).
pub const PWS_BIRTH_GAIN: f32 = 1.0;
/// × every sequence length.
pub const PWS_TIME_SCALE: f32 = 1.0;

// --- Client data (fdid 166342, build 2.5.6.69795; Era identical) -------------

/// The aura name the shield is keyed on (`abilities.ron`'s `name`, which is
/// what `Aura::ability_name` carries; `the_aura_name_is_the_ability_name` pins
/// the two together).
pub const PW_SHIELD_AURA_NAME: &str = "Power Word: Shield";

/// Half the capsule's height: the feet sit this far below a body's centre.
const BODY_HALF_HEIGHT: f32 = 1.25;

/// A card in client units: width, height, bottom z and top z above the Base
/// attachment (the feet).
pub const PWS_RIM_QUAD: [f32; 4] = [3.18, 4.08, -0.91, 3.17];
pub const PWS_STREAK_QUAD: [f32; 4] = [2.26, 2.90, -0.12, 2.78];
/// Bones 1–3 scale about this height (client units above the feet).
pub const PWS_PIVOT_Z: f32 = 1.11;

/// Sequence lengths, ms.
pub const PWS_BIRTH_MS: f32 = 700.0;
pub const PWS_LOOP_MS: f32 = 633.0;
pub const PWS_DEATH_MS: f32 = 1100.0;
/// The rim's death fade runs out here, ms (the streaks keep flickering past it).
const PWS_RIM_FADE_MS: f32 = 1000.0;

/// Texture weights (ms, weight), all linear (interp 1).
const RIM_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (500.0, 0.12),
    (700.0, 0.15),
];
const RIM_LOOP: &[(f32, f32)] = &[(0.0, 0.15), (300.0, 0.06), (633.0, 0.15)];
const A_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (400.0, 0.12),
    (500.0, 0.0),
    (600.0, 0.12),
    (700.0, 0.0),
];
const A_LOOP: &[(f32, f32)] = &[
    (0.0, 0.0),
    (100.0, 0.12),
    (200.0, 0.0),
    (300.0, 0.12),
    (400.0, 0.0),
    (533.0, 0.2),
    (633.0, 0.0),
];
const A_DEATH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (100.0, 0.12),
    (234.0, 0.0),
    (334.0, 0.12),
    (434.0, 0.0),
    (567.0, 0.2),
    (667.0, 0.0),
    (800.0, 0.12),
    (900.0, 0.0),
    (1100.0, 0.0),
];
const B_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (400.0, 0.0),
    (500.0, 0.12),
    (600.0, 0.0),
    (700.0, 0.12),
];
const B_LOOP: &[(f32, f32)] = &[
    (0.0, 0.12),
    (100.0, 0.0),
    (200.0, 0.2),
    (300.0, 0.0),
    (400.0, 0.12),
    (533.0, 0.0),
    (633.0, 0.12),
];
const B_DEATH: &[(f32, f32)] = &[
    (0.0, 0.12),
    (100.0, 0.0),
    (234.0, 0.2),
    (334.0, 0.0),
    (434.0, 0.12),
    (567.0, 0.0),
    (667.0, 0.12),
    (800.0, 0.0),
    (900.0, 0.12),
    (1000.0, 0.0),
    (1100.0, 0.0),
];
/// Bone scale (width, height): the bone's y and z axes.
const SCALE_BIRTH: &[(f32, [f32; 2])] = &[
    (0.0, [0.0, 0.0]),
    (200.0, [0.0, 0.0]),
    (300.0, [0.522, 1.11]),
    (500.0, [1.09, 1.0]),
    (700.0, [0.901, 1.123]),
];
const SCALE_LOOP: &[(f32, [f32; 2])] = &[
    (0.0, [0.901, 1.123]),
    (300.0, [1.04, 1.0]),
    (633.0, [0.901, 1.123]),
];
/// Death: the first key is replaced by wherever the shield was.
const SCALE_DEATH: &[(f32, [f32; 2])] = &[
    (0.0, [0.901, 1.123]),
    (334.0, [1.04, 1.0]),
    (1000.0, [1.271, 1.158]),
    (1100.0, [1.271, 1.158]),
];

/// The three colours, sRGB (the client's per-batch colour tracks).
const RIM_RGB: [f32; 3] = [1.0, 0.91, 0.318];
const STREAK_A_RGB: [f32; 3] = [0.969, 0.992, 0.318];
const STREAK_B_RGB: [f32; 3] = [1.0, 0.976, 0.318];

/// Measured radial profiles (angular mean of the decoded textures; r is the
/// fraction of the card's half-size). Generated, never shipped as assets.
const RIM_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.47, 0.0),
    (0.50, 0.93),
    (0.55, 0.74),
    (0.60, 0.46),
    (0.65, 0.20),
    (0.70, 0.03),
    (0.75, 0.0),
    (1.0, 0.0),
];
const A_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.40, 0.0),
    (0.50, 0.009),
    (0.55, 0.037),
    (0.60, 0.105),
    (0.65, 0.166),
    (0.70, 0.177),
    (0.75, 0.154),
    (0.80, 0.114),
    (0.85, 0.072),
    (0.90, 0.029),
    (0.95, 0.006),
    (1.0, 0.0),
];
const B_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.45, 0.0),
    (0.50, 0.005),
    (0.55, 0.018),
    (0.60, 0.060),
    (0.65, 0.134),
    (0.70, 0.173),
    (0.75, 0.163),
    (0.80, 0.133),
    (0.85, 0.097),
    (0.90, 0.058),
    (0.95, 0.020),
    (1.0, 0.005),
];
/// The brightest a streak texel gets (the decoded textures peak near 0.56).
const STREAK_TEXEL_MAX: f32 = 0.58;
const PWS_TEXTURE_PX: u32 = 256;

// --- The pure model (what the probes test) -----------------------------------

/// Linear keyframes, held flat outside their range.
fn keys(track: &[(f32, f32)], t: f32) -> f32 {
    if t <= track[0].0 {
        return track[0].1;
    }
    for pair in track.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let u = (t - a.0) / (b.0 - a.0).max(f32::EPSILON);
            return a.1 + (b.1 - a.1) * u;
        }
    }
    track[track.len() - 1].1
}

fn keys2(track: &[(f32, [f32; 2])], t: f32) -> Vec2 {
    let w: Vec<(f32, f32)> = track.iter().map(|&(t, v)| (t, v[0])).collect();
    let h: Vec<(f32, f32)> = track.iter().map(|&(t, v)| (t, v[1])).collect();
    Vec2::new(keys(&w, t), keys(&h, t))
}

/// `PWS_SPREAD` scales a key's swing away from 1; the birth's zero keys stay zero.
fn spread(v: f32) -> f32 {
    if v == 0.0 {
        0.0
    } else {
        1.0 + (v - 1.0) * PWS_SPREAD
    }
}

/// One phase's length in seconds.
pub fn pws_phase_secs(phase: PwsPhase) -> f32 {
    let ms = match phase {
        PwsPhase::Birth => PWS_BIRTH_MS,
        PwsPhase::Loop => PWS_LOOP_MS,
        PwsPhase::Death => PWS_DEATH_MS,
    };
    ms / 1000.0 * PWS_TIME_SCALE
}

/// The shield's pose at one instant: card scale (width, height) and the three
/// texture weights in [`PwsLayer::ALL`] order, gains applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PwsSample {
    pub scale: Vec2,
    pub weights: [f32; 3],
}

impl PwsSample {
    pub fn weight(&self, layer: PwsLayer) -> f32 {
        self.weights[layer as usize]
    }
}

/// Samples a shield. `t` is seconds into `phase`; the loop wraps on its own.
pub fn pws_sample(shield: &PwShieldVisual) -> PwsSample {
    let ms = shield.t / PWS_TIME_SCALE * 1000.0;
    let gains = [PWS_RIM_GAIN, PWS_STREAK_GAIN, PWS_STREAK_GAIN];
    match shield.phase {
        PwsPhase::Birth => {
            let s = keys2(SCALE_BIRTH, ms);
            // The spike carries the birth gain, settling to each layer's gain by 700 ms.
            let blend = ((ms - 300.0) / 400.0).clamp(0.0, 1.0);
            let w = [keys(RIM_BIRTH, ms), keys(A_BIRTH, ms), keys(B_BIRTH, ms)];
            PwsSample {
                scale: Vec2::new(spread(s.x), spread(s.y)),
                weights: std::array::from_fn(|i| {
                    w[i] * (PWS_BIRTH_GAIN + (gains[i] - PWS_BIRTH_GAIN) * blend)
                }),
            }
        }
        PwsPhase::Loop => {
            let m = ms.rem_euclid(PWS_LOOP_MS);
            let s = keys2(SCALE_LOOP, m);
            let w = [keys(RIM_LOOP, m), keys(A_LOOP, m), keys(B_LOOP, m)];
            PwsSample {
                scale: Vec2::new(spread(s.x), spread(s.y)),
                weights: std::array::from_fn(|i| w[i] * gains[i]),
            }
        }
        PwsPhase::Death => {
            let mut track: Vec<(f32, [f32; 2])> = SCALE_DEATH
                .iter()
                .map(|&(t, v)| (t, [spread(v[0]), spread(v[1])]))
                .collect();
            track[0].1 = shield.from_scale.to_array();
            PwsSample {
                scale: keys2(&track, ms),
                weights: [
                    shield.from_rim * (1.0 - ms / PWS_RIM_FADE_MS).max(0.0),
                    keys(A_DEATH, ms) * gains[1],
                    keys(B_DEATH, ms) * gains[2],
                ],
            }
        }
    }
}

/// A card's centre above the FEET and its half-size, both in yards, at `scale`.
/// Bones 1–3 scale about the shared pivot, so a taller card also rises.
pub fn pws_card_extent(layer: PwsLayer, scale: Vec2) -> (f32, Vec2) {
    let quad = match layer {
        PwsLayer::Rim => PWS_RIM_QUAD,
        PwsLayer::StreakA | PwsLayer::StreakB => PWS_STREAK_QUAD,
    };
    let r = PWS_CLIENT_TO_RIG * PWS_SIZE;
    let centre = (quad[2] + quad[3]) / 2.0 * r;
    let pivot = PWS_PIVOT_Z * r;
    (
        pivot + (centre - pivot) * scale.y,
        Vec2::new(quad[0] / 2.0 * r * scale.x, quad[1] / 2.0 * r * scale.y),
    )
}

/// Where a card stands, given the centre of the bearer's rendered body.
pub fn pws_card_translation(body_centre: Vec3, layer: PwsLayer, scale: Vec2, camera: Vec3) -> Vec3 {
    let feet = body_centre + Vec3::Y * (PWS_LIFT - BODY_HALF_HEIGHT);
    let toward_camera = (camera - body_centre).with_y(0.0).normalize_or_zero();
    let (centre, _) = pws_card_extent(layer, scale);
    feet + toward_camera * PWS_PUSH + Vec3::Y * centre
}

/// The cylindrical billboard: an upright card (its `Rectangle` faces +Z) yawed
/// to face the camera, never tipped toward it.
pub fn pws_card_rotation(card: Vec3, camera: Vec3) -> Quat {
    let d = (camera - card).with_y(0.0);
    if d.length_squared() < 1e-8 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_y(d.x.atan2(d.z))
}

/// Whether this aura is a live Power Word: Shield (an absorb with something left).
pub fn is_live_pw_shield(aura: &Aura) -> bool {
    aura.effect_type == AuraType::Absorb
        && aura.ability_name == PW_SHIELD_AURA_NAME
        && aura.magnitude > 0.0
}

// --- Textures ----------------------------------------------------------------

/// The bench's hash, ported bit for bit (visual-only; never `game_rng`).
fn pws_hash(n: u32) -> f32 {
    let mut s = n.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (((s >> 22) ^ s) as f64 / 4_294_967_295.0) as f32
}

struct Streak {
    theta: f32,
    width: f32,
    amp: f32,
    drift: f32,
}

/// A streak ring: the measured radial mean times an angular pattern of `n`
/// soft streaks normalised to mean 1 (the bench's `streakFn`).
fn streak_alpha(profile: &[(f32, f32)], n: u32, seed: u32) -> impl Fn(f32, f32) -> f32 {
    use std::f32::consts::TAU;
    let streaks: Vec<Streak> = (0..n)
        .map(|i| Streak {
            theta: (i as f32 + pws_hash(seed + i * 3) * 0.7) / n as f32 * TAU,
            width: 0.05 + 0.07 * pws_hash(seed + i * 3 + 1),
            amp: 0.5 + pws_hash(seed + i * 3 + 2),
            drift: (pws_hash(seed + i * 7) - 0.5) * 0.08,
        })
        .collect();
    let angular = move |theta: f32, r: f32| -> f32 {
        let mut s = 0.12;
        for q in &streaks {
            let mut d = (theta - q.theta).abs();
            d = d.min(TAU - d);
            s += q.amp * (-(d / q.width).powi(2)).exp() * (1.0 + q.drift * (r - 0.7) * 10.0);
        }
        s
    };
    let mean = (0..720)
        .map(|k| angular(k as f32 / 720.0 * TAU, 0.7))
        .sum::<f32>()
        / 720.0;
    let profile = profile.to_vec();
    move |r, theta| {
        (keys(&profile, r) * angular(theta.rem_euclid(TAU), r) / mean).min(STREAK_TEXEL_MAX)
    }
}

/// White texels, the shape in alpha: `alpha(r, theta)` over the unit disc.
fn pws_texture(alpha: impl Fn(f32, f32) -> f32) -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let size = PWS_TEXTURE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let a = alpha((dx * dx + dy * dy).sqrt(), dy.atan2(dx)).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            data[i..i + 3].fill(255);
            data[i + 3] = (a * 255.0).round() as u8;
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

/// Shared by every shield: one quad, three textures.
pub struct PwsAssets {
    quad: Handle<Mesh>,
    textures: [Handle<Image>; 3],
}

impl PwsAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        let rim = keys_fn(RIM_PROFILE);
        Self {
            // 2 × 2, so a card's scale IS its half-size.
            quad: meshes.add(Rectangle::new(2.0, 2.0)),
            textures: [
                images.add(pws_texture(move |r, _| rim(r))),
                images.add(pws_texture(streak_alpha(A_PROFILE, 17, 101))),
                images.add(pws_texture(streak_alpha(B_PROFILE, 15, 577))),
            ],
        }
    }
}

fn keys_fn(track: &'static [(f32, f32)]) -> impl Fn(f32) -> f32 {
    move |r| keys(track, r)
}

fn layer_rgb(layer: PwsLayer) -> Color {
    let c = match layer {
        PwsLayer::Rim => RIM_RGB,
        PwsLayer::StreakA => STREAK_A_RGB,
        PwsLayer::StreakB => STREAK_B_RGB,
    };
    Color::srgb(c[0], c[1], c[2])
}

/// The client material: unlit, additive, no depth write, both faces. The
/// texture weight rides in the base colour's alpha (Add premultiplies by it).
/// `unlit` is the faithful choice here: the client adds colour × texture ×
/// weight and nothing else, which is exactly what the bench drew.
fn pws_material(texture: &Handle<Image>, layer: PwsLayer) -> StandardMaterial {
    StandardMaterial {
        base_color: layer_rgb(layer).with_alpha(0.0),
        base_color_texture: Some(texture.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

// --- Systems -----------------------------------------------------------------

/// Update (graphical-only): start, advance and end each bearer's shield.
///
/// A bearer whose live shield has no rig in birth or loop gets a new one. A rig
/// whose bearer no longer has a live shield (it broke, ran out, was dispelled,
/// or the bearer died) plays its death and then despawns. A rig whose bearer
/// entity is gone despawns at once.
pub fn update_pw_shield_visuals(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<PwsAssets>>,
    bearers: Query<(Entity, &ActiveAuras), With<Combatant>>,
    alive: Query<(), With<Combatant>>,
    mut rigs: Query<(Entity, &mut PwShieldVisual)>,
) {
    let dt = time.delta_secs();
    let shielded: Vec<Entity> = bearers
        .iter()
        .filter(|(_, auras)| auras.auras.iter().any(is_live_pw_shield))
        .map(|(e, _)| e)
        .collect();

    let mut held: Vec<Entity> = Vec::new();
    for (rig_entity, mut rig) in rigs.iter_mut() {
        if alive.get(rig.bearer).is_err() {
            commands.entity(rig_entity).despawn();
            continue;
        }
        rig.t += dt;
        if rig.phase == PwsPhase::Birth && rig.t >= pws_phase_secs(PwsPhase::Birth) {
            rig.t -= pws_phase_secs(PwsPhase::Birth);
            rig.phase = PwsPhase::Loop;
        }
        let up = shielded.contains(&rig.bearer);
        if rig.phase != PwsPhase::Death && !up {
            let now = pws_sample(&rig);
            rig.from_scale = now.scale;
            rig.from_rim = now.weight(PwsLayer::Rim);
            rig.phase = PwsPhase::Death;
            rig.t = 0.0;
        }
        if rig.phase == PwsPhase::Death {
            if rig.t >= pws_phase_secs(PwsPhase::Death) {
                commands.entity(rig_entity).despawn();
            }
        } else {
            held.push(rig.bearer);
        }
    }

    for bearer in shielded {
        if held.contains(&bearer) {
            continue;
        }
        let assets = assets.get_or_insert_with(|| PwsAssets::build(&mut meshes, &mut images));
        commands
            .spawn((
                PwShieldVisual {
                    bearer,
                    phase: PwsPhase::Birth,
                    t: 0.0,
                    from_scale: Vec2::ZERO,
                    from_rim: 0.0,
                },
                // The rig stays at the origin, so each card's local pose is its world pose.
                Transform::IDENTITY,
                Visibility::default(),
                PlayMatchEntity,
            ))
            .with_children(|rig| {
                for layer in PwsLayer::ALL {
                    rig.spawn((
                        PwShieldCard { layer },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(
                            materials.add(pws_material(&assets.textures[layer as usize], layer)),
                        ),
                        // Zero-sized until the first placement: the birth's first 200 ms are empty.
                        Transform::from_scale(Vec3::splat(PWS_MIN_SCALE)),
                        NotShadowCaster,
                    ));
                }
            });
    }
}

/// A card is never scaled to exactly zero (a singular matrix breaks normals).
const PWS_MIN_SCALE: f32 = 1e-4;

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each card on its bearer's rendered body, turn it
/// to the camera, and set its weight.
pub fn place_pw_shield_cards(
    camera: Query<&Transform, (With<Camera3d>, Without<PwShieldCard>)>,
    rigs: Query<(&PwShieldVisual, &Children)>,
    bearers: Query<(&Transform, Option<&Children>), (With<Combatant>, Without<PwShieldCard>)>,
    bodies: Query<&Transform, (With<VisualBody>, Without<PwShieldCard>, Without<Combatant>)>,
    mut cards: Query<
        (
            &PwShieldCard,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<Combatant>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let Ok((bearer_tf, bearer_children)) = bearers.get(rig.bearer) else {
            continue;
        };
        let body_local = bearer_children
            .and_then(|kids| kids.iter().find_map(|k| bodies.get(k).ok()))
            .map(|body| body.translation)
            .unwrap_or(Vec3::ZERO);
        let body_centre = bearer_tf.transform_point(body_local);
        let sample = pws_sample(rig);
        for child in children.iter() {
            let Ok((card, mut tf, material)) = cards.get_mut(child) else {
                continue;
            };
            let at = pws_card_translation(body_centre, card.layer, sample.scale, cam.translation);
            let (_, half) = pws_card_extent(card.layer, sample.scale);
            tf.translation = at;
            tf.rotation = pws_card_rotation(at, cam.translation);
            tf.scale = Vec3::new(half.x.max(PWS_MIN_SCALE), half.y.max(PWS_MIN_SCALE), 1.0);
            if let Some(m) = materials.get_mut(&material.0) {
                m.base_color
                    .set_alpha(sample.weight(card.layer).clamp(0.0, 1.0));
            }
        }
    }
}
