use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::render::view::NoFrustumCulling;

use super::heal_cast::spell_hand_local;
use super::hunter_shots::{
    particle_rotation, spawn_client_particle, ClientEmitter, EmitterAssets, EmitterSprite,
};
use super::school_impact::impact_origin;
use super::warlock_dots::UA_CAMERA_LIFT;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ==============================================================================
// Mind Flay — Drain Mana's channel: two flickering stars joined by a mana beam
// ==============================================================================
//
// From the client data, `docs/design/2026-10-17-priest-talent-client-data.md`
// §6 (AS-335). Mind Flay plays visual 277, which is Drain Mana's whole visual.
// For the 3 s channel it holds three pieces:
//
//   stars  -> `manafunnel_impact_chest.m2` twice: at the Priest's casting hand
//             (kit 430, the `(11,12)` channel row) and on the victim's chest
//             (kit 814, the `(7,8)` state row). Each is two `starflash_grey`
//             quads, 2.38 yd tinted (49, 180, 255) and 1.1 yd white, additive,
//             their scale flickering 0.99 <-> 1.25 in eight steps every 267 ms.
//   beam   -> the `manabeam` chain (`SpellChainEffects` 4) between the two:
//             0.25 yd wide, 2.78 yd segments, re-seeded every 66 ms, white,
//             additive.
//   emitters at the victim's star: dark wisps (P1, 30/s, black, alpha-blended)
//             and blue sparks (P0, 20/s at 1.67 yd/s).
//
// Played as the signed-off bench plays it (AS-335 Priest Talent Visuals
// Bench, https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
// 1792207490-53c9, "Mind Flay", every layer kept and every multiplier at 1.0;
// its source as committed with the sign-off is
// `docs/design/benches/2026-10-17-priest-talent-bench.html`, the "flay" tab's
// `layers` and `customDraw`). The bench draws each star as two soft
// camera-facing glows (its `bbGlow`) and the beam as two additive strokes, a
// 0.25 yd pale-blue one under a 0.1 yd white core, zig-zagged through jittered
// joints that it re-seeds each 66 ms.
//
// **The slow** (aura 33 on the same spell) has no client visual. The ruling
// (2026-10-17) adds our shipped slow ring in a Shadow tint:
// `SlowTint::Shadow` in `slow_ring.rs`, routed by the aura's name.
//
// **The beam is a style, not a Mind Flay mesh.** The client's channel beams
// share one shape and differ only in texture (Drain Life's chain 8 is this
// geometry with `soulbeam.blp`), so the beam is a [`ChannelBeamStyle`] with a
// [`BeamTexture`]: another channel adds its texture there and its arm to
// [`ChannelVisual::for_channel`]. Drain Life keeps its own authored cylinder
// (`drain_life.rs`).
//
// Three placements the bench (which draws without depth) did not need:
//
// - The caster's star sits on the shipped spell-hand socket
//   (`heal_cast::spell_hand_local`, the main hand), not the bench's
//   0.45 yd offset, which is inside the 0.5 yd capsule.
// - Both stars, and the emitters on the victim's, are held in front of their
//   body, [`MANA_FUNNEL_CAMERA_LIFT`] nearer the camera than its axis
//   ([`held_before_body`]), so their white cores beat the body's depth test
//   (and a stacked Corruption shroud's) instead of reading as a dim annulus
//   round the silhouette — the UA glow's fix (`warlock_dots.rs`). A hand on
//   the far side of the Priest comes round in front of it. The move is along
//   the view ray, so each star stays on its anchor on screen.
// - The beam's ends stay on the hand and the chest themselves: it runs into
//   the victim's body.
//
// Lifecycle: a channel visual exists exactly while its caster is alive and
// channelling a mapped ability, uninterrupted, on a living target. An
// interrupt (the `interrupted` flag), the channel's end, either unit's death
// or a new channel on another target takes it down the next frame.
//
// Graphical-only: reads `ChannelingState`, writes no sim state, draws no
// `game_rng` (the scatter is the shared deterministic hash), and is registered
// in `states/mod.rs` only — headless stays byte-identical.

// ── The bench's knobs (signed off 2026-10-17, version 1792207490-53c9) ──────

