//! Moonfire landing (graphical-only): a moon orb overhead and a thin beam of
//! moonlight dropping onto the victim, with a violet glow and spiralling wisps
//! at its feet.
//!
//! Source: `moonfire_impact_base.m2` (kit 3293, @Base), built to the user's
//! TBC reference and the signed-off AS-160 bench —
//! `docs/design/2026-10-03-druid-client-data.md` (*Bench sign-off*, Moonfire
//! block, and Rulings 2 and 6) and its drawing code in
//! `docs/design/benches/2026-10-03-druid-visuals-bench.html`. The mesh stands
//! 13.9 yd tall with its narrowest radius at the top (the orb) and the base
//! (the beam core); every texture-weight track is dark by 2267 ms.
//!
//! Consumes the [`MoonfireLanding`] marker `process_casting` spawns at the
//! landing. Every piece is additive, unlit and casts no shadow. Every soft
//! edge — the beam's white core in its violet halo, the orb, the base glow —
//! is VERTEX ALPHA on one mesh, never nested geometry: each bench layer is a
//! sum of linear falloffs, so its profile is piecewise linear and a mesh with
//! a vertex at every breakpoint reproduces it exactly.
//!
//! Moonfire's DoT draws nothing (ruling 2): this landing is its whole visual.
//!
//! Two placements the bench (which draws without depth) did not need:
//! - The camera-facing pieces (beam, base column, orb) sit just in front of the
//!   victim's capsule, toward the camera — on its axis the capsule would hide
//!   the beam exactly where it lands.
//! - The wisps curl out from just outside the capsule rather than from the
//!   bench's 0.35 yd, which is inside it.

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::render::view::NoFrustumCulling;
use std::f32::consts::TAU;

use super::heal_impact::{ARENA_FLOOR_WORLD_Y, COMBATANT_BODY_RADIUS, HEALING_WAVE_UNDERGLOW_LIFT};
use crate::states::play_match::components::*;

// --- Bench sign-off (AS-160) -------------------------------------------------
// Moonfire — magic_cast_hand.m2 (kit 730) + moonfire_impact_base.m2 (kit 3293).
// The cast-hand constants belong to the cast-hands card.

pub const MOONFIRE_ORB_HEIGHT: f32 = 13.90;
pub const MOONFIRE_ORB_RADIUS: f32 = 0.55;
pub const MOONFIRE_BEAM_WIDTH: f32 = 0.18;
pub const MOONFIRE_BASE_GLOW_RADIUS: f32 = 0.90;
pub const MOONFIRE_WISPS: usize = 3;
pub const MOONFIRE_LANDING_SECS: f32 = 2.27;
/// orb, beam, base glow, wisps
pub const MOONFIRE_LAYERS: [bool; 4] = [true, true, true, true];
pub const MOONFIRE_OPACITY: f32 = 1.00;

// --- Bench drawing code, ported ----------------------------------------------

/// The bench's own clock: its envelopes are keyed in ms over a 2267 ms
/// landing, which `MOONFIRE_LANDING_SECS` stretches.
const BENCH_LANDING_MS: f32 = 2267.0;

/// Beam brightness: up by 150 ms, held to 1000, half by 1500, gone by 2267.
const BEAM_ENVELOPE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (150.0, 1.0),
    (1000.0, 1.0),
    (1500.0, 0.55),
    (2267.0, 0.0),
];
/// Orb brightness.
const ORB_ENVELOPE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 1.0),
    (1000.0, 1.0),
    (1600.0, 0.4),
    (2267.0, 0.0),
];
/// Base glow: the client's `blue_glow2` peaks at 667 ms, the afterglow is gone
/// by 2267.
const BASE_ENVELOPE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.5),
    (667.0, 0.6),
    (1333.0, 0.3),
    (2267.0, 0.0),
];
/// Wisps: the ribbon layer, peaking at 600 ms and gone by 1533.
const WISP_ENVELOPE: &[(f32, f32)] = &[(0.0, 0.0), (600.0, 0.8), (1533.0, 0.0)];

/// One linear falloff across a camera-facing band: `weight` at the centre line,
/// nothing at `half_width`, held from the ground to `top`.
#[derive(Clone, Copy, Debug)]
struct Band {
    half_width: f32,
    top: f32,
    srgb: [u8; 3],
    weight: f32,
}

