//! Inner Focus — the arcane hand flash and four blue comets converging on the
//! Priest's chest as it is cast, then a violet-cyan glow on both palms until a
//! cast spends it (AS-352).
//!
//! From the client data joined in
//! `docs/design/2026-10-17-priest-talent-client-data.md` §1 (AS-335; Inner
//! Focus 14751, SpellVisual 4372):
//!
//! - **Cast** (kit 425): `magic_cast_hand.m2` on both hands, Moonfire's
//!   shipped arcane hands (`heal_cast.rs`, [`HealCastKind::Arcane`]).
//! - **Impact** (kit 3772): `innerfocus_impact_chest.m2` at the chest, 0.4 s:
//!   four blue comets streak in from the corners of a 1.5 yd square and cross
//!   at the chest, each trailing a ribbon, with a small spark pop at the end.
//! - **State** (kit 3773): `magic_precast_hand.m2` on BOTH hands for as long
//!   as the buff waits to be spent: a glow about 0.74 yd across, pulsing 1.15
//!   -> 0.95 -> 1.15 on a 533 ms loop, shedding motes and small rings.
//!
//! Played as the signed-off AS-335 Priest Talent Visuals Bench draws it
//! (https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
//! 1792207490-53c9, the "Inner Focus" tab; its source as committed with the
//! sign-off is `docs/design/benches/2026-10-17-priest-talent-bench.html`).
//! Every layer on, every multiplier 1.0. The pieces are the tab's
//! `customDraw` and emitter records, one for one:
//!
//!   comets -> four heads (`sprite('star')`, 0.66 yd across shrinking to
//!             0.26, (44,155,225)) flying from (±0.74, ±0.74) to (∓0.37,
//!             ∓0.37) yd in the Priest's upright plane, 0.3 yd in front of
//!             it, in 400 ms, then fading out over 500 ms; each trails five
//!             0.17 yd glows (`sprite('glow')`, (0,121,198)) at 6%, 12%, ...
//!             30% of the flight behind it, each fainter by a sixth;
//!   pop    -> stars from the chest, the rate rising 0 -> 60/s over 200 ms,
//!             held to 400 ms, then off: 18 stars, 2.6 yd/s, 0.6 s lives,
//!             violet -> red -> sky blue;
//!   palm   -> two camera-facing glows on each palm (`bbGlow`): 0.74 yd
//!             violet (150,90,240) at 0.7 and 0.4 yd cyan (120,220,255) at
//!             0.6, both scaled by the 533 ms pulse;
//!   motes  -> soft glows, white -> lilac -> violet, 0.6 s lives, drifting
//!             out at 0.19 yd/s (`palmmotes`).
//!
//! **Blend, as the bench composites each call.** The palm glows (`bbGlow`)
//! and both emitters are `lighter`: `AlphaMode::Add`. The comets' heads and
//! trails are drawn by `customDraw`'s own `sprite()` calls, which the canvas
//! composites `source-over`: `AlphaMode::Blend`.
//!
//! **The palm glow yields to the cast hands.** Both sit on the same spell
//! hands, so on a hand where a cast-hand rig of the same Priest is live
//! ([`HealCastHand`]: Inner Focus's own arcane flash, a precast loop, any
//! launch flare) the palm glow is hidden, its clock running on, and it comes
//! back when that rig ends. The two never draw on one hand at once
//! ([`palm_yields`]). In play the Priest AI casts Inner Focus one frame ahead
//! of the Flash Heal it feeds, which spends it at once, so the glow shows
//! only while that heal is held back.
//!
//! **Lifecycle.** The flash rides the sim's `AuraLanding` marker
//! (`apply_pending_auras`, on the buff's application), which also flashes the
//! arcane hands (`flash_instant_hands`): the AI applies Inner Focus inline,
//! with no cast state to flash from. The palm state is detected
//! renderer-side off `ActiveAuras`, as the Discipline states are: a living
//! Priest holding the buff wears one rig, raised on the first frame it holds
//! it and taken down on the frame it no longer does — spent by a cast
//! (`next_cast::consume`, as the cast is chosen), purged, its Priest dead
//! (auras stay on a corpse) or gone. A cast with a cast bar that does not
//! land gives the buff back (`next_cast::restore`): the palms rise again, and
//! the comets do not replay (a restore is no landing). A polymorphed Priest's
//! palms and flash are hidden with its body. A flash plays out over a Priest
//! that dies under it and goes at once with one that is gone. A Priest
//! cannot stealth; nothing else here reads stealth. Everything is a
//! `PlayMatchEntity`.
//!
//! **Held in front of the body.** The body is an `AlphaMode::Blend` capsule
//! that writes no depth, and the transparent pass sorts by each mesh's
//! translation, so a piece deeper than the body's centre would be painted over
//! by it: the comets and pop whenever the Priest faces away from the camera,
//! a palm on the body's far side. Every piece is drawn on its own view ray,
//! slid toward the camera until it stands nearer than the RENDERED body's
//! nearest point, and scaled by the same ratio (the shared body hold, `body_hold.rs`;
//! AS-347's rule): the same screen footprint, sorted in front. Only a Priest
//! bears either piece, so no pet body is ever held against.
//!
//! **Departures from the bench**, each deliberate:
//!
//! 1. **The hold** above, which the 2D canvas does not need.
//! 2. **The palms are the shipped spell hands** ([`spell_hand_local`], the
//!    sockets every cast hand and Vengeance's glow use), not the bench's
//!    (±0.45, 1.3, +0.15) yd, which stand inside the 0.5 yd capsule. Sharing
//!    the socket is what lets the glow yield to the cast hands.
//! 3. **The motes are born at each palm.** The bench draws one emitter, 21/s,
//!    born in a 0.9 x 0.1 x 0.9 yd box centred between its hands. Here each
//!    palm emits half of it, [`PALM_MOTE_RATE`] = 10.5/s, born in the half of
//!    that box it sat in, [`PALM_MOTE_BOX`] = 0.45 x 0.1 x 0.45 yd, re-centred
//!    on the palm (the card and the client both shed per hand).
//! 4. **The comets' plane turns with the Priest.** The bench has no facing:
//!    its comets fly in its world x-y plane, 0.3 yd toward +z. Here that
//!    plane is the Priest's own upright plane (its right and up), 0.3 yd
//!    along its facing, as the client's chest attachment turns with the body.
//! 5. **The stars' white core is tinted** (the shared star sprite is white
//!    throughout and takes the star's colour), as for Power Infusion.
//! 6. **Particles ride the rig in height too.** The bench's `follow` moves
//!    them with the body in x and z only.
//! 7. **The state ends with its buff.** The bench repeats the flash every 6 s
//!    and holds the palms in between; here the flash plays once per landing
//!    and the palms go on the frame the buff does.
//!
//! **At the match camera** (60 yd, pitch 38.7°, FOV 45°, about 22 px a yard at
//! 1080p): a comet's head is about 15 px across at its start and 6 px as the
//! four cross; a palm's violet glow about 16-19 px across at 0.7 alpha.
//!
//! Graphical-only: nothing here writes sim state, no `game_rng` is drawn (the
//! scatter is the bench's own hash), and every system is registered in
//! `states/mod.rs` only. Headless never runs any of it.

