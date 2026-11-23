use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::view::NoFrustumCulling;

use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::druid_heals::{body_point, keyed};
use super::hunter_shots::{draw, ramp3, PALETTE_STEPS};
use super::shield::disc_texture;
use super::spell_bolts::star_flash_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// Soul Link's rune column and Demonic Sacrifice's triad
// ==============================================================================
//
// From the Classic client data, joined in
// `docs/design/2026-10-17-warlock-talent-client-data.md` §9-10 (AS-334):
//
// - Soul Link (19028 / 25228 -> visual 969) lands kit 991,
//   `amplifymagic_impact_base.m2`, at the feet: a column of eleven red rune
//   circles that rises off the ground and fans upward, each fainter than the
//   last, with red sparks spiralling out, gone in about 2.5 s. The client draws
//   no link, no beam and no lasting state on either body. Ruled (2026-10-17):
//   on the Warlock only, once, when the link forms at the gates.
// - Demonic Sacrifice (18788) is an instakill on the demon with no impact of
//   its own: the demon dies (AS-299's mechanic, `demonology::demonic_bonds`,
//   plays its ordinary death and never despawns it). The visible moment is
//   the buff's one-shot at the Warlock's chest,
//   `demonicsacrifice_succubus_chest.m2` (kit 5134): three glowing orbs spin
//   round the chest trailing ribbons, then motes stream up off three outer
//   points, 2.37 s. Ruled (2026-10-17): violet, Touch of Shadow's colour. The
//   buff draws nothing after; its icon carries it.
//
// Played as the signed-off bench plays them (AS-334 Warlock Talent Visuals
// Bench, https://claude.ai/artifact/KwBsy13gPub9sKmnKdYMgg, version
// 1792203924-603a, the "Soul Link" and "Demonic Sacrifice" tabs, at their
// defaults; source committed as
// `docs/design/benches/2026-10-17-warlock-talent-bench.html`). Every number
// below is the bench's, as its `customDraw` and layer records draw it, and
// where the card's prose differs the bench is what ships:
//
// - The rune column's alpha ladder runs 1.0, then 0.45 down to 0.09 by 0.04 a
//   ring: the bench's `max(0.05, ...)` floor is never reached by eleven rings.
// - The rune fade keys run in each ring's own time, so the column's last ring
//   is gone at 3.734 s, past the model's 2.5 s sequence (`SOUL_LINK_SECS`).
//   The ring scale snap (2.48 -> 0.91 at 200 ms -> 0.98) is keyed on the
//   landing's clock, so it has settled to 0.957 by the first ring at 334 ms.
// - The sparks are the bench's one merged emitter: 56/s at one speed, 2.4
//   yd/s, in a 0.6 rad cone about straight up from a 0.8 yd square at the
//   feet, each star turning at +3 rev/s (the client's three emitters run 1.67
//   / 2.22 / 3.33 yd/s and spin +-3).
// - Each orb trails ONE ribbon, 0.33 yd wide at the orb tapering to nothing
//   over 0.65 s (the client's second, 0.17 yd / 0.75 s ribbon is not drawn).
// - The motes are drawn in the bench's `colorMap` of the Felhunter's blue
//   tracks: the inner motes run (89.25, 24, 255) -> (185.55, 113.6, 255) ->
//   (202.65, 137.6, 255), and the outer motes turn from white through violet
//   and switch to green (25, 213, 0) at 84.8% of their life (the client's
//   Succubus tracks are (60,0,255) -> (124,107,255) -> (126,126,255) and
//   white -> (123,105,255) -> (25,213,0)).
//
// Departures from the bench, for the match camera (60 yd, pitch 38.7°, 45°):
//
// - The rune rings' 2 px canvas stroke is a world-space band
//   `SOUL_LINK_RUNE_STROKE` (0.09 yd) wide, about 2 px at the match camera; drawn at
//   the bench's own 0.02 yd it would be under half a pixel.
// - Every additive piece carries the house's 2.5x emissive glow over its
//   colour (`hunter_shots.rs`), and a particle's colour and alpha step
//   through the shared 8-step palette (`animate_client_particles`), so the
//   outer motes' green shows from 87.5% of their life.
// - A ribbon is one camera-facing strip whose width and alpha taper per
//   vertex, without the canvas's round caps.
// - Held in front of the body. The bench is a 2D canvas that draws every
//   piece over the capsule. Here the body is alpha-blended and writes no
//   depth, so Bevy orders it against these additive pieces by each mesh's
//   TRANSLATION alone: a ring low on the column, a far orb or a far mote
//   whose translation sits deeper than the body's would be drawn first and
//   painted over. So every piece is drawn on its own view ray, slid toward
//   the camera until it stands nearer than the body capsule's nearest point,
//   and scaled by the same ratio (the shared body hold, `body_hold.rs`):
//   the same pixels the bench gives it, always drawn after the body. (The
//   ribbons need none: each one's translation is the chest on the body's
//   axis.) The particles keep their true positions on [`BodyHeldParticle`]
//   and are held only for the draw.
//
// Both rigs follow their Warlock's RENDERED body (the bench's `customDraw`
// reads the body each frame; here the walk bob and the Cyclone lift carry
// it), placed after every writer of it ([`place_demonology_landings`]);
// their particles are world-space after birth, as the bench's
// emitters are (no `follow`), and finish their own short lives. A Warlock's
// death retires its rig at once; a polymorph does not (a landing plays out on
// the sheep, as the empowered flash does).
//
// The column is spawned off the gates opening (no sim marker: Soul Link has
// no aura); the triad rides the sim's `AuraLanding` marker for Touch of
// Shadow (`apply_pending_auras`). Scatter is the shared deterministic hash
// (never `game_rng`), nothing here writes sim state, and every system is
// registered in `states/mod.rs` only — headless stays byte-identical.

// ── Bench sign-off constants (AS-334, version 1792203924-603a) ──────────────

// Soul Link landing: amplifymagic_impact_base.m2 (kit 991) at the feet, once when the link forms. AS-334 bench.
pub const SOUL_LINK_RUNE_COLUMN: bool = true;
pub const SOUL_LINK_SPARKS: bool = true;
// Ruled: on the Warlock only (the bench's SOUL_LINK_ON_DEMON = false).
pub const SOUL_LINK_SECS: f32 = 2.50;
pub const SOUL_LINK_SIZE_MUL: f32 = 1.00;
// No sustained link or beam (client has none).

// Demonic Sacrifice buff landing: demonicsacrifice_succubus_chest.m2 on the Warlock's chest. AS-334 bench.
// (The bench's SACRIFICE_* names, prefixed DEMONIC_ to keep them apart from Blessing of Sacrifice's.)
// DEMONIC_SACRIFICE_PALETTE: SuccubusViolet (ruled 2026-10-17) — the colours below are that palette's.
pub const DEMONIC_SACRIFICE_ORBS_AND_RIBBONS: bool = true;
pub const DEMONIC_SACRIFICE_INNER_MOTES: bool = true;
pub const DEMONIC_SACRIFICE_OUTER_MOTES: bool = true;
pub const DEMONIC_SACRIFICE_ORB_REV_PER_SEC: f32 = 0.94;
pub const DEMONIC_SACRIFICE_SECS: f32 = 2.37;
pub const DEMONIC_SACRIFICE_SIZE_MUL: f32 = 1.00;
// Nothing on the demon (client: instakill). Touch of Shadow buff: icon only after the landing.

