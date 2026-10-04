use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::view::NoFrustumCulling;
use std::collections::{HashMap, HashSet};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

use super::school_impact::{IMPACT_PET_BODY_Y, IMPACT_PET_STATURE};
use super::spell_bolts::{soft_band_texture, soft_dot_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Druid heals over time and Mark of the Wild — Rejuvenation's ribbon swirl,
// Lifebloom's head pulse and gold bloom, Mark of the Wild's paw glyph
// ==============================================================================
//
// From the Classic Era client data (build 1.15.9.69547), joined in
// `docs/design/2026-10-03-druid-client-data.md`, and ported from the signed-off
// AS-160 Druid Visuals Bench (`docs/design/benches/2026-10-03-druid-visuals-bench.html`):
//
// - **Rejuvenation** (`rejuvenation_impact_base.m2`, kit 56) plays ONCE when the
//   aura lands: five green ribbons orbit the body for 3 s, with two flare-spark
//   emitters and a faint white starburst at the core. The client has no
//   sustained state for it, so its ticks draw nothing (Rulings 1).
// - **Lifebloom** (`lifebloom_state.m2`, kit 6966) is the Druid's one sustained
//   ally channel: a burst of green streaks over the head for 100–600 ms of each
//   1834 ms loop, and star flashes for 0–967 ms. It lives exactly as long as
//   the aura does on a living bearer, one pulse per caster's Lifebloom.
// - **The bloom** (`lifebloom_impact.m2`, kit 6965): four gold layers at the
//   chest — a swelling glow, a shockwave sphere, rising star streaks and
//   drifting stars — when the aura blooms.
// - **Mark of the Wild** (`markofwild_impact_head.m2`, kit 542): two crossed
//   0.7 yd plates of the client's own paw texture (`spells/agility_128.blp`,
//   decoded to `assets/textures/effects/agility_128.png`), red-orange and gold,
//   1.03 yd above the head for 0.667 s.
//
// The emitter tables below are the bench's layer records, verbatim: the bench's
// `keys()` tracks (normalised life for colour, alpha and scale; milliseconds of
// the loop for rate and window), its cone/sphere launch, drag and tail
// streaks. The bench's own stand-ins are approved shapes and are what is drawn:
// the ribbon orbit is the bench's, and a ribbon is a stretched BAND, never a
// row of round sprites.
//
// Spawning: the landing and the bloom ride the sim's own markers
// (`AuraLanding` from `apply_pending_auras`, `BloomBurst` from
// `process_blooms`); the Lifebloom pulse is detected here off `ActiveAuras`.
// Scatter is a deterministic hash (never `game_rng`), nothing here writes sim
// state, and every system is registered in `states/mod.rs` only.

// ── Bench sign-off constants (AS-160, 2026-10-03) ─────────────────────────────

// Rejuvenation landing — rejuvenation_impact_base.m2 (kit 56), AS-160 bench
pub const REJUV_LANDING_SECS: f32 = 3.00;
pub const REJUV_RIBBONS: bool = true;
pub const REJUV_RIBBON_ORBIT_REV_PER_SEC: f32 = 1.00;
pub const REJUV_RIBBON_ORBIT_RADIUS: f32 = 0.75;
pub const REJUV_RIBBON_WIDTH: f32 = 0.278;
pub const REJUV_RIBBON_TRAIL_SECS: f32 = 0.45;
pub const REJUV_FLARE_LARGE: bool = true;
pub const REJUV_FLARE_SMALL: bool = true;
pub const REJUV_STARBURST: bool = true;
pub const REJUV_SIZE_MUL: f32 = 1.00;
pub const REJUV_DENSITY_MUL: f32 = 1.00;
// HoT ticks: no visual (client has no (7,8) state) — `HotVisual::for_hot`.

// Lifebloom sustained state — lifebloom_state.m2 (kit 6966), AS-160 bench
pub const LIFEBLOOM_STATE_LOOP_MS: u32 = 1834;
pub const LIFEBLOOM_STATE_STREAKS: bool = true;
pub const LIFEBLOOM_STATE_FLASHES: bool = true;
pub const LIFEBLOOM_STATE_SIZE_MUL: f32 = 1.00;
pub const LIFEBLOOM_STATE_SPREAD_MUL: f32 = 1.00;
pub const LIFEBLOOM_STATE_LIFE_MUL: f32 = 1.00;
pub const LIFEBLOOM_STATE_DENSITY_MUL: f32 = 1.00;
pub const LIFEBLOOM_STATE_OPACITY: f32 = 1.00;
/// The client draws one state however many stacks there are.
pub const LIFEBLOOM_STATE_SCALES_WITH_STACKS: bool = false;

// Lifebloom bloom — lifebloom_impact.m2 (kit 6965), AS-160 bench
pub const BLOOM_SECS: f32 = 1.87;
pub const BLOOM_LAYERS: [bool; 4] = [true, true, true, true]; // glow, shockwave, streaks, drift stars
pub const BLOOM_SIZE_MUL: f32 = 1.00;
pub const BLOOM_SPREAD_MUL: f32 = 1.00;
pub const BLOOM_DENSITY_MUL: f32 = 1.00;
pub const BLOOM_OPACITY: f32 = 1.00;

// Mark of the Wild — markofwild_impact_head.m2 (kit 542), AS-160 bench
pub const MOTW_GLYPH_SIZE: f32 = 0.70;
pub const MOTW_GLYPH_HEIGHT_ABOVE_HEAD: f32 = 1.03;
pub const MOTW_GLYPH_SECS: f32 = 0.667;
pub const MOTW_PLATES: [bool; 2] = [true, true]; // red-orange, gold
pub const MOTW_OPACITY: f32 = 1.00;

/// The decoded client texture the glyph plates wear: `spells/agility_128.blp`
/// (CASC fdid 165558), a white paw on black. Opaque, so it is drawn additively:
/// the black adds nothing.
pub const MOTW_GLYPH_TEXTURE: &str = "textures/effects/agility_128.png";

// ── Bench geometry ───────────────────────────────────────────────────────────

/// The bench's anchor heights above the floor (`ANCH`): chest and head. The
/// bench body is the combatant capsule — centre 1.0 above the floor, crown at
/// 2.25 — so these are the game's own heights.
pub const DRUID_CHEST_ABOVE_FLOOR: f32 = 1.45;
pub const DRUID_HEAD_ABOVE_FLOOR: f32 = 2.25;
/// A combatant's transform sits this far above the floor (the capsule centre).
pub const BODY_CENTRE_ABOVE_FLOOR: f32 = 1.0;

/// The five ribbons' heights above the floor and their greens (the client's
/// five colour tracks), as the bench draws them.
pub const REJUV_RIBBON_HEIGHTS: [f32; 5] = [0.7, 1.0, 1.35, 1.7, 2.0];
pub const REJUV_RIBBON_COLORS: [[u8; 3]; 5] = [
    [0, 172, 49],
    [70, 91, 13],
    [156, 228, 54],
    [93, 196, 12],
    [24, 128, 0],
];
/// The ribbons' texture alpha (`crystalball.blp`, alpha 0.5).
pub const REJUV_RIBBON_ALPHA: f32 = 0.5;
/// How many segments a ribbon's trail is drawn with (the bench's `steps`).
pub const REJUV_RIBBON_SEGMENTS: usize = 14;
/// The ribbons' vertical bob: amplitude and rate (`0.12 * sin(t*3 + i)`).
const REJUV_RIBBON_BOB: f32 = 0.12;
const REJUV_RIBBON_BOB_RATE: f32 = 3.0;
/// The ribbons' fade in and out, seconds.
const REJUV_RIBBON_FADE_IN: f32 = 0.15;
const REJUV_RIBBON_FADE_OUT: f32 = 0.40;

/// The glyph plates' colours: red-orange and gold.
pub const MOTW_PLATE_COLORS: [[u8; 3]; 2] = [[223, 52, 0], [255, 201, 0]];

// ── Rendering constants ──────────────────────────────────────────────────────

/// Emissive gain of an additive particle over its colour — the house's 2-4x
/// glow convention (`hunter_shots.rs` uses the same).
const PARTICLE_GLOW: f32 = 2.5;
const RIBBON_GLOW: f32 = 2.2;
const PLATE_GLOW: f32 = 2.2;
/// Steps in a particle's colour/alpha palette. The bench's tracks key at 0.5
/// and 0.8 of a life; sixteen steps resolve both.
const PALETTE_STEPS: usize = 16;
/// A streak is drawn only above this speed, yd/s (the bench's `sp > 0.2`).
const STREAK_MIN_SPEED: f32 = 0.2;

// ── The emitter model (the bench's `Emitter`) ────────────────────────────────

/// What a particle's sprite is drawn with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DruidSprite {
    /// The bench's `glow`: a soft round light.
    Glow,
    /// The bench's `star`: a four-point star with a hot centre.
    Star,
}