use std::f32::consts::TAU;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{body_capsule, held_in, BodyCapsule, BodyHold, HoldDisjoint, HoldView};
use super::cheat_death::{particle_sprite_image, GLOW_SOFT};
use super::druid_heals::{body_point, keyed};
use super::heal_cast::{flash_instant_hands, spell_hand_local, HealCastAssets};
use super::paladin_oneshots::{bench_hash, oneshot_glow_texture};
use super::spell_bolts::star_flash_texture;
use crate::states::play_match::components::*;

// ── The bench's constants block (signed off at version 1792207490-53c9) ─────

// Inner Focus: innerfocus_impact_chest.m2 (kit 3772) on cast; magic_precast_hand.m2 (kit 3773) on both hands until spent. AS-335 bench.
pub const INNER_FOCUS_COMETS: bool = true; // 400 ms
pub const INNER_FOCUS_SPARK_POP: bool = true;
pub const INNER_FOCUS_PALM_GLOW: bool = true; // 0.74 yd, 533 ms pulse
pub const INNER_FOCUS_PALM_MOTES: bool = true;
pub const INNER_FOCUS_SIZE_MUL: f32 = 1.00;
pub const INNER_FOCUS_OPACITY_MUL: f32 = 1.00;

// ── The flash: innerfocus_impact_chest.m2 ────────────────────────────────────

/// The chest attachment, yards above the feet (the bench's `ANCH.chest`).
pub const FOCUS_CHEST: f32 = 1.45;
/// The comets' plane stands this far in front of the Priest (`vp[2] + 0.3`).
pub const COMET_FORWARD: f32 = 0.3;
/// Each comet starts at (±START, ±START) and ends at (∓END, ∓END) yd in the
/// plane: `lerp(0.74 s, -0.37 s, u)`.
pub const COMET_START: f32 = 0.74;
pub const COMET_END: f32 = 0.37;
/// The four corners a comet starts from, `(sx, sy)`, in the bench's order.
pub const COMET_CORNERS: [(f32, f32); 4] = [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
/// The flight, then the fade after it: drawn while under 900 ms.
pub const COMET_FLIGHT_SECS: f32 = 0.4;
pub const COMET_FADE_SECS: f32 = 0.5;
/// A head's full width at the start (`0.33 × 2`), and the share it loses by
/// the end of the flight (`1 − 0.6 u`).
pub const COMET_HEAD_WIDTH: f32 = 0.66;
pub const COMET_HEAD_SHRINK: f32 = 0.6;
pub const COMET_HEAD_RGB: [u8; 3] = [44, 155, 225];
/// The trail: five glows, glow `k` at `u − 0.06 k` of the flight, its alpha
/// `1 − k / 6` of the head's.
pub const COMET_TRAIL_DOTS: u8 = 5;
pub const COMET_TRAIL_STEP: f32 = 0.06;
pub const COMET_TRAIL_WIDTH: f32 = 0.17;
pub const COMET_TRAIL_RGB: [u8; 3] = [0, 121, 198];

/// The spark pop's rate, per second, keyed in seconds from the cast (the
/// bench's `rate:[[0,0],[200,60],[400,60],[401,0]]`).
pub const POP_RATE: [(f32, f32); 4] = [(0.0, 0.0), (0.2, 60.0), (0.4, 60.0), (0.401, 0.0)];
/// Stars it bears: the floor of the track's integral, 18.03.
pub const POP_STARS: u8 = 18;
pub const POP_SPEED: f32 = 2.6;
pub const POP_LIFE: f32 = 0.6;
pub const POP_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [88, 0, 248]),
    (0.5, [255, 60, 60]),
    (1.0, [98, 181, 239]),
];
pub const POP_ALPHA: [(f32, f32); 2] = [(0.0, 1.0), (1.0, 0.0)];
/// The bench's `scale`: the star's RADIUS, yards.
pub const POP_RADIUS: [(f32, f32); 3] = [(0.0, 0.02), (0.5, 0.12), (1.0, 0.03)];

