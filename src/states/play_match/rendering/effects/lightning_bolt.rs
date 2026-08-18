//! Signature Lightning Bolt visual effect (graphical-only).
//!
//! Consumes the `LightningBoltStrike` marker spawned in the shared casting
//! completion path and renders a stylized "thick nuke arc" flash-crack: a
//! jagged forked bolt from caster to target that appears at full length,
//! flashes bright, and decays quickly, plus a strong impact burst at the
//! target. All randomness here uses a self-contained visual-only PRNG — never
//! the sim `game_rng` — so this module draws no sim RNG and is registered in
//! graphical mode only, keeping the headless sim byte-identical.
//!
//! Geometry knobs (jag, branch count, thickness, color, lifetime) are the
//! consts below and are meant to be tuned in-engine.

use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// --- Tuning knobs -----------------------------------------------------------

/// Chest-height offset applied to both endpoints (entity translations sit near
/// the ground; the bolt should span chest to chest).
const BOLT_HEIGHT: f32 = 1.2;
/// Midpoint-displacement subdivisions for the main channel (2^N segments).
const MAIN_SUBDIVISIONS: u32 = 5;
/// Subdivisions for each fork/branch.
const BRANCH_SUBDIVISIONS: u32 = 3;
/// Number of forks branching off the main channel.
const BRANCH_COUNT: u32 = 4;
/// Coarsest-level jag displacement in world units (halves each subdivision).
const DISPLACE: f32 = 1.0;
/// Cross-section of each bolt segment cuboid (the "thick" in thick nuke arc).
const BOLT_WIDTH: f32 = 0.14;
const BRANCH_MIN_LEN: f32 = 1.5;
const BRANCH_MAX_LEN: f32 = 4.0;
/// Flash-crack lifetime (seconds) for the bolt.
const BOLT_LIFETIME: f32 = 0.30;
/// Fraction of lifetime the bolt holds at full brightness before decaying.
const FLASH_HOLD: f32 = 0.85;
/// Impact-burst lifetime (seconds).
const BURST_LIFETIME: f32 = 0.22;
const BURST_INIT_SCALE: f32 = 0.4;
const BURST_FINAL_SCALE: f32 = 2.2;

/// Bolt base color (RGB, linear-ish); alpha is driven by the flash envelope.
const BOLT_BASE_COLOR: (f32, f32, f32) = (0.60, 0.82, 1.0);
/// Bolt emissive (high for bloom, blue-white).
const BOLT_EMISSIVE: (f32, f32, f32) = (1.4, 2.2, 4.0);
/// Impact-burst emissive (brighter white-blue — NOT the shared purple
/// SpellImpactEffect, which has no per-instance color).
const BURST_EMISSIVE: (f32, f32, f32) = (2.5, 3.0, 4.5);
/// Impact-burst base color (white-blue).
const BURST_BASE_COLOR: (f32, f32, f32) = (0.8, 0.9, 1.0);

// --- Runtime components (graphical-only) ------------------------------------

/// Drives the fade of an active bolt. Lives on the strike entity; the bolt's
/// segment cuboids are its children and share `material`.
#[derive(Component)]
pub struct LightningBoltVisual {
    lifetime: f32,
    initial_lifetime: f32,
    material: Handle<StandardMaterial>,
}

/// A standalone expanding/fading impact burst at the strike's endpoint.
#[derive(Component)]
pub struct LightningBoltBurst {
    lifetime: f32,
    initial_lifetime: f32,
    initial_scale: f32,
    final_scale: f32,
    material: Handle<StandardMaterial>,
}

// --- Visual-only PRNG (never game_rng) --------------------------------------

struct BoltRng(u64);

impl BoltRng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next_f32(&mut self) -> f32 {
        // xorshift64
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next_f32()
    }
}