// ── Shared ───────────────────────────────────────────────────────────────────

/// Emissive gain of every additive piece over its colour — the house's 2-4x
/// glow convention (`hunter_shots.rs` emitters use the same).
pub const DEMONOLOGY_GLOW: f32 = 2.5;
/// The bench's `glow` sprite gradient: full at the centre, 0.45 at 0.45 of
/// the radius, nothing at the rim.
pub const DEMONOLOGY_GLOW_STOPS: [(f32, f32); 3] = [(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)];
/// Generated glow sprite, pixels on a side.
const DEMONOLOGY_GLOW_PX: u32 = 128;

/// A colour keyed in the bench's 0-255 sRGB units, kept fractional: the
/// bench's `colorMap` produces values such as 185.55.
type Rgb = [f32; 3];

fn srgb(c: Rgb) -> Color {
    Color::srgb(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0)
}

fn glow_of(color: Color) -> LinearRgba {
    let c = color.to_linear();
    LinearRgba::rgb(
        c.red * DEMONOLOGY_GLOW,
        c.green * DEMONOLOGY_GLOW,
        c.blue * DEMONOLOGY_GLOW,
    )
}

/// An additive material: `color` at `alpha`, lit by the house glow, on
/// `texture` (both channels) or flat.
fn additive_material(color: Color, alpha: f32, texture: Option<Handle<Image>>) -> StandardMaterial {
    StandardMaterial {
        base_color: color.with_alpha(alpha),
        base_color_texture: texture.clone(),
        // `unlit` stays false or Bevy discards the emissive (see
        // `spell_bolts.rs`).
        emissive: glow_of(color),
        emissive_texture: texture,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// The bench's `glow` sprite: the soft round light the orbs and the inner
/// motes draw with.
pub fn demonology_glow_texture() -> Image {
    disc_texture(DEMONOLOGY_GLOW_PX, |r, _| keyed(&DEMONOLOGY_GLOW_STOPS, r))
}

/// One particle emitter as the bench's layer record carries it, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DemonologyEmitter {
    /// Particles per second (ignored when `track` is set).
    pub rate: f32,
    /// A rate track, `(seconds, particles per second)`, linearly interpolated.
    pub track: Option<&'static [(f32, f32)]>,
    /// `(start, end)` of the emission window, seconds from the landing.
    pub window: (f32, f32),
    pub life: f32,
    /// Launch speed, yd/s.
    pub speed: f32,
    /// Downward acceleration, yd/s² (negative rises).
    pub gravity: f32,
    /// Sprite RADIUS at birth, midlife and death, yards.
    pub radius: [f32; 3],
    pub alpha: [f32; 3],
}

impl DemonologyEmitter {
    /// Particles emitted between `t0` and `t1` seconds into the landing: the
    /// integral of the rate, so a frame rate cannot change the count.
    pub fn count_between(&self, t0: f32, t1: f32) -> f32 {
        let (a, b) = (t0.max(self.window.0), t1.min(self.window.1));
        if b <= a {
            return 0.0;
        }
        let Some(track) = self.track else {
            return self.rate * (b - a);
        };
        let mut total = 0.0;
        for pair in track.windows(2) {
            let ((x0, r0), (x1, r1)) = (pair[0], pair[1]);
            let (lo, hi) = (a.max(x0), b.min(x1));
            if hi <= lo || x1 <= x0 {
                continue;
            }
            let at = |x: f32| r0 + (r1 - r0) * (x - x0) / (x1 - x0);
            total += 0.5 * (at(lo) + at(hi)) * (hi - lo);
        }
        total
    }

    /// The size ramp a particle is drawn at (diameters).
    fn size(&self) -> ParticleSize {
        ParticleSize::radii(self.radius)
    }
}

/// `PALETTE_STEPS` additive materials sampling `color(t)` and the emitter's
/// alpha ramp, as `animate_client_particles` steps them.
fn demonology_palette(
    materials: &mut Assets<StandardMaterial>,
    texture: &Handle<Image>,
    e: &DemonologyEmitter,
    color: impl Fn(f32) -> Rgb,
) -> Arc<[Handle<StandardMaterial>]> {
    (0..PALETTE_STEPS)
        .map(|i| {
            let t = (i as f32 + 0.5) / PALETTE_STEPS as f32;
            let alpha = ramp3(e.alpha, 0.5, t);
            materials.add(additive_material(
                srgb(color(t)),
                alpha,
                Some(texture.clone()),
            ))
        })
        .collect()
}

/// A three-key colour ramp (birth, midlife, death), keyed at half-life.
pub fn demonology_ramp_rgb(keys: [Rgb; 3], t: f32) -> Rgb {
    let channel = |c: usize| ramp3([keys[0][c], keys[1][c], keys[2][c]], 0.5, t);
    [channel(0), channel(1), channel(2)]
}

#[allow(clippy::too_many_arguments)]
fn spawn_particle(
    commands: &mut Commands,
    quad: &Handle<Mesh>,
    palette: &Arc<[Handle<StandardMaterial>]>,
    e: &DemonologyEmitter,
    at: Vec3,
    velocity: Vec3,
    facing: ParticleFacing,
    body: Entity,
) {
    commands.spawn((
        ClientParticle {
            age: 0.0,
            life: e.life,
            velocity,
            gravity: e.gravity,
            size: e.size(),
            mid: 0.5,
            palette: palette.clone(),
            step: 0,
            facing,
            owned: false,
        },
        Mesh3d(quad.clone()),
        MeshMaterial3d(palette[0].clone()),
        Transform::from_translation(at),
        NotShadowCaster,
        PlayMatchEntity,
        BodyHeldParticle { body, at },
    ));
}

// ── Soul Link: the rune column (amplifymagic_impact_base.m2) ─────────────────

/// The model's one sequence, seconds. [`SOUL_LINK_SECS`] over it is the
/// bench's duration knob.
const SOUL_LINK_MODEL_SECS: f32 = 2.5;
/// The column's eleven `aurarune_a` rings.
pub const SOUL_LINK_RUNE_COUNT: usize = 11;
/// The first ring fades in at 334 ms, each next one 100 ms later.
pub const SOUL_LINK_RUNE_FIRST_MS: f32 = 334.0;
pub const SOUL_LINK_RUNE_STAGGER_MS: f32 = 100.0;
/// A ring's fade, in its own milliseconds: in over 80 ms, held to 850 ms,
/// out by `1600 + 80 × index`.
pub const SOUL_LINK_RUNE_FADE_IN_MS: f32 = 80.0;
pub const SOUL_LINK_RUNE_HOLD_TO_MS: f32 = 850.0;
pub const SOUL_LINK_RUNE_GONE_MS: f32 = 1600.0;
pub const SOUL_LINK_RUNE_GONE_STEP_MS: f32 = 80.0;
/// The ring radius at the settled scale: `(1.76 - 0.05 × index) / 2` yards.
pub const SOUL_LINK_RUNE_HALF_WIDTH: f32 = 1.76;
pub const SOUL_LINK_RUNE_HALF_WIDTH_STEP: f32 = 0.05;
/// The rune ring's bone scale on the landing's clock, over its settled 0.98.
pub const SOUL_LINK_RUNE_SCALE_KEYS: [(f32, f32); 3] = [(0.0, 2.48), (200.0, 0.91), (400.0, 0.98)];
pub const SOUL_LINK_RUNE_SCALE_SETTLED: f32 = 0.98;
/// The column's rise: the top ring climbs to 5.06 yd over 2.5 s, each ring
/// below it `index / 10` of that.
pub const SOUL_LINK_RUNE_RISE_KEYS: [(f32, f32); 2] = [(0.0, 0.0), (2500.0, 5.06)];
/// The rings sit this far above the floor before they rise.
pub const SOUL_LINK_RUNE_ABOVE_FLOOR: f32 = 0.05;
/// Every third ring, from the first, is pink; the rest pure red.
pub const SOUL_LINK_RUNE_RED: Rgb = [255.0, 0.0, 0.0];
pub const SOUL_LINK_RUNE_PINK: Rgb = [255.0, 105.0, 105.0];
/// The bench's rune ring: a thin circle with eight ticks from 0.82 of the
/// radius out to it.
pub const SOUL_LINK_RUNE_TICKS: usize = 8;
pub const SOUL_LINK_RUNE_TICK_INNER: f32 = 0.82;
/// The ring's stroke, yards: about 2 px at the match camera (a departure from
/// the bench's 2 px canvas line, see the header).
pub const SOUL_LINK_RUNE_STROKE: f32 = 0.09;
/// Segments round a ring.
const SOUL_LINK_RUNE_SEGMENTS: usize = 48;

/// The red sparks (P0-P2 merged): 56/s for the first second from a 0.8 yd
/// square at the feet, at 2.4 yd/s in a 0.6 rad cone about straight up, each
/// living 1 s, pale red -> red -> pink, alpha 1 -> 0, radius 0.08 -> 0.01 yd.
pub const SOUL_LINK_SPARK_EMITTER: DemonologyEmitter = DemonologyEmitter {
    rate: 56.0,
    track: None,
    window: (0.0, 1.0),
    life: 1.0,
    speed: 2.4,
    gravity: 0.0,
    radius: [0.08, 0.045, 0.01],
    alpha: [1.0, 0.5, 0.0],
};
pub const SOUL_LINK_SPARK_COLORS: [Rgb; 3] = [
    [255.0, 190.0, 170.0],
    [255.0, 40.0, 40.0],
    [255.0, 150.0, 140.0],
];
pub const SOUL_LINK_SPARK_AREA: f32 = 0.8;
pub const SOUL_LINK_SPARK_CONE: f32 = 0.6;
/// Each star turns in the view plane, revolutions per second.
pub const SOUL_LINK_SPARK_SPIN_REV_PER_SEC: f32 = 3.0;

/// The bench's duration knob over the model.
pub fn soul_link_time_scale() -> f32 {
    SOUL_LINK_SECS / SOUL_LINK_MODEL_SECS
}

/// A ring's peak alpha: 1.0 for the first, then 0.45 falling by 0.04 a ring.
pub fn soul_link_rune_peak(index: usize) -> f32 {
    if index == 0 {
        1.0
    } else {
        (0.45 - 0.04 * (index as f32 - 1.0)).max(0.05)
    }
}

/// A ring's colour: pink for every third from the first, else red.
pub fn soul_link_rune_color(index: usize) -> Rgb {
    if index.is_multiple_of(3) {
        SOUL_LINK_RUNE_PINK
    } else {
        SOUL_LINK_RUNE_RED
    }
}

/// A ring's settled radius, yards.
pub fn soul_link_rune_radius(index: usize) -> f32 {
    (SOUL_LINK_RUNE_HALF_WIDTH - SOUL_LINK_RUNE_HALF_WIDTH_STEP * index as f32)
        * 0.5
        * SOUL_LINK_SIZE_MUL
}

/// One ring's frame: its alpha, the scale over its settled radius, and its
/// height above the floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoulLinkRuneFrame {
    pub alpha: f32,
    pub scale: f32,
    pub height: f32,
}

