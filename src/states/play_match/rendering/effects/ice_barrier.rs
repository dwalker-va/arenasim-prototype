//! Ice Barrier's own pieces (AS-358): the geometry and emitters the client's
//! `iceshield_state.m2` adds to the shared shield cards, and the Mage's cast
//! hands.
//!
//! The barrier is the third [`ShieldKind`] on the shield rig in `shield.rs`,
//! which owns its lifecycle, its cards, their placement and its tuning
//! (`IB_TUNING`). This module holds only what no other shield has:
//!
//! - **Icicles** (`ice3b_b`, submeshes 3–5): five clusters of downward icicles
//!   flat on the card, at the shoulders and knees, drawn twice: a plain pass
//!   and a sphere-mapped glint that turns with the view.
//! - **Frost mist** (`toonsmoke16`): puffs falling from just above the head for
//!   the barrier's whole life and the first 767 ms of its death, so the last of
//!   them outlive it.
//! - **Hands** (kit 207, `ice_precast_high_hand.m2`): a 1 s deep-blue glow and
//!   pulsing ring with sparks on both of the Mage's spell hands
//!   (`heal_cast.rs`, `HealCastKind::Frost`).
//!
//! Every number is the bench's signed-off value or the client's measurement
//! (`docs/design/2026-10-17-ice-barrier-client-data.md`,
//! `docs/design/benches/2026-10-17-ice-barrier-bench.html`). The icicles and
//! mist are procedural stand-ins drawn from the measured positions, sizes and
//! colours, as the bench drew them; no client art ships.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use std::f32::consts::TAU;

use super::shield::{disc_texture, keys, pws_hash, PWS_CLIENT_TO_RIG, PWS_STREAK_QUAD};

// --- The icicles ---------------------------------------------------------------

/// The icicles: `[y_left, y_right, z_top, y_tip, z_tip]` in the card plane
/// (client units, +y = the camera's right, z above the feet), read off
/// submeshes 3–5 of the model. Upper left, upper right, lower left, lower right.
pub const IB_ICICLES: [[f32; 5]; 15] = [
    [-0.69, -0.55, 1.90, -0.63, 1.52],
    [-0.60, -0.45, 2.00, -0.59, 1.72],
    [-0.50, -0.34, 2.04, -0.42, 1.70],
    [-0.38, -0.22, 2.09, -0.30, 1.79],
    [-0.26, -0.02, 2.12, -0.15, 1.71],
    [0.38, 0.56, 1.93, 0.52, 1.55],
    [0.50, 0.66, 1.95, 0.59, 1.68],
    [0.60, 0.72, 1.92, 0.67, 1.62],
    [-0.70, -0.58, 0.64, -0.66, 0.37],
    [-0.62, -0.50, 0.60, -0.59, 0.41],
    [-0.56, -0.42, 0.56, -0.48, 0.27],
    [0.14, 0.28, 0.33, 0.21, 0.05],
    [0.28, 0.42, 0.42, 0.375, 0.15],
    [0.38, 0.50, 0.47, 0.43, 0.17],
    [0.46, 0.61, 0.54, 0.50, 0.32],
];
/// `ice3b_b`: its brightest and mean texel, and mean alpha.
const ICE_LIGHT: [f32; 3] = [208.0, 232.0, 248.0];
const ICE_DEEP: [f32; 3] = [95.0, 160.0, 245.0];
const ICE_ALPHA: f32 = 0.62;
/// An icicle's crown cap height, client units.
const ICE_CAP: f32 = 0.05;
/// How bright the glint's sheen peaks, as a multiple of its weight.
const GLINT_PEAK: f32 = 2.5;

fn srgb_linear(c: [f32; 3]) -> [f32; 3] {
    let l = Color::srgb(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0).to_linear();
    [l.red, l.green, l.blue]
}

