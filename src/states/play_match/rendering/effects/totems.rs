use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::play_match::components::*;
use crate::states::play_match::map_config::ActiveMapGeometry;
use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use std::f32::consts::{FRAC_PI_8, TAU};
use std::sync::Arc;

use super::hunter_shots::{draw, rune_texture, ClientEmitter, EmitterAssets, EmitterSprite};
use super::spell_bolts::{soft_band_texture, soft_dot_texture};

// ==============================================================================
// Totem Visuals (Shaman, graphical-only)
// ==============================================================================
//
// The Classic client's element-totem template, from
// docs/design/2026-09-27-shaman-totems-client-data.md (AS-130). All four
// elements share ONE model in the client (`<element>elementaltotem.m2`, the
// same 267-vertex mesh and emitter rig, differing only by texture and tint),
// so they share one template here too: one post mesh, one rig, tinted by
// `TotemElement::color`. A new totem costs nothing.
//
// - **The post** — a carved post 1.0 yd tall (waist-high on a player), tapering,
//   with a winged crown near the top and a circular rune plate on its front
//   glowing in the element colour. One mesh for every element.
// - **The ribbons** — they ORBIT the post, one revolution a second, at 47% of
//   its height (the client hangs them off bones on a 1000 ms rotation loop). A
//   ribbon is the trail its edge leaves over its edge life, and because the
//   orbit is a rigid rotation at a constant rate, that trail is a FIXED arc in
//   the orbit's own frame. So each element's ribbons are one static mesh — an
//   arc fading from head to tail — under an entity that turns, and nothing is
//   rebuilt per frame.
// - **The top flame** — a small element-coloured flame, emitted at the top.
// - **Birth** (client sequence 127, 1.867 s) — the post rises into place with a
//   settling wobble.
// - **Death** (sequence 1, 1.2 s) — the flame goes out, the post tilts 18
//   degrees, an element-tinted dust burst kicks out at the base and an
//   orange-to-grey smoke puff rises. The smoke is ALPHA-blended, as the
//   client's is; everything else here is additive.
//
// Colours are OUR element authorities (the user's AS-130 ruling): the client's
// yellow-green Earth would alias Nature healing, its violet Air Arcane and
// Shadow. A standing totem is a world object with its own budget tier — it may
// carry standing motion — but it stays bound by the colour budget.
//
// **The death outlives the gameplay entity.** The sim despawns a totem the
// frame it expires, and — far more often — the frame its Shaman recasts the
// element (the AI refreshes below 5 s remaining, so most totems never reach
// expiry at all). The rig is therefore a top-level entity DETACHED from the
// totem: it notices the totem is gone and plays death from that frame, for
// expiry and replacement alike. Starting death 1.2 s before expiry instead
// would never play it for a replaced totem, which is most of them.
//
// The radius disc is NOT part of the rig. It is gameplay information the
// client does not draw — how far the buff reaches — so it is a child of the
// gameplay totem, full size from the first frame and gone the frame the buff
// is.
//
// Graphical-only: registered in `states/mod.rs` only, scattered by a
// deterministic hash (never `game_rng`), and writing nothing to the gameplay
// totem but the `Visibility` its disc child needs. Its `Transform` and `Totem`
// are never touched — the pulse system keys off them.

// ── The bench's knobs (AS-130 Totem Bench, signed off 2026-09-27) ──────────

/// Post height in yards (client mesh 3.25 yd at display scale 1.0; the
/// creature scale is server-side, so the user set this off an in-game
/// reference).
pub const TOTEM_POST_HEIGHT: f32 = 1.00;
/// Emissive gain on the rune plate.
pub const TOTEM_RUNE_GLOW: f32 = 0.80;
pub const TOTEM_RADIUS_DISC_ALPHA: f32 = 0.08;

/// Birth rise and death tilt (client sequences 127 and 1).
pub const TOTEM_BIRTH_SECS: f32 = 1.867;
pub const TOTEM_DEATH_SECS: f32 = 1.2;
pub const TOTEM_DEATH_TILT_DEG: f32 = 18.0;

/// Orbiting ribbons (client: bones on a 1000 ms global loop, radius 0.29 at
/// 47% height).
pub const TOTEM_RIBBON_ORBIT_RADIUS: f32 = 0.29;
pub const TOTEM_RIBBON_ORBIT_HEIGHT_FRAC: f32 = 0.47;
pub const TOTEM_RIBBON_REV_PER_SEC: f32 = 1.0;
/// Per element, indexed by [`element_slot`] (Air, Earth, Fire, Water): count,
/// half-height and edge life are the client's.
pub const TOTEM_RIBBON_COUNT: [u32; 4] = [2, 2, 3, 2];
pub const TOTEM_RIBBON_HALF_HEIGHT: [f32; 4] = [0.139, 0.194, 0.222, 0.139];
pub const TOTEM_RIBBON_LIFETIME: [f32; 4] = [0.51, 0.58, 0.53, 0.51];
pub const TOTEM_RIBBON_ALPHA: f32 = 0.80;
/// How far the ribbon tint is lightened from the element colour toward white.
pub const TOTEM_RIBBON_LIGHTEN: f32 = 0.35;

