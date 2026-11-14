use std::f32::consts::PI;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::cheat_death::{plays_cheat_death, spawn_cheat_death, CHEAT_DEATH_SAVED_CUE};
use super::hunter_shots::{
    particle_rotation, spawn_client_particle, ClientEmitter, EmitterAssets, EmitterSprite,
};
use super::immolate::FEET_Y;
use super::warlock_dots::dot_anchor;
use crate::states::play_match::combat_core::KillingBlowPrevented;
use crate::states::play_match::components::*;
use crate::states::play_match::utils::get_next_fct_offset;
use crate::states::play_match::FCT_HEIGHT;

// ==============================================================================
// Guardian Spirit — the spirit over the shoulders, and the shared "Saved" cue
// ==============================================================================
//
// From the client data, `docs/design/2026-10-17-priest-talent-client-data.md`
// §5 (AS-335). Guardian Spirit is a Wrath spell, read from Wrath Classic
// 3.4.5.63697: its cast is the shipped holy hands (kit 270, `heal_cast.rs`),
// its state for 10 s is `priest_guardianspirit.m2` (kit 11186) at the ally's
// Base attachment, every emitter 1.7-2.0 yd above the feet behind the
// shoulders, and its save is the shipped Heal landing (kit 232,
// `heal_impact.rs`'s `HealStream`).
//
// Played as the signed-off bench plays it (AS-335 Priest Talent Visuals
// Bench, https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
// 1792207490-53c9, "Guardian Spirit", every layer kept and every multiplier
// at 1.0; its source as committed with the sign-off is
// `docs/design/benches/2026-10-17-priest-talent-bench.html`, the "gs" tab's
// `customDraw`). The bench draws the state as soft camera-facing glow panels
// (its `bbGlow`) plus one particle emitter, so this does too:
//
//   halo   -> a dark alpha-blended disc behind it (the client's `flare05_dark`),
//             a thin gold ring lying level above it, and a pale-gold ring glow
//             that widens 0.28 -> 1.04 yd each 0.466 s loop (`ring_bow`);
//   wings  -> two gold lobes either side, the pair 2.2 -> 2.8 yd wide
//             (`wings3`): growing through the onset, then breathing;
//   band   -> a horizontal aura band widening 0.96 -> 3.9 yd each loop
//             (`hero_aura_02_h`), and in the onset two long gold flares
//             (`flare_long`) that flash 0.54-1.1 s;
//   sparks -> gold sparks, 100 a second at 2.8 yd/s, living 0.5 s.
//
// The onset is the first 1.8 s; the loop re-emits every 0.466 s.
//
// **The spirit owns its application.** Its face and rider route off the
// family's aura band by NAME (`AuraApplyRoute::for_instance`,
// `AuraApplyOwner::GuardianSpirit`): the onset is the landing.
//
// **The save.** A killing blow prevented spawns `KillingBlowPrevented` in both
// modes (`combat_core/follow_ups.rs`); `consume_killing_blow_signals` reads it
// here, graphical-only, and plays the client's heal landing (kit 232) with the
// heal's number when the save carried a heal, and the authored "Saved" cue —
// the user's ruling on AS-335 — for every save its holder lives through:
// Guardian Spirit's and the Rogue's Cheat Death's alike ([`spawn_saved_cue`]).
// A save whose holder another blow of the same frame kills plays nothing. The
// cue is gold "Saved" in `fct::INVULNERABLE` at number size, stacked one line
// above the heal's number ON SCREEN (`fct::LINE_ABOVE`, so it holds at the
// match camera's 60 yd as well as the bench's 7), and a gold body flash,
// 0.6 s, peaking at 0.7 alpha at 80 ms.
//
// Graphical-only: reads `ActiveAuras` and the marker, writes no sim state,
// draws no `game_rng` (the sparks' scatter is the shared deterministic hash),
// and is registered in `states/mod.rs` only — headless stays byte-identical.

// ── The bench's knobs (signed off 2026-10-17, version 1792207490-53c9) ──────