// ── The palms: magic_precast_hand.m2 ─────────────────────────────────────────

/// The glows' size pulse: 1.15 -> 0.95 -> 1.15 on a 533 ms loop.
pub const PALM_PULSE_SECS: f32 = 0.533;
pub const PALM_PULSE: [(f32, f32); 3] = [(0.0, 1.15), (0.266, 0.95), (0.533, 1.15)];
/// The two glows: full width before the pulse, colour, alpha.
pub const PALM_OUTER: (f32, [u8; 3], f32) = (0.74, [150, 90, 240], 0.7);
pub const PALM_INNER: (f32, [u8; 3], f32) = (0.4, [120, 220, 255], 0.6);
/// Motes per second on each palm: half the bench's 21/s (departure 3).
pub const PALM_MOTE_RATE: f32 = 10.5;
pub const PALM_MOTE_LIFE: f32 = 0.6;
pub const PALM_MOTE_SPEED: f32 = 0.19;
/// The box a palm's motes are born in: `(across, tall)` yards, half the
/// bench's `area` 0.9 and its whole `areaY` 0.1 (departure 3).
pub const PALM_MOTE_BOX: (f32, f32) = (0.45, 0.1);
pub const PALM_MOTE_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 255, 255]),
    (0.5, [200, 160, 255]),
    (1.0, [162, 42, 231]),
];
pub const PALM_MOTE_ALPHA: [(f32, f32); 3] = [(0.0, 0.25), (0.5, 1.0), (1.0, 0.04)];
/// The bench's `scale`: the mote's RADIUS, yards.
pub const PALM_MOTE_RADIUS: [(f32, f32); 3] = [(0.0, 0.03), (0.5, 0.14), (1.0, 0.3)];
/// A palm's pool: enough that a slot is free when its next mote is born
/// (`ceil(rate × life)`).
pub const PALM_MOTE_SLOTS: u8 = 7;

/// The two hands: the main hand (`+1`) and the off hand (`-1`).
pub const FOCUS_HAND_SIDES: [f32; 2] = [1.0, -1.0];

// ── Births ───────────────────────────────────────────────────────────────────

/// The birth times, seconds, of an emitter whose rate follows the
/// piecewise-linear `track`: the `k`-th is born where the rate's integral
/// first reaches `k` — the bench's per-frame accumulator, in continuous time,
/// so the count and every birth are the same at any frame rate.
pub fn track_births(track: &[(f32, f32)]) -> Vec<f32> {
    let mut births = Vec::new();
    let mut done = 0.0_f64;
    for pair in track.windows(2) {
        let (t0, r0) = (pair[0].0 as f64, pair[0].1 as f64);
        let (t1, r1) = (pair[1].0 as f64, pair[1].1 as f64);
        let span = t1 - t0;
        if span <= 0.0 {
            continue;
        }
        let slope = (r1 - r0) / span;
        let total = done + 0.5 * (r0 + r1) * span;
        let mut k = births.len() as f64 + 1.0;
        while k <= total + 1e-9 {
            let need = k - done;
            let s = if slope.abs() < 1e-9 {
                need / r0
            } else {
                (-r0 + (r0 * r0 + 2.0 * slope * need).max(0.0).sqrt()) / slope
            };
            births.push((t0 + s.clamp(0.0, span)) as f32);
            k += 1.0;
        }
        done = total;
    }
    births
}

/// The pop's star `i` (0-based): its birth, seconds after the cast.
pub fn pop_birth(i: u8) -> Option<f32> {
    track_births(&POP_RATE).get(i as usize).copied()
}

/// The particle in `slot` of a pool of `slots` filled at a steady `rate`
/// whose particles live `life`, `age` seconds after the onset: its birth index
/// `j` and its own age, or `None` while the slot is empty. Particle `j` is
/// born at `(j + 1) / rate` (the bench's accumulator: the first once a whole
/// particle is owed) and takes slot `j mod slots`.
pub fn pool_particle(slot: u8, slots: u8, rate: f32, life: f32, age: f32) -> Option<(u32, f32)> {
    let born = (age.max(0.0) * rate).floor() as u32;
    let (slot, slots) = (slot as u32, slots as u32);
    if born <= slot {
        return None;
    }
    let last = born - 1;
    let j = last - (last - slot) % slots;
    let page = age - (j + 1) as f32 / rate;
    (0.0..life).contains(&page).then_some((j, page.max(0.0)))
}

// ── Poses ────────────────────────────────────────────────────────────────────

/// One piece's look at a moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FocusPose {
    /// Where it truly is, from its anchor (the chest, or the palm).
    pub at: Vec3,
    /// The sprite's full width, yards.
    pub size: f32,
    pub rgb: [u8; 3],
    pub alpha: f32,
    /// Its roll in the camera's plane, radians.
    pub roll: f32,
}