/// The beam: a white core in a violet halo, all three bands floor to orb.
fn beam_bands() -> [Band; 3] {
    let w = MOONFIRE_BEAM_WIDTH;
    let h = MOONFIRE_ORB_HEIGHT;
    [
        Band {
            half_width: w * 3.0,
            top: h,
            srgb: [130, 60, 255],
            weight: 0.55,
        },
        Band {
            half_width: w * 1.4,
            top: h,
            srgb: [190, 150, 255],
            weight: 0.8,
        },
        Band {
            half_width: w * 0.5,
            top: h,
            srgb: [255, 255, 255],
            weight: 1.0,
        },
    ]
}

/// The violet column of light standing round the victim.
fn base_column_bands() -> [Band; 2] {
    let r = MOONFIRE_BASE_GLOW_RADIUS;
    [
        Band {
            half_width: r,
            top: 2.6,
            srgb: [110, 50, 220],
            weight: 1.0,
        },
        Band {
            half_width: r * 0.6,
            top: 1.6,
            srgb: [150, 110, 255],
            weight: 0.6,
        },
    ]
}

/// The pool of light on the ground under the column.
const BASE_POOL_SRGB: [u8; 3] = [160, 120, 255];
const BASE_POOL_WEIGHT: f32 = 0.5;

/// One radial glow sprite (the bench's `sprite('glow', …)`): `weight` at the
/// centre, 0.45 of it at 0.45 of the radius, nothing at the radius.
#[derive(Clone, Copy, Debug)]
struct Glow {
    radius: f32,
    srgb: [u8; 3],
    weight: f32,
}

/// The moon: a white-blue core inside a wider blue glow. The bench sizes each
/// sprite by its diameter (`orb × 5` and `orb × 2.4`).
fn orb_glows() -> [Glow; 2] {
    let r = MOONFIRE_ORB_RADIUS;
    [
        Glow {
            radius: r * 5.0 * 0.5,
            srgb: [150, 170, 255],
            weight: 0.6,
        },
        Glow {
            radius: r * 2.4 * 0.5,
            srgb: [240, 248, 255],
            weight: 1.0,
        },
    ]
}
/// The glow sprite's radial falloff knots, as (fraction of radius, alpha).
const GLOW_FALLOFF: [(f32, f32); 3] = [(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)];
const ORB_SEGMENTS: usize = 32;
const POOL_SEGMENTS: usize = 40;

/// Wisp ribbons: `WISP_POINTS + 1` samples a wisp, `WISP_TURN` radians of curl
/// along it, rising `WISP_RISE` yd while widening by `WISP_FLARE`.
const WISP_POINTS: usize = 24;
const WISP_TURN: f32 = 5.0;
const WISP_RISE: f32 = 2.2;
const WISP_FLARE: f32 = 0.35;
/// Where a wisp starts: just outside the capsule (see the module note).
const WISP_START_RADIUS: f32 = COMBATANT_BODY_RADIUS + 0.08;
/// Ribbon width, and how fast the spiral turns (one radian per 350 ms).
const WISP_WIDTH: f32 = 0.06;
const WISP_SPIN_MS_PER_RAD: f32 = 350.0;
/// The wisps grow to their full height over their first 600 ms.
const WISP_GROW_MS: f32 = 600.0;
const WISP_SRGB: [u8; 3] = [210, 200, 255];
const WISP_WEIGHT: f32 = 0.9;

/// How far the camera-facing pieces sit in front of the victim's axis.
const FRONT_OF_BODY: f32 = COMBATANT_BODY_RADIUS + 0.05;

// --- Pure helpers ------------------------------------------------------------

/// Piecewise-linear keyframes, held flat outside their range (the bench's `keys`).
fn envelope(keys: &[(f32, f32)], t: f32) -> f32 {
    let Some(&(t0, v0)) = keys.first() else {
        return 0.0;
    };
    if t <= t0 {
        return v0;
    }
    for pair in keys.windows(2) {
        let ((ta, va), (tb, vb)) = (pair[0], pair[1]);
        if t <= tb {
            let u = if tb > ta { (t - ta) / (tb - ta) } else { 1.0 };
            return va + (vb - va) * u;
        }
    }
    keys[keys.len() - 1].1
}