pub const GUARDIAN_SPIRIT_WINGS: bool = true;
pub const GUARDIAN_SPIRIT_HALO: bool = true;
pub const GUARDIAN_SPIRIT_BAND: bool = true;
pub const GUARDIAN_SPIRIT_SPARKS: bool = true;
/// The onset (`Stand`, 1800 ms) and the loop (sequence 158, 466 ms).
pub const GUARDIAN_SPIRIT_ONSET_SECS: f32 = 1.8;
pub const GUARDIAN_SPIRIT_LOOP_SECS: f32 = 0.466;
/// Multipliers over the client (1.0 = client). The bench's size knob scales
/// every panel; its spread knob (also 1.0) scales them with it.
pub const GUARDIAN_SPIRIT_SIZE_MUL: f32 = 1.0;
pub const GUARDIAN_SPIRIT_OPACITY_MUL: f32 = 1.0;
/// The authored "Saved" cue (the AS-335 ruling), shared with Cheat Death.
pub const GUARDIAN_SPIRIT_SAVED_CUE: bool = true;

// ── Where it sits ────────────────────────────────────────────────────────────

/// The spirit's centre above the bearer's feet (the bench's `top`, y 1.9).
pub const GUARDIAN_SPIRIT_HEIGHT: f32 = 1.9;
/// How far behind the bearer's shoulders, along its facing (the bench's
/// `top`, z -0.25: the model's emitters sit behind the shoulders).
pub const GUARDIAN_SPIRIT_BEHIND: f32 = 0.25;
/// The sparks' emitter, relative to the spirit's centre: the bench emits them
/// at 1.85 yd above the feet, 0.05 below the panels.
pub const GUARDIAN_SPIRIT_SPARK_DROP: f32 = 0.05;

// ── The panels ───────────────────────────────────────────────────────────────

/// One of the spirit's drawn panels.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SpiritPanel {
    /// The dark circle behind the halo: alpha-blended, hard-edged.
    DarkCircle,
    /// The thin gold ring lying level above the halo.
    HaloRing,
    /// The pale-gold ring glow, widening each loop.
    RingGlow,
    /// The wings' two lobes.
    WingLeft,
    WingRight,
    /// The horizontal aura band, widening each loop.
    Band,
    /// The onset's two long gold flares.
    FlareLeft,
    FlareRight,
}

impl SpiritPanel {
    pub const ALL: [SpiritPanel; 8] = [
        SpiritPanel::DarkCircle,
        SpiritPanel::HaloRing,
        SpiritPanel::RingGlow,
        SpiritPanel::WingLeft,
        SpiritPanel::WingRight,
        SpiritPanel::Band,
        SpiritPanel::FlareLeft,
        SpiritPanel::FlareRight,
    ];

    /// The panel's colour, the bench's sRGB.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            SpiritPanel::DarkCircle => [60, 55, 40],
            SpiritPanel::HaloRing => [253, 221, 8],
            SpiritPanel::RingGlow => [255, 255, 127],
            SpiritPanel::WingLeft | SpiritPanel::WingRight => [255, 200, 70],
            SpiritPanel::Band => [249, 240, 170],
            SpiritPanel::FlareLeft | SpiritPanel::FlareRight => [255, 211, 107],
        }
    }

    /// What the panel is drawn with.
    pub fn sprite(self) -> PanelSprite {
        match self {
            SpiritPanel::DarkCircle => PanelSprite::Disc,
            SpiritPanel::HaloRing => PanelSprite::LevelRing,
            SpiritPanel::RingGlow
            | SpiritPanel::WingLeft
            | SpiritPanel::WingRight
            | SpiritPanel::Band
            | SpiritPanel::FlareLeft
            | SpiritPanel::FlareRight => PanelSprite::Glow,
        }
    }

    /// Whether the panel adds light (the client's blend 4); the dark circle
    /// is the one blend-2 layer, which darkens.
    pub fn additive(self) -> bool {
        self != SpiritPanel::DarkCircle
    }

    /// The bench layer the panel belongs to, and whether that layer is kept.
    fn kept(self) -> bool {
        match self {
            SpiritPanel::DarkCircle | SpiritPanel::HaloRing | SpiritPanel::RingGlow => {
                GUARDIAN_SPIRIT_HALO
            }
            SpiritPanel::WingLeft | SpiritPanel::WingRight => GUARDIAN_SPIRIT_WINGS,
            SpiritPanel::Band | SpiritPanel::FlareLeft | SpiritPanel::FlareRight => {
                GUARDIAN_SPIRIT_BAND
            }
        }
    }
}

/// How a panel is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelSprite {
    /// The bench's soft glow (its `bbGlow`): [`panel_glow_alpha`] across a
    /// camera-facing quad.
    Glow,
    /// A hard-edged disc facing the camera.
    Disc,
    /// A thin ring lying level, in the ground plane.
    LevelRing,
}

