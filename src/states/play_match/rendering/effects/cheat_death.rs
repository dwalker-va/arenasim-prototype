use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::druid_heals::{body_point, keyed, keyed_color};
use super::guardian_spirit::panel_glow_alpha;
use super::heal_impact::COMBATANT_BODY_RADIUS;
use super::hunter_shots::{
    draw, particle_rotation, ramp3, ramp_color, ClientEmitter, EmitterSprite, PALETTE_STEPS,
};
use crate::states::play_match::combat_core::CHEAT_DEATH_NAME;
use crate::states::play_match::components::*;

// ==============================================================================
// Cheat Death — the skull over the head, and the lavender ghost-fire
// ==============================================================================
//
// From the client data, `docs/design/2026-10-17-rogue-talent-client-data.md`
// §6 (AS-336). The 3 s "Cheating Death" buff (TBC 45182) plays
// `cheat_death.m2` (kit 9543) at the Rogue's feet: a 1.9 s onset, then a loop
// that holds until the buff ends.
//
// Played as the signed-off bench plays it (AS-336 Rogue Talent Visuals Bench,
// https://claude.ai/artifact/9oFjSgkeqQ4t5u4VNtyGMz, version
// 1792302646-d16f, "Cheat Death", every layer kept and every multiplier at
// 1.0). Its source is committed as
// `docs/design/benches/2026-10-17-rogue-talent-bench.html`; the version's id
// is a commit time, and the build follows `f559b28f` (committed 4 s after
// it), whose "cheat" tab is byte-identical to the one before it. The pieces
// are its `customDraw`, ported, and the particles its two emitter records:
//
//   skull    -> a lavender skull 1.3 yd across, 2.9 yd above the feet, in
//               by 0.12 s and gone by 0.867 s: an authored shape (there is no
//               `skull.blp`), its dark sockets and nose, and two white eye
//               glows;
//   rings    -> over the head (3.04 yd), 0.54-1.4 s: two thin level rings
//               widening, a shockwave glow 0.56 -> 1.67 yd changing slate ->
//               white -> mauve, and a white flare;
//   streaks  -> 300 a second at 0.567-0.833 s, out of the skull at 8.3 yd/s
//               under drag, white -> lavender: each a round `glow` with a
//               tail behind it that shortens as it slows;
//   smoke    -> indigo smoke (the bench's `smoke` sprite) at 3.3 yd, sinking,
//               emitted to 1.1 s;
//   column   -> a tall light column through the body, peaking at 0.767 s;
//   fire     -> the 3 s state: eighteen lavender flames rising round the
//               body, laid over it (source-over, not added), in from 0.8 s,
//               full at 1.367 s, out over 0.6 s once the buff ends.
//
// Everything but the sockets and the flames adds light, as the bench's
// `lighter` composite does: colour × sprite × alpha, unlit.
//
// **Departures from the bench**, each forced by the 3D world:
//
// - The flames hug the body capsule's outline 0.05 yd out
//   ([`flame_ring_radius`]) instead of the bench's 0.4 -> 0.26 yd: the
//   capsule is 0.5 yd across its radius and would bury them, where the bench
//   paints them over the body in 2D. The far side's flames are hidden by the
//   body.
// - A tail is a camera-facing quad without the canvas stroke's round caps
//   (a cap's radius is at most 0.02 yd once the streak settles).
// - The thin rings keep the bench's stroke in yards (3 and 4 px at its 8 yd
//   framing), so they are thinner than a pixel at the match camera.
//
// **What starts it.** The save's own marker, `KillingBlowPrevented`, read by
// `consume_killing_blow_signals` (`guardian_spirit.rs`), which also lands the
// shared "Saved" cue (AS-354) on every save: [`plays_cheat_death`] names the
// saves this rig plays for. The gold flash comes first and the skull peaks
// just after; the bench plays them together and the user approved the
// overlap. The buff's application is owned here
// (`AuraApplyOwner::CheatDeath`), so the generic aura band stays off it.
//
// **Nothing on a corpse.** A save whose Rogue another blow of the same frame
// kills raises no rig (the consumer's own check), and a rig is taken down,
// with every piece and particle under it, the frame its Rogue dies.
//
// Graphical-only: reads `ActiveAuras` and the marker, writes no sim state,
// draws no `game_rng` (the particles' scatter is the shared deterministic
// hash), and is registered in `states/mod.rs` only — headless stays
// byte-identical.

// ── The bench's knobs (signed off, version 1792302646-d16f) ─────────────────

pub const CHEAT_DEATH_SAVED_CUE: bool = true;
/// 1.3 yd, (0.58, 0.47, 1.0), out by 0.867 s.
pub const CHEAT_DEATH_SKULL: bool = true;
pub const CHEAT_DEATH_RINGS: bool = true;
/// 300/s, 0.567-0.833 s, 8.3 yd/s.
pub const CHEAT_DEATH_STREAKS: bool = true;
pub const CHEAT_DEATH_SMOKE: bool = true;
pub const CHEAT_DEATH_LIGHT_COLUMN: bool = true;
/// For the 3 s buff, 0.6 s fade.
pub const CHEAT_DEATH_GHOST_FIRE: bool = true;
pub const CHEAT_DEATH_SIZE_MUL: f32 = 1.0;
pub const CHEAT_DEATH_OPACITY_MUL: f32 = 1.0;

// ── Its clock ────────────────────────────────────────────────────────────────

/// The onset (sequence 0, `Stand`, 1900 ms).
pub const CHEAT_DEATH_ONSET_SECS: f32 = 1.9;
/// The ghost-fire's way in: nothing before 0.8 s, full from 1.367 s.
pub const CHEAT_DEATH_FIRE_IN: (f32, f32) = (0.8, 1.367);
/// The ghost-fire's way out once the buff ends (sequence 159's fade).
pub const CHEAT_DEATH_FIRE_OUT_SECS: f32 = 0.6;
/// Flames in the ghost-fire column.
pub const CHEAT_DEATH_FLAMES: u8 = 18;

/// The thin rings' strokes: 3 and 4 px at the bench's framing (8 yd off,
/// 1000 px focal), so 3 / (1000 / 8) and 4 / (1000 / 8) yd.
pub const CHEAT_DEATH_INNER_RING_WIDTH: f32 = 3.0 * 8.0 / 1000.0;
pub const CHEAT_DEATH_OUTER_RING_WIDTH: f32 = 4.0 * 8.0 / 1000.0;

