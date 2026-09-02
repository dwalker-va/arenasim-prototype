use bevy::prelude::*;
use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::render::mesh::{ConeAnchor, Indices, PrimitiveTopology};
use std::f32::consts::TAU;

use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ==============================================================================
// Frostbolt and Shadow Bolt — the two bespoke caster missiles
// ==============================================================================
//
// From the Classic Era client data (build 1.15.9.69547), walked
// `SpellXSpellVisual -> SpellVisual -> SpellVisualMissile ->
// SpellVisualEffectName` and then parsed out of the M2/SKIN/TXID chunks. Every
// rank of each spell shares one visual, so there is exactly one shape to match.
//
// `spells/frostbolt.m2` (fdid 166214, SpellVisual 13):
//   - NOT an orb. A pointed dart: the vertex cloud runs -2.017..+0.828 along
//     its length against a ~1.6 cross-section, tapering to a POINT at the front
//     (max radius 0.005 in the leading bucket), swelling to 0.42 at the body,
//     flaring to 0.90, then tapering away down a long tail.
//   - NOT blue. `M2Color[0]` is rgb(0.835, 0.961, 1.000) — a pale near-white.
//     The saturation everyone remembers comes from the ribbons, not the body.
//   - Body is 172 vert / 72 tri on an ALPHA material plus a 10-tri additive
//     core; textures are `spells/ice3b_b.blp` over an 8-frame
//     `spells/clouds8x8.blp`, shedding `spells/snowflake2.blp` from a sphere
//     emitter at the head.
//   - TWO ribbon emitters (`particles/ribbonblur1b.blp`), on separate bones.
//   - One global loop, 667ms.
//
// `spells/deathcoil_missile.m2` (fdid 165891, SpellVisual 64):
//   - Shadow Bolt has no model of its own. Every rank points at DEATH COIL's
//     missile — the same kind of shared visual Cheap Shot has with Sap.
//   - COMPACT and dense: bounding radius 0.658 against Frostbolt's 2.054, and
//     flattened along travel (0.40 deep vs 0.93 across).
//   - The core is an 82 vert / 94 tri **Opaque** body (the model's only
//     non-glow texture is `spells/skull.blp`) wrapped in two small additive
//     quads of `spells/purple_glow{,2}.blp`. Tint rgb(0.384, 0.247, 0.737).
//   - TWO ribbon emitters (`particles/gradient64ba.blp`).
//   - THREE global loops — 200ms, 267ms, 433ms — layers churning against each
//     other, which is what makes it read as boiling rather than gliding.
//
// **The finding that drove the whole design: in the source these two differ by
// SILHOUETTE and DENSITY, not by hue.** Frostbolt is a long, bright, translucent
// streak; Shadow Bolt is a small dark mass with a glow around it. Before this,
// both were `Sphere::new(0.3)` and differed only in colour — which is why they
// read as the same spell twice.
//
// Two deliberate divergences from the source, both about what survives at
// 35 yd/s across an arena:
//
//   1. **No skull.** It is 94 triangles of detail on a 0.5yd object crossing the
//      screen in half a second, and it is not expressible in Bevy primitives.
//      The Opaque *dark core* carries the same meaning — a dense mote, not a
//      lamp — which is the half of it that actually reads.
//   2. **Frostbolt is scaled to `FROSTBOLT_SCALE`.** A literal 2.85-unit dart is
//      longer than a combatant capsule is tall (2.5yd).
//
// Graphical-only: spawned off `Added<Projectile>`, no `game_rng` draw and no sim
// write, so headless stays byte-identical. The per-mote scatter uses the same
// deterministic hash the nova crystals use (`nova_jitter`'s shape), never the
// game RNG.

// ── Frostbolt ─────────────────────────────────────────────────────────────

/// `M2Color[0]` of `spells/frostbolt.m2`. Pale, not blue — see the module note.
const FROSTBOLT_SHARD_COLOR: Color = Color::srgb(0.835, 0.961, 1.000);
const FROSTBOLT_SHARD_EMISSIVE: LinearRgba = LinearRgba::rgb(1.30, 1.85, 2.40);
/// The ribbons carry the saturation the body does not.
const FROSTBOLT_RIBBON_COLOR: Color = Color::srgb(0.420, 0.760, 1.000);
const FROSTBOLT_RIBBON_EMISSIVE: LinearRgba = LinearRgba::rgb(0.90, 2.00, 3.40);

/// Fraction of the client model's size. See divergence 2 in the module note.
const FROSTBOLT_SCALE: f32 = 0.55;
/// 5, matching `ROOT_SPIKE_SIDES` and `NOVA_CRYSTAL_SIDES` — this project's
/// established ice-crystal facet count. Faceting is not decoration: a smooth
/// spindle is rotationally symmetric, so its roll would be invisible.
const FROSTBOLT_SHARD_SIDES: u32 = 5;
/// Nose length, client 0.98 (widest point to the tip).
const FROSTBOLT_TIP_LEN: f32 = 0.98;
/// Tail length, client 1.87.
const FROSTBOLT_TAIL_LEN: f32 = 1.87;
/// Radius at the widest point, client 0.42.
const FROSTBOLT_BODY_RADIUS: f32 = 0.42;
/// The model's single global loop, 667ms.
const FROSTBOLT_SPIN_PERIOD: f32 = 0.667;
/// Head flare, client 0.90.
const FROSTBOLT_FLARE_RADIUS: f32 = 0.90;
/// A tight bright point just ahead of the shard's shoulder — the model's
/// 10-triangle additive core.
const FROSTBOLT_TIP_GLOW_RADIUS: f32 = 0.29;

/// Lateral offset of each ribbon anchor from the axis. The source's two
/// emitters sit on separate bones; carried on a rolling body they trace a
/// HELIX, which is the only place the shard's roll becomes visible — but only
/// if the two strands are far enough apart to be seen as two. At the first
/// shipped value they sat well inside one band's own width and the braid read
/// as a single line.
const FROSTBOLT_RIBBON_SEP: f32 = 0.51;
/// Half-width of the ribbon band.
const FROSTBOLT_RIBBON_HALF_WIDTH: f32 = 0.18;
const FROSTBOLT_RIBBON_LIFE: f32 = 0.18;
/// Distance between dropped segments, in YARDS. Segments are stretched to
/// `step * BOLT_TRAIL_OVERLAP`, so this is a density knob now rather than a
/// continuity risk — bigger means fewer entities, not a gappier trail.
const FROSTBOLT_RIBBON_STEP: f32 = 0.45;

/// `spells/snowflake2.blp`, shed from the sphere emitter at the head.
const FROSTBOLT_FLAKE_RATE: f32 = 26.0;
const FROSTBOLT_FLAKE_LIFE: f32 = 0.50;
/// Lateral scatter speed, yd/s.
const FROSTBOLT_FLAKE_SPREAD: f32 = 1.60;
const FROSTBOLT_FLAKE_RADIUS: f32 = 0.08;

// ── Shadow Bolt ───────────────────────────────────────────────────────────

/// `M2Color[0]` of `spells/deathcoil_missile.m2`.
const SHADOWBOLT_GLOW_COLOR: Color = Color::srgb(0.384, 0.247, 0.737);
const SHADOWBOLT_GLOW_EMISSIVE: LinearRgba = LinearRgba::rgb(1.55, 0.85, 2.60);
/// The Opaque body, read dark. This is the whole contrast with Frostbolt: a
/// mass that occludes its own glow, not another light source.
const SHADOWBOLT_CORE_COLOR: Color = Color::srgb(0.090, 0.050, 0.150);
/// Deliberately low. Past roughly 0.6 the core stops reading as mass.
const SHADOWBOLT_CORE_EMISSIVE: LinearRgba = LinearRgba::rgb(0.18, 0.07, 0.30);

