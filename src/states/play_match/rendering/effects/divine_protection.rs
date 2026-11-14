//! Divine Protection — a red-gold globe round the Paladin's chest for the
//! buff's life (AS-376).
//!
//! From the client data joined in
//! `docs/design/2026-10-18-paladin-talent-client-data.md` §3 (Wrath 498,
//! SpellVisual 11817): the cast plays Divine Shield's Holy hand kits, and the
//! `(7,8)` state kit 11101 holds `wrongfully_accused_impact.m2` at the chest
//! for the whole 8 s. That model is a globe of radius about 0.75 yd round the
//! torso on one 2.634 s sequence, which the state loops: a pale-gold layer
//! (`aura_02_h`, then `flare1_tc`), a red layer (`lavalump2`, then
//! `fire1white_invert`, flashing white), a fire burst and flash at the pulse,
//! and red glints. It reads as a red-gold globe that pulses once every 2.6 s.
//!
//! Played as the signed-off AS-337 Paladin Talent Visuals Bench draws it
//! (https://claude.ai/artifact/31rnhhKpACF51iUsqK5g7k, version
//! 1792379243-e3d6, "Divine Protection" tab, committed as
//! `docs/design/benches/2026-10-18-paladin-talent-bench.html`): every layer on,
//! at the client's values (`DIVINE_PROTECTION_SIZE_MUL` and
//! `DIVINE_PROTECTION_OPACITY_MUL` 1.0). The pieces below are the bench's
//! `customDraw` for the effect, ported one for one: its camera-facing glows
//! (`bbGlow`), its level rings (`ring3`), the pulse's fourteen flames and the
//! six glints (`sprite`). Its cast hands are the instant Holy flash AS-374
//! built (`HealCastKind::HolyFlash`), as Aura Mastery's are.
//!
//! **Lifecycle.** Detected renderer-side off `ActiveAuras`, like Guardian
//! Spirit and the shields: a living unit holding Divine Protection wears one
//! globe, raised (and its hands flashed) on the first frame it holds it, and
//! taken down on the frame it no longer does — run out, purged, its bearer
//! dead (auras stay on a corpse) or gone. Its envelope fades it in over 0.2 s
//! and out over the buff's last 0.3 s, read off the aura's own remaining
//! time, so a natural expiry has already faded when the globe goes. Divine
//! Protection is applied inline by the Paladin AI with no cast state, so the
//! onset is where the hands flash; the animation sandbox lands it through a
//! zero-length cast, whose `Landed` marker `heal_cast.rs` leaves alone
//! ([`flashes_hands_at_onset`]).
//!
//! **Anchor.** The globe rides the bearer's RENDERED body (its `VisualBody`:
//! walk bob, fear bob, swing step, Cyclone lift), placed in `PostUpdate` after
//! every writer of it, as the shield cards are (`shield.rs`). That keeps its
//! draw order against the blended capsule fixed: every piece but the flying
//! flames and the glints stands on the body's axis, above its centre, so it
//! sorts in front at every camera pitch. Divine Shield's cards stand on the
//! same axis lower down; [`GLOBE_SORT_BIAS`] keeps their blended outline and
//! tint behind the globe's light, so the two draw together, the globe over
//! the cards.
//!
//! Graphical-only: nothing here writes sim state, no `game_rng` is drawn (the
//! scatter is the bench's own hash), and every system is registered in
//! `states/mod.rs` only. Headless never runs any of it.

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::druid_heals::{body_point, keyed, BODY_CENTRE_ABOVE_FLOOR};
use super::heal_cast::{flash_instant_hands, HealCastAssets};
use super::paladin_oneshots::{
    bench_hash, oneshot_glow_texture, oneshot_ring_texture, piece_rotation, PieceFacing, PiecePose,
    ONESHOT_CHEST, RING_STROKE,
};
use super::shield::disc_texture;
use super::spell_bolts::star_flash_texture;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ── The bench's constants block (signed off at version 1792379243-e3d6) ─────