pub const MIND_FLAY_STARS: bool = true;
pub const MIND_FLAY_BEAM: bool = true;
pub const MIND_FLAY_WISPS: bool = true;
pub const MIND_FLAY_SPARKS: bool = true;
/// Multipliers over the client (1.0 = client): `SIZE` scales the stars,
/// `OPACITY` their alpha (the bench's two knobs on this tab).
pub const MIND_FLAY_SIZE_MUL: f32 = 1.0;
pub const MIND_FLAY_OPACITY_MUL: f32 = 1.0;

// ── Routing ──────────────────────────────────────────────────────────────────

/// A channel the client draws with a channel visual of its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ChannelVisual {
    /// Visual 277, Drain Mana's: the flickering mana-funnel stars at both ends
    /// and the mana beam between them.
    ManaFunnel,
}

impl ChannelVisual {
    /// The channel visual `ability` plays, or `None` when it has none here.
    /// The one router the systems and `tests/lands_silently_audit.rs` ask.
    pub fn for_channel(ability: AbilityType) -> Option<ChannelVisual> {
        match ability {
            AbilityType::MindFlay => Some(ChannelVisual::ManaFunnel),
            _ => None,
        }
    }

    /// The beam this visual draws between its two ends.
    pub fn beam(self) -> ChannelBeamStyle {
        match self {
            ChannelVisual::ManaFunnel => MANA_BEAM,
        }
    }
}

// ── The stars ────────────────────────────────────────────────────────────────

/// One of a mana-funnel star's two quads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum StarQuad {
    /// The 2.38 yd pale-blue quad.
    Outer,
    /// The 1.1 yd white quad.
    Inner,
}

impl StarQuad {
    pub const ALL: [StarQuad; 2] = [StarQuad::Outer, StarQuad::Inner];

    /// Its size across before the flicker, yards (the client's quads).
    pub fn size(self) -> f32 {
        MIND_FLAY_SIZE_MUL
            * match self {
                StarQuad::Outer => 2.38,
                StarQuad::Inner => 1.1,
            }
    }

    /// Its colour (sRGB): the client's tint on the outer quad, white inside.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            StarQuad::Outer => [49, 180, 255],
            StarQuad::Inner => [255, 255, 255],
        }
    }

    /// Its alpha, the bench's `bbGlow` weight.
    pub fn alpha(self) -> f32 {
        MIND_FLAY_OPACITY_MUL
            * match self {
                StarQuad::Outer => 0.45,
                StarQuad::Inner => 0.55,
            }
    }
}

/// Which end of the channel a star sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum StarEnd {
    /// The Priest's casting hand.
    Hand,
    /// The victim's chest. The emitters play here only.
    Chest,
}

impl StarEnd {
    pub const ALL: [StarEnd; 2] = [StarEnd::Hand, StarEnd::Chest];
}

/// The stars' scale flicker: the model's bone track, eight steps over its
/// 267 ms global loop (the bench's table).
pub const MANA_FUNNEL_FLICKER: [f32; 8] = [0.99, 1.1, 1.25, 1.05, 1.2, 0.99, 1.15, 1.02];
/// How long each flicker step holds, seconds (the bench's 33.4 ms).
pub const MANA_FUNNEL_FLICKER_STEP_SECS: f32 = 0.0334;

/// The stars' scale `age` seconds into the channel.
pub fn mana_funnel_flicker(age: f32) -> f32 {
    let step = (age.max(0.0) / MANA_FUNNEL_FLICKER_STEP_SECS).floor() as usize;
    MANA_FUNNEL_FLICKER[step % MANA_FUNNEL_FLICKER.len()]
}

/// How far in front of its body's axis each star (and the victim's emitters)
/// is held, toward the camera: the UA glow's lift, which clears the body and a
/// stacked Corruption shroud.
pub const MANA_FUNNEL_CAMERA_LIFT: f32 = UA_CAMERA_LIFT;

/// The caster's star: on the main spell hand's socket, turned with the caster.
pub fn mana_funnel_hand(caster: &Transform) -> Vec3 {
    caster.translation + caster.rotation * spell_hand_local(1.0)
}