/// How far a piece drawn over another stands nearer the camera, so the
/// transparent sort draws it after: the sockets over the skull, the eyes over
/// the sockets (a socket drawn over an eye would put the eye out).
pub const CHEAT_DEATH_LAYER_STEP: f32 = 0.01;

// ── Where it sits (yards above the Rogue's feet) ─────────────────────────────

pub const CHEAT_DEATH_SKULL_HEIGHT: f32 = 2.9;
pub const CHEAT_DEATH_SKULL_SIZE: f32 = 1.3;
pub const CHEAT_DEATH_RING_HEIGHT: f32 = 3.04;
pub const CHEAT_DEATH_COLUMN_HEIGHT: f32 = 3.8;

// ── The pieces ───────────────────────────────────────────────────────────────

/// One drawn piece of the onset or the state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum CheatDeathPiece {
    /// The lavender skull: cranium and jaw, open at the sockets and nose.
    SkullBody,
    /// The sockets and nose, dark, over whatever is behind the skull.
    SkullSockets,
    EyeLeft,
    EyeRight,
    /// The two thin level rings over the head.
    InnerRing,
    OuterRing,
    /// The widening shockwave glow over the head.
    Shockwave,
    /// The white flare over the head.
    Flare,
    /// The tall light column through the body.
    LightColumn,
    /// One flame of the ghost-fire column, `0..CHEAT_DEATH_FLAMES`.
    Flame(u8),
}

/// How a piece is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheatDeathSprite {
    /// The bench's `bbGlow`: [`panel_glow_alpha`] on a camera-facing quad.
    Glow,
    /// The skull's cranium and jaw ([`skull_body_alpha`]).
    SkullBody,
    /// The skull's sockets and nose ([`skull_sockets_alpha`]).
    SkullSockets,
    /// An eye's glow ([`eye_glow_alpha`]).
    EyeGlow,
    /// The bench's `flame` sprite ([`flame_alpha`]).
    Flame,
    /// A thin ring lying level, its radius and stroke built into its mesh.
    LevelRing,
}

impl CheatDeathPiece {
    /// Every piece a rig carries: each kept layer's.
    pub fn all() -> Vec<CheatDeathPiece> {
        use CheatDeathPiece::*;
        let mut pieces = Vec::new();
        if CHEAT_DEATH_SKULL {
            pieces.extend([SkullBody, SkullSockets, EyeLeft, EyeRight]);
        }
        if CHEAT_DEATH_RINGS {
            pieces.extend([InnerRing, OuterRing, Shockwave, Flare]);
        }
        if CHEAT_DEATH_LIGHT_COLUMN {
            pieces.push(LightColumn);
        }
        if CHEAT_DEATH_GHOST_FIRE {
            pieces.extend((0..CHEAT_DEATH_FLAMES).map(Flame));
        }
        pieces
    }

    pub fn sprite(self) -> CheatDeathSprite {
        use CheatDeathPiece::*;
        match self {
            SkullBody => CheatDeathSprite::SkullBody,
            SkullSockets => CheatDeathSprite::SkullSockets,
            EyeLeft | EyeRight => CheatDeathSprite::EyeGlow,
            InnerRing | OuterRing => CheatDeathSprite::LevelRing,
            Shockwave | Flare | LightColumn => CheatDeathSprite::Glow,
            Flame(_) => CheatDeathSprite::Flame,
        }
    }

    /// Whether the bench paints the piece source-over, laid over what is
    /// behind it (the dark sockets, and the ghost-fire's flames, which
    /// `ghostFire` draws with the canvas's default composite), rather than
    /// adding light (`lighter`, every other piece).
    pub fn source_over(self) -> bool {
        matches!(
            self,
            CheatDeathPiece::SkullSockets | CheatDeathPiece::Flame(_)
        )
    }

    /// The piece's colour as it is spawned, the bench's sRGB (the
    /// shockwave's colour then moves with its pose).
    pub fn rgb(self) -> [u8; 3] {
        use CheatDeathPiece::*;
        match self {
            SkullBody => [149, 120, 255],
            SkullSockets => [20, 10, 40],
            EyeLeft | EyeRight => [255, 255, 255],
            InnerRing => [255, 250, 240],
            OuterRing => [255, 255, 214],
            Shockwave => [91, 117, 150],
            Flare => [255, 255, 255],
            LightColumn => [216, 241, 251],
            Flame(_) => [181, 134, 244],
        }
    }
}

/// A piece at one moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CheatDeathPose {
    /// Offset from the Rogue's feet along the world's axes.
    pub world: Vec3,
    /// Offset along the camera's axes: right, up, and toward the camera.
    pub view: Vec3,
    /// A camera-facing quad's width and height in yards; a level ring's
    /// radius (to the middle of its stroke) and its stroke width.
    pub size: Vec2,
    /// sRGB, 0-255.
    pub rgb: [f32; 3],
    pub alpha: f32,
}

fn rgb_f(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32, c[1] as f32, c[2] as f32]
}

/// The skull's alpha at `ms` (the bench's `keys([[0,0],[120,1],[567,1],[867,0]])`).
fn skull_alpha(ms: f32) -> f32 {
    keyed(&[(0.0, 0.0), (120.0, 1.0), (567.0, 1.0), (867.0, 0.0)], ms)
}

/// The ghost-fire's alpha at `age`, the buff having ended at `ended` (if it
/// has): in over 0.8-1.367 s, then held, then out over 0.6 s.
pub fn fire_alpha(age: f32, ended: Option<f32>) -> f32 {
    let (from, full) = CHEAT_DEATH_FIRE_IN;
    let fade_in = ((age - from) / (full - from)).clamp(0.0, 1.0);
    let fade_out = ended.map_or(1.0, |end| {
        (1.0 - (age - end) / CHEAT_DEATH_FIRE_OUT_SECS).clamp(0.0, 1.0)
    });
    fade_in * fade_out * CHEAT_DEATH_OPACITY_MUL
}

/// The combatant capsule's radius `y` yards above the feet: 0.5, its
/// cylinder from 0.25 to 1.75 yd, its hemispheres closing to the floor and to
/// 2.25 yd (`Capsule3d::new(0.5, 1.5)` centred 1.0 above the floor).
pub fn body_radius_at(y: f32) -> f32 {
    let beyond = ((y - 1.0).abs() - 0.75).max(0.0);
    (COMBATANT_BODY_RADIUS * COMBATANT_BODY_RADIUS - beyond * beyond)
        .max(0.0)
        .sqrt()
}