/// A particle rate: one value, or a track keyed in milliseconds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DruidRate {
    Flat(f32),
    Track(&'static [(f32, f32)]),
}

/// One emitter, as the bench's layer record carries it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DruidEmitter {
    /// Unique key; the palette cache is keyed on it.
    pub name: &'static str,
    pub sprite: DruidSprite,
    /// Birth height above the floor: the bench anchor plus the record's z.
    pub height: f32,
    /// Horizontal offset from the bearer's axis (the record's x and y).
    pub offset: Vec2,
    /// Side of the horizontal square the particle is born in.
    pub area: f32,
    /// Launch speed, yd/s.
    pub speed: f32,
    /// Cone half-angle about straight up, radians; `>= 3.1` is a full sphere.
    pub v_range: f32,
    pub life: f32,
    pub rate: DruidRate,
    /// On/off keys in milliseconds (`windowOn`); `None` is always on.
    pub window: Option<&'static [(f32, bool)]>,
    /// The loop the rate and window repeat over, ms; `None` plays once.
    pub loop_ms: Option<f32>,
    /// Colour, alpha and scale tracks over the particle's normalised life.
    pub color: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// Sprite RADIUS, yards.
    pub scale: &'static [(f32, f32)],
    pub drag: f32,
    /// Tail length factor; `0.0` draws no streak.
    pub tail: f32,
    /// Sprite spin, revolutions per second.
    pub spin: f32,
    /// Particles move with the bearer.
    pub follow: bool,
}

const WHITE: [u8; 3] = [255, 255, 255];
const GOLD_RAMP: &[(f32, [u8; 3])] = &[(0.0, [255, 156, 0]), (0.5, [254, 255, 156]), (1.0, WHITE)];
const FLASH_ALPHA: &[(f32, f32)] = &[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)];
const FLASH_SCALE: &[(f32, f32)] = &[(0.0, 0.0), (0.5, 0.139), (1.0, 0.0)];
const FIRST_SECOND: &[(f32, bool)] = &[(0.0, true), (1000.0, false)];

/// `rejuvenation_impact_base.m2`'s three core emitters: P0 and P2 flare sparks,
/// P1 the faint starburst.
pub const REJUV_EMITTERS: [DruidEmitter; 3] = [
    DruidEmitter {
        name: "rejuvenation/P0 flare (large)",
        sprite: DruidSprite::Star,
        height: DRUID_CHEST_ABOVE_FLOOR - 0.36,
        offset: Vec2::ZERO,
        area: 0.9,
        speed: 0.0,
        v_range: 3.14,
        life: 0.3,
        rate: DruidRate::Flat(20.0),
        window: None,
        loop_ms: None,
        color: &[
            (0.0, [0, 84, 20]),
            (0.5, [253, 253, 253]),
            (1.0, [89, 219, 26]),
        ],
        alpha: &[(0.0, 0.39), (0.5, 1.0), (1.0, 0.39)],
        scale: &[(0.0, 0.278), (1.0, 0.278)],
        drag: 0.0,
        tail: 0.0,
        spin: 0.0,
        follow: false,
    },
    DruidEmitter {
        name: "rejuvenation/P2 flare (small)",
        sprite: DruidSprite::Star,
        height: DRUID_CHEST_ABOVE_FLOOR - 0.3,
        offset: Vec2::ZERO,
        area: 0.9,
        speed: 0.0,
        v_range: 3.14,
        life: 0.3,
        rate: DruidRate::Flat(20.0),
        window: None,
        loop_ms: None,
        color: &[
            (0.0, [0, 84, 20]),
            (0.5, [253, 253, 40]),
            (1.0, [141, 219, 103]),
        ],
        alpha: &[(0.0, 0.39), (0.5, 1.0), (1.0, 0.39)],
        scale: &[(0.0, 0.139), (1.0, 0.139)],
        drag: 0.0,
        tail: 0.0,
        spin: 0.0,
        follow: false,
    },
    DruidEmitter {
        name: "rejuvenation/P1 starburst",
        sprite: DruidSprite::Glow,
        height: DRUID_CHEST_ABOVE_FLOOR - 0.32,
        offset: Vec2::ZERO,
        area: 0.0,
        speed: 0.0,
        v_range: 0.0,
        life: 0.2,
        rate: DruidRate::Flat(28.8),
        window: None,
        loop_ms: None,
        color: &[(0.0, WHITE), (1.0, WHITE)],
        alpha: &[(0.0, 0.0), (0.5, 0.22), (1.0, 0.0)],
        scale: &[(0.0, 0.972), (0.5, 1.389), (1.0, 0.972)],
        drag: 0.0,
        tail: 0.0,
        spin: 0.0,
        follow: false,
    },
];
const REJUV_LAYERS: [bool; 3] = [REJUV_FLARE_LARGE, REJUV_FLARE_SMALL, REJUV_STARBURST];