/// Ring `index` `age` seconds into the landing.
pub fn soul_link_rune_frame(index: usize, age: f32) -> SoulLinkRuneFrame {
    let ms = age * 1000.0 / soul_link_time_scale();
    let start = SOUL_LINK_RUNE_FIRST_MS + SOUL_LINK_RUNE_STAGGER_MS * index as f32;
    let peak = soul_link_rune_peak(index);
    let alpha = if ms < start {
        0.0
    } else {
        keyed(
            &[
                (0.0, 0.0),
                (SOUL_LINK_RUNE_FADE_IN_MS, peak),
                (SOUL_LINK_RUNE_HOLD_TO_MS, peak),
                (
                    SOUL_LINK_RUNE_GONE_MS + SOUL_LINK_RUNE_GONE_STEP_MS * index as f32,
                    0.0,
                ),
            ],
            ms - start,
        )
    };
    SoulLinkRuneFrame {
        alpha,
        scale: keyed(&SOUL_LINK_RUNE_SCALE_KEYS, ms) / SOUL_LINK_RUNE_SCALE_SETTLED,
        height: SOUL_LINK_RUNE_ABOVE_FLOOR
            + keyed(&SOUL_LINK_RUNE_RISE_KEYS, ms) * (index as f32 / 10.0),
    }
}

/// How long a column lives: until its last ring has faded.
pub fn soul_link_life() -> f32 {
    let last = SOUL_LINK_RUNE_COUNT - 1;
    (SOUL_LINK_RUNE_FIRST_MS
        + SOUL_LINK_RUNE_STAGGER_MS * last as f32
        + SOUL_LINK_RUNE_GONE_MS
        + SOUL_LINK_RUNE_GONE_STEP_MS * last as f32)
        / 1000.0
        * soul_link_time_scale()
}

/// Where a spark is born, relative to the feet, and how it launches (the
/// bench's `spawn`: a square at the feet, a cone about straight up).
pub fn soul_link_spark_launch(seed: u32) -> (Vec3, Vec3) {
    let pol = (draw(seed, 1) * 2.0 - 1.0) * SOUL_LINK_SPARK_CONE;
    let az = draw(seed, 2) * TAU;
    let dir = Vec3::new(pol.sin() * az.cos(), pol.cos(), pol.sin() * az.sin());
    let area = SOUL_LINK_SPARK_AREA * SOUL_LINK_SIZE_MUL;
    let at = Vec3::new(
        (draw(seed, 4) - 0.5) * area,
        0.0,
        (draw(seed, 5) - 0.5) * area,
    );
    (at, dir * SOUL_LINK_SPARK_EMITTER.speed * SOUL_LINK_SIZE_MUL)
}

/// A spark's facing: turning in the view plane from a scattered start.
pub fn soul_link_spark_facing(seed: u32) -> ParticleFacing {
    ParticleFacing::Spin {
        mrad_per_sec: (SOUL_LINK_SPARK_SPIN_REV_PER_SEC * TAU * 1000.0) as i16,
        start_mrad: (draw(seed, 6) * TAU * 1000.0) as i16,
    }
}

