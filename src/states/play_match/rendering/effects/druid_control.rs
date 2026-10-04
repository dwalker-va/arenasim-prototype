use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use std::f32::consts::TAU;

use super::hard_cc::cc_jitter;
use crate::states::play_match::components::*;

// ==============================================================================
// Druid control — Entangling Roots' bark roots and Cyclone's funnel
// ==============================================================================
//
// Two receiver-side treatments, both built to the AS-160 bench sign-off
// (`docs/design/2026-10-03-druid-client-data.md`, *Bench sign-off*) and ported
// from the bench's drawing code (`docs/design/benches/2026-10-03-druid-visuals-
// bench.html`).
//
// * **Entangling Roots** is `RootStyle::Roots`: six tapered, curling bark roots
//   (`entanglingroots_state.m2`, kit 66) that grow up along their own length out
//   of the ground around the victim over the model's 1.5s birth, kicking up
//   brown dust and pebbles while they grow, hold, and withdraw over its 1.3s
//   death on every exit. The rig itself is the hard-CC treatment's `CcRig`
//   (`hard_cc.rs` spawns it, flares it and arms its retract); this module only
//   builds the roots and drives their growth.
//
// * **Cyclone** borrows the wind-vortex kit 861 (`cyclone_state.m2`, from
//   Enveloping Winds — the Era client has no Druid Cyclone). Its funnel is a
//   stream of `cyclonegeo` STRIP MESHES: each particle is a ~200° twisted cloud
//   band round the victim that rises and grows over its life, so the bands stack
//   into a funnel narrow at the waist and wide at the top. Dark core (P0), body
//   (P1+P2) and crown (P3) bands, toon smoke at the feet (P5) and pebbles kicked
//   up at the 2.54yd rim (P6-P10). The violet-tipped skirt (P4) is off.
//
//   The victim is lifted 0.35yd, bobs and spins slowly through its OWN
//   transform channel, [`CycloneLift`] on the `VisualBody`. It never touches the
//   `OriginalMesh` / `OriginalBodyMaterial` restore slots, so a Fear that landed
//   before the Cyclone (the Cyclone's immunity blocks only NEW auras) keeps its
//   husk tint on the lifted body, and either can end first.
//
// Graphical-only: registered in `states/mod.rs` alone, never in
// `add_core_combat_systems`. Every random draw is the `cc_jitter` hash, never
// `game_rng`, and nothing here writes sim state — headless stays byte-identical
// by construction.

// ------------------------------------------------------------------------------
// Entangling Roots — entanglingroots_state.m2 (kit 66), AS-160 bench sign-off
// ------------------------------------------------------------------------------

/// The RON `name:` of the root aura that wears bark roots rather than the
/// school's default (it shares Nature with Spider Web's silk).
pub const ENTANGLING_ROOTS_AURA: &str = "Entangling Roots";

pub const ROOTS_BIRTH_SECS: f32 = 1.5;
pub const ROOTS_DEATH_SECS: f32 = 1.3;
pub const ROOTS_COUNT: usize = 6;
pub const ROOTS_HEIGHT: f32 = 1.10;
pub const ROOTS_RING_RADIUS: f32 = 0.60;
pub const ROOTS_CURL: f32 = 0.80;
/// Diameter at the base; each root tapers to a point at its tip.
pub const ROOTS_BASE_THICKNESS: f32 = 0.110;
pub const ROOTS_BARK: Color = Color::srgb(0.376, 0.275, 0.165);
/// roots, dust, pebbles
pub const ROOTS_LAYERS: [bool; 3] = [true, true, true];
pub const ROOTS_DUST_SIZE_MUL: f32 = 1.00;
pub const ROOTS_DUST_DENSITY_MUL: f32 = 1.00;

/// Segments per root — the bench's 11-point polyline.
pub const ROOT_SEGMENTS: usize = 10;

/// Dust puffs (P0+P2 `dust1`): 30/s through the birth, stopping at 1500ms.
const ROOT_DUST_RATE: f32 = 30.0;
const ROOT_DUST_SPEED: f32 = 0.278;
const ROOT_DUST_CONE: f32 = 1.571;
const ROOT_DUST_LIFE: f32 = 2.0;
const ROOT_DUST_AREA: f32 = 1.0;
const ROOT_DUST_SPIN_REV: f32 = 0.4;
/// Pebbles (P1 `partrock`): rate 0 -> 20 -> 10 -> 0 over the birth, gravity 4.2.
const ROOT_PEBBLE_RATE: [(f32, f32); 4] = [(0.0, 0.0), (0.167, 20.0), (0.834, 10.0), (1.5, 0.0)];
const ROOT_PEBBLE_SPEED: f32 = 3.333;
const ROOT_PEBBLE_CONE: f32 = 1.222;
const ROOT_PEBBLE_LIFE: f32 = 3.0;
const ROOT_PEBBLE_GRAVITY: f32 = 4.167;
const ROOT_PEBBLE_SPIN_REV: f32 = -0.4;
/// Pebbles come to rest here rather than falling through the floor.
const ROOT_PEBBLE_FLOOR: f32 = 0.02;
/// Height above the ground both birth emitters spawn at.
const ROOT_EMIT_Y: f32 = 0.1;

// ------------------------------------------------------------------------------
// Cyclone — cyclone_state.m2 (kit 861, borrowed from Enveloping Winds), AS-160
// bench sign-off
// ------------------------------------------------------------------------------

/// dark core, body, crown, violet skirt, smoke, rim
pub const CYCLONE_LAYERS: [bool; 6] = [true, true, true, false, true, true];
pub const CYCLONE_BAND_SIZE_MUL: f32 = 1.00;
pub const CYCLONE_BAND_RISE_MUL: f32 = 1.00;
pub const CYCLONE_BAND_LIFE_MUL: f32 = 1.00;
pub const CYCLONE_DENSITY_MUL: f32 = 1.00;
pub const CYCLONE_FUNNEL_SPIN_REV_PER_SEC: f32 = 1.20;
pub const CYCLONE_DARKNESS: f32 = 0.55;
pub const CYCLONE_OPACITY: f32 = 0.80;
pub const CYCLONE_VICTIM_LIFT: f32 = 0.35;
pub const CYCLONE_VICTIM_SPIN_REV_PER_SEC: f32 = 0.75;
pub const CYCLONE_VICTIM_BOB: f32 = 0.08;