/// `lifebloom_state.m2`'s two emitters on its 1834 ms loop.
pub const LIFEBLOOM_EMITTERS: [DruidEmitter; 2] = [
    DruidEmitter {
        name: "lifebloom_state/P0 green streaks",
        sprite: DruidSprite::Glow,
        height: DRUID_HEAD_ABOVE_FLOOR + 0.54 - 0.25,
        offset: Vec2::ZERO,
        area: 0.0,
        speed: 1.111,
        v_range: 0.35,
        life: 0.6,
        rate: DruidRate::Track(&[(0.0, 15.0), (100.0, 20.0), (600.0, 5.0), (1834.0, 15.0)]),
        window: Some(&[(0.0, false), (100.0, true), (600.0, false)]),
        loop_ms: Some(LIFEBLOOM_STATE_LOOP_MS as f32),
        color: &[(0.0, [22, 185, 0]), (0.8, WHITE), (1.0, [252, 255, 156])],
        alpha: &[(0.0, 0.13), (0.8, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.083), (0.8, 0.014), (1.0, 0.0)],
        drag: 0.5,
        tail: 1.85,
        spin: 0.0,
        follow: true,
    },
    DruidEmitter {
        name: "lifebloom_state/P1 star flashes",
        sprite: DruidSprite::Star,
        height: DRUID_HEAD_ABOVE_FLOOR - 0.13 - 0.25,
        offset: Vec2::new(-0.16, -0.02),
        area: 0.0,
        speed: 3.333,
        v_range: 0.6,
        life: 0.5,
        rate: DruidRate::Flat(10.0),
        window: Some(&[(0.0, true), (967.0, false)]),
        loop_ms: Some(LIFEBLOOM_STATE_LOOP_MS as f32),
        color: &[(0.0, [0, 213, 85]), (0.5, [254, 255, 156]), (1.0, WHITE)],
        alpha: FLASH_ALPHA,
        scale: FLASH_SCALE,
        drag: 0.0,
        tail: 0.8,
        spin: 0.05,
        follow: true,
    },
];
const LIFEBLOOM_LAYERS: [bool; 2] = [LIFEBLOOM_STATE_STREAKS, LIFEBLOOM_STATE_FLASHES];

/// `lifebloom_impact.m2`'s four gold emitters at the chest.
pub const BLOOM_EMITTERS: [DruidEmitter; 4] = [
    DruidEmitter {
        name: "lifebloom_impact/P0 swelling glow",
        sprite: DruidSprite::Glow,
        height: DRUID_CHEST_ABOVE_FLOOR + 0.05,
        offset: Vec2::ZERO,
        area: 0.0,
        speed: 0.0,
        v_range: 0.0,
        life: 0.9,
        rate: DruidRate::Track(&[(0.0, 15.0), (100.0, 20.0), (633.0, 5.0), (1867.0, 5.0)]),
        window: Some(&[(0.0, false), (100.0, true), (633.0, false)]),
        loop_ms: None,
        color: &[(0.0, [255, 156, 0]), (0.8, WHITE), (1.0, [252, 255, 156])],
        alpha: &[(0.0, 0.0), (0.8, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.228), (0.8, 0.883), (1.0, 0.783)],
        drag: 0.5,
        tail: 0.0,
        spin: 0.0,
        follow: false,
    },
    DruidEmitter {
        name: "lifebloom_impact/P1 shockwave sphere",
        sprite: DruidSprite::Glow,
        height: DRUID_CHEST_ABOVE_FLOOR + 0.12,
        offset: Vec2::new(0.02, 0.0),
        area: 0.0,
        speed: 3.333,
        v_range: 3.14,
        life: 1.0,
        rate: DruidRate::Flat(10.0),
        window: Some(FIRST_SECOND),
        loop_ms: None,
        color: GOLD_RAMP,
        alpha: FLASH_ALPHA,
        scale: FLASH_SCALE,
        drag: 0.0,
        tail: 1.75,
        spin: 0.0,
        follow: false,
    },
    DruidEmitter {
        name: "lifebloom_impact/P2 rising star streaks",
        sprite: DruidSprite::Star,
        height: DRUID_CHEST_ABOVE_FLOOR,
        offset: Vec2::new(0.02, 0.0),
        area: 0.0,
        speed: 3.333,
        v_range: 0.15,
        life: 1.0,
        rate: DruidRate::Flat(10.0),
        window: Some(FIRST_SECOND),
        loop_ms: None,
        color: GOLD_RAMP,
        alpha: FLASH_ALPHA,
        scale: FLASH_SCALE,
        drag: 0.0,
        tail: 3.4,
        spin: -0.05,
        follow: false,
    },
    DruidEmitter {
        name: "lifebloom_impact/P3 drifting stars",
        sprite: DruidSprite::Star,
        height: DRUID_CHEST_ABOVE_FLOOR + 0.47,
        offset: Vec2::ZERO,
        area: 0.0,
        speed: 1.111,
        v_range: 3.14,
        life: 1.25,
        rate: DruidRate::Flat(16.0),
        window: Some(FIRST_SECOND),
        loop_ms: None,
        color: GOLD_RAMP,
        alpha: FLASH_ALPHA,
        scale: FLASH_SCALE,
        drag: 0.0,
        tail: 2.05,
        spin: -0.05,
        follow: false,
    },
];

/// How one effect plays its emitters: which are on, and the bench's knobs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct EffectKnobs {
    pub size: f32,
    pub spread: f32,
    /// Stretches particle lives (and, for a one-shot, its windows).
    pub time: f32,
    pub density: f32,
    pub opacity: f32,
}

