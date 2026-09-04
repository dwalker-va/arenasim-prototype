use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use crate::states::play_match::components::*;
use crate::states::match_config::CharacterClass;
use super::dispel_burst::dispel_burst_colors;
use super::spell_bolts::soft_dot_texture;

// ==============================================================================
// Dispel Ribbon Visual Effects
// ==============================================================================
// A rippling ribbon that coils AROUND the dispelled combatant and climbs it —
// the "you got cleansed" indicator (see DispelRibbon). Graphical only;
// registered in states/mod.rs, never in headless systems.rs.
//
// The ribbon is the whole treatment. An earlier pass added a flash-and-band
// beat at the chest to mark the instant, borrowed from the damage impacts, and
// it overpowered the ribbon: a dispel read as a hit with a strip behind it. The
// instant is now marked in the ribbon's own vocabulary — it IGNITES (a bright
// emissive spike that settles in a quarter second) and a fold rolls along it
// from the held end — so there is one shape on screen and it is the right one.
//
// It ends by PLAYING OUT, not fading: partway up, the top end fixes in place
// and the bottom end keeps rising, consuming the strip from below like a
// ribbon drawn through a ring, while sparks stream off the fixed top until the
// bottom has caught up and nothing is left. An opaque ribbon cannot fade by
// alpha anyway, and a fade was the wrong shape: the aura LEAVES the unit.

/// Number of turns the ribbon helix coils through over its baked length. Fewer
/// turns over the same height is a steeper, shorter strip — less ribbon to
/// consume, so the play-out finishes sooner with the same vertical endpoints.
const RIBBON_TURNS: f32 = 1.5;
/// Baked vertical span (yards) of the helix geometry itself.
const RIBBON_HEIGHT: f32 = 1.4;
/// Ribbon band width (yards). Thin so it reads as a defined ribbon, not a blob.
const RIBBON_WIDTH: f32 = 0.18;
/// Radius of the combatant capsule (`Capsule3d::new(0.5, 1.5)`).
const BODY_RADIUS: f32 = 0.5;
/// Horizontal coil radius (yards). Must clear [`BODY_RADIUS`] by more than the
/// ripple's inward swing plus half the band, or the coil is INSIDE the mesh
/// and invisible until it climbs above the head — which is exactly why the
/// first build (0.35) could only be seen on Purge, whose coil starts at chest
/// height and pokes out over the crown. Pinned by
/// `the_coil_wraps_outside_the_body`.
const RIBBON_RADIUS: f32 = 0.92;
/// Number of strip segments along the helix (mesh resolution). High enough
/// that a fold spans a dozen segments and reads as cloth, not facets.
const RIBBON_SEGMENTS: usize = 96;

/// Where the helix STARTS, measured from the combatant's transform — which is
/// the capsule's CENTRE, spanning -1.25..+1.25. The first build anchored the
/// ribbon at +1.9, reasoning from a capsule standing on its transform; on the
/// real rig that is 0.65yd above the top of the head, so the whole strip played
/// in empty air and rose further from there.
///
/// The Classic client anchors Dispel Magic (`dispel_low_base.m2`), Cleanse
/// (`clense_base.m2`) and Devour Magic (`arcanespirit_impact_base.m2`) at
/// BASE attachment 19 — the feet — and only Purge (`purge_new_impact_chest.m2`)
/// at the chest. So the friendly dispels start at the feet and climb the whole
/// body to the head; the Shaman's Purge starts around the chest and lifts off
/// it. Both keep the coil ON the unit for their whole life.
const RIBBON_BASE_START: f32 = -1.25;
const RIBBON_BASE_RISE: f32 = 1.6;
/// Centred on the chest anchor the shared impact uses.
const RIBBON_CHEST_START: f32 = super::IMPACT_CHEST_Y - RIBBON_HEIGHT * 0.5;
const RIBBON_CHEST_RISE: f32 = 0.7;
/// Spin rate (radians/sec) of the ribbon's slow Y-axis rotation.
const RIBBON_SPIN_RATE: f32 = 3.0;