/// How far outside the body a flame stands.
pub const CHEAT_DEATH_FLAME_CLEARANCE: f32 = 0.05;

/// The radius a flame `y` yards up stands at: just outside the body.
///
/// A departure from the bench, whose flames run 0.4 -> 0.26 yd from the
/// axis (the client's 0.8 yd cylinder about its thinner body). The bench
/// paints them over the body in 2D; here the body is a 0.5 yd capsule that
/// would bury every one of them (the AS-10 lesson, as `immolate.rs`'s flame
/// ring). So each flame hugs the capsule's outline, and the far side's are
/// hidden by the body, as fire round a body is.
pub fn flame_ring_radius(y: f32) -> f32 {
    body_radius_at(y) + CHEAT_DEATH_FLAME_CLEARANCE
}

/// One flame's place in its rise, 0..1 (the bench's `ph`).
pub fn flame_phase(k: u8, ms: f32) -> f32 {
    let period = 520.0 + (k % 5) as f32 * 60.0;
    (ms + k as f32 * 97.0).rem_euclid(period) / period
}

/// A piece's pose `age` seconds after the save, the buff having ended at
/// `ended`; `None` while it draws nothing. Transcribes the bench's
/// `customDraw` for the "Cheat Death" tab.
pub fn cheat_death_pose(
    piece: CheatDeathPiece,
    age: f32,
    ended: Option<f32>,
) -> Option<CheatDeathPose> {
    use CheatDeathPiece::*;
    let s = CHEAT_DEATH_SIZE_MUL;
    let op = CHEAT_DEATH_OPACITY_MUL;
    let ms = age * 1000.0;
    let pose = |world: Vec3, view: Vec3, size: Vec2, rgb: [f32; 3], alpha: f32| {
        (alpha > 0.0).then_some(CheatDeathPose {
            world,
            view,
            size,
            rgb,
            alpha,
        })
    };
    match piece {
        SkullBody | SkullSockets | EyeLeft | EyeRight => {
            if ms >= 900.0 {
                return None;
            }
            let a = skull_alpha(ms) * op;
            let at = Vec3::Y * CHEAT_DEATH_SKULL_HEIGHT;
            let size = CHEAT_DEATH_SKULL_SIZE * s;
            let r = size * 0.5;
            let rgb = rgb_f(piece.rgb());
            match piece {
                SkullBody => pose(at, Vec3::ZERO, Vec2::splat(size), rgb, a * 0.75),
                SkullSockets => pose(
                    at,
                    Vec3::Z * CHEAT_DEATH_LAYER_STEP,
                    Vec2::splat(size),
                    rgb,
                    a * 0.85,
                ),
                _ => {
                    let side = if piece == EyeLeft { -1.0 } else { 1.0 };
                    pose(
                        at,
                        Vec3::new(side * 0.24 * r, 0.12 * r, 2.0 * CHEAT_DEATH_LAYER_STEP),
                        Vec2::splat(0.4 * r),
                        rgb,
                        a,
                    )
                }
            }
        }
        InnerRing | OuterRing | Shockwave | Flare => {
            if !(ms > 540.0 && ms < 1400.0) {
                return None;
            }
            let at = Vec3::Y * CHEAT_DEATH_RING_HEIGHT;
            match piece {
                InnerRing => {
                    let r1 = keyed(&[(567.0, 0.56), (1067.0, 0.97)], ms);
                    let a1 = keyed(&[(540.0, 0.0), (700.0, 1.0), (1100.0, 0.0)], ms);
                    pose(
                        at,
                        Vec3::ZERO,
                        Vec2::new(r1 * s * 0.5, CHEAT_DEATH_INNER_RING_WIDTH),
                        rgb_f(piece.rgb()),
                        a1 * op,
                    )
                }
                OuterRing => {
                    let r2 = keyed(&[(700.0, 0.56), (1200.0, 0.83)], ms);
                    let a2 = keyed(&[(680.0, 0.0), (760.0, 1.0), (1200.0, 0.0)], ms);
                    pose(
                        at,
                        Vec3::ZERO,
                        Vec2::new(r2 * s * 0.55, CHEAT_DEATH_OUTER_RING_WIDTH),
                        rgb_f(piece.rgb()),
                        a2 * op,
                    )
                }
                Shockwave => {
                    let r3 = keyed(&[(700.0, 0.56), (1300.0, 1.67)], ms);
                    let a3 = keyed(&[(680.0, 0.0), (760.0, 1.0), (1300.0, 0.0)], ms);
                    let c = keyed_color(
                        &[
                            (700.0, [91, 117, 150]),
                            (880.0, [255, 255, 255]),
                            (1300.0, [152, 99, 157]),
                        ],
                        ms,
                    )
                    .to_srgba();
                    pose(
                        at,
                        Vec3::ZERO,
                        Vec2::splat(r3 * s),
                        [c.red * 255.0, c.green * 255.0, c.blue * 255.0],
                        a3 * 0.5 * op,
                    )
                }
                _ => pose(
                    at,
                    Vec3::ZERO,
                    Vec2::splat(1.3 * s),
                    rgb_f(piece.rgb()),
                    keyed(&[(540.0, 0.0), (650.0, 0.39), (1300.0, 0.0)], ms) * op,
                ),
            }
        }
        LightColumn => {
            if !(ms > 700.0 && ms < 1150.0) {
                return None;
            }
            pose(
                Vec3::Y * CHEAT_DEATH_COLUMN_HEIGHT,
                Vec3::ZERO,
                Vec2::new(3.2 * s * 0.35, 16.8 * s * 0.6),
                rgb_f(piece.rgb()),
                keyed(&[(700.0, 0.0), (767.0, 1.0), (1100.0, 0.0)], ms) * 0.6 * op,
            )
        }
        Flame(k) => {
            let a = fire_alpha(age, ended);
            if a <= 0.0 {
                return None;
            }
            let angle = k as f32 / CHEAT_DEATH_FLAMES as f32 * TAU + ms / 900.0;
            let ph = flame_phase(k, ms);
            let y = -0.1 + ph * 2.3 * s;
            let r = flame_ring_radius(y);
            pose(
                Vec3::new(r * angle.cos(), y, r * angle.sin()),
                Vec3::ZERO,
                Vec2::splat(0.42 * s * (1.0 - ph * 0.5)),
                rgb_f(piece.rgb()),
                a * (ph * PI).sin() * 0.8,
            )
        }
    }
}