/// A panel at one moment, relative to the spirit's centre: `right` along the
/// camera's right, `up` along world up, `width` × `height` in yards (a level
/// ring's `width` is its diameter), and its alpha.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PanelPose {
    pub right: f32,
    pub up: f32,
    pub width: f32,
    pub height: f32,
    pub alpha: f32,
}

/// The gold ring's radius and line width. The bench strokes it 3 px wide at
/// its own framing (7 yd off, 1000 px focal): 3 / (1000 / 7) = 0.021 yd.
pub const HALO_RING_RADIUS: f32 = 0.35;
pub const HALO_RING_WIDTH: f32 = 0.021;
/// The gold ring's height above the spirit's centre.
pub const HALO_RING_LIFT: f32 = 0.15;

/// The bench's keyed track, linearly interpolated, held at its ends.
fn keyed(track: &[(f32, f32)], t: f32) -> f32 {
    if t <= track[0].0 {
        return track[0].1;
    }
    for pair in track.windows(2) {
        let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
        if t <= t1 {
            let span = t1 - t0;
            let u = if span > 0.0 { (t - t0) / span } else { 1.0 };
            return v0 + (v1 - v0) * u;
        }
    }
    track[track.len() - 1].1
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Where the spirit is in its loop, 0..1 (the bench's `lp`).
pub fn spirit_loop_phase(age: f32) -> f32 {
    (age % GUARDIAN_SPIRIT_LOOP_SECS) / GUARDIAN_SPIRIT_LOOP_SECS
}

/// Whether the spirit is in its onset.
pub fn spirit_in_onset(age: f32) -> bool {
    age < GUARDIAN_SPIRIT_ONSET_SECS
}

/// The wings' full span at `age` (the bench's `w`): growing 2.2 -> 2.8 yd
/// across the onset, then breathing between the two on a 0.932 s sine.
pub fn spirit_wing_span(age: f32) -> f32 {
    let t = if spirit_in_onset(age) {
        (age / GUARDIAN_SPIRIT_ONSET_SECS).min(1.0)
    } else {
        0.5 + 0.5 * (age / GUARDIAN_SPIRIT_LOOP_SECS * PI).sin()
    };
    lerp(2.2, 2.8, t)
}

/// A panel's pose at `age` seconds into the spirit, or `None` while it draws
/// nothing (its layer cut, or its window closed). Transcribes the bench's
/// `customDraw` for the "Guardian Spirit" tab.
pub fn spirit_panel_pose(panel: SpiritPanel, age: f32) -> Option<PanelPose> {
    if !panel.kept() {
        return None;
    }
    let s = GUARDIAN_SPIRIT_SIZE_MUL;
    let op = GUARDIAN_SPIRIT_OPACITY_MUL;
    let ms = age * 1000.0;
    let onset = spirit_in_onset(age);
    let lp = spirit_loop_phase(age);
    let pose = match panel {
        SpiritPanel::DarkCircle => PanelPose {
            right: 0.0,
            up: 0.0,
            width: 1.6 * s,
            height: 1.6 * s,
            alpha: 0.25 * op,
        },
        SpiritPanel::HaloRing => PanelPose {
            right: 0.0,
            up: HALO_RING_LIFT,
            width: 2.0 * HALO_RING_RADIUS * s,
            height: HALO_RING_WIDTH,
            alpha: 0.9 * op,
        },
        SpiritPanel::RingGlow => {
            // The onset's flash of rings: one ring sweeping 0.28 -> 1.04 yd
            // through 533-1100 ms; otherwise one each loop.
            let rr = if onset && ms > 533.0 && ms < 1100.0 {
                keyed(&[(533.0, 0.28), (1100.0, 1.04)], ms)
            } else {
                0.28 + 0.76 * lp
            };
            PanelPose {
                right: 0.0,
                up: 0.0,
                width: rr * s,
                height: rr * s,
                alpha: if onset { 0.6 } else { 0.35 } * op,
            }
        }
        SpiritPanel::WingLeft | SpiritPanel::WingRight => {
            let w = spirit_wing_span(age);
            let fade_in = if onset { (ms / 400.0).min(1.0) } else { 1.0 };
            let side = if panel == SpiritPanel::WingLeft {
                -1.0
            } else {
                1.0
            };
            PanelPose {
                right: side * w * 0.32 * s,
                up: 0.05,
                width: w * 0.6 * s,
                height: 0.5 * s,
                alpha: 0.55 * fade_in * op,
            }
        }
        SpiritPanel::Band => PanelPose {
            right: 0.0,
            up: -0.1,
            width: lerp(0.96, 3.9, lp) * s,
            height: 0.8 * s,
            alpha: (0.25 + if onset { 0.2 } else { 0.0 }) * op,
        },
        SpiritPanel::FlareLeft | SpiritPanel::FlareRight => {
            if !(onset && ms > 540.0 && ms < 1100.0) {
                return None;
            }
            let side = if panel == SpiritPanel::FlareLeft {
                -1.0
            } else {
                1.0
            };
            PanelPose {
                right: side * 0.6 * s,
                up: 0.0,
                width: 1.4 * s,
                height: 0.14 * s,
                alpha: ((ms - 540.0) / 560.0 * PI).sin() * op,
            }
        }
    };
    Some(pose)
}

/// The bench's glow falloff (its `bbGlow` gradient): full at the centre, 0.45
/// halfway out, nothing at the rim, linear between. `r` is the distance from
/// the centre as a fraction of the panel's half-width.
pub fn panel_glow_alpha(r: f32) -> f32 {
    keyed(&[(0.0, 1.0), (0.5, 0.45), (1.0, 0.0)], r.max(0.0))
}

/// Generated glow sprite, pixels on a side.
const PANEL_GLOW_PX: u32 = 64;

/// The glow sprite: white, with [`panel_glow_alpha`] in the alpha channel.
fn panel_glow_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = PANEL_GLOW_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let a = panel_glow_alpha((dx * dx + dy * dy).sqrt());
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0).round() as u8;
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

/// Emissive gain of an additive panel over its colour: the client emitters'
/// convention (`hunter_shots.rs`'s `EMITTER_GLOW`).
const PANEL_GLOW: f32 = 2.5;

/// A panel's material at `alpha`: additive with an emissive glow, or (the
/// dark circle) a flat unlit colour that can darken.
fn panel_material(
    panel: SpiritPanel,
    alpha: f32,
    texture: Option<Handle<Image>>,
) -> StandardMaterial {
    let [r, g, b] = panel.rgb();
    let color = Color::srgb_u8(r, g, b);
    if panel.additive() {
        let c = color.to_linear();
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: texture.clone(),
            emissive: LinearRgba::rgb(
                c.red * PANEL_GLOW,
                c.green * PANEL_GLOW,
                c.blue * PANEL_GLOW,
            ),
            emissive_texture: texture,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    }
}

// ── The sparks ───────────────────────────────────────────────────────────────

/// P0, the gold sparks (`t_vfx_flar_blur_64`), as the bench plays them: 100 a
/// second in every direction at 2.8 yd/s from a 0.8 × 0.3 × 0.8 yd box,
/// living 0.5 s, white -> gold -> olive gold, fading out. A size key is the
/// sprite's RADIUS (the bench draws a key as a radius).
pub const GUARDIAN_SPIRIT_SPARK_EMITTER: ClientEmitter = ClientEmitter {
    name: "priest_guardianspirit/p0 sparks",
    sprite: EmitterSprite::Flare,
    additive: true,
    rate: 100.0,
    window: None,
    life: 0.5,
    speed: 2.8,
    gravity: 0.0,
    area: 0.0,
    column: Some((0.8, 0.3)),
    rise: false,
    color: [[255, 255, 255], [235, 210, 0], [183, 183, 0]],
    alpha: [1.0, 0.5, 0.0],
    size: ParticleSize::radii([0.06, 0.045, 0.03]),
    mid: 0.5,
    track: None,
};

// ── Components ───────────────────────────────────────────────────────────────

/// One bearer's spirit: a top-level rig at the spirit's centre, parent of its
/// panels and its sparks. Lives exactly as long as a Guardian Spirit is on a
/// living bearer.
#[derive(Component, Debug)]
pub struct GuardianSpiritRig {
    pub bearer: Entity,
    /// Seconds since the spirit appeared.
    pub age: f32,
    /// Sparks owed but not yet emitted.
    pub carry: f32,
    /// Sparks emitted, the scatter's seed.
    pub emitted: u32,
}

/// One panel of a [`GuardianSpiritRig`], with the material it fades.
#[derive(Component, Debug)]
pub struct GuardianSpiritPanel {
    pub panel: SpiritPanel,
    pub material: Handle<StandardMaterial>,
}

/// The "Saved" cue's gold body flash on `target`, `age` seconds in.
#[derive(Component, Debug)]
pub struct SavedFlash {
    pub target: Entity,
    pub age: f32,
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Whether an aura of this compound is Guardian Spirit's: the face and its
/// rider both carry `CompoundDebuff::GuardianSpirit`. The one predicate the
/// state and the lands-silently audit both ask.
pub fn wears_guardian_spirit(compound: Option<CompoundDebuff>) -> bool {
    compound == Some(CompoundDebuff::GuardianSpirit)
}

/// Where a bearer's spirit sits: [`GUARDIAN_SPIRIT_HEIGHT`] above its feet
/// (in a pet's smaller stature, should one ever bear it), and
/// [`GUARDIAN_SPIRIT_BEHIND`] behind its shoulders along its facing (a
/// combatant faces its local +Z).
pub fn guardian_spirit_anchor(bearer: &Transform, is_pet: bool) -> Vec3 {
    let at = dot_anchor(FEET_Y + GUARDIAN_SPIRIT_HEIGHT, bearer.translation, is_pet);
    let back = bearer.rotation * Vec3::NEG_Z;
    let back = Vec3::new(back.x, 0.0, back.z).normalize_or_zero();
    at + back * GUARDIAN_SPIRIT_BEHIND
}

// ── Systems: the state ───────────────────────────────────────────────────────

/// Raise a spirit over every living unit holding Guardian Spirit, and take it
/// down when the spirit ends (spent on a save, run out, purged) or its bearer
/// dies or is gone.
#[allow(clippy::type_complexity)]
pub fn update_guardian_spirit_states(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<SpiritAssets>>,
    bearers: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&Pet>,
    )>,
    rigs: Query<(Entity, &GuardianSpiritRig)>,
) {
    let holds = |entity: Entity| {
        bearers.get(entity).is_ok_and(|(_, c, _, auras, _)| {
            c.is_alive()
                && auras
                    .is_some_and(|a| a.auras.iter().any(|au| wears_guardian_spirit(au.compound)))
        })
    };
    for (rig, spirit) in rigs.iter() {
        if !holds(spirit.bearer) {
            commands.entity(rig).despawn();
        }
    }
    for (entity, _, transform, _, pet) in bearers.iter() {
        if !holds(entity) || rigs.iter().any(|(_, s)| s.bearer == entity) {
            continue;
        }
        let assets = assets.get_or_insert_with(|| SpiritAssets::build(&mut meshes, &mut images));
        let rig = commands
            .spawn((
                GuardianSpiritRig {
                    bearer: entity,
                    age: 0.0,
                    carry: 0.0,
                    emitted: 0,
                },
                Transform::from_translation(guardian_spirit_anchor(transform, pet.is_some())),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .id();
        for panel in SpiritPanel::ALL {
            let (mesh, texture) = match panel.sprite() {
                PanelSprite::Glow => (assets.quad.clone(), Some(assets.glow.clone())),
                PanelSprite::Disc => (assets.disc.clone(), None),
                PanelSprite::LevelRing => (assets.ring.clone(), None),
            };
            let material = materials.add(panel_material(panel, 0.0, texture));
            let child = commands
                .spawn((
                    GuardianSpiritPanel {
                        panel,
                        material: material.clone(),
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::ZERO),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(rig).add_child(child);
        }
    }
}

/// The meshes and the glow sprite every spirit draws with.
pub struct SpiritAssets {
    /// A unit quad facing +Z (the glow panels).
    quad: Handle<Mesh>,
    /// A unit-diameter disc facing +Z (the dark circle).
    disc: Handle<Mesh>,
    /// A ring of the gold ring's proportions at unit diameter, facing +Z.
    ring: Handle<Mesh>,
    glow: Handle<Image>,
}

impl SpiritAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        let half_width = HALO_RING_WIDTH / (4.0 * HALO_RING_RADIUS);
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            disc: meshes.add(Circle::new(0.5)),
            ring: meshes.add(Annulus::new(0.5 - half_width, 0.5 + half_width)),
            glow: images.add(panel_glow_texture()),
        }
    }
}

/// Follow each spirit's bearer, age it, pose and fade its panels to the
/// camera, retire its spent sparks and emit the sparks it owes.
///
/// The rig is unrotated, so a child's world frame is the rig's: a glow panel
/// turned to the camera's rotation faces it, and its `right` offset is laid
/// along the camera's right.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_guardian_spirits(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sparks: Local<Option<EmitterAssets>>,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<GuardianSpiritRig>,
            Without<GuardianSpiritPanel>,
        ),
    >,
    mut rigs: Query<
        (
            Entity,
            &mut GuardianSpiritRig,
            &mut Transform,
            Option<&Children>,
        ),
        Without<GuardianSpiritPanel>,
    >,
    bearers: Query<
        (&Transform, Option<&Pet>),
        (
            With<Combatant>,
            Without<GuardianSpiritRig>,
            Without<GuardianSpiritPanel>,
        ),
    >,
    mut panels: Query<
        (&GuardianSpiritPanel, &mut Transform),
        (Without<GuardianSpiritRig>, Without<Camera3d>),
    >,
    particles: Query<&ClientParticle>,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next().map_or(Quat::IDENTITY, |c| c.rotation);
    let cam_right = cam * Vec3::X;
    for (entity, mut rig, mut transform, children) in rigs.iter_mut() {
        let Ok((bearer, pet)) = bearers.get(rig.bearer) else {
            continue;
        };
        transform.translation = guardian_spirit_anchor(bearer, pet.is_some());
        let age = rig.age;
        for child in children.into_iter().flat_map(|c| c.iter()) {
            if let Ok((panel, mut pose_at)) = panels.get_mut(child) {
                let Some(material) = materials.get_mut(&panel.material) else {
                    continue;
                };
                match spirit_panel_pose(panel.panel, age) {
                    Some(pose) => {
                        pose_at.translation = cam_right * pose.right + Vec3::Y * pose.up;
                        (pose_at.rotation, pose_at.scale) = match panel.panel.sprite() {
                            PanelSprite::Glow | PanelSprite::Disc => {
                                (cam, Vec3::new(pose.width, pose.height, 1.0))
                            }
                            // Tipped back a quarter turn, the ring lies level.
                            PanelSprite::LevelRing => (
                                Quat::from_rotation_x(-PI / 2.0),
                                Vec3::new(pose.width, pose.width, 1.0),
                            ),
                        };
                        material.base_color.set_alpha(pose.alpha.clamp(0.0, 1.0));
                    }
                    None => {
                        pose_at.scale = Vec3::ZERO;
                        material.base_color.set_alpha(0.0);
                    }
                }
            } else if particles.get(child).is_ok_and(|p| p.age >= p.life) {
                commands.entity(child).despawn();
            }
        }
        rig.age += dt;

        if !GUARDIAN_SPIRIT_SPARKS {
            continue;
        }
        let e = &GUARDIAN_SPIRIT_SPARK_EMITTER;
        rig.carry += e.rate * dt;
        if rig.carry < 1.0 {
            continue;
        }
        let assets = sparks.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        let quad = assets.quad();
        let palette = assets.palette(&mut materials, e);
        while rig.carry >= 1.0 {
            rig.carry -= 1.0;
            let seed = rig
                .bearer
                .index()
                .wrapping_mul(0x9E37_79B9)
                .wrapping_add(rig.emitted.wrapping_mul(0x85EB_CA6B));
            rig.emitted = rig.emitted.wrapping_add(1);
            let at = Vec3::NEG_Y * GUARDIAN_SPIRIT_SPARK_DROP;
            let spark = spawn_client_particle(&mut commands, &quad, e, &palette, at, seed, true);
            commands.entity(entity).add_child(spark);
        }
    }
}

