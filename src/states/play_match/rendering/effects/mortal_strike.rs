//! Signature Mortal Strike visual effect (graphical-only).
//!
//! The signature is the WEAPON TRAIL: a crimson streak that follows the blade
//! through its rising-diagonal arc, which is how Classic-era warrior ability
//! visuals read. The arc itself lives in `weapon_swing.rs`
//! ([`SwingArc::RisingDiagonal`]); this module draws the streak it leaves, plus
//! a short impact flash and struck-metal sparks at the contact point.
//!
//! Deliberately NOT blood: Rend owns the bleed-drip vocabulary
//! (`affliction.rs`), and reusing it would make Mortal Strike's hit read as a
//! large Rend tick. Nothing here touches the victim's body — the lingering
//! Mortal Wounds debuff states itself elsewhere, by breaking incoming heals
//! (`mortal_wounds.rs`).
//!
//! All randomness is a self-contained visual-only hash, never the sim
//! `game_rng`, and every system here is registered in `states/mod.rs` only, so
//! headless stays byte-identical.

use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use crate::states::play_match::components::*;

// --- Tuning knobs -----------------------------------------------------------

/// How long a trail sample survives after it is laid down (seconds). The whole
/// streak is this much of the blade's recent path, so it doubles as the trail's
/// apparent length.
const TRAIL_LIFETIME: f32 = 0.22;
/// How far down the blade from the tip the ribbon's inner edge sits (model
/// units along the weapon's local +Y, its haft axis). The ribbon spans tip to
/// this point, so a larger value is a wider streak.
const TRAIL_SPAN: f32 = 0.55;
/// Distance from the socket origin (the grip) to the blade tip, in the
/// weapon's local frame. The mount's own scale is applied on top by the
/// socket's `GlobalTransform`.
const TRAIL_TIP_LOCAL: f32 = 1.25;
/// Peak trail opacity at the leading edge.
const TRAIL_ALPHA: f32 = 0.68;
/// Trail crimson. Kept clear of Berserker Rage's additive orange-red
/// `(1.0, 0.35, 0.1)` by being markedly redder, and clear of Rend's opaque
/// blood by being an additive light streak rather than a liquid.
const TRAIL_BASE_COLOR: (f32, f32, f32) = (0.72, 0.10, 0.06);
const TRAIL_EMISSIVE: (f32, f32, f32) = (2.6, 0.22, 0.14);
/// Below this many samples there is no quad to build yet.
const TRAIL_MIN_SAMPLES: usize = 2;
/// Hard cap on retained samples — a frame spike must not grow the mesh without
/// bound.
const TRAIL_MAX_SAMPLES: usize = 96;

/// Impact flash radius (yards) and lifetime (seconds).
///
/// Deliberately SMALLER and SHORTER than the sparks' reach and life. The flash
/// is additive, so anything inside it is washed out rather than lit — at a
/// radius comparable to how far a spark travels in the flash's lifetime, the
/// debris spends its whole visible phase submerged and the hit reads as a plain
/// glowing ball. Keep `FLASH_RADIUS` well under `SPARK_SPEED * FLASH_LIFETIME`
/// so the sparks are clear of it almost immediately.
const FLASH_RADIUS: f32 = 0.38;
const FLASH_LIFETIME: f32 = 0.11;
/// Fraction of the flash's life spent snapping open. It pops to full and then
/// collapses, rather than expanding as it fades — an expanding bubble covers
/// the most screen area exactly when the sparks need to be seen.
const FLASH_SNAP: f32 = 0.2;
/// The flash's own emissive, dimmer than the trail's. It is a struck-metal
/// core, not the brightest thing on screen; the sparks carry the impact.
const FLASH_EMISSIVE: (f32, f32, f32) = (1.7, 0.28, 0.17);

/// Struck-metal sparks — debris on the physics-lite recipe, not gore. These
/// carry the impact, so they outlive and outrun the flash (see `FLASH_RADIUS`).
const SPARK_COUNT: u32 = 14;
const SPARK_SPEED: f32 = 7.5;
const SPARK_GRAVITY: f32 = 15.0;
const SPARK_LIFETIME: f32 = 0.34;
const SPARK_LENGTH: f32 = 0.13;
/// Per-spark speed varies in `SPARK_SPEED * [MIN, MIN + SPAN]`. The floor is
/// deliberately well above zero: a near-stationary spark would sit inside the
/// flash for its whole life, which is the artifact these bounds exist to
/// prevent (see `flash_cannot_swallow_the_sparks`).
const SPARK_SPEED_MIN: f32 = 0.7;
const SPARK_SPEED_SPAN: f32 = 0.6;
/// Crits get a visibly bigger flourish. Cosmetic only — this scale is never
/// read by sim code and carries no balance meaning.
const CRIT_SCALE: f32 = 1.35;