// Divine Protection (Wrath visual 11817): Divine Shield's hand kits on cast;
// kit 11101 wrongfully_accused_impact.m2 @Chest as the 8 s state, 2.634 s
// cycle. AS-337 bench. Every layer on.
pub const DIVINE_PROTECTION_HANDS: bool = true;
pub const DIVINE_PROTECTION_GOLD_GLOBE: bool = true;
pub const DIVINE_PROTECTION_RED_LAYERS: bool = true;
pub const DIVINE_PROTECTION_PULSE: bool = true;
pub const DIVINE_PROTECTION_GLINTS: bool = true;
/// The globe's radius, yards (the client's ≈ 0.75).
pub const DIVINE_PROTECTION_GLOBE_RADIUS: f32 = 0.75;
/// The model's sequence, which the state loops.
pub const DIVINE_PROTECTION_CYCLE_SECS: f32 = 2.634;
pub const DIVINE_PROTECTION_SIZE_MUL: f32 = 1.0;
pub const DIVINE_PROTECTION_OPACITY_MUL: f32 = 1.0;
// AuraApplyRoute: owned by the globe (by name), not the band.

/// Where the globe stands: the chest attachment, yards above the floor (the
/// bench's `ANCH.chest`).
pub const DIVINE_PROTECTION_CHEST: f32 = ONESHOT_CHEST;

// ── The envelope ─────────────────────────────────────────────────────────────

/// The globe fades in over this long from its onset...
pub const GLOBE_FADE_IN_SECS: f32 = 0.2;
/// ...and out over the buff's last this long.
pub const GLOBE_FADE_OUT_SECS: f32 = 0.3;

/// The whole globe's weight `age` seconds after its onset with `remaining`
/// seconds of the buff left (the bench's `env`: 0 → 1 by 0.2 s, held, 1 → 0
/// over the last 0.3 s).
pub fn globe_envelope(age: f32, remaining: f32) -> f32 {
    let rise = (age / GLOBE_FADE_IN_SECS).clamp(0.0, 1.0);
    let fall = (remaining / GLOBE_FADE_OUT_SECS).clamp(0.0, 1.0);
    rise.min(fall)
}

/// Where `age` falls in the model's cycle, seconds.
pub fn globe_cycle(age: f32) -> f32 {
    age.max(0.0).rem_euclid(DIVINE_PROTECTION_CYCLE_SECS)
}

// ── The cycle's tracks (seconds into the cycle) ──────────────────────────────

/// The pale-gold layer: `aura_02_h` in by 0.267 s and out by 0.834 s, and
/// `flare1_tc` in by 0.7 s and out by 1.2 s, each at weight 0.8; the layer
/// takes the stronger.
pub const GLOBE_GOLD_RING_WEIGHT: [(f32, f32); 5] = [
    (0.0, 0.0),
    (0.167, 0.0),
    (0.267, 0.8),
    (0.734, 0.8),
    (0.834, 0.0),
];
pub const GLOBE_GOLD_FLARE_WEIGHT: [(f32, f32); 5] =
    [(0.0, 0.0), (0.6, 0.0), (0.7, 0.8), (1.1, 0.8), (1.2, 0.0)];
/// The red layer: `lavalump2` at 0.8 from 1.134 s to 1.534 s, and
/// `fire1white_invert` peaking at 1.0 at 1.5 s; the layer takes the stronger.
pub const GLOBE_RED_LAVA_WEIGHT: [(f32, f32); 5] = [
    (0.0, 0.0),
    (1.034, 0.0),
    (1.134, 0.8),
    (1.534, 0.8),
    (1.634, 0.0),
];
pub const GLOBE_RED_FIRE_WEIGHT: [(f32, f32); 4] =
    [(0.0, 0.0), (1.267, 0.0), (1.5, 1.0), (1.834, 0.0)];
/// The red layer's colour: red, flashing white at the pulse (1.267 s).
pub const GLOBE_RED_RGB: [(f32, [u8; 3]); 4] = [
    (0.0, [246, 35, 35]),
    (1.267, [255, 255, 255]),
    (1.5, [249, 58, 44]),
    (1.8, [246, 35, 35]),
];

/// The pale-gold layer's glow (a camera-facing quad, 1.15 × the globe's
/// diameter) and rings (the equator at the globe's radius, and two at 0.8 of
/// it, 0.35 yd above and below), with each one's alpha over the weight.
pub const GLOBE_GOLD_GLOW_RGB: [u8; 3] = [252, 236, 190];
pub const GLOBE_GOLD_GLOW_SPAN: f32 = 1.15;
pub const GLOBE_GOLD_GLOW_ALPHA: f32 = 0.5;
pub const GLOBE_GOLD_RING_RGB: [u8; 3] = [252, 240, 200];
pub const GLOBE_GOLD_RING_ALPHA: f32 = 0.8;
pub const GLOBE_GOLD_TROPIC_LIFT: f32 = 0.35;
pub const GLOBE_GOLD_TROPIC_RADIUS: f32 = 0.8;
pub const GLOBE_GOLD_TROPIC_ALPHA: f32 = 0.5;
/// The red layer's glow (the globe's diameter) and its ring (1.02 × the
/// radius, on the equator).
pub const GLOBE_RED_GLOW_ALPHA: f32 = 0.55;
pub const GLOBE_RED_RING_RADIUS: f32 = 1.02;
pub const GLOBE_RED_RING_ALPHA: f32 = 0.8;

