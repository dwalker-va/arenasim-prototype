use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use crate::states::play_match::components::*;

// ==============================================================================
// The finisher hand flash — a red glow riding the Rogue's striking hand
// ==============================================================================
//
// From the client data (`docs/design/2026-10-17-rogue-talent-client-data.md`
// §1): Rupture's cast kit 394 attaches `bloodlust_state_hand.m2` to both
// hands — a red glow quad (`red_glow3`, about 0.9 yd across, additive), P0
// shockwave rings yellow (255,216,0) to red (250,5,0) growing 0.22 to 0.41 yd,
// and P1 red cloud puffs shrinking 0.28 to 0.03 yd. Expose Armor carries the
// same kit (AS-366 reuses this), and so does Warrior Execute.
//
// As the signed-off AS-336 bench draws it (version 1792302646-d16f,
// `strikeFlash`, committed as `docs/design/benches/2026-10-17-rogue-talent-
// bench.html`), the flash RIDES THE STRIKING HAND through the backhand stroke
// (`SwingStyle::Backhand`) rather than sitting on both hands: it builds from
// the windup to full at contact and is out by 0.9 s, the stroke's end. The off
// hand gets a smaller, dimmer glow of its own.
//
// Every piece is placed from the weapon sockets' LOCAL transforms composed down
// the unit -> body -> socket chain, after the swing and the body lean have
// written them this frame — so the flash sits on the hand the frame it is
// drawn, never a frame behind it, as reading the sockets' propagated
// `GlobalTransform` in `Update` would.
//
// Graphical-only: spawned by the instant-ability router, scattered by nothing
// (the puffs' phases are fixed), writing no sim state, registered in
// `states/mod.rs` only — headless stays byte-identical.

// ── The bench's numbers ──────────────────────────────────────────────────────

/// How long the flash plays: the backhand stroke's length.
pub const FINISHER_FLASH_SECS: f32 = 0.9;
/// The flash's strength over its life, `(seconds, weight)`: builds through the
/// windup, full at the stroke's contact (0.33 s), out by 0.9 s.
pub const FINISHER_FLASH_KEYS: [(f32, f32); 5] = [
    (0.0, 0.0),
    (0.12, 0.8),
    (0.33, 1.0),
    (0.65, 0.6),
    (0.9, 0.0),
];

/// `red_glow3` on the striking hand: a camera-facing soft glow.
pub const FINISHER_GLOW_RGB: [u8; 3] = [255, 30, 10];
pub const FINISHER_GLOW_DIAMETER: f32 = 0.9;
pub const FINISHER_GLOW_ALPHA: f32 = 0.55;
/// The off hand's smaller, dimmer glow.
pub const FINISHER_OFF_GLOW_DIAMETER: f32 = 0.6;
pub const FINISHER_OFF_GLOW_ALPHA: f32 = 0.35;

/// The P0 ring, lying flat round the striking hand: yellow turning red by
/// `FINISHER_RING_TURN_SECS`, and growing across the whole flash.
pub const FINISHER_RING_FROM_RGB: [u8; 3] = [255, 216, 0];
pub const FINISHER_RING_TO_RGB: [u8; 3] = [250, 5, 0];
pub const FINISHER_RING_TURN_SECS: f32 = 0.45;
/// The ring's diameter at the start and at the end of the flash.
pub const FINISHER_RING_DIAMETER: (f32, f32) = (0.22, 0.41);
pub const FINISHER_RING_ALPHA: f32 = 0.9;
/// The ring's inner edge as a fraction of its outer: a thin line, as the
/// bench strokes it.
pub const FINISHER_RING_INNER: f32 = 0.85;

/// The P1 puffs: a fixed set cycling on the hand, each drifting back from the
/// victim and up as it shrinks and fades, alpha-blended as the bench paints
/// them.
pub const FINISHER_PUFFS: u32 = 6;
pub const FINISHER_PUFF_CYCLE_SECS: f32 = 0.7;
pub const FINISHER_PUFF_RGB: [u8; 3] = [255, 40, 0];
/// A puff's diameter at birth; it shrinks to nothing over its cycle.
pub const FINISHER_PUFF_DIAMETER: f32 = 0.28;
pub const FINISHER_PUFF_DRIFT_BACK: f32 = 0.25;
pub const FINISHER_PUFF_RISE: f32 = 0.15;