/// The fold: hold a ribbon at one end, lift it sharply and pull it back down,
/// and a single fold rolls away along its length, shrinking as it goes. That
/// is the motion — NOT a standing wave over the whole strip, which the first
/// ripple was and which read as tessellation. The fold is a derivative-of-
/// Gaussian wavelet (an up lobe then a down lobe) in the strip's own parameter
/// `t`, launched at the bottom end at birth, travelling toward the top, and
/// displacing each ring along the band's surface normal (mostly up, slightly
/// outward). A second, smaller fold follows — the echo of the flick.
///
/// `FOLD_AMP` is the peak displacement in yards; `FOLD_SIGMA` the fold's width
/// as a fraction of the strip's length (about seven segments at 96); `FOLD_SPEED`
/// how much of the strip it crosses per second; `FOLD_DECAY` how much the
/// amplitude has attenuated (as a power of e) by the time it reaches the top.
const FOLD_AMP: f32 = 0.24;
const FOLD_SIGMA: f32 = 0.07;
const FOLD_SPEED: f32 = 1.7;
const FOLD_DECAY: f32 = 1.1;
/// The echo: launched this many seconds after the first fold, at this fraction
/// of its amplitude.
const FOLD_ECHO_DELAY: f32 = 0.32;
const FOLD_ECHO_SCALE: f32 = 0.55;
/// Outward bulge as a fraction of the vertical displacement — the band lies on
/// a ramp, so its normal leans outward a little. Always positive (a fold bulges
/// out on both lobes), so the coil never swings toward the body.
const FOLD_OUTWARD: f32 = 0.35;

/// The play-out: the last fraction of the life during which the top end is
/// fixed and the bottom end rises through the strip, consuming it. Before it,
/// the whole helix climbs; the climb is complete exactly when the play-out
/// begins. `ribbon_climb` / `ribbon_consumed` are the two curves.
const PLAYOUT_FRACTION: f32 = 0.45;

/// Sparks stream off the fixed top end while the strip plays out — the aura
/// leaving. Additive soft dots in the class colour, rising and shrinking.
const SPARK_RATE: f32 = 48.0;
const SPARK_RISE: f32 = 1.5;
const SPARK_SCATTER: f32 = 0.45;
const SPARK_LIFE: f32 = 0.55;
const SPARK_RADIUS: f32 = 0.09;

/// The ribbon is OPAQUE. This is load-bearing, not a style choice. The body
/// capsule is an alpha-blended material (for stealth), and Bevy draws blended
/// meshes back-to-front by entity distance WITHOUT depth writes — so a blended
/// ribbon sorted before the capsule was painted over by it everywhere,
/// including the half of every coil that was nearer the camera. Only the parts
/// outside the body's silhouette survived, which is what the first screenshots
/// showed. An opaque ribbon writes depth and sits in front of the body where it
/// is in front of the body. Pinned by `the_ribbon_writes_depth`. It cannot
/// fade by alpha, which is why it plays out instead.

/// Ignition: the emissive spikes to `1 + IGNITE_BOOST` times its resting
/// value at spawn and decays with this time constant. This is what marks the
/// INSTANT of the dispel, in place of the flash-and-band beat that used to.
const IGNITE_BOOST: f32 = 3.0;
const IGNITE_SECS: f32 = 0.28;

/// Color for the dispel ribbon: the same class hue as the burst, but near-opaque
/// with a moderated emissive so it reads as a *solid* ribbon (a colored surface
/// with a sheen) rather than the wispy additive glow of the sphere bursts. Kept
/// separate from `dispel_burst_colors` so the burst (Master's Call) is unaffected.
fn dispel_ribbon_colors(class: CharacterClass) -> (Color, LinearRgba) {
    let (base, emissive) = dispel_burst_colors(class);
    (
        // Opaque on screen; the alpha here is inert (see `FADE_TAPER`).
        base.with_alpha(1.0),
        // Trim the emissive so it's a colored sheen + light bloom, not a pure glow.
        LinearRgba::new(emissive.red * 0.6, emissive.green * 0.6, emissive.blue * 0.6, 1.0),
    )
}