/// A colour track at `t`, interpolated in sRGB bytes and truncated, as the
/// bench's canvas does (`rgb()`'s `|0`).
pub fn focus_track_rgb(track: &[(f32, [u8; 3])], t: f32) -> [u8; 3] {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = track.iter().map(|(t, v)| (*t, v[c] as f32)).collect();
        keyed(&keys, t).floor().clamp(0.0, 255.0) as u8
    };
    [channel(0), channel(1), channel(2)]
}

/// The bench's scatter for particle `j` of layer `layer`, draw `k`: 0..1.
fn scatter(layer: u32, j: u32, k: u32) -> f32 {
    bench_hash(
        j.wrapping_mul(5)
            .wrapping_add(layer)
            .wrapping_mul(8)
            .wrapping_add(k)
            .wrapping_add(352),
    )
}

/// The bench's `vRange` 3.14: a random point of the cube `[-1, 1]³`,
/// normalised.
fn sphere_dir(layer: u32, j: u32) -> Vec3 {
    Vec3::new(
        scatter(layer, j, 1) * 2.0 - 1.0,
        scatter(layer, j, 2) * 2.0 - 1.0,
        scatter(layer, j, 3) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y)
}

/// One drawn piece of the flash.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum FlashPiece {
    /// Comet `comet`'s head ([`COMET_CORNERS`] order).
    CometHead { comet: u8 },
    /// Glow `dot` (1..=5) of its trail.
    CometTrail { comet: u8, dot: u8 },
    /// The pop's star `star` (0..18).
    Pop { star: u8 },
}

/// What a piece's sprite is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusSprite {
    /// The bench's `sprite('star')`.
    Star,
    /// The bench's `sprite('glow')`.
    Glow,
    /// The bench's `bbGlow` panel.
    Panel,
}

impl FlashPiece {
    pub fn sprite(self) -> FocusSprite {
        match self {
            FlashPiece::CometHead { .. } | FlashPiece::Pop { .. } => FocusSprite::Star,
            FlashPiece::CometTrail { .. } => FocusSprite::Glow,
        }
    }

    /// How the bench composites it: `customDraw`'s `sprite()` calls are
    /// `source-over`, its emitter `lighter`.
    pub fn alpha_mode(self) -> AlphaMode {
        match self {
            FlashPiece::CometHead { .. } | FlashPiece::CometTrail { .. } => AlphaMode::Blend,
            FlashPiece::Pop { .. } => AlphaMode::Add,
        }
    }
}

/// The flash's pieces, by its layer toggles.
pub fn flash_pieces() -> Vec<FlashPiece> {
    let mut pieces = Vec::new();
    if INNER_FOCUS_COMETS {
        for comet in 0..COMET_CORNERS.len() as u8 {
            pieces.push(FlashPiece::CometHead { comet });
            for dot in 1..=COMET_TRAIL_DOTS {
                pieces.push(FlashPiece::CometTrail { comet, dot });
            }
        }
    }
    if INNER_FOCUS_SPARK_POP {
        pieces.extend((0..POP_STARS).map(|star| FlashPiece::Pop { star }));
    }
    pieces
}

/// The flash's whole life: until its last comet has faded and its last star
/// has lived out its life.
pub fn focus_flash_secs() -> f32 {
    let last_star = pop_birth(POP_STARS - 1).unwrap_or(0.0) + POP_LIFE;
    (COMET_FLIGHT_SECS + COMET_FADE_SECS).max(last_star)
}

/// How far through its flight a comet is `age` seconds in.
pub fn comet_progress(age: f32) -> f32 {
    (age / COMET_FLIGHT_SECS).clamp(0.0, 1.0)
}

/// The comets' weight `age` seconds in: full through the flight, fading out
/// over the half second after, nothing from 900 ms on.
pub fn comet_weight(age: f32) -> f32 {
    if !(0.0..COMET_FLIGHT_SECS + COMET_FADE_SECS).contains(&age) {
        0.0
    } else if age < COMET_FLIGHT_SECS {
        1.0
    } else {
        (1.0 - (age - COMET_FLIGHT_SECS) / COMET_FADE_SECS).max(0.0)
    }
}

/// Where comet `comet` is `u` of the way through its flight, in its plane:
/// `(across, up)` yards from the chest.
pub fn comet_point(comet: u8, u: f32) -> Vec2 {
    let (sx, sy) = COMET_CORNERS[comet as usize];
    let lerp = |s: f32| COMET_START * s + (-COMET_END * s - COMET_START * s) * u;
    Vec2::new(lerp(sx), lerp(sy))
}