impl DruidEffect {
    /// The emitters this effect plays (empty for the glyph).
    pub fn emitters(self) -> &'static [DruidEmitter] {
        match self {
            DruidEffect::RejuvenationSwirl => &REJUV_EMITTERS,
            DruidEffect::LifebloomPulse => &LIFEBLOOM_EMITTERS,
            DruidEffect::LifebloomBloom => &BLOOM_EMITTERS,
            DruidEffect::MarkOfTheWildGlyph => &[],
        }
    }

    /// Whether emitter `i` is a kept layer.
    pub fn layer_on(self, i: usize) -> bool {
        match self {
            DruidEffect::RejuvenationSwirl => REJUV_LAYERS[i],
            DruidEffect::LifebloomPulse => LIFEBLOOM_LAYERS[i],
            DruidEffect::LifebloomBloom => BLOOM_LAYERS[i],
            DruidEffect::MarkOfTheWildGlyph => false,
        }
    }

    pub fn knobs(self) -> EffectKnobs {
        match self {
            DruidEffect::RejuvenationSwirl => EffectKnobs {
                size: REJUV_SIZE_MUL,
                spread: 1.0,
                time: REJUV_LANDING_SECS / 3.0,
                density: REJUV_DENSITY_MUL,
                opacity: 1.0,
            },
            DruidEffect::LifebloomPulse => EffectKnobs {
                size: LIFEBLOOM_STATE_SIZE_MUL,
                spread: LIFEBLOOM_STATE_SPREAD_MUL,
                time: LIFEBLOOM_STATE_LIFE_MUL,
                density: LIFEBLOOM_STATE_DENSITY_MUL,
                opacity: LIFEBLOOM_STATE_OPACITY,
            },
            DruidEffect::LifebloomBloom => EffectKnobs {
                size: BLOOM_SIZE_MUL,
                spread: BLOOM_SPREAD_MUL,
                time: BLOOM_SECS / 1.867,
                density: BLOOM_DENSITY_MUL,
                opacity: BLOOM_OPACITY,
            },
            DruidEffect::MarkOfTheWildGlyph => EffectKnobs {
                size: MOTW_GLYPH_SIZE / 0.7,
                spread: 1.0,
                time: MOTW_GLYPH_SECS / 0.667,
                density: 1.0,
                opacity: MOTW_OPACITY,
            },
        }
    }

    /// How long a one-shot's emitters run, seconds. A pulse loops for as long
    /// as its aura lives, so it has none.
    pub fn emit_secs(self) -> Option<f32> {
        match self {
            DruidEffect::RejuvenationSwirl => Some(REJUV_LANDING_SECS),
            // The last window closes at 1000 ms of the 1867 ms model.
            DruidEffect::LifebloomBloom => Some(1.0 * self.knobs().time),
            DruidEffect::MarkOfTheWildGlyph => Some(MOTW_GLYPH_SECS),
            DruidEffect::LifebloomPulse => None,
        }
    }

    /// A one-shot's whole life: its emit window plus its longest particle, so
    /// the rig never retires a particle mid-flight. `None` for the pulse.
    pub fn one_shot_life(self) -> Option<f32> {
        let tail = self
            .emitters()
            .iter()
            .map(|e| e.life * self.knobs().time)
            .fold(0.0_f32, f32::max);
        self.emit_secs().map(|secs| secs + tail)
    }
}

impl From<AuraLandingKind> for DruidEffect {
    fn from(kind: AuraLandingKind) -> Self {
        match kind {
            AuraLandingKind::RejuvenationSwirl => DruidEffect::RejuvenationSwirl,
            AuraLandingKind::MarkOfTheWildGlyph => DruidEffect::MarkOfTheWildGlyph,
        }
    }
}

impl From<BloomVisual> for DruidEffect {
    fn from(kind: BloomVisual) -> Self {
        match kind {
            BloomVisual::GoldBurst => DruidEffect::LifebloomBloom,
        }
    }
}

// ── The bench's helpers, ported ──────────────────────────────────────────────

/// The bench's `keys()`: piecewise-linear over `(t, value)` keys, held at the
/// ends.
pub fn keyed(keys: &[(f32, f32)], t: f32) -> f32 {
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for pair in keys.windows(2) {
        let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
        if t <= t1 {
            let span = t1 - t0;
            let u = if span > 0.0 { (t - t0) / span } else { 1.0 };
            return v0 + (v1 - v0) * u;
        }
    }
    keys[keys.len() - 1].1
}

/// [`keyed`] over colour keys.
pub fn keyed_color(keys: &[(f32, [u8; 3])], t: f32) -> Color {
    let channel = |c: usize| {
        let ks: Vec<(f32, f32)> = keys.iter().map(|(t, v)| (*t, v[c] as f32)).collect();
        keyed(&ks, t) / 255.0
    };
    Color::srgb(channel(0), channel(1), channel(2))
}

/// The bench's `windowOn`: the value of the last key at or before `ms`.
pub fn window_on(window: Option<&[(f32, bool)]>, ms: f32) -> bool {
    let Some(keys) = window else {
        return true;
    };
    let mut on = keys[0].1;
    for (t, v) in keys {
        if ms >= *t {
            on = *v;
        }
    }
    on
}

impl DruidEmitter {
    /// Particles per second at `ms` into the effect (or its loop).
    pub fn rate_at(&self, ms: f32) -> f32 {
        match self.rate {
            DruidRate::Flat(r) => r,
            DruidRate::Track(keys) => keyed(keys, ms),
        }
    }

    /// Whether the emitter is emitting at `ms` into the effect: inside its
    /// loop position, inside its window.
    pub fn emitting_at(&self, ms: f32) -> bool {
        let at = match self.loop_ms {
            Some(period) => ms.rem_euclid(period),
            None => ms,
        };
        window_on(self.window, at)
    }

    fn loop_position(&self, ms: f32) -> f32 {
        match self.loop_ms {
            Some(period) => ms.rem_euclid(period),
            None => ms,
        }
    }
}

/// Deterministic 0..1 scatter. Visual only — never `game_rng`.
fn jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

fn draw(seed: u32, k: u32) -> f32 {
    jitter(seed ^ k.wrapping_mul(0x9E37_79B9).wrapping_add(k))
}