/// Whether this dispeller's ribbon starts at the feet (the source's BASE
/// attachment) or around the chest (Purge's chest attachment).
pub fn ribbon_starts_at_base(class: CharacterClass) -> bool {
    class != CharacterClass::Shaman
}

/// How far the helix has climbed, 0..1, at a lifetime progress (`1.0` = just
/// spawned). The climb completes when the play-out begins and holds there.
pub fn ribbon_climb(progress: f32) -> f32 {
    ((1.0 - progress.clamp(0.0, 1.0)) / (1.0 - PLAYOUT_FRACTION)).clamp(0.0, 1.0)
}

/// How much of the strip has been consumed from the bottom, 0..1, at a
/// lifetime progress. Zero until the play-out begins; one at expiry.
pub fn ribbon_consumed(progress: f32) -> f32 {
    ((PLAYOUT_FRACTION - progress.clamp(0.0, 1.0)) / PLAYOUT_FRACTION).clamp(0.0, 1.0)
}

/// World position of the helix's base ring (parameter `t = 0`, whether or not
/// that part of the strip still exists) for a given victim transform and
/// lifetime progress. Pure, so the on-body claim can be asserted directly.
pub fn ribbon_origin(class: CharacterClass, target: Vec3, progress: f32) -> Vec3 {
    let (start, rise) = if ribbon_starts_at_base(class) {
        (RIBBON_BASE_START, RIBBON_BASE_RISE)
    } else {
        (RIBBON_CHEST_START, RIBBON_CHEST_RISE)
    };
    target + Vec3::Y * (start + ribbon_climb(progress) * rise)
}

/// The top ring's centre in the rig's frame — where the sparks come from.
pub fn ribbon_top_local() -> Vec3 {
    let angle = RIBBON_TURNS * std::f32::consts::TAU;
    Vec3::new(angle.cos() * RIBBON_RADIUS, RIBBON_HEIGHT, angle.sin() * RIBBON_RADIUS)
}

/// The helix's vertical extent, for probes.
pub fn ribbon_height() -> f32 {
    RIBBON_HEIGHT
}

/// The coil radius and the body radius it must clear, for probes.
pub fn ribbon_radii() -> (f32, f32) {
    (RIBBON_RADIUS, BODY_RADIUS)
}

/// Emissive multiplier at `age` seconds after the dispel — the ignition, on
/// top of the life-long fade. Pure so the spike can be asserted.
pub fn ribbon_ignition(age: f32) -> f32 {
    1.0 + IGNITE_BOOST * (-age.max(0.0) / IGNITE_SECS).exp()
}

/// Signed vertical displacement (yards) of the strip at parameter `t` (0 at
/// the held bottom end, 1 at the top), `age` seconds after the flick.
///
/// Each fold is `d/dt` of a Gaussian centred where the fold has travelled to:
/// positive (lifted) on the leading side, negative (pulled down) trailing,
/// normalised so its peak is `FOLD_AMP` before attenuation. Pure, so the
/// rolling can be asserted: the peak moves up the strip with age and shrinks.
pub fn ribbon_fold(t: f32, age: f32) -> f32 {
    let one = |launch: f32, scale: f32| -> f32 {
        let since = age - launch;
        if since < 0.0 {
            return 0.0;
        }
        let centre = since * FOLD_SPEED;
        if centre > 1.0 + 3.0 * FOLD_SIGMA {
            return 0.0;
        }
        let x = (t - centre) / FOLD_SIGMA;
        // -x·exp(-x²/2) peaks at |x| = 1 with value exp(-1/2); normalise to 1.
        let wavelet = -x * (-0.5 * x * x).exp() / (-0.5f32).exp();
        // Attenuate with distance travelled; hold the ends of the strip still.
        let atten = (-FOLD_DECAY * centre.clamp(0.0, 1.0)).exp();
        let ends = (t * std::f32::consts::PI).sin().clamp(0.0, 1.0).powf(0.35);
        FOLD_AMP * scale * atten * wavelet * ends
    };
    one(0.0, 1.0) + one(FOLD_ECHO_DELAY, FOLD_ECHO_SCALE)
}