// ── The sprites ──────────────────────────────────────────────────────────────
//
// Each is white, its shape in the alpha channel, over the square [-1, 1]² of
// its quad (`x` to the right, `up` to the quad's top). The skull's units are
// the bench's `R`, half the skull's size.

/// The skull's cranium: the bench's arc, centre 0.15 up, radius 0.62, cut at
/// the chord where it meets the jaw.
const CRANIUM_CENTRE_UP: f32 = 0.15;
const CRANIUM_RADIUS: f32 = 0.62;
/// The jaw: from the chord down to 0.42 below the centre, 0.42 either side.
const JAW_BOTTOM: f32 = -0.42;
const JAW_HALF_WIDTH: f32 = 0.42;

/// The chord the cranium's arc ends on, `0.05 π` past level either side.
fn cranium_chord() -> (f32, f32) {
    let a = 0.05 * PI;
    (
        CRANIUM_CENTRE_UP - CRANIUM_RADIUS * a.sin(),
        CRANIUM_RADIUS * a.cos(),
    )
}

fn in_skull(x: f32, up: f32) -> bool {
    let (chord_up, chord_half) = cranium_chord();
    if up >= chord_up {
        x * x + (up - CRANIUM_CENTRE_UP).powi(2) <= CRANIUM_RADIUS * CRANIUM_RADIUS
    } else if up >= JAW_BOTTOM {
        let u = (chord_up - up) / (chord_up - JAW_BOTTOM);
        x.abs() <= chord_half + (JAW_HALF_WIDTH - chord_half) * u
    } else {
        false
    }
}

/// The sockets: ellipses 0.14 × 0.17 at (±0.24, 0.12).
fn in_socket(x: f32, up: f32) -> bool {
    let dx = (x.abs() - 0.24) / 0.14;
    let dy = (up - 0.12) / 0.17;
    dx * dx + dy * dy <= 1.0
}

/// The nose: a triangle from 0.05 below the centre down to a 0.14 wide base
/// at 0.2 below.
fn in_nose(x: f32, up: f32) -> bool {
    if !(-0.2..=-0.05).contains(&up) {
        return false;
    }
    x.abs() <= 0.07 * (-0.05 - up) / 0.15
}

/// The skull's cranium and jaw, open at the sockets and nose. `x` and `up`
/// are in the bench's `R`, so the quad spans `[-1, 1]`.
pub fn skull_body_alpha(x: f32, up: f32) -> f32 {
    if in_skull(x, up) && !in_socket(x, up) && !in_nose(x, up) {
        1.0
    } else {
        0.0
    }
}

/// The skull's sockets and nose.
pub fn skull_sockets_alpha(x: f32, up: f32) -> f32 {
    if in_socket(x, up) || in_nose(x, up) {
        1.0
    } else {
        0.0
    }
}

/// An eye's glow: full at its centre, nothing at its rim, linear between (the
/// bench's two-stop gradient over `R × 0.2`).
pub fn eye_glow_alpha(x: f32, up: f32) -> f32 {
    (1.0 - (x * x + up * up).sqrt()).max(0.0)
}

/// The bench's `flame` sprite: its radial gradient (0.8 at the centre, 0.55
/// at 0.45, nothing at the rim) filling an ellipse 0.6 wide and 1 tall whose
/// centre stands 0.25 above the sprite's.
pub fn flame_alpha(x: f32, up: f32) -> f32 {
    let ex = x / 0.6;
    let ey = up - 0.25;
    if ex * ex + ey * ey > 1.0 {
        return 0.0;
    }
    keyed(
        &[(0.0, 0.8), (0.45, 0.55), (1.0, 0.0)],
        (x * x + up * up).sqrt(),
    )
}

/// The bench's `bbGlow`, as a sprite.
pub fn glow_alpha(x: f32, up: f32) -> f32 {
    panel_glow_alpha((x * x + up * up).sqrt())
}

impl CheatDeathSprite {
    /// The sprite's alpha at `(x, up)` in its quad's `[-1, 1]²`; `None` for a
    /// level ring, drawn untextured.
    pub fn alpha_at(self, x: f32, up: f32) -> Option<f32> {
        match self {
            CheatDeathSprite::Glow => Some(glow_alpha(x, up)),
            CheatDeathSprite::SkullBody => Some(skull_body_alpha(x, up)),
            CheatDeathSprite::SkullSockets => Some(skull_sockets_alpha(x, up)),
            CheatDeathSprite::EyeGlow => Some(eye_glow_alpha(x, up)),
            CheatDeathSprite::Flame => Some(flame_alpha(x, up)),
            CheatDeathSprite::LevelRing => None,
        }
    }
}

/// Sprite pixels on a side.
pub const CHEAT_DEATH_SPRITE_PX: u32 = 128;
/// Subsamples a side per pixel, so the hard-edged skull is antialiased as the
/// bench's canvas fill is.
const SUBSAMPLES: u32 = 3;