/// The pulse, from 1.267 s (the bench draws it for 0.7 s, inside which its
/// flash and flames each run out on their own tracks): a 1.4 yd flash...
pub const GLOBE_PULSE_START: f32 = 1.267;
pub const GLOBE_PULSE_FLASH_SIZE: f32 = 1.4;
pub const GLOBE_PULSE_FLASH_RGB: [u8; 3] = [255, 200, 150];
pub const GLOBE_PULSE_FLASH_ALPHA: [(f32, f32); 4] =
    [(0.0, 0.0), (0.033, 1.0), (0.2, 0.6), (0.4, 0.0)];
/// ...and fourteen flames thrown out from the chest at 2.8 yd/s, each living
/// 0.5 s, in three volleys 12 ms apart, white → orange → white.
pub const GLOBE_PULSE_FLAMES: u8 = 14;
pub const GLOBE_FLAME_LIFE: f32 = 0.5;
pub const GLOBE_FLAME_STAGGER: f32 = 0.012;
pub const GLOBE_FLAME_SPEED: f32 = 2.8;
pub const GLOBE_FLAME_SIZE: f32 = 0.3;
pub const GLOBE_FLAME_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 255, 255]),
    (0.5, [233, 98, 32]),
    (1.0, [255, 255, 255]),
];
pub const GLOBE_FLAME_ALPHA: [(f32, f32); 3] = [(0.0, 0.51), (0.5, 0.29), (1.0, 0.0)];

/// Six glints on the globe's skin while the cycle is between 0.2 s and
/// 1.9 s, each twinkling over 0.5 s and moving on to a new point every 0.5 s,
/// white → red → white.
pub const GLOBE_GLINTS: u8 = 6;
pub const GLOBE_GLINT_WINDOW: (f32, f32) = (0.2, 1.9);
pub const GLOBE_GLINT_PERIOD: f32 = 0.5;
pub const GLOBE_GLINT_SIZE: f32 = 0.2;
/// The glints' orbit: `(across, up)` over the globe's radius, the up
/// component's frequency over the turn, and the turn per glint, radians.
pub const GLOBE_GLINT_ORBIT: (f32, f32) = (0.9, 0.8);
pub const GLOBE_GLINT_LIFT_FREQ: f32 = 1.3;
pub const GLOBE_GLINT_TURN: f32 = 1.7;
pub const GLOBE_GLINT_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 255, 255]),
    (0.5, [223, 32, 32]),
    (1.0, [255, 255, 255]),
];
pub const GLOBE_GLINT_ALPHA: [(f32, f32); 3] = [(0.0, 0.0), (0.5, 0.5), (1.0, 0.0)];

/// The bench's `sprite('flame')`: a radial glow `(0.8, 0.55, 0)` of its
/// radius, cut to an upright ellipse 0.6 of the radius across and the radius
/// tall, whose centre sits a quarter of the radius above the sprite's.
pub const GLOBE_FLAME_STOPS: [(f32, f32); 3] = [(0.0, 0.8), (0.45, 0.55), (1.0, 0.0)];
pub const GLOBE_FLAME_ELLIPSE: (f32, f32) = (0.6, 1.0);
pub const GLOBE_FLAME_ELLIPSE_LIFT: f32 = 0.25;

/// The transparent pass's sort bias for every piece, yards of view depth
/// (Bevy adds it to the sort distance; larger draws later, in front). Divine
/// Shield's blended outline and tint stand on the same axis, their centres
/// up to about 0.04 yd of view depth ahead of the globe's lower ring at the
/// steepest pitch the camera allows, and would otherwise draw over its light
/// and dim it. Small beside any other unit's distance, and truncated to an
/// i32 for the depth-bias state, so the depth test is untouched.
pub const GLOBE_SORT_BIAS: f32 = 0.1;

// ── Routing ──────────────────────────────────────────────────────────────────