/// Whether Soul Link's column plays on `holder` as the gates open: the holder
/// alive with the link (unsevered, not sacrificing) and its demon alive. The
/// one test the spawn system and the lands-silently audit share.
pub fn soul_link_forms(holder: &Combatant, pet_alive: bool) -> bool {
    holder.is_alive()
        && holder.talent_mechanics.soul_link().is_some()
        && !holder.soul_link_severed
        && !holder.sacrifice_pending
        && pet_alive
}

/// A flat rune ring of `radius` lying in the ground plane, facing up: the
/// circle as a band `SOUL_LINK_RUNE_STROKE` wide and eight ticks from
/// [`SOUL_LINK_RUNE_TICK_INNER`] of the radius out to it.
pub fn soul_link_rune_ring_mesh(radius: f32) -> Mesh {
    let half = SOUL_LINK_RUNE_STROKE * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let flat = |r: f32, a: f32| [r * a.cos(), 0.0, r * a.sin()];
    for k in 0..=SOUL_LINK_RUNE_SEGMENTS {
        let a = k as f32 / SOUL_LINK_RUNE_SEGMENTS as f32 * TAU;
        positions.push(flat(radius - half, a));
        positions.push(flat(radius + half, a));
    }
    for k in 0..SOUL_LINK_RUNE_SEGMENTS as u32 {
        let b = k * 2;
        indices.extend_from_slice(&[b, b + 1, b + 3, b, b + 3, b + 2]);
    }
    for k in 0..SOUL_LINK_RUNE_TICKS {
        let a = k as f32 / SOUL_LINK_RUNE_TICKS as f32 * TAU;
        let along = Vec3::new(a.cos(), 0.0, a.sin());
        let across = Vec3::new(-a.sin(), 0.0, a.cos()) * half;
        let (inner, outer) = (along * radius * SOUL_LINK_RUNE_TICK_INNER, along * radius);
        let b = positions.len() as u32;
        for p in [
            inner - across,
            inner + across,
            outer + across,
            outer - across,
        ] {
            positions.push(p.to_array());
        }
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    let n = positions.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.5, 0.5]; n]);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ── Demonic Sacrifice: the triad (demonicsacrifice_succubus_chest.m2) ────────

/// The bench's own print of the 2.367 s sequence at its ×1.0 duration knob.
/// [`DEMONIC_SACRIFICE_SECS`] over it is that knob, so the model plays at its
/// own 2367 ms keys rather than stretched by the print's rounding.
const DEMONIC_SACRIFICE_BENCH_SECS: f32 = 2.37;
/// The bench's chest anchor, above the floor (`ANCH.chest`).
pub const DEMONIC_SACRIFICE_CHEST_ABOVE_FLOOR: f32 = 1.45;
/// Three orbs, a third of a turn apart.
pub const DEMONIC_SACRIFICE_ORB_COUNT: usize = 3;
/// An orb's distance from the chest axis and its drift:
/// `0.83 + 0.28 × sin(2.7 t)` yards.
pub const DEMONIC_SACRIFICE_ORB_RADIUS: f32 = 0.83;
pub const DEMONIC_SACRIFICE_ORB_DRIFT: f32 = 0.28;
pub const DEMONIC_SACRIFICE_ORB_DRIFT_RATE: f32 = 2.7;
/// The ribbons bob `0.1 × sin(4 t + k)` about the chest; the orb glow does
/// not (the bench draws it at the chest's height).
pub const DEMONIC_SACRIFICE_ORB_BOB: f32 = 0.1;
pub const DEMONIC_SACRIFICE_ORB_BOB_RATE: f32 = 4.0;
/// The orbs' and ribbons' alpha, keyed in milliseconds.
pub const DEMONIC_SACRIFICE_ORB_ALPHA_KEYS: [(f32, f32); 4] =
    [(0.0, 0.0), (334.0, 0.8), (1000.0, 0.8), (2367.0, 0.0)];
/// From here on the orbs are gone: the rig retires.
pub const DEMONIC_SACRIFICE_ORB_GONE_MS: f32 = 2367.0;
/// The orb glow: the bench's `glow` sprite, 0.35 yd across, violet.
pub const DEMONIC_SACRIFICE_ORB_GLOW_DIAMETER: f32 = 0.35;
pub const DEMONIC_SACRIFICE_ORB_COLOR: Rgb = [150.0, 120.0, 255.0];
/// Each orb's ribbon: 0.33 yd wide at the orb, tapering in width and alpha
/// to nothing over the trail's last 0.65 s, in 16 segments.
pub const DEMONIC_SACRIFICE_RIBBON_WIDTH: f32 = 0.33;
pub const DEMONIC_SACRIFICE_RIBBON_TRAIL_SECS: f32 = 0.65;
pub const DEMONIC_SACRIFICE_RIBBON_SEGMENTS: usize = 16;

/// The inner motes, at the orbs: the rate ramps to 100/s by 334 ms, holds to
/// 1000 ms and falls to nothing at 2367 ms; each lives 0.6 s at 0.83 yd/s,
/// fading in and out, radius 0 -> 0.07 -> 0.
pub const DEMONIC_SACRIFICE_INNER_MOTE_TRACK: [(f32, f32); 4] =
    [(0.0, 0.0), (0.334, 100.0), (1.0, 100.0), (2.367, 0.0)];
pub const DEMONIC_SACRIFICE_INNER_MOTE_EMITTER: DemonologyEmitter = DemonologyEmitter {
    rate: 0.0,
    track: Some(&DEMONIC_SACRIFICE_INNER_MOTE_TRACK),
    window: (0.0, 2.367),
    life: 0.6,
    speed: 0.83,
    gravity: 0.0,
    radius: [0.0, 0.07, 0.0],
    alpha: [0.0, 1.0, 0.0],
};
pub const DEMONIC_SACRIFICE_INNER_MOTE_COLORS: [Rgb; 3] = [
    [89.25, 24.0, 255.0],
    [185.55, 113.6, 255.0],
    [202.65, 137.6, 255.0],
];

/// The outer motes, at three points 1.75 yd out: 50/s from 1000 to 1667 ms,
/// born still in a 0.42 yd square and rising under -6.94 yd/s² gravity, each
/// living 0.8 s, alpha 1 -> 0, radius 0.07 -> 0.03 -> 0.01.
pub const DEMONIC_SACRIFICE_OUTER_MOTE_EMITTER: DemonologyEmitter = DemonologyEmitter {
    rate: 50.0,
    track: None,
    window: (1.0, 1.667),
    life: 0.8,
    speed: 0.0,
    gravity: -6.94,
    radius: [0.07, 0.03, 0.01],
    alpha: [1.0, 0.5, 0.0],
};
pub const DEMONIC_SACRIFICE_OUTER_MOTE_RADIUS: f32 = 1.75;
pub const DEMONIC_SACRIFICE_OUTER_MOTE_AREA: f32 = 0.42;
/// The outer motes' source track (the Felhunter's blue), which the bench's
/// `colorMap` turns violet, and the green it switches to once the source's
/// green channel falls below 60.
pub const DEMONIC_SACRIFICE_OUTER_MOTE_SOURCE: [Rgb; 3] = [
    [220.0, 229.0, 246.0],
    [105.0, 140.0, 255.0],
    [0.0, 25.0, 213.0],
];
pub const DEMONIC_SACRIFICE_OUTER_MOTE_GREEN: Rgb = [25.0, 213.0, 0.0];