/// Where along the strip the first fold's centre is at `age`, in `t`. Past
/// `1.0` it has run off the top.
pub fn ribbon_fold_centre(age: f32) -> f32 {
    age.max(0.0) * FOLD_SPEED
}

/// The fold's peak displacement at launch, for probes.
pub fn ribbon_fold_amp() -> f32 {
    FOLD_AMP
}


/// Vertex positions of the helix with the fold rolled along it and the bottom
/// `consumed` of it gone.
///
/// The centerline orbits the vertical axis at `radius`; each ring is displaced
/// by [`ribbon_fold`] along the band's normal — up, with an outward lean of
/// `FOLD_OUTWARD` on the fold's magnitude. The band's width never changes:
/// width flutter is what made the first ripple read as tessellation. `age`
/// below zero gives the plain helix. `consumed` (0..1) is how much of the
/// strip has played out from the bottom: the rings are respread over the part
/// that remains, so the vertex count never changes and the strip shortens
/// toward its top end. Two vertices per ring, bottom to top, left then right —
/// the layout the index buffer assumes.
pub fn ribbon_positions(
    turns: f32,
    height: f32,
    width: f32,
    radius: f32,
    segments: usize,
    age: f32,
    consumed: f32,
) -> Vec<[f32; 3]> {
    use std::f32::consts::TAU;
    let t0 = consumed.clamp(0.0, 1.0);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    for i in 0..=segments {
        let u = i as f32 / segments as f32;
        let t = t0 + (1.0 - t0) * u; // where on the ORIGINAL strip this ring sits
        let angle = t * turns * TAU;
        let (sin_a, cos_a) = angle.sin_cos();
        let fold = if age >= 0.0 { ribbon_fold(t, age) } else { 0.0 };
        let r = radius + FOLD_OUTWARD * fold.abs();
        let y = t * height + fold;

        // Centerline orbits the vertical axis, lifted by the fold.
        let center = Vec3::new(cos_a * r, y, sin_a * r);
        // Width is offset along the horizontal radial direction, so the band
        // reads as a coiling ramp rather than a vertical wall.
        let radial = Vec3::new(cos_a, 0.0, sin_a);
        let half = radial * (width * 0.5);

        let left = center - half;
        let right = center + half;
        positions.push([left.x, left.y, left.z]);
        positions.push([right.x, right.y, right.z]);
    }
    positions
}