const SHADOWBOLT_SCALE: f32 = 0.85;
/// Radius of the dense core. The client's solid body measures roughly 0.6
/// across its round axes.
///
/// It is a SPHERE, deliberately, and getting there took a wrong turn worth
/// recording. The client model's three vertex spans are 0.398 / 0.927 / 0.927,
/// so it is a lens — flattened on ONE axis. The first build read that as
/// "flattened along travel" and squashed the local Z, which renders a coin
/// facing the direction of flight: edge-on from the side, it is a flat tab
/// rather than a mote. The flattened axis is in fact LATERAL — Frostbolt's own
/// 2.844 span fixes travel as the third component, and Shadow Bolt's long pair
/// sit on the second and third. A lateral flattening is not a stable silhouette
/// on a missile that rolls, and it reads as a disc from the arena's elevated
/// camera, so it buys nothing here. A sphere reads as dense mass from every
/// bearing, which is the whole job of this piece.
const SHADOWBOLT_CORE_RADIUS: f32 = 0.33;
/// Client glow billboard, 0.49.
const SHADOWBOLT_HALO_RADIUS: f32 = 0.49;
const SHADOWBOLT_CHURN_RADIUS: f32 = 0.66;
/// The three global loops, 200 / 267 / 433 ms. Two churn quads counter-orbit on
/// A and B while the halo breathes on the pulse period; because the three are
/// mutually incommensurate the composite never repeats on a visible beat.
const SHADOWBOLT_CHURN_PERIOD_A: f32 = 0.200;
const SHADOWBOLT_PULSE_PERIOD: f32 = 0.267;
const SHADOWBOLT_CHURN_PERIOD_B: f32 = 0.433;

const SHADOWBOLT_RIBBON_SEP: f32 = 0.21;
const SHADOWBOLT_RIBBON_HALF_WIDTH: f32 = 0.11;
const SHADOWBOLT_RIBBON_LIFE: f32 = 0.15;
const SHADOWBOLT_RIBBON_STEP: f32 = 0.42;

const SHADOWBOLT_MOTE_RATE: f32 = 10.0;
const SHADOWBOLT_MOTE_LIFE: f32 = 0.45;
const SHADOWBOLT_MOTE_SPREAD: f32 = 0.55;
const SHADOWBOLT_MOTE_RADIUS: f32 = 0.11;

// ── shared ────────────────────────────────────────────────────────────────

/// Pixels on a side for the generated soft-dot sprite.
const BOLT_SPRITE_PX: u32 = 64;
/// Falloff exponent of the soft dot. Higher is a tighter point of light.
const BOLT_SPRITE_FALLOFF: f32 = 2.2;
/// Downward drift on shed sprites, yd/s². They are vapour, not gravel, so this
/// is far below real gravity — just enough that the trail settles.
const BOLT_SHED_SINK: f32 = 1.1;
/// How a dying trail segment shrinks. Segments share one material per kind
/// (there are dozens live per bolt and a per-segment material would be an asset
/// each), so the fade is carried by scale — under additive blending a shrinking
/// sprite contributes proportionally less light, which reads as the same thing.
/// It narrows the band only; see `BoltTrail::length`.
const BOLT_TRAIL_SHRINK_POW: f32 = 0.85;
/// How far each segment overruns its own spacing. Above 1.0 consecutive
/// segments overlap, which is what makes the ribbon continuous instead of a
/// row of separate marks.
const BOLT_TRAIL_OVERLAP: f32 = 1.35;

/// Which bespoke missile an ability carries, if any.
///
/// The single list both the spawner and the probes derive from — an ability
/// absent here falls through to the generic sphere in `spawn_projectile_visuals`,
/// which is what every other projectile still gets.
pub fn bolt_kind_for(ability: AbilityType) -> Option<BoltKind> {
    match ability {
        AbilityType::Frostbolt => Some(BoltKind::Frost),
        AbilityType::Shadowbolt => Some(BoltKind::Shadow),
        _ => None,
    }
}

impl BoltKind {
    fn scale(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_SCALE,
            BoltKind::Shadow => SHADOWBOLT_SCALE,
        }
    }
    /// Period of the roll that carries the ribbon anchors around the axis.
    fn spin_period(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_SPIN_PERIOD,
            BoltKind::Shadow => SHADOWBOLT_CHURN_PERIOD_B,
        }
    }
    fn ribbon_sep(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_RIBBON_SEP,
            BoltKind::Shadow => SHADOWBOLT_RIBBON_SEP,
        }
    }
    fn ribbon_half_width(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_RIBBON_HALF_WIDTH,
            BoltKind::Shadow => SHADOWBOLT_RIBBON_HALF_WIDTH,
        }
    }
    fn ribbon_life(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_RIBBON_LIFE,
            BoltKind::Shadow => SHADOWBOLT_RIBBON_LIFE,
        }
    }
    fn ribbon_step(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_RIBBON_STEP,
            BoltKind::Shadow => SHADOWBOLT_RIBBON_STEP,
        }
    }
    fn shed_rate(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_FLAKE_RATE,
            BoltKind::Shadow => SHADOWBOLT_MOTE_RATE,
        }
    }
    fn shed_life(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_FLAKE_LIFE,
            BoltKind::Shadow => SHADOWBOLT_MOTE_LIFE,
        }
    }
    fn shed_radius(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_FLAKE_RADIUS,
            BoltKind::Shadow => SHADOWBOLT_MOTE_RADIUS,
        }
    }
    fn shed_spread(self) -> f32 {
        match self {
            BoltKind::Frost => FROSTBOLT_FLAKE_SPREAD,
            BoltKind::Shadow => SHADOWBOLT_MOTE_SPREAD,
        }
    }
}

/// How a stretched trail segment must sit: its length along the direction it
/// was laid down, its face turned as square to the camera as that allows.
///
/// A trail segment cannot take the camera's rotation wholesale the way a round
/// sprite can — that would swing its long axis off the flight path and the
/// ribbon would splay. Only the roll ABOUT the travel axis is free, and this
/// spends it on facing the viewer.
///
/// Pure, so the constraint can be asserted without a render world.
pub fn trail_segment_rotation(dir: Vec3, to_camera: Vec3) -> Quat {
    let x = dir.normalize_or_zero();
    if x == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    // Degenerate when looking straight down the barrel: any roll is as good as
    // any other, so pick a stable perpendicular instead of dividing by zero.
    let y = to_camera.cross(x);
    let y = if y.length_squared() < 1e-6 {
        x.any_orthonormal_vector()
    } else {
        y.normalize()
    };
    Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y)))
}

/// Deterministic 0..1 scatter. Visual-only — the same hash shape as
/// `nova_jitter`, and for the same reason: touching `game_rng` from a
/// graphical-only system would desync headless from graphical.
fn bolt_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// A soft round dot, white, with the whole shape in the alpha channel.
///
/// Generated rather than shipped as an asset for the same reason
/// `sparkle_texture` and `create_surface_texture` are. This is the ONLY way to
/// get a soft edge: a glow built from bigger, dimmer geometry renders as a hard
/// silhouette however faint it is.
fn soft_dot_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = BOLT_SPRITE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - r).clamp(0.0, 1.0).powf(BOLT_SPRITE_FALLOFF);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0) as u8;
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