/// The victim eases up into the lift over this long, and back down on exit;
/// the funnel fades out over the same span.
pub const CYCLONE_EASE_SECS: f32 = 0.6;
/// The bob's angular rate — the bench's `sin(ms / 400)`.
const CYCLONE_BOB_RATE: f32 = 2.5;

/// The bench draws each band at `band size × 1.25` and its alpha at
/// `opacity × 0.55`; both factors are part of the signed-off look.
const CYCLONE_BAND_SCALE: f32 = 1.25;
const CYCLONE_BAND_ALPHA: f32 = 0.55;
/// The band's centre sits `0.9 × scale` above its emitter height (bench `yc`).
const CYCLONE_BAND_CENTRE: f32 = 0.9;

/// The strip's span: from 122° round the axis through 198° more (`cyclonegeo`).
const STRIP_A0_DEG: f32 = 122.0;
const STRIP_SPAN_DEG: f32 = 198.0;
/// Columns across the strip. The edge keys are piecewise-linear in `u`, so this
/// only smooths the arc between them.
const STRIP_COLUMNS: usize = 20;
/// The strip's edges, keyed on the fraction `u` along its span, read off the
/// `cyclonegeo` vertices: the bottom edge dips then climbs, the top edge
/// climbs, and the two meet at a tip at each end.
const STRIP_BOTTOM: [(f32, f32); 7] = [
    (0.0, -0.55),
    (0.16, -0.86),
    (0.35, -0.85),
    (0.5, -0.69),
    (0.66, -0.42),
    (0.82, 0.01),
    (1.0, 0.73),
];
const STRIP_TOP: [(f32, f32); 7] = [
    (0.0, -0.55),
    (0.16, 0.13),
    (0.35, 0.51),
    (0.5, 0.78),
    (0.66, 0.96),
    (0.82, 0.96),
    (1.0, 0.73),
];
const STRIP_RADIUS: [(f32, f32); 5] = [
    (0.0, 1.57),
    (0.16, 1.52),
    (0.5, 1.5),
    (0.82, 1.52),
    (1.0, 1.73),
];

/// One funnel emitter (P0-P4): where its bands start, how fast they rise, how
/// long they live, how many per second, and their colour, alpha and scale over
/// life. Colours are 0-255 as the M2 stores them.
struct BandSpec {
    base_y: f32,
    speed: f32,
    life: f32,
    rate: f32,
    color: [(f32, [f32; 3]); 3],
    alpha: [(f32, f32); 3],
    scale: [(f32, f32); 3],
}

const GREY: [f32; 3] = [153.0, 153.0, 153.0];
const WHITE: [f32; 3] = [255.0, 255.0, 255.0];

/// P0 dark core, P1+P2 body, P3 crown, P4 violet-tipped skirt — in
/// `CYCLONE_LAYERS` order.
const BAND_SPECS: [BandSpec; 4] = [
    BandSpec {
        base_y: 0.0,
        speed: 2.778,
        life: 1.0,
        rate: 13.0,
        color: [
            (0.0, GREY),
            (0.5, [81.0, 81.0, 81.0]),
            (1.0, [30.0, 30.0, 30.0]),
        ],
        alpha: [(0.0, 0.39), (0.5, 1.0), (1.0, 0.0)],
        scale: [(0.0, 0.417), (0.5, 0.139), (1.0, 0.417)],
    },
    BandSpec {
        base_y: 0.0,
        speed: 1.389,
        life: 3.0,
        rate: 34.0,
        color: [(0.0, GREY), (0.7, WHITE), (1.0, WHITE)],
        alpha: [(0.0, 0.39), (0.7, 0.85), (1.0, 0.0)],
        scale: [(0.0, 0.556), (0.7, 0.3), (1.0, 1.111)],
    },
    BandSpec {
        base_y: 1.36,
        speed: 0.278,
        life: 1.0,
        rate: 11.0,
        color: [(0.0, GREY), (0.7, WHITE), (1.0, WHITE)],
        alpha: [(0.0, 0.39), (0.7, 0.24), (1.0, 0.0)],
        scale: [(0.0, 0.222), (0.7, 0.694), (1.0, 1.111)],
    },
    BandSpec {
        base_y: -0.33,
        speed: 1.389,
        life: 3.0,
        rate: 22.0,
        color: [(0.0, GREY), (0.7, WHITE), (1.0, [31.0, 3.0, 243.0])],
        alpha: [(0.0, 0.39), (0.7, 0.78), (1.0, 0.0)],
        scale: [(0.0, 0.556), (0.7, 0.194), (1.0, 1.111)],
    },
];

/// Toon smoke (P5) at the feet.
const CYCLONE_SMOKE_RATE: f32 = 10.0;
const CYCLONE_SMOKE_SPEED: f32 = 0.5;
const CYCLONE_SMOKE_LIFE: f32 = 2.0;
const CYCLONE_SMOKE_DRAG: f32 = 0.4;
const CYCLONE_SMOKE_SPIN_REV: f32 = 2.0;
const CYCLONE_SMOKE_Y: f32 = 0.07;
/// Rim pebbles (P6-P10): `cyclonerock` models kicked up from the funnel's rim.
/// The M2's gravity is negative, so they ACCELERATE upward as they fly.
const CYCLONE_RIM_RATE: f32 = 16.0;
const CYCLONE_RIM_RADIUS: f32 = 2.54;
const CYCLONE_RIM_SPEED: f32 = 3.265;
const CYCLONE_RIM_LIFE: f32 = 0.7;
const CYCLONE_RIM_GRAVITY: f32 = -6.944;
const CYCLONE_RIM_Y: f32 = 0.15;

/// Uniform scale applied to a pet's treatment, as `hard_cc.rs` does.
const PET_STATURE: f32 = 0.55;

const PUFF_TEXTURE_PX: u32 = 64;
const CLOUD_TEXTURE_W: u32 = 64;
const CLOUD_TEXTURE_H: u32 = 32;

// ==============================================================================
// Components
// ==============================================================================