/// Whether `ability`'s hands flash at its globe's onset rather than off a
/// cast marker, so `heal_cast.rs` leaves a `Landed` marker naming it alone
/// (the animation sandbox lands every entry through a zero-length cast).
pub fn flashes_hands_at_onset(ability: AbilityType) -> bool {
    ability == AbilityType::DivineProtection
}

/// The Divine Protection a unit holds, if any: its remaining seconds. A unit
/// holds one at most (refresh-only stacking keys it by its source).
pub fn divine_protection_remaining(auras: &ActiveAuras) -> Option<f32> {
    auras
        .auras
        .iter()
        .find(|a| is_divine_protection(a.effect_type, &a.ability_name))
        .map(|a| a.duration)
}

// ── The pieces ───────────────────────────────────────────────────────────────

/// One drawn piece of the globe.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum GlobePiece {
    /// The pale-gold layer's camera-facing glow.
    GoldGlow,
    /// Its ring on the equator.
    GoldRing,
    /// Its ring 0.35 yd above the equator.
    GoldRingUpper,
    /// Its ring 0.35 yd below.
    GoldRingLower,
    /// The red layer's camera-facing glow.
    RedGlow,
    /// Its ring, just outside the gold equator.
    RedRing,
    /// The pulse's flash.
    PulseFlash,
    /// One of the pulse's flames.
    PulseFlame { index: u8 },
    /// One of the red glints.
    Glint { index: u8 },
}

/// The sprite a piece is drawn with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlobeSprite {
    /// The soft glow (`bbGlow`).
    Glow,
    /// A thin ring (`ring3`).
    Ring,
    /// The upright flame (`sprite('flame')`).
    Flame,
    /// A four-point star (`sprite('star')`).
    Star,
}

impl GlobePiece {
    pub fn sprite(self) -> GlobeSprite {
        match self {
            GlobePiece::GoldGlow | GlobePiece::RedGlow | GlobePiece::PulseFlash => {
                GlobeSprite::Glow
            }
            GlobePiece::GoldRing
            | GlobePiece::GoldRingUpper
            | GlobePiece::GoldRingLower
            | GlobePiece::RedRing => GlobeSprite::Ring,
            GlobePiece::PulseFlame { .. } => GlobeSprite::Flame,
            GlobePiece::Glint { .. } => GlobeSprite::Star,
        }
    }
}

/// The pieces a globe draws, by its layer toggles.
pub fn globe_pieces() -> Vec<GlobePiece> {
    let mut pieces = Vec::new();
    if DIVINE_PROTECTION_GOLD_GLOBE {
        pieces.extend([
            GlobePiece::GoldGlow,
            GlobePiece::GoldRing,
            GlobePiece::GoldRingUpper,
            GlobePiece::GoldRingLower,
        ]);
    }
    if DIVINE_PROTECTION_RED_LAYERS {
        pieces.extend([GlobePiece::RedGlow, GlobePiece::RedRing]);
    }
    if DIVINE_PROTECTION_PULSE {
        pieces.push(GlobePiece::PulseFlash);
        pieces.extend((0..GLOBE_PULSE_FLAMES).map(|index| GlobePiece::PulseFlame { index }));
    }
    if DIVINE_PROTECTION_GLINTS {
        pieces.extend((0..GLOBE_GLINTS).map(|index| GlobePiece::Glint { index }));
    }
    pieces
}

/// A colour track at `t`, interpolated in sRGB bytes and truncated, as the
/// bench's canvas does (`rgb()`'s `|0`).
fn globe_track_rgb(track: &[(f32, [u8; 3])], t: f32) -> [u8; 3] {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = track.iter().map(|(t, v)| (*t, v[c] as f32)).collect();
        keyed(&keys, t).floor().clamp(0.0, 255.0) as u8
    };
    [channel(0), channel(1), channel(2)]
}

/// The pale-gold layer's weight `cycle` seconds into the cycle.
pub fn globe_gold_weight(cycle: f32) -> f32 {
    keyed(&GLOBE_GOLD_RING_WEIGHT, cycle).max(keyed(&GLOBE_GOLD_FLARE_WEIGHT, cycle))
}

/// The red layer's weight.
pub fn globe_red_weight(cycle: f32) -> f32 {
    keyed(&GLOBE_RED_LAVA_WEIGHT, cycle).max(keyed(&GLOBE_RED_FIRE_WEIGHT, cycle))
}