/// Turn every live spark to face the camera, after the client particles have
/// aged and moved.
#[allow(clippy::type_complexity)]
pub fn billboard_guardian_spirit_sparks(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<GuardianSpiritRig>,
            Without<ClientParticle>,
        ),
    >,
    rigs: Query<(&Transform, &Children), With<GuardianSpiritRig>>,
    mut particles: Query<
        (&ClientParticle, &mut Transform),
        (Without<Camera3d>, Without<GuardianSpiritRig>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        for child in children.iter() {
            if let Ok((particle, mut part)) = particles.get_mut(child) {
                part.rotation = particle_rotation(particle, rig.rotation, cam.rotation);
            }
        }
    }
}

// ── The save, and the shared "Saved" cue ─────────────────────────────────────

/// The "Saved" text's offset ON SCREEN from the save's text anchor: one
/// floating line up (`fct::LINE_STEP` points), over the heal's number. The
/// bench lifts it above the number at its own 7 yd framing; a world offset
/// that reads there shrinks to an overlap at the match camera's 60 yd, so the
/// stack is held in points instead.
pub fn saved_text_offset() -> FctScreenOffset {
    FctScreenOffset(crate::ui::theme::fct::LINE_ABOVE)
}
/// The word, and how long it floats (a heal number's 1.5 s).
pub const SAVED_TEXT: &str = "Saved";
pub const SAVED_TEXT_SECS: f32 = 1.5;
/// The gold body flash: its life, its peak and when it peaks, its size and
/// height on the body, and its colour (the bench's `(255, 215, 90)`).
pub const SAVED_FLASH_SECS: f32 = 0.6;
pub const SAVED_FLASH_PEAK_ALPHA: f32 = 0.7;
pub const SAVED_FLASH_PEAK_SECS: f32 = 0.08;
pub const SAVED_FLASH_SIZE: Vec2 = Vec2::new(2.2, 3.0);
pub const SAVED_FLASH_HEIGHT: f32 = 1.3;
pub const SAVED_FLASH_RGB: [u8; 3] = [255, 215, 90];