/// A landing's age in the bench's milliseconds.
fn bench_ms(age: f32) -> f32 {
    age * BENCH_LANDING_MS / MOONFIRE_LANDING_SECS
}

fn linear(srgb: [u8; 3], weight: f32) -> Vec3 {
    let c = Color::srgb_u8(srgb[0], srgb[1], srgb[2]).to_linear();
    Vec3::new(c.red, c.green, c.blue) * weight
}

/// The summed light of `bands` at `x` across, at a height they all reach.
fn band_light(bands: &[Band], x: f32, height: f32) -> Vec3 {
    bands
        .iter()
        .filter(|b| b.top >= height)
        .map(|b| linear(b.srgb, b.weight) * (1.0 - x.abs() / b.half_width).max(0.0))
        .sum()
}

/// One glow's falloff at `rho` from its centre.
fn glow_falloff(glow: &Glow, rho: f32) -> f32 {
    envelope(&GLOW_FALLOFF, rho / glow.radius).max(0.0) * glow.weight
}

fn glow_light(glows: &[Glow], rho: f32) -> Vec3 {
    glows
        .iter()
        .filter(|g| rho < g.radius)
        .map(|g| linear(g.srgb, 1.0) * glow_falloff(g, rho))
        .sum()
}

fn color(light: Vec3) -> [f32; 4] {
    [light.x, light.y, light.z, 1.0]
}