/// The red layer's colour.
pub fn globe_red_rgb(cycle: f32) -> [u8; 3] {
    globe_track_rgb(&GLOBE_RED_RGB, cycle)
}

/// The bench's flame direction for flame `k`: its hash, centred and
/// normalised.
pub fn globe_flame_direction(k: u8) -> Vec3 {
    let k = k as u32;
    Vec3::new(
        bench_hash(k) - 0.5,
        bench_hash(k + 7) - 0.5,
        bench_hash(k + 13) - 0.5,
    )
    .normalize_or_zero()
}

/// A piece's pose `age` seconds after the globe's onset with `remaining`
/// seconds of the buff left: its position relative to the chest, its size
/// (a quad's full width and height), facing, colour and alpha — the size and
/// opacity knobs applied. Pure: the probes and the placement read one
/// definition.
pub fn globe_piece_pose(piece: GlobePiece, age: f32, remaining: f32) -> PiecePose {
    let env = globe_envelope(age, remaining) * DIVINE_PROTECTION_OPACITY_MUL;
    let cyc = globe_cycle(age);
    let r = DIVINE_PROTECTION_GLOBE_RADIUS;
    let flat = PieceFacing::Flat { turn: 0.0 };
    // A ring of radius `radius`: its sprite's stroke runs at RING_STROKE.0 of
    // the quad's half-width, so the quad is sized to put it at `radius`.
    let ring = |lift: f32, radius: f32, rgb: [u8; 3], alpha: f32| PiecePose {
        at: Vec3::Y * lift,
        size: Vec2::splat(2.0 * radius / RING_STROKE.0),
        facing: flat,
        rgb,
        alpha,
    };
    let glow = |size: f32, rgb: [u8; 3], alpha: f32| PiecePose {
        at: Vec3::ZERO,
        size: Vec2::splat(size),
        facing: PieceFacing::Camera,
        rgb,
        alpha,
    };
    let gold = globe_gold_weight(cyc) * env;
    let red = globe_red_weight(cyc) * env;
    let pulse = cyc - GLOBE_PULSE_START;
    let pose = match piece {
        GlobePiece::GoldGlow => glow(
            2.0 * r * GLOBE_GOLD_GLOW_SPAN,
            GLOBE_GOLD_GLOW_RGB,
            gold * GLOBE_GOLD_GLOW_ALPHA,
        ),
        GlobePiece::GoldRing => ring(0.0, r, GLOBE_GOLD_RING_RGB, gold * GLOBE_GOLD_RING_ALPHA),
        GlobePiece::GoldRingUpper => ring(
            GLOBE_GOLD_TROPIC_LIFT,
            r * GLOBE_GOLD_TROPIC_RADIUS,
            GLOBE_GOLD_RING_RGB,
            gold * GLOBE_GOLD_TROPIC_ALPHA,
        ),
        GlobePiece::GoldRingLower => ring(
            -GLOBE_GOLD_TROPIC_LIFT,
            r * GLOBE_GOLD_TROPIC_RADIUS,
            GLOBE_GOLD_RING_RGB,
            gold * GLOBE_GOLD_TROPIC_ALPHA,
        ),
        GlobePiece::RedGlow => glow(2.0 * r, globe_red_rgb(cyc), red * GLOBE_RED_GLOW_ALPHA),
        GlobePiece::RedRing => ring(
            0.0,
            r * GLOBE_RED_RING_RADIUS,
            globe_red_rgb(cyc),
            red * GLOBE_RED_RING_ALPHA,
        ),
        GlobePiece::PulseFlash => {
            // Its track is dark at both ends, so it bounds itself.
            glow(
                GLOBE_PULSE_FLASH_SIZE,
                GLOBE_PULSE_FLASH_RGB,
                keyed(&GLOBE_PULSE_FLASH_ALPHA, pulse) * env,
            )
        }
        GlobePiece::PulseFlame { index } => {
            let since = pulse - (index % 3) as f32 * GLOBE_FLAME_STAGGER;
            // A flame lives 0.5 s from its volley, inside the pulse's 0.7 s.
            let lit = (0.0..=GLOBE_FLAME_LIFE).contains(&since);
            let ph = (since / GLOBE_FLAME_LIFE).clamp(0.0, 1.0);
            PiecePose {
                at: if lit {
                    globe_flame_direction(index) * GLOBE_FLAME_SPEED * since
                } else {
                    Vec3::ZERO
                },
                size: Vec2::splat(GLOBE_FLAME_SIZE),
                facing: PieceFacing::Camera,
                rgb: globe_track_rgb(&GLOBE_FLAME_RGB, ph),
                alpha: if lit {
                    keyed(&GLOBE_FLAME_ALPHA, ph) * env
                } else {
                    0.0
                },
            }
        }
        GlobePiece::Glint { index } => {
            let lit = cyc > GLOBE_GLINT_WINDOW.0 && cyc < GLOBE_GLINT_WINDOW.1;
            let beat = (cyc - GLOBE_GLINT_WINDOW.0) / GLOBE_GLINT_PERIOD;
            let ph = (beat + index as f32 / GLOBE_GLINTS as f32).rem_euclid(1.0);
            let a = index as f32 * GLOBE_GLINT_TURN + beat.floor();
            PiecePose {
                at: Vec3::new(
                    r * GLOBE_GLINT_ORBIT.0 * a.cos(),
                    r * GLOBE_GLINT_ORBIT.1 * (a * GLOBE_GLINT_LIFT_FREQ).sin(),
                    r * GLOBE_GLINT_ORBIT.0 * a.sin(),
                ),
                size: Vec2::splat(GLOBE_GLINT_SIZE),
                // The canvas turns a sprite clockwise on screen.
                facing: PieceFacing::CameraRolled { roll: -a },
                rgb: globe_track_rgb(&GLOBE_GLINT_RGB, ph),
                alpha: if lit {
                    keyed(&GLOBE_GLINT_ALPHA, ph) * env
                } else {
                    0.0
                },
            }
        }
    };
    PiecePose {
        at: pose.at * DIVINE_PROTECTION_SIZE_MUL,
        size: pose.size * DIVINE_PROTECTION_SIZE_MUL,
        ..pose
    }
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// Generated sprite textures, pixels on a side.
const GLOBE_SPRITE_PX: u32 = 128;

/// The bench's flame sprite: [`GLOBE_FLAME_STOPS`] over the sprite's radius, cut to
/// the upright [`GLOBE_FLAME_ELLIPSE`] lifted [`GLOBE_FLAME_ELLIPSE_LIFT`]. The image's top
/// row is the quad's top.
pub fn globe_flame_texture() -> Image {
    disc_texture(GLOBE_SPRITE_PX, |r, angle| {
        // `disc_texture` measures down the image; up is negative.
        let (x, y) = (r * angle.cos(), -r * angle.sin());
        let (w, h) = GLOBE_FLAME_ELLIPSE;
        let inside = (x / w).powi(2) + ((y - GLOBE_FLAME_ELLIPSE_LIFT) / h).powi(2) <= 1.0;
        if inside && r < 1.0 {
            keyed(&GLOBE_FLAME_STOPS, r)
        } else {
            0.0
        }
    })
}

/// The mesh and the textures every globe draws with, built once.
pub struct GlobeAssets {
    quad: Handle<Mesh>,
    glow: Handle<Image>,
    ring: Handle<Image>,
    flame: Handle<Image>,
    star: Handle<Image>,
}

impl GlobeAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            glow: images.add(oneshot_glow_texture()),
            ring: images.add(oneshot_ring_texture()),
            flame: images.add(globe_flame_texture()),
            star: images.add(star_flash_texture()),
        }
    }

    fn texture(&self, sprite: GlobeSprite) -> Handle<Image> {
        match sprite {
            GlobeSprite::Glow => self.glow.clone(),
            GlobeSprite::Ring => self.ring.clone(),
            GlobeSprite::Flame => self.flame.clone(),
            GlobeSprite::Star => self.star.clone(),
        }
    }
}