/// A soft-edged BAND: opaque along its length, falling off across its width.
///
/// The trail needs this and not [`soft_dot_texture`]. A round sprite's alpha
/// falls off in every direction, so a row of them keeps visibly separate bright
/// cores however tightly they are packed — which is exactly how the first build
/// shipped, as a dotted line. Holding alpha flat along the length lets adjacent
/// stretched segments merge into one continuous ribbon.
fn soft_band_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = BOLT_SPRITE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;

    for y in 0..size {
        // -1..1 ACROSS the band; nothing varies along it.
        let dy = (y as f32 - centre) / centre;
        let a = (1.0 - dy.abs()).clamp(0.0, 1.0).powf(BOLT_SPRITE_FALLOFF);
        let a = (a * 255.0) as u8;
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = a;
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

/// Meshes and materials shared by every bolt of every caster.
///
/// Built once and reused: a bolt spawns dozens of trail segments and a busy
/// match has several in flight, so per-instance assets would churn the asset
/// server for no visual gain. Nothing here is mutated at runtime — the pulses,
/// churn and fades are all carried by Transform.
/// `pub` only because it appears in a `Local<..>` on two `pub` systems; every
/// field stays private and nothing outside this module constructs one.
pub struct BoltAssets {
    quad: Handle<Mesh>,
    /// The soft dot itself, so an impact can build its own fading material
    /// against the same sprite.
    dot: Handle<Image>,
    /// Frostbolt's shockwave ring and its ice chips, plus Shadow Bolt's arc.
    ring: Handle<Mesh>,
    chip: Handle<Mesh>,
    crescent: Handle<Mesh>,
    frost_chip: Handle<StandardMaterial>,
    blot: Handle<Mesh>,
    frost_tip: Handle<Mesh>,
    frost_tail: Handle<Mesh>,
    frost_shard: Handle<StandardMaterial>,
    frost_glow: Handle<StandardMaterial>,
    frost_ribbon: Handle<StandardMaterial>,
    shadow_ribbon: Handle<StandardMaterial>,
    shadow_core_mesh: Handle<Mesh>,
    shadow_core: Handle<StandardMaterial>,
    shadow_glow: Handle<StandardMaterial>,
}

impl BoltAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        let dot = images.add(soft_dot_texture());
        let band = images.add(soft_band_texture());

        // The sprite drives BOTH channels: base_color_texture supplies the
        // alpha that shapes the dot, emissive_texture keeps it from emitting
        // where the dot is transparent. `unlit` stays false throughout or the
        // emissive is discarded outright by Bevy's PBR shader.
        fn glow(
            materials: &mut Assets<StandardMaterial>,
            dot: &Handle<Image>,
            color: Color,
            emissive: LinearRgba,
        ) -> Handle<StandardMaterial> {
            materials.add(StandardMaterial {
                base_color: color,
                base_color_texture: Some(dot.clone()),
                emissive,
                emissive_texture: Some(dot.clone()),
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        }

        let frost_glow = glow(
            materials,
            &dot,
            FROSTBOLT_SHARD_COLOR,
            FROSTBOLT_SHARD_EMISSIVE,
        );
        let frost_ribbon = glow(
            materials,
            &band,
            FROSTBOLT_RIBBON_COLOR,
            FROSTBOLT_RIBBON_EMISSIVE,
        );
        let shadow_ribbon = glow(
            materials,
            &band,
            SHADOWBOLT_GLOW_COLOR,
            SHADOWBOLT_GLOW_EMISSIVE,
        );
        let shadow_glow = glow(
            materials,
            &dot,
            SHADOWBOLT_GLOW_COLOR,
            SHADOWBOLT_GLOW_EMISSIVE,
        );

        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            dot: dot.clone(),
            // A unit-radius ring; the impact scales it out to its full reach.
            // Built as a soft-rimmed band rather than an `Annulus`, whose
            // uniform alpha renders a hard-edged hoop — a drawn circle, not a
            // shockwave.
            ring: meshes.add(build_arc_band(
                FROST_IMPACT_RING_SEGMENTS,
                std::f32::consts::TAU,
                FROST_IMPACT_RING_THICKNESS,
                false,
            )),
            // The chips are the same 5-sided crystal the shard, the root ice
            // and the nova all use — an ice chip is not a round dot.
            chip: meshes.add(
                Cone::new(0.45, 1.0)
                    .mesh()
                    .resolution(FROSTBOLT_SHARD_SIDES)
                    .anchor(ConeAnchor::Base),
            ),
            blot: meshes.add(Sphere::new(1.0)),
            crescent: meshes.add(build_arc_band(
                SHADOW_IMPACT_ARC_SEGMENTS,
                SHADOW_IMPACT_ARC_SWEEP,
                SHADOW_IMPACT_ARC_WIDTH,
                true,
            )),
            frost_chip: materials.add(StandardMaterial {
                base_color: FROST_IMPACT_COLOR,
                emissive: FROST_IMPACT_EMISSIVE,
                perceptual_roughness: 0.22,
                ..default()
            }),
            // Anchored at the BASE so each cone's base sits on the shard's
            // widest point and it grows away from there — the two together are
            // one spindle sharing a rim.
            frost_tip: meshes.add(
                Cone::new(FROSTBOLT_BODY_RADIUS, FROSTBOLT_TIP_LEN)
                    .mesh()
                    .resolution(FROSTBOLT_SHARD_SIDES)
                    .anchor(ConeAnchor::Base),
            ),
            frost_tail: meshes.add(
                Cone::new(FROSTBOLT_BODY_RADIUS, FROSTBOLT_TAIL_LEN)
                    .mesh()
                    .resolution(FROSTBOLT_SHARD_SIDES)
                    .anchor(ConeAnchor::Base),
            ),
            // The source body is an Alpha material, not additive: the shard is
            // a translucent solid that still catches the arena sun on its
            // facets, which is what makes the roll legible.
            frost_shard: materials.add(StandardMaterial {
                base_color: FROSTBOLT_SHARD_COLOR.with_alpha(0.88),
                emissive: FROSTBOLT_SHARD_EMISSIVE,
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.22,
                cull_mode: None,
                double_sided: true,
                ..default()
            }),
            frost_glow,
            frost_ribbon,
            shadow_ribbon,
            shadow_core_mesh: meshes.add(Sphere::new(SHADOWBOLT_CORE_RADIUS)),
            // Opaque, and emissive kept low on purpose. This is the one part of
            // either bolt that is NOT a light.
            shadow_core: materials.add(StandardMaterial {
                base_color: SHADOWBOLT_CORE_COLOR,
                emissive: SHADOWBOLT_CORE_EMISSIVE,
                perceptual_roughness: 0.55,
                ..default()
            }),
            shadow_glow,
        }
    }

    fn ribbon_material(&self, kind: BoltKind) -> Handle<StandardMaterial> {
        match kind {
            BoltKind::Frost => self.frost_ribbon.clone(),
            BoltKind::Shadow => self.shadow_ribbon.clone(),
        }
    }

    fn shed_material(&self, kind: BoltKind) -> Handle<StandardMaterial> {
        match kind {
            BoltKind::Frost => self.frost_glow.clone(),
            BoltKind::Shadow => self.shadow_glow.clone(),
        }
    }
}