fn mesh_from(
    positions: Vec<[f32; 3]>,
    normal: [f32; 3],
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
    let n = positions.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    // Unlit: normals and UVs only satisfy the vertex layout.
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// A camera-facing band mesh in its local XY plane (facing +Z), standing on
/// y = 0: a vertex column at every band edge and the centre line, and a row at
/// every band top, so each band's linear falloff and hard top are exact.
fn build_band_mesh(bands: &[Band]) -> Mesh {
    let mut xs: Vec<f32> = vec![0.0];
    for b in bands {
        xs.push(b.half_width);
        xs.push(-b.half_width);
    }
    xs.sort_by(f32::total_cmp);
    xs.dedup();
    let mut ys: Vec<f32> = vec![0.0];
    ys.extend(bands.iter().map(|b| b.top));
    ys.sort_by(f32::total_cmp);
    ys.dedup();

    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    // One strip per height interval: a band that ends at an interval's top
    // lights it, and the next interval up starts without it.
    for rows in ys.windows(2) {
        let (y0, y1) = (rows[0], rows[1]);
        let base = positions.len() as u32;
        for &x in &xs {
            let light = color(band_light(bands, x, y1));
            positions.push([x, y0, 0.0]);
            positions.push([x, y1, 0.0]);
            colors.push(light);
            colors.push(light);
        }
        for i in 0..(xs.len() as u32 - 1) {
            let a = base + i * 2;
            indices.extend_from_slice(&[a, a + 2, a + 3, a, a + 3, a + 1]);
        }
    }
    mesh_from(positions, [0.0, 0.0, 1.0], colors, indices)
}

/// A radial fan with a ring of vertices at every radius in `rings` (the first
/// must be 0), coloured by `light(rho)`. `flat` lays it on the ground (XZ,
/// facing up); otherwise it faces +Z.
fn build_fan_mesh(rings: &[f32], segments: usize, flat: bool, light: impl Fn(f32) -> Vec3) -> Mesh {
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for &rho in rings {
        let c = color(light(rho));
        for s in 0..segments {
            let a = s as f32 / segments as f32 * TAU;
            let (u, v) = (rho * a.cos(), rho * a.sin());
            positions.push(if flat { [u, 0.0, v] } else { [u, v, 0.0] });
            colors.push(c);
        }
    }
    let n = segments as u32;
    for ring in 0..(rings.len() as u32 - 1) {
        for s in 0..n {
            let (s0, s1) = (s, (s + 1) % n);
            let (i0, i1) = (ring * n, (ring + 1) * n);
            // Wound to face +Y when flat and +Z when standing.
            if flat {
                indices.extend_from_slice(&[i0 + s0, i1 + s1, i1 + s0, i0 + s0, i0 + s1, i1 + s1]);
            } else {
                indices.extend_from_slice(&[i0 + s0, i1 + s0, i1 + s1, i0 + s0, i1 + s1, i0 + s1]);
            }
        }
    }
    let normal = if flat {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    mesh_from(positions, normal, colors, indices)
}

/// The orb's rings: the centre, and every knot of both glows' falloff.
fn orb_rings() -> Vec<f32> {
    let mut rings: Vec<f32> = orb_glows()
        .iter()
        .flat_map(|g| GLOW_FALLOFF.iter().map(move |(f, _)| f * g.radius))
        .collect();
    rings.sort_by(f32::total_cmp);
    rings.dedup();
    rings
}

fn build_orb_mesh() -> Mesh {
    let glows = orb_glows();
    build_fan_mesh(&orb_rings(), ORB_SEGMENTS, false, |rho| {
        glow_light(&glows, rho)
    })
}

fn build_pool_mesh() -> Mesh {
    let light = linear(BASE_POOL_SRGB, BASE_POOL_WEIGHT);
    build_fan_mesh(
        &[0.0, MOONFIRE_BASE_GLOW_RADIUS],
        POOL_SEGMENTS,
        true,
        |_| light,
    )
}

/// The centre line of wisp `w` at `ms` into the landing, rig-local. Each point
/// carries its fraction along the wisp.
fn wisp_points(w: usize, ms: f32) -> Vec<(Vec3, f32)> {
    let rise = (ms / WISP_GROW_MS).clamp(0.0, 1.0);
    (0..=WISP_POINTS)
        .map(|k| {
            let u = k as f32 / WISP_POINTS as f32;
            let ang =
                w as f32 * TAU / MOONFIRE_WISPS as f32 + u * WISP_TURN + ms / WISP_SPIN_MS_PER_RAD;
            let r = WISP_START_RADIUS + WISP_FLARE * u;
            (
                Vec3::new(r * ang.cos(), u * WISP_RISE * rise, r * ang.sin()),
                u,
            )
        })
        .collect()
}

/// Rewrite the wisp mesh: one camera-facing ribbon per wisp, fading from its
/// root to nothing at its tip. `eye` is the camera, rig-local.
fn rebuild_wisp_mesh(mesh: &mut Mesh, ms: f32, eye: Vec3) {
    let light = linear(WISP_SRGB, 1.0);
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for w in 0..MOONFIRE_WISPS {
        let points = wisp_points(w, ms);
        let base = positions.len() as u32;
        for (k, &(p, u)) in points.iter().enumerate() {
            let prev = points[k.saturating_sub(1)].0;
            let next = points[(k + 1).min(points.len() - 1)].0;
            let side = (next - prev).cross(eye - p).normalize_or_zero() * (WISP_WIDTH * 0.5);
            let alpha = WISP_WEIGHT * (1.0 - u);
            positions.push((p - side).to_array());
            positions.push((p + side).to_array());
            colors.push([light.x, light.y, light.z, alpha]);
            colors.push([light.x, light.y, light.z, alpha]);
        }
        for k in 0..WISP_POINTS as u32 {
            let a = base + k * 2;
            indices.extend_from_slice(&[a, a + 2, a + 3, a, a + 3, a + 1]);
        }
    }
    let n = positions.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}

// --- Runtime components (graphical-only) -------------------------------------

/// Which layer a landing piece is, and so which envelope fades it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layer {
    Orb,
    Beam,
    Base,
    Wisps,
}

impl Layer {
    fn envelope(self) -> &'static [(f32, f32)] {
        match self {
            Layer::Orb => ORB_ENVELOPE,
            Layer::Beam => BEAM_ENVELOPE,
            Layer::Base => BASE_ENVELOPE,
            Layer::Wisps => WISP_ENVELOPE,
        }
    }
}

/// The playing landing, on the marker entity; its pieces are children.
#[derive(Component)]
pub struct MoonfireLandingRig {
    age: f32,
    materials: Vec<(Layer, Handle<StandardMaterial>)>,
    wisps: Option<Handle<Mesh>>,
}