/// The flash's alpha at `age`: up to its peak at 80 ms, down to nothing at
/// 0.6 s, linear between.
pub fn saved_flash_alpha(age: f32) -> f32 {
    if !(0.0..SAVED_FLASH_SECS).contains(&age) {
        return 0.0;
    }
    keyed(
        &[
            (0.0, 0.0),
            (SAVED_FLASH_PEAK_SECS, SAVED_FLASH_PEAK_ALPHA),
            (SAVED_FLASH_SECS, 0.0),
        ],
        age,
    )
}

/// The authored "Saved" cue on `holder`, whatever saved it: gold "Saved"
/// (`fct::INVULNERABLE`, at number size) one line above `text_anchor`
/// ([`saved_text_offset`]), and the gold body flash. Guardian Spirit's save and
/// the Rogue's Cheat Death both land it, through
/// [`consume_killing_blow_signals`].
pub fn spawn_saved_cue(commands: &mut Commands, holder: Entity, text_anchor: Vec3) {
    commands.spawn((
        FloatingCombatText {
            world_position: text_anchor,
            text: SAVED_TEXT.to_string(),
            color: crate::ui::theme::fct::INVULNERABLE,
            lifetime: SAVED_TEXT_SECS,
            vertical_offset: 0.0,
            is_crit: false,
        },
        saved_text_offset(),
        PlayMatchEntity,
    ));
    commands.spawn((
        SavedFlash {
            target: holder,
            age: 0.0,
        },
        PlayMatchEntity,
    ));
}