/// The bench's duration knob over the model.
pub fn sacrifice_time_scale() -> f32 {
    DEMONIC_SACRIFICE_SECS / DEMONIC_SACRIFICE_BENCH_SECS
}

/// How long a triad's rig lives, seconds: until its orbs have faded.
pub fn sacrifice_life() -> f32 {
    DEMONIC_SACRIFICE_ORB_GONE_MS / 1000.0 * sacrifice_time_scale()
}

/// The orbs' and ribbons' alpha `age` seconds into the landing.
pub fn sacrifice_orb_alpha(age: f32) -> f32 {
    let ms = age * 1000.0 / sacrifice_time_scale();
    if ms > DEMONIC_SACRIFICE_ORB_GONE_MS {
        return 0.0;
    }
    keyed(&DEMONIC_SACRIFICE_ORB_ALPHA_KEYS, ms)
}

/// Orb `k`'s angle round the chest `t` seconds in.
pub fn sacrifice_orb_angle(k: usize, t: f32) -> f32 {
    t * DEMONIC_SACRIFICE_ORB_REV_PER_SEC * TAU
        + k as f32 * TAU / DEMONIC_SACRIFICE_ORB_COUNT as f32
}

/// Orb `k`'s glow, relative to the chest, `t` seconds in.
pub fn sacrifice_orb_point(k: usize, t: f32) -> Vec3 {
    let r = (DEMONIC_SACRIFICE_ORB_RADIUS
        + DEMONIC_SACRIFICE_ORB_DRIFT * (t * DEMONIC_SACRIFICE_ORB_DRIFT_RATE).sin())
        * DEMONIC_SACRIFICE_SIZE_MUL;
    let a = sacrifice_orb_angle(k, t);
    Vec3::new(r * a.cos(), 0.0, r * a.sin())
}

/// The point orb `k`'s ribbon runs through, relative to the chest, `t`
/// seconds in: the orb's, bobbing.
pub fn sacrifice_ribbon_point(k: usize, t: f32) -> Vec3 {
    sacrifice_orb_point(k, t)
        + Vec3::Y
            * DEMONIC_SACRIFICE_ORB_BOB
            * (t * DEMONIC_SACRIFICE_ORB_BOB_RATE + k as f32).sin()
}

/// The points orb `k`'s ribbon runs through at `t`, newest first: the bench's
/// `DEMONIC_SACRIFICE_RIBBON_SEGMENTS + 1` samples over the last `DEMONIC_SACRIFICE_RIBBON_TRAIL_SECS`, stopping
/// where the trail reaches the landing.
pub fn sacrifice_ribbon_trail(k: usize, t: f32) -> Vec<Vec3> {
    (0..=DEMONIC_SACRIFICE_RIBBON_SEGMENTS)
        .map(|s| {
            t - DEMONIC_SACRIFICE_RIBBON_TRAIL_SECS * s as f32
                / DEMONIC_SACRIFICE_RIBBON_SEGMENTS as f32
        })
        .take_while(|tt| *tt >= 0.0)
        .map(|tt| sacrifice_ribbon_point(k, tt))
        .collect()
}

/// The taper at trail sample `s`: the ribbon's width and alpha there, over
/// their values at the orb.
pub fn sacrifice_ribbon_taper(s: usize) -> f32 {
    1.0 - s as f32 / DEMONIC_SACRIFICE_RIBBON_SEGMENTS as f32
}

/// The outer motes' colour `t` of the way through a life: the bench's
/// `colorMap` over the source track — violet `(min(255, 0.9 r + 0.35 b),
/// 0.8 g, b)`, and green once the source's green is under 60 and its blue
/// over 150.
pub fn sacrifice_outer_mote_color(t: f32) -> Rgb {
    let c = demonology_ramp_rgb(DEMONIC_SACRIFICE_OUTER_MOTE_SOURCE, t);
    if c[1] < 60.0 && c[2] > 150.0 {
        return DEMONIC_SACRIFICE_OUTER_MOTE_GREEN;
    }
    [(c[0] * 0.9 + c[2] * 0.35).min(255.0), c[1] * 0.8, c[2]]
}

/// Where an inner mote is born, relative to the chest, and how it launches
/// `t` seconds in: at one of the three orbs (at their undrifted radius, as
/// the bench's `orbit` spawns), along a normalised cube direction.
pub fn sacrifice_inner_mote_launch(seed: u32, t: f32) -> (Vec3, Vec3) {
    let k = ((draw(seed, 1) * DEMONIC_SACRIFICE_ORB_COUNT as f32) as usize)
        .min(DEMONIC_SACRIFICE_ORB_COUNT - 1);
    let a = sacrifice_orb_angle(k, t);
    let r = DEMONIC_SACRIFICE_ORB_RADIUS * DEMONIC_SACRIFICE_SIZE_MUL;
    let at = Vec3::new(r * a.cos(), 0.0, r * a.sin());
    let dir = Vec3::new(
        draw(seed, 2) * 2.0 - 1.0,
        draw(seed, 3) * 2.0 - 1.0,
        draw(seed, 4) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y);
    (
        at,
        dir * DEMONIC_SACRIFICE_INNER_MOTE_EMITTER.speed * DEMONIC_SACRIFICE_SIZE_MUL,
    )
}

/// Where an outer mote is born, relative to the chest, `t` seconds in: in a
/// square about one of the three outer points, still (it rises under its
/// gravity).
pub fn sacrifice_outer_mote_launch(seed: u32, t: f32) -> (Vec3, Vec3) {
    let k = ((draw(seed, 1) * DEMONIC_SACRIFICE_ORB_COUNT as f32) as usize)
        .min(DEMONIC_SACRIFICE_ORB_COUNT - 1);
    let a = sacrifice_orb_angle(k, t);
    let r = DEMONIC_SACRIFICE_OUTER_MOTE_RADIUS * DEMONIC_SACRIFICE_SIZE_MUL;
    let area = DEMONIC_SACRIFICE_OUTER_MOTE_AREA * DEMONIC_SACRIFICE_SIZE_MUL;
    let at = Vec3::new(
        r * a.cos() + (draw(seed, 2) - 0.5) * area,
        0.0,
        r * a.sin() + (draw(seed, 3) - 0.5) * area,
    );
    (at, Vec3::ZERO)
}