/// The victim's star: at its chest attachment (a pet's, for a pet).
pub fn mana_funnel_chest(target: &Transform, is_pet: bool) -> Vec3 {
    impact_origin(ImpactAnchor::Chest, target.translation, is_pet)
}

/// Where a star on `anchor`, on the body standing at `body`, is drawn for a
/// camera at `camera`: on the anchor's own view ray (so it stays on the
/// anchor on screen), no farther from the camera than
/// [`MANA_FUNNEL_CAMERA_LIFT`] in front of the body's axis at the anchor's
/// height. A hand on the body's far side comes round in front of it; one
/// already in front stays where it is.
pub fn held_before_body(anchor: Vec3, body: Vec3, camera: Option<Vec3>) -> Vec3 {
    let Some(eye) = camera else {
        return anchor;
    };
    let axis = Vec3::new(body.x, anchor.y, body.z);
    let depth = eye
        .distance(anchor)
        .min(eye.distance(axis) - MANA_FUNNEL_CAMERA_LIFT);
    eye + (anchor - eye).normalize_or_zero() * depth
}

/// The bench's glow falloff (its `bbGlow` gradient): full at the centre, 0.45
/// halfway out, nothing at the rim, linear between. `r` is the distance from
/// the centre as a fraction of the quad's half-width.
pub fn mana_funnel_glow_alpha(r: f32) -> f32 {
    let r = r.max(0.0);
    if r <= 0.5 {
        1.0 - 1.1 * r
    } else {
        (0.45 * (1.0 - r) / 0.5).max(0.0)
    }
}

/// Generated glow sprite, pixels on a side.
pub const MANA_FUNNEL_GLOW_PX: u32 = 64;

/// The star sprite: white, with [`mana_funnel_glow_alpha`] in alpha.
pub fn mana_funnel_glow_texture() -> Image {
    let size = MANA_FUNNEL_GLOW_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let a = mana_funnel_glow_alpha((dx * dx + dy * dy).sqrt());
            let i = ((y * size + x) * 4) as usize;
            data[i..i + 4].copy_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
        }
    }
    texture(size, size, data)
}

/// Emissive gain of an additive piece over its colour: the client emitters'
/// convention (`hunter_shots.rs`'s `EMITTER_GLOW`).
pub const MANA_FUNNEL_GLOW: f32 = 2.5;