/// Play every prevented killing blow: the client's heal landing (kit 232,
/// Heal's rising gold stream) and the heal's number when the save landed a
/// heal, the shared "Saved" cue for every save its holder lived through, and
/// for Cheat Death its skull and ghost-fire (`cheat_death.rs`).
/// A save whose holder another blow of the same frame killed plays nothing:
/// there is no one saved to show it on. Consumes the marker.
///
/// FixedUpdate after the combat phases, as every marker consumer runs: the
/// sim can tick several times a rendered frame.
pub fn consume_killing_blow_signals(
    mut commands: Commands,
    markers: Query<(Entity, &KillingBlowPrevented)>,
    holders: Query<(&Transform, &Combatant)>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    for (marker, saved) in markers.iter() {
        commands.entity(marker).despawn();
        let Ok((holder, combatant)) = holders.get(saved.holder) else {
            continue;
        };
        if !combatant.is_alive() {
            continue;
        }
        let (dx, dy) = fct_states
            .get_mut(saved.holder)
            .map_or((0.0, 0.0), |mut state| get_next_fct_offset(&mut state));
        let anchor = holder.translation + Vec3::new(dx, FCT_HEIGHT + dy, 0.0);
        if let Some(healed) = saved.heal {
            commands.spawn((
                FloatingCombatText {
                    world_position: anchor,
                    text: format!("+{healed:.0}"),
                    color: crate::ui::theme::fct::HEAL,
                    lifetime: SAVED_TEXT_SECS,
                    vertical_offset: dy,
                    is_crit: false,
                },
                PlayMatchEntity,
            ));
            commands.spawn((
                HealImpact {
                    target: saved.holder,
                    kind: HealImpactKind::HealStream,
                    age: 0.0,
                },
                PlayMatchEntity,
            ));
        }
        if saved_cue_plays(&saved.by) {
            spawn_saved_cue(&mut commands, saved.holder, anchor);
        }
        if plays_cheat_death(&saved.by) {
            spawn_cheat_death(&mut commands, saved.holder);
        }
    }
}