/// Emission state on an Entangling Roots `CcRig` hub: the birth emitters'
/// fractional spawn carry, and a counter feeding the jitter hash.
#[derive(Component, Default)]
pub struct RootTendrils {
    dust_acc: f32,
    pebble_acc: f32,
    seq: u32,
}

/// One segment of one bark root, a child of the root rig's hub. Its pose is
/// rewritten every frame from the root's growth by [`update_root_tendrils`].
#[derive(Component, Clone, Copy, Debug)]
pub struct RootSegment {
    pub root: usize,
    pub segment: usize,
}

/// A cycloned unit's funnel: a WORLD-SPACE hub at the victim's feet, spinning
/// about its axis, whose children are the [`CycloneBand`] strips. `owner` is the
/// sim entity; `retract` is `Some` once the Cyclone has ended, while the funnel
/// stops emitting and fades out.
#[derive(Component)]
pub struct CycloneFunnel {
    pub owner: Entity,
    pub age: f32,
    pub retract: Option<f32>,
    band_acc: [f32; 4],
    smoke_acc: f32,
    rim_acc: f32,
    seq: u32,
}

/// One cloud strip in a Cyclone funnel — a `cyclonegeo` particle.
#[derive(Component)]
pub struct CycloneBand {
    pub layer: usize,
    pub age: f32,
    pub life: f32,
    /// Fixed rotation about the funnel axis, drawn at spawn.
    rot: f32,
}

/// The cycloned victim's lift, bob and spin, on its `VisualBody`.
///
/// Its OWN transform channel. Every other writer of the body's local Y (the
/// gaits, the fear run, the death sink, the victory bounce) and of its rotation
/// (the swing lean, the heal-cast posture, the death fall, the pet tilt) sets
/// it ABSOLUTELY each frame, in `Update`. [`apply_cyclone_lift`] runs after all
/// of them, in `PostUpdate`, and composes the lift on top of whatever they left
/// — so it needs no ordering edge against any one of them and none of them
/// needs to know it exists. Where nobody wrote a channel this frame (a body
/// whose rotation no system owns), it recognises its own last write and
/// composes on the base it saw before, so the lift never accumulates.
///
/// `SparseSet`: inserted and removed on the frame clock (AS-175). It lives on
/// the body child, never the combatant, but the storage keeps the rule simple.
#[derive(Component, Debug)]
#[component(storage = "SparseSet")]
pub struct CycloneLift {
    /// The Cyclone is up. Set each frame by [`update_cyclone_visuals`].
    pub held: bool,
    pub age: f32,
    /// 0 on the ground, 1 fully lifted; eases at `1 / CYCLONE_EASE_SECS`.
    pub weight: f32,
    /// Accumulated spin about the vertical, wrapped to `[0, TAU)`.
    pub yaw: f32,
    /// Pets lift and bob by their smaller stature.
    stature: f32,
    spin_down: Option<SpinDown>,
    base_y: f32,
    base_rotation: Quat,
    written_y: Option<f32>,
    written_rotation: Option<Quat>,
}

/// The spin's wind-down once the Cyclone ends: a constant deceleration from
/// full spin to rest exactly facing forward again, so the body (and the weapons
/// riding it) are never left turned away from the unit's facing.
#[derive(Debug, Clone, Copy)]
struct SpinDown {
    from: f32,
    elapsed: f32,
    duration: f32,
}

impl CycloneLift {
    fn new(stature: f32) -> Self {
        Self {
            held: true,
            age: 0.0,
            weight: 0.0,
            yaw: 0.0,
            stature,
            spin_down: None,
            base_y: 0.0,
            base_rotation: Quat::IDENTITY,
            written_y: None,
            written_rotation: None,
        }
    }

    /// The vertical offset this lift adds to the body this frame.
    pub fn offset(&self) -> f32 {
        let bob = CYCLONE_VICTIM_BOB * (self.age * CYCLONE_BOB_RATE).sin();
        self.weight * (CYCLONE_VICTIM_LIFT + bob) * self.stature
    }

    /// Whether the lift has fully played out: down on the ground and facing
    /// forward, with nothing holding it up.
    pub fn settled(&self) -> bool {
        !self.held && self.weight <= 0.0 && self.yaw == 0.0 && self.spin_down.is_none()
    }

    /// Advance the ease, the bob clock and the spin by `dt`.
    pub fn advance(&mut self, dt: f32) {
        let spin = CYCLONE_VICTIM_SPIN_REV_PER_SEC * TAU;
        self.age += dt;
        let step = dt / CYCLONE_EASE_SECS;
        if self.held {
            self.weight = (self.weight + step).min(1.0);
            // A re-cyclone during the wind-down picks the spin straight back up.
            self.spin_down = None;
            if spin > 0.0 {
                self.yaw = (self.yaw + spin * dt).rem_euclid(TAU);
            }
            return;
        }
        self.weight = (self.weight - step).max(0.0);
        if self.yaw == 0.0 && self.spin_down.is_none() {
            return;
        }
        let down = *self.spin_down.get_or_insert_with(|| {
            // Wind down to the next whole turn, but never by less than the
            // ease's own coast, or a body caught a hair short of forward would
            // stop dead from full spin.
            let mut distance = TAU - self.yaw;
            let coast = spin * CYCLONE_EASE_SECS * 0.5;
            if distance < coast {
                distance += TAU;
            }
            SpinDown {
                from: self.yaw,
                elapsed: 0.0,
                duration: 2.0 * distance / spin.max(1e-3),
            }
        });
        let elapsed = down.elapsed + dt;
        if elapsed >= down.duration {
            self.yaw = 0.0;
            self.spin_down = None;
        } else {
            let travelled = spin * elapsed - spin * elapsed * elapsed / (2.0 * down.duration);
            self.yaw = (down.from + travelled).rem_euclid(TAU);
            self.spin_down = Some(SpinDown { elapsed, ..down });
        }
    }
}