/// Where a bench height above the floor sits for a bearer at `translation`.
/// A pet's rendered body hangs below its sim transform and is about half a
/// combatant, so the same stature correction every other effect applies is
/// applied here (the AS-14 boar lesson).
pub fn body_point(height_above_floor: f32, translation: Vec3, is_pet: bool) -> Vec3 {
    let local = height_above_floor - BODY_CENTRE_ABOVE_FLOOR;
    let y = if is_pet {
        IMPACT_PET_BODY_Y + local * IMPACT_PET_STATURE
    } else {
        local
    };
    translation + Vec3::Y * y
}

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// Where a particle is born and how it launches (the bench's `spawn`).
pub fn particle_launch(
    e: &DruidEmitter,
    knobs: EffectKnobs,
    translation: Vec3,
    is_pet: bool,
    seed: u32,
) -> (Vec3, Vec3) {
    let s = stature(is_pet);
    let pol = (draw(seed, 1) * 2.0 - 1.0) * e.v_range;
    let az = draw(seed, 2) * TAU;
    let dir = if e.v_range >= 3.1 {
        // A uniform direction on the sphere.
        let u = draw(seed, 3) * 2.0 - 1.0;
        let r = (1.0 - u * u).max(0.0).sqrt();
        Vec3::new(r * az.cos(), u, r * az.sin())
    } else {
        Vec3::new(pol.sin() * az.cos(), pol.cos(), pol.sin() * az.sin())
    };
    let area = e.area * knobs.spread * s;
    let at = body_point(e.height, translation, is_pet)
        + Vec3::new(
            e.offset.x * knobs.spread * s + (draw(seed, 4) - 0.5) * area,
            0.0,
            e.offset.y * knobs.spread * s + (draw(seed, 5) - 0.5) * area,
        );
    (at, dir * e.speed * knobs.spread * s)
}

/// The bench's drag: `v *= max(0, 1 - drag*dt*3)`.
pub fn drag_factor(drag: f32, dt: f32) -> f32 {
    (1.0 - drag * dt * 3.0).max(0.0)
}

/// Length of a particle's streak behind it, yards (the bench's
/// `min(tail*0.12, speed*0.06) * size`), or `None` when it draws none.
pub fn streak_length(e: &DruidEmitter, speed: f32, size: f32) -> Option<f32> {
    (e.tail > 0.0 && speed > STREAK_MIN_SPEED).then(|| (e.tail * 0.12).min(speed * 0.06) * size)
}

/// A Rejuvenation ribbon's point `seconds` into the swirl, around a bearer at
/// `translation` (the bench's ribbon `customDraw`).
pub fn ribbon_point(index: usize, seconds: f32, translation: Vec3, is_pet: bool) -> Vec3 {
    let dir = if index % 2 == 1 { 1.0 } else { -1.0 };
    let angle = seconds * REJUV_RIBBON_ORBIT_REV_PER_SEC * TAU * dir + index as f32 * TAU / 5.0;
    let radius = REJUV_RIBBON_ORBIT_RADIUS * stature(is_pet);
    let height = REJUV_RIBBON_HEIGHTS[index]
        + REJUV_RIBBON_BOB * (seconds * REJUV_RIBBON_BOB_RATE + index as f32).sin();
    let centre = body_point(height, translation, is_pet);
    centre + Vec3::new(radius * angle.cos(), 0.0, radius * angle.sin())
}

/// The swirl's fade at `seconds` (the bench: 150 ms in, 400 ms out).
pub fn ribbon_fade(seconds: f32) -> f32 {
    if !(0.0..=REJUV_LANDING_SECS).contains(&seconds) {
        return 0.0;
    }
    (seconds / REJUV_RIBBON_FADE_IN).min(1.0)
        * ((REJUV_LANDING_SECS - seconds) / REJUV_RIBBON_FADE_OUT).min(1.0)
}

/// The points a ribbon's trail runs through at `seconds`, newest first: the
/// bench's `REJUV_RIBBON_SEGMENTS + 1` samples over the last
/// `REJUV_RIBBON_TRAIL_SECS`, stopping where the trail reaches the landing.
pub fn ribbon_trail(index: usize, seconds: f32, translation: Vec3, is_pet: bool) -> Vec<Vec3> {
    (0..=REJUV_RIBBON_SEGMENTS)
        .map(|k| seconds - REJUV_RIBBON_TRAIL_SECS * k as f32 / REJUV_RIBBON_SEGMENTS as f32)
        .take_while(|t| *t >= 0.0)
        .map(|t| ribbon_point(index, t, translation, is_pet))
        .collect()
}

/// A glyph plate's alpha `seconds` into the glyph: the client's texture
/// weight `0 -> 0.5 @167 ms -> 0 @667 ms`, doubled as the bench draws it, and
/// for the gold plate gone by 500 ms.
pub fn glyph_alpha(layer: usize, seconds: f32) -> f32 {
    let ms = seconds * 1000.0 * 0.667 / MOTW_GLYPH_SECS;
    if !(0.0..667.0).contains(&ms) {
        return 0.0;
    }
    let weight = match layer {
        0 => keyed(&[(0.0, 0.0), (167.0, 0.5), (667.0, 0.0)], ms),
        _ => keyed(&[(0.0, 0.0), (167.0, 0.5), (500.0, 0.0), (667.0, 0.0)], ms),
    };
    weight * 2.0 * MOTW_OPACITY
}

/// The glyph's centre for a bearer at `translation`: 1.03 yd above the crown.
pub fn glyph_centre(translation: Vec3, is_pet: bool) -> Vec3 {
    body_point(
        DRUID_HEAD_ABOVE_FLOOR + MOTW_GLYPH_HEIGHT_ABOVE_HEAD,
        translation,
        is_pet,
    )
}

/// Does this bearer carry a Lifebloom from `caster`?
pub fn has_lifebloom_from(auras: Option<&ActiveAuras>, caster: Option<Entity>) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|au| is_lifebloom(au) && au.caster == caster)
    })
}