/// Top flame (client e0: 35/s, life 0.62 s, speed 0.91).
pub const TOTEM_FLAME_RATE: f32 = 35.0;
pub const TOTEM_FLAME_LIFETIME: f32 = 0.62;
pub const TOTEM_FLAME_SPEED: f32 = 0.91;
/// Where the flame is born, as a fraction of the post's height: in the knob,
/// as the client's sits at z 3.0-3.08 of its 3.41.
pub const TOTEM_FLAME_HEIGHT_FRAC: f32 = 0.92;
/// The flame keeps emitting this long into death (the client enables it for
/// the first 100 ms of the death sequence only).
pub const TOTEM_FLAME_DEATH_CUTOFF: f32 = 0.1;

/// Death: dust 33-433 ms at the base, orange-to-grey smoke 266-1166 ms.
pub const TOTEM_DEATH_DUST_RATE: f32 = 94.6;
pub const TOTEM_DEATH_DUST_WINDOW: (f32, f32) = (0.033, 0.433);
pub const TOTEM_DEATH_DUST_LIFETIME: f32 = 0.5;
pub const TOTEM_DEATH_DUST_SPEED: f32 = 2.78;
pub const TOTEM_DEATH_SMOKE_RATE: f32 = 32.0;
pub const TOTEM_DEATH_SMOKE_WINDOW: (f32, f32) = (0.266, 1.166);
pub const TOTEM_DEATH_SMOKE_LIFETIME: f32 = 2.0;

// ── The post's shape ───────────────────────────────────────────────────────
//
// Heights are fractions of `TOTEM_POST_HEIGHT`; radii are yards, as the bench
// drew them (its silhouette is taken from the client mesh's extents).

/// Radius at the base and at the top of the tapering shaft.
const SHAFT_BASE_RADIUS: f32 = 0.30;
const SHAFT_TOP_RADIUS: f32 = 0.22;
/// Where the shaft ends and the crown begins.
const SHAFT_TOP: f32 = 0.74;
/// The three carved bands around the shaft: height, and how far they stand
/// proud of it.
const BAND_HEIGHTS: [f32; 3] = [0.18, 0.36, 0.56];
const BAND_HALF_HEIGHT: f32 = 0.018;
const BAND_PROUD: f32 = 0.02;
/// The crown collar under the wings.
const CROWN_BOTTOM: f32 = 0.72;
const CROWN_TOP: f32 = 0.93;
const CROWN_TOP_RADIUS: f32 = 0.17;
/// The winged crown, `(x yd, height fraction)` around its outline, and half
/// its depth front to back.
const CROWN_OUTLINE: [(f32, f32); 8] = [
    (0.22, 0.72),
    (0.55, 0.78),
    (0.34, 0.84),
    (0.176, 0.93),
    (-0.176, 0.93),
    (-0.34, 0.84),
    (-0.55, 0.78),
    (-0.22, 0.72),
];
const CROWN_HALF_DEPTH: f32 = 0.09;
/// The head knob, closing to a point at the post's full height.
const KNOB_RADIUS: f32 = 0.13;
const KNOB_SHOULDER: f32 = 0.975;
/// Sides of every turned section. A FACE, not an edge, looks down +Z, so the
/// rune plate sits flat on one.
const POST_SIDES: usize = 8;
/// The post wood, before the element tint.
const POST_WOOD: [f32; 3] = [96.0, 72.0, 48.0];
/// How far the wood is tinted toward the element colour.
const POST_TINT: f32 = 0.18;

/// The rune plate: its height, its glyph's diameter, the soft glow's, and how
/// far it stands off the face.
const RUNE_HEIGHT_FRAC: f32 = 0.46;
const RUNE_DIAMETER: f32 = 0.34;
const RUNE_GLOW_DIAMETER: f32 = 0.56;
const RUNE_STANDOFF: f32 = 0.025;
/// Emissive gain of the plate before `TOTEM_RUNE_GLOW`.
const RUNE_GAIN: f32 = 3.0;

/// Arc segments per ribbon.
const RIBBON_SEGMENTS: usize = 24;
/// Emissive gain of the ribbons over their tint.
const RIBBON_GLOW: f32 = 1.5;

/// The birth rise completes at this fraction of the birth, leaving the rest to
/// the wobble settling; the wobble's peak, in degrees, and its half-cycles.
const BIRTH_RISE_FRAC: f32 = 0.7;
const BIRTH_WOBBLE_DEG: f32 = 6.0;
const BIRTH_WOBBLE_HALF_CYCLES: f32 = 4.0;

/// The bench drew each particle as a soft disc whose RADIUS was a multiple of
/// the client size key; these are the diameters that reproduce it.
const FLAME_SIZE: [f32; 3] = [0.218, 0.398, 0.0];
const FIRE_FLAME_SIZE: [f32; 3] = [0.451, 0.832, 0.328];
const DUST_SIZE: [f32; 3] = [0.512, 0.266, 0.112];
const SMOKE_SIZE: [f32; 3] = [0.656, 0.900, 1.372];

// ── The per-element tables ─────────────────────────────────────────────────

/// Index into the per-element ribbon tables, which follow the bench's
/// (alphabetical) order rather than [`TotemElement::index`].
pub fn element_slot(element: TotemElement) -> usize {
    match element {
        TotemElement::Air => 0,
        TotemElement::Earth => 1,
        TotemElement::Fire => 2,
        TotemElement::Water => 3,
    }
}

fn srgb_bytes(color: Color) -> [f32; 3] {
    let s = color.to_srgba();
    [s.red * 255.0, s.green * 255.0, s.blue * 255.0]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn to_u8(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| v.round().clamp(0.0, 255.0) as u8)
}