fn texture(width: u32, height: u32, data: Vec<u8>) -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// An additive glow material: `rgb` at `alpha`, the sprite driving both the
/// colour and the emissive (and `unlit` false, or Bevy drops the emissive).
fn additive_material(rgb: [u8; 3], alpha: f32, sprite: Handle<Image>) -> StandardMaterial {
    let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
    let c = color.to_linear();
    StandardMaterial {
        base_color: color.with_alpha(alpha),
        base_color_texture: Some(sprite.clone()),
        emissive: LinearRgba::rgb(
            c.red * MANA_FUNNEL_GLOW,
            c.green * MANA_FUNNEL_GLOW,
            c.blue * MANA_FUNNEL_GLOW,
        ),
        emissive_texture: Some(sprite),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

// ── The beam ─────────────────────────────────────────────────────────────────

/// What a channel beam is drawn with: the client's chain-effect texture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BeamTexture {
    /// `manabeam.blp`, as the bench draws it: a 0.25 yd pale-blue stroke at
    /// 0.35 under a 0.1 yd white core at 0.9, added together.
    ManaBeam,
}

/// The mana beam's outer stroke: its colour and alpha (the bench's
/// `[0.25, 0.35, [150, 210, 255]]`).
pub const MANA_BEAM_OUTER_RGB: [u8; 3] = [150, 210, 255];
pub const MANA_BEAM_OUTER_ALPHA: f32 = 0.35;
/// The core stroke: its width as a fraction of the beam's, colour and alpha
/// (the bench's `[0.1, 0.9, [240, 250, 255]]`).
pub const MANA_BEAM_CORE_FRACTION: f32 = 0.1 / 0.25;
pub const MANA_BEAM_CORE_RGB: [u8; 3] = [240, 250, 255];
pub const MANA_BEAM_CORE_ALPHA: f32 = 0.9;

/// Texels across a beam texture, and along it (it does not vary along).
pub const BEAM_TEXTURE_ACROSS_PX: u32 = 64;
pub const BEAM_TEXTURE_ALONG_PX: u32 = 4;

impl BeamTexture {
    /// The texel at `d`, the distance from the beam's centre line as a
    /// fraction of its half-width: `(sRGB, alpha)`. The mana beam adds its two
    /// strokes as the bench's `lighter` composite does; inside the core every
    /// channel saturates (0.35 x 150 + 0.9 x 240 > 255, and so on), so the
    /// core is opaque white.
    pub fn texel(self, d: f32) -> ([u8; 3], f32) {
        match self {
            BeamTexture::ManaBeam => {
                let d = d.abs();
                if d > 1.0 {
                    return ([0, 0, 0], 0.0);
                }
                let mut added = [0.0f32; 3];
                let mut strokes = vec![(MANA_BEAM_OUTER_RGB, MANA_BEAM_OUTER_ALPHA)];
                if d <= MANA_BEAM_CORE_FRACTION {
                    strokes.push((MANA_BEAM_CORE_RGB, MANA_BEAM_CORE_ALPHA));
                }
                for (rgb, a) in strokes {
                    for c in 0..3 {
                        added[c] += rgb[c] as f32 * a;
                    }
                }
                // Back to colour and alpha: the brightest channel is the
                // alpha, the colour the light per unit of it.
                let peak = added.iter().cloned().fold(0.0f32, f32::max).min(255.0);
                let alpha = peak / 255.0;
                let rgb = added.map(|c| (c.min(255.0) / alpha).min(255.0).round() as u8);
                (rgb, alpha)
            }
        }
    }

    /// The texture: `v` runs across the beam, edge to edge; nothing varies
    /// along it. Each texel averages four sub-samples across, so a stroke's
    /// edge is antialiased as the canvas strokes it.
    pub fn image(self) -> Image {
        let (w, h) = (BEAM_TEXTURE_ALONG_PX, BEAM_TEXTURE_ACROSS_PX);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            let mut light = [0.0f32; 3];
            let mut alpha = 0.0;
            const SUB: u32 = 4;
            for s in 0..SUB {
                let v = (y as f32 + (s as f32 + 0.5) / SUB as f32) / h as f32;
                let (rgb, a) = self.texel(v * 2.0 - 1.0);
                for c in 0..3 {
                    light[c] += rgb[c] as f32 * a / SUB as f32;
                }
                alpha += a / SUB as f32;
            }
            let rgb = if alpha > 0.0 {
                light.map(|c| (c / alpha).min(255.0).round() as u8)
            } else {
                [0, 0, 0]
            };
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[
                    rgb[0],
                    rgb[1],
                    rgb[2],
                    (alpha * 255.0).round() as u8,
                ]);
            }
        }
        texture(w, h, data)
    }
}

/// A channel beam's shape: the chain effect's width and segment length, how
/// often its joints are re-seeded (the texture's flip frame), how far a joint
/// strays, and its texture.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ChannelBeamStyle {
    pub texture: BeamTexture,
    /// Across the beam, yards (`StartWidth` / `EndWidth`).
    pub width: f32,
    /// Average segment length, yards (`AvgSegLen`).
    pub segment_len: f32,
    /// Seconds between re-seeds (`TimePerFlipFrame`).
    pub flip_secs: f32,
    /// How far an inner joint strays along each screen axis, yards.
    pub jitter: f32,
}

/// The mana beam: chain effect 4, as the bench draws it. Its joints stray 6 px
/// at the bench's framing (11 yd off, 1000 px focal): 6 x 11 / 1000 yd.
pub const MANA_BEAM: ChannelBeamStyle = ChannelBeamStyle {
    texture: BeamTexture::ManaBeam,
    width: 0.25,
    segment_len: 2.78,
    flip_secs: 0.066,
    jitter: 6.0 * 11.0 / 1000.0,
};

/// The beam's re-seed `age` seconds into the channel.
pub fn beam_flip(style: &ChannelBeamStyle, age: f32) -> u32 {
    (age.max(0.0) / style.flip_secs).floor() as u32
}