/// A camera-facing piece of a landing, held in front of the victim. `height`
/// is its centre above the floor; `upright` pieces turn about the vertical
/// only (the beam and the column), the rest face the camera fully (the orb).
#[derive(Component)]
pub struct MoonfireBillboard {
    rig: Entity,
    height: f32,
    upright: bool,
}

fn additive_material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::linear_rgba(1.0, 1.0, 1.0, 0.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

fn rig_position(landing: &MoonfireLanding, targets: &Query<&Transform, MoonfireOthers>) -> Vec3 {
    let at = targets
        .get(landing.target)
        .map(|t| t.translation)
        .unwrap_or(landing.origin);
    Vec3::new(at.x, ARENA_FLOOR_WORLD_Y, at.z)
}

type MoonfireOthers = (Without<MoonfireLandingRig>, Without<MoonfireBillboard>);

/// Shared, never-changing meshes, built on the first landing.
pub struct MoonfireMeshes {
    beam: Handle<Mesh>,
    column: Handle<Mesh>,
    orb: Handle<Mesh>,
    pool: Handle<Mesh>,
}

// --- Systems -------------------------------------------------------------------

/// Dress each new landing: the beam, orb, base column and pool, and the wisps.
pub fn spawn_moonfire_landings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cached: Local<Option<MoonfireMeshes>>,
    landings: Query<
        (Entity, &MoonfireLanding),
        (Added<MoonfireLanding>, Without<MoonfireLandingRig>),
    >,
    targets: Query<&Transform, MoonfireOthers>,
) {
    if landings.is_empty() {
        return;
    }
    let shared = cached.get_or_insert_with(|| MoonfireMeshes {
        beam: meshes.add(build_band_mesh(&beam_bands())),
        column: meshes.add(build_band_mesh(&base_column_bands())),
        orb: meshes.add(build_orb_mesh()),
        pool: meshes.add(build_pool_mesh()),
    });
    let [orb_on, beam_on, base_on, wisps_on] = MOONFIRE_LAYERS;

    for (entity, landing) in landings.iter() {
        let mut layer_materials = Vec::new();
        let mut material_for = |layer: Layer, materials: &mut Assets<StandardMaterial>| {
            let handle = additive_material(materials);
            layer_materials.push((layer, handle.clone()));
            handle
        };
        // (mesh, material, transform, billboard height + upright)
        let mut pieces: Vec<(
            Handle<Mesh>,
            Handle<StandardMaterial>,
            Transform,
            Option<(f32, bool)>,
        )> = Vec::new();
        if beam_on {
            let m = material_for(Layer::Beam, &mut materials);
            pieces.push((
                shared.beam.clone(),
                m,
                Transform::default(),
                Some((0.0, true)),
            ));
        }
        if base_on {
            let m = material_for(Layer::Base, &mut materials);
            pieces.push((
                shared.column.clone(),
                m.clone(),
                Transform::default(),
                Some((0.0, true)),
            ));
            pieces.push((
                shared.pool.clone(),
                m,
                Transform::from_xyz(0.0, HEALING_WAVE_UNDERGLOW_LIFT, 0.0),
                None,
            ));
        }
        if orb_on {
            let m = material_for(Layer::Orb, &mut materials);
            let at = Transform::from_xyz(0.0, MOONFIRE_ORB_HEIGHT, 0.0);
            pieces.push((
                shared.orb.clone(),
                m,
                at,
                Some((MOONFIRE_ORB_HEIGHT, false)),
            ));
        }
        let wisps = if wisps_on && MOONFIRE_WISPS > 0 {
            let m = material_for(Layer::Wisps, &mut materials);
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
            );
            rebuild_wisp_mesh(&mut mesh, 0.0, Vec3::new(0.0, 10.0, 10.0));
            Some((meshes.add(mesh), m))
        } else {
            None
        };

        commands
            .entity(entity)
            .try_insert((
                MoonfireLandingRig {
                    age: 0.0,
                    materials: layer_materials,
                    wisps: wisps.as_ref().map(|(mesh, _)| mesh.clone()),
                },
                Transform::from_translation(rig_position(landing, &targets)),
                Visibility::default(),
            ))
            .with_children(|parent| {
                for (mesh, material, transform, billboard) in pieces {
                    let mut piece = parent.spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        transform,
                        NotShadowCaster,
                    ));
                    if let Some((height, upright)) = billboard {
                        piece.insert(MoonfireBillboard {
                            rig: entity,
                            height,
                            upright,
                        });
                    }
                }
                if let Some((mesh, material)) = wisps {
                    // Rebuilt every frame, so its spawn-time bounds mean nothing.
                    parent.spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::default(),
                        NotShadowCaster,
                        NoFrustumCulling,
                    ));
                }
            });
    }
}