fn is_lifebloom(aura: &Aura) -> bool {
    aura.effect_type == AuraType::HealingOverTime
        && HotVisual::for_hot(&aura.ability_name) == Some(HotVisual::SustainedPulse)
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// Sprites, palettes and the paw texture, built once per system, lazily.
pub struct DruidAssets {
    quad: Handle<Mesh>,
    dot: Handle<Image>,
    star: Handle<Image>,
    band: Handle<Image>,
    paw: Handle<Image>,
    /// `(sprite palette, streak palette)` per emitter name.
    palettes: HashMap<
        &'static str,
        (
            Arc<[Handle<StandardMaterial>]>,
            Arc<[Handle<StandardMaterial>]>,
        ),
    >,
}

impl DruidAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        asset_server: &AssetServer,
    ) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            dot: images.add(soft_dot_texture()),
            star: images.add(star_flash_texture()),
            band: images.add(soft_band_texture()),
            paw: asset_server.load(MOTW_GLYPH_TEXTURE),
            palettes: HashMap::new(),
        }
    }

    fn palettes(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        e: &DruidEmitter,
        opacity: f32,
    ) -> (
        Arc<[Handle<StandardMaterial>]>,
        Arc<[Handle<StandardMaterial>]>,
    ) {
        if let Some(p) = self.palettes.get(e.name) {
            return p.clone();
        }
        let texture = match e.sprite {
            DruidSprite::Glow => self.dot.clone(),
            DruidSprite::Star => self.star.clone(),
        };
        let build =
            |materials: &mut Assets<StandardMaterial>, texture: &Handle<Image>, gain: f32| {
                (0..PALETTE_STEPS)
                    .map(|i| {
                        let t = (i as f32 + 0.5) / PALETTE_STEPS as f32;
                        let color = keyed_color(e.color, t);
                        let alpha = (keyed(e.alpha, t) * opacity * gain).clamp(0.0, 1.0);
                        additive(
                            materials,
                            color,
                            alpha,
                            PARTICLE_GLOW,
                            Some(texture.clone()),
                        )
                    })
                    .collect::<Arc<[_]>>()
            };
        let sprite = build(materials, &texture, 1.0);
        // The bench draws the streak at 0.85 of the sprite's alpha.
        let streak = build(materials, &self.band.clone(), 0.85);
        self.palettes
            .insert(e.name, (sprite.clone(), streak.clone()));
        (sprite, streak)
    }
}