/// Rewrite a ribbon's band through `points` (newest first, relative to the
/// rig): a strip facing `eye` (in the rig's frame), `DEMONIC_SACRIFICE_RIBBON_WIDTH` wide at the
/// orb, its width and vertex alpha tapering to nothing at the tail.
pub fn write_sacrifice_ribbon(mesh: &mut Mesh, points: &[Vec3], eye: Vec3) {
    let n = points.len();
    let mut positions = Vec::with_capacity(n * 2);
    let mut normals = Vec::with_capacity(n * 2);
    let mut uvs = Vec::with_capacity(n * 2);
    let mut colors = Vec::with_capacity(n * 2);
    let mut indices = Vec::new();
    if n >= 2 {
        for (s, p) in points.iter().enumerate() {
            let prev = points[s.saturating_sub(1)];
            let next = points[(s + 1).min(n - 1)];
            let tangent = (prev - next).normalize_or_zero();
            let view = (eye - *p).normalize_or_zero();
            let taper = sacrifice_ribbon_taper(s);
            let side = tangent.cross(view).normalize_or_zero()
                * (DEMONIC_SACRIFICE_RIBBON_WIDTH * DEMONIC_SACRIFICE_SIZE_MUL * taper * 0.5);
            for (v, at) in [(0.0, *p + side), (1.0, *p - side)] {
                positions.push(at.to_array());
                normals.push(view.to_array());
                uvs.push([s as f32 / DEMONIC_SACRIFICE_RIBBON_SEGMENTS as f32, v]);
                colors.push([1.0, 1.0, 1.0, taper]);
            }
        }
        for s in 0..(n - 1) as u32 {
            let b = s * 2;
            indices.extend_from_slice(&[b, b + 1, b + 3, b, b + 3, b + 2]);
        }
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}

fn empty_ribbon_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    write_sacrifice_ribbon(&mut mesh, &[], Vec3::Z);
    mesh
}

// ── Components ───────────────────────────────────────────────────────────────

/// Soul Link's rune column on a Warlock (graphical only): a rig at its feet
/// whose [`SoulLinkRune`] children are the eleven rings, and which emits the
/// world-space sparks.
#[derive(Component)]
pub struct SoulLinkColumn {
    pub warlock: Entity,
    pub age: f32,
    /// Sparks owed, carried between frames.
    pub carry: f32,
    pub emitted: u32,
    quad: Handle<Mesh>,
    sparks: Arc<[Handle<StandardMaterial>]>,
}

/// A world-space spark or mote held in front of its Warlock's body for the
/// draw: `at` is where it truly is, which `animate_client_particles` moves
/// (`unhold_demonology_particles` puts it back before, and
/// [`place_demonology_landings`] draws it held after).
#[derive(Component)]
pub struct BodyHeldParticle {
    pub body: Entity,
    pub at: Vec3,
}

/// One ring of a [`SoulLinkColumn`], `index` from the bottom; it owns its
/// material, since its alpha is its own.
#[derive(Component)]
pub struct SoulLinkRune {
    pub index: usize,
    pub material: Handle<StandardMaterial>,
}

/// Demonic Sacrifice's triad on a Warlock (graphical only): a rig at its
/// chest whose children are the three [`SacrificeOrb`] glows and their
/// [`SacrificeRibbon`]s, and which emits the world-space motes.
#[derive(Component)]
pub struct SacrificeTriad {
    pub warlock: Entity,
    pub age: f32,
    /// Motes owed per emitter (inner, outer), carried between frames.
    pub carry: [f32; 2],
    pub emitted: u32,
    /// The orbs' and ribbons' shared material (one alpha).
    pub orb_material: Handle<StandardMaterial>,
    pub ribbon_material: Handle<StandardMaterial>,
    quad: Handle<Mesh>,
    inner: Arc<[Handle<StandardMaterial>]>,
    outer: Arc<[Handle<StandardMaterial>]>,
}

/// One orb's glow of a [`SacrificeTriad`], `index` 0-2.
#[derive(Component)]
pub struct SacrificeOrb {
    pub index: usize,
}

/// One orb's ribbon of a [`SacrificeTriad`], rebuilt every frame in the
/// rig's frame.
#[derive(Component)]
pub struct SacrificeRibbon {
    pub index: usize,
    pub mesh: Handle<Mesh>,
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// Meshes, textures and particle palettes, built once, lazily.
pub struct DemonologyAssets {
    quad: Handle<Mesh>,
    rune_rings: Vec<Handle<Mesh>>,
    glow: Handle<Image>,
    sparks: Arc<[Handle<StandardMaterial>]>,
    inner: Arc<[Handle<StandardMaterial>]>,
    outer: Arc<[Handle<StandardMaterial>]>,
}

impl DemonologyAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let glow = images.add(demonology_glow_texture());
        let star = images.add(star_flash_texture());
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            rune_rings: (0..SOUL_LINK_RUNE_COUNT)
                .map(|i| meshes.add(soul_link_rune_ring_mesh(soul_link_rune_radius(i))))
                .collect(),
            sparks: demonology_palette(materials, &star, &SOUL_LINK_SPARK_EMITTER, |t| {
                demonology_ramp_rgb(SOUL_LINK_SPARK_COLORS, t)
            }),
            inner: demonology_palette(
                materials,
                &glow,
                &DEMONIC_SACRIFICE_INNER_MOTE_EMITTER,
                |t| demonology_ramp_rgb(DEMONIC_SACRIFICE_INNER_MOTE_COLORS, t),
            ),
            outer: demonology_palette(
                materials,
                &star,
                &DEMONIC_SACRIFICE_OUTER_MOTE_EMITTER,
                sacrifice_outer_mote_color,
            ),
            glow,
        }
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): a rune column at the feet of every Warlock whose
/// Soul Link forms as the gates open ([`soul_link_forms`]), and a triad at
/// the chest of every living unit a Touch of Shadow landing marks. Only the
/// triad's landing markers are consumed.
#[allow(clippy::too_many_arguments)]
pub fn spawn_demonology_landings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<DemonologyAssets>>,
    mut gates_were_open: Local<bool>,
    countdown: Option<Res<MatchCountdown>>,
    landings: Query<(Entity, &AuraLanding)>,
    holders: Query<(Entity, &Combatant, &Transform), Without<Pet>>,
    pets: Query<(&Pet, &Combatant)>,
) {
    let gates_open = countdown.is_some_and(|c| c.gates_opened);
    let gates_opening = gates_open && !*gates_were_open;
    *gates_were_open = gates_open;

    if gates_opening && SOUL_LINK_RUNE_COLUMN {
        let mut linked: Vec<(Entity, Vec3)> = holders
            .iter()
            .filter(|(entity, holder, _)| {
                let pet_alive = pets
                    .iter()
                    .any(|(pet, c)| pet.owner == *entity && c.is_alive());
                soul_link_forms(holder, pet_alive)
            })
            .map(|(entity, _, transform)| (entity, transform.translation))
            .collect();
        linked.sort_by_key(|(entity, _)| *entity);
        for (warlock, at) in linked {
            let assets = assets.get_or_insert_with(|| {
                DemonologyAssets::build(&mut meshes, &mut images, &mut materials)
            });
            spawn_soul_link_column(&mut commands, &mut materials, assets, warlock, at);
        }
    }

    for (marker, landing) in landings.iter() {
        if landing.kind != AuraLandingKind::SacrificeTriad {
            continue;
        }
        commands.entity(marker).despawn();
        // A corpse's triad is retired by its first animation, the frame it
        // is built: `living_warlock` is the one living test.
        let Ok((warlock, _, transform)) = holders.get(landing.target) else {
            continue;
        };
        let assets = assets.get_or_insert_with(|| {
            DemonologyAssets::build(&mut meshes, &mut images, &mut materials)
        });
        spawn_sacrifice_triad(
            &mut commands,
            &mut meshes,
            &mut materials,
            assets,
            warlock,
            transform.translation,
        );
    }
}