// ── Components ───────────────────────────────────────────────────────────────

/// One bearer's globe: a rig at its chest, parent of the pieces. Lives exactly
/// as long as Divine Protection is on a living bearer.
#[derive(Component, Debug)]
pub struct DivineProtectionGlobe {
    pub bearer: Entity,
    /// Seconds since the globe appeared.
    pub age: f32,
    /// The buff's remaining seconds, as last read.
    pub remaining: f32,
}

/// One piece of a [`DivineProtectionGlobe`].
#[derive(Component, Debug)]
pub struct DivineProtectionGlobePiece {
    pub piece: GlobePiece,
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): raise a globe on every living unit holding Divine
/// Protection, flashing its Holy hands, age each globe and read the buff's
/// remaining time, and take a globe down when the buff ends (run out or
/// purged) or its bearer dies or is gone.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_divine_protection_globes(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<GlobeAssets>>,
    mut hand_assets: Local<Option<HealCastAssets>>,
    bearers: Query<(Entity, &Combatant, &Transform, &ActiveAuras)>,
    casters: Query<&Children>,
    bodies: Query<(), With<VisualBody>>,
    mut globes: Query<(Entity, &mut DivineProtectionGlobe)>,
) {
    let dt = time.delta_secs();
    let held = |entity: Entity| {
        bearers
            .get(entity)
            .ok()
            .filter(|(_, combatant, _, _)| combatant.is_alive())
            .and_then(|(_, _, _, auras)| divine_protection_remaining(auras))
    };
    let mut worn = Vec::new();
    for (rig, mut globe) in globes.iter_mut() {
        match held(globe.bearer) {
            Some(remaining) => {
                globe.age += dt;
                globe.remaining = remaining;
                worn.push(globe.bearer);
            }
            None => commands.entity(rig).despawn(),
        }
    }
    for (entity, _, transform, _) in bearers.iter() {
        let Some(remaining) = held(entity) else {
            continue;
        };
        if worn.contains(&entity) {
            continue;
        }
        let assets = assets.get_or_insert_with(|| GlobeAssets::build(&mut meshes, &mut images));
        let rig = commands
            .spawn((
                DivineProtectionGlobe {
                    bearer: entity,
                    age: 0.0,
                    remaining,
                },
                Transform::from_translation(body_point(
                    DIVINE_PROTECTION_CHEST,
                    transform.translation,
                    false,
                )),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .id();
        for piece in globe_pieces() {
            let pose = globe_piece_pose(piece, 0.0, remaining);
            let material = materials.add(StandardMaterial {
                base_color: Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2]).with_alpha(0.0),
                base_color_texture: Some(assets.texture(piece.sprite())),
                // The bench adds colour × sprite × alpha and nothing else.
                unlit: true,
                alpha_mode: AlphaMode::Add,
                depth_bias: GLOBE_SORT_BIAS,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            let child = commands
                .spawn((
                    DivineProtectionGlobePiece { piece },
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(1e-3)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(rig).add_child(child);
        }
        if DIVINE_PROTECTION_HANDS {
            flash_instant_hands(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &mut hand_assets,
                &casters,
                &bodies,
                entity,
                AbilityType::DivineProtection,
            );
        }
    }
}

/// Where a bearer's globe stands: [`DIVINE_PROTECTION_CHEST`] above the feet
/// of its rendered body (`body_local`, its `VisualBody`'s translation).
pub fn divine_protection_anchor(bearer: &Transform, body_local: Vec3) -> Vec3 {
    bearer.transform_point(body_local)
        + Vec3::Y * (DIVINE_PROTECTION_CHEST - BODY_CENTRE_ABOVE_FLOOR)
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each globe on its bearer's rendered chest, and
/// pose, turn and fade its pieces. The rig is unrotated, so a piece's local
/// pose is its world one.
#[allow(clippy::type_complexity)]
pub fn place_divine_protection_globes(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<DivineProtectionGlobe>,
            Without<DivineProtectionGlobePiece>,
        ),
    >,
    mut globes: Query<
        (&DivineProtectionGlobe, &mut Transform, &Children),
        (
            Without<Combatant>,
            Without<VisualBody>,
            Without<DivineProtectionGlobePiece>,
        ),
    >,
    bearers: Query<
        (&Transform, Option<&Children>),
        (
            With<Combatant>,
            Without<DivineProtectionGlobe>,
            Without<DivineProtectionGlobePiece>,
        ),
    >,
    bodies: Query<
        &Transform,
        (
            With<VisualBody>,
            Without<Combatant>,
            Without<DivineProtectionGlobe>,
            Without<DivineProtectionGlobePiece>,
        ),
    >,
    mut pieces: Query<
        (
            &DivineProtectionGlobePiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<DivineProtectionGlobe>,
            Without<Combatant>,
            Without<VisualBody>,
        ),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cam = camera.iter().next().map(|c| c.rotation);
    for (globe, mut transform, children) in globes.iter_mut() {
        let Ok((bearer, bearer_children)) = bearers.get(globe.bearer) else {
            continue;
        };
        let body_local = bearer_children
            .and_then(|kids| kids.iter().find_map(|k| bodies.get(k).ok()))
            .map(|body| body.translation)
            .unwrap_or(Vec3::ZERO);
        transform.translation = divine_protection_anchor(bearer, body_local);
        for child in children.iter() {
            let Ok((piece, mut part, material)) = pieces.get_mut(child) else {
                continue;
            };
            let pose = globe_piece_pose(piece.piece, globe.age, globe.remaining);
            part.translation = pose.at;
            part.scale = pose.size.extend(1.0).max(Vec3::splat(1e-3));
            part.rotation = piece_rotation(pose.facing, Quat::IDENTITY, cam);
            let color = Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2])
                .with_alpha(pose.alpha.clamp(0.0, 1.0));
            // Only touch the asset when the look moved: `get_mut` marks it
            // changed and re-uploads it.
            if materials
                .get(&material.0)
                .is_some_and(|m| m.base_color != color)
            {
                if let Some(m) = materials.get_mut(&material.0) {
                    m.base_color = color;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants block, as the signed-off bench exports it. (Every
    /// layer on is the probes' set equality over the drawn pieces.)
    #[test]
    fn the_constants_are_the_benchs() {
        assert_eq!(DIVINE_PROTECTION_GLOBE_RADIUS, 0.75);
        assert_eq!(DIVINE_PROTECTION_CYCLE_SECS, 2.634);
        assert_eq!(DIVINE_PROTECTION_SIZE_MUL, 1.0);
        assert_eq!(DIVINE_PROTECTION_OPACITY_MUL, 1.0);
        assert_eq!(DIVINE_PROTECTION_CHEST, 1.45);
    }

    /// The envelope: in over 0.2 s, out over the last 0.3 s.
    #[test]
    fn the_globe_fades_in_and_out() {
        assert_eq!(globe_envelope(0.0, 8.0), 0.0);
        assert!((globe_envelope(0.1, 7.9) - 0.5).abs() < 1e-6);
        assert_eq!(globe_envelope(0.2, 7.8), 1.0);
        assert_eq!(globe_envelope(4.0, 4.0), 1.0);
        assert!((globe_envelope(7.85, 0.15) - 0.5).abs() < 1e-5);
        assert_eq!(globe_envelope(8.0, 0.0), 0.0);
    }

    /// The layers' weights at the bench's keys, the colour's flash, and the
    /// cycle repeating.
    #[test]
    fn the_layers_follow_the_cycle() {
        assert_eq!(globe_gold_weight(0.1), 0.0);
        assert!((globe_gold_weight(0.5) - 0.8).abs() < 1e-6);
        assert!(
            (globe_gold_weight(0.9) - 0.8).abs() < 1e-6,
            "the flare holds it"
        );
        assert_eq!(globe_gold_weight(1.3), 0.0);
        assert_eq!(globe_red_weight(0.9), 0.0);
        assert!((globe_red_weight(1.3) - 0.8).abs() < 1e-6);
        assert!((globe_red_weight(1.5) - 1.0).abs() < 1e-6, "the fire peaks");
        assert_eq!(globe_red_weight(1.9), 0.0);
        assert_eq!(globe_red_rgb(1.267), [255, 255, 255]);
        assert_eq!(globe_red_rgb(1.8), [246, 35, 35]);
        assert_eq!(globe_red_rgb(2.5), [246, 35, 35]);
        assert!((globe_cycle(2.634 + 0.5) - 0.5).abs() < 1e-5);
    }

    /// The flame sprite: soft from its centre, narrow across, its tip lifted.
    #[test]
    fn the_flame_sprite_is_an_upright_teardrop() {
        let image = globe_flame_texture();
        let px = GLOBE_SPRITE_PX as usize;
        let alpha_at = |x: f32, y_up: f32| {
            let col = (((x + 1.0) / 2.0) * px as f32) as usize;
            let row = (((1.0 - y_up) / 2.0) * px as f32) as usize;
            image.data.as_ref().unwrap()[(row.min(px - 1) * px + col.min(px - 1)) * 4 + 3]
        };
        assert!((200..=205).contains(&alpha_at(0.0, 0.0)), "0.8 at the core");
        assert_eq!(alpha_at(0.7, 0.0), 0, "narrow across");
        assert!(alpha_at(0.0, 0.7) > 50, "tall above");
        assert!(alpha_at(0.0, -0.7) > 0, "and below, as far as the ellipse");
        assert_eq!(alpha_at(0.0, -0.8), 0, "the ellipse ends 0.75 below");
    }
}