// --- Runtime components (graphical-only) ------------------------------------

/// One live Mortal Strike weapon trail, following its owner's main-hand blade.
///
/// Samples the socket's world-space blade segment every frame while the stroke
/// is playing, then stops sampling and lets the tail age out. Owner-scoped by
/// construction (one trail entity per stroke, holding its owner), so two
/// Warriors striking at once never share geometry.
#[derive(Component)]
pub struct MortalStrikeTrail {
    /// The SIM combatant swinging (not the socket or the body child).
    owner: Entity,
    /// World-space (tip, inner) pairs, oldest first.
    samples: Vec<(Vec3, Vec3)>,
    /// Seconds of stroke left to sample. Counts down past zero: once negative
    /// the blade has stopped and the tail is fading, and the trail is despawned
    /// at `-TRAIL_LIFETIME`.
    sampling_left: f32,
    mesh: Handle<Mesh>,
}

/// The impact flash at the contact point: an additive sphere that expands and
/// fades over [`FLASH_LIFETIME`].
#[derive(Component)]
pub struct MortalStrikeFlash {
    lifetime: f32,
    initial_lifetime: f32,
    radius: f32,
    material: Handle<StandardMaterial>,
}

/// One struck-metal spark. A transient, unattached world particle with
/// ballistic motion that self-expires — no owner bookkeeping needed (the
/// physics-lite debris recipe).
#[derive(Component)]
pub struct MortalStrikeSpark {
    velocity: Vec3,
    lifetime: f32,
    initial_lifetime: f32,
}

// --- Visual-only jitter (never game_rng) ------------------------------------

/// Cheap deterministic jitter in `[0, 1)` from a seed. Mirrors
/// `affliction::drip_jitter` — visual-only, so it never perturbs the sim's
/// seeded `GameRng` and headless stays byte-identical.
fn spark_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

// --- Spawn ------------------------------------------------------------------