/// A white sprite with `alpha` in its alpha channel. Row 0 is the top of the
/// quad (a `Rectangle`'s V runs down from its top edge).
fn sprite_image(alpha: impl Fn(f32, f32) -> f32) -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = CHEAT_DEATH_SPRITE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let n = SUBSAMPLES as f32;
    for row in 0..size {
        for col in 0..size {
            let mut sum = 0.0;
            for sy in 0..SUBSAMPLES {
                for sx in 0..SUBSAMPLES {
                    let x = (col as f32 + (sx as f32 + 0.5) / n) / size as f32 * 2.0 - 1.0;
                    let up = 1.0 - (row as f32 + (sy as f32 + 0.5) / n) / size as f32 * 2.0;
                    sum += alpha(x, up).clamp(0.0, 1.0);
                }
            }
            let i = ((row * size + col) * 4) as usize;
            data[i..i + 3].fill(255);
            data[i + 3] = (sum / (n * n) * 255.0).round() as u8;
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

/// The sprite as an image; `None` for a level ring.
pub fn cheat_death_sprite_image(sprite: CheatDeathSprite) -> Option<Image> {
    sprite.alpha_at(0.0, 0.0)?;
    Some(sprite_image(move |x, up| {
        sprite.alpha_at(x, up).unwrap_or(0.0)
    }))
}

/// A level ring's mesh: an annulus `width` wide about `radius`, facing +Z.
pub fn level_ring_mesh(radius: f32, width: f32) -> Mesh {
    let half = width * 0.5;
    Mesh::from(Annulus::new((radius - half).max(0.0), radius + half))
}

// ── The particles ────────────────────────────────────────────────────────────

/// How a layer's particles launch.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CheatDeathLaunch {
    /// A uniformly random direction (the bench's `vRange` 3.14).
    Sphere,
    /// Within `cone` radians of straight DOWN: the bench's cone about up
    /// (`vRange`) flown at a negative speed, so the smoke sinks.
    Sinking { cone: f32 },
}

/// One of the onset's emitters, as the bench's layer record carries it: the
/// record as a [`ClientEmitter`] (its `window` or `track` is the emission
/// window from the save; its `speed` the launch speed), where the particles
/// are born, how they launch, their drag, the bench sprite each is drawn
/// with, and its tail.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CheatDeathLayer {
    pub on: bool,
    pub emitter: ClientEmitter,
    /// The bench sprite's radial alpha stops at its centre, 0.45 of the way
    /// out and its rim (`sprite()`'s `soft`): `glow` is `[1, 0.45, 0]`,
    /// `smoke` `[0.85, 0.5, 0]`.
    pub soft: [f32; 3],
    /// The bench's `tail`: a particle faster than 0.2 yd/s trails a line
    /// `min(tail × 0.12, speed × 0.06)` yd long behind it; `0.0` draws none.
    pub tail: f32,
    /// Birth height above the feet (the bench's `pos` height).
    pub height: f32,
    /// Horizontal birth offset from the Rogue's axis (the bench's `pos` x, y).
    pub offset: Vec2,
    pub launch: CheatDeathLaunch,
    /// The velocity's decay per second: the bench integrates `drag` as
    /// `v × (1 − 3 × drag × dt)` a frame, so this is `3 × drag`.
    pub drag: f32,
}

/// P0's rate (`[[0,50],[567,100],[1100,0]]` ms), across the window it runs
/// in (`[[0,0],[167,1],[1100,0]]`): from its value at 167 ms.
const SMOKE_TRACK: &[(f32, f32)] = &[
    (0.167, 50.0 + 50.0 * 167.0 / 567.0),
    (0.567, 100.0),
    (1.1, 0.0),
];

/// The emitters at the bench's size and opacity knobs.
const fn at_knobs(mut e: ClientEmitter) -> ClientEmitter {
    e.size = e.size.scaled(CHEAT_DEATH_SIZE_MUL);
    let op = CHEAT_DEATH_OPACITY_MUL;
    e.alpha = [e.alpha[0] * op, e.alpha[1] * op, e.alpha[2] * op];
    e
}

/// The bench's `glow` and `smoke` sprites' alpha stops.
pub const GLOW_SOFT: [f32; 3] = [1.0, 0.45, 0.0];
pub const SMOKE_SOFT: [f32; 3] = [0.85, 0.5, 0.0];

/// The onset's two emitters.
pub const CHEAT_DEATH_LAYERS: [CheatDeathLayer; 2] = [
    // P6, `t_vfx_flar_blur`: 300/s at 0.567-0.833 s out of the skull, in
    // every direction at 8.3 yd/s under drag 0.6, living 0.9 s, white ->
    // lavender, flashing large at birth and settling to 0.056 by 5% of life.
    // The bench draws each as a round `glow` with a tail behind it (`tail`
    // 3) that shortens as the streak slows.
    CheatDeathLayer {
        on: CHEAT_DEATH_STREAKS,
        emitter: at_knobs(ClientEmitter {
            name: "cheat_death/p6 streaks",
            sprite: EmitterSprite::Glow,
            additive: true,
            rate: 300.0,
            window: Some((0.567, 0.833)),
            life: 0.9,
            speed: 8.3,
            gravity: 0.0,
            area: 0.0,
            column: None,
            rise: false,
            color: [[255, 255, 255], [235, 210, 255], [183, 183, 255]],
            alpha: [0.2, 1.0, 0.0],
            size: ParticleSize::radii([0.4, 0.056, 0.056]),
            mid: 0.05,
            track: None,
        }),
        height: 3.17,
        offset: Vec2::new(-0.2, 0.0),
        launch: CheatDeathLaunch::Sphere,
        drag: 3.0 * 0.6,
        soft: GLOW_SOFT,
        tail: 3.0,
    },
    // P0, `toonsmoke16`: indigo smoke at 3.3 yd, 50 -> 100/s, emitted
    // 0.167-1.1 s, sinking at 0.56 yd/s, living 2.5 s.
    CheatDeathLayer {
        on: CHEAT_DEATH_SMOKE,
        emitter: at_knobs(ClientEmitter {
            name: "cheat_death/p0 smoke",
            sprite: EmitterSprite::Glow,
            additive: true,
            rate: 0.0,
            window: ClientEmitter::tracked(SMOKE_TRACK),
            life: 2.5,
            speed: 0.56,
            gravity: 0.0,
            area: 0.0,
            column: None,
            rise: false,
            color: [[42, 12, 157], [60, 15, 203], [97, 110, 203]],
            alpha: [0.31, 0.7, 0.0],
            size: ParticleSize::radii([0.83, 0.26, 0.42]),
            mid: 0.5,
            track: Some(SMOKE_TRACK),
        }),
        height: 3.3,
        offset: Vec2::ZERO,
        launch: CheatDeathLaunch::Sinking { cone: 0.23 },
        drag: 0.0,
        soft: SMOKE_SOFT,
        tail: 0.0,
    },
];

impl CheatDeathLayer {
    /// Particles this layer owes between `t0` and `t1` seconds after the save.
    pub fn count_between(&self, t0: f32, t1: f32) -> f32 {
        if !self.on {
            return 0.0;
        }
        self.emitter.count_between(t0, t1)
    }

    /// When this layer's last particle is spent.
    pub fn done_secs(&self) -> f32 {
        if self.on {
            self.emitter.end() + self.emitter.life
        } else {
            0.0
        }
    }

    /// Where a particle is born, from the Rogue's feet, and how it launches.
    pub fn launch(&self, seed: u32) -> (Vec3, Vec3) {
        let at = Vec3::new(self.offset.x, self.height, self.offset.y);
        let speed = self.emitter.speed;
        let velocity = match self.launch {
            CheatDeathLaunch::Sphere => {
                let u = draw(seed, 1) * 2.0 - 1.0;
                let theta = draw(seed, 2) * TAU;
                let r = (1.0 - u * u).max(0.0).sqrt();
                Vec3::new(r * theta.cos(), u, r * theta.sin()) * speed
            }
            CheatDeathLaunch::Sinking { cone } => {
                let polar = (draw(seed, 1) * 2.0 - 1.0) * cone;
                let azimuth = draw(seed, 2) * TAU;
                -Vec3::new(
                    polar.sin() * azimuth.cos(),
                    polar.cos(),
                    polar.sin() * azimuth.sin(),
                ) * speed
            }
        };
        (at, velocity)
    }
}