/// Build the twisting-ribbon mesh: a flat strip of quads whose centerline follows
/// a helix coiling upward. Modeled on `create_arena_floor_mesh` (the codebase's raw-vertex
/// mesh precedent). `radius` must be > 0 so the band coils laterally rather than
/// twisting in place. Returns a `TriangleList` with POSITION / NORMAL / UV_0 and
/// U32 indices; render it double-sided (`cull_mode: None`) since the band is thin.
/// The positions are rewritten every frame by [`update_dispel_ribbons`] to
/// ripple; only the vertex layout has to hold.
fn build_dispel_ribbon_mesh(
    turns: f32,
    height: f32,
    width: f32,
    radius: f32,
    segments: usize,
) -> Mesh {
    let positions = ribbon_positions(turns, height, width, radius, segments, -1.0, 0.0);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(2 * (segments + 1));
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(2 * (segments + 1));
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        // Emissive + blended surface makes lighting negligible; an up-facing
        // normal is a fine, stable approximation for the ramp band.
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([0.0, t]);
        uvs.push([1.0, t]);
    }

    // Two triangles per segment over the 4 edge vertices of segments i and i+1.
    let mut indices: Vec<u32> = Vec::with_capacity(6 * segments);
    for i in 0..segments {
        let bl = (2 * i) as u32; // bottom-left
        let br = (2 * i + 1) as u32; // bottom-right
        let tl = (2 * (i + 1)) as u32; // top-left
        let tr = (2 * (i + 1) + 1) as u32; // top-right
        indices.extend_from_slice(&[bl, br, tr, bl, tr, tl]);
    }

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Spawn visual mesh for new dispel ribbons.
pub fn spawn_dispel_ribbon_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut dot: Local<Option<Handle<Image>>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    new_ribbons: Query<(Entity, &DispelRibbon), (Added<DispelRibbon>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (ribbon_entity, ribbon) in new_ribbons.iter() {
        let Ok(target_transform) = transforms.get(ribbon.target) else {
            continue;
        };

        let (base_color, emissive) = dispel_ribbon_colors(ribbon.caster_class);
        let ignite = ribbon_ignition(0.0);

        // Each ribbon owns its mesh: the positions are rewritten per frame.
        let mesh = meshes.add(build_dispel_ribbon_mesh(
            RIBBON_TURNS,
            RIBBON_HEIGHT,
            RIBBON_WIDTH,
            RIBBON_RADIUS,
            RIBBON_SEGMENTS,
        ));
        let material = materials.add(StandardMaterial {
            base_color: base_color.with_alpha(1.0),
            emissive: LinearRgba::new(
                emissive.red * ignite,
                emissive.green * ignite,
                emissive.blue * ignite,
                1.0,
            ),
            // OPAQUE — a blended ribbon loses the draw-order fight with the
            // blended body capsule and is painted over. See the note above
            // `PLAYOUT_FRACTION`.
            alpha_mode: AlphaMode::Opaque,
            // Thin ribbon: render both faces so it never vanishes from behind.
            cull_mode: None,
            double_sided: true,
            ..default()
        });

        let position = ribbon_origin(ribbon.caster_class, target_transform.translation, 1.0);

        // The sparks' shared sprite and this ribbon's own spark material.
        let dot = dot.get_or_insert_with(|| images.add(soft_dot_texture())).clone();
        let quad = quad
            .get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0)))
            .clone();
        let (spark_color, spark_emissive) = dispel_burst_colors(ribbon.caster_class);
        let spark_material = materials.add(StandardMaterial {
            base_color: spark_color.with_alpha(1.0),
            base_color_texture: Some(dot.clone()),
            emissive: LinearRgba::rgb(
                spark_emissive.red * 1.3,
                spark_emissive.green * 1.3,
                spark_emissive.blue * 1.3,
            ),
            emissive_texture: Some(dot),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        });

        commands.entity(ribbon_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
            NotShadowCaster,
            DispelRibbonRig {
                spark_mesh: quad,
                spark_material,
                emit_carry: 0.0,
                emitted: 0,
            },
        ));
    }
}