fn spawn_soul_link_column(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &DemonologyAssets,
    warlock: Entity,
    at: Vec3,
) {
    let runes: Vec<Entity> = (0..SOUL_LINK_RUNE_COUNT)
        .map(|index| {
            let frame = soul_link_rune_frame(index, 0.0);
            let material = materials.add(additive_material(
                srgb(soul_link_rune_color(index)),
                frame.alpha,
                None,
            ));
            commands
                .spawn((
                    SoulLinkRune {
                        index,
                        material: material.clone(),
                    },
                    Mesh3d(assets.rune_rings[index].clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(Vec3::Y * frame.height)
                        .with_scale(Vec3::splat(frame.scale)),
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    commands
        .spawn((
            SoulLinkColumn {
                warlock,
                age: 0.0,
                carry: 0.0,
                emitted: 0,
                quad: assets.quad.clone(),
                sparks: assets.sparks.clone(),
            },
            Transform::from_translation(body_point(0.0, at, false)),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&runes);
}

fn spawn_sacrifice_triad(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &DemonologyAssets,
    warlock: Entity,
    at: Vec3,
) {
    let alpha = sacrifice_orb_alpha(0.0);
    let orb_material = materials.add(additive_material(
        srgb(DEMONIC_SACRIFICE_ORB_COLOR),
        alpha,
        Some(assets.glow.clone()),
    ));
    let ribbon_material = materials.add(additive_material(
        srgb(DEMONIC_SACRIFICE_ORB_COLOR),
        alpha,
        None,
    ));
    let mut children = Vec::new();
    if DEMONIC_SACRIFICE_ORBS_AND_RIBBONS {
        for index in 0..DEMONIC_SACRIFICE_ORB_COUNT {
            children.push(
                commands
                    .spawn((
                        SacrificeOrb { index },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(orb_material.clone()),
                        Transform::from_translation(sacrifice_orb_point(index, 0.0)).with_scale(
                            Vec3::splat(
                                DEMONIC_SACRIFICE_ORB_GLOW_DIAMETER * DEMONIC_SACRIFICE_SIZE_MUL,
                            ),
                        ),
                        NotShadowCaster,
                    ))
                    .id(),
            );
            let mesh = meshes.add(empty_ribbon_mesh());
            children.push(
                commands
                    .spawn((
                        SacrificeRibbon {
                            index,
                            mesh: mesh.clone(),
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(ribbon_material.clone()),
                        Transform::IDENTITY,
                        NoFrustumCulling,
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }
    }
    commands
        .spawn((
            SacrificeTriad {
                warlock,
                age: 0.0,
                carry: [0.0; 2],
                emitted: 0,
                orb_material,
                ribbon_material,
                quad: assets.quad.clone(),
                inner: assets.inner.clone(),
                outer: assets.outer.clone(),
            },
            Transform::from_translation(body_point(DEMONIC_SACRIFICE_CHEST_ABOVE_FLOOR, at, false)),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&children);
}

fn living_warlock<'a>(
    holders: &'a Query<
        (&Combatant, &Transform),
        (Without<SoulLinkColumn>, Without<SacrificeTriad>),
    >,
    warlock: Entity,
) -> Option<&'a Transform> {
    holders
        .get(warlock)
        .ok()
        .filter(|(c, _)| c.is_alive())
        .map(|(_, t)| t)
}

/// Animate (graphical-only): raise, scale and fade each column's rings (their
/// true poses: [`place_demonology_landings`] stands the column on its
/// Warlock's rendered feet and holds them), emit its sparks over their first
/// second, and retire it once its last ring has faded, or at once with its
/// Warlock.
#[allow(clippy::type_complexity)]
pub fn animate_soul_link_columns(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut columns: Query<(Entity, &mut SoulLinkColumn, &Children)>,
    mut runes: Query<
        (&SoulLinkRune, &mut Transform),
        (Without<SoulLinkColumn>, Without<Combatant>),
    >,
    holders: Query<(&Combatant, &Transform), (Without<SoulLinkColumn>, Without<SacrificeTriad>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut column, children) in columns.iter_mut() {
        let Some(at) = living_warlock(&holders, column.warlock) else {
            commands.entity(entity).despawn();
            continue;
        };
        let t0 = column.age;
        column.age += dt;
        if column.age >= soul_link_life() {
            commands.entity(entity).despawn();
            continue;
        }
        // Sparks are born at the sim feet: world-space after birth.
        let feet = body_point(0.0, at.translation, false);
        for child in children.iter() {
            let Ok((rune, mut part)) = runes.get_mut(child) else {
                continue;
            };
            let frame = soul_link_rune_frame(rune.index, column.age);
            part.translation = Vec3::Y * frame.height;
            part.scale = Vec3::splat(frame.scale.max(1e-4));
            if let Some(material) = materials.get_mut(&rune.material) {
                material.base_color.set_alpha(frame.alpha);
            }
        }

        if !SOUL_LINK_SPARKS {
            continue;
        }
        let scale = soul_link_time_scale();
        column.carry += SOUL_LINK_SPARK_EMITTER.count_between(t0 / scale, column.age / scale);
        while column.carry >= 1.0 {
            column.carry -= 1.0;
            let seed = entity
                .index()
                .wrapping_mul(0x9E37_79B9)
                .wrapping_add(column.emitted.wrapping_mul(0x85EB_CA6B));
            column.emitted = column.emitted.wrapping_add(1);
            let (offset, velocity) = soul_link_spark_launch(seed);
            spawn_particle(
                &mut commands,
                &column.quad,
                &column.sparks,
                &SOUL_LINK_SPARK_EMITTER,
                feet + offset,
                velocity,
                soul_link_spark_facing(seed),
                column.warlock,
            );
        }
    }
}

/// Animate (graphical-only): turn and fade each triad's orbs (their true
/// poses: [`place_demonology_landings`] stands the triad on its Warlock's
/// rendered chest, holds them and rebuilds their ribbons), emit its inner and
/// outer motes, and retire it once its orbs have faded, or at once with its
/// Warlock.
#[allow(clippy::type_complexity)]
pub fn animate_sacrifice_triads(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut triads: Query<(Entity, &mut SacrificeTriad, &Children)>,
    mut orbs: Query<(&SacrificeOrb, &mut Transform), (Without<SacrificeTriad>, Without<Combatant>)>,
    holders: Query<(&Combatant, &Transform), (Without<SoulLinkColumn>, Without<SacrificeTriad>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut triad, children) in triads.iter_mut() {
        let Some(at) = living_warlock(&holders, triad.warlock) else {
            commands.entity(entity).despawn();
            continue;
        };
        let t0 = triad.age;
        triad.age += dt;
        if triad.age >= sacrifice_life() {
            commands.entity(entity).despawn();
            continue;
        }
        // Motes are born at the sim chest: world-space after birth.
        let chest = body_point(DEMONIC_SACRIFICE_CHEST_ABOVE_FLOOR, at.translation, false);
        let scale = sacrifice_time_scale();
        let t = triad.age / scale;

        let alpha = sacrifice_orb_alpha(triad.age);
        for handle in [&triad.orb_material, &triad.ribbon_material] {
            if let Some(material) = materials.get_mut(handle) {
                material.base_color.set_alpha(alpha);
            }
        }
        for child in children.iter() {
            if let Ok((orb, mut part)) = orbs.get_mut(child) {
                part.translation = sacrifice_orb_point(orb.index, t);
                part.scale =
                    Vec3::splat(DEMONIC_SACRIFICE_ORB_GLOW_DIAMETER * DEMONIC_SACRIFICE_SIZE_MUL);
            }
        }

        let (from, to) = (t0 / scale, t);
        let layers: [(bool, &DemonologyEmitter); 2] = [
            (
                DEMONIC_SACRIFICE_INNER_MOTES,
                &DEMONIC_SACRIFICE_INNER_MOTE_EMITTER,
            ),
            (
                DEMONIC_SACRIFICE_OUTER_MOTES,
                &DEMONIC_SACRIFICE_OUTER_MOTE_EMITTER,
            ),
        ];
        for (layer, (on, e)) in layers.into_iter().enumerate() {
            if !on {
                continue;
            }
            triad.carry[layer] += e.count_between(from, to);
            while triad.carry[layer] >= 1.0 {
                triad.carry[layer] -= 1.0;
                let seed = entity
                    .index()
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(triad.emitted.wrapping_mul(0x85EB_CA6B));
                triad.emitted = triad.emitted.wrapping_add(1);
                let (offset, velocity, palette) = if layer == 0 {
                    let (o, v) = sacrifice_inner_mote_launch(seed, to);
                    (o, v, triad.inner.clone())
                } else {
                    let (o, v) = sacrifice_outer_mote_launch(seed, to);
                    (o, v, triad.outer.clone())
                };
                spawn_particle(
                    &mut commands,
                    &triad.quad,
                    &palette,
                    e,
                    chest + offset,
                    velocity,
                    ParticleFacing::Camera,
                    triad.warlock,
                );
            }
        }
    }
}

/// Before the particles age (graphical-only): put every held spark and mote
/// back where it truly is, so `animate_client_particles` moves the true
/// position and never the drawn one.
pub fn unhold_demonology_particles(mut particles: Query<(&BodyHeldParticle, &mut Transform)>) {
    for (held, mut transform) in particles.iter_mut() {
        transform.translation = held.at;
    }
}

/// `BodyPlacement` (PostUpdate), after the particles age: stand each Soul Link
/// column on its Warlock's rendered feet and each Sacrifice triad on its
/// rendered chest; hold every ring, orb, spark and mote in front of that
/// rendered body (the shared body hold), its size scaled to keep its pixels;
/// turn the orbs to the camera and rebuild the ribbons facing it. A spark's
/// and mote's true position is recorded first, so the ager moves it and
/// never the held one.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn place_demonology_landings(
    hold: BodyHold,
    mut meshes: ResMut<Assets<Mesh>>,
    mut columns: Query<
        (&SoulLinkColumn, &mut Transform, &Children),
        (
            Without<SacrificeTriad>,
            Without<SoulLinkRune>,
            Without<SacrificeOrb>,
            HoldDisjoint,
        ),
    >,
    mut triads: Query<
        (&SacrificeTriad, &mut Transform, &Children),
        (
            Without<SoulLinkColumn>,
            Without<SoulLinkRune>,
            Without<SacrificeOrb>,
            HoldDisjoint,
        ),
    >,
    mut runes: Query<
        &mut Transform,
        (
            With<SoulLinkRune>,
            Without<SoulLinkColumn>,
            Without<SacrificeTriad>,
            Without<SacrificeOrb>,
            HoldDisjoint,
        ),
    >,
    mut orbs: Query<
        &mut Transform,
        (
            With<SacrificeOrb>,
            Without<SoulLinkColumn>,
            Without<SacrificeTriad>,
            Without<SoulLinkRune>,
            HoldDisjoint,
        ),
    >,
    ribbons: Query<&SacrificeRibbon>,
    mut particles: Query<
        (&mut BodyHeldParticle, &mut Transform),
        (
            Without<SoulLinkColumn>,
            Without<SacrificeTriad>,
            Without<SoulLinkRune>,
            Without<SacrificeOrb>,
            HoldDisjoint,
        ),
    >,
) {
    let view = hold.view();
    for (column, mut transform, children) in columns.iter_mut() {
        let Some(body) = hold.body(column.warlock) else {
            continue;
        };
        let feet = body.point(0.0);
        transform.translation = feet;
        for child in children.iter() {
            let Ok(mut part) = runes.get_mut(child) else {
                continue;
            };
            let (drawn, k) = held_in(view, feet + part.translation, body.capsule());
            part.translation = drawn - feet;
            part.scale *= k;
        }
    }
    for (triad, mut transform, children) in triads.iter_mut() {
        let Some(body) = hold.body(triad.warlock) else {
            continue;
        };
        let chest = body.point(DEMONIC_SACRIFICE_CHEST_ABOVE_FLOOR);
        transform.translation = chest;
        let t = triad.age / sacrifice_time_scale();
        // The camera in the rig's frame (the rig is never turned).
        let eye = view
            .map(|c| c.eye - chest)
            .unwrap_or(Vec3::new(0.0, 40.0, 40.0));
        for child in children.iter() {
            if let Ok(mut part) = orbs.get_mut(child) {
                let (drawn, k) = held_in(view, chest + part.translation, body.capsule());
                part.translation = drawn - chest;
                part.scale *= k;
                if let Some(c) = view {
                    part.rotation = c.rotation;
                }
            } else if let Ok(ribbon) = ribbons.get(child) {
                // No hold: a ribbon's translation is the chest on the body's
                // own axis, above its centre, so it sorts in front of the
                // body from any camera above the horizon.
                if let Some(mesh) = meshes.get_mut(&ribbon.mesh) {
                    write_sacrifice_ribbon(mesh, &sacrifice_ribbon_trail(ribbon.index, t), eye);
                }
            }
        }
    }
    for (mut held_particle, mut transform) in particles.iter_mut() {
        held_particle.at = transform.translation;
        let Some(body) = hold.body(held_particle.body) else {
            continue;
        };
        let (drawn, k) = held_in(view, held_particle.at, body.capsule());
        transform.translation = drawn;
        transform.scale *= k;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_play_the_bench_at_its_defaults() {
        assert_eq!(soul_link_time_scale(), 1.0);
        assert_eq!(sacrifice_time_scale(), 1.0);
        assert!((soul_link_life() - 3.734).abs() < 1e-5);
    }

    #[test]
    fn the_spark_count_is_its_rate_over_its_second() {
        assert!((SOUL_LINK_SPARK_EMITTER.count_between(0.0, 5.0) - 56.0).abs() < 1e-4);
        // The inner track's area: 100 × (0.334 / 2 + 0.666 + 1.367 / 2).
        let inner = DEMONIC_SACRIFICE_INNER_MOTE_EMITTER.count_between(0.0, 5.0);
        assert!(
            (inner - 100.0 * (0.167 + 0.666 + 0.6835)).abs() < 1e-3,
            "{inner}"
        );
        assert!(
            (DEMONIC_SACRIFICE_OUTER_MOTE_EMITTER.count_between(0.0, 5.0) - 50.0 * 0.667).abs()
                < 1e-3
        );
    }
}