/// When the onset's last particle is spent: the rig stays up until then.
pub fn cheat_death_particles_secs() -> f32 {
    CHEAT_DEATH_LAYERS
        .iter()
        .map(CheatDeathLayer::done_secs)
        .fold(0.0, f32::max)
}

/// A bench particle sprite (`sprite()`'s `glow` or `smoke`): `soft[0]` at the
/// centre, `soft[1]` 0.45 of the way out, `soft[2]` at the rim, linear
/// between, nothing beyond.
pub fn particle_sprite_alpha(soft: [f32; 3], r: f32) -> f32 {
    if r > 1.0 {
        return 0.0;
    }
    keyed(
        &[(0.0, soft[0]), (0.45, soft[1]), (1.0, soft[2])],
        r.max(0.0),
    )
}

/// A layer's particle sprite as an image.
pub fn particle_sprite_image(soft: [f32; 3]) -> Image {
    sprite_image(move |x, up| particle_sprite_alpha(soft, (x * x + up * up).sqrt()))
}

/// A layer's palette: `PALETTE_STEPS` materials sampling its colour and
/// alpha ramps over the particle's life, each adding colour × sprite ×
/// alpha (the bench's `lighter`).
fn particle_palette(
    materials: &mut Assets<StandardMaterial>,
    e: &ClientEmitter,
    sprite: &Handle<Image>,
) -> Arc<[Handle<StandardMaterial>]> {
    (0..PALETTE_STEPS)
        .map(|i| {
            let t = (i as f32 + 0.5) / PALETTE_STEPS as f32;
            materials.add(StandardMaterial {
                base_color: ramp_color(e.color, e.mid, t).with_alpha(ramp3(e.alpha, e.mid, t)),
                base_color_texture: Some(sprite.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        })
        .collect()
}

/// The bench's tail behind a particle moving at `speed`, yards (its
/// `min(tail × 0.12, speed × 0.06) × size`), or `None` when it draws none
/// (no tail, or slower than 0.2 yd/s).
pub fn tail_length(tail: f32, speed: f32) -> Option<f32> {
    (tail > 0.0 && speed > 0.2).then(|| (tail * 0.12).min(speed * 0.06) * CHEAT_DEATH_SIZE_MUL)
}

/// A tail as it is drawn, in the rig's frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TailPose {
    /// The midpoint of the line from the particle back along its flight.
    pub at: Vec3,
    /// A camera-facing quad rolled so its long (local Y) axis lies along the
    /// line as the camera sees it.
    pub rotation: Quat,
    /// Width (the bench's stroke, 0.7 of the sprite's radius) and length
    /// (the line's length as the camera sees it).
    pub size: Vec2,
    /// sRGB 0-1, and alpha (0.85 of the particle's).
    pub color: Color,
}

/// The tail of `particle`, standing at `at`, seen through a camera turned
/// `camera`: the bench's `streak` — a line from the particle back along its
/// flight, `min(tail × 0.12, speed × 0.06)` long, stroked 0.7 of its sprite's
/// radius wide in its colour at 0.85 of its alpha.
pub fn tail_pose(
    layer: &CheatDeathLayer,
    particle: &ClientParticle,
    at: Vec3,
    camera: Quat,
) -> Option<TailPose> {
    let e = &layer.emitter;
    let length = tail_length(layer.tail, particle.velocity.length())?;
    let t = (particle.age / particle.life).clamp(0.0, 1.0);
    let line = particle.velocity.normalize() * length;
    let (right, up, back) = (camera * Vec3::X, camera * Vec3::Y, camera * Vec3::Z);
    let seen = line - back * line.dot(back);
    let (sx, sy) = (seen.dot(right), seen.dot(up));
    if sx * sx + sy * sy < 1e-10 {
        return None;
    }
    let diameter = ramp3(particle.size.diameter_keys(), particle.mid, t);
    Some(TailPose {
        at: at - line * 0.5,
        rotation: camera * Quat::from_rotation_z((-sx).atan2(sy)),
        size: Vec2::new(diameter * 0.5 * 0.7, seen.length()),
        color: ramp_color(e.color, e.mid, t).with_alpha(0.85 * ramp3(e.alpha, e.mid, t)),
    })
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Whether a prevented killing blow, by what saved it, plays this rig. The one
/// predicate the marker's consumer and the lands-silently audit both ask.
pub fn plays_cheat_death(saved_by: &str) -> bool {
    saved_by == CHEAT_DEATH_NAME
}

/// Whether a unit holds the Cheat Death buff its save left.
pub fn holds_cheat_death(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| a.auras.iter().any(|au| au.ability_name == CHEAT_DEATH_NAME))
}

// ── Components ───────────────────────────────────────────────────────────────

/// One save's skull and ghost-fire: a top-level rig at the Rogue's feet,
/// unrotated, parent of its pieces and its particles.
#[derive(Component, Debug)]
pub struct CheatDeathRig {
    pub bearer: Entity,
    /// Seconds since the save.
    pub age: f32,
    /// The age at which the buff ended, once it has.
    pub ended: Option<f32>,
    /// Whether the buff has been seen on the Rogue (a save that lands its
    /// buff through a pending aura shows it a tick late).
    pub held: bool,
    /// Particles owed per layer, carried between frames.
    pub carry: [f32; 2],
    /// Particles emitted, the scatter's seed.
    pub emitted: u32,
}

impl CheatDeathRig {
    pub fn new(bearer: Entity) -> Self {
        Self {
            bearer,
            age: 0.0,
            ended: None,
            held: false,
            carry: [0.0; 2],
            emitted: 0,
        }
    }
}

/// One piece of a [`CheatDeathRig`], with the material it fades and the
/// mesh it is drawn with.
#[derive(Component, Debug)]
pub struct CheatDeathPart {
    pub piece: CheatDeathPiece,
    pub material: Handle<StandardMaterial>,
    pub mesh: Handle<Mesh>,
}

/// A particle's tail (the bench's `streak`), under the rig beside its
/// particle, with the material it colours.
#[derive(Component, Debug)]
pub struct CheatDeathTail {
    pub head: Entity,
    /// Its particle's layer in [`CHEAT_DEATH_LAYERS`].
    pub layer: usize,
    pub material: Handle<StandardMaterial>,
}

/// Raise the rig for a save on `holder` (the marker's consumer calls this).
pub fn spawn_cheat_death(commands: &mut Commands, holder: Entity) {
    commands.spawn((
        CheatDeathRig::new(holder),
        Transform::default(),
        Visibility::default(),
        PlayMatchEntity,
    ));
}

/// When the buff ends, by what the rig has seen: the frame it is gone after
/// being held, or, never seen, at the end of the onset.
pub fn cheat_death_ended(age: f32, held_now: bool, seen: bool) -> bool {
    !held_now && (seen || age >= CHEAT_DEATH_ONSET_SECS)
}

/// Whether a rig has played out: its ghost-fire faded and its last particle
/// spent.
pub fn cheat_death_finished(age: f32, ended: Option<f32>) -> bool {
    ended.is_some_and(|end| age >= end + CHEAT_DEATH_FIRE_OUT_SECS)
        && age >= cheat_death_particles_secs()
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// The meshes and sprites every rig draws with, built once.
pub struct CheatDeathAssets {
    quad: Handle<Mesh>,
    glow: Handle<Image>,
    skull: Handle<Image>,
    sockets: Handle<Image>,
    eye: Handle<Image>,
    flame: Handle<Image>,
}

impl CheatDeathAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        let mut image =
            |sprite| images.add(cheat_death_sprite_image(sprite).expect("a textured sprite"));
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            glow: image(CheatDeathSprite::Glow),
            skull: image(CheatDeathSprite::SkullBody),
            sockets: image(CheatDeathSprite::SkullSockets),
            eye: image(CheatDeathSprite::EyeGlow),
            flame: image(CheatDeathSprite::Flame),
        }
    }

    fn texture(&self, sprite: CheatDeathSprite) -> Option<Handle<Image>> {
        match sprite {
            CheatDeathSprite::Glow => Some(self.glow.clone()),
            CheatDeathSprite::SkullBody => Some(self.skull.clone()),
            CheatDeathSprite::SkullSockets => Some(self.sockets.clone()),
            CheatDeathSprite::EyeGlow => Some(self.eye.clone()),
            CheatDeathSprite::Flame => Some(self.flame.clone()),
            CheatDeathSprite::LevelRing => None,
        }
    }
}