/// Age each landing, keep it on its victim, fade every layer along its
/// envelope, curl the wisps and turn the camera-facing pieces.
pub fn update_moonfire_landings(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<Entity, With<Camera3d>>,
    mut rigs: Query<(
        Entity,
        &MoonfireLanding,
        &mut MoonfireLandingRig,
        &mut Transform,
    )>,
    mut parts: Query<(&MoonfireBillboard, &mut Transform), Without<MoonfireLandingRig>>,
    others: Query<&Transform, MoonfireOthers>,
) {
    let dt = time.delta_secs();
    let eye = camera
        .iter()
        .next()
        .and_then(|c| others.get(c).ok())
        .map(|t| t.translation);
    let mut placed: Vec<(Entity, Vec3)> = Vec::new();

    for (entity, landing, mut rig, mut transform) in rigs.iter_mut() {
        rig.age += dt;
        let at = rig_position(landing, &others);
        transform.translation = at;
        placed.push((entity, at));

        let ms = bench_ms(rig.age);
        for (layer, handle) in &rig.materials {
            if let Some(material) = materials.get_mut(handle) {
                let alpha = (envelope(layer.envelope(), ms) * MOONFIRE_OPACITY).max(0.0);
                material.base_color = Color::linear_rgba(1.0, 1.0, 1.0, alpha);
            }
        }
        if let (Some(handle), Some(eye)) = (&rig.wisps, eye) {
            if let Some(mesh) = meshes.get_mut(handle) {
                rebuild_wisp_mesh(mesh, ms, eye - at);
            }
        }
    }

    let Some(eye) = eye else {
        return;
    };
    for (billboard, mut transform) in parts.iter_mut() {
        let Some(&(_, at)) = placed.iter().find(|(e, _)| *e == billboard.rig) else {
            continue;
        };
        let to_eye = eye - (at + Vec3::Y * billboard.height);
        let flat = Vec3::new(to_eye.x, 0.0, to_eye.z).normalize_or_zero();
        transform.translation = flat * FRONT_OF_BODY + Vec3::Y * billboard.height;
        transform.rotation = if billboard.upright {
            Quat::from_rotation_y(flat.x.atan2(flat.z))
        } else {
            Transform::IDENTITY.looking_to(-to_eye, Vec3::Y).rotation
        };
    }
}