/// The generated sprites, pixels on a side.
const SPRITE_PX: u32 = 64;
/// The bench's soft glow (`bbGlow`): alpha 1 at the centre, 0.45 halfway out,
/// 0 at the rim.
pub const FINISHER_GLOW_PROFILE: [(f32, f32); 3] = [(0.0, 1.0), (0.5, 0.45), (1.0, 0.0)];
/// The bench's cloud puff (`sprite('cloud')`): 0.9 at the centre, 0.6 at 0.45
/// of the radius, 0 at the rim.
pub const FINISHER_CLOUD_PROFILE: [(f32, f32); 3] = [(0.0, 0.9), (0.45, 0.6), (1.0, 0.0)];

// ── Components ───────────────────────────────────────────────────────────────

/// One finisher hand flash, riding `caster`'s hands. The rig stays at the world
/// origin; its pieces are its children, placed in world space each frame.
#[derive(Component)]
pub struct FinisherHandFlash {
    pub caster: Entity,
    pub age: f32,
    /// Unit vector on the ground plane pointing AWAY from the victim, which
    /// the puffs drift along.
    pub back: Vec3,
}

/// What one piece of a [`FinisherHandFlash`] draws.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinisherFlashPiece {
    /// `red_glow3` on the striking hand.
    Glow,
    /// The P0 ring round the striking hand.
    Ring,
    /// One of the P1 puffs.
    Puff(u32),
    /// The off hand's glow.
    OffGlow,
}

impl FinisherFlashPiece {
    /// Every piece one flash spawns, in spawn order.
    pub fn all() -> Vec<FinisherFlashPiece> {
        let mut pieces = vec![FinisherFlashPiece::Glow, FinisherFlashPiece::Ring];
        pieces.extend((0..FINISHER_PUFFS).map(FinisherFlashPiece::Puff));
        pieces.push(FinisherFlashPiece::OffGlow);
        pieces
    }

    /// Whether the piece faces the camera (the ring lies flat instead).
    pub fn faces_camera(self) -> bool {
        self != FinisherFlashPiece::Ring
    }

    /// How the piece composites, as the bench draws it: the glows and the
    /// ring add light (`lighter`); the puffs are painted OVER the scene
    /// (`source-over`), so they read as red smoke rather than more glow.
    pub fn alpha_mode(self) -> AlphaMode {
        match self {
            FinisherFlashPiece::Puff(_) => AlphaMode::Blend,
            _ => AlphaMode::Add,
        }
    }
}

/// Where and how one piece draws at one moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FinisherPieceState {
    pub at: Vec3,
    /// Full width in yards (the ring's outer diameter).
    pub diameter: f32,
    pub rgb: [f32; 3],
    pub alpha: f32,
}

// ── The pure model ───────────────────────────────────────────────────────────

/// The flash's weight `age` seconds in: [`FINISHER_FLASH_KEYS`], interpolated.
pub fn finisher_flash_weight(age: f32) -> f32 {
    let keys = FINISHER_FLASH_KEYS;
    if age <= keys[0].0 {
        return keys[0].1;
    }
    for pair in keys.windows(2) {
        let ((t0, w0), (t1, w1)) = (pair[0], pair[1]);
        if age <= t1 {
            return w0 + (w1 - w0) * (age - t0) / (t1 - t0);
        }
    }
    keys[keys.len() - 1].1
}

fn rgb(c: [u8; 3]) -> [f32; 3] {
    [
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
    ]
}

/// How far through its cycle puff `index` is at `age`, 0..1.
pub fn finisher_puff_phase(index: u32, age: f32) -> f32 {
    (age / FINISHER_PUFF_CYCLE_SECS + index as f32 / FINISHER_PUFFS as f32).fract()
}