/// Midpoint-displacement jagged polyline from `start` to `end`.
fn jagged(start: Vec3, end: Vec3, rng: &mut BoltRng, subdivisions: u32, displace: f32) -> Vec<Vec3> {
    let mut pts = vec![start, end];
    let mut d = displace;
    for _ in 0..subdivisions {
        let mut next = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            let a = w[0];
            let b = w[1];
            next.push(a);
            let seg = b - a;
            let len = seg.length().max(0.001);
            let dir = seg / len;
            // Two perpendicular axes so the jag leaves the caster->target line
            // in 3D rather than staying planar.
            let helper = if dir.dot(Vec3::Y).abs() > 0.9 { Vec3::X } else { Vec3::Y };
            let perp1 = dir.cross(helper).normalize();
            let perp2 = dir.cross(perp1).normalize();
            let mid = (a + b) * 0.5;
            next.push(mid + perp1 * rng.range(-d, d) + perp2 * rng.range(-d, d) * 0.5);
        }
        next.push(*pts.last().unwrap());
        pts = next;
        d *= 0.55;
    }
    pts
}

/// Transform for one bolt segment: a unit cuboid stretched along its axis.
fn segment_transform(a: Vec3, b: Vec3) -> Transform {
    let seg = b - a;
    let len = seg.length().max(0.001);
    let rotation = Quat::from_rotation_arc(Vec3::Y, seg / len);
    Transform {
        translation: (a + b) * 0.5,
        rotation,
        scale: Vec3::new(BOLT_WIDTH, len, BOLT_WIDTH),
    }
}