/// Spawn the full Mortal Strike flourish. Called from the instant-attack router
/// (`instant_attack.rs`) at the landed hit, with the stroke already started on
/// the attacker's socket.
///
/// `stroke_secs` is the styled stroke's total duration, so the trail samples
/// for exactly as long as the blade is moving.
pub fn spawn_mortal_strike_flourish(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    attacker: Entity,
    impact: Vec3,
    is_crit: bool,
    stroke_secs: f32,
) {
    let scale = if is_crit { CRIT_SCALE } else { 1.0 };
    let (br, bg, bb) = TRAIL_BASE_COLOR;
    let (er, eg, eb) = TRAIL_EMISSIVE;

    // --- trail -------------------------------------------------------------
    // An empty placeholder mesh; `update_mortal_strike_trail` rewrites its
    // attributes each frame from the accumulated samples.
    let mesh = meshes.add(empty_trail_mesh());
    let trail_material = materials.add(StandardMaterial {
        base_color: Color::srgb(br, bg, bb),
        emissive: LinearRgba::new(er, eg, eb, 1.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        cull_mode: None, // the ribbon is a thin band, visible from both sides
        ..default()
    });
    commands.spawn((
        MortalStrikeTrail {
            owner: attacker,
            samples: Vec::new(),
            sampling_left: stroke_secs,
            mesh: mesh.clone(),
        },
        Mesh3d(mesh),
        MeshMaterial3d(trail_material),
        Transform::IDENTITY,
        PlayMatchEntity,
    ));

    // --- impact flash ------------------------------------------------------
    let (fer, feg, feb) = FLASH_EMISSIVE;
    let flash_material = materials.add(StandardMaterial {
        base_color: Color::srgb(br, bg, bb),
        emissive: LinearRgba::new(fer, feg, feb, 1.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    commands.spawn((
        MortalStrikeFlash {
            lifetime: FLASH_LIFETIME,
            initial_lifetime: FLASH_LIFETIME,
            radius: FLASH_RADIUS * scale,
            material: flash_material.clone(),
        },
        Mesh3d(meshes.add(Sphere::new(1.0))),
        MeshMaterial3d(flash_material),
        Transform::from_translation(impact).with_scale(Vec3::splat(0.01)),
        PlayMatchEntity,
    ));

    // --- sparks ------------------------------------------------------------
    let spark_mesh = meshes.add(Cuboid::new(0.035, 0.035, SPARK_LENGTH));
    let spark_material = materials.add(StandardMaterial {
        base_color: Color::srgb(br, bg, bb),
        emissive: LinearRgba::new(er * 1.2, eg * 1.6, eb * 1.6, 1.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    let count = ((SPARK_COUNT as f32) * scale).round() as u32;
    for i in 0..count {
        let seed = attacker.index().wrapping_mul(31).wrapping_add(i.wrapping_mul(2_654_435_761));
        let j1 = spark_jitter(seed);
        let j2 = spark_jitter(seed.wrapping_add(7));
        let j3 = spark_jitter(seed.wrapping_add(19));
        // A cone biased up and outward, following the rising cut.
        let yaw = j1 * std::f32::consts::TAU;
        let pitch = 0.25 + 0.9 * j2;
        let speed = SPARK_SPEED * (SPARK_SPEED_MIN + SPARK_SPEED_SPAN * j3) * scale;
        let velocity = Vec3::new(
            yaw.cos() * pitch.cos() * speed,
            pitch.sin() * speed,
            yaw.sin() * pitch.cos() * speed,
        );
        let life = SPARK_LIFETIME * (0.7 + 0.6 * j3);
        commands.spawn((
            MortalStrikeSpark { velocity, lifetime: life, initial_lifetime: life },
            Mesh3d(spark_mesh.clone()),
            MeshMaterial3d(spark_material.clone()),
            Transform::from_translation(impact),
            PlayMatchEntity,
        ));
    }
}

/// An empty triangle-list mesh with the attributes the trail rebuild writes.
/// Spawning with a real (if degenerate) mesh keeps the render pipeline happy
/// on the first frame, before any samples exist.
fn empty_trail_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    mesh.insert_indices(Indices::U32(Vec::new()));
    mesh
}

// --- Update -----------------------------------------------------------------

/// Update (graphical-only): extend each live trail with this frame's blade
/// segment and rebuild its ribbon mesh.
///
/// The blade's world segment is read from the socket's `GlobalTransform`, so
/// the trail follows the real animated weapon rather than re-deriving the pose.
/// Sampling stops when the stroke's duration is spent; the tail then ages out
/// and the entity despawns in `cleanup_mortal_strike`.
pub fn update_mortal_strike_trail(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut trails: Query<&mut MortalStrikeTrail>,
    sockets: Query<(&WeaponSocket, &GlobalTransform)>,
) {
    let dt = time.delta_secs();
    for mut trail in trails.iter_mut() {
        // Always count down, including PAST zero: the tail's fade-out and the
        // despawn in `cleanup_mortal_strike` both key off how far negative this
        // has gone. Decrementing only while positive strands every trail just
        // short of its despawn condition, leaking one entity per strike.
        let sampling = trail.sampling_left > 0.0;
        trail.sampling_left -= dt;

        if sampling {
            // Main hand only: Mortal Strike is a single-weapon special, and a
            // dual-wielder would otherwise lay down two crossing ribbons.
            let segment = sockets.iter().find_map(|(socket, global)| {
                (socket.owner == trail.owner && socket.hand == WeaponHand::Main).then(|| {
                    let tip = global.transform_point(Vec3::Y * TRAIL_TIP_LOCAL);
                    let inner =
                        global.transform_point(Vec3::Y * (TRAIL_TIP_LOCAL - TRAIL_SPAN));
                    (tip, inner)
                })
            });
            if let Some(seg) = segment {
                trail.samples.push(seg);
                if trail.samples.len() > TRAIL_MAX_SAMPLES {
                    trail.samples.remove(0);
                }
            }
        }

        // Drop samples older than the trail's memory. Samples are laid down one
        // per frame, so the count that fits in TRAIL_LIFETIME depends on frame
        // rate; deriving it from dt keeps the streak the same LENGTH IN TIME at
        // any frame rate rather than the same number of quads.
        let keep = ((TRAIL_LIFETIME / dt.max(1e-4)).ceil() as usize).clamp(2, TRAIL_MAX_SAMPLES);
        if trail.samples.len() > keep {
            let excess = trail.samples.len() - keep;
            trail.samples.drain(0..excess);
        }

        if let Some(mesh) = meshes.get_mut(&trail.mesh) {
            rebuild_trail_mesh(mesh, &trail.samples, fade_of(&trail));
        }
    }
}

/// Overall trail opacity: full while the stroke plays, then a quick fade so the
/// streak dissipates instead of vanishing on a frame boundary.
fn fade_of(trail: &MortalStrikeTrail) -> f32 {
    if trail.sampling_left > 0.0 {
        1.0
    } else {
        (1.0 + trail.sampling_left / TRAIL_LIFETIME).clamp(0.0, 1.0)
    }
}

/// Rewrite a trail's ribbon geometry from its samples: a quad strip between the
/// blade-tip path and the inner-edge path, with vertex alpha ramping from
/// nothing at the oldest sample to `fade` at the newest, so the streak tapers
/// off behind the blade.
///
/// Same shape as `build_dispel_ribbon_mesh`'s strip, with the helix centerline
/// swapped for the recorded sweep.
fn rebuild_trail_mesh(mesh: &mut Mesh, samples: &[(Vec3, Vec3)], fade: f32) {
    let n = samples.len();
    if n < TRAIL_MIN_SAMPLES || fade <= 0.0 {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
        mesh.insert_indices(Indices::U32(Vec::new()));
        return;
    }

    let mut positions = Vec::with_capacity(n * 2);
    let mut normals = Vec::with_capacity(n * 2);
    let mut uvs = Vec::with_capacity(n * 2);
    let mut colors = Vec::with_capacity(n * 2);

    for (i, (tip, inner)) in samples.iter().enumerate() {
        // 0 at the oldest sample, 1 at the newest — squared so the tail thins
        // out quickly and the leading edge stays bright.
        let t = i as f32 / (n - 1) as f32;
        let a = t * t * fade;
        positions.push([tip.x, tip.y, tip.z]);
        positions.push([inner.x, inner.y, inner.z]);
        // Unlit material: normals are required by the vertex layout but do not
        // affect shading.
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([t, 0.0]);
        uvs.push([t, 1.0]);
        // The inner edge is dimmer, so the band reads as a streak trailing the
        // edge rather than a flat slab.
        colors.push([1.0, 1.0, 1.0, a * TRAIL_ALPHA]);
        colors.push([1.0, 1.0, 1.0, a * TRAIL_ALPHA * 0.25]);
    }

    let mut indices = Vec::with_capacity((n - 1) * 6);
    for i in 0..(n - 1) as u32 {
        let base = i * 2;
        indices.extend_from_slice(&[base, base + 1, base + 3]);
        indices.extend_from_slice(&[base, base + 3, base + 2]);
    }

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}

/// Update (graphical-only): expand and fade the impact flash.
pub fn update_mortal_strike_flash(
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(&mut MortalStrikeFlash, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut flash, mut transform) in flashes.iter_mut() {
        flash.lifetime -= dt;
        let k = (flash.lifetime / flash.initial_lifetime).clamp(0.0, 1.0);
        // Pop open, then COLLAPSE. An envelope that expands while it fades
        // covers its own debris at exactly the wrong moment; this one is
        // largest while brightest and shrinks out of the sparks' way.
        let elapsed = 1.0 - k;
        let pop = if elapsed < FLASH_SNAP {
            elapsed / FLASH_SNAP
        } else {
            k / (1.0 - FLASH_SNAP)
        };
        transform.scale = Vec3::splat((0.3 + 0.7 * pop.clamp(0.0, 1.0)) * flash.radius);
        if let Some(material) = materials.get_mut(&flash.material) {
            material.base_color = material.base_color.with_alpha(k);
            let (er, eg, eb) = FLASH_EMISSIVE;
            material.emissive = LinearRgba::new(er * k, eg * k, eb * k, 1.0);
        }
    }
}

/// Update (graphical-only): integrate spark ballistics and shrink them out.
pub fn update_mortal_strike_sparks(
    time: Res<Time>,
    mut sparks: Query<(&mut MortalStrikeSpark, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut spark, mut transform) in sparks.iter_mut() {
        spark.lifetime -= dt;
        spark.velocity.y -= SPARK_GRAVITY * dt;
        let velocity = spark.velocity;
        transform.translation += velocity * dt;
        // Point each spark along its own flight.
        if velocity.length_squared() > 1e-6 {
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, velocity.normalize());
        }
        let k = (spark.lifetime / spark.initial_lifetime).clamp(0.0, 1.0);
        transform.scale = Vec3::new(k.max(0.15), k.max(0.15), 0.4 + 0.6 * k);
    }
}

/// Cleanup (graphical-only): despawn spent flourish pieces.
pub fn cleanup_mortal_strike(
    mut commands: Commands,
    trails: Query<(Entity, &MortalStrikeTrail)>,
    flashes: Query<(Entity, &MortalStrikeFlash)>,
    sparks: Query<(Entity, &MortalStrikeSpark)>,
) {
    for (entity, trail) in trails.iter() {
        // Done once the stroke has finished AND the tail has fully aged out.
        if trail.sampling_left <= -TRAIL_LIFETIME {
            commands.entity(entity).despawn();
        }
    }
    for (entity, flash) in flashes.iter() {
        if flash.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    for (entity, spark) in sparks.iter() {
        if spark.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trail_mesh_is_empty_below_two_samples() {
        let mut mesh = empty_trail_mesh();
        rebuild_trail_mesh(&mut mesh, &[(Vec3::ZERO, Vec3::Y)], 1.0);
        assert_eq!(mesh.count_vertices(), 0);
    }

    #[test]
    fn trail_mesh_builds_one_quad_per_sample_gap() {
        let mut mesh = empty_trail_mesh();
        let samples = vec![
            (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.5, 0.0)),
            (Vec3::new(1.0, 1.2, 0.0), Vec3::new(1.0, 0.7, 0.0)),
            (Vec3::new(2.0, 1.6, 0.0), Vec3::new(2.0, 1.1, 0.0)),
        ];
        rebuild_trail_mesh(&mut mesh, &samples, 1.0);
        assert_eq!(mesh.count_vertices(), 6, "two vertices per sample");
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("expected U32 indices");
        };
        assert_eq!(indices.len(), 12, "two triangles per sample gap");
    }

    #[test]
    fn trail_alpha_ramps_toward_the_leading_edge() {
        let mut mesh = empty_trail_mesh();
        let samples = vec![
            (Vec3::ZERO, Vec3::Y),
            (Vec3::X, Vec3::X + Vec3::Y),
            (Vec3::X * 2.0, Vec3::X * 2.0 + Vec3::Y),
        ];
        rebuild_trail_mesh(&mut mesh, &samples, 1.0);
        let colors = mesh
            .attribute(Mesh::ATTRIBUTE_COLOR)
            .and_then(|a| match a {
                bevy::render::mesh::VertexAttributeValues::Float32x4(v) => Some(v.clone()),
                _ => None,
            })
            .expect("color attribute");
        // Oldest tip vertex is transparent, newest is at full trail alpha.
        assert!(colors[0][3] < 1e-6, "tail fades to nothing");
        assert!(
            colors[colors.len() - 2][3] > colors[0][3],
            "leading edge is brighter than the tail"
        );
    }

    #[test]
    fn flash_cannot_swallow_the_sparks() {
        // The flash is additive, so debris inside it is washed out, not lit.
        // Even the SLOWEST spark must be clear of the flash before the flash
        // is gone, or the hit reads as a plain glowing ball with no debris —
        // the artifact this bound exists to prevent. Fail-first against the
        // original numbers (0.9yd flash over 0.18s vs 5.0 yd/s sparks, where
        // the slowest travelled 0.45yd inside a 0.9yd sphere).
        let slowest_reach = SPARK_SPEED * SPARK_SPEED_MIN * FLASH_LIFETIME;
        assert!(
            slowest_reach > FLASH_RADIUS,
            "slowest spark reaches {slowest_reach:.3}yd during a {FLASH_LIFETIME}s flash of \
             radius {FLASH_RADIUS}yd — it never escapes the glow"
        );
    }

    #[test]
    fn sparks_outlive_the_flash() {
        // The debris must still be on screen after the flash collapses, so the
        // last thing the eye reads is the spray, not the glow.
        assert!(SPARK_LIFETIME > FLASH_LIFETIME * 2.0);
    }

    #[test]
    fn the_flash_collapses_rather_than_expanding_as_it_fades() {
        // Scale must peak early and shrink; an envelope that grows while it
        // fades covers its own debris exactly when the debris needs to read.
        let scale_at = |elapsed_fraction: f32| {
            let k = 1.0 - elapsed_fraction;
            let pop = if elapsed_fraction < FLASH_SNAP {
                elapsed_fraction / FLASH_SNAP
            } else {
                k / (1.0 - FLASH_SNAP)
            };
            (0.3 + 0.7 * pop.clamp(0.0, 1.0)) * FLASH_RADIUS
        };
        let peak = scale_at(FLASH_SNAP);
        assert!(peak > scale_at(0.0), "the flash snaps open");
        assert!(peak > scale_at(0.6), "and is already collapsing by mid-life");
        assert!(scale_at(1.0) < scale_at(0.6), "and keeps shrinking to the end");
    }

    #[test]
    fn spark_jitter_is_bounded_and_varies() {
        let a = spark_jitter(1);
        let b = spark_jitter(2);
        assert!((0.0..1.0).contains(&a) && (0.0..1.0).contains(&b));
        assert!((a - b).abs() > f32::EPSILON, "distinct seeds give distinct jitter");
    }
}