/// Lit-emissive additive material: `unlit` would discard the emissive (see
/// `warlock_dots.rs`), and `AlphaMode::Add` gates the whole fragment by alpha.
fn additive(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    alpha: f32,
    gain: f32,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    let c = color.to_linear();
    materials.add(StandardMaterial {
        base_color: color.with_alpha(alpha),
        base_color_texture: texture.clone(),
        emissive: LinearRgba::rgb(c.red * gain, c.green * gain, c.blue * gain),
        emissive_texture: texture,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

fn palette_step(t: f32) -> usize {
    ((t.clamp(0.0, 1.0) * PALETTE_STEPS as f32) as usize).min(PALETTE_STEPS - 1)
}

/// An empty ribbon mesh; `animate_druid_effects` rewrites it every frame.
fn empty_ribbon_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    write_ribbon(&mut mesh, &[], Vec3::Z, 0.0);
    mesh
}

/// Rewrite a ribbon's band through `points` (newest first): a camera-facing
/// strip `REJUV_RIBBON_WIDTH` wide, vertex alpha falling from the head of the
/// ribbon to nothing at its tail — the bench's `0.5 * (1 - k/steps)` per
/// segment, a stretched band and never a row of sprites.
pub fn write_ribbon(mesh: &mut Mesh, points: &[Vec3], eye: Vec3, width: f32) {
    let n = points.len();
    let mut positions = Vec::with_capacity(n * 2);
    let mut normals = Vec::with_capacity(n * 2);
    let mut uvs = Vec::with_capacity(n * 2);
    let mut colors = Vec::with_capacity(n * 2);
    let mut indices = Vec::new();
    if n >= 2 {
        for (k, p) in points.iter().enumerate() {
            let prev = points[k.saturating_sub(1)];
            let next = points[(k + 1).min(n - 1)];
            let tangent = (prev - next).normalize_or_zero();
            let view = (eye - *p).normalize_or_zero();
            let side = tangent.cross(view).normalize_or_zero() * (width * 0.5);
            let u = k as f32 / REJUV_RIBBON_SEGMENTS as f32;
            let a = 1.0 - u;
            for (v, at) in [(0.0, *p + side), (1.0, *p - side)] {
                positions.push(at.to_array());
                normals.push(view.to_array());
                uvs.push([u, v]);
                colors.push([1.0, 1.0, 1.0, a]);
            }
        }
        for k in 0..(n - 1) as u32 {
            let b = k * 2;
            indices.extend_from_slice(&[b, b + 1, b + 3, b, b + 3, b + 2]);
        }
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): build a rig for every landing and bloom the sim
/// marked this frame, and a Lifebloom pulse for every caster's Lifebloom on a
/// living bearer that has none.
///
/// The markers are consumed here. A pulse is keyed by `(bearer, caster)`: two
/// Druids' Lifeblooms on one ally are two auras and two pulses, and a refresh
/// keeps the one pulse it already has.
#[allow(clippy::too_many_arguments)]
pub fn spawn_druid_effects(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
    mut assets: Local<Option<DruidAssets>>,
    landings: Query<(Entity, &AuraLanding)>,
    blooms: Query<(Entity, &BloomBurst)>,
    bearers: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&Pet>,
    )>,
    rigs: Query<&DruidEffectRig>,
) {
    let assets =
        assets.get_or_insert_with(|| DruidAssets::build(&mut meshes, &mut images, &asset_server));

    let mut new_rigs: Vec<(Entity, DruidEffect, Option<Entity>)> = Vec::new();
    for (marker, landing) in landings.iter() {
        commands.entity(marker).despawn();
        new_rigs.push((landing.target, landing.kind.into(), None));
    }
    for (marker, bloom) in blooms.iter() {
        commands.entity(marker).despawn();
        new_rigs.push((bloom.target, bloom.kind.into(), None));
    }

    let pulsing: HashSet<(Entity, Option<Entity>)> = rigs
        .iter()
        .filter(|r| r.effect == DruidEffect::LifebloomPulse)
        .map(|r| (r.target, r.caster))
        .collect();
    for (entity, combatant, _, auras, _) in bearers.iter() {
        if !combatant.is_alive() {
            continue;
        }
        let Some(auras) = auras else { continue };
        let mut seen = HashSet::new();
        for aura in auras.auras.iter().filter(|a| is_lifebloom(a)) {
            let key = (entity, aura.caster);
            if !pulsing.contains(&key) && seen.insert(key) {
                new_rigs.push((entity, DruidEffect::LifebloomPulse, aura.caster));
            }
        }
    }

    for (target, effect, caster) in new_rigs {
        let Ok((_, _, transform, _, pet)) = bearers.get(target) else {
            continue;
        };
        let is_pet = pet.is_some();
        let rig = commands
            .spawn((
                DruidEffectRig {
                    target,
                    effect,
                    caster,
                    age: 0.0,
                    carry: vec![0.0; effect.emitters().len()],
                    emitted: 0,
                },
                Transform::from_translation(transform.translation)
                    .with_rotation(transform.rotation),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .id();
        match effect {
            DruidEffect::RejuvenationSwirl if REJUV_RIBBONS => {
                for (index, rgb) in REJUV_RIBBON_COLORS.iter().enumerate() {
                    let mesh = meshes.add(empty_ribbon_mesh());
                    let material = additive(
                        &mut materials,
                        Color::srgb_u8(rgb[0], rgb[1], rgb[2]),
                        0.0,
                        RIBBON_GLOW,
                        Some(assets.band.clone()),
                    );
                    commands.spawn((
                        RejuvenationRibbon {
                            rig,
                            index,
                            mesh: mesh.clone(),
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        NoFrustumCulling,
                        NotShadowCaster,
                        PlayMatchEntity,
                    ));
                }
            }
            DruidEffect::MarkOfTheWildGlyph => {
                let lift = glyph_centre(transform.translation, is_pet) - transform.translation;
                let size = MOTW_GLYPH_SIZE * stature(is_pet);
                for (layer, rgb) in MOTW_PLATE_COLORS.iter().enumerate() {
                    if !MOTW_PLATES[layer] {
                        continue;
                    }
                    let material = additive(
                        &mut materials,
                        Color::srgb_u8(rgb[0], rgb[1], rgb[2]),
                        0.0,
                        PLATE_GLOW,
                        Some(assets.paw.clone()),
                    );
                    // Two crossed plates, a quarter turn apart about the
                    // vertical: one always faces the camera well enough to read.
                    let plate = commands
                        .spawn((
                            MarkOfTheWildPlate { rig, layer },
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(material),
                            Transform::from_translation(lift)
                                .with_rotation(Quat::from_rotation_y(layer as f32 * FRAC_PI_2))
                                .with_scale(Vec3::splat(size)),
                            NotShadowCaster,
                        ))
                        .id();
                    commands.entity(rig).add_child(plate);
                }
            }
            _ => {}
        }
    }
}

/// Animate (graphical-only): follow each bearer, emit each rig's particles,
/// rebuild the Rejuvenation ribbons, fade the glyph plates, and retire rigs.
///
/// A pulse retires the frame its Lifebloom stops being drawable — the aura
/// expired, was dispelled or purged (all three remove it from `ActiveAuras`,
/// or remove `ActiveAuras` itself when it was the last aura), or its bearer
/// died with it still up (death preserves auras; the corpse must not pulse).
/// A refresh keeps the aura, so it keeps the pulse.
#[allow(clippy::too_many_arguments)]
pub fn animate_druid_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
    mut assets: Local<Option<DruidAssets>>,
    mut rigs: Query<(Entity, &mut DruidEffectRig, &mut Transform)>,
    bearers: Query<
        (&Combatant, &Transform, Option<&ActiveAuras>, Option<&Pet>),
        Without<DruidEffectRig>,
    >,
    ribbons: Query<(&RejuvenationRibbon, &MeshMaterial3d<StandardMaterial>)>,
    plates: Query<(&MarkOfTheWildPlate, &MeshMaterial3d<StandardMaterial>)>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    let assets =
        assets.get_or_insert_with(|| DruidAssets::build(&mut meshes, &mut images, &asset_server));
    let eye = camera.iter().next().map(|c| c.translation());

    for (rig_entity, mut rig, mut transform) in rigs.iter_mut() {
        let bearer = bearers.get(rig.target).ok();
        // Retire.
        let retire = match rig.effect {
            DruidEffect::LifebloomPulse => !bearer.is_some_and(|(c, _, auras, _)| {
                c.is_alive() && has_lifebloom_from(auras, rig.caster)
            }),
            effect => bearer.is_none() || rig.age >= effect.one_shot_life().unwrap_or(0.0),
        };
        if retire {
            commands.entity(rig_entity).despawn();
            continue;
        }
        let (_, bearer_transform, _, pet) = bearer.expect("checked above");
        let is_pet = pet.is_some();
        let at = bearer_transform.translation;
        transform.translation = at;
        transform.rotation = bearer_transform.rotation;

        let before = rig.age;
        rig.age += dt;
        let age = rig.age;
        let effect = rig.effect;
        let knobs = effect.knobs();

        // Emit, integrating the rate over the frame so a frame-rate change
        // does not change the density.
        let emit_until = effect.emit_secs().unwrap_or(f32::INFINITY);
        for (ei, e) in effect.emitters().iter().enumerate() {
            if !effect.layer_on(ei) {
                continue;
            }
            // A one-shot's windows stretch with its time knob; a pulse's loop
            // is the client's and does not.
            let time_scale = match effect {
                DruidEffect::LifebloomPulse => 1.0,
                _ => knobs.time,
            };
            let ms = age * 1000.0 / time_scale;
            if age > emit_until || before >= emit_until || !e.emitting_at(ms) {
                continue;
            }
            let rate = e.rate_at(e.loop_position(ms)) * knobs.density;
            rig.carry[ei] += rate * dt;
            let (sprite_palette, streak_palette) =
                assets.palettes(&mut materials, e, knobs.opacity);
            while rig.carry[ei] >= 1.0 {
                rig.carry[ei] -= 1.0;
                let seed = rig_entity
                    .index()
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let (position, velocity) = particle_launch(e, knobs, at, is_pet, seed);
                let sprite = commands
                    .spawn((
                        DruidParticlePart::Sprite,
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(sprite_palette[0].clone()),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id();
                let mut parts = vec![sprite];
                if e.tail > 0.0 {
                    parts.push(
                        commands
                            .spawn((
                                DruidParticlePart::Streak,
                                Mesh3d(assets.quad.clone()),
                                MeshMaterial3d(streak_palette[0].clone()),
                                Transform::from_scale(Vec3::splat(1e-4)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
                commands
                    .spawn((
                        DruidParticle {
                            rig: rig_entity,
                            effect,
                            emitter: ei,
                            age: 0.0,
                            life: e.life * knobs.time,
                            velocity,
                            follow: e.follow.then_some((rig.target, at)),
                            angle: draw(seed, 6) * TAU,
                            stature: stature(is_pet),
                            sprite_palette: sprite_palette.clone(),
                            streak_palette: streak_palette.clone(),
                        },
                        Transform::from_translation(position),
                        Visibility::default(),
                        PlayMatchEntity,
                    ))
                    .add_children(&parts);
            }
        }

        if effect == DruidEffect::MarkOfTheWildGlyph {
            for (plate, material) in plates.iter() {
                if plate.rig != rig_entity {
                    continue;
                }
                if let Some(m) = materials.get_mut(&material.0) {
                    let a = glyph_alpha(plate.layer, age);
                    m.base_color = m.base_color.with_alpha(a);
                }
            }
        }

        if effect == DruidEffect::RejuvenationSwirl {
            let fade = ribbon_fade(age);
            for (ribbon, material) in ribbons.iter() {
                if ribbon.rig != rig_entity {
                    continue;
                }
                let points = ribbon_trail(ribbon.index, age, at, is_pet);
                let points = if fade > 0.0 { points } else { Vec::new() };
                let view_from = eye.unwrap_or(at + Vec3::new(0.0, 4.0, 10.0));
                if let Some(mesh) = meshes.get_mut(&ribbon.mesh) {
                    write_ribbon(
                        mesh,
                        &points,
                        view_from,
                        REJUV_RIBBON_WIDTH * REJUV_SIZE_MUL * stature(is_pet),
                    );
                }
                if let Some(m) = materials.get_mut(&material.0) {
                    m.base_color = m.base_color.with_alpha(REJUV_RIBBON_ALPHA * fade);
                }
            }
        }
    }
}

/// Animate (graphical-only): age, drag, move and follow every particle, step
/// its palette, and retire it at the end of its life — or with its rig, which
/// is how a pulse's particles leave with its aura.
#[allow(clippy::type_complexity)]
pub fn animate_druid_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut DruidParticle, &mut Transform)>,
    rigs: Query<(), With<DruidEffectRig>>,
    bearers: Query<&Transform, (With<Combatant>, Without<DruidParticle>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut transform) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life || rigs.get(p.rig).is_err() {
            commands.entity(entity).despawn();
            continue;
        }
        let e = p.effect.emitters()[p.emitter];
        let drag = drag_factor(e.drag, dt);
        p.velocity *= drag;
        let velocity = p.velocity;
        transform.translation += velocity * dt;
        if let Some((target, last)) = p.follow {
            if let Ok(bearer) = bearers.get(target) {
                let delta = bearer.translation - last;
                transform.translation += Vec3::new(delta.x, 0.0, delta.z);
                p.follow = Some((target, bearer.translation));
            }
        }
        p.angle += e.spin * TAU * dt;
    }
}

/// Billboard (graphical-only): turn each particle's sprite to the camera,
/// sized and coloured along its life, and lay its streak back along its
/// flight as the camera sees it.
#[allow(clippy::type_complexity)]
pub fn billboard_druid_particles(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    particles: Query<(&DruidParticle, &Children)>,
    mut parts: Query<
        (
            &DruidParticlePart,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<DruidParticle>,
    >,
) {
    let cam = camera
        .iter()
        .next()
        .map(|c| c.compute_transform().rotation)
        .unwrap_or(Quat::IDENTITY);
    for (p, children) in particles.iter() {
        let e = p.effect.emitters()[p.emitter];
        let knobs = p.effect.knobs();
        let t = (p.age / p.life).clamp(0.0, 1.0);
        let step = palette_step(t);
        let (sprite_palette, streak_palette) = (&p.sprite_palette, &p.streak_palette);
        // The scale track is a RADIUS; the quad is a unit diameter.
        let radius = keyed(e.scale, t) * knobs.size * p.stature;
        let speed = p.velocity.length();
        for child in children.iter() {
            let Ok((part, mut transform, mut material)) = parts.get_mut(child) else {
                continue;
            };
            match part {
                DruidParticlePart::Sprite => {
                    transform.rotation = cam * Quat::from_rotation_z(p.angle);
                    transform.translation = Vec3::ZERO;
                    transform.scale = Vec3::splat((radius * 2.0).max(1e-4));
                    material.0 = sprite_palette[step].clone();
                }
                DruidParticlePart::Streak => {
                    let Some(length) = streak_length(&e, speed, knobs.size * p.stature) else {
                        transform.scale = Vec3::splat(1e-4);
                        continue;
                    };
                    // Roll the camera-facing quad until its long (local X)
                    // axis lies back along the flight as the camera sees it.
                    let back = -p.velocity / speed;
                    let (right, up) = (cam * Vec3::X, cam * Vec3::Y);
                    let roll = back.dot(up).atan2(back.dot(right));
                    transform.rotation = cam * Quat::from_rotation_z(roll);
                    transform.translation = back * (length * 0.5);
                    // The bench's line is 0.7 of the sprite's radius wide.
                    transform.scale = Vec3::new(length.max(1e-4), (radius * 0.7).max(1e-4), 1.0);
                    material.0 = streak_palette[step].clone();
                }
            }
        }
    }
}

/// Cleanup (graphical-only): despawn the Rejuvenation ribbons whose swirl has
/// retired. (Glyph plates are children of their rig and leave with it;
/// particles retire themselves.)
pub fn cleanup_druid_effects(
    mut commands: Commands,
    ribbons: Query<(Entity, &RejuvenationRibbon)>,
    rigs: Query<(), With<DruidEffectRig>>,
) {
    for (entity, ribbon) in ribbons.iter() {
        if rigs.get(ribbon.rig).is_err() {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_is_the_benchs_keys() {
        let k = [(0.0, 0.0), (167.0, 0.5), (667.0, 0.0)];
        assert_eq!(keyed(&k, -1.0), 0.0);
        assert!((keyed(&k, 167.0) - 0.5).abs() < 1e-6);
        assert!((keyed(&k, 417.0) - 0.25).abs() < 1e-6);
        assert_eq!(keyed(&k, 900.0), 0.0);
    }

    #[test]
    fn window_on_is_the_benchs_window() {
        let w: &[(f32, bool)] = &[(0.0, false), (100.0, true), (600.0, false)];
        assert!(!window_on(Some(w), 50.0));
        assert!(window_on(Some(w), 100.0));
        assert!(window_on(Some(w), 599.0));
        assert!(!window_on(Some(w), 600.0));
        assert!(window_on(None, 1e6));
    }

    #[test]
    fn the_pulse_loops_on_the_clients_1834_ms() {
        let streaks = LIFEBLOOM_EMITTERS[0];
        let flashes = LIFEBLOOM_EMITTERS[1];
        for loop_start in [0.0, 1834.0, 3668.0] {
            assert!(!streaks.emitting_at(loop_start + 50.0));
            assert!(streaks.emitting_at(loop_start + 300.0));
            assert!(!streaks.emitting_at(loop_start + 700.0));
            assert!(flashes.emitting_at(loop_start + 900.0));
            assert!(!flashes.emitting_at(loop_start + 1000.0));
        }
    }

    #[test]
    fn every_emitter_has_a_unique_palette_key() {
        let mut names: Vec<&str> = [
            &REJUV_EMITTERS[..],
            &LIFEBLOOM_EMITTERS[..],
            &BLOOM_EMITTERS[..],
        ]
        .iter()
        .flat_map(|t| t.iter().map(|e| e.name))
        .collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }
}