/// A flash piece's pose `age` seconds after the cast, from the chest, for a
/// Priest whose level facing is `forward` and whose right is `right`. `None`
/// when it draws nothing. Pure: the probes and the placement read one
/// definition.
pub fn flash_piece_pose(
    piece: FlashPiece,
    age: f32,
    right: Vec3,
    forward: Vec3,
) -> Option<FocusPose> {
    let size_mul = INNER_FOCUS_SIZE_MUL;
    let op = INNER_FOCUS_OPACITY_MUL;
    let in_plane = |p: Vec2| right * p.x + Vec3::Y * p.y + forward * COMET_FORWARD;
    match piece {
        FlashPiece::CometHead { comet } => {
            let a = comet_weight(age);
            if a <= 0.0 {
                return None;
            }
            let u = comet_progress(age);
            Some(FocusPose {
                at: in_plane(comet_point(comet, u)),
                size: COMET_HEAD_WIDTH * (1.0 - u * COMET_HEAD_SHRINK) * size_mul,
                rgb: COMET_HEAD_RGB,
                alpha: a * op,
                roll: 0.0,
            })
        }
        FlashPiece::CometTrail { comet, dot } => {
            let a = comet_weight(age);
            if a <= 0.0 {
                return None;
            }
            let u = (comet_progress(age) - dot as f32 * COMET_TRAIL_STEP).max(0.0);
            Some(FocusPose {
                at: in_plane(comet_point(comet, u)),
                size: COMET_TRAIL_WIDTH * size_mul,
                rgb: COMET_TRAIL_RGB,
                alpha: a * op * (1.0 - dot as f32 / (COMET_TRAIL_DOTS + 1) as f32),
                roll: 0.0,
            })
        }
        FlashPiece::Pop { star } => {
            let page = age - pop_birth(star)?;
            if !(0.0..POP_LIFE).contains(&page) {
                return None;
            }
            let t = page / POP_LIFE;
            let j = star as u32;
            Some(FocusPose {
                at: sphere_dir(0, j) * POP_SPEED * page,
                size: 2.0 * keyed(&POP_RADIUS, t) * size_mul,
                rgb: focus_track_rgb(&POP_RGB, t),
                alpha: keyed(&POP_ALPHA, t) * op,
                // The canvas turns a sprite clockwise on screen.
                roll: -scatter(0, j, 7) * TAU,
            })
        }
    }
}

/// One drawn piece of a palm.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum PalmPiece {
    /// The 0.74 yd violet glow.
    Outer,
    /// The 0.4 yd cyan glow.
    Inner,
    /// One slot of the palm's mote pool.
    Mote { slot: u8 },
}

impl PalmPiece {
    pub fn sprite(self) -> FocusSprite {
        match self {
            PalmPiece::Outer | PalmPiece::Inner => FocusSprite::Panel,
            PalmPiece::Mote { .. } => FocusSprite::Glow,
        }
    }

    /// Every palm call the bench makes is `lighter`.
    pub fn alpha_mode(self) -> AlphaMode {
        AlphaMode::Add
    }
}

/// A palm's pieces, by its layer toggles.
pub fn palm_pieces() -> Vec<PalmPiece> {
    let mut pieces = Vec::new();
    if INNER_FOCUS_PALM_GLOW {
        pieces.extend([PalmPiece::Outer, PalmPiece::Inner]);
    }
    if INNER_FOCUS_PALM_MOTES {
        pieces.extend((0..PALM_MOTE_SLOTS).map(|slot| PalmPiece::Mote { slot }));
    }
    pieces
}

/// The palms' size pulse `age` seconds into the state.
pub fn palm_pulse(age: f32) -> f32 {
    keyed(&PALM_PULSE, age.max(0.0).rem_euclid(PALM_PULSE_SECS))
}

/// A palm piece's pose on `hand` (0 main, 1 off) `age` seconds into the
/// state, from the palm. `None` while a mote slot is empty. Pure.
pub fn palm_piece_pose(piece: PalmPiece, age: f32, hand: u32) -> Option<FocusPose> {
    let size_mul = INNER_FOCUS_SIZE_MUL;
    let op = INNER_FOCUS_OPACITY_MUL;
    let glow = |(width, rgb, alpha): (f32, [u8; 3], f32)| FocusPose {
        at: Vec3::ZERO,
        size: width * palm_pulse(age) * size_mul,
        rgb,
        alpha: alpha * op,
        roll: 0.0,
    };
    match piece {
        PalmPiece::Outer => Some(glow(PALM_OUTER)),
        PalmPiece::Inner => Some(glow(PALM_INNER)),
        PalmPiece::Mote { slot } => {
            let (j, page) =
                pool_particle(slot, PALM_MOTE_SLOTS, PALM_MOTE_RATE, PALM_MOTE_LIFE, age)?;
            let layer = 1 + hand;
            let (across, tall) = PALM_MOTE_BOX;
            let born = Vec3::new(
                (scatter(layer, j, 4) - 0.5) * across,
                (scatter(layer, j, 5) - 0.5) * tall,
                (scatter(layer, j, 6) - 0.5) * across,
            );
            let t = page / PALM_MOTE_LIFE;
            Some(FocusPose {
                at: born + sphere_dir(layer, j) * PALM_MOTE_SPEED * page,
                size: 2.0 * keyed(&PALM_MOTE_RADIUS, t) * size_mul,
                rgb: focus_track_rgb(&PALM_MOTE_RGB, t),
                alpha: keyed(&PALM_MOTE_ALPHA, t) * op,
                roll: -scatter(layer, j, 7) * TAU,
            })
        }
    }
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Whether a unit holds Inner Focus.
pub fn holds_inner_focus(auras: &ActiveAuras) -> bool {
    auras
        .auras
        .iter()
        .any(|a| a.effect_type == AuraType::NextCast && a.ability_name == INNER_FOCUS_AURA)
}

/// Whether the palm glow on `priest`'s `side` hand yields to a live cast-hand
/// rig of that Priest on that hand, so the two never draw on one hand at
/// once.
pub fn palm_yields<'a>(
    priest: Entity,
    side: f32,
    hands: impl IntoIterator<Item = &'a HealCastHand>,
) -> bool {
    hands
        .into_iter()
        .any(|hand| hand.caster == priest && hand.side == side)
}