/// The particles' quad, each layer's sprite and palette, built once.
pub struct CheatDeathParticleAssets {
    quad: Handle<Mesh>,
    palettes: Vec<Arc<[Handle<StandardMaterial>]>>,
}

impl CheatDeathParticleAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let palettes = CHEAT_DEATH_LAYERS
            .iter()
            .map(|layer| {
                let sprite = images.add(particle_sprite_image(layer.soft));
                particle_palette(materials, &layer.emitter, &sprite)
            })
            .collect();
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            palettes,
        }
    }
}

/// A piece's material: added light (`lighter`: colour × sprite × alpha and
/// nothing else), or laid over what is behind it (alpha-blended: the sockets
/// and the flames).
fn part_material(piece: CheatDeathPiece, texture: Option<Handle<Image>>) -> StandardMaterial {
    let [r, g, b] = piece.rgb();
    StandardMaterial {
        base_color: Color::srgb_u8(r, g, b).with_alpha(0.0),
        base_color_texture: texture,
        unlit: true,
        alpha_mode: if piece.source_over() {
            AlphaMode::Blend
        } else {
            AlphaMode::Add
        },
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// Build each new rig at its Rogue's feet, with every piece under it.
pub fn build_cheat_death_rigs(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<CheatDeathAssets>>,
    mut rigs: Query<(Entity, &CheatDeathRig, &mut Transform), Added<CheatDeathRig>>,
    bearers: Query<&Transform, (With<Combatant>, Without<CheatDeathRig>)>,
) {
    for (rig, saved, mut transform) in rigs.iter_mut() {
        if let Ok(bearer) = bearers.get(saved.bearer) {
            transform.translation = body_point(0.0, bearer.translation, false);
        }
        let assets =
            assets.get_or_insert_with(|| CheatDeathAssets::build(&mut meshes, &mut images));
        for piece in CheatDeathPiece::all() {
            let sprite = piece.sprite();
            let mesh = match sprite {
                CheatDeathSprite::LevelRing => meshes.add(level_ring_mesh(0.5, 0.0)),
                _ => assets.quad.clone(),
            };
            let material = materials.add(part_material(piece, assets.texture(sprite)));
            let part = commands
                .spawn((
                    CheatDeathPart {
                        piece,
                        material: material.clone(),
                        mesh: mesh.clone(),
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::ZERO),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(rig).add_child(part);
        }
    }
}

/// Play every rig: take it down the frame its Rogue dies or is gone; follow
/// the Rogue's feet; note when the buff ends; pose and fade every piece to
/// the camera; retire spent particles and emit the ones owed; and retire the
/// rig once it has played out. Runs before `animate_client_particles`, which
/// ages what it emits.
///
/// The rig is unrotated, so a child's world frame is the rig's: a quad turned
/// to the camera's rotation faces it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_cheat_death(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut particle_assets: Local<Option<CheatDeathParticleAssets>>,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<CheatDeathRig>,
            Without<CheatDeathPart>,
        ),
    >,
    mut rigs: Query<
        (
            Entity,
            &mut CheatDeathRig,
            &mut Transform,
            Option<&Children>,
        ),
        Without<CheatDeathPart>,
    >,
    bearers: Query<
        (&Transform, &Combatant, Option<&ActiveAuras>),
        (Without<CheatDeathRig>, Without<CheatDeathPart>),
    >,
    mut parts: Query<
        (&CheatDeathPart, &mut Transform),
        (Without<CheatDeathRig>, Without<Camera3d>),
    >,
    particles: Query<&ClientParticle>,
    tails: Query<&CheatDeathTail>,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next().map_or(Quat::IDENTITY, |c| c.rotation);
    let (cam_right, cam_up, cam_back) = (cam * Vec3::X, cam * Vec3::Y, cam * Vec3::Z);
    for (entity, mut rig, mut transform, children) in rigs.iter_mut() {
        let Ok((bearer, combatant, auras)) = bearers.get(rig.bearer) else {
            commands.entity(entity).despawn();
            continue;
        };
        if !combatant.is_alive() {
            commands.entity(entity).despawn();
            continue;
        }
        let (from, to) = (rig.age, rig.age + dt);
        let held = holds_cheat_death(auras);
        rig.held |= held;
        if rig.ended.is_none() && cheat_death_ended(from, held, rig.held) {
            rig.ended = Some(from);
        }
        if cheat_death_finished(from, rig.ended) {
            commands.entity(entity).despawn();
            continue;
        }
        rig.age = to;
        transform.translation = body_point(0.0, bearer.translation, false);

        for child in children.into_iter().flat_map(|c| c.iter()) {
            if let Ok((part, mut pose_at)) = parts.get_mut(child) {
                let Some(material) = materials.get_mut(&part.material) else {
                    continue;
                };
                let Some(pose) = cheat_death_pose(part.piece, from, rig.ended) else {
                    pose_at.scale = Vec3::ZERO;
                    material.base_color.set_alpha(0.0);
                    continue;
                };
                pose_at.translation = pose.world
                    + cam_right * pose.view.x
                    + cam_up * pose.view.y
                    + cam_back * pose.view.z;
                if part.piece.sprite() == CheatDeathSprite::LevelRing {
                    if let Some(mesh) = meshes.get_mut(&part.mesh) {
                        *mesh = level_ring_mesh(pose.size.x, pose.size.y);
                    }
                    // Tipped back a quarter turn, the ring lies level.
                    pose_at.rotation = Quat::from_rotation_x(-PI / 2.0);
                    pose_at.scale = Vec3::ONE;
                } else {
                    pose_at.rotation = cam;
                    pose_at.scale = pose.size.extend(1.0);
                }
                let [r, g, b] = pose.rgb;
                material.base_color =
                    Color::srgba(r / 255.0, g / 255.0, b / 255.0, pose.alpha.clamp(0.0, 1.0));
            } else if particles.get(child).is_ok_and(|p| p.age >= p.life) {
                commands.entity(child).despawn();
            } else if let Ok(tail) = tails.get(child) {
                // A tail goes with its particle.
                if particles.get(tail.head).map_or(true, |p| p.age >= p.life) {
                    commands.entity(child).despawn();
                }
            }
        }

        let assets = particle_assets.get_or_insert_with(|| {
            CheatDeathParticleAssets::build(&mut meshes, &mut images, &mut materials)
        });
        for (i, layer) in CHEAT_DEATH_LAYERS.iter().enumerate() {
            rig.carry[i] += layer.count_between(from, to);
            if rig.carry[i] < 1.0 {
                continue;
            }
            let e = &layer.emitter;
            let palette = assets.palettes[i].clone();
            let quad = assets.quad.clone();
            while rig.carry[i] >= 1.0 {
                rig.carry[i] -= 1.0;
                let seed = entity
                    .index()
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(rig.emitted.wrapping_mul(0x85EB_CA6B));
                rig.emitted = rig.emitted.wrapping_add(1);
                let (at, velocity) = layer.launch(seed);
                let mut particle = commands.spawn((
                    ClientParticle {
                        age: 0.0,
                        life: e.life,
                        velocity,
                        gravity: 0.0,
                        size: e.size,
                        mid: e.mid,
                        palette: palette.clone(),
                        step: 0,
                        facing: ParticleFacing::Camera,
                        owned: true,
                    },
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(palette[0].clone()),
                    Transform::from_translation(at),
                    NotShadowCaster,
                ));
                if layer.drag > 0.0 {
                    particle.insert(ParticleDrag {
                        per_sec: layer.drag,
                    });
                }
                let particle = particle.id();
                commands.entity(entity).add_child(particle);
                if layer.tail > 0.0 {
                    let material = materials.add(StandardMaterial {
                        base_color: Color::NONE,
                        unlit: true,
                        alpha_mode: AlphaMode::Add,
                        cull_mode: None,
                        double_sided: true,
                        ..default()
                    });
                    let tail = commands
                        .spawn((
                            CheatDeathTail {
                                head: particle,
                                layer: i,
                                material: material.clone(),
                            },
                            Mesh3d(quad.clone()),
                            MeshMaterial3d(material),
                            Transform::from_translation(at).with_scale(Vec3::ZERO),
                            NotShadowCaster,
                        ))
                        .id();
                    commands.entity(entity).add_child(tail);
                }
            }
        }
    }
}

/// Turn every live particle of a rig to the camera, and lay each tail behind
/// its particle as the camera sees it, after the client particles have aged
/// and moved.
#[allow(clippy::type_complexity)]
pub fn billboard_cheat_death_particles(
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<CheatDeathRig>,
            Without<ClientParticle>,
            Without<CheatDeathTail>,
        ),
    >,
    rigs: Query<(&Transform, &Children), With<CheatDeathRig>>,
    mut particles: Query<
        (&ClientParticle, &mut Transform),
        (
            Without<Camera3d>,
            Without<CheatDeathRig>,
            Without<CheatDeathTail>,
        ),
    >,
    mut tails: Query<
        (&CheatDeathTail, &mut Transform),
        (
            Without<Camera3d>,
            Without<CheatDeathRig>,
            Without<ClientParticle>,
        ),
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
        for child in children.iter() {
            let Ok((tail, mut line)) = tails.get_mut(child) else {
                continue;
            };
            let pose = particles
                .get(tail.head)
                .ok()
                .filter(|(p, _)| p.age < p.life)
                .and_then(|(p, at)| {
                    tail_pose(
                        &CHEAT_DEATH_LAYERS[tail.layer],
                        p,
                        at.translation,
                        cam.rotation,
                    )
                });
            let Some(pose) = pose else {
                line.scale = Vec3::ZERO;
                continue;
            };
            line.translation = pose.at;
            line.rotation = pose.rotation;
            line.scale = pose.size.extend(1.0);
            if let Some(m) = materials.get_mut(&tail.material) {
                m.base_color = pose.color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_cheat_death_save_plays_the_rig() {
        assert!(plays_cheat_death("Cheat Death"));
        assert!(!plays_cheat_death("Guardian Spirit"));
        assert!(!plays_cheat_death(""));
    }

    #[test]
    fn the_cranium_meets_the_jaw_at_its_chord() {
        let (up, half) = cranium_chord();
        assert!((up - 0.05301).abs() < 1e-4, "{up}");
        assert!((half - 0.61237).abs() < 1e-4, "{half}");
    }
}