/// Despawn each landing, pieces and all, once it has played out.
pub fn cleanup_moonfire_landings(
    mut commands: Commands,
    rigs: Query<(Entity, &MoonfireLandingRig)>,
) {
    for (entity, rig) in rigs.iter() {
        if rig.age >= MOONFIRE_LANDING_SECS {
            // Hierarchy-aware in Bevy 0.16: the pieces go with it.
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(m: &Mesh) -> Vec<[f32; 4]> {
        match m.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(bevy::render::mesh::VertexAttributeValues::Float32x4(c)) => c.clone(),
            _ => panic!("no vertex colours"),
        }
    }
    fn pos(m: &Mesh) -> Vec<[f32; 3]> {
        match m.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::render::mesh::VertexAttributeValues::Float32x3(p)) => p.clone(),
            _ => panic!("no positions"),
        }
    }

    #[test]
    fn the_landing_plays_out_inside_its_life() {
        for layer in [Layer::Orb, Layer::Beam, Layer::Base, Layer::Wisps] {
            assert_eq!(
                envelope(layer.envelope(), 0.0),
                0.0,
                "{layer:?} starts dark"
            );
            assert_eq!(
                envelope(layer.envelope(), bench_ms(MOONFIRE_LANDING_SECS)),
                0.0,
                "{layer:?} is dark by the end of the landing"
            );
        }
        // Bright from about 150 ms to 1000 ms, half by 1500.
        assert_eq!(envelope(BEAM_ENVELOPE, 150.0), 1.0);
        assert_eq!(envelope(BEAM_ENVELOPE, 1000.0), 1.0);
        assert!((envelope(BEAM_ENVELOPE, 1500.0) - 0.55).abs() < 1e-6);
    }

    /// The beam is ONE mesh whose soft edge is vertex colour: dark at its outer
    /// edge, white-hot at its centre line, violet between.
    #[test]
    fn the_beam_is_a_white_core_in_a_violet_halo() {
        let mesh = build_band_mesh(&beam_bands());
        let (p, c) = (pos(&mesh), rgb(&mesh));
        let halo = MOONFIRE_BEAM_WIDTH * 3.0;
        for (p, c) in p.iter().zip(&c) {
            if (p[0].abs() - halo).abs() < 1e-6 {
                assert_eq!(&c[..3], &[0.0, 0.0, 0.0], "the halo's edge is dark");
            }
            if p[0] == 0.0 {
                assert!(
                    c[0] >= 1.0 && c[1] >= 1.0 && c[2] >= 1.0,
                    "the core is white: {c:?}"
                );
            }
            if (p[0].abs() - MOONFIRE_BEAM_WIDTH * 1.4).abs() < 1e-6 {
                assert!(
                    c[2] > c[0] && c[0] > c[1],
                    "between, the beam is violet: {c:?}"
                );
            }
        }
        let top = p.iter().map(|p| p[1]).fold(0.0, f32::max);
        assert_eq!(top, MOONFIRE_ORB_HEIGHT, "the beam reaches the orb");
    }

    /// A vertex at every breakpoint makes the mesh's linear interpolation the
    /// bench's own profile, so a sample between vertices matches it.
    #[test]
    fn the_beam_profile_is_exact_between_vertices() {
        let bands = beam_bands();
        let x = MOONFIRE_BEAM_WIDTH * 0.9; // between the 0.5 and 1.4 knots
        let (x0, x1) = (MOONFIRE_BEAM_WIDTH * 0.5, MOONFIRE_BEAM_WIDTH * 1.4);
        let u = (x - x0) / (x1 - x0);
        let lerped = band_light(&bands, x0, 1.0).lerp(band_light(&bands, x1, 1.0), u);
        assert!((lerped - band_light(&bands, x, 1.0)).length() < 1e-5);
    }

    #[test]
    fn the_base_column_drops_its_inner_band_above_its_top() {
        let bands = base_column_bands();
        let high = band_light(&bands, 0.0, 2.0);
        let low = band_light(&bands, 0.0, 1.0);
        assert!(
            low.x > high.x,
            "the inner band lights only the lower column"
        );
        assert_eq!(
            band_light(&bands, 0.0, 2.7),
            Vec3::ZERO,
            "nothing above 2.6"
        );
    }

    #[test]
    fn the_orb_fades_to_nothing_at_its_rim() {
        let glows = orb_glows();
        let rim = glows.iter().map(|g| g.radius).fold(0.0, f32::max);
        assert_eq!(glow_light(&glows, rim), Vec3::ZERO);
        let centre = glow_light(&glows, 0.0);
        assert!(
            centre.min_element() > 0.9,
            "the moon's centre is white: {centre:?}"
        );
        assert_eq!(*orb_rings().first().unwrap(), 0.0);
    }

    #[test]
    fn the_wisps_curl_up_from_outside_the_body() {
        for w in 0..MOONFIRE_WISPS {
            let pts = wisp_points(w, 1000.0);
            assert_eq!(pts.len(), WISP_POINTS + 1);
            for (p, _) in &pts {
                assert!(Vec2::new(p.x, p.z).length() > COMBATANT_BODY_RADIUS);
            }
            let top = pts.last().unwrap().0.y;
            assert!(
                (top - WISP_RISE).abs() < 1e-5,
                "grown wisps reach {WISP_RISE}"
            );
        }
        // Still rising at 300 ms.
        assert!(wisp_points(0, 300.0).last().unwrap().0.y < WISP_RISE);
    }
}