/// Build the bespoke rig on a newly spawned Frostbolt or Shadow Bolt.
///
/// The projectile entity itself is aimed by `move_projectiles`, which sets
/// `Quat::from_rotation_arc(Vec3::Z, direction)` — so **local +Z is travel** and
/// every child below is posed in that frame.
pub fn spawn_bolt_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<BoltAssets>>,
    new_projectiles: Query<(Entity, &Projectile, &Transform), Added<Projectile>>,
) {
    if new_projectiles.is_empty() {
        return;
    }
    let assets = assets.get_or_insert_with(|| BoltAssets::build(&mut meshes, &mut materials, &mut images));

    for (entity, projectile, transform) in new_projectiles.iter() {
        let Some(kind) = bolt_kind_for(projectile.ability) else {
            continue;
        };
        let s = kind.scale();

        commands.entity(entity).insert(BoltRig {
            kind,
            age: 0.0,
            ribbon_carry: 0.0,
            shed_carry: 0.0,
            shed_count: 0,
            last_pos: transform.translation,
            seed: entity.index(),
        });

        match kind {
            BoltKind::Frost => {
                // The shard is a hub that ROLLS about the travel axis; the two
                // cones hang off it. Bevy's `Cone` runs along +Y, so each needs
                // the Y->Z (or Y->-Z) rotation baked into its own transform —
                // getting this wrong points the dart across its own flight path.
                let shard = commands
                    .spawn((
                        BoltShard,
                        Transform::from_scale(Vec3::splat(s)),
                        Visibility::default(),
                    ))
                    .id();
                let tip = commands
                    .spawn((
                        Mesh3d(assets.frost_tip.clone()),
                        MeshMaterial3d(assets.frost_shard.clone()),
                        Transform::from_rotation(Quat::from_rotation_arc(Vec3::Y, Vec3::Z)),
                        NotShadowCaster,
                    ))
                    .id();
                let tail = commands
                    .spawn((
                        Mesh3d(assets.frost_tail.clone()),
                        MeshMaterial3d(assets.frost_shard.clone()),
                        Transform::from_rotation(Quat::from_rotation_arc(Vec3::Y, -Vec3::Z)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(shard).add_children(&[tip, tail]);
                commands.entity(entity).add_child(shard);

                spawn_sprite(
                    &mut commands,
                    entity,
                    assets,
                    assets.frost_glow.clone(),
                    BoltSpriteRole::Flare,
                    FROSTBOLT_FLARE_RADIUS * s,
                    Vec3::ZERO,
                );
                spawn_sprite(
                    &mut commands,
                    entity,
                    assets,
                    assets.frost_glow.clone(),
                    BoltSpriteRole::TipGlow,
                    FROSTBOLT_TIP_GLOW_RADIUS * s,
                    Vec3::Z * (FROSTBOLT_TIP_LEN * 0.55 * s),
                );
            }
            BoltKind::Shadow => {
                let core = commands
                    .spawn((
                        BoltCore,
                        Mesh3d(assets.shadow_core_mesh.clone()),
                        MeshMaterial3d(assets.shadow_core.clone()),
                        Transform::from_scale(Vec3::splat(s)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(entity).add_child(core);

                for (role, radius) in [
                    (BoltSpriteRole::Halo, SHADOWBOLT_HALO_RADIUS),
                    (BoltSpriteRole::ChurnA, SHADOWBOLT_CHURN_RADIUS * 0.80),
                    (BoltSpriteRole::ChurnB, SHADOWBOLT_CHURN_RADIUS * 0.66),
                ] {
                    spawn_sprite(
                        &mut commands,
                        entity,
                        assets,
                        assets.shadow_glow.clone(),
                        role,
                        radius * s,
                        Vec3::ZERO,
                    );
                }
            }
        }
    }
}

fn spawn_sprite(
    commands: &mut Commands,
    parent: Entity,
    assets: &BoltAssets,
    material: Handle<StandardMaterial>,
    role: BoltSpriteRole,
    radius: f32,
    at: Vec3,
) {
    let sprite = commands
        .spawn((
            BoltSprite { role, radius },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(at).with_scale(Vec3::splat(radius * 2.0)),
            NotShadowCaster,
        ))
        .id();
    commands.entity(parent).add_child(sprite);
}

/// Roll the shard, breathe the halo, orbit the churn quads, and lay down both
/// ribbons and the shed sprites.
///
/// Ribbon emission is driven by DISTANCE, not by elapsed time, for the same
/// reason the gaits are: a time-driven emitter lays its segments further apart
/// the faster the thing moves, so the trail becomes a dotted line exactly when
/// the bolt is most visible. At a fixed yard spacing the trail stays continuous
/// whatever the projectile's speed, and because segments are interpolated along
/// the frame's own step the spacing does not depend on the frame rate either.
pub fn animate_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<BoltAssets>>,
    mut bolts: Query<(&mut BoltRig, &Transform, &Children)>,
    mut shards: Query<&mut Transform, (With<BoltShard>, Without<BoltRig>)>,
    mut sprites: Query<(&BoltSprite, &mut Transform), (Without<BoltRig>, Without<BoltShard>)>,
) {
    if bolts.is_empty() {
        return;
    }
    let assets =
        assets.get_or_insert_with(|| BoltAssets::build(&mut meshes, &mut materials, &mut images));
    let dt = time.delta_secs();

    for (mut rig, transform, children) in bolts.iter_mut() {
        rig.age += dt;
        let kind = rig.kind;
        let scale = kind.scale();
        let age = rig.age;
        let pos = transform.translation;
        let spin = age / kind.spin_period() * TAU;

        // The travel frame. `move_projectiles` aims local +Z down the flight
        // path, so X and Y here span the plane the ribbons and scatter live in.
        let right = transform.rotation * Vec3::X;
        let up = transform.rotation * Vec3::Y;

        for child in children.iter() {
            if let Ok(mut shard) = shards.get_mut(child) {
                // Roll about the travel axis. The shard's parent already points
                // its +Z down the flight path, so a plain Z rotation IS a roll.
                shard.rotation = Quat::from_rotation_z(spin);
            }
            if let Ok((sprite, mut sprite_transform)) = sprites.get_mut(child) {
                let pulse = match sprite.role {
                    BoltSpriteRole::Flare => {
                        0.85 + 0.15 * (age / FROSTBOLT_SPIN_PERIOD * TAU).sin()
                    }
                    BoltSpriteRole::Halo => {
                        0.86 + 0.14 * (age / SHADOWBOLT_PULSE_PERIOD * TAU).sin()
                    }
                    _ => 1.0,
                };
                // The two churn quads counter-orbit on the source's own 200ms
                // and 433ms loops, on perpendicular axes. Neither travels far —
                // the point is that the glow boils instead of sliding along as
                // one rigid blob. Every other role keeps its spawn offset.
                match sprite.role {
                    BoltSpriteRole::ChurnA => {
                        sprite_transform.translation = Vec3::X
                            * ((age / SHADOWBOLT_CHURN_PERIOD_A * TAU).cos()
                                * SHADOWBOLT_CORE_RADIUS
                                * 0.5
                                * scale);
                    }
                    BoltSpriteRole::ChurnB => {
                        sprite_transform.translation = Vec3::Y
                            * ((-age / SHADOWBOLT_CHURN_PERIOD_B * TAU).sin()
                                * SHADOWBOLT_CORE_RADIUS
                                * 0.5
                                * scale);
                    }
                    _ => {}
                }
                sprite_transform.scale = Vec3::splat(sprite.radius * 2.0 * pulse);
            }
        }

        // ── the two ribbons ────────────────────────────────────────────────
        let travelled = pos.distance(rig.last_pos);
        let step = kind.ribbon_step();
        if travelled > 0.0 {
            rig.ribbon_carry += travelled;
            let sep = kind.ribbon_sep() * scale;
            let half_width = kind.ribbon_half_width() * scale;
            let length = step * BOLT_TRAIL_OVERLAP;
            let dir = transform.rotation * Vec3::Z;
            let life = kind.ribbon_life();
            let material = assets.ribbon_material(kind);
            let from = rig.last_pos;
            // The two anchors sit on opposite sides of the axis and are carried
            // around by the roll, so the pair traces a HELIX behind the bolt —
            // the only place the shard's rotation is actually visible.
            let offset = right * (spin.cos() * sep) + up * (spin.sin() * sep);
            while rig.ribbon_carry >= step {
                rig.ribbon_carry -= step;
                let back = rig.ribbon_carry.min(travelled);
                let at = from.lerp(pos, ((travelled - back) / travelled).clamp(0.0, 1.0));
                // A `Rectangle` is CENTRED on its origin, so a band placed at
                // the emission point would reach half its length AHEAD of it and
                // poke out through the bolt's nose. Walk the centre back so the
                // band's leading edge sits where it was laid instead.
                let at = at - dir * (length * 0.5);
                for side in [offset, -offset] {
                    commands.spawn((
                        BoltTrail {
                            age: 0.0,
                            life,
                            half_width,
                            length,
                            dir,
                        },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(material.clone()),
                        // X spans the band's length, Y its width — a
                        // `Rectangle` meshes into XY facing +Z, and
                        // `trail_segment_rotation` puts local X on `dir`.
                        Transform::from_translation(at + side)
                            .with_scale(Vec3::new(length, half_width * 2.0, 1.0)),
                        // Glowing spell effects do not cast shadows. Without
                        // this the trail painted a dotted black line across the
                        // arena floor beside itself.
                        NotShadowCaster,
                        PlayMatchEntity,
                    ));
                }
            }
        }

        // ── shed sprites ───────────────────────────────────────────────────
        rig.shed_carry += dt * kind.shed_rate();
        let shed_life = kind.shed_life();
        let shed_radius = kind.shed_radius() * scale;
        let spread = kind.shed_spread();
        let shed_material = assets.shed_material(kind);
        while rig.shed_carry >= 1.0 {
            rig.shed_carry -= 1.0;
            let seed = rig
                .seed
                .wrapping_mul(2_654_435_761)
                .wrapping_add(rig.shed_count);
            rig.shed_count = rig.shed_count.wrapping_add(1);
            let theta = bolt_jitter(seed) * TAU;
            let speed = bolt_jitter(seed.wrapping_add(7919)) * spread;
            let velocity = right * (theta.cos() * speed) + up * (theta.sin() * speed);
            commands.spawn((
                BoltMote {
                    age: 0.0,
                    life: shed_life,
                    radius: shed_radius,
                    velocity,
                },
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(shed_material.clone()),
                Transform::from_translation(pos).with_scale(Vec3::splat(shed_radius * 2.0)),
                NotShadowCaster,
                PlayMatchEntity,
            ));
        }

        rig.last_pos = pos;
    }
}

/// Turns every bolt sprite to face the camera.
///
/// A quad is a point of light only while it is face-on; edge-on it vanishes.
/// The rig's children hang off a projectile whose rotation is rewritten every
/// frame by `move_projectiles`, so copying the camera's rotation straight in is
/// not enough — the parent's aim composes on top of it. Pre-multiplying by the
/// parent's inverse cancels that, exactly as `billboard_cc_beads` cancels its
/// hub's spin.
///
/// Trail segments and motes are ROOT entities left behind in world space, so
/// they take the camera's rotation directly with nothing to cancel.
pub fn billboard_bolt_sprites(
    camera: Query<&Transform, (With<Camera3d>, Without<BoltSprite>, Without<BoltRig>)>,
    rigs: Query<(&Transform, &Children), With<BoltRig>>,
    mut sprites: Query<&mut Transform, (With<BoltSprite>, Without<BoltRig>)>,
    // `Without<Camera3d>` is load-bearing, not defensive: the camera's own
    // `&Transform` above is read-only, and Bevy rejects the pair as a B0001
    // access conflict unless the mutable query provably cannot match it. The
    // other two exclusions do the same job against the read-only rig query.
    mut motes: Query<
        &mut Transform,
        (
            With<BoltMote>,
            Without<Camera3d>,
            Without<BoltSprite>,
            Without<BoltRig>,
        ),
    >,
    mut trails: Query<
        (&BoltTrail, &mut Transform),
        (
            Without<BoltMote>,
            Without<Camera3d>,
            Without<BoltSprite>,
            Without<BoltRig>,
        ),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let facing = rig.rotation.inverse() * cam.rotation;
        for child in children.iter() {
            if let Ok(mut sprite) = sprites.get_mut(child) {
                sprite.rotation = facing;
            }
        }
    }
    for mut mote in motes.iter_mut() {
        mote.rotation = cam.rotation;
    }
    // A band's long axis belongs to the flight path, not the camera; only its
    // roll about that axis is free to face the viewer.
    for (trail, mut transform) in trails.iter_mut() {
        let to_camera = cam.translation - transform.translation;
        transform.rotation = trail_segment_rotation(trail.dir, to_camera);
    }
}

/// Shrinks and retires trail segments.
///
/// The fade is carried by SCALE rather than by alpha because every segment of a
/// kind shares one material — a bolt lays down dozens and per-segment materials
/// would be an asset apiece. Under additive blending a shrinking sprite
/// contributes proportionally less light, so it reads as a fade regardless.
pub fn update_bolt_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut trails: Query<(Entity, &mut BoltTrail, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut trail, mut transform) in trails.iter_mut() {
        trail.age += dt;
        if trail.age >= trail.life {
            commands.entity(entity).despawn();
            continue;
        }
        // Width only. Shrinking the LENGTH would pull each segment away from
        // its neighbours and open the ribbon back up into beads as it faded.
        let remaining = 1.0 - trail.age / trail.life;
        transform.scale = Vec3::new(
            trail.length,
            trail.half_width * 2.0 * remaining.powf(BOLT_TRAIL_SHRINK_POW),
            1.0,
        );
    }
}

/// Drifts, sinks, shrinks and retires the shed flakes and motes.
pub fn update_bolt_motes(
    mut commands: Commands,
    time: Res<Time>,
    mut motes: Query<(Entity, &mut BoltMote, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut mote, mut transform) in motes.iter_mut() {
        mote.age += dt;
        if mote.age >= mote.life {
            commands.entity(entity).despawn();
            continue;
        }
        mote.velocity.y -= BOLT_SHED_SINK * dt;
        let velocity = mote.velocity;
        transform.translation += velocity * dt;
        let remaining = 1.0 - mote.age / mote.life;
        transform.scale = Vec3::splat(mote.radius * 2.0 * remaining.powf(BOLT_TRAIL_SHRINK_POW));
    }
}

/// How far the Frostbolt shard reaches ahead of and behind its widest point, in
/// world yards. Exposed for the probes, which assert the drawn extent rather
/// than trusting the constants — the same reason `nova_outer_radius` is public.
pub fn frostbolt_shard_extent() -> (f32, f32) {
    (
        FROSTBOLT_TIP_LEN * FROSTBOLT_SCALE,
        FROSTBOLT_TAIL_LEN * FROSTBOLT_SCALE,
    )
}

/// A bolt's ribbon geometry in world yards:
/// `(separation, half_width, step, segment_length)`.
pub fn bolt_ribbon_geometry(kind: BoltKind) -> (f32, f32, f32, f32) {
    (
        kind.ribbon_sep() * kind.scale(),
        kind.ribbon_half_width() * kind.scale(),
        kind.ribbon_step(),
        kind.ribbon_step() * BOLT_TRAIL_OVERLAP,
    )
}

/// Shadow Bolt's glow diameter in world yards — the width that has to stay
/// clearly smaller than Frostbolt's length for the two to read apart.
pub fn shadowbolt_glow_width() -> f32 {
    SHADOWBOLT_HALO_RADIUS * 2.0 * SHADOWBOLT_SCALE
}

/// The rotation a bolt sprite needs so that it ends up facing the camera
/// despite hanging off a parent that is aimed down the flight path.
///
/// Pure, so the cancellation can be asserted without a render world:
/// `parent * bolt_billboard_rotation(parent, camera) == camera`.
pub fn bolt_billboard_rotation(parent: Quat, camera: Quat) -> Quat {
    parent.inverse() * camera
}

// ==============================================================================
// Impact — what the bolts do when they land
// ==============================================================================
//
// Both impacts attach to the victim's CHEST (`SpellVisualKitModelAttach`
// attachment 34 on kit 4991 / 219), not to the ground, and they differ in SHAPE
// exactly the way the missiles differ in silhouette:
//
// `spells/ice_impactdd_med_chest.m2` (fdid 166370) — RADIAL. All 33 of its bone
// pivots sit at the origin, so its 28 additive emitters fire from a single
// point; it carries no ribbons and almost no mesh (8 vertices). Reach is a
// bounding radius of 1.636. Its textures name the parts: `shockwave10.blp` (an
// expanding ring), `cyanstarflash.blp` (a star flash), `snowflake2/3.blp`
// (shards) and `dust1_a.blp`. Colour runs deep saturated blue,
// rgb(0.000, 0.200, 0.847), settling to rgb(0.169, 0.110, 0.620); the pulses
// peak at 567ms and 967ms and it is over at 1234ms.
//
// `spells/deathcoil_impact_chest.m2` (fdid 165890) — BILATERAL, and shared with
// Death Coil like the missile is. Its pivots cluster at y -0.653 and y +0.573,
// both pushed forward to z +0.366, with a RIBBON emitter at each: two arcs
// whipping out from opposite sides of the chest rather than a burst. Reach is
// larger than frost's (2.187) but it is far snappier — everything is gone by
// 667ms against frost's 1234ms. Its batches are an Opaque white core plus one
// additive layer.
//
// Three deliberate divergences:
//
//   1. **Shadow Bolt's burst is Shadow-purple, not the source's chartreuse.**
//      The additive batch of `deathcoil_impact_chest.m2` is tinted
//      rgb(0.518, 1.000, 0.000) — Death Coil's green, verified against the skin
//      batches rather than assumed. This project has already ruled on exactly
//      this conflict once: Frost Nova takes `SpellSchool::color_rgb8` over its
//      own measured endpoint because the source colour read as grey. Green on a
//      Shadow spell would read as Nature to a player, which spends the colour
//      budget badly.
//   2. **Both are compressed.** Frostbolt recasts about every 1.5s, so a
//      literal 1234ms burst would still be on screen when the next one landed.
//      The tempo GAP is preserved, because it is half of what tells the two
//      hits apart.
//   3. **No dust.** `dust1_a.blp` is a ground-contact cue; these land on a
//      chest in mid-air.

/// Height of chest attachment 34 on this project's 2.5yd capsule.
const IMPACT_CHEST_Y: f32 = 1.45;

/// `SpellSchool::Frost` — this project's colour authority.
const FROST_IMPACT_COLOR: Color = Color::srgb(0.392, 0.706, 1.000);
const FROST_IMPACT_EMISSIVE: LinearRgba = LinearRgba::rgb(0.90, 2.00, 3.60);
const FROST_IMPACT_FLASH_RADIUS: f32 = 0.85;
const FROST_IMPACT_FLASH_SECS: f32 = 0.16;
/// `shockwave10.blp`. The client's own bounding radius is 1.636; pulled in a
/// little because at full reach the ring dwarfed the unit it was hitting.
const FROST_IMPACT_RING_RADIUS: f32 = 1.35;
const FROST_IMPACT_RING_SECS: f32 = 0.38;
/// Fraction of the ring's radius. Fatter than it looks — the rims are
/// transparent, so the visible stroke is much narrower than the band.
const FROST_IMPACT_RING_THICKNESS: f32 = 0.24;
const FROST_IMPACT_RING_SEGMENTS: u32 = 72;
/// Ice chips, standing in for `snowflake2/3.blp`. Thrown radially, because the
/// source's emitters all share one origin.
const FROST_IMPACT_SHARD_COUNT: u32 = 13;
const FROST_IMPACT_SHARD_SPEED: f32 = 3.4;
const FROST_IMPACT_SHARD_LIFE: f32 = 0.42;
const FROST_IMPACT_SHARD_RADIUS: f32 = 0.10;
const FROST_IMPACT_SHARD_GRAVITY: f32 = 6.5;
const FROST_IMPACT_SHARD_SPIN: f32 = 7.0;

/// `SpellSchool::Shadow` — see divergence 1 in the section note.
const SHADOW_IMPACT_COLOR: Color = Color::srgb(0.580, 0.510, 0.788);
/// Pushed well above the missile's glow: on screen the burst was reading as a
/// faint smudge, and it has only a fifth of a second to land.
const SHADOW_IMPACT_EMISSIVE: LinearRgba = LinearRgba::rgb(3.60, 2.30, 6.00);
/// Lateral offset of each arc from the chest. The client's pivots are at
/// +/-0.6 on a smaller model; this is the value the preview settled on.
const SHADOW_IMPACT_ARC_OFFSET: f32 = 0.34;
/// The client pushes both anchors forward to z +0.366 — the arcs sit in FRONT
/// of the chest, toward the caster.
const SHADOW_IMPACT_ARC_FORWARD: f32 = 0.37;
const SHADOW_IMPACT_ARC_RADIUS: f32 = 0.86;
const SHADOW_IMPACT_ARC_WIDTH: f32 = 0.42;
const SHADOW_IMPACT_ARC_SECS: f32 = 0.30;
/// How far around each crescent sweeps, in radians.
const SHADOW_IMPACT_ARC_SWEEP: f32 = 2.1;
const SHADOW_IMPACT_ARC_SEGMENTS: u32 = 24;
/// A dark violet blot punched onto the victim — the source's **Opaque** batch,
/// taken literally.
///
/// This is what makes the burst read. Everything else in both impacts is
/// additive, and additive can only ever BRIGHTEN: against the arena's pale sand
/// and a lit capsule, a desaturated `SpellSchool::Shadow` glow lifts the pixels
/// barely at all, so the hit came out looking blended and slight next to
/// Frostbolt's near-white ring. A dark, alpha-blended mass moves the pixels the
/// other way, which is the only direction available to a shadow spell on a
/// light background — and it is exactly what the model's Opaque batch is for.
const SHADOW_IMPACT_BLOT_COLOR: Color = Color::srgb(0.100, 0.050, 0.170);
const SHADOW_IMPACT_BLOT_RADIUS: f32 = 0.62;
const SHADOW_IMPACT_BLOT_SECS: f32 = 0.22;
const SHADOW_IMPACT_BLOT_ALPHA: f32 = 0.85;

/// The additive flash riding on top of the blot.
const SHADOW_IMPACT_CORE_RADIUS: f32 = 0.72;
const SHADOW_IMPACT_CORE_SECS: f32 = 0.20;

/// How long a kind's whole burst lasts.
pub fn bolt_impact_life(kind: BoltKind) -> f32 {
    match kind {
        BoltKind::Frost => FROST_IMPACT_FLASH_SECS
            .max(FROST_IMPACT_RING_SECS)
            .max(FROST_IMPACT_SHARD_LIFE),
        BoltKind::Shadow => SHADOW_IMPACT_ARC_SECS
            .max(SHADOW_IMPACT_CORE_SECS)
            .max(SHADOW_IMPACT_BLOT_SECS),
    }
}

/// Where a bolt's burst plays, given its victim's feet.
pub fn bolt_impact_origin(target: Vec3) -> Vec3 {
    target + Vec3::Y * IMPACT_CHEST_Y
}

/// The two lateral anchors Shadow Bolt's arcs hang from, in the rig's frame.
///
/// Pure so the bilateral layout can be asserted directly: `side` is `+1`/`-1`,
/// local X is lateral and local Z points back at the caster.
pub fn shadow_arc_anchor(side: f32) -> Vec3 {
    Vec3::new(
        side * SHADOW_IMPACT_ARC_OFFSET,
        0.0,
        SHADOW_IMPACT_ARC_FORWARD,
    )
}

/// Unit direction of the `i`th of `n` ice chips.
///
/// A golden-angle spiral over the sphere: even coverage with no clustering, and
/// deterministic, so this never touches `game_rng`. A chip fan that clumps is
/// one chip drawn many times.
pub fn frost_shard_direction(i: u32, n: u32) -> Vec3 {
    let n = n.max(1) as f32;
    let y = 1.0 - 2.0 * (i as f32 + 0.5) / n;
    let r = (1.0 - y * y).max(0.0).sqrt();
    let theta = i as f32 * 2.399_963_2;
    Vec3::new(r * theta.cos(), y, r * theta.sin())
}

/// A four-point star: a tight core with narrow rays on both axes.
///
/// Stands in for `cyanstarflash.blp`. Same shape as the stun sparkle in
/// `hard_cc.rs` and generated for the same reason, but kept local so the two
/// can be tuned apart.
fn star_flash_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = BOLT_SPRITE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            let core = (1.0 - r).clamp(0.0, 1.0).powf(3.0);
            let ray = |across: f32, along: f32| {
                let width = (1.0 - across.abs() / 0.10).clamp(0.0, 1.0);
                let reach = (1.0 - along.abs()).clamp(0.0, 1.0).powf(1.6);
                width * width * reach * 0.85
            };
            let a = (core + ray(dy, dx) + ray(dx, dy)).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0) as u8;
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

/// A soft-edged band of unit radius in the XY plane.
///
/// `taper_ends` gives Shadow Bolt's open crescent, which fades to nothing at
/// both tips; without it the band closes into Frostbolt's shockwave ring.
///
/// Vertex-coloured rather than textured, the way `frost_nova`'s rings and
/// `mortal_strike`'s blade ribbon are: alpha runs to zero at both tips and at
/// both rims, so the stroke tapers to nothing instead of ending on a hard cut.
/// Pinned by `the_crescent_tapers_to_nothing_at_its_tips`.
pub fn build_arc_band(segments: u32, sweep: f32, width: f32, taper_ends: bool) -> Mesh {
    use bevy::render::render_asset::RenderAssetUsages;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let n = segments.max(3);
    // A closed loop must not repeat its seam vertex, or the ring shows a hairline
    // where the duplicate pair meets.
    let closed = !taper_ends;
    let rings = if closed { n } else { n + 1 };
    for i in 0..rings {
        let t = i as f32 / n as f32;
        let angle = (t - 0.5) * sweep;
        let (s, c) = angle.sin_cos();
        // An open stroke fades to nothing at both tips; a closed one holds full
        // strength the whole way round.
        let taper = if taper_ends {
            (t * std::f32::consts::PI).sin()
        } else {
            1.0
        };
        let half = width * 0.5 * if taper_ends { taper } else { 1.0 };
        for (k, edge) in [-1.0f32, 0.0, 1.0].iter().enumerate() {
            let r = 1.0 + edge * half;
            positions.push([c * r, s * r, 0.0]);
            normals.push([0.0, 0.0, 1.0]);
            uvs.push([t, (k as f32) * 0.5]);
            // Rims transparent, spine solid — this is the ENTIRE soft edge. An
            // untapered annulus renders as a hard-drawn hoop, which reads as a
            // geometric shape rather than a wavefront.
            let across = if *edge == 0.0 { 1.0 } else { 0.0 };
            colors.push([1.0, 1.0, 1.0, across * taper]);
        }
    }
    for i in 0..n {
        let a = i * 3;
        let b = ((i + 1) % rings) * 3;
        // Two quads: outer rim -> spine, spine -> inner rim.
        for k in 0..2 {
            let (p0, p1) = (a + k, b + k);
            indices.extend_from_slice(&[p0, p1, p0 + 1, p1, p1 + 1, p0 + 1]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// The roll a Shadow Bolt arc needs so that it bulges along the SAME direction
/// it is anchored on, once billboarded.
///
/// This is the whole bilateral read, and getting it wrong is subtle: the two
/// anchors are offset along a WORLD axis (the rig's lateral), while a
/// billboarded crescent curves within the CAMERA plane. Rolling by a fixed
/// ±90°, as the first build did, left those two unrelated — so instead of two
/// claws straddling the incoming line, the pair landed as the top and bottom of
/// a ring around the victim, which also made it read almost identically to
/// Frostbolt's shockwave.
///
/// The fix is to project the anchor axis into the camera plane and roll to its
/// angle there, flipping by π for the far side.
///
/// `lateral` is the rig's world-space X; `side` is `+1`/`-1`.
pub fn arc_roll(lateral: Vec3, camera: Quat, side: f32) -> f32 {
    let right = camera * Vec3::X;
    let up = camera * Vec3::Y;
    let angle = (lateral.dot(up)).atan2(lateral.dot(right));
    if side < 0.0 {
        angle + std::f32::consts::PI
    } else {
        angle
    }
}

/// Build the burst on a landed bolt.
///
/// Every piece hangs off the impact entity, which is posed with local **+Z
/// pointing back at the caster** and then follows the victim's chest. That
/// frame is what makes Shadow Bolt's arcs straddle the incoming line instead of
/// a fixed world axis.
pub fn spawn_bolt_impacts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<BoltAssets>>,
    mut star: Local<Option<Handle<Image>>>,
    new_impacts: Query<(Entity, &BoltImpact), Added<BoltImpact>>,
    targets: Query<&Transform>,
) {
    if new_impacts.is_empty() {
        return;
    }
    let assets =
        assets.get_or_insert_with(|| BoltAssets::build(&mut meshes, &mut materials, &mut images));
    let star = star
        .get_or_insert_with(|| images.add(star_flash_texture()))
        .clone();

    for (entity, impact) in new_impacts.iter() {
        let at = targets
            .get(impact.target)
            .map(|t| bolt_impact_origin(t.translation))
            .unwrap_or(Vec3::ZERO);
        // Local +Z looks back down the incoming line.
        let rotation = if impact.from.length_squared() > 1e-6 {
            Quat::from_rotation_arc(Vec3::Z, impact.from.normalize())
        } else {
            Quat::IDENTITY
        };
        commands.entity(entity).insert((
            Transform::from_translation(at).with_rotation(rotation),
            Visibility::default(),
        ));

        // Impacts are one-shots a second or two apart, so unlike the trail they
        // can each own their materials — which is what lets them fade by alpha
        // instead of only by scale. Same trade `spawn_cc_flare` makes.
        let (color, emissive) = match impact.kind {
            BoltKind::Frost => (FROST_IMPACT_COLOR, FROST_IMPACT_EMISSIVE),
            BoltKind::Shadow => (SHADOW_IMPACT_COLOR, SHADOW_IMPACT_EMISSIVE),
        };
        let glow = |materials: &mut Assets<StandardMaterial>, texture: Option<Handle<Image>>| {
            materials.add(StandardMaterial {
                base_color: color,
                base_color_texture: texture.clone(),
                emissive,
                emissive_texture: texture,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        };

        let mut parts: Vec<Entity> = Vec::new();
        match impact.kind {
            BoltKind::Frost => {
                parts.push(
                    commands
                        .spawn((
                            BoltImpactSprite {
                                role: BoltImpactRole::Flash,
                                radius: FROST_IMPACT_FLASH_RADIUS,
                                side: 0.0,
                            },
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(glow(&mut materials, Some(star.clone()))),
                            Transform::default(),
                            NotShadowCaster,
                        ))
                        .id(),
                );
                parts.push(
                    commands
                        .spawn((
                            BoltImpactSprite {
                                role: BoltImpactRole::Ring,
                                radius: FROST_IMPACT_RING_RADIUS,
                                side: 0.0,
                            },
                            Mesh3d(assets.ring.clone()),
                            MeshMaterial3d(glow(&mut materials, None)),
                            Transform::default(),
                            NotShadowCaster,
                        ))
                        .id(),
                );
                // Ice chips. These fade by shrinking, so they share one
                // material — there can be a dozen per hit.
                for i in 0..FROST_IMPACT_SHARD_COUNT {
                    let dir = frost_shard_direction(i, FROST_IMPACT_SHARD_COUNT);
                    let speed = FROST_IMPACT_SHARD_SPEED * (0.55 + 0.45 * bolt_jitter(i));
                    parts.push(
                        commands
                            .spawn((
                                BoltImpactShard {
                                    velocity: dir * speed,
                                    spin: FROST_IMPACT_SHARD_SPIN * (bolt_jitter(i ^ 0x9E37) - 0.5),
                                },
                                Mesh3d(assets.chip.clone()),
                                MeshMaterial3d(assets.frost_chip.clone()),
                                Transform::from_rotation(Quat::from_rotation_arc(Vec3::Y, dir)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
            }
            BoltKind::Shadow => {
                parts.push(
                    commands
                        .spawn((
                            BoltImpactSprite {
                                role: BoltImpactRole::Core,
                                radius: SHADOW_IMPACT_CORE_RADIUS,
                                side: 0.0,
                            },
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(glow(&mut materials, Some(assets.dot.clone()))),
                            Transform::default(),
                            NotShadowCaster,
                        ))
                        .id(),
                );
                // Spawned FIRST so it sits behind the additive layers.
                parts.push(
                    commands
                        .spawn((
                            BoltImpactSprite {
                                role: BoltImpactRole::Blot,
                                radius: SHADOW_IMPACT_BLOT_RADIUS,
                                side: 0.0,
                            },
                            Mesh3d(assets.blot.clone()),
                            MeshMaterial3d(materials.add(StandardMaterial {
                                base_color: SHADOW_IMPACT_BLOT_COLOR
                                    .with_alpha(SHADOW_IMPACT_BLOT_ALPHA),
                                // Blend, NOT Add — the whole point is to darken.
                                alpha_mode: AlphaMode::Blend,
                                perceptual_roughness: 0.7,
                                ..default()
                            })),
                            Transform::default(),
                            NotShadowCaster,
                        ))
                        .id(),
                );
                for side in [1.0f32, -1.0] {
                    parts.push(
                        commands
                            .spawn((
                                BoltImpactSprite {
                                    role: BoltImpactRole::Arc,
                                    radius: SHADOW_IMPACT_ARC_RADIUS,
                                    side,
                                },
                                Mesh3d(assets.crescent.clone()),
                                MeshMaterial3d(glow(&mut materials, None)),
                                Transform::from_translation(shadow_arc_anchor(side)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
            }
        }
        commands.entity(entity).add_children(&parts);
    }
}

/// Drive every live burst, and retire it when it is spent.
pub fn animate_bolt_impacts(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut impacts: Query<(Entity, &mut BoltImpact, &mut Transform, &Children)>,
    // The victim lookup is read-only, so it has to be provably disjoint from
    // the two mutable part queries below or Bevy rejects the set as B0001. No
    // combatant is ever a burst part, but only the filters can say so.
    targets: Query<
        &Transform,
        (
            With<Combatant>,
            Without<BoltImpact>,
            Without<BoltImpactSprite>,
            Without<BoltImpactShard>,
        ),
    >,
    mut sprites: Query<
        (
            &BoltImpactSprite,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<BoltImpact>, Without<BoltImpactShard>),
    >,
    mut shards: Query<
        (&mut BoltImpactShard, &mut Transform),
        (Without<BoltImpact>, Without<BoltImpactSprite>),
    >,
) {
    let dt = time.delta_secs();

    for (entity, mut impact, mut transform, children) in impacts.iter_mut() {
        impact.age += dt;
        let age = impact.age;
        if age >= bolt_impact_life(impact.kind) {
            commands.entity(entity).despawn();
            continue;
        }
        // Chest-attached: follow a victim that is still moving.
        if let Ok(target) = targets.get(impact.target) {
            transform.translation = bolt_impact_origin(target.translation);
        }

        for child in children.iter() {
            if let Ok((sprite, mut part, material)) = sprites.get_mut(child) {
                let (span, scale, alpha) = match sprite.role {
                    BoltImpactRole::Flash => {
                        let k = (age / FROST_IMPACT_FLASH_SECS).clamp(0.0, 1.0);
                        (
                            FROST_IMPACT_FLASH_SECS,
                            sprite.radius * 2.0 * (0.35 + 0.65 * k.sqrt()),
                            1.0 - k,
                        )
                    }
                    BoltImpactRole::Ring => {
                        let k = (age / FROST_IMPACT_RING_SECS).clamp(0.0, 1.0);
                        // Fast out of the gate then easing off, the way a
                        // shockwave loses speed.
                        (FROST_IMPACT_RING_SECS, sprite.radius * k.sqrt(), 1.0 - k)
                    }
                    BoltImpactRole::Core => {
                        let k = (age / SHADOW_IMPACT_CORE_SECS).clamp(0.0, 1.0);
                        (
                            SHADOW_IMPACT_CORE_SECS,
                            sprite.radius * 2.0 * (0.5 + 0.5 * k),
                            1.0 - k,
                        )
                    }
                    BoltImpactRole::Arc => {
                        let k = (age / SHADOW_IMPACT_ARC_SECS).clamp(0.0, 1.0);
                        (SHADOW_IMPACT_ARC_SECS, sprite.radius * k.sqrt(), 1.0 - k)
                    }
                    BoltImpactRole::Blot => {
                        let k = (age / SHADOW_IMPACT_BLOT_SECS).clamp(0.0, 1.0);
                        // Snaps open, then sinks away.
                        (
                            SHADOW_IMPACT_BLOT_SECS,
                            sprite.radius * (0.45 + 0.55 * k.powf(0.4)),
                            (1.0 - k) * SHADOW_IMPACT_BLOT_ALPHA,
                        )
                    }
                };
                if age > span {
                    part.scale = Vec3::ZERO;
                    continue;
                }
                part.scale = Vec3::splat(scale.max(1e-4));
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(alpha);
                }
            }

            if let Ok((mut shard, mut part)) = shards.get_mut(child) {
                if age > FROST_IMPACT_SHARD_LIFE {
                    part.scale = Vec3::ZERO;
                    continue;
                }
                shard.velocity.y -= FROST_IMPACT_SHARD_GRAVITY * dt;
                let velocity = shard.velocity;
                part.translation += velocity * dt;
                part.rotate_local_y(shard.spin * dt);
                let k = 1.0 - age / FROST_IMPACT_SHARD_LIFE;
                part.scale = Vec3::splat((FROST_IMPACT_SHARD_RADIUS * k.powf(0.7)).max(1e-4));
            }
        }
    }
}

/// Turns the flat pieces of a burst to face the camera.
///
/// The flash, ring, core and both arcs are all flat meshes hanging off a rig
/// that is aimed down the incoming line, so — exactly as with the missile
/// sprites — the rig's own rotation has to be cancelled out. The arcs keep
/// their roll: `side` flips one of them so the pair opens away from each other.
pub fn billboard_bolt_impacts(
    camera: Query<&Transform, (With<Camera3d>, Without<BoltImpact>, Without<BoltImpactSprite>)>,
    rigs: Query<(&Transform, &Children), With<BoltImpact>>,
    mut sprites: Query<
        (&BoltImpactSprite, &mut Transform),
        (
            Without<BoltImpact>,
            Without<Camera3d>,
            Without<BoltImpactShard>,
        ),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let facing = rig.rotation.inverse() * cam.rotation;
        for child in children.iter() {
            if let Ok((sprite, mut part)) = sprites.get_mut(child) {
                if sprite.role == BoltImpactRole::Blot {
                    // A sphere has no facing to correct.
                    continue;
                }
                part.rotation = match sprite.role {
                    // A crescent is built bulging toward its own local +X, so
                    // it has to be rolled onto whatever direction its anchor
                    // projects to on screen — see [`arc_roll`].
                    BoltImpactRole::Arc => {
                        let lateral = rig.rotation * Vec3::X;
                        facing
                            * Quat::from_rotation_z(arc_roll(lateral, cam.rotation, sprite.side))
                    }
                    _ => facing,
                };
            }
        }
    }
}