fn from_bytes(c: [f32; 3]) -> Color {
    Color::srgb(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0)
}

const WHITE: [f32; 3] = [255.0, 255.0, 255.0];

/// The ribbon tint: the element colour lightened toward white.
pub fn ribbon_tint(element: TotemElement) -> Color {
    from_bytes(mix(
        srgb_bytes(element.color()),
        WHITE,
        TOTEM_RIBBON_LIGHTEN,
    ))
}

/// The top flame: element-coloured, whitening as it burns out. Fire's is the
/// client's larger flame.
fn flame_emitter(element: TotemElement) -> ClientEmitter {
    let c = srgb_bytes(element.color());
    ClientEmitter {
        name: match element {
            TotemElement::Air => "totem_flame_air",
            TotemElement::Earth => "totem_flame_earth",
            TotemElement::Fire => "totem_flame_fire",
            TotemElement::Water => "totem_flame_water",
        },
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: TOTEM_FLAME_RATE,
        window: None,
        life: TOTEM_FLAME_LIFETIME,
        speed: TOTEM_FLAME_SPEED,
        gravity: 0.0,
        area: 0.14,
        column: None,
        rise: true,
        color: [
            to_u8(c),
            to_u8(mix(c, WHITE, 0.3)),
            to_u8(mix(c, WHITE, 0.6)),
        ],
        alpha: [0.58, 1.0, 0.0],
        size: if element == TotemElement::Fire {
            FIRE_FLAME_SIZE
        } else {
            FLAME_SIZE
        },
        mid: 0.5,
        track: None,
    }
}

/// The death dust (`dust5a`): element-tinted, whitening, at the base.
fn dust_emitter(element: TotemElement) -> ClientEmitter {
    let c = srgb_bytes(element.color());
    ClientEmitter {
        name: match element {
            TotemElement::Air => "totem_dust_air",
            TotemElement::Earth => "totem_dust_earth",
            TotemElement::Fire => "totem_dust_fire",
            TotemElement::Water => "totem_dust_water",
        },
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: TOTEM_DEATH_DUST_RATE,
        window: Some(TOTEM_DEATH_DUST_WINDOW),
        life: TOTEM_DEATH_DUST_LIFETIME,
        speed: TOTEM_DEATH_DUST_SPEED,
        gravity: 0.0,
        area: 0.22,
        column: None,
        rise: false,
        color: [to_u8(c), to_u8(mix(c, WHITE, 0.5)), [255, 255, 255]],
        alpha: [1.0, 1.0, 0.0],
        size: DUST_SIZE,
        mid: 0.5,
        track: None,
    }
}

/// The death smoke (`toonsmoke16_2`): orange to grey, alpha-blended, identical
/// for every element.
const SMOKE_EMITTER: ClientEmitter = ClientEmitter {
    name: "totem_death_smoke",
    sprite: EmitterSprite::Glow,
    additive: false,
    rate: TOTEM_DEATH_SMOKE_RATE,
    window: Some(TOTEM_DEATH_SMOKE_WINDOW),
    life: TOTEM_DEATH_SMOKE_LIFETIME,
    speed: 1.0,
    gravity: 0.0,
    area: 0.1,
    column: None,
    rise: true,
    color: [[255, 104, 22], [126, 126, 126], [174, 174, 174]],
    alpha: [0.8, 0.8, 0.0],
    size: SMOKE_SIZE,
    mid: 0.5,
    track: None,
};

/// Which of a rig's three emitters a particle comes from.
#[derive(Clone, Copy)]
enum Puff {
    Flame,
    Dust,
    Smoke,
}

// ── Meshes ─────────────────────────────────────────────────────────────────

/// A flat-shaded triangle soup. Every triangle is wound to face `outward`, so
/// the post survives back-face culling however a section was listed.
#[derive(Default)]
struct Soup {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
}

impl Soup {
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, outward: Vec3) {
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 {
            return; // degenerate (a cone's apex)
        }
        let (b, c, n) = if n.dot(outward) < 0.0 {
            (c, b, -n)
        } else {
            (b, c, n)
        };
        let n = n.normalize();
        for p in [a, b, c] {
            self.positions.push(p.to_array());
            self.normals.push(n.to_array());
        }
    }

    fn ring(y: f32, r: f32, i: usize) -> Vec3 {
        let a = FRAC_PI_8 + i as f32 * TAU / POST_SIDES as f32;
        Vec3::new(r * a.cos(), y, r * a.sin())
    }

    /// A closed eight-sided frustum from `(y0, r0)` up to `(y1, r1)`.
    fn frustum(&mut self, y0: f32, r0: f32, y1: f32, r1: f32) {
        for i in 0..POST_SIDES {
            let (b0, b1) = (Self::ring(y0, r0, i), Self::ring(y0, r0, i + 1));
            let (t0, t1) = (Self::ring(y1, r1, i), Self::ring(y1, r1, i + 1));
            let mid = (b0 + b1 + t0 + t1) * 0.25;
            let out = Vec3::new(mid.x, 0.0, mid.z);
            self.tri(b0, t0, b1, out);
            self.tri(b1, t0, t1, out);
            self.tri(Vec3::new(0.0, y0, 0.0), b0, b1, Vec3::NEG_Y);
            self.tri(Vec3::new(0.0, y1, 0.0), t0, t1, Vec3::Y);
        }
    }

    /// `outline` (x, y) extruded `half_depth` either side of z = 0.
    fn prism(&mut self, outline: &[(f32, f32)], half_depth: f32) {
        let n = outline.len();
        let centre = outline
            .iter()
            .fold(Vec2::ZERO, |s, &(x, y)| s + Vec2::new(x, y))
            / n as f32;
        let at = |i: usize, z: f32| Vec3::new(outline[i % n].0, outline[i % n].1, z);
        let c = |z: f32| Vec3::new(centre.x, centre.y, z);
        for i in 0..n {
            self.tri(
                c(half_depth),
                at(i, half_depth),
                at(i + 1, half_depth),
                Vec3::Z,
            );
            self.tri(
                c(-half_depth),
                at(i, -half_depth),
                at(i + 1, -half_depth),
                Vec3::NEG_Z,
            );
            let edge_mid = (at(i, 0.0) + at(i + 1, 0.0)) * 0.5;
            let out = edge_mid - c(0.0);
            self.tri(
                at(i, -half_depth),
                at(i, half_depth),
                at(i + 1, -half_depth),
                out,
            );
            self.tri(
                at(i + 1, -half_depth),
                at(i, half_depth),
                at(i + 1, half_depth),
                out,
            );
        }
    }

    fn into_mesh(self) -> Mesh {
        let uvs = vec![[0.0, 0.0]; self.positions.len()];
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    }
}