/// How many segments the beam runs in over `length` yards: one per
/// [`ChannelBeamStyle::segment_len`], at least one.
pub fn beam_segments(style: &ChannelBeamStyle, length: f32) -> usize {
    ((length / style.segment_len).round() as usize).max(1)
}

/// The beam's joints from `from` to `to`: the ends exact, each inner joint
/// strayed in the camera's plane by the bench's offsets for re-seed `flip`
/// (`sin(seed 1.7 + i 2.3)` across, `cos(seed 1.3 + i)` down the screen).
pub fn beam_joints(
    style: &ChannelBeamStyle,
    from: Vec3,
    to: Vec3,
    flip: u32,
    camera_right: Vec3,
    camera_up: Vec3,
) -> Vec<Vec3> {
    let n = beam_segments(style, from.distance(to));
    let seed = flip as f32;
    (0..=n)
        .map(|i| {
            let at = from.lerp(to, i as f32 / n as f32);
            if i == 0 || i == n {
                return at;
            }
            let i = i as f32;
            at + camera_right * (seed * 1.7 + i * 2.3).sin() * style.jitter
                - camera_up * (seed * 1.3 + i).cos() * style.jitter
        })
        .collect()
}

/// The ribbon's two edges at each joint: `width` apart, across the beam and
/// square to the view from `camera`, so the ribbon faces it.
pub fn beam_edges(joints: &[Vec3], width: f32, camera: Vec3) -> Vec<(Vec3, Vec3)> {
    let n = joints.len();
    (0..n)
        .map(|k| {
            let ahead = joints[(k + 1).min(n - 1)];
            let behind = joints[k.saturating_sub(1)];
            let along = (ahead - behind).normalize_or_zero();
            let view = (camera - joints[k]).normalize_or_zero();
            let side = along.cross(view).normalize_or_zero() * (0.5 * width);
            (joints[k] - side, joints[k] + side)
        })
        .collect()
}