/// Cheap deterministic jitter in [0, 1). Visual only.
fn spark_jitter(seed: u32) -> f32 {
    let s = seed
        .wrapping_mul(747_796_405)
        .wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

/// Update dispel ribbons: follow the target, climb the body, spin, roll the
/// fold, ignite, then play out from the bottom while sparks leave the top.
pub fn update_dispel_ribbons(
    mut commands: Commands,
    time: Res<Time>,
    mut ribbons: Query<(
        Entity,
        &mut DispelRibbon,
        &mut DispelRibbonRig,
        &mut Transform,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, (Without<DispelRibbon>, Without<DispelSpark>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut ribbon, mut rig, mut ribbon_transform, mesh_handle, material_handle) in
        ribbons.iter_mut()
    {
        ribbon.lifetime -= dt;
        ribbon.spin += dt * RIBBON_SPIN_RATE;

        // Progress: 1.0 (just spawned) → 0.0 (expired)
        let progress = (ribbon.lifetime / ribbon.initial_lifetime).max(0.0);
        let age = (ribbon.initial_lifetime - ribbon.lifetime).max(0.0);
        let consumed = ribbon_consumed(progress);

        // Follow the target and climb it; once the play-out begins the climb
        // is complete and the base ring holds, so the top end is FIXED in the
        // unit's frame. If the target is gone (died mid-ribbon), freeze at the
        // last anchored position — matches DispelBurst / HealingLightColumn.
        if let Ok(target_transform) = transforms.get(ribbon.target) {
            ribbon_transform.translation =
                ribbon_origin(ribbon.caster_class, target_transform.translation, progress);
        }
        ribbon_transform.rotation = Quat::from_rotation_y(ribbon.spin);

        // The fold rolls up the strip and the bottom plays out: rewrite the
        // vertex positions.
        if let Some(mesh) = meshes.get_mut(&mesh_handle.0) {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                ribbon_positions(
                    RIBBON_TURNS,
                    RIBBON_HEIGHT,
                    RIBBON_WIDTH,
                    RIBBON_RADIUS,
                    RIBBON_SEGMENTS,
                    age,
                    consumed,
                ),
            );
        }

        // Ignite at birth and settle; the strip stays lit until it is gone.
        let (_, emissive) = dispel_ribbon_colors(ribbon.caster_class);
        let ignite = ribbon_ignition(age);
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.emissive = LinearRgba::new(
                emissive.red * ignite,
                emissive.green * ignite,
                emissive.blue * ignite,
                1.0,
            );
        }

        // While the strip plays out, sparks stream off the fixed top end.
        if consumed > 0.0 && consumed < 1.0 {
            rig.emit_carry += SPARK_RATE * dt;
            let top = ribbon_transform.translation + ribbon_transform.rotation * ribbon_top_local();
            while rig.emit_carry >= 1.0 {
                rig.emit_carry -= 1.0;
                let i = rig.emitted;
                rig.emitted += 1;
                let seed = entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
                let a = spark_jitter(seed) * std::f32::consts::TAU;
                let m = SPARK_SCATTER * spark_jitter(seed ^ 0x51ED);
                let rise = SPARK_RISE * (0.7 + 0.6 * spark_jitter(seed ^ 0x27D4));
                commands.spawn((
                    DispelSpark {
                        velocity: Vec3::new(a.cos() * m, rise, a.sin() * m),
                        age: 0.0,
                        life: SPARK_LIFE,
                        radius: SPARK_RADIUS,
                    },
                    Mesh3d(rig.spark_mesh.clone()),
                    MeshMaterial3d(rig.spark_material.clone()),
                    Transform::from_translation(top).with_scale(Vec3::splat(SPARK_RADIUS * 2.0)),
                    NotShadowCaster,
                    PlayMatchEntity,
                ));
            }
        }
    }
}