/// A short-lived particle of either treatment: Entangling Roots' birth dust and
/// pebbles, and Cyclone's foot smoke and rim pebbles. World-space and
/// self-expiring, so a treatment that ends lets its debris finish its flight.
#[derive(Component)]
pub struct DruidMote {
    kind: MoteKind,
    velocity: Vec3,
    age: f32,
    life: f32,
    angle: f32,
    /// Size multiplier — a pet's stature.
    size: f32,
    tumble_axis: Vec3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MoteKind {
    RootDust,
    RootPebble,
    CycloneSmoke,
    CycloneRock,
}

impl MoteKind {
    fn is_puff(self) -> bool {
        matches!(self, MoteKind::RootDust | MoteKind::CycloneSmoke)
    }
}

// ==============================================================================
// Pure seams
// ==============================================================================

/// Piecewise-linear keyframes, clamped at both ends — the bench's `keys`.
fn keyed<const N: usize>(keys: &[(f32, f32); N], t: f32) -> f32 {
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for w in keys.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            let span = (b.0 - a.0).max(1e-6);
            return a.1 + (b.1 - a.1) * (t - a.0) / span;
        }
    }
    keys[N - 1].1
}

fn keyed_rgb<const N: usize>(keys: &[(f32, [f32; 3]); N], t: f32) -> [f32; 3] {
    let channel = |c: usize| {
        let mut flat = [(0.0, 0.0); N];
        for (i, (k, v)) in keys.iter().enumerate() {
            flat[i] = (*k, v[c]);
        }
        keyed(&flat, t)
    };
    [channel(0), channel(1), channel(2)]
}

/// How far the roots have grown, 0..1: eased out over the birth, and withdrawn
/// over the death from wherever they had reached when the exit was armed — so
/// a root broken mid-birth sinks from its partial height rather than first
/// growing on. `retract` counts up from the exit, as on `CcRig`.
pub fn root_growth(age: f32, retract: Option<f32>) -> f32 {
    let birth = |t: f32| (t / ROOTS_BIRTH_SECS).clamp(0.0, 1.0);
    let grow = match retract {
        None => birth(age),
        Some(r) => birth((age - r).max(0.0)) * (1.0 - (r / ROOTS_DEATH_SECS).clamp(0.0, 1.0)),
    };
    1.0 - (1.0 - grow).powi(3)
}

/// How long bark roots take to withdraw once their exit is armed.
pub fn root_tendrils_retract_secs() -> f32 {
    ROOTS_DEATH_SECS
}

/// Point `k` (0..=ROOT_SEGMENTS) along root `i` at growth `e`, in the rig's
/// local space (y up from the ground). The bench's root polyline: each root
/// starts outside the body and curls inward and round as it climbs, alternate
/// roots curling opposite ways, so the ring twists like grasping fingers.
pub fn root_point(i: usize, k: usize, e: f32) -> Vec3 {
    let fi = i as f32;
    let a0 = fi * TAU / ROOTS_COUNT as f32 + 0.3 * (fi * 7.1).sin();
    let u = k as f32 / ROOT_SEGMENTS as f32 * e;
    let r = ROOTS_RING_RADIUS * (1.25 - 0.75 * u) + 0.05 * (fi * 3.0 + k as f32).sin();
    let curl = if i % 2 == 1 { 1.0 } else { -1.0 };
    let a = a0 + ROOTS_CURL * u * 1.6 * curl;
    let height = ROOTS_HEIGHT * u * (0.85 + 0.3 * (fi * 1.9).sin());
    Vec3::new(r * a.cos(), height, r * a.sin())
}

/// A segment's radius at its base end: the root tapers linearly from
/// `ROOTS_BASE_THICKNESS` to a point.
fn root_radius(k: usize) -> f32 {
    0.5 * ROOTS_BASE_THICKNESS * (1.0 - k as f32 / (ROOT_SEGMENTS + 1) as f32)
}

/// Pose of a unit-height, origin-centred segment mesh spanning `a` to `b`.
fn segment_pose(a: Vec3, b: Vec3) -> Transform {
    let delta = b - a;
    let len = delta.length();
    if len < 1e-4 {
        return Transform::from_translation(a).with_scale(Vec3::ZERO);
    }
    Transform {
        translation: a + delta * 0.5,
        rotation: Quat::from_rotation_arc(Vec3::Y, delta / len),
        scale: Vec3::new(1.0, len, 1.0),
    }
}

/// A point on the `cyclonegeo` strip at fraction `u` along its span, on its
/// bottom (`top = false`) or top edge, in the band's local space at unit scale.
/// The band's centre height is folded in, so a band's scale grows it about its
/// emitter height exactly as the bench's `yc + edge(u) × scale` does.
pub fn strip_point(u: f32, top: bool) -> Vec3 {
    let angle = (STRIP_A0_DEG + STRIP_SPAN_DEG * u).to_radians();
    let r = keyed(&STRIP_RADIUS, u);
    let edge = if top {
        keyed(&STRIP_TOP, u)
    } else {
        keyed(&STRIP_BOTTOM, u)
    };
    Vec3::new(r * angle.cos(), CYCLONE_BAND_CENTRE + edge, r * angle.sin())
}