// ── Components ───────────────────────────────────────────────────────────────

/// One Inner Focus flash at a Priest's chest, parent of its pieces.
#[derive(Component, Debug)]
pub struct InnerFocusFlash {
    pub priest: Entity,
    pub age: f32,
}

/// One piece of an [`InnerFocusFlash`].
#[derive(Component, Debug)]
pub struct InnerFocusFlashPiece {
    pub piece: FlashPiece,
}

/// One Priest's palm state: a rig at its feet, parent of its two palms. Lives
/// exactly as long as the Priest holds Inner Focus alive.
#[derive(Component, Debug)]
pub struct InnerFocusPalms {
    pub priest: Entity,
    /// Seconds since the state rose.
    pub age: f32,
}

/// One palm of an [`InnerFocusPalms`] rig, parent of its pieces.
#[derive(Component, Debug)]
pub struct InnerFocusPalm {
    /// `+1` the main hand, `-1` the off hand ([`FOCUS_HAND_SIDES`]).
    pub side: f32,
}

impl InnerFocusPalm {
    /// The hand's index for the scatter: 0 main, 1 off.
    pub fn index(&self) -> u32 {
        u32::from(self.side < 0.0)
    }
}

/// One piece of an [`InnerFocusPalm`].
#[derive(Component, Debug)]
pub struct InnerFocusPalmPiece {
    pub piece: PalmPiece,
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// The quad and the sprites every piece draws with, built once.
pub struct InnerFocusAssets {
    quad: Handle<Mesh>,
    star: Handle<Image>,
    glow: Handle<Image>,
    panel: Handle<Image>,
}

impl InnerFocusAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            star: images.add(star_flash_texture()),
            glow: images.add(particle_sprite_image(GLOW_SOFT)),
            panel: images.add(oneshot_glow_texture()),
        }
    }

    fn texture(&self, sprite: FocusSprite) -> Handle<Image> {
        match sprite {
            FocusSprite::Star => self.star.clone(),
            FocusSprite::Glow => self.glow.clone(),
            FocusSprite::Panel => self.panel.clone(),
        }
    }
}

/// An unlit material for one piece: the bench draws colour × sprite × alpha,
/// added or blended, and nothing else.
fn piece_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Handle<Image>,
    alpha_mode: AlphaMode,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::WHITE.with_alpha(0.0),
        base_color_texture: Some(texture),
        unlit: true,
        alpha_mode,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