/// One piece's state `age` seconds in, with the striking hand at `hand`, the
/// off hand at `off` (if the caster holds a weapon there) and `back` pointing
/// away from the victim. `None` for a piece with nothing to draw on.
pub fn finisher_piece_state(
    piece: FinisherFlashPiece,
    age: f32,
    hand: Vec3,
    off: Option<Vec3>,
    back: Vec3,
) -> Option<FinisherPieceState> {
    let weight = finisher_flash_weight(age);
    let life = (age / FINISHER_FLASH_SECS).clamp(0.0, 1.0);
    Some(match piece {
        FinisherFlashPiece::Glow => FinisherPieceState {
            at: hand,
            diameter: FINISHER_GLOW_DIAMETER,
            rgb: rgb(FINISHER_GLOW_RGB),
            alpha: FINISHER_GLOW_ALPHA * weight,
        },
        FinisherFlashPiece::Ring => {
            let turn = (age / FINISHER_RING_TURN_SECS).clamp(0.0, 1.0);
            let (from, to) = (rgb(FINISHER_RING_FROM_RGB), rgb(FINISHER_RING_TO_RGB));
            let (d0, d1) = FINISHER_RING_DIAMETER;
            FinisherPieceState {
                at: hand,
                diameter: d0 + (d1 - d0) * life,
                rgb: [0, 1, 2].map(|c| from[c] + (to[c] - from[c]) * turn),
                alpha: FINISHER_RING_ALPHA * weight,
            }
        }
        FinisherFlashPiece::Puff(index) => {
            let phase = finisher_puff_phase(index, age);
            FinisherPieceState {
                at: hand
                    + back * FINISHER_PUFF_DRIFT_BACK * phase
                    + Vec3::Y * FINISHER_PUFF_RISE * phase,
                diameter: FINISHER_PUFF_DIAMETER * (1.0 - phase),
                rgb: rgb(FINISHER_PUFF_RGB),
                alpha: weight * (1.0 - phase),
            }
        }
        FinisherFlashPiece::OffGlow => FinisherPieceState {
            at: off?,
            diameter: FINISHER_OFF_GLOW_DIAMETER,
            rgb: rgb(FINISHER_GLOW_RGB),
            alpha: FINISHER_OFF_GLOW_ALPHA * weight,
        },
    })
}

/// A white sprite whose alpha falls off along `profile` — `(radius fraction,
/// alpha)` keys, linearly interpolated, zero past the last.
pub fn finisher_sprite_texture(profile: [(f32, f32); 3]) -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = SPRITE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let a = profile_alpha(profile, (dx * dx + dy * dy).sqrt());
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

/// `profile`'s alpha at radius fraction `r`.
pub fn profile_alpha(profile: [(f32, f32); 3], r: f32) -> f32 {
    if r <= profile[0].0 {
        return profile[0].1;
    }
    for pair in profile.windows(2) {
        let ((r0, a0), (r1, a1)) = (pair[0], pair[1]);
        if r <= r1 {
            return a0 + (a1 - a0) * (r - r0) / (r1 - r0);
        }
    }
    0.0
}

// ── Spawn ────────────────────────────────────────────────────────────────────

/// The meshes and sprites every flash draws with, built once.
pub struct FinisherFlashAssets {
    quad: Handle<Mesh>,
    ring: Handle<Mesh>,
    glow: Handle<Image>,
    cloud: Handle<Image>,
}

impl FinisherFlashAssets {
    pub fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            ring: meshes.add(Annulus::new(FINISHER_RING_INNER, 1.0).mesh().resolution(40)),
            glow: images.add(finisher_sprite_texture(FINISHER_GLOW_PROFILE)),
            cloud: images.add(finisher_sprite_texture(FINISHER_CLOUD_PROFILE)),
        }
    }
}