/// The `cyclonegeo` strip as a mesh: two rows of vertices along the span,
/// `u` across and `v` from the bottom edge to the top.
fn strip_mesh() -> Mesh {
    let mut positions = Vec::with_capacity((STRIP_COLUMNS + 1) * 2);
    let mut normals = Vec::with_capacity((STRIP_COLUMNS + 1) * 2);
    let mut uvs = Vec::with_capacity((STRIP_COLUMNS + 1) * 2);
    for c in 0..=STRIP_COLUMNS {
        let u = c as f32 / STRIP_COLUMNS as f32;
        let angle = (STRIP_A0_DEG + STRIP_SPAN_DEG * u).to_radians();
        let outward = [angle.cos(), 0.0, angle.sin()];
        for (row, top) in [(0.0, false), (1.0, true)] {
            positions.push(strip_point(u, top).to_array());
            normals.push(outward);
            uvs.push([u, 1.0 - row]);
        }
    }
    let mut indices = Vec::with_capacity(STRIP_COLUMNS * 6);
    for c in 0..STRIP_COLUMNS as u32 {
        let (b0, t0, b1, t1) = (2 * c, 2 * c + 1, 2 * c + 2, 2 * c + 3);
        indices.extend_from_slice(&[b0, b1, t0, t0, b1, t1]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// A band's alpha-blended cloud: a translucent fill with three brighter streaks
/// running along the span, the bench's band drawing (`fill × 0.6`, streaks at
/// 0.3 / 0.55 / 0.8 of the way up at `× 0.7`). White — the material tints it.
fn cloud_texture() -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let (w, h) = (CLOUD_TEXTURE_W, CLOUD_TEXTURE_H);
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let v = 1.0 - (y as f32 + 0.5) / h as f32;
        // A soft lip at both edges so the strip's outline does not alias.
        let lip = (v.min(1.0 - v) / 0.08).clamp(0.0, 1.0);
        let streak = [0.3_f32, 0.55, 0.8]
            .iter()
            .map(|c| (1.0 - ((v - c) / 0.045).abs()).clamp(0.0, 1.0))
            .fold(0.0_f32, f32::max);
        // Streak composited over the fill, as the bench strokes over its fill.
        let a = (0.6 + 0.7 * streak * (1.0 - 0.6)) * lip;
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A soft round puff — the bench's dust/smoke sprite: a radial gradient at 85%
/// in the centre, 50% at 0.45 of the radius, nothing at the rim.
fn puff_texture() -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let size = PUFF_TEXTURE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            let a = keyed(&[(0.0, 0.85), (0.45, 0.5), (1.0, 0.0)], r);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
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

/// A direction in a cone of half-angle `cone` about +Y (the M2 emitter's
/// `vRange`), from two hash draws. A cone of a half-turn or more is a sphere.
fn cone_direction(cone: f32, j1: f32, j2: f32, j3: f32) -> Vec3 {
    if cone >= 3.1 {
        let v = Vec3::new(j1 * 2.0 - 1.0, j2 * 2.0 - 1.0, j3 * 2.0 - 1.0);
        return v.try_normalize().unwrap_or(Vec3::Y);
    }
    let polar = (j1 * 2.0 - 1.0) * cone;
    let azimuth = j2 * TAU;
    Vec3::new(
        polar.sin() * azimuth.cos(),
        polar.cos(),
        polar.sin() * azimuth.sin(),
    )
}

// ==============================================================================
// Shared assets
// ==============================================================================

/// Handles shared by every particle of both treatments, built once.
#[derive(Clone)]
pub struct DruidControlAssets {
    quad: Handle<Mesh>,
    pebble: Handle<Mesh>,
    puff: Handle<Image>,
    strip: Handle<Mesh>,
    cloud: Handle<Image>,
}

impl DruidControlAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            pebble: meshes.add(Cuboid::new(1.0, 0.7, 0.85)),
            puff: images.add(puff_texture()),
            strip: meshes.add(strip_mesh()),
            cloud: images.add(cloud_texture()),
        }
    }
}

fn mote_material(kind: MoteKind, puff: &Handle<Image>) -> StandardMaterial {
    if kind.is_puff() {
        StandardMaterial {
            base_color: Color::NONE,
            base_color_texture: Some(puff.clone()),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color: Color::NONE,
            perceptual_roughness: 0.95,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_mote(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &DruidControlAssets,
    kind: MoteKind,
    at: Vec3,
    velocity: Vec3,
    life: f32,
    size: f32,
    seed: u32,
) {
    let mesh = if kind.is_puff() {
        assets.quad.clone()
    } else {
        assets.pebble.clone()
    };
    let axis = Vec3::new(
        cc_jitter(seed.wrapping_add(11)) - 0.5,
        cc_jitter(seed.wrapping_add(12)) - 0.5,
        cc_jitter(seed.wrapping_add(13)) - 0.5,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y);
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(materials.add(mote_material(kind, &assets.puff))),
        // Zero scale until its first update sizes it, so nothing renders at
        // a default size for a frame.
        Transform::from_translation(at).with_scale(Vec3::ZERO),
        NotShadowCaster,
        DruidMote {
            kind,
            velocity,
            age: 0.0,
            life,
            angle: cc_jitter(seed.wrapping_add(14)) * TAU,
            size,
            tumble_axis: axis,
        },
        PlayMatchEntity,
    ));
}

// ==============================================================================
// Entangling Roots
// ==============================================================================

/// The bark roots for a `RootStyle::Roots` rig: `ROOTS_COUNT` roots of
/// `ROOT_SEGMENTS` tapered segments each, all at zero length until
/// [`update_root_tendrils`] grows them. Opaque and lit — solid wood, like the
/// ice crystals beside it in this treatment.
pub fn build_bark_roots(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    _owner_seed: u32,
) -> Vec<(
    Mesh3d,
    MeshMaterial3d<StandardMaterial>,
    Transform,
    RootSegment,
    NotShadowCaster,
)> {
    if !ROOTS_LAYERS[0] {
        return Vec::new();
    }
    let bark = ROOTS_BARK.to_srgba();
    let mut parts = Vec::with_capacity(ROOTS_COUNT * ROOT_SEGMENTS);
    for k in 0..ROOT_SEGMENTS {
        // The bench bands each root light and dark along its length so the
        // bark reads as knotted rather than as a smooth tube.
        let shade = 0.75 + 0.25 * (k as f32 * 1.7).sin();
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(bark.red * shade, bark.green * shade, bark.blue * shade),
            perceptual_roughness: 0.92,
            ..default()
        });
        let mesh = meshes.add(
            ConicalFrustum {
                radius_bottom: root_radius(k),
                radius_top: root_radius(k + 1),
                height: 1.0,
            }
            .mesh()
            .resolution(7),
        );
        for root in 0..ROOTS_COUNT {
            parts.push((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::ZERO),
                RootSegment { root, segment: k },
                NotShadowCaster,
            ));
        }
    }
    parts
}