/// The shaft's radius at height fraction `f`.
fn shaft_radius(f: f32) -> f32 {
    SHAFT_BASE_RADIUS + (SHAFT_TOP_RADIUS - SHAFT_BASE_RADIUS) * (f / SHAFT_TOP)
}

/// The carved post every element shares: its base on y = 0, its knob's point
/// at exactly `height`.
pub fn totem_post_mesh(height: f32) -> Mesh {
    let h = |f: f32| f * height;
    let mut soup = Soup::default();
    soup.frustum(0.0, SHAFT_BASE_RADIUS, h(SHAFT_TOP), SHAFT_TOP_RADIUS);
    for f in BAND_HEIGHTS {
        let r = shaft_radius(f) + BAND_PROUD;
        soup.frustum(h(f - BAND_HALF_HEIGHT), r, h(f + BAND_HALF_HEIGHT), r);
    }
    soup.frustum(
        h(CROWN_BOTTOM),
        SHAFT_TOP_RADIUS,
        h(CROWN_TOP),
        CROWN_TOP_RADIUS,
    );
    let wings: Vec<(f32, f32)> = CROWN_OUTLINE.iter().map(|&(x, f)| (x, h(f))).collect();
    soup.prism(&wings, CROWN_HALF_DEPTH);
    soup.frustum(h(CROWN_TOP), KNOB_RADIUS, h(KNOB_SHOULDER), KNOB_RADIUS);
    soup.frustum(h(KNOB_SHOULDER), KNOB_RADIUS, height, 0.0);
    soup.into_mesh()
}

/// Where the rune plate stands off the post's front face.
fn rune_plate_z() -> f32 {
    shaft_radius(RUNE_HEIGHT_FRAC) * FRAC_PI_8.cos() + RUNE_STANDOFF
}