/// Spawn a finisher hand flash on `caster`, struck toward `toward` (the
/// victim's position, or `None` to drift the puffs straight back from the
/// caster's facing). Called by the instant-ability router as the stroke
/// starts.
pub fn spawn_finisher_hand_flash(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    assets: &mut Option<FinisherFlashAssets>,
    caster: Entity,
    caster_pos: Vec3,
    toward: Option<Vec3>,
) {
    let assets = assets.get_or_insert_with(|| FinisherFlashAssets::build(meshes, images));
    let back = toward
        .map(|t| (caster_pos - t).with_y(0.0).normalize_or_zero())
        .unwrap_or(Vec3::NEG_Z);
    let rig = commands
        .spawn((
            FinisherHandFlash {
                caster,
                age: 0.0,
                back,
            },
            Transform::default(),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .id();
    // Per-piece materials: every alpha is animated absolutely each frame.
    let parts: Vec<Entity> = FinisherFlashPiece::all()
        .into_iter()
        .map(|piece| {
            let (mesh, texture, color) = match piece {
                FinisherFlashPiece::Ring => (assets.ring.clone(), None, FINISHER_RING_FROM_RGB),
                FinisherFlashPiece::Puff(_) => (
                    assets.quad.clone(),
                    Some(assets.cloud.clone()),
                    FINISHER_PUFF_RGB,
                ),
                FinisherFlashPiece::Glow | FinisherFlashPiece::OffGlow => (
                    assets.quad.clone(),
                    Some(assets.glow.clone()),
                    FINISHER_GLOW_RGB,
                ),
            };
            let [r, g, b] = rgb(color);
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(r, g, b, 0.0),
                base_color_texture: texture,
                // The kit adds colour x sprite x weight and nothing else.
                unlit: true,
                alpha_mode: piece.alpha_mode(),
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            commands
                .spawn((
                    piece,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    // Hidden until the first placement puts it on a hand.
                    Transform::from_scale(Vec3::splat(1e-3)),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    commands.entity(rig).add_children(&parts);
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// A socket's world position, composed from the unit, its body and the socket's
/// own local transform — this frame's, not last frame's propagated one.
pub(super) fn socket_world(unit: &Transform, body: &Transform, socket: &Transform) -> Vec3 {
    unit.mul_transform(*body).mul_transform(*socket).translation
}

/// Update (graphical-only), after the swing and the body lean: age every flash,
/// put its pieces on its caster's hands, and retire it at the end of its life
/// or the moment its caster dies or is gone.
#[allow(clippy::type_complexity)]
pub fn update_finisher_hand_flashes(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&Transform, (With<Camera3d>, Without<FinisherFlashPiece>)>,
    mut rigs: Query<(Entity, &mut FinisherHandFlash, &Children)>,
    units: Query<
        (&Combatant, &Transform),
        (
            Without<FinisherFlashPiece>,
            Without<WeaponSocket>,
            Without<VisualBody>,
        ),
    >,
    bodies: Query<&Transform, (With<VisualBody>, Without<FinisherFlashPiece>)>,
    sockets: Query<(&WeaponSocket, &Transform, &ChildOf), Without<FinisherFlashPiece>>,
    mut pieces: Query<
        (
            &FinisherFlashPiece,
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<Camera3d>,
            Without<WeaponSocket>,
            Without<VisualBody>,
            Without<Combatant>,
        ),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let camera = camera.iter().next().map(|c| c.rotation);
    for (rig, mut flash, children) in rigs.iter_mut() {
        flash.age += dt;
        let Ok((caster, unit)) = units.get(flash.caster) else {
            commands.entity(rig).despawn();
            continue;
        };
        if flash.age >= FINISHER_FLASH_SECS || !caster.is_alive() {
            commands.entity(rig).despawn();
            continue;
        }
        let hand_of = |hand: WeaponHand| {
            sockets
                .iter()
                .find(|(s, _, _)| s.owner == flash.caster && s.hand == hand)
                .and_then(|(_, local, parent)| {
                    bodies
                        .get(parent.parent())
                        .ok()
                        .map(|body| socket_world(unit, body, local))
                })
        };
        let main = hand_of(WeaponHand::Main);
        let off = hand_of(WeaponHand::Off);
        for child in children.iter() {
            let Ok((piece, mut transform, mut visibility, material)) = pieces.get_mut(child) else {
                continue;
            };
            let state = main
                .and_then(|hand| finisher_piece_state(*piece, flash.age, hand, off, flash.back));
            let Some(state) = state else {
                *visibility = Visibility::Hidden;
                continue;
            };
            *visibility = Visibility::Inherited;
            transform.translation = state.at;
            if piece.faces_camera() {
                transform.scale = Vec3::splat(state.diameter.max(1e-4));
                if let Some(camera) = camera {
                    transform.rotation = camera;
                }
            } else {
                // The annulus is built at a unit OUTER radius in the XY plane;
                // tipped a quarter turn it lies flat round the hand.
                transform.scale = Vec3::splat((state.diameter * 0.5).max(1e-4));
                transform.rotation = Quat::from_rotation_x(-FRAC_PI_2);
            }
            if let Some(m) = materials.get_mut(&material.0) {
                let [r, g, b] = state.rgb;
                m.base_color = Color::srgba(r, g, b, state.alpha.clamp(0.0, 1.0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flash_builds_to_full_at_contact_and_is_out_by_the_strokes_end() {
        assert_eq!(finisher_flash_weight(0.0), 0.0);
        assert!((finisher_flash_weight(0.12) - 0.8).abs() < 1e-6);
        assert!((finisher_flash_weight(0.33) - 1.0).abs() < 1e-6);
        assert!((finisher_flash_weight(0.65) - 0.6).abs() < 1e-6);
        assert_eq!(finisher_flash_weight(0.9), 0.0);
        assert_eq!(finisher_flash_weight(5.0), 0.0);
        // The peak is the contact.
        let peak = (0..=90)
            .map(|i| (i as f32 * 0.01, finisher_flash_weight(i as f32 * 0.01)))
            .fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        assert!((peak.0 - 0.33).abs() < 1e-3, "{peak:?}");
    }

    #[test]
    fn the_ring_turns_yellow_to_red_and_grows() {
        let at = |age| {
            finisher_piece_state(FinisherFlashPiece::Ring, age, Vec3::ZERO, None, Vec3::Z).unwrap()
        };
        let start = at(0.0);
        assert_eq!(start.rgb, rgb([255, 216, 0]));
        assert!((start.diameter - 0.22).abs() < 1e-6);
        let red = rgb([250, 5, 0]);
        for age in [0.45, 0.8] {
            let c = at(age).rgb;
            assert!((0..3).all(|i| (c[i] - red[i]).abs() < 1e-6), "{age}: {c:?}");
        }
        assert!((at(0.9).diameter - 0.41).abs() < 1e-6);
    }

    #[test]
    fn a_puff_drifts_back_and_up_as_it_shrinks() {
        let back = Vec3::NEG_X;
        // Puff 0 is at phase 0.5 a third of the way through its cycle.
        let age = 0.5 * FINISHER_PUFF_CYCLE_SECS;
        let s =
            finisher_piece_state(FinisherFlashPiece::Puff(0), age, Vec3::ZERO, None, back).unwrap();
        assert!((s.at.x + 0.125).abs() < 1e-5, "{:?}", s.at);
        assert!((s.at.y - 0.075).abs() < 1e-5, "{:?}", s.at);
        assert!((s.diameter - 0.14).abs() < 1e-5);
        assert!((s.alpha - finisher_flash_weight(age) * 0.5).abs() < 1e-5);
    }

    #[test]
    fn the_puffs_are_spread_evenly_through_the_cycle() {
        let phases: Vec<f32> = (0..FINISHER_PUFFS)
            .map(|i| finisher_puff_phase(i, 0.0))
            .collect();
        for (i, p) in phases.iter().enumerate() {
            assert!((p - i as f32 / 6.0).abs() < 1e-6, "{phases:?}");
        }
    }

    #[test]
    fn no_off_hand_draws_no_off_glow() {
        assert!(
            finisher_piece_state(FinisherFlashPiece::OffGlow, 0.3, Vec3::ZERO, None, Vec3::Z)
                .is_none()
        );
        let s = finisher_piece_state(
            FinisherFlashPiece::OffGlow,
            0.33,
            Vec3::ZERO,
            Some(Vec3::X),
            Vec3::Z,
        )
        .unwrap();
        assert_eq!(s.at, Vec3::X);
        assert!((s.diameter - 0.6).abs() < 1e-6);
        assert!((s.alpha - 0.35).abs() < 1e-6);
    }

    #[test]
    fn the_sprites_fall_off_as_the_bench_draws_them() {
        assert_eq!(profile_alpha(FINISHER_GLOW_PROFILE, 0.0), 1.0);
        assert!((profile_alpha(FINISHER_GLOW_PROFILE, 0.5) - 0.45).abs() < 1e-6);
        assert!((profile_alpha(FINISHER_GLOW_PROFILE, 0.75) - 0.225).abs() < 1e-6);
        assert_eq!(profile_alpha(FINISHER_GLOW_PROFILE, 1.2), 0.0);
        assert!((profile_alpha(FINISHER_CLOUD_PROFILE, 0.0) - 0.9).abs() < 1e-6);
        assert!((profile_alpha(FINISHER_CLOUD_PROFILE, 0.45) - 0.6).abs() < 1e-6);
        // The texture carries the profile: its centre and its corner.
        let image = finisher_sprite_texture(FINISHER_GLOW_PROFILE);
        let data = image.data.as_ref().unwrap();
        let px = SPRITE_PX as usize;
        let centre = ((px / 2) * px + px / 2) * 4;
        assert!(data[centre + 3] > 240, "{}", data[centre + 3]);
        assert_eq!(data[3], 0, "the corner is clear");
        assert_eq!(&data[centre..centre + 3], &[255, 255, 255]);
    }
}