/// Grows, holds and withdraws each Entangling Roots rig's bark roots, and emits
/// the birth's dust and pebbles.
///
/// Runs after `update_cc_rigs`, which ages the rig and arms nothing itself —
/// the exit was armed by `update_hard_cc_visuals` on whatever path ended the
/// root (expiry, damage break, dispel, death).
#[allow(clippy::type_complexity)]
pub fn update_root_tendrils(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<DruidControlAssets>>,
    mut hubs: Query<(&CcRig, &Transform, &mut RootTendrils, Option<&Children>)>,
    mut segments: Query<(&RootSegment, &mut Transform), Without<CcRig>>,
) {
    let dt = time.delta_secs();
    for (rig, hub, mut tendrils, children) in hubs.iter_mut() {
        let age = (rig.age - rig.delay).max(0.0);
        let e = root_growth(age, rig.retract);
        if let Some(children) = children {
            for child in children.iter() {
                if let Ok((seg, mut transform)) = segments.get_mut(child) {
                    *transform = segment_pose(
                        root_point(seg.root, seg.segment, e),
                        root_point(seg.root, seg.segment + 1, e),
                    );
                }
            }
        }

        // The birth's dust and pebbles: only while the roots are coming up.
        if rig.retract.is_some() || age >= ROOTS_BIRTH_SECS || rig.age < rig.delay {
            continue;
        }
        let assets =
            assets.get_or_insert_with(|| DruidControlAssets::build(&mut meshes, &mut images));
        let stature = hub.scale.x.max(1e-3);
        let origin = Vec3::new(hub.translation.x, 0.0, hub.translation.z);
        let seed_base = rig.owner.index().wrapping_mul(7919);

        if ROOTS_LAYERS[1] {
            tendrils.dust_acc += ROOT_DUST_RATE * ROOTS_DUST_DENSITY_MUL * dt;
            while tendrils.dust_acc >= 1.0 {
                tendrils.dust_acc -= 1.0;
                tendrils.seq = tendrils.seq.wrapping_add(1);
                let s = seed_base.wrapping_add(tendrils.seq.wrapping_mul(97));
                let offset = Vec3::new(
                    (cc_jitter(s) - 0.5) * ROOT_DUST_AREA,
                    ROOT_EMIT_Y,
                    (cc_jitter(s.wrapping_add(1)) - 0.5) * ROOT_DUST_AREA,
                ) * stature;
                let dir = cone_direction(
                    ROOT_DUST_CONE,
                    cc_jitter(s.wrapping_add(2)),
                    cc_jitter(s.wrapping_add(3)),
                    cc_jitter(s.wrapping_add(4)),
                );
                spawn_mote(
                    &mut commands,
                    &mut materials,
                    assets,
                    MoteKind::RootDust,
                    origin + offset,
                    dir * ROOT_DUST_SPEED * stature,
                    ROOT_DUST_LIFE,
                    stature * ROOTS_DUST_SIZE_MUL,
                    s,
                );
            }
        }
        if ROOTS_LAYERS[2] {
            tendrils.pebble_acc += keyed(&ROOT_PEBBLE_RATE, age) * dt;
            while tendrils.pebble_acc >= 1.0 {
                tendrils.pebble_acc -= 1.0;
                tendrils.seq = tendrils.seq.wrapping_add(1);
                let s = seed_base.wrapping_add(tendrils.seq.wrapping_mul(97));
                let dir = cone_direction(
                    ROOT_PEBBLE_CONE,
                    cc_jitter(s.wrapping_add(5)),
                    cc_jitter(s.wrapping_add(6)),
                    cc_jitter(s.wrapping_add(7)),
                );
                spawn_mote(
                    &mut commands,
                    &mut materials,
                    assets,
                    MoteKind::RootPebble,
                    origin + Vec3::Y * ROOT_EMIT_Y * stature,
                    dir * ROOT_PEBBLE_SPEED * stature,
                    ROOT_PEBBLE_LIFE,
                    stature,
                    s,
                );
            }
        }
    }
}

// ==============================================================================
// Cyclone
// ==============================================================================

/// Whether a unit is in a Cyclone this frame. Death is an exit: aura processing
/// skips the dead, so a Cyclone can outlive its victim on the corpse.
fn is_cycloned(combatant: &Combatant, auras: Option<&ActiveAuras>) -> bool {
    combatant.is_alive()
        && auras.is_some_and(|a| a.auras.iter().any(|au| au.effect_type == AuraType::Cyclone))
}

/// The single owner of the Cyclone treatment's lifetime: spawns a funnel when a
/// unit is cycloned, arms its fade when the Cyclone ends by ANY path (expiry,
/// the aura component going, the vec emptied, death), and holds or releases the
/// victim's [`CycloneLift`].
///
/// Like the hard-CC treatment it RECONCILES rather than keying on a marker: a
/// funnel despawned out from under a live Cyclone (the animation sandbox's
/// leftover sweep) is rebuilt next frame. Nothing is inserted on the combatant
/// itself — the funnel is its own entity and the lift lives on the body.
#[allow(clippy::type_complexity)]
pub fn update_cyclone_visuals(
    mut commands: Commands,
    combatants: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&Pet>,
        Option<&Children>,
    )>,
    bodies: Query<(), With<VisualBody>>,
    mut lifts: Query<&mut CycloneLift>,
    mut funnels: Query<&mut CycloneFunnel>,
) {
    for (entity, combatant, transform, auras, pet, children) in combatants.iter() {
        let cycloned = is_cycloned(combatant, auras);
        let stature = if pet.is_some() { PET_STATURE } else { 1.0 };

        // ---- Funnel ----
        let held = funnels
            .iter()
            .any(|f| f.owner == entity && f.retract.is_none());
        if cycloned && !held {
            commands.spawn((
                Transform::from_translation(Vec3::new(
                    transform.translation.x,
                    0.0,
                    transform.translation.z,
                ))
                .with_scale(Vec3::splat(stature)),
                Visibility::default(),
                CycloneFunnel {
                    owner: entity,
                    age: 0.0,
                    retract: None,
                    band_acc: [0.0; 4],
                    smoke_acc: 0.0,
                    rim_acc: 0.0,
                    seq: entity.index().wrapping_mul(104_729),
                },
                PlayMatchEntity,
            ));
        } else if !cycloned {
            for mut funnel in funnels.iter_mut() {
                if funnel.owner == entity && funnel.retract.is_none() {
                    funnel.retract = Some(0.0);
                }
            }
        }

        // ---- Victim lift ----
        let Some(body) = children.and_then(|cs| cs.iter().find(|&c| bodies.contains(c))) else {
            continue;
        };
        match lifts.get_mut(body) {
            Ok(mut lift) => lift.held = cycloned,
            Err(_) if cycloned => {
                commands.entity(body).try_insert(CycloneLift::new(stature));
            }
            Err(_) => {}
        }
    }
}