/// An element's ribbons, in the orbit's own frame: `count` arcs, evenly
/// spaced, each running from its head at angle `k * TAU / count` back along
/// the orbit by the distance it travels in one edge life, fading from head to
/// tail. The orbit turns this whole mesh at `TOTEM_RIBBON_REV_PER_SEC`, which
/// is exactly the trail a client ribbon edge leaves.
///
/// A point at angle `phi` sits at `(r cos phi, y, -r sin phi)`: the frame
/// `Quat::from_rotation_y(theta)` carries to angle `phi + theta`, so the
/// orbit's turning advances the head and the tail trails behind it.
pub fn totem_ribbon_mesh(element: TotemElement) -> Mesh {
    let slot = element_slot(element);
    let count = TOTEM_RIBBON_COUNT[slot] as usize;
    let half = TOTEM_RIBBON_HALF_HEIGHT[slot];
    let sweep = TAU * TOTEM_RIBBON_REV_PER_SEC * TOTEM_RIBBON_LIFETIME[slot];
    let r = TOTEM_RIBBON_ORBIT_RADIUS;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for k in 0..count {
        let head = k as f32 * TAU / count as f32;
        let base = positions.len() as u32;
        for i in 0..=RIBBON_SEGMENTS {
            let along = i as f32 / RIBBON_SEGMENTS as f32;
            let phi = head - sweep * along;
            let (x, z) = (r * phi.cos(), -r * phi.sin());
            let fade = 1.0 - along;
            for (y, v) in [(-half, 0.0), (half, 1.0)] {
                positions.push([x, y, z]);
                normals.push([phi.cos(), 0.0, -phi.sin()]);
                uvs.push([along, v]);
                colors.push([1.0, 1.0, 1.0, fade]);
            }
        }
        for i in 0..RIBBON_SEGMENTS as u32 {
            let (b0, t0, b1, t1) = (
                base + 2 * i,
                base + 2 * i + 1,
                base + 2 * i + 2,
                base + 2 * i + 3,
            );
            indices.extend_from_slice(&[b0, t0, b1, b1, t0, t1]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// Build a flat ground disc of `radius` centered at `center` (world XZ), clipped
/// to `bounds` so it never spills past the arena walls. Vertices are in LOCAL
/// space (offsets from `center`) lying in the XZ plane at y=0, so the mesh can be
/// parented to an entity sitting at `center`. Reusable by any ground decal that
/// must stay inside the arena.
///
/// Clipping is a per-direction march against [`ArenaBounds::contains`], which is
/// shape-agnostic: this used to be eight hard-coded octagon half-planes, which
/// silently collapsed the disc to zero radius on Nagrand's bowl (any totem outside
/// the retired 76×46 rectangle failed every plane test at once). The disc now
/// stops at the walkable edge rather than exactly at the wall — a `WALL_OFFSET`
/// (1.5yd) inset, and the only bound that holds for every shape.
fn arena_clipped_disc_mesh(bounds: &ArenaBounds, center: Vec2, radius: f32) -> Mesh {
    const SEGMENTS: usize = 96;
    /// Radial march step. Fine enough that the clip reads as a clean edge on a
    /// decal this faint.
    const STEP: f32 = 0.2;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(SEGMENTS + 1);
    let mut indices: Vec<u32> = Vec::with_capacity(SEGMENTS * 3);
    positions.push([0.0, 0.0, 0.0]); // fan center (index 0)
    for i in 0..SEGMENTS {
        let a = (i as f32) / (SEGMENTS as f32) * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        // March outward until the point leaves the arena, capped at `radius`.
        let mut t = 0.0_f32;
        while t + STEP <= radius {
            let probe = center + dir * (t + STEP);
            if !bounds.contains(Vec3::new(probe.x, 1.0, probe.y)) {
                break;
            }
            t += STEP;
        }
        let p = dir * t;
        positions.push([p.x, 0.0, p.y]); // dir.y maps to world/local Z
    }
    for i in 0..SEGMENTS {
        indices.push(0);
        indices.push(1 + i as u32);
        indices.push(1 + ((i + 1) % SEGMENTS) as u32);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs = vec![[0.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

// ── Shared assets ──────────────────────────────────────────────────────────

/// Everything one element's totems share.
struct ElementAssets {
    post_material: Handle<StandardMaterial>,
    rune_material: Handle<StandardMaterial>,
    rune_glow_material: Handle<StandardMaterial>,
    ribbon_mesh: Handle<Mesh>,
    ribbon_material: StandardMaterial,
    palettes: [Arc<[Handle<StandardMaterial>]>; 3],
}

/// Meshes and materials every totem shares, built once. Only the per-rig
/// ribbon material is ever mutated, and each rig has its own copy of it.
/// `pub` only because it appears in a `Local<..>` on a `pub` system.
pub struct TotemAssets {
    post_mesh: Handle<Mesh>,
    quad: Handle<Mesh>,
    elements: [ElementAssets; 4],
}

impl TotemAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        let mut emitters = EmitterAssets::build(meshes, images);
        let rune = images.add(rune_texture());
        let dot = images.add(soft_dot_texture());
        let band = images.add(soft_band_texture());
        let glow = |materials: &mut Assets<StandardMaterial>,
                    texture: &Handle<Image>,
                    color: Color,
                    alpha: f32,
                    gain: f32| {
            let c = color.to_linear();
            materials.add(StandardMaterial {
                base_color: color.with_alpha(alpha),
                base_color_texture: Some(texture.clone()),
                emissive: LinearRgba::rgb(c.red * gain, c.green * gain, c.blue * gain),
                emissive_texture: Some(texture.clone()),
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        };
        let mut element = |element: TotemElement| {
            let color = element.color();
            let tint = ribbon_tint(element);
            let t = tint.to_linear();
            let smoke = emitters.palette(materials, &SMOKE_EMITTER);
            ElementAssets {
                post_material: materials.add(StandardMaterial {
                    base_color: from_bytes(mix(POST_WOOD, srgb_bytes(color), POST_TINT)),
                    perceptual_roughness: 0.8,
                    ..default()
                }),
                rune_material: glow(materials, &rune, color, 1.0, RUNE_GAIN * TOTEM_RUNE_GLOW),
                rune_glow_material: glow(materials, &dot, color, 0.85, TOTEM_RUNE_GLOW),
                ribbon_mesh: meshes.add(totem_ribbon_mesh(element)),
                ribbon_material: StandardMaterial {
                    base_color: tint.with_alpha(TOTEM_RIBBON_ALPHA),
                    base_color_texture: Some(band.clone()),
                    emissive: LinearRgba::rgb(
                        t.red * RIBBON_GLOW,
                        t.green * RIBBON_GLOW,
                        t.blue * RIBBON_GLOW,
                    ),
                    emissive_texture: Some(band.clone()),
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                },
                palettes: [
                    emitters.palette(materials, &flame_emitter(element)),
                    emitters.palette(materials, &dust_emitter(element)),
                    smoke,
                ],
            }
        };
        let elements = [
            element(TotemElement::Air),
            element(TotemElement::Earth),
            element(TotemElement::Fire),
            element(TotemElement::Water),
        ];
        Self {
            post_mesh: meshes.add(totem_post_mesh(TOTEM_POST_HEIGHT)),
            quad: emitters.quad(),
            elements,
        }
    }

    fn element(&self, element: TotemElement) -> &ElementAssets {
        &self.elements[element_slot(element)]
    }
}

// ── The lifecycle, as pure functions of the rig's clocks ───────────────────

/// How far the post has risen, 0..1, `age` seconds into its life: an ease-out
/// over the first `BIRTH_RISE_FRAC` of the birth.
pub fn birth_rise(age: f32) -> f32 {
    let k = (age / (TOTEM_BIRTH_SECS * BIRTH_RISE_FRAC)).clamp(0.0, 1.0);
    1.0 - (1.0 - k).powi(3)
}

/// The post's lean, radians, about its local Z: the birth's settling wobble,
/// then — once dying — the death tilt, growing linearly to
/// `TOTEM_DEATH_TILT_DEG`.
pub fn totem_lean(age: f32, death_age: Option<f32>) -> f32 {
    if let Some(d) = death_age {
        return (TOTEM_DEATH_TILT_DEG * (d / TOTEM_DEATH_SECS).clamp(0.0, 1.0)).to_radians();
    }
    let k = (age / TOTEM_BIRTH_SECS).clamp(0.0, 1.0);
    let wobble = (k * std::f32::consts::PI * BIRTH_WOBBLE_HALF_CYCLES).sin() * (1.0 - k);
    (BIRTH_WOBBLE_DEG * wobble).to_radians()
}

/// The ribbons' orbit angle, radians, `age` seconds into the totem's life.
pub fn ribbon_angle(age: f32) -> f32 {
    (TAU * TOTEM_RIBBON_REV_PER_SEC * age).rem_euclid(TAU)
}

// ── Systems ────────────────────────────────────────────────────────────────

/// Build the visuals on newly spawned totems: the radius disc on the gameplay
/// totem itself (clipped to the arena walls), and the detached rig that draws
/// the post, ribbons and flame. Registered ONLY in `StatesPlugin::build` —
/// never in `add_core_combat_systems`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_totem_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<TotemAssets>>,
    // Per-map arena shape, for clipping the buff-radius decal to the real walls.
    // `Option` so a scene without the resource simply skips the map-aware clip.
    map_geometry: Option<Res<ActiveMapGeometry>>,
    new_totems: Query<(Entity, &Totem, &Transform), (Added<Totem>, Without<Children>)>,
) {
    if new_totems.is_empty() {
        return;
    }
    let assets =
        assets.get_or_insert_with(|| TotemAssets::build(&mut meshes, &mut materials, &mut images));
    let bounds = map_geometry.as_ref().map(|g| g.bounds).unwrap_or_default();
    for (totem_entity, totem, transform) in new_totems.iter() {
        let color = totem.element.color();
        let s = color.to_srgba();
        let element = assets.element(totem.element);

        // Very subtle ground disc marking the buff radius, clipped to the active
        // map's walkable region so it never spills past the walls. `Add` blend per
        // the project's ground-indicator convention to avoid z-fighting flicker.
        let disc_mesh = meshes.add(arena_clipped_disc_mesh(
            &bounds,
            transform.translation.xz(),
            totem.radius,
        ));
        let disc_mat = materials.add(StandardMaterial {
            base_color: color.with_alpha(TOTEM_RADIUS_DISC_ALPHA),
            emissive: LinearRgba::new(s.red * 0.18, s.green * 0.18, s.blue * 0.18, 1.0),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        });
        // The core-spawned Totem entity has only Transform/Totem (no visibility
        // components). Give it `Visibility` (which pulls in InheritedVisibility +
        // ViewVisibility) so the disc inherits a valid visibility chain —
        // otherwise Bevy logs B0004 for every totem.
        commands
            .entity(totem_entity)
            .try_insert(Visibility::default())
            .with_children(|parent| {
                // a hair above the floor so it doesn't z-fight it
                parent.spawn((
                    Mesh3d(disc_mesh),
                    MeshMaterial3d(disc_mat),
                    Transform::from_xyz(0.0, 0.03, 0.0),
                ));
            });

        // The rig. Everything starts at its birth pose (risen 0), so nothing
        // draws a frame at full height before the rise begins.
        let post = commands
            .spawn((
                TotemPart::Post,
                Mesh3d(assets.post_mesh.clone()),
                MeshMaterial3d(element.post_material.clone()),
                Transform::from_scale(Vec3::new(1.0, 1e-3, 1.0)),
            ))
            .id();
        let rune_glow = commands
            .spawn((
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(element.rune_glow_material.clone()),
                Transform::from_scale(Vec3::splat(RUNE_GLOW_DIAMETER)),
                NotShadowCaster,
            ))
            .id();
        let rune = commands
            .spawn((
                TotemPart::Rune,
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(element.rune_material.clone()),
                // `Rectangle` faces +Z: flat on the post's front face.
                Transform::from_xyz(0.0, 0.0, rune_plate_z())
                    .with_scale(Vec3::splat(RUNE_DIAMETER)),
                NotShadowCaster,
            ))
            .add_child(rune_glow)
            .id();
        let ribbon_material = materials.add(element.ribbon_material.clone());
        let orbit = commands
            .spawn((
                TotemPart::Orbit,
                Mesh3d(element.ribbon_mesh.clone()),
                MeshMaterial3d(ribbon_material.clone()),
                Transform::default(),
                NotShadowCaster,
            ))
            .id();
        let body = commands
            .spawn((TotemPart::Body, Transform::default(), Visibility::default()))
            .add_children(&[post, rune, orbit])
            .id();
        commands
            .spawn((
                TotemRig {
                    totem: totem_entity,
                    element: totem.element,
                    age: 0.0,
                    death_age: None,
                    body,
                    post,
                    rune,
                    orbit,
                    ribbon_material,
                    seed: totem_entity.index(),
                    emitted: 0,
                    carry: [0.0; 3],
                    quad: assets.quad.clone(),
                    palettes: element.palettes.clone(),
                },
                Transform::from_translation(transform.translation),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .add_child(body);
    }
}

/// Pose every rig for its clocks — the rise, the wobble or death tilt, the
/// orbit — emit its flame, dust and smoke, and start its death the frame its
/// totem is gone.
#[allow(clippy::too_many_arguments)]
pub fn update_totem_visuals(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    totems: Query<(), With<Totem>>,
    camera: Query<&Transform, (With<Camera3d>, Without<TotemRig>, Without<TotemPart>)>,
    mut rigs: Query<(&mut TotemRig, &Transform), Without<TotemPart>>,
    mut parts: Query<&mut Transform, (With<TotemPart>, Without<TotemRig>, Without<Camera3d>)>,
) {
    let dt = time.delta_secs();
    let facing = camera.iter().next().map(|c| c.rotation).unwrap_or_default();
    for (mut rig, rig_transform) in rigs.iter_mut() {
        rig.age += dt;
        let previous_death = rig.death_age;
        if let Some(d) = rig.death_age.as_mut() {
            *d += dt;
        } else if totems.get(rig.totem).is_err() {
            rig.death_age = Some(0.0);
        }
        let (age, death) = (rig.age, rig.death_age);

        let rise = birth_rise(age);
        let height = TOTEM_POST_HEIGHT * rise;
        let lean = Quat::from_rotation_z(totem_lean(age, death));
        if let Ok(mut body) = parts.get_mut(rig.body) {
            body.rotation = lean;
        }
        if let Ok(mut post) = parts.get_mut(rig.post) {
            post.scale = Vec3::new(1.0, rise.max(1e-3), 1.0);
        }
        if let Ok(mut rune) = parts.get_mut(rig.rune) {
            rune.translation.y = height * RUNE_HEIGHT_FRAC;
        }
        if let Ok(mut orbit) = parts.get_mut(rig.orbit) {
            orbit.translation.y = height * TOTEM_RIBBON_ORBIT_HEIGHT_FRAC;
            // A dying totem's ribbons stop where they are and fade over one
            // edge life: no new edge is laid, and the last ones age out.
            if death.is_none() {
                orbit.rotation = Quat::from_rotation_y(ribbon_angle(age));
            }
        }
        if let Some(d) = death {
            let life = TOTEM_RIBBON_LIFETIME[element_slot(rig.element)];
            let fade = (1.0 - d / life).clamp(0.0, 1.0);
            if let Some(m) = materials.get_mut(&rig.ribbon_material) {
                m.base_color.set_alpha(TOTEM_RIBBON_ALPHA * fade);
            }
        }

        // Emission, integrated over this frame's slice of each window.
        let base = rig_transform.translation;
        let top = base + lean * Vec3::Y * (height * TOTEM_FLAME_HEIGHT_FRAC);
        let flame = match death {
            None => TOTEM_FLAME_RATE * dt,
            Some(d) => {
                let from = previous_death.unwrap_or(0.0);
                TOTEM_FLAME_RATE * (d.min(TOTEM_FLAME_DEATH_CUTOFF) - from).max(0.0)
            }
        };
        let (dust, smoke) = match death {
            Some(d) => {
                let from = previous_death.unwrap_or(0.0);
                let e = dust_emitter(rig.element);
                (
                    e.count_between(from, d),
                    SMOKE_EMITTER.count_between(from, d),
                )
            }
            None => (0.0, 0.0),
        };
        for (slot, (owed, puff)) in [
            (flame, Puff::Flame),
            (dust, Puff::Dust),
            (smoke, Puff::Smoke),
        ]
        .into_iter()
        .enumerate()
        {
            rig.carry[slot] += owed;
            while rig.carry[slot] >= 1.0 {
                rig.carry[slot] -= 1.0;
                let seed = rig
                    .seed
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let (at, velocity, e) = match puff {
                    Puff::Flame => {
                        let (off, v) = flame_launch(seed);
                        (top + off, v, flame_emitter(rig.element))
                    }
                    Puff::Dust => {
                        let (off, v) = dust_launch(seed);
                        (base + off, v, dust_emitter(rig.element))
                    }
                    Puff::Smoke => {
                        let (off, v) = smoke_launch(seed);
                        (base + off, v, SMOKE_EMITTER)
                    }
                };
                let palette = &rig.palettes[slot];
                commands.spawn((
                    ClientParticle {
                        age: 0.0,
                        life: e.life,
                        velocity,
                        gravity: e.gravity,
                        size: e.size,
                        mid: e.mid,
                        palette: palette.clone(),
                        step: 0,
                        facing: ParticleFacing::Camera,
                        owned: false,
                    },
                    Mesh3d(rig.quad.clone()),
                    MeshMaterial3d(palette[0].clone()),
                    Transform::from_translation(at)
                        .with_rotation(facing)
                        .with_scale(Vec3::splat(e.size[0].max(1e-4))),
                    NotShadowCaster,
                    PlayMatchEntity,
                ));
            }
        }
    }
}

/// Retire every rig whose death has played out. Its particles are world-space
/// and see themselves out.
pub fn cleanup_totem_visuals(mut commands: Commands, rigs: Query<(Entity, &TotemRig)>) {
    for (entity, rig) in rigs.iter() {
        if rig.death_age.is_some_and(|d| d >= TOTEM_DEATH_SECS) {
            commands.entity(entity).try_despawn();
        }
    }
}

// ── Particle launches (deterministic hash scatter, never `game_rng`) ───────

/// A flame lick: born in a small ring in the knob, rising with a little drift.
fn flame_launch(seed: u32) -> (Vec3, Vec3) {
    let a = draw(seed, 1) * TAU;
    let r = draw(seed, 2) * 0.07;
    let offset = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
    let velocity = Vec3::new(
        (draw(seed, 3) - 0.5) * 0.5,
        TOTEM_FLAME_SPEED * (0.6 + 0.4 * draw(seed, 4)),
        (draw(seed, 5) - 0.5) * 0.5,
    );
    (offset, velocity)
}

/// A dust mote: kicked outward along the ground from the base, a little up.
fn dust_launch(seed: u32) -> (Vec3, Vec3) {
    let a = draw(seed, 1) * TAU;
    let speed = TOTEM_DEATH_DUST_SPEED * (0.5 + 0.5 * draw(seed, 2));
    let offset = Vec3::new(
        (draw(seed, 3) - 0.5) * 0.22,
        0.05,
        (draw(seed, 4) - 0.5) * 0.22,
    );
    let velocity = Vec3::new(a.cos() * speed, draw(seed, 5) * 0.8, a.sin() * speed);
    (offset, velocity)
}

/// A smoke puff: born up the lower half of the post, rising and spreading.
fn smoke_launch(seed: u32) -> (Vec3, Vec3) {
    let a = draw(seed, 1) * TAU;
    let offset = Vec3::new(
        (draw(seed, 2) - 0.5) * 0.1,
        0.2 + draw(seed, 3) * TOTEM_POST_HEIGHT * 0.5,
        (draw(seed, 4) - 0.5) * 0.1,
    );
    let velocity = Vec3::new(a.cos() * 0.6, 1.0, a.sin() * 0.6);
    (offset, velocity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::mesh::VertexAttributeValues;

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(p)) => {
                p.iter().map(|&v| Vec3::from_array(v)).collect()
            }
            _ => panic!("no positions"),
        }
    }

    #[test]
    fn the_post_stands_on_the_ground_and_is_exactly_its_height() {
        let p = positions(&totem_post_mesh(TOTEM_POST_HEIGHT));
        let (lo, hi) = p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
            (lo.min(v.y), hi.max(v.y))
        });
        assert!(lo.abs() < 1e-6, "base at {lo}");
        assert!((hi - TOTEM_POST_HEIGHT).abs() < 1e-6, "top at {hi}");
    }

    #[test]
    fn every_post_face_points_out_of_the_solid() {
        // Culled with the default back-face cull, an inward face is a hole. The
        // turned sections face away from the axis (or up/down on a cap), the
        // wing prism away from its own centre (or front/back).
        let mesh = totem_post_mesh(TOTEM_POST_HEIGHT);
        let p = positions(&mesh);
        for tri in p.chunks(3) {
            let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize();
            let c = (tri[0] + tri[1] + tri[2]) / 3.0;
            let radial = Vec3::new(c.x, 0.0, c.z);
            let is_cap = n.y.abs() > 0.99;
            let is_wing_face = n.z.abs() > 0.99;
            if is_cap || is_wing_face {
                continue;
            }
            assert!(n.dot(radial) > -1e-4, "inward face at {c:?}, normal {n:?}");
        }
    }

    #[test]
    fn a_ribbon_trails_its_head_by_one_edge_life_of_orbit() {
        for element in TotemElement::ALL {
            let slot = element_slot(element);
            let mesh = totem_ribbon_mesh(element);
            let p = positions(&mesh);
            let per_ribbon = 2 * (RIBBON_SEGMENTS + 1);
            assert_eq!(p.len(), per_ribbon * TOTEM_RIBBON_COUNT[slot] as usize);
            for v in &p {
                let r = Vec2::new(v.x, v.z).length();
                assert!((r - TOTEM_RIBBON_ORBIT_RADIUS).abs() < 1e-5);
                assert!((v.y.abs() - TOTEM_RIBBON_HALF_HEIGHT[slot]).abs() < 1e-6);
            }
            // The head of ribbon 0 sits at angle 0; its tail one edge life of
            // orbit behind, turning the way the orbit turns (-z at +angle).
            let angle = |v: Vec3| (-v.z).atan2(v.x);
            let head = angle(p[0]);
            let tail = angle(p[per_ribbon - 1]);
            let sweep = TAU * TOTEM_RIBBON_REV_PER_SEC * TOTEM_RIBBON_LIFETIME[slot];
            assert!(head.abs() < 1e-5);
            assert!(
                ((head - tail).rem_euclid(TAU) - sweep.rem_euclid(TAU)).abs() < 1e-4,
                "{element:?}"
            );
        }
    }

    #[test]
    fn the_emitter_tables_carry_the_signed_off_values() {
        for element in TotemElement::ALL {
            let f = flame_emitter(element);
            assert_eq!((f.rate, f.life), (35.0, 0.62));
            let d = dust_emitter(element);
            assert_eq!(d.window, Some((0.033, 0.433)));
            // ~38 motes a death at 94.6/s over 400 ms.
            assert!((d.count_between(0.0, 2.0) - 94.6 * 0.4).abs() < 1e-3);
        }
        assert!(
            !SMOKE_EMITTER.additive,
            "the client's smoke is alpha-blended"
        );
        assert_eq!(flame_emitter(TotemElement::Fire).size, FIRE_FLAME_SIZE);
    }
}