fn hidden(sheep: bool) -> Visibility {
    if sheep {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): for every Inner Focus the sim landed, flash the
/// Priest's arcane hands and raise the flash at its chest. Only this
/// landing's markers are consumed; a Priest that is gone gets nothing.
#[allow(clippy::too_many_arguments)]
pub fn spawn_inner_focus_flashes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<InnerFocusAssets>>,
    mut hand_assets: Local<Option<HealCastAssets>>,
    landings: Query<(Entity, &AuraLanding)>,
    priests: Query<Has<PolymorphedVisual>, With<Combatant>>,
    casters: Query<&Children>,
    bodies: Query<(), With<VisualBody>>,
) {
    for (marker, landing) in landings.iter() {
        if landing.kind != AuraLandingKind::InnerFocusFlash {
            continue;
        }
        commands.entity(marker).despawn();
        let Ok(sheep) = priests.get(landing.target) else {
            continue;
        };
        flash_instant_hands(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut hand_assets,
            &casters,
            &bodies,
            landing.target,
            crate::states::play_match::abilities::AbilityType::InnerFocus,
        );
        let assets =
            assets.get_or_insert_with(|| InnerFocusAssets::build(&mut meshes, &mut images));
        let rig = commands
            .spawn((
                InnerFocusFlash {
                    priest: landing.target,
                    age: 0.0,
                },
                Transform::default(),
                hidden(sheep),
                PlayMatchEntity,
            ))
            .id();
        for piece in flash_pieces() {
            let material = piece_material(
                &mut materials,
                assets.texture(piece.sprite()),
                piece.alpha_mode(),
            );
            let child = commands
                .spawn((
                    InnerFocusFlashPiece { piece },
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(1e-3)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(rig).add_child(child);
        }
    }
}

/// Update (graphical-only): age each flash, hide it while its Priest is a
/// sheep, and retire it after its life or at once when its Priest is gone.
pub fn update_inner_focus_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut rigs: Query<(Entity, &mut InnerFocusFlash, &mut Visibility)>,
    priests: Query<Has<PolymorphedVisual>, With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (entity, mut flash, mut visibility) in rigs.iter_mut() {
        let Ok(sheep) = priests.get(flash.priest) else {
            commands.entity(entity).despawn();
            continue;
        };
        flash.age += dt;
        if flash.age >= focus_flash_secs() {
            commands.entity(entity).despawn();
            continue;
        }
        visibility.set_if_neq(hidden(sheep));
    }
}

/// Update (graphical-only), after the cast hands' own update: raise a palm
/// state on every living Priest holding Inner Focus, age each, hide it while
/// its Priest is a sheep, hide each palm while a cast-hand rig of its Priest
/// is live on that hand ([`palm_yields`]), and take it down when the buff is
/// spent or purged or its Priest dies or is gone.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_inner_focus_palms(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<InnerFocusAssets>>,
    bearers: Query<(Entity, &Combatant, &ActiveAuras, Has<PolymorphedVisual>)>,
    cast_hands: Query<&HealCastHand>,
    mut rigs: Query<(Entity, &mut InnerFocusPalms, &mut Visibility, &Children)>,
    mut palms: Query<(&InnerFocusPalm, &mut Visibility), Without<InnerFocusPalms>>,
) {
    let dt = time.delta_secs();
    let held = |entity: Entity| {
        bearers
            .get(entity)
            .ok()
            .filter(|(_, c, auras, _)| c.is_alive() && holds_inner_focus(auras))
            .map(|(_, _, _, sheep)| sheep)
    };
    let palm_visibility = |priest: Entity, side: f32| {
        if palm_yields(priest, side, cast_hands.iter()) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        }
    };
    let mut worn = Vec::new();
    for (entity, mut rig, mut visibility, children) in rigs.iter_mut() {
        let Some(sheep) = held(rig.priest) else {
            commands.entity(entity).despawn();
            continue;
        };
        rig.age += dt;
        visibility.set_if_neq(hidden(sheep));
        for child in children.iter() {
            if let Ok((palm, mut palm_vis)) = palms.get_mut(child) {
                palm_vis.set_if_neq(palm_visibility(rig.priest, palm.side));
            }
        }
        worn.push(rig.priest);
    }
    for (entity, _, _, _) in bearers.iter() {
        let Some(sheep) = held(entity) else {
            continue;
        };
        if worn.contains(&entity) {
            continue;
        }
        let assets =
            assets.get_or_insert_with(|| InnerFocusAssets::build(&mut meshes, &mut images));
        let rig = commands
            .spawn((
                InnerFocusPalms {
                    priest: entity,
                    age: 0.0,
                },
                Transform::default(),
                hidden(sheep),
                PlayMatchEntity,
            ))
            .id();
        for side in FOCUS_HAND_SIDES {
            let palm = commands
                .spawn((
                    InnerFocusPalm { side },
                    Transform::default(),
                    palm_visibility(entity, side),
                ))
                .id();
            commands.entity(rig).add_child(palm);
            for piece in palm_pieces() {
                let material = piece_material(
                    &mut materials,
                    assets.texture(piece.sprite()),
                    piece.alpha_mode(),
                );
                let child = commands
                    .spawn((
                        InnerFocusPalmPiece { piece },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(material),
                        Transform::from_scale(Vec3::splat(1e-3)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(palm).add_child(child);
            }
        }
    }
}

/// Write a posed piece onto its entity: local to a parent at `parent` (an
/// unrotated rig), held, turned to the camera, sized and coloured. A piece
/// that draws nothing is shrunk to nothing and cleared.
fn write_piece(
    pose: Option<FocusPose>,
    anchor: Vec3,
    parent: Vec3,
    capsule: BodyCapsule,
    view: Option<HoldView>,
    transform: &mut Transform,
    material: &Handle<StandardMaterial>,
    materials: &mut Assets<StandardMaterial>,
) {
    let (rgb, alpha) = match pose {
        Some(p) => {
            let (drawn, k) = held_in(view, anchor + p.at, capsule);
            transform.translation = drawn - parent;
            transform.rotation = match view {
                Some(v) => v.rotation * Quat::from_rotation_z(p.roll),
                None => Quat::from_rotation_z(p.roll),
            };
            transform.scale = Vec3::new(p.size * k, p.size * k, 1.0).max(Vec3::splat(1e-3));
            (p.rgb, p.alpha.clamp(0.0, 1.0))
        }
        None => {
            transform.scale = Vec3::splat(1e-3);
            ([255, 255, 255], 0.0)
        }
    };
    let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).with_alpha(alpha);
    // Only touch the asset when the look moved: `get_mut` marks it changed
    // and re-uploads it.
    if materials
        .get(material)
        .is_some_and(|m| m.base_color != color)
    {
        if let Some(m) = materials.get_mut(material) {
            m.base_color = color;
        }
    }
}

/// A combatant's level facing (its local +Z) and right (its local +X).
fn level_axes(unit: &Transform) -> (Vec3, Vec3) {
    let level = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize_or_zero();
    (
        level(unit.rotation * Vec3::Z),
        level(unit.rotation * Vec3::X),
    )
}

/// Where a Priest's `side` palm stands in the world: the spell-hand socket on
/// its rendered body (`body`, its `VisualBody`'s local transform).
pub fn focus_palm_point(priest: &Transform, body: &Transform, side: f32) -> Vec3 {
    priest.transform_point(body.transform_point(spell_hand_local(side)))
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each flash at its Priest's RENDERED chest and
/// each palm on its rendered spell hand, and pose, hold, turn and colour every
/// piece. The rigs are unrotated, so a piece's local pose is its world
/// offset.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn place_inner_focus(
    hold: BodyHold,
    mut flashes: Query<
        (&InnerFocusFlash, &mut Transform, &Children),
        (
            Without<InnerFocusPalms>,
            Without<InnerFocusPalm>,
            Without<InnerFocusFlashPiece>,
            Without<InnerFocusPalmPiece>,
            HoldDisjoint,
        ),
    >,
    mut rigs: Query<
        (&InnerFocusPalms, &mut Transform, &Children),
        (
            Without<InnerFocusFlash>,
            Without<InnerFocusPalm>,
            Without<InnerFocusFlashPiece>,
            Without<InnerFocusPalmPiece>,
            HoldDisjoint,
        ),
    >,
    mut palms: Query<
        (&InnerFocusPalm, &mut Transform, &Children),
        (
            Without<InnerFocusFlash>,
            Without<InnerFocusPalms>,
            Without<InnerFocusFlashPiece>,
            Without<InnerFocusPalmPiece>,
            HoldDisjoint,
        ),
    >,
    mut flash_pieces: Query<
        (
            &InnerFocusFlashPiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<InnerFocusFlash>,
            Without<InnerFocusPalms>,
            Without<InnerFocusPalm>,
            Without<InnerFocusPalmPiece>,
            HoldDisjoint,
        ),
    >,
    mut palm_pieces: Query<
        (
            &InnerFocusPalmPiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<InnerFocusFlash>,
            Without<InnerFocusPalms>,
            Without<InnerFocusPalm>,
            Without<InnerFocusFlashPiece>,
            HoldDisjoint,
        ),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let view = hold.view();
    // A unit's transform, its rendered root and its body's local transform.
    let unit_of = |unit: Entity| {
        let body = hold.body(unit)?;
        Some((body.unit, body.root, body.local))
    };
    for (flash, mut rig_transform, children) in flashes.iter_mut() {
        let Some((unit, root, _)) = unit_of(flash.priest) else {
            continue;
        };
        let chest = body_point(FOCUS_CHEST, root, false);
        rig_transform.translation = chest;
        let capsule = body_capsule(root, false);
        let (forward, right) = level_axes(&unit);
        for child in children.iter() {
            let Ok((piece, mut transform, material)) = flash_pieces.get_mut(child) else {
                continue;
            };
            let pose = flash_piece_pose(piece.piece, flash.age, right, forward);
            write_piece(
                pose,
                chest,
                chest,
                capsule,
                view,
                &mut transform,
                &material.0,
                &mut materials,
            );
        }
    }
    for (rig, mut rig_transform, rig_children) in rigs.iter_mut() {
        let Some((unit, root, local)) = unit_of(rig.priest) else {
            continue;
        };
        rig_transform.translation = unit.translation;
        let capsule = body_capsule(root, false);
        for palm_entity in rig_children.iter() {
            let Ok((palm, mut palm_transform, palm_children)) = palms.get_mut(palm_entity) else {
                continue;
            };
            let at = focus_palm_point(&unit, &local, palm.side);
            palm_transform.translation = at - unit.translation;
            for child in palm_children.iter() {
                let Ok((piece, mut transform, material)) = palm_pieces.get_mut(child) else {
                    continue;
                };
                let pose = palm_piece_pose(piece.piece, rig.age, palm.index());
                write_piece(
                    pose,
                    at,
                    at,
                    capsule,
                    view,
                    &mut transform,
                    &material.0,
                    &mut materials,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants block, as the signed-off bench exports it.
    #[test]
    fn the_constants_are_the_benchs() {
        assert_eq!(INNER_FOCUS_SIZE_MUL, 1.0);
        assert_eq!(INNER_FOCUS_OPACITY_MUL, 1.0);
        assert_eq!(FOCUS_CHEST, 1.45);
    }

    /// The pop's track integrates to 18.03: 18 stars, six over the ramp (at
    /// `sqrt(k / 150)`), twelve over the plateau, the last at 400 ms.
    #[test]
    fn the_pop_bears_eighteen_stars_on_the_benchs_track() {
        let births = track_births(&POP_RATE);
        assert_eq!(births.len(), POP_STARS as usize);
        for (i, b) in births.iter().enumerate() {
            let k = (i + 1) as f32;
            let want = if k <= 6.0 {
                (k / 150.0).sqrt()
            } else {
                0.2 + (k - 6.0) / 60.0
            };
            assert!((b - want).abs() < 1e-5, "star {i}: {b} vs {want}");
        }
        assert!((focus_flash_secs() - 1.0).abs() < 1e-5);
    }

    /// A palm's pool always outnumbers what it can hold alive.
    #[test]
    fn the_palm_pool_outnumbers_its_live_motes() {
        assert!(PALM_MOTE_SLOTS as f32 >= PALM_MOTE_RATE * PALM_MOTE_LIFE);
        assert_eq!(
            PALM_MOTE_SLOTS as f32,
            (PALM_MOTE_RATE * PALM_MOTE_LIFE).ceil()
        );
    }

    /// A slot holds its latest birth, never one past its life.
    #[test]
    fn a_pool_slot_holds_its_latest_birth() {
        assert_eq!(pool_particle(0, 7, 10.5, 0.6, 0.05), None);
        let (j, page) = pool_particle(0, 7, 10.5, 0.6, 0.1).unwrap();
        assert_eq!(j, 0);
        assert!((page - (0.1 - 1.0 / 10.5)).abs() < 1e-6);
        for k in 0..3000 {
            let age = k as f32 * 0.0071;
            for slot in 0..7 {
                if let Some((j, page)) = pool_particle(slot, 7, 10.5, 0.6, age) {
                    assert_eq!(j % 7, slot as u32);
                    assert!((0.0..0.6).contains(&page));
                }
            }
        }
    }
}