/// Follows, spins, emits and fades each Cyclone funnel, and ages its bands.
///
/// `Without<Combatant>` on the hub query keeps its `Transform` access disjoint
/// from the owners' (B0001), as in `update_cc_rigs`.
#[allow(clippy::type_complexity)]
pub fn update_cyclone_funnels(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<DruidControlAssets>>,
    mut funnels: Query<
        (
            Entity,
            &mut CycloneFunnel,
            &mut Transform,
            Option<&Children>,
        ),
        (Without<Combatant>, Without<CycloneBand>),
    >,
    owners: Query<
        &Transform,
        (
            With<Combatant>,
            Without<CycloneFunnel>,
            Without<CycloneBand>,
        ),
    >,
    mut bands: Query<
        (
            &mut CycloneBand,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<Combatant>, Without<CycloneFunnel>),
    >,
) {
    let dt = time.delta_secs();
    let assets = assets.get_or_insert_with(|| DruidControlAssets::build(&mut meshes, &mut images));

    for (hub_entity, mut funnel, mut hub, children) in funnels.iter_mut() {
        funnel.age += dt;
        if let Some(r) = funnel.retract.as_mut() {
            *r += dt;
        }
        let Ok(owner) = owners.get(funnel.owner) else {
            commands.entity(hub_entity).despawn();
            continue;
        };
        if funnel.retract.is_some_and(|r| r >= CYCLONE_EASE_SECS) {
            commands.entity(hub_entity).despawn();
            continue;
        }

        hub.translation = Vec3::new(owner.translation.x, 0.0, owner.translation.z);
        // Wall-clock spin, never sim displacement (fixed-timestep strobe).
        hub.rotation = Quat::from_rotation_y(-funnel.age * CYCLONE_FUNNEL_SPIN_REV_PER_SEC * TAU);
        let stature = hub.scale.x.max(1e-3);
        let fade = 1.0 - funnel.retract.map_or(0.0, |r| r / CYCLONE_EASE_SECS);

        // Age the strips already in the funnel.
        if let Some(children) = children {
            for child in children.iter() {
                let Ok((mut band, mut transform, material)) = bands.get_mut(child) else {
                    continue;
                };
                band.age += dt;
                if band.age >= band.life {
                    commands.entity(child).despawn();
                    continue;
                }
                pose_band(&band, &mut transform);
                if let Some(mat) = materials.get_mut(&material.0) {
                    mat.base_color = band_color(&band, fade);
                }
            }
        }

        if funnel.retract.is_some() {
            continue;
        }

        // Emit new strips, layer by layer.
        for (layer, spec) in BAND_SPECS.iter().enumerate() {
            if !CYCLONE_LAYERS[layer] {
                continue;
            }
            funnel.band_acc[layer] += spec.rate * CYCLONE_DENSITY_MUL * dt;
            while funnel.band_acc[layer] >= 1.0 {
                funnel.band_acc[layer] -= 1.0;
                funnel.seq = funnel.seq.wrapping_add(1);
                let band = CycloneBand {
                    layer,
                    age: 0.0,
                    life: spec.life * CYCLONE_BAND_LIFE_MUL,
                    rot: cc_jitter(funnel.seq) * TAU,
                };
                let mut transform = Transform::default();
                pose_band(&band, &mut transform);
                let material = materials.add(StandardMaterial {
                    base_color: band_color(&band, 1.0),
                    base_color_texture: Some(assets.cloud.clone()),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                });
                let strip = commands
                    .spawn((
                        Mesh3d(assets.strip.clone()),
                        MeshMaterial3d(material),
                        transform,
                        NotShadowCaster,
                        band,
                    ))
                    .id();
                commands.entity(hub_entity).add_child(strip);
            }
        }

        let origin = hub.translation;
        // Toon smoke at the feet.
        if CYCLONE_LAYERS[4] {
            funnel.smoke_acc += CYCLONE_SMOKE_RATE * CYCLONE_DENSITY_MUL * dt;
            while funnel.smoke_acc >= 1.0 {
                funnel.smoke_acc -= 1.0;
                funnel.seq = funnel.seq.wrapping_add(1);
                let s = funnel.seq.wrapping_mul(131);
                let dir = cone_direction(
                    3.14,
                    cc_jitter(s),
                    cc_jitter(s.wrapping_add(1)),
                    cc_jitter(s.wrapping_add(2)),
                );
                spawn_mote(
                    &mut commands,
                    &mut materials,
                    assets,
                    MoteKind::CycloneSmoke,
                    origin + Vec3::Y * CYCLONE_SMOKE_Y * stature,
                    dir * CYCLONE_SMOKE_SPEED * stature,
                    CYCLONE_SMOKE_LIFE,
                    stature,
                    s,
                );
            }
        }
        // Pebbles kicked up from the rim.
        if CYCLONE_LAYERS[5] {
            funnel.rim_acc += CYCLONE_RIM_RATE * CYCLONE_DENSITY_MUL * dt;
            while funnel.rim_acc >= 1.0 {
                funnel.rim_acc -= 1.0;
                funnel.seq = funnel.seq.wrapping_add(1);
                let s = funnel.seq.wrapping_mul(131);
                let a = cc_jitter(s.wrapping_add(3)) * TAU;
                let at = origin
                    + Vec3::new(
                        CYCLONE_RIM_RADIUS * a.cos(),
                        CYCLONE_RIM_Y,
                        CYCLONE_RIM_RADIUS * a.sin(),
                    ) * stature;
                spawn_mote(
                    &mut commands,
                    &mut materials,
                    assets,
                    MoteKind::CycloneRock,
                    at,
                    Vec3::Y * CYCLONE_RIM_SPEED * stature,
                    CYCLONE_RIM_LIFE,
                    stature,
                    s,
                );
            }
        }
    }
}

/// A band's local pose under its hub at its current age: risen from its
/// emitter height, turned to its own angle, grown by its scale track.
fn pose_band(band: &CycloneBand, transform: &mut Transform) {
    let spec = &BAND_SPECS[band.layer];
    let t = (band.age / band.life).clamp(0.0, 1.0);
    let scale = keyed(&spec.scale, t) * CYCLONE_BAND_SIZE_MUL * CYCLONE_BAND_SCALE;
    transform.translation = Vec3::Y * (spec.base_y + spec.speed * CYCLONE_BAND_RISE_MUL * band.age);
    transform.rotation = Quat::from_rotation_y(-band.rot);
    transform.scale = Vec3::splat(scale);
}