/// Spawn the bolt mesh + impact burst for each newly created strike.
pub fn spawn_lightning_bolt(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Cached unit meshes — identical for every strike, so build once and clone
    // the handles instead of re-uploading GPU buffers per cast.
    mut unit_meshes: Local<Option<(Handle<Mesh>, Handle<Mesh>)>>,
    strikes: Query<(Entity, &LightningBoltStrike), (Added<LightningBoltStrike>, Without<LightningBoltVisual>)>,
) {
    let (seg_mesh, burst_mesh) = unit_meshes
        .get_or_insert_with(|| {
            (meshes.add(Cuboid::new(1.0, 1.0, 1.0)), meshes.add(Sphere::new(1.0)))
        })
        .clone();

    for (entity, strike) in strikes.iter() {
        // Seed the visual-only RNG from the entity id + endpoints so each strike
        // varies without touching the sim RNG stream.
        let seed = (entity.index() as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ ((strike.start.x.to_bits() as u64) << 1)
            ^ ((strike.start.z.to_bits() as u64) << 17)
            ^ ((strike.end.x.to_bits() as u64) << 31)
            ^ ((strike.end.z.to_bits() as u64) << 5);
        let mut rng = BoltRng::new(seed);

        let start = strike.start + Vec3::Y * BOLT_HEIGHT;
        let end = strike.end + Vec3::Y * BOLT_HEIGHT;

        // One shared additive/emissive material for every segment of this bolt,
        // so the fade in update_lightning_bolt mutates a single asset.
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(BOLT_BASE_COLOR.0, BOLT_BASE_COLOR.1, BOLT_BASE_COLOR.2, 1.0),
            emissive: LinearRgba::rgb(BOLT_EMISSIVE.0, BOLT_EMISSIVE.1, BOLT_EMISSIVE.2),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });

        // Main channel.
        let main = jagged(start, end, &mut rng, MAIN_SUBDIVISIONS, DISPLACE);

        // Collect every segment transform (main + forks) before spawning children.
        let mut seg_transforms: Vec<Transform> = Vec::new();
        for w in main.windows(2) {
            seg_transforms.push(segment_transform(w[0], w[1]));
        }
        for _ in 0..BRANCH_COUNT {
            if main.len() < 3 {
                break;
            }
            let idx = 1 + (rng.next_f32() * (main.len() as f32 - 2.0)) as usize;
            let p = main[idx.min(main.len() - 1)];
            let toward = (end - p).normalize_or_zero();
            let base_dir = if toward == Vec3::ZERO { Vec3::Y } else { toward };
            let axis = Vec3::new(
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
            )
            .normalize_or_zero();
            let axis = if axis == Vec3::ZERO { Vec3::Y } else { axis };
            let dir = Quat::from_axis_angle(axis, rng.range(0.5, 1.4)) * base_dir;
            let branch_end = p + dir * rng.range(BRANCH_MIN_LEN, BRANCH_MAX_LEN);
            let branch = jagged(p, branch_end, &mut rng, BRANCH_SUBDIVISIONS, DISPLACE * 0.5);
            for w in branch.windows(2) {
                seg_transforms.push(segment_transform(w[0], w[1]));
            }
        }

        commands
            .entity(entity)
            .try_insert((
                LightningBoltVisual {
                    lifetime: BOLT_LIFETIME,
                    initial_lifetime: BOLT_LIFETIME,
                    material: material.clone(),
                },
                // Identity transform so children (spawned in world space) render
                // at their absolute positions, and so transform/visibility
                // propagation reaches them.
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|parent| {
                for t in &seg_transforms {
                    parent.spawn((Mesh3d(seg_mesh.clone()), MeshMaterial3d(material.clone()), *t));
                }
            });

        // Impact burst at the strike endpoint — bespoke white-blue, expands and
        // fades. NOT SpellImpactEffect (which is hardcoded purple, shared with
        // Mind Blast, with no per-instance color).
        let burst_material = materials.add(StandardMaterial {
            base_color: Color::srgba(BURST_BASE_COLOR.0, BURST_BASE_COLOR.1, BURST_BASE_COLOR.2, 1.0),
            emissive: LinearRgba::rgb(BURST_EMISSIVE.0, BURST_EMISSIVE.1, BURST_EMISSIVE.2),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        commands.spawn((
            LightningBoltBurst {
                lifetime: BURST_LIFETIME,
                initial_lifetime: BURST_LIFETIME,
                initial_scale: BURST_INIT_SCALE,
                final_scale: BURST_FINAL_SCALE,
                material: burst_material.clone(),
            },
            Mesh3d(burst_mesh.clone()),
            MeshMaterial3d(burst_material),
            Transform::from_translation(end).with_scale(Vec3::splat(BURST_INIT_SCALE)),
            PlayMatchEntity,
        ));
    }
}

/// Drive the flash-crack envelope for bolts and the expand/fade for bursts.
pub fn update_lightning_bolt(
    time: Res<Time>,
    mut bolts: Query<&mut LightningBoltVisual>,
    mut bursts: Query<(&mut LightningBoltBurst, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for mut bolt in bolts.iter_mut() {
        bolt.lifetime -= dt;
        let t = (bolt.lifetime / bolt.initial_lifetime).clamp(0.0, 1.0);
        // Hold at full brightness, then decay quickly — the "crack".
        let intensity = if t > FLASH_HOLD { 1.0 } else { t / FLASH_HOLD };
        if let Some(mat) = materials.get_mut(&bolt.material) {
            let (r, g, b) = BOLT_EMISSIVE;
            mat.emissive = LinearRgba::rgb(r * intensity, g * intensity, b * intensity);
            let (cr, cg, cb) = BOLT_BASE_COLOR;
            mat.base_color = Color::srgba(cr, cg, cb, intensity);
        }
    }

    for (mut burst, mut transform) in bursts.iter_mut() {
        burst.lifetime -= dt;
        let t = (burst.lifetime / burst.initial_lifetime).clamp(0.0, 1.0);
        let progress = 1.0 - t;
        let scale = burst.initial_scale + (burst.final_scale - burst.initial_scale) * progress;
        transform.scale = Vec3::splat(scale);
        if let Some(mat) = materials.get_mut(&burst.material) {
            let (r, g, b) = BURST_EMISSIVE;
            mat.emissive = LinearRgba::rgb(r * t, g * t, b * t);
            let (cr, cg, cb) = BURST_BASE_COLOR;
            mat.base_color = Color::srgba(cr, cg, cb, t);
        }
    }
}

/// Despawn expired bolts (with their segment children) and bursts.
pub fn cleanup_lightning_bolt(
    mut commands: Commands,
    bolts: Query<(Entity, &LightningBoltVisual)>,
    bursts: Query<(Entity, &LightningBoltBurst)>,
) {
    for (entity, bolt) in bolts.iter() {
        if bolt.lifetime <= 0.0 {
            // despawn is hierarchy-aware in Bevy 0.16 — children go with it.
            commands.entity(entity).despawn();
        }
    }
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