/// Whether a save, by what saved it, lands the shared "Saved" cue: each
/// bench's own knob (Cheat Death's, or Guardian Spirit's for every other).
pub fn saved_cue_plays(saved_by: &str) -> bool {
    if plays_cheat_death(saved_by) {
        CHEAT_DEATH_SAVED_CUE
    } else {
        GUARDIAN_SPIRIT_SAVED_CUE
    }
}

/// Give each new flash its panel: a camera-facing glow quad, additive gold.
pub fn spawn_saved_flashes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<Image>)>>,
    flashes: Query<Entity, (Added<SavedFlash>, Without<Mesh3d>)>,
) {
    for entity in flashes.iter() {
        let (quad, glow) = assets
            .get_or_insert_with(|| {
                (
                    meshes.add(Rectangle::new(1.0, 1.0)),
                    images.add(panel_glow_texture()),
                )
            })
            .clone();
        let [r, g, b] = SAVED_FLASH_RGB;
        let color = Color::srgb_u8(r, g, b);
        let c = color.to_linear();
        let material = materials.add(StandardMaterial {
            base_color: color.with_alpha(0.0),
            base_color_texture: Some(glow.clone()),
            emissive: LinearRgba::rgb(
                c.red * PANEL_GLOW,
                c.green * PANEL_GLOW,
                c.blue * PANEL_GLOW,
            ),
            emissive_texture: Some(glow),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        commands.entity(entity).try_insert((
            Mesh3d(quad),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::ZERO),
            Visibility::default(),
            NotShadowCaster,
        ));
    }
}

/// Age each flash on its holder's body, facing the camera, and retire it at
/// the end of its life (or when its holder is gone).
#[allow(clippy::type_complexity)]
pub fn animate_saved_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<&Transform, (With<Camera3d>, Without<SavedFlash>)>,
    holders: Query<(&Transform, Option<&Pet>), (With<Combatant>, Without<SavedFlash>)>,
    mut flashes: Query<(
        Entity,
        &mut SavedFlash,
        &mut Transform,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next().map_or(Quat::IDENTITY, |c| c.rotation);
    for (entity, mut flash, mut transform, material) in flashes.iter_mut() {
        let Ok((holder, pet)) = holders.get(flash.target) else {
            commands.entity(entity).despawn();
            continue;
        };
        if flash.age >= SAVED_FLASH_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation = dot_anchor(
            FEET_Y + SAVED_FLASH_HEIGHT,
            holder.translation,
            pet.is_some(),
        );
        transform.rotation = cam;
        transform.scale = SAVED_FLASH_SIZE.extend(1.0);
        if let Some(m) = material.and_then(|m| materials.get_mut(&m.0)) {
            m.base_color.set_alpha(saved_flash_alpha(flash.age));
        }
        flash.age += dt;
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
        assert!(close(panel_glow_alpha(0.0), 1.0));
        assert!(close(panel_glow_alpha(0.25), 0.725));
        assert!(close(panel_glow_alpha(0.5), 0.45));
        assert!(close(panel_glow_alpha(0.75), 0.225));
        assert!(close(panel_glow_alpha(1.0), 0.0));
        assert!(close(panel_glow_alpha(1.4), 0.0));
    }

    #[test]
    fn the_flash_peaks_at_eighty_milliseconds_and_is_gone_at_six_tenths() {
        assert!(close(saved_flash_alpha(0.0), 0.0));
        assert!(close(saved_flash_alpha(0.04), 0.35));
        assert!(close(saved_flash_alpha(0.08), 0.7));
        assert!(close(saved_flash_alpha(0.34), 0.35));
        assert!(close(saved_flash_alpha(0.6), 0.0));
        assert!(close(saved_flash_alpha(0.7), 0.0));
    }

    #[test]
    fn only_the_guardian_spirit_compound_wears_the_spirit() {
        assert!(wears_guardian_spirit(Some(CompoundDebuff::GuardianSpirit)));
        assert!(!wears_guardian_spirit(None));
        for other in CompoundDebuff::ALL {
            if other != CompoundDebuff::GuardianSpirit {
                assert!(!wears_guardian_spirit(Some(other)), "{other:?}");
            }
        }
    }
}