/// A band's tint at its current age: its colour track darkened by
/// `CYCLONE_DARKNESS` (the bench's `c × (1 - 0.75 × darkness)`), its alpha track
/// scaled by the opacity and the funnel's exit fade.
fn band_color(band: &CycloneBand, fade: f32) -> Color {
    let spec = &BAND_SPECS[band.layer];
    let t = (band.age / band.life).clamp(0.0, 1.0);
    let rgb = keyed_rgb(&spec.color, t);
    let dark = 1.0 - 0.75 * CYCLONE_DARKNESS;
    let alpha = keyed(&spec.alpha, t) * CYCLONE_OPACITY * CYCLONE_BAND_ALPHA * fade;
    Color::srgba(
        rgb[0] / 255.0 * dark,
        rgb[1] / 255.0 * dark,
        rgb[2] / 255.0 * dark,
        alpha.clamp(0.0, 1.0),
    )
}

/// Composes each cycloned victim's lift, bob and spin onto its body, after every
/// other writer of the body's transform has run. See [`CycloneLift`].
///
/// In `PostUpdate`, before transform propagation, so the body renders where it
/// is lifted to on this frame and every `Update` writer is already done.
pub fn apply_cyclone_lift(
    mut commands: Commands,
    time: Res<Time>,
    mut bodies: Query<(Entity, &mut CycloneLift, &mut Transform), With<VisualBody>>,
) {
    let dt = time.delta_secs();
    for (entity, mut lift, mut transform) in bodies.iter_mut() {
        // What the other writers left this frame — or, where nobody wrote a
        // channel, the base this lift composed on last frame.
        let base_y = match lift.written_y {
            Some(w) if transform.translation.y == w => lift.base_y,
            _ => transform.translation.y,
        };
        let base_rotation = match lift.written_rotation {
            Some(w) if transform.rotation == w => lift.base_rotation,
            _ => transform.rotation,
        };
        lift.base_y = base_y;
        lift.base_rotation = base_rotation;

        lift.advance(dt);
        if lift.settled() {
            transform.translation.y = base_y;
            transform.rotation = base_rotation;
            commands.entity(entity).remove::<CycloneLift>();
            continue;
        }
        transform.translation.y = base_y + lift.offset();
        transform.rotation = Quat::from_rotation_y(lift.yaw) * base_rotation;
        lift.written_y = Some(transform.translation.y);
        lift.written_rotation = Some(transform.rotation);
    }
}

// ==============================================================================
// Motes
// ==============================================================================

/// Flies, tumbles, sizes and fades every Druid mote, faces the puffs to the
/// camera, and despawns the spent.
pub fn update_druid_motes(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&Transform, (With<Camera3d>, Without<DruidMote>)>,
    mut motes: Query<(
        Entity,
        &mut DruidMote,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let facing = camera.iter().next().map(|c| c.rotation);
    for (entity, mut mote, mut transform, material) in motes.iter_mut() {
        mote.age += dt;
        if mote.age >= mote.life {
            commands.entity(entity).despawn();
            continue;
        }
        let t = mote.age / mote.life;
        let (gravity, drag, spin_rev) = match mote.kind {
            MoteKind::RootDust => (0.0, 0.0, ROOT_DUST_SPIN_REV),
            MoteKind::RootPebble => (ROOT_PEBBLE_GRAVITY, 0.0, ROOT_PEBBLE_SPIN_REV),
            MoteKind::CycloneSmoke => (0.0, CYCLONE_SMOKE_DRAG, CYCLONE_SMOKE_SPIN_REV),
            MoteKind::CycloneRock => (CYCLONE_RIM_GRAVITY, 0.0, 0.0),
        };
        // The bench's integration: gravity, then a per-step drag factor.
        mote.velocity.y -= gravity * mote.size * dt;
        let damp = (1.0 - drag * dt * 3.0).max(0.0);
        mote.velocity *= damp;
        let step = mote.velocity * dt;
        transform.translation += step;
        if mote.kind == MoteKind::RootPebble && transform.translation.y < ROOT_PEBBLE_FLOOR {
            transform.translation.y = ROOT_PEBBLE_FLOOR;
            mote.velocity = Vec3::ZERO;
        }
        mote.angle += spin_rev * TAU * dt;

        // Size (the track is a radius, so the unit quad / pebble spans twice
        // it), colour and alpha over life.
        let (radius, rgb, alpha) = match mote.kind {
            MoteKind::RootDust => (
                keyed(&[(0.0, 0.0), (0.2, 0.417), (1.0, 0.694)], t),
                keyed_rgb(
                    &[
                        (0.0, [106.0, 91.0, 0.0]),
                        (0.2, [126.0, 112.0, 34.0]),
                        (1.0, [157.0, 146.0, 87.0]),
                    ],
                    t,
                ),
                keyed(&[(0.0, 0.39), (0.2, 0.69), (1.0, 0.0)], t),
            ),
            MoteKind::RootPebble => (
                0.028,
                keyed_rgb(&[(0.0, [106.0, 91.0, 0.0]), (1.0, [157.0, 146.0, 87.0])], t),
                1.0,
            ),
            MoteKind::CycloneSmoke => (
                keyed(&[(0.0, 0.4), (0.5, 0.55), (1.0, 0.7)], t),
                keyed_rgb(
                    &[(0.0, [180.0, 180.0, 175.0]), (1.0, [220.0, 220.0, 215.0])],
                    t,
                ),
                keyed(&[(0.0, 0.3), (0.5, 0.3), (1.0, 0.0)], t),
            ),
            MoteKind::CycloneRock => (
                0.06,
                [98.0, 85.0, 77.0],
                keyed(&[(0.0, 0.8), (0.95, 0.9), (1.0, 0.0)], t),
            ),
        };
        transform.scale = Vec3::splat(2.0 * radius * mote.size);
        if mote.kind.is_puff() {
            if let Some(facing) = facing {
                transform.rotation = facing * Quat::from_rotation_z(mote.angle);
            }
        } else {
            transform.rotation = Quat::from_axis_angle(mote.tumble_axis, mote.angle);
        }
        if let Some(mat) = materials.get_mut(&material.0) {
            mat.base_color = Color::srgba(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0, alpha);
        }
    }
}