/// The ribbon mesh through `edges`: a quad per segment, `u` along the beam and
/// `v` across it, facing `camera`.
pub fn beam_mesh(edges: &[(Vec3, Vec3)], camera: Vec3) -> Mesh {
    let n = edges.len();
    let mut positions = Vec::with_capacity(n * 2);
    let mut normals = Vec::with_capacity(n * 2);
    let mut uvs = Vec::with_capacity(n * 2);
    for (k, (a, b)) in edges.iter().enumerate() {
        let u = k as f32 / (n.max(2) - 1) as f32;
        let normal = (camera - (*a + *b) * 0.5).normalize_or(Vec3::Y);
        positions.push(a.to_array());
        positions.push(b.to_array());
        normals.push(normal.to_array());
        normals.push(normal.to_array());
        uvs.push([u, 0.0]);
        uvs.push([u, 1.0]);
    }
    let mut indices = Vec::with_capacity(n.saturating_sub(1) * 6);
    for k in 0..n.saturating_sub(1) as u32 {
        let (a, b, c, d) = (2 * k, 2 * k + 1, 2 * k + 2, 2 * k + 3);
        indices.extend_from_slice(&[a, b, c, b, d, c]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ── The emitters (at the victim's star) ─────────────────────────────────────

/// P1, the dark wisps (`gradient64flipa`), as the bench plays them: 30 a
/// second in every direction at 2.2 yd/s, living 0.5 s, black and
/// alpha-blended (they darken), fading out; radii 0.01 -> 0.14 -> 0.03 (the
/// bench draws a key as a radius).
pub const MIND_FLAY_WISP_EMITTER: ClientEmitter = ClientEmitter {
    name: "manafunnel_impact_chest/p1 wisps",
    sprite: EmitterSprite::Glow,
    additive: false,
    rate: 30.0,
    window: None,
    life: 0.5,
    speed: 2.2,
    gravity: 0.0,
    area: 0.0,
    column: None,
    rise: false,
    color: [[0, 0, 0], [0, 0, 0], [0, 0, 0]],
    alpha: [1.0, 0.5, 0.0],
    size: ParticleSize::radii([0.01, 0.14, 0.03]),
    mid: 0.5,
    track: None,
};

/// P0, the blue sparks (`flare`): 20 a second in every direction at
/// 1.67 yd/s, living 0.7 s, violet -> blue -> black, fading out; radii
/// 0.08 -> 0.06 -> 0.003. Added light.
pub const MIND_FLAY_SPARK_EMITTER: ClientEmitter = ClientEmitter {
    name: "manafunnel_impact_chest/p0 sparks",
    sprite: EmitterSprite::Flare,
    additive: true,
    rate: 20.0,
    window: None,
    life: 0.7,
    speed: 1.67,
    gravity: 0.0,
    area: 0.0,
    column: None,
    rise: false,
    color: [[45, 20, 139], [0, 42, 255], [0, 0, 0]],
    alpha: [1.0, 0.5, 0.0],
    size: ParticleSize::radii([0.08, 0.06, 0.003]),
    mid: 0.5,
    track: None,
};

/// The victim star's emitters, in [`ManaFunnelChannel::carry`] order, with
/// whether each is kept.
pub fn mind_flay_emitters() -> [(bool, &'static ClientEmitter); 2] {
    [
        (MIND_FLAY_WISPS, &MIND_FLAY_WISP_EMITTER),
        (MIND_FLAY_SPARKS, &MIND_FLAY_SPARK_EMITTER),
    ]
}

// ── Components ───────────────────────────────────────────────────────────────

/// One channel's visual: an unmoving root at the origin, parent of the two
/// stars and the beam. Lives exactly as long as its channel qualifies
/// ([`sync_mind_flay_channels`]).
#[derive(Component, Debug)]
pub struct ManaFunnelChannel {
    pub caster: Entity,
    pub target: Entity,
    pub ability: AbilityType,
    /// Seconds since the channel's visual appeared.
    pub age: f32,
    /// Particles owed but not yet emitted, per emitter.
    pub carry: [f32; 2],
    /// Particles emitted, the scatter's seed.
    pub emitted: u32,
}

/// A star's rig: at its end, unrotated, parent of its two quads (and, on the
/// victim's, the emitters' particles).
#[derive(Component, Debug)]
pub struct ManaFunnelStar {
    pub end: StarEnd,
}

/// One quad of a [`ManaFunnelStar`].
#[derive(Component, Debug)]
pub struct ManaFunnelStarQuad {
    pub quad: StarQuad,
}

/// A channel's beam: a ribbon mesh in world space, rebuilt each frame.
#[derive(Component, Debug)]
pub struct ChannelBeam {
    pub style: ChannelBeamStyle,
}

/// The meshes, sprites and materials every channel visual shares.
pub struct ManaFunnelAssets {
    quad: Handle<Mesh>,
    star: [Handle<StandardMaterial>; 2],
    mana_beam: Handle<StandardMaterial>,
}

impl ManaFunnelAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let glow = images.add(mana_funnel_glow_texture());
        let beam = images.add(BeamTexture::ManaBeam.image());
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            star: StarQuad::ALL
                .map(|q| materials.add(additive_material(q.rgb(), q.alpha(), glow.clone()))),
            mana_beam: materials.add(additive_material([255, 255, 255], 1.0, beam)),
        }
    }

    fn star(&self, quad: StarQuad) -> Handle<StandardMaterial> {
        self.star[StarQuad::ALL.iter().position(|q| *q == quad).unwrap_or(0)].clone()
    }

    fn beam(&self, texture: BeamTexture) -> Handle<StandardMaterial> {
        match texture {
            BeamTexture::ManaBeam => self.mana_beam.clone(),
        }
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// The channel `caster` is running that draws a channel visual, as
/// `(target, ability)`, or `None`: interrupted, unmapped, its caster dead, or
/// its target dead or gone.
fn live_channel(
    caster: &Combatant,
    channel: &ChannelingState,
    units: &Query<&Combatant>,
) -> Option<(Entity, AbilityType)> {
    if channel.interrupted || !caster.is_alive() {
        return None;
    }
    ChannelVisual::for_channel(channel.ability)?;
    units
        .get(channel.target)
        .is_ok_and(Combatant::is_alive)
        .then_some((channel.target, channel.ability))
}

/// Raise a channel visual for every live mapped channel, and take each down
/// when its channel ends, is interrupted, or loses its caster or its target.
#[allow(clippy::too_many_arguments)]
pub fn sync_mind_flay_channels(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<ManaFunnelAssets>>,
    casters: Query<(Entity, &Combatant, &ChannelingState)>,
    units: Query<&Combatant>,
    visuals: Query<(Entity, &ManaFunnelChannel)>,
) {
    let live = |caster: Entity| {
        casters
            .get(caster)
            .ok()
            .and_then(|(_, c, channel)| live_channel(c, channel, &units))
    };
    for (entity, visual) in visuals.iter() {
        if live(visual.caster) != Some((visual.target, visual.ability)) {
            commands.entity(entity).despawn();
        }
    }
    for (caster, combatant, channel) in casters.iter() {
        let Some((target, ability)) = live_channel(combatant, channel, &units) else {
            continue;
        };
        if visuals
            .iter()
            .any(|(_, v)| v.caster == caster && v.target == target && v.ability == ability)
        {
            continue;
        }
        let Some(style) = ChannelVisual::for_channel(ability).map(ChannelVisual::beam) else {
            continue;
        };
        let assets = assets.get_or_insert_with(|| {
            ManaFunnelAssets::build(&mut meshes, &mut images, &mut materials)
        });
        let root = commands
            .spawn((
                ManaFunnelChannel {
                    caster,
                    target,
                    ability,
                    age: 0.0,
                    carry: [0.0; 2],
                    emitted: 0,
                },
                Transform::IDENTITY,
                Visibility::default(),
                PlayMatchEntity,
            ))
            .id();
        if MIND_FLAY_STARS {
            for end in StarEnd::ALL {
                let star = commands
                    .spawn((
                        ManaFunnelStar { end },
                        Transform::IDENTITY,
                        Visibility::default(),
                    ))
                    .id();
                for quad in StarQuad::ALL {
                    let piece = commands
                        .spawn((
                            ManaFunnelStarQuad { quad },
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(assets.star(quad)),
                            Transform::from_scale(Vec3::ZERO),
                            NotShadowCaster,
                        ))
                        .id();
                    commands.entity(star).add_child(piece);
                }
                commands.entity(root).add_child(star);
            }
        }
        if MIND_FLAY_BEAM {
            let beam = commands
                .spawn((
                    ChannelBeam { style },
                    Mesh3d(meshes.add(beam_mesh(&[], Vec3::ZERO))),
                    MeshMaterial3d(assets.beam(style.texture)),
                    Transform::IDENTITY,
                    NotShadowCaster,
                    // Rebuilt in world space every frame: its bounds never
                    // follow it.
                    NoFrustumCulling,
                ))
                .id();
            commands.entity(root).add_child(beam);
        }
    }
}

/// Pose every channel visual: each star on its end, held toward the camera,
/// its quads facing it at the flicker's scale; the beam rebuilt from hand to
/// chest; the victim star's spent particles retired and owed ones emitted.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_mind_flay_channels(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut emitters: Local<Option<EmitterAssets>>,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<ManaFunnelStar>,
            Without<ManaFunnelStarQuad>,
        ),
    >,
    mut channels: Query<(&mut ManaFunnelChannel, &Children)>,
    units: Query<
        (&Transform, Option<&Pet>),
        (
            With<Combatant>,
            Without<ManaFunnelStar>,
            Without<ManaFunnelStarQuad>,
        ),
    >,
    mut stars: Query<
        (&ManaFunnelStar, &mut Transform, &Children),
        (
            Without<ManaFunnelStarQuad>,
            Without<Camera3d>,
            Without<Combatant>,
        ),
    >,
    mut quads: Query<
        (&ManaFunnelStarQuad, &mut Transform),
        (
            Without<ManaFunnelStar>,
            Without<Camera3d>,
            Without<Combatant>,
        ),
    >,
    beams: Query<(&ChannelBeam, &Mesh3d)>,
    particles: Query<&ClientParticle>,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next().copied();
    let eye = cam.map(|c| c.translation);
    let cam_rotation = cam.map_or(Quat::IDENTITY, |c| c.rotation);
    let (cam_right, cam_up) = (cam_rotation * Vec3::X, cam_rotation * Vec3::Y);
    for (mut channel, children) in channels.iter_mut() {
        let (Ok((caster, _)), Ok((target, pet))) =
            (units.get(channel.caster), units.get(channel.target))
        else {
            continue;
        };
        let hand = mana_funnel_hand(caster);
        let chest = mana_funnel_chest(target, pet.is_some());
        let age = channel.age;
        let flicker = mana_funnel_flicker(age);
        let mut chest_star = None;
        for child in children.iter() {
            if let Ok((star, mut at, pieces)) = stars.get_mut(child) {
                let (end, body) = match star.end {
                    StarEnd::Hand => (hand, caster.translation),
                    StarEnd::Chest => {
                        chest_star = Some(child);
                        (chest, target.translation)
                    }
                };
                at.translation = held_before_body(end, body, eye);
                for piece in pieces.iter() {
                    if let Ok((quad, mut pose)) = quads.get_mut(piece) {
                        pose.rotation = cam_rotation;
                        pose.scale =
                            Vec3::new(quad.quad.size() * flicker, quad.quad.size() * flicker, 1.0);
                    } else if particles.get(piece).is_ok_and(|p| p.age >= p.life) {
                        commands.entity(piece).despawn();
                    }
                }
            } else if let Ok((beam, mesh)) = beams.get(child) {
                if let Some(m) = meshes.get_mut(&mesh.0) {
                    let style = beam.style;
                    let joints = beam_joints(
                        &style,
                        hand,
                        chest,
                        beam_flip(&style, age),
                        cam_right,
                        cam_up,
                    );
                    let view = eye.unwrap_or(hand + Vec3::Y);
                    *m = beam_mesh(&beam_edges(&joints, style.width, view), view);
                }
            }
        }
        channel.age += dt;

        let Some(chest_star) = chest_star else {
            continue;
        };
        for (k, (kept, e)) in mind_flay_emitters().into_iter().enumerate() {
            if !kept {
                continue;
            }
            channel.carry[k] += e.rate * dt;
            if channel.carry[k] < 1.0 {
                continue;
            }
            let assets =
                emitters.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
            let quad = assets.quad();
            let palette = assets.palette(&mut materials, e);
            while channel.carry[k] >= 1.0 {
                channel.carry[k] -= 1.0;
                let seed = channel
                    .caster
                    .index()
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(channel.emitted.wrapping_mul(0x85EB_CA6B));
                channel.emitted = channel.emitted.wrapping_add(1);
                let spark = spawn_client_particle(
                    &mut commands,
                    &quad,
                    e,
                    &palette,
                    Vec3::ZERO,
                    seed,
                    true,
                );
                commands.entity(chest_star).add_child(spark);
            }
        }
    }
}

/// Turn every live particle of a victim's star to face the camera, after the
/// client particles have aged and moved.
#[allow(clippy::type_complexity)]
pub fn billboard_mind_flay_particles(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<ManaFunnelStar>,
            Without<ClientParticle>,
        ),
    >,
    stars: Query<(&Transform, &Children), With<ManaFunnelStar>>,
    mut particles: Query<
        (&ClientParticle, &mut Transform),
        (Without<Camera3d>, Without<ManaFunnelStar>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (star, children) in stars.iter() {
        for child in children.iter() {
            if let Ok((particle, mut part)) = particles.get_mut(child) {
                part.rotation = particle_rotation(particle, star.rotation, cam.rotation);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn the_glow_falloff_is_the_benchs_gradient() {
        for (r, want) in [
            (0.0, 1.0),
            (0.25, 0.725),
            (0.5, 0.45),
            (0.75, 0.225),
            (1.0, 0.0),
            (1.4, 0.0),
        ] {
            assert!(close(mana_funnel_glow_alpha(r), want), "{r}");
        }
    }

    #[test]
    fn only_mind_flay_draws_the_mana_funnel() {
        assert_eq!(
            ChannelVisual::for_channel(AbilityType::MindFlay),
            Some(ChannelVisual::ManaFunnel)
        );
        assert_eq!(ChannelVisual::for_channel(AbilityType::DrainLife), None);
    }
}