/// Drive the sparks: rise, shrink, face the camera, and retire. They are
/// world-space particles, unattached, so a ribbon that ends mid-stream leaves
/// its last sparks to finish on their own.
pub fn update_dispel_sparks(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&Transform, (With<Camera3d>, Without<DispelSpark>)>,
    mut sparks: Query<(Entity, &mut DispelSpark, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let facing = camera.iter().next().map(|t| t.rotation);
    for (entity, mut spark, mut transform) in sparks.iter_mut() {
        spark.age += dt;
        if spark.age >= spark.life {
            commands.entity(entity).despawn();
            continue;
        }
        let velocity = spark.velocity;
        transform.translation += velocity * dt;
        let k = 1.0 - spark.age / spark.life;
        transform.scale = Vec3::splat((spark.radius * 2.0 * k.powf(0.6)).max(1e-4));
        if let Some(rotation) = facing {
            transform.rotation = rotation;
        }
    }
}

/// Cleanup expired dispel ribbons.
pub fn cleanup_expired_dispel_ribbons(
    mut commands: Commands,
    ribbons: Query<(Entity, &DispelRibbon)>,
) {
    for (entity, ribbon) in ribbons.iter() {
        if ribbon.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod dispel_ribbon_mesh_tests {
    use super::*;
    use std::f32::consts::TAU;

    fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v.clone(),
            _ => panic!("expected Float32x3 positions"),
        }
    }

    #[test]
    fn vertex_and_index_counts_match_segments() {
        let segments = 32;
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, 0.35, 0.35, segments);
        assert_eq!(positions(&mesh).len(), 2 * (segments + 1));
        let index_count = match mesh.indices().unwrap() {
            Indices::U32(v) => v.len(),
            Indices::U16(v) => v.len(),
        };
        assert_eq!(index_count, 6 * segments);
    }

    #[test]
    fn vertices_span_the_full_rise() {
        let height = 1.4;
        let mesh = build_dispel_ribbon_mesh(2.5, height, 0.35, 0.35, 48);
        let ys: Vec<f32> = positions(&mesh).iter().map(|p| p[1]).collect();
        let min_y = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_y = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(min_y.abs() < 1e-4, "min Y should be ~0, got {min_y}");
        assert!((max_y - height).abs() < 1e-4, "max Y should be ~height, got {max_y}");
    }

    #[test]
    fn centerline_covers_requested_turns() {
        // First and last segment centerline angles should span turns * TAU.
        let turns = 2.5;
        let segments = 48;
        let mesh = build_dispel_ribbon_mesh(turns, 1.4, 0.35, 0.35, segments);
        let pos = positions(&mesh);
        // Centerline at each segment = midpoint of its two edge vertices, in XZ.
        let first = pos[0];
        let last = pos[pos.len() - 1];
        let first_angle = first[2].atan2(first[0]);
        // Sanity: the first centerline angle is near 0 (cos≈1).
        assert!(first_angle.abs() < 0.3, "first angle near 0, got {first_angle}");
        // The helix advances monotonically: the y of the last vertex >> first.
        assert!(last[1] > first[1]);
        // Number of full turns is encoded in height/turns geometry; assert the
        // angular range by walking centerline angle deltas.
        let mut total = 0.0_f32;
        let mut prev = first_angle;
        for i in 1..=segments {
            let v = pos[2 * i];
            let a = v[2].atan2(v[0]);
            let mut d = a - prev;
            while d > std::f32::consts::PI {
                d -= TAU;
            }
            while d < -std::f32::consts::PI {
                d += TAU;
            }
            total += d;
            prev = a;
        }
        assert!(
            (total.abs() - turns * TAU).abs() < 0.2,
            "unwrapped angular span {:.3} should be ~{:.3}",
            total.abs(),
            turns * TAU
        );
    }

    #[test]
    fn radius_produces_lateral_coil() {
        let radius = 0.35;
        let width = 0.35;
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, width, radius, 48);
        let max_horiz = positions(&mesh)
            .iter()
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(0.0_f32, f32::max);
        // A real lateral coil reaches out to ~radius + width/2, not ~0.
        assert!(
            (max_horiz - (radius + width * 0.5)).abs() < 0.05,
            "max horizontal extent {max_horiz} should be ~{}",
            radius + width * 0.5
        );
    }

    #[test]
    fn attributes_present_equal_length_and_indices_valid() {
        let mesh = build_dispel_ribbon_mesh(2.5, 1.4, 0.35, 0.35, 24);
        let vcount = positions(&mesh).len();
        for attr in [Mesh::ATTRIBUTE_NORMAL, Mesh::ATTRIBUTE_UV_0] {
            let len = match mesh.attribute(attr).unwrap() {
                bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v.len(),
                bevy::render::mesh::VertexAttributeValues::Float32x2(v) => v.len(),
                _ => panic!("unexpected attribute kind"),
            };
            assert_eq!(len, vcount, "attribute length should equal vertex count");
        }
        match mesh.indices().unwrap() {
            Indices::U32(v) => assert!(v.iter().all(|&i| (i as usize) < vcount)),
            Indices::U16(v) => assert!(v.iter().all(|&i| (i as usize) < vcount)),
        }
    }

    /// The folded strip keeps the same vertex layout as the plain one, so the
    /// index buffer built once stays valid for every frame's rewrite.
    #[test]
    fn the_fold_keeps_the_vertex_layout() {
        let plain = ribbon_positions(2.5, 1.4, 0.26, 0.92, 96, -1.0, 0.0);
        let folded = ribbon_positions(2.5, 1.4, 0.26, 0.92, 96, 0.2, 0.0);
        assert_eq!(plain.len(), folded.len());
        // Heights move by at most the fold's amplitude; the ends stay put.
        for (a, b) in plain.iter().zip(folded.iter()) {
            assert!((a[1] - b[1]).abs() <= FOLD_AMP * 1.6 + 1e-5);
        }
        assert!((plain[0][1] - folded[0][1]).abs() < 1e-3);
        assert!((plain[plain.len() - 1][1] - folded[folded.len() - 1][1]).abs() < 1e-3);
        assert!(plain.iter().zip(folded.iter()).any(|(a, b)| (a[1] - b[1]).abs() > 0.02));
    }
}