/// One icicle's three pieces in card-local yards about the streak card's
/// centre, at rig scale `r`: the light left facet, the deep right facet and
/// the crown cap (a convex fan).
fn icicle_polys(ic: [f32; 5], r: f32) -> [Vec<[f32; 2]>; 3] {
    let [yl, yr, zt, yt, zz] = ic;
    let ym = (yl + yr) / 2.0;
    let centre_z = (PWS_STREAK_QUAD[2] + PWS_STREAK_QUAD[3]) / 2.0;
    let p = |y: f32, z: f32| [y * r, (z - centre_z) * r];
    [
        vec![p(yl, zt), p(ym, zt - ICE_CAP * 0.5), p(yt, zz)],
        vec![p(ym, zt - ICE_CAP * 0.5), p(yr, zt), p(yt, zz)],
        vec![
            p(ym, zt - ICE_CAP * 0.5),
            p(yl, zt),
            p(yl + 0.03, zt + ICE_CAP),
            p(ym, zt + ICE_CAP * 1.3),
            p(yr - 0.03, zt + ICE_CAP),
            p(yr, zt),
        ],
    ]
}

/// A flat mesh of fans with one vertex colour per fan.
fn fan_mesh(fans: &[(Vec<[f32; 2]>, [f32; 4])]) -> Mesh {
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for (poly, colour) in fans {
        let base = positions.len() as u32;
        for v in poly {
            positions.push([v[0], v[1], 0.0]);
            colors.push(*colour);
        }
        for k in 1..poly.len() as u32 - 1 {
            indices.extend_from_slice(&[base, base + k, base + k + 1]);
        }
    }
    let n = positions.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    // Unlit: normals and UVs are required by the vertex layout only.
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// The icicles' plain pass at rig scale `r`: every icicle's light and deep
/// facets and its cap, coloured from `ice3b_b` in the bench's proportions.
/// Built about the streak card's centre, so the card's bone scale is its
/// transform scale.
pub fn ib_icicle_mesh(r: f32) -> Mesh {
    let light = srgb_linear(ICE_LIGHT);
    let deep = srgb_linear(ICE_DEEP);
    let mid = srgb_linear(std::array::from_fn(|i| (ICE_LIGHT[i] + ICE_DEEP[i]) / 2.0));
    let mut fans = Vec::new();
    for ic in IB_ICICLES {
        let [left, right, cap] = icicle_polys(ic, r);
        fans.push((left, [light[0], light[1], light[2], ICE_ALPHA * 0.85]));
        fans.push((right, [deep[0], deep[1], deep[2], ICE_ALPHA]));
        fans.push((cap, [mid[0], mid[1], mid[2], ICE_ALPHA * 0.9]));
    }
    fan_mesh(&fans)
}

/// The glint's sheen on the `index`th icicle at camera yaw `yaw` (the yaw of
/// the camera about the bearer): the bench's stand-in for the client's
/// sphere-mapped pass, a sheen that turns as the view does.
pub fn ib_glint_sheen(index: usize, yaw: f32) -> f32 {
    let ic = IB_ICICLES[index];
    let mid = (ic[0] + ic[1]) / 2.0;
    0.5 + 0.5 * (yaw * 3.0 + mid * 5.0 + ic[2] * 2.0).sin()
}

/// The glint pass at rig scale `r` and camera yaw `yaw`: each icicle's light
/// facet in white, at its sheen.
pub fn ib_glint_mesh(r: f32, yaw: f32) -> Mesh {
    let fans: Vec<_> = IB_ICICLES
        .iter()
        .enumerate()
        .map(|(i, ic)| {
            let [left, _, _] = icicle_polys(*ic, r);
            (left, [1.0, 1.0, 1.0, ib_glint_sheen(i, yaw) * GLINT_PEAK])
        })
        .collect();
    fan_mesh(&fans)
}

/// The camera's yaw about a point.
pub fn ib_view_yaw(from: Vec3, camera: Vec3) -> f32 {
    let d = camera - from;
    d.x.atan2(d.z)
}

// --- The frost mist --------------------------------------------------------------

/// Frost mist (particle 0, bone 4): spawn height above the feet, fall speed,
/// life, rate (all client units and seconds), and when it stops in the death.
pub const IB_MIST_Z: f32 = 2.541;
pub const IB_MIST_SPEED: f32 = 1.667;
const MIST_SPEED_VAR: f32 = 0.2;
/// The fall cone's half-angle, radians (13°).
const MIST_CONE: f32 = 0.227;
pub const IB_MIST_LIFE: f32 = 1.8;
pub const IB_MIST_PER_SEC: f32 = 11.7;
/// The spawn plane: its length runs toward the camera, its width across.
const MIST_AREA_LEN: f32 = 1.389;
const MIST_AREA_WID: f32 = 0.075;
pub const IB_MIST_STOP_MS: f32 = 767.0;
/// A puff's diameter at birth, midlife and death, client units (twice the
/// client's half-size keys 0.278, 0.417, 0.556).
pub const IB_MIST_DIAMETER: [f32; 3] = [0.556, 0.834, 1.112];
/// Its colour, sRGB, at birth, midlife and death: the client track's blue
/// with white added as it moves toward white, as the bench drew it.
pub const IB_MIST_COLOUR: [[u8; 3]; 3] = [[81, 172, 243], [138, 229, 255], [234, 255, 255]];
/// Its alpha at birth, midlife and death.
pub const IB_MIST_ALPHA_KEYS: [f32; 3] = [0.0, 0.39, 0.0];
/// How far a puff rolls in the view plane over its life, radians.
pub const IB_MIST_ROLL: f32 = 1.5;
/// How many puff shapes the mist draws from.
pub const IB_MIST_VARIANTS: u32 = 4;
const MIST_TEXTURE_PX: u32 = 64;

/// One puff's birth: world position and velocity, seeded by `seed` (a hash,
/// never `game_rng`), off the bearer's rendered feet and the camera. `spread`
/// scales the fall cone.
pub fn ib_mist_emit(seed: u32, feet: Vec3, camera: Vec3, spread: f32) -> (Vec3, Vec3) {
    let toward = (camera - feet).with_y(0.0).normalize_or(Vec3::Z);
    let across = Vec3::new(toward.z, 0.0, -toward.x);
    let r = PWS_CLIENT_TO_RIG;
    let n = |k: u32| pws_hash(seed.wrapping_mul(7).wrapping_add(k));
    let along = (n(0) - 0.5) * MIST_AREA_LEN * r;
    let side = (n(1) - 0.5) * MIST_AREA_WID * r;
    let at = feet + Vec3::Y * (IB_MIST_Z * r) + toward * along + across * side;
    let theta = n(2) * MIST_CONE * spread;
    let phi = n(3) * TAU;
    let speed = IB_MIST_SPEED * r * (1.0 + (n(4) * 2.0 - 1.0) * MIST_SPEED_VAR);
    let horizontal = across * phi.cos() + toward * phi.sin();
    let velocity = Vec3::Y * (-theta.cos() * speed) + horizontal * (theta.sin() * speed);
    (at, velocity)
}

/// A smoke puff standing in for one `toonsmoke16` frame: a soft blob with a
/// few lumps (the bench's `puffFn`), variant `i`.
pub fn ib_mist_texture(i: u32) -> Image {
    let seed = 31 + i * 7;
    let lumps: Vec<(f32, f32, f32)> = (0..5)
        .map(|k| {
            (
                pws_hash(seed + k) * TAU,
                0.25 + 0.25 * pws_hash(seed + k + 9),
                0.35 + 0.2 * pws_hash(seed + k + 19),
            )
        })
        .collect();
    disc_texture(MIST_TEXTURE_PX, move |r, theta| {
        let (x, y) = (r * theta.cos(), r * theta.sin());
        let mut v = (1.0 - r).max(0.0).powf(1.6) * 0.6;
        for &(th, lr, w) in &lumps {
            let d = ((x - lr * th.cos()).powi(2) + (y - lr * th.sin()).powi(2)).sqrt();
            v += 0.45 * (-((d / w).powi(2))).exp();
        }
        v * (1.0 - r * r).max(0.0)
    })
}

// --- The caster's hands (kit 207, `ice_precast_high_hand.m2`) ----------------

/// The kit's single 1000 ms sequence.
pub const IB_HAND_SECS: f32 = 1.0;
/// Glow card and ring diameters, client units (× [`PWS_CLIENT_TO_RIG`]).
pub const IB_HAND_GLOW: f32 = 0.76;
pub const IB_HAND_RING: f32 = 0.6;
/// The glow card's and ring's colours (sRGB) and the ring's weight.
pub const IB_HAND_GLOW_RGB: [f32; 3] = [0.106, 0.42, 1.0];
pub const IB_HAND_RING_RGB: [f32; 3] = [0.157, 0.357, 1.0];
pub const IB_HAND_RING_ALPHA: f32 = 0.75;
/// The sparks: how many per hand, their size and circling radius (yards),
/// spin (radians per second), colour and alpha.
pub const IB_HAND_SPARKS: u32 = 4;
pub const IB_HAND_SPARK_SIZE: f32 = 0.12;
pub const IB_HAND_SPARK_RADIUS: f32 = 0.16;
pub const IB_HAND_SPARK_SPIN: f32 = 9.0;
pub const IB_HAND_SPARK_RGB: [f32; 3] = [0.75, 0.9, 1.0];
pub const IB_HAND_SPARK_ALPHA: f32 = 0.9;
/// The glow and ring pulse on the model's 533 ms global loop.
const HAND_PULSE: &[(f32, f32)] = &[(0.0, 1.139), (300.0, 0.957), (533.0, 1.139)];
const HAND_PULSE_MS: f32 = 533.0;
/// The one-shot's fade in and out, as fractions of its length (ours: the
/// client snaps, and a hard pop strobes at render rates above the tick rate).
const HAND_FADE_IN: f32 = 0.08;
const HAND_FADE_OUT: f32 = 0.1;

/// The hand flash's envelope at `age` seconds: a short fade in, held, a short
/// fade out.
pub fn ib_hand_envelope(age: f32) -> f32 {
    let u = (age / IB_HAND_SECS).clamp(0.0, 1.0);
    if u < HAND_FADE_IN {
        u / HAND_FADE_IN
    } else if u > 1.0 - HAND_FADE_OUT {
        (1.0 - u) / HAND_FADE_OUT
    } else {
        1.0
    }
}

/// The glow and ring's pulse at `age`.
pub fn ib_hand_pulse(age: f32) -> f32 {
    keys(HAND_PULSE, (age * 1000.0).rem_euclid(HAND_PULSE_MS))
}

/// Rig-local position of the `index`th spark at `age` on the hand at `side`.
pub fn ib_hand_spark_local(index: u32, age: f32, side: f32) -> Vec3 {
    let a = age * IB_HAND_SPARK_SPIN + index as f32 * TAU / IB_HAND_SPARKS as f32 + side;
    let r = IB_HAND_SPARK_RADIUS;
    Vec3::new(r * a.cos(), r * a.sin(), r * (a * 1.3).sin())
}

/// The glow card's texture: a soft disc, `(1 - r)^2.2`.
pub fn ib_hand_glow_texture() -> Image {
    disc_texture(128, |r, _| (1.0 - r).max(0.0).powf(2.2))
}

/// The ring's texture: a soft band peaking at 0.62 of the radius.
pub fn ib_hand_ring_texture() -> Image {
    disc_texture(128, |r, _| (-((r - 0.62) / 0.12).powi(2)).exp() * 0.9)
}
