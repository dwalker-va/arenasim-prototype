use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::ConeAnchor;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

use super::school_impact::{impact_origin, ImpactStyle};
use super::spell_bolts::{soft_band_texture, soft_dot_texture, star_flash_texture};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ==============================================================================
// Hunter shots — the client's violet shot, Serpent Sting's green one, and the
// Auto Shot arrow
// ==============================================================================
//
// From the Classic Era client data (build 1.15.9.69547), joined in
// docs/design/2026-09-26-hunter-shot-client-data.md:
//
// - **Aimed, Arcane and Concussive Shot fire ONE missile and land ONE impact.**
//   Aimed and Concussive are literally the same `SpellVisual` row (3180);
//   Arcane Shot's (3299) points at the same models. The missile is
//   `arcaneshot_missile.m2` — a small additive violet core with four particle
//   emitters and a thin white ribbon — and the landing is
//   `magic_impact_chest.m2` at chest attachment 34.
// - **Serpent Sting** is the same missile template recoloured yellow-green, its
//   sparks smaller and dripping under gravity (`poisonshot_missile.m2`), and it
//   lands in a lingering olive cloud (`bestowdisease_impact_chest.m2`).
// - **Auto Shot is the only arrow.** The bow's missile set is sound-only; the
//   arrow is the ammo item's `arrowflight_01.m2`, a 1.52 yd shaft with the same
//   ribbon, and it has no impact model at all — the victim's wound flinch
//   (`hit_reaction.rs`) is the whole landing.
//
// So the client's grammar is: an arrow is an auto attack, and a glowing shot is
// an ability. The three shared shots stay distinguishable by other channels
// (Aimed Shot's cast and heal-refused tell, Concussive Shot's slow ring).
//
// The emitter tables below are TRANSCRIPTIONS of the parsed M2 records, as the
// AS-136 Hunter Shot Bench carries them per emitter (the doc's tables merge the
// two spark emitters and round the ramps; the bench keeps them apart). Knobs
// the user tuned in the bench sit on top of them as named constants, and the
// landing's three-of-six cut is the user's sign-off, not a data finding.
//
// Colour check against Shadow Bolt: the shot's core is close in HUE to the
// Shadow Bolt halo (`spell_bolts.rs`). They separate on MATERIAL — this is a
// bright additive core that cools to lilac and white, Shadow Bolt an opaque
// dark core in a halo — so the core here must never be darkened.
//
// Graphical-only: spawned off `Added<Projectile>` and the landed `SchoolImpact`,
// scattered by a deterministic hash (never `game_rng`), writing no sim state,
// registered in `states/mod.rs` only — headless stays byte-identical.

// ── The bench's knobs (AS-136 Hunter Shot Bench, signed off 2026-09-26) ────

/// Billboard core of the shared shot (client extent ~0.6 yd).
pub const HUNTER_SHOT_CORE_DIAMETER: f32 = 0.60;
/// Emissive gain on the core. The core is additive and must stay bright — see
/// the Shadow Bolt colour check in the module note.
pub const HUNTER_SHOT_CORE_BRIGHTNESS: f32 = 1.0;
/// Multipliers over the client's per-emitter rates and sizes (1.0 = client).
pub const HUNTER_SHOT_RATE_SCALE: f32 = 1.0;
pub const HUNTER_SHOT_SIZE_SCALE: f32 = 1.0;

/// The white ribbon every Hunter missile and the arrow share (client: 0.25 /
/// 0.11 / 0.2).
pub const HUNTER_TRAIL_ALPHA: f32 = 0.25;
pub const HUNTER_TRAIL_HEIGHT: f32 = 0.11;
pub const HUNTER_TRAIL_LIFETIME: f32 = 0.20;

/// The shared landing's scale over the client's sizes. A uniformly scaled
/// model scales its spawn areas and particle speeds with it, as the bench did.
pub const HUNTER_IMPACT_SCALE: f32 = 0.8;
/// Particle speed (and gravity) scale on the shared landing: how far it throws.
pub const HUNTER_IMPACT_SPREAD: f32 = 0.6;
/// Emission windows and particle lives on the shared landing (1.0 = client).
pub const HUNTER_IMPACT_TIME_SCALE: f32 = 1.0;
/// Serpent Sting's cloud plays at the client's values: no scale, no spread,
/// its two lingering emitters open for the full 3.0 s sequence.
pub const SERPENT_IMPACT_SCALE: f32 = 1.0;
pub const SERPENT_IMPACT_LINGER: f32 = 3.0;

/// Auto Shot arrow (`arrowflight_01.m2`): 1.52 yd shaft, 0.14 yd across.
pub const AUTO_ARROW_LENGTH: f32 = 1.52;
pub const AUTO_ARROW_WIDTH: f32 = 0.14;
/// Cosmetic arrow flight speed, yd/s. `BaseMissileSpeed` is 0 in the client —
/// an engine constant — so this matches the shots' RON `projectile_speed`.
pub const AUTO_ARROW_SPEED: f32 = 45.0;

// ── Rendering constants ────────────────────────────────────────────────────

/// Steps in a colour-ramp palette. Every particle of an emitter shares these
/// materials and swaps between them as it ages.
const PALETTE_STEPS: usize = 8;
/// Emissive gain of an additive particle over its ramp colour — the low end of
/// the repo's 2-4x glow convention.
const EMITTER_GLOW: f32 = 2.5;
/// The white-hot centre of the core, as a fraction of its diameter.
const CORE_HOT_FRACTION: f32 = 0.35;
/// Emissive gain of the core's two glows, before `HUNTER_SHOT_CORE_BRIGHTNESS`.
const CORE_GLOW: f32 = 3.0;
/// Ribbon segment spacing in yards, and how far each overruns it (see
/// `spell_bolts.rs` — above 1.0 consecutive segments merge into one band).
const TRAIL_STEP: f32 = 0.40;
const TRAIL_OVERLAP: f32 = 1.35;
/// How fast the flat rune turns, rad/s.
const RUNE_SPIN: f32 = 2.0;
/// Generated rune sprite, pixels on a side.
const RUNE_PX: u32 = 96;
/// Generated shockwave ring sprite, pixels on a side.
const RING_PX: u32 = 128;
/// How much longer than wide a streak particle is drawn.
const STREAK_STRETCH: f32 = 4.0;

// ── The emitter model ──────────────────────────────────────────────────────

/// What a particle is drawn with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmitterSprite {
    /// A soft round glow (`genericglow5`, `toonsmoke16`, `clouds8x8fade`).
    Glow,
    /// A star glint (`flare`, `star5a`, `stargradient64`).
    Flare,
    /// The expanding rune (`aurarune7`), lying flat in the ground plane.
    Rune,
    /// A thin shockwave ring (`shockwave8`, `whiteringthin128`), facing the
    /// camera.
    Ring,
    /// A rune disc facing the camera and turning in the view plane
    /// (`aurarune256`, Counterspell's seal).
    Seal,
    /// A soft glow stretched along the particle's own flight (the ribbon-blur
    /// and streak emitters), facing the camera.
    Streak,
}

/// One particle emitter, transcribed from its M2 record.
///
/// Ramps are keyed at birth, midlife and death, as the source keys them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ClientEmitter {
    /// Unique key: model and emitter. The palette cache is keyed on it.
    pub name: &'static str,
    pub sprite: EmitterSprite,
    /// `true` for M2 blend 4 (additive); `false` for blend 2 (alpha), which
    /// can darken the sand.
    pub additive: bool,
    /// Particles per second.
    pub rate: f32,
    /// `(start, end)` of the emission window in seconds from the hit, or
    /// `None` for an emitter that runs for as long as its missile flies.
    pub window: Option<(f32, f32)>,
    pub life: f32,
    /// Launch speed, yd/s, in a uniformly random direction (straight up for a
    /// `rise` emitter). NEGATIVE converges: the particle is born out along its
    /// direction and travels in, arriving near the centre at the end of its
    /// life.
    pub speed: f32,
    /// Downward acceleration, yd/s².
    pub gravity: f32,
    /// Side of the cube the particles are born in.
    pub area: f32,
    /// `(width, height)` of a column the particles are born in, instead of the
    /// cube.
    pub column: Option<(f32, f32)>,
    /// Launch mostly upward — smoke.
    pub rise: bool,
    pub color: [[u8; 3]; 3],
    pub alpha: [f32; 3],
    /// Diameter, yards.
    pub size: [f32; 3],
    /// Where in the life the three ramps' middle key sits, 0..1. The source
    /// keys most emitters at half-life; some earlier (0.25) or later (0.75).
    pub mid: f32,
    /// A keyed rate track, `(seconds, particles per second)` linearly
    /// interpolated, for an emitter whose rate RAMPS (Mind Blast, Holy Shock)
    /// rather than holding one `rate` across its window. When set, `rate` is
    /// ignored and `window` must span the track exactly — build both with
    /// [`ClientEmitter::tracked`] — so that scaling the window (the `time`
    /// knob) stretches the track with it.
    pub track: Option<&'static [(f32, f32)]>,
}

impl ClientEmitter {
    /// When the emission window opens.
    pub fn start(&self) -> f32 {
        self.window.map(|(s, _)| s).unwrap_or(0.0)
    }

    /// When the emission window closes; `0.0` for an always-on emitter.
    pub fn end(&self) -> f32 {
        self.window.map(|(_, e)| e).unwrap_or(0.0)
    }

    /// The window a rate `track` spans: its first key to its last.
    pub const fn tracked(track: &'static [(f32, f32)]) -> Option<(f32, f32)> {
        Some((track[0].0, track[track.len() - 1].0))
    }

    /// The constant rate that emits `count` particles across `window` — how a
    /// one-off burst is written as an emitter.
    pub const fn burst(count: f32, window: (f32, f32)) -> f32 {
        count / (window.1 - window.0)
    }

    /// How far the window has been stretched from the track's own keys.
    fn track_stretch(&self, track: &[(f32, f32)]) -> f32 {
        let keyed = track[track.len() - 1].0 - track[0].0;
        let window = self.end() - self.start();
        if keyed > 0.0 {
            window / keyed
        } else {
            1.0
        }
    }

    /// Particles emitted between `t0` and `t1` seconds after the hit — the
    /// integral of the rate over that span, clipped to the window. Integrated
    /// rather than sampled per frame, so a burst written as a short window
    /// emits its whole count whatever the frame rate.
    pub fn count_between(&self, t0: f32, t1: f32) -> f32 {
        let (start, end) = (self.start(), self.end());
        let (a, b) = (t0.max(start), t1.min(end));
        if b <= a {
            return 0.0;
        }
        let Some(track) = self.track else {
            return self.rate * (b - a);
        };
        // Integrate the piecewise-linear track in its own key time.
        let k = self.track_stretch(track);
        let (a, b) = (track[0].0 + (a - start) / k, track[0].0 + (b - start) / k);
        let mut total = 0.0;
        for pair in track.windows(2) {
            let ((x0, r0), (x1, r1)) = (pair[0], pair[1]);
            let (lo, hi) = (a.max(x0), b.min(x1));
            if hi <= lo || x1 <= x0 {
                continue;
            }
            let at = |x: f32| r0 + (r1 - r0) * (x - x0) / (x1 - x0);
            total += 0.5 * (at(lo) + at(hi)) * (hi - lo);
        }
        total * k
    }

    /// The emitter played at a different scale. Size, spawn volume and speed
    /// follow `size` (a scaled model); speed and gravity additionally follow
    /// `spread`; windows and lives follow `time`.
    pub const fn scaled(self, size: f32, spread: f32, time: f32) -> Self {
        let mut e = self;
        e.size = [e.size[0] * size, e.size[1] * size, e.size[2] * size];
        e.area *= size;
        if let Some((w, h)) = e.column {
            e.column = Some((w * size, h * size));
        }
        e.speed *= size * spread;
        e.gravity *= size * spread;
        if let Some((s, t)) = e.window {
            e.window = Some((s * time, t * time));
        }
        e.life *= time;
        e
    }
}

/// Linear interpolation over a birth / `mid` / death ramp.
fn ramp3(keys: [f32; 3], mid: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let mid = mid.clamp(1e-3, 1.0 - 1e-3);
    if t < mid {
        keys[0] + (keys[1] - keys[0]) * (t / mid)
    } else {
        keys[1] + (keys[2] - keys[1]) * ((t - mid) / (1.0 - mid))
    }
}

fn ramp_color(keys: [[u8; 3]; 3], mid: f32, t: f32) -> Color {
    let channel = |c: usize| {
        ramp3(
            [keys[0][c] as f32, keys[1][c] as f32, keys[2][c] as f32],
            mid,
            t,
        )
    };
    Color::srgb(channel(0) / 255.0, channel(1) / 255.0, channel(2) / 255.0)
}

/// The palette step a particle `t` of the way through its life shows.
fn palette_step(t: f32) -> usize {
    ((t.clamp(0.0, 1.0) * PALETTE_STEPS as f32) as usize).min(PALETTE_STEPS - 1)
}

// ── The tables ─────────────────────────────────────────────────────────────

/// `arcaneshot_missile.m2` (165592) — the shot Aimed, Arcane and Concussive
/// Shot all fire. All four emitters loop a 167 ms Stand and are always on in
/// flight.
const ARCANESHOT_MISSILE: [ClientEmitter; 4] = [
    ClientEmitter {
        name: "arcaneshot_missile/e0 puffs",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 6.7,
        window: None,
        life: 0.75,
        speed: 0.056,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[22, 34, 168], [78, 33, 201], [203, 103, 255]],
        alpha: [0.59, 0.98, 0.0],
        size: [0.142, 0.264, 0.142],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "arcaneshot_missile/e1 sparks",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 20.0,
        window: None,
        life: 1.1,
        speed: 0.333,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[77, 15, 207], [114, 59, 190], [165, 222, 255]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.083, 0.028, 0.014],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "arcaneshot_missile/e2 sparks",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 10.0,
        window: None,
        life: 1.1,
        speed: 0.333,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[84, 0, 237], [131, 94, 237], [183, 227, 255]],
        alpha: [0.39, 1.0, 0.0],
        size: [0.083, 0.028, 0.014],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "arcaneshot_missile/e3 core stream",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 100.0,
        window: None,
        life: 0.5,
        speed: 1.111,
        gravity: 0.0,
        area: 0.028,
        column: None,
        rise: false,
        color: [[0, 0, 154], [97, 67, 207], [245, 245, 236]],
        alpha: [0.39, 1.0, 0.2],
        size: [0.194, 0.139, 0.028],
        mid: 0.5,
        track: None,
    },
];

/// `poisonshot_missile.m2` (166648) — Serpent Sting. The same template: same
/// rates, lives and speeds, recoloured, with smaller sparks that drip under
/// gravity 1.39.
const POISONSHOT_MISSILE: [ClientEmitter; 4] = [
    ClientEmitter {
        name: "poisonshot_missile/e0 puffs",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 6.7,
        window: None,
        life: 0.75,
        speed: 0.056,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[119, 201, 2], [141, 207, 12], [209, 234, 92]],
        alpha: [0.19, 0.77, 0.0],
        size: [0.142, 0.264, 0.142],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "poisonshot_missile/e1 sparks",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 20.0,
        window: None,
        life: 1.1,
        speed: 0.333,
        gravity: 1.389,
        area: 0.0,
        column: None,
        rise: false,
        color: [[109, 183, 55], [144, 208, 106], [230, 255, 92]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.056, 0.028, 0.014],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "poisonshot_missile/e2 sparks",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 10.0,
        window: None,
        life: 1.1,
        speed: 0.333,
        gravity: 1.389,
        area: 0.0,
        column: None,
        rise: false,
        color: [[109, 201, 0], [161, 237, 52], [202, 255, 127]],
        alpha: [0.39, 1.0, 0.0],
        size: [0.056, 0.028, 0.014],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "poisonshot_missile/e3 core stream",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 100.0,
        window: None,
        life: 0.5,
        speed: 1.111,
        gravity: 0.0,
        area: 0.028,
        column: None,
        rise: false,
        color: [[158, 255, 64], [109, 201, 0], [91, 169, 0]],
        alpha: [0.39, 1.0, 0.2],
        size: [0.139, 0.083, 0.028],
        mid: 0.5,
        track: None,
    },
];

/// `magic_impact_chest.m2` (166525), cut to the three layers the user kept:
/// smoke (e3), stars (e4) and rune (e5). The streaks (e0), the ring (e1) and
/// the falling sparks (e2) are dropped — at the client's raw values the full
/// model read "way too exaggerated". Client values; played scaled, below.
const MAGIC_IMPACT_KEPT: [ClientEmitter; 3] = [
    ClientEmitter {
        name: "magic_impact_chest/e3 smoke",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 40.0,
        window: Some((0.0, 0.2)),
        life: 0.5,
        speed: 1.636,
        gravity: 0.0,
        area: 0.028,
        column: None,
        rise: false,
        color: [[35, 123, 233], [188, 188, 188], [145, 81, 205]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.4, 0.6, 0.828],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "magic_impact_chest/e4 stars",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 24.3,
        window: Some((0.0, 0.433)),
        life: 0.75,
        speed: 1.161,
        gravity: 0.0,
        area: 0.19,
        column: None,
        rise: false,
        color: [[35, 123, 233], [163, 55, 239], [255, 255, 255]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.15, 0.217, 0.031],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "magic_impact_chest/e5 rune",
        sprite: EmitterSprite::Rune,
        additive: true,
        rate: 21.4,
        window: Some((0.0, 0.2)),
        life: 0.4,
        speed: 0.261,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[37, 249, 82], [151, 64, 233], [255, 255, 255]],
        alpha: [0.53, 0.78, 0.0],
        size: [0.197, 0.925, 1.764],
        mid: 0.5,
        track: None,
    },
];

/// `bestowdisease_impact_chest.m2` (165679) — Serpent Sting's landing, a 3 s
/// sequence that LINGERS: glints in a column, rising alpha-blended olive smoke,
/// and a 167 ms dark-green spray.
const BESTOWDISEASE_IMPACT: [ClientEmitter; 3] = [
    ClientEmitter {
        name: "bestowdisease_impact_chest/e0 glints",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 10.0,
        window: Some((0.0, SERPENT_IMPACT_LINGER)),
        life: 1.5,
        speed: 0.0,
        gravity: 0.0,
        area: 0.0,
        column: Some((0.278, 1.111)),
        rise: false,
        color: [[0, 119, 38], [171, 214, 0], [194, 196, 0]],
        alpha: [0.0, 1.0, 0.0],
        size: [0.028, 0.556, 0.028],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "bestowdisease_impact_chest/e1 smoke",
        sprite: EmitterSprite::Glow,
        additive: false,
        rate: 20.0,
        window: Some((0.0, SERPENT_IMPACT_LINGER)),
        life: 2.5,
        speed: 0.278,
        gravity: 0.0,
        area: 0.0,
        column: Some((0.0, 0.278)),
        rise: true,
        color: [[18, 55, 0], [106, 146, 0], [163, 196, 64]],
        alpha: [0.39, 1.0, 0.0],
        size: [0.222, 0.306, 0.694],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "bestowdisease_impact_chest/e2 spray",
        sprite: EmitterSprite::Flare,
        additive: false,
        rate: 150.0,
        window: Some((0.0, 0.167)),
        life: 0.6,
        speed: 3.333,
        gravity: 0.0,
        area: 0.278,
        column: None,
        rise: false,
        color: [[49, 102, 1], [99, 136, 20], [115, 152, 1]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.111, 0.056, 0.028],
        mid: 0.5,
        track: None,
    },
];

pub(super) const fn scale_all<const N: usize>(
    table: [ClientEmitter; N],
    size: f32,
    spread: f32,
    time: f32,
) -> [ClientEmitter; N] {
    let mut out = table;
    let mut i = 0;
    while i < N {
        out[i] = table[i].scaled(size, spread, time);
        i += 1;
    }
    out
}

/// What each landing actually plays.
static SHOT_LANDING: [ClientEmitter; 3] = scale_all(
    MAGIC_IMPACT_KEPT,
    HUNTER_IMPACT_SCALE,
    HUNTER_IMPACT_SPREAD,
    HUNTER_IMPACT_TIME_SCALE,
);
static SERPENT_LANDING: [ClientEmitter; 3] =
    scale_all(BESTOWDISEASE_IMPACT, SERPENT_IMPACT_SCALE, 1.0, 1.0);
/// What each missile actually plays.
static SHOT_MISSILE: [ClientEmitter; 4] =
    scale_all(ARCANESHOT_MISSILE, HUNTER_SHOT_SIZE_SCALE, 1.0, 1.0);
static SERPENT_MISSILE: [ClientEmitter; 4] =
    scale_all(POISONSHOT_MISSILE, HUNTER_SHOT_SIZE_SCALE, 1.0, 1.0);

// ── Routing ────────────────────────────────────────────────────────────────

/// Which of the client's two Hunter shots an ability fires.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HunterShotKind {
    /// `arcaneshot_missile` + `magic_impact_chest`: Aimed, Arcane and
    /// Concussive Shot.
    Shared,
    /// `poisonshot_missile` + `bestowdisease_impact_chest`.
    SerpentSting,
}

/// The single list the missile spawner, the landing override and the probes
/// derive from. `None` for everything else, which keeps whatever missile and
/// landing it had.
pub fn hunter_shot_for(ability: AbilityType) -> Option<HunterShotKind> {
    match ability {
        AbilityType::AimedShot | AbilityType::ArcaneShot | AbilityType::ConcussiveShot => {
            Some(HunterShotKind::Shared)
        }
        AbilityType::SerpentSting => Some(HunterShotKind::SerpentSting),
        _ => None,
    }
}

impl HunterShotKind {
    /// The emitters the missile sheds in flight.
    pub fn missile_emitters(self) -> &'static [ClientEmitter; 4] {
        match self {
            HunterShotKind::Shared => &SHOT_MISSILE,
            HunterShotKind::SerpentSting => &SERPENT_MISSILE,
        }
    }

    /// The emitters the landing plays.
    pub fn landing_emitters(self) -> &'static [ClientEmitter] {
        match self {
            HunterShotKind::Shared => &SHOT_LANDING,
            HunterShotKind::SerpentSting => &SERPENT_LANDING,
        }
    }

    /// `(glow, white-hot centre)` of the missile core. The glow is the
    /// `purple_glow.blp` tint (inferred from the texture, as the bench notes);
    /// Serpent Sting's is its model colour (0.63, 1.0, 0.11).
    pub fn core_colors(self) -> (Color, Color) {
        match self {
            HunterShotKind::Shared => (Color::srgb_u8(120, 70, 235), Color::srgb_u8(255, 255, 255)),
            HunterShotKind::SerpentSting => {
                (Color::srgb_u8(161, 255, 28), Color::srgb_u8(245, 255, 220))
            }
        }
    }

    /// The landing, as the shared impact plays it. Pure emitters: no flash,
    /// ring, spray or blot — the client draws none of those here.
    pub fn landing_style(self) -> ImpactStyle {
        ImpactStyle {
            color: self.core_colors().0,
            emissive: EMITTER_GLOW,
            flash: None,
            ring: None,
            spray: None,
            blot: None,
            smoulder: None,
            emitters: self.landing_emitters(),
        }
    }
}

// ── Shared emitter machinery ───────────────────────────────────────────────

/// Deterministic 0..1 scatter. Visual only — never `game_rng`.
fn shot_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

/// The `k`th independent jitter draw for particle `seed`.
fn draw(seed: u32, k: u32) -> f32 {
    shot_jitter(seed ^ k.wrapping_mul(0x9E37_79B9).wrapping_add(k))
}

/// Where a particle is born, relative to its emitter, and how it launches.
fn particle_launch(e: &ClientEmitter, seed: u32) -> (Vec3, Vec3) {
    let offset = match e.column {
        Some((w, h)) => Vec3::new(
            (draw(seed, 1) - 0.5) * w,
            (draw(seed, 2) - 0.5) * h,
            (draw(seed, 3) - 0.5) * w,
        ),
        None => {
            Vec3::new(
                draw(seed, 1) - 0.5,
                draw(seed, 2) - 0.5,
                draw(seed, 3) - 0.5,
            ) * e.area
        }
    };
    // A uniform direction on the sphere.
    let u = draw(seed, 4) * 2.0 - 1.0;
    let theta = draw(seed, 5) * TAU;
    let r = (1.0 - u * u).max(0.0).sqrt();
    let dir = Vec3::new(r * theta.cos(), u, r * theta.sin());
    if e.speed < 0.0 {
        // Converging: born out along the direction, far enough to arrive
        // near the centre as the life runs out (0.9 of the way), and
        // travelling in.
        let reach = -e.speed * e.life * 0.9;
        return (offset + dir * reach, dir * e.speed);
    }
    let velocity = if e.rise {
        Vec3::new(dir.x * 0.2, 1.0, dir.z * 0.2) * e.speed
    } else {
        dir * e.speed
    };
    (offset, velocity)
}

/// Spawn one particle of `e` at `at` (world space for a missile, the rig's
/// frame for a landing). The caller parents an `owned` particle.
pub fn spawn_client_particle(
    commands: &mut Commands,
    quad: &Handle<Mesh>,
    e: &ClientEmitter,
    palette: &Arc<[Handle<StandardMaterial>]>,
    at: Vec3,
    seed: u32,
    owned: bool,
) -> Entity {
    let (offset, velocity) = particle_launch(e, seed);
    let facing = match e.sprite {
        EmitterSprite::Rune => ParticleFacing::Flat,
        EmitterSprite::Seal => ParticleFacing::Seal,
        EmitterSprite::Streak => ParticleFacing::Streak,
        EmitterSprite::Glow | EmitterSprite::Flare | EmitterSprite::Ring => ParticleFacing::Camera,
    };
    let mut particle = commands.spawn((
        ClientParticle {
            age: 0.0,
            life: e.life,
            velocity,
            gravity: e.gravity,
            size: e.size,
            mid: e.mid,
            palette: palette.clone(),
            step: 0,
            facing,
            owned,
        },
        Mesh3d(quad.clone()),
        MeshMaterial3d(palette[0].clone()),
        Transform::from_translation(at + offset).with_scale(Vec3::splat(e.size[0].max(1e-4))),
        NotShadowCaster,
    ));
    if !owned {
        particle.insert(PlayMatchEntity);
    }
    particle.id()
}

/// A rune: a thin ring with eight ticks inside it, white, with the whole shape
/// in the alpha channel. Generated, like every other sprite here.
fn rune_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = RUNE_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    let band = |r: f32, mid: f32, half: f32| (1.0 - ((r - mid).abs() / half)).clamp(0.0, 1.0);

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            let ring = band(r, 0.86, 0.07);
            // Eight radial ticks between r = 0.56 and 0.72.
            let a = dy.atan2(dx);
            let sector = TAU / 8.0;
            let off = (a / sector).round() * sector - a;
            let across = (off * r).abs();
            let tick = if (0.56..=0.72).contains(&r) {
                (1.0 - across / 0.045).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let alpha = ring.max(tick);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (alpha * 255.0) as u8;
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

/// A thin soft ring, white, with the shape in the alpha channel — the
/// shockwave the interrupt and Heroic Strike landings expand.
fn ring_texture() -> Image {
    use bevy::image::Image;
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = RING_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            // A bright rim with a soft falloff on the inside, as a shockwave
            // trails its front.
            let rim = (1.0 - ((r - 0.9) / 0.06).abs()).clamp(0.0, 1.0);
            let wake = if (0.62..0.9).contains(&r) {
                0.35 * ((r - 0.62) / 0.28)
            } else {
                0.0
            };
            let alpha = rim.max(wake);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (alpha * 255.0) as u8;
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

/// Sprites and the ramp palettes every client emitter draws with. Built once
/// per system, lazily; a palette is built the first time its emitter plays and
/// shared by every particle it ever emits.
pub struct EmitterAssets {
    quad: Handle<Mesh>,
    dot: Handle<Image>,
    star: Handle<Image>,
    rune: Handle<Image>,
    ring: Handle<Image>,
    palettes: HashMap<&'static str, Arc<[Handle<StandardMaterial>]>>,
}

impl EmitterAssets {
    pub fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            dot: images.add(soft_dot_texture()),
            star: images.add(star_flash_texture()),
            rune: images.add(rune_texture()),
            ring: images.add(ring_texture()),
            palettes: HashMap::new(),
        }
    }

    pub fn quad(&self) -> Handle<Mesh> {
        self.quad.clone()
    }

    /// `PALETTE_STEPS` materials sampling the emitter's colour and alpha ramps.
    pub fn palette(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        e: &ClientEmitter,
    ) -> Arc<[Handle<StandardMaterial>]> {
        if let Some(p) = self.palettes.get(e.name) {
            return p.clone();
        }
        let texture = match e.sprite {
            EmitterSprite::Glow | EmitterSprite::Streak => self.dot.clone(),
            EmitterSprite::Flare => self.star.clone(),
            EmitterSprite::Rune | EmitterSprite::Seal => self.rune.clone(),
            EmitterSprite::Ring => self.ring.clone(),
        };
        let palette: Arc<[Handle<StandardMaterial>]> = (0..PALETTE_STEPS)
            .map(|i| {
                let t = (i as f32 + 0.5) / PALETTE_STEPS as f32;
                let color = ramp_color(e.color, e.mid, t);
                let alpha = ramp3(e.alpha, e.mid, t);
                materials.add(if e.additive {
                    // The sprite drives BOTH channels, and `unlit` stays false
                    // or Bevy discards the emissive (see `spell_bolts.rs`).
                    StandardMaterial {
                        base_color: color.with_alpha(alpha),
                        base_color_texture: Some(texture.clone()),
                        emissive: emissive_of(color, EMITTER_GLOW),
                        emissive_texture: Some(texture.clone()),
                        alpha_mode: AlphaMode::Add,
                        cull_mode: None,
                        double_sided: true,
                        ..default()
                    }
                } else {
                    // Blend 2: a flat, unlit colour that can DARKEN — the
                    // olive smoke and the dark spray are this.
                    StandardMaterial {
                        base_color: color.with_alpha(alpha),
                        base_color_texture: Some(texture.clone()),
                        alpha_mode: AlphaMode::Blend,
                        unlit: true,
                        cull_mode: None,
                        double_sided: true,
                        ..default()
                    }
                })
            })
            .collect();
        self.palettes.insert(e.name, palette.clone());
        palette
    }
}

fn emissive_of(color: Color, strength: f32) -> LinearRgba {
    let c = color.to_linear();
    LinearRgba::rgb(c.red * strength, c.green * strength, c.blue * strength)
}

/// The rotation a particle needs under a parent turned by `parent`, so that a
/// camera-facing one faces `camera`, a flat one lies in the ground plane, a
/// seal turns in the view plane and a streak lies along its flight.
pub fn particle_rotation(particle: &ClientParticle, parent: Quat, camera: Quat) -> Quat {
    let world = match particle.facing {
        ParticleFacing::Camera => camera,
        // `Rectangle` faces +Z; tipped back a quarter turn it faces world up.
        ParticleFacing::Flat => {
            Quat::from_rotation_y(particle.age * RUNE_SPIN) * Quat::from_rotation_x(-FRAC_PI_2)
        }
        ParticleFacing::Seal => camera * Quat::from_rotation_z(particle.age * RUNE_SPIN),
        ParticleFacing::Streak => {
            // Roll the camera-facing quad about the view axis until its long
            // (local Y) axis lies along the flight as the camera sees it.
            let v = parent * particle.velocity;
            let (right, up) = (camera * Vec3::X, camera * Vec3::Y);
            let (sx, sy) = (v.dot(right), v.dot(up));
            if sx * sx + sy * sy > 1e-8 {
                camera * Quat::from_rotation_z((-sx).atan2(sy))
            } else {
                camera
            }
        }
    };
    parent.inverse() * world
}

/// Age, move, resize and recolour every client-emitter particle, missile and
/// landing alike, and retire the world-space ones.
///
/// A landing's particles are children of its rig and are retired with it, so
/// here a spent one is only collapsed to nothing.
pub fn animate_client_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(
        Entity,
        &mut ClientParticle,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut transform, mut material) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life {
            if p.owned {
                transform.scale = Vec3::ZERO;
            } else {
                commands.entity(entity).despawn();
            }
            continue;
        }
        p.velocity.y -= p.gravity * dt;
        let velocity = p.velocity;
        transform.translation += velocity * dt;
        let t = p.age / p.life;
        let size = ramp3(p.size, p.mid, t).max(1e-4);
        transform.scale = match p.facing {
            ParticleFacing::Streak => Vec3::new(size, size * STREAK_STRETCH, size),
            _ => Vec3::splat(size),
        };
        let step = palette_step(t);
        if step != p.step {
            p.step = step;
            material.0 = p.palette[step].clone();
        }
    }
}

// ── The missiles ───────────────────────────────────────────────────────────

/// Meshes and materials every Hunter missile shares.
pub struct HunterShotAssets {
    emitters: EmitterAssets,
    ribbon: Handle<StandardMaterial>,
    shared_core: (Handle<StandardMaterial>, Handle<StandardMaterial>),
    serpent_core: (Handle<StandardMaterial>, Handle<StandardMaterial>),
}

fn glow_material(
    materials: &mut Assets<StandardMaterial>,
    texture: &Handle<Image>,
    color: Color,
    gain: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: Some(texture.clone()),
        emissive: emissive_of(color, gain),
        emissive_texture: Some(texture.clone()),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

/// The ribbon's material: white, additive, at the client's 0.25 alpha, on the
/// soft band sprite the bolt ribbons use.
fn ribbon_material(
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Handle<StandardMaterial> {
    let band = images.add(soft_band_texture());
    materials.add(StandardMaterial {
        base_color: Color::WHITE.with_alpha(HUNTER_TRAIL_ALPHA),
        base_color_texture: Some(band.clone()),
        emissive: LinearRgba::rgb(1.5, 1.5, 1.5),
        emissive_texture: Some(band),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

impl HunterShotAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        let emitters = EmitterAssets::build(meshes, images);
        let core = |materials: &mut Assets<StandardMaterial>, kind: HunterShotKind| {
            let (glow, hot) = kind.core_colors();
            (
                glow_material(
                    materials,
                    &emitters.dot,
                    glow,
                    CORE_GLOW * HUNTER_SHOT_CORE_BRIGHTNESS,
                ),
                glow_material(
                    materials,
                    &emitters.dot,
                    hot,
                    CORE_GLOW * HUNTER_SHOT_CORE_BRIGHTNESS,
                ),
            )
        };
        let shared_core = core(materials, HunterShotKind::Shared);
        let serpent_core = core(materials, HunterShotKind::SerpentSting);
        Self {
            ribbon: ribbon_material(materials, images),
            shared_core,
            serpent_core,
            emitters,
        }
    }

    fn core(&self, kind: HunterShotKind) -> &(Handle<StandardMaterial>, Handle<StandardMaterial>) {
        match kind {
            HunterShotKind::Shared => &self.shared_core,
            HunterShotKind::SerpentSting => &self.serpent_core,
        }
    }
}

/// Build the rig on a newly spawned Hunter shot: a billboarded additive core
/// with a white-hot centre, and the emitter state its flight sheds from.
pub fn spawn_hunter_shot_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HunterShotAssets>>,
    new_projectiles: Query<(Entity, &Projectile, &Transform), Added<Projectile>>,
) {
    if new_projectiles.is_empty() {
        return;
    }
    let assets = assets
        .get_or_insert_with(|| HunterShotAssets::build(&mut meshes, &mut materials, &mut images));

    for (entity, projectile, transform) in new_projectiles.iter() {
        let Some(kind) = hunter_shot_for(projectile.ability) else {
            continue;
        };
        let palettes = kind
            .missile_emitters()
            .iter()
            .map(|e| assets.emitters.palette(&mut materials, e))
            .collect();
        commands.entity(entity).insert((
            HunterShotRig {
                kind,
                carry: [0.0; 4],
                emitted: 0,
                ribbon_carry: 0.0,
                last_pos: transform.translation,
                seed: entity.index(),
                quad: assets.emitters.quad(),
                ribbon_material: assets.ribbon.clone(),
                palettes,
            },
            Visibility::default(),
        ));

        let (glow, hot) = assets.core(kind).clone();
        for (material, diameter) in [
            (glow, HUNTER_SHOT_CORE_DIAMETER),
            (hot, HUNTER_SHOT_CORE_DIAMETER * CORE_HOT_FRACTION),
        ] {
            let sprite = commands
                .spawn((
                    HunterShotCore,
                    Mesh3d(assets.emitters.quad()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(diameter)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(entity).add_child(sprite);
        }
    }
}

/// Lay ribbon segments along the step `from -> to`, one every `TRAIL_STEP`
/// yards. Distance-driven, like the bolt ribbons, so the trail stays one
/// continuous band whatever the speed or frame rate. The segments are
/// `BoltTrail`s, so the bolt systems fade and turn them.
fn lay_ribbon(
    commands: &mut Commands,
    quad: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    carry: &mut f32,
    from: Vec3,
    to: Vec3,
) {
    let travelled = from.distance(to);
    if travelled <= 0.0 {
        return;
    }
    *carry += travelled;
    let dir = (to - from) / travelled;
    let length = TRAIL_STEP * TRAIL_OVERLAP;
    while *carry >= TRAIL_STEP {
        *carry -= TRAIL_STEP;
        let back = carry.min(travelled);
        let at = from.lerp(to, ((travelled - back) / travelled).clamp(0.0, 1.0));
        // Walk the centre back so the band's leading edge sits where it was
        // laid, not half a length ahead of the missile.
        let at = at - dir * (length * 0.5);
        commands.spawn((
            BoltTrail {
                age: 0.0,
                life: HUNTER_TRAIL_LIFETIME,
                half_width: HUNTER_TRAIL_HEIGHT * 0.5,
                length,
                dir,
            },
            Mesh3d(quad.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at).with_scale(Vec3::new(length, HUNTER_TRAIL_HEIGHT, 1.0)),
            NotShadowCaster,
            PlayMatchEntity,
        ));
    }
}

/// Shed each missile's four emitters and lay its ribbon.
///
/// Particles are born along the frame's own step, at the point the missile
/// passed when each one fell due, so a 100/s stream behind a 45 yd/s missile is
/// a continuous stream and not a clump per frame.
pub fn animate_hunter_shots(
    mut commands: Commands,
    time: Res<Time>,
    mut rigs: Query<(&mut HunterShotRig, &Transform)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut rig, transform) in rigs.iter_mut() {
        let pos = transform.translation;
        let from = rig.last_pos;
        let emitters = rig.kind.missile_emitters();
        for (ei, e) in emitters.iter().enumerate() {
            let rate = e.rate * HUNTER_SHOT_RATE_SCALE;
            rig.carry[ei] += rate * dt;
            while rig.carry[ei] >= 1.0 {
                rig.carry[ei] -= 1.0;
                // What is still owed after this one is how long ago it fell
                // due, as a fraction of the frame.
                let ago = (rig.carry[ei] / (rate * dt)).clamp(0.0, 1.0);
                let at = pos.lerp(from, ago);
                let seed = rig
                    .seed
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                spawn_client_particle(
                    &mut commands,
                    &rig.quad,
                    e,
                    &rig.palettes[ei],
                    at,
                    seed,
                    false,
                );
            }
        }
        let (quad, material) = (rig.quad.clone(), rig.ribbon_material.clone());
        lay_ribbon(
            &mut commands,
            &quad,
            &material,
            &mut rig.ribbon_carry,
            from,
            pos,
        );
        rig.last_pos = pos;
    }
}

/// Turn the missile cores and every world-space particle toward the camera.
/// A landing's particles hang off a yawed rig and are turned by
/// `billboard_school_impacts`, which cancels the rig's rotation.
pub fn billboard_hunter_shots(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<ClientParticle>,
            Without<HunterShotCore>,
            Without<HunterShotRig>,
        ),
    >,
    rigs: Query<(&Transform, &Children), With<HunterShotRig>>,
    mut cores: Query<
        &mut Transform,
        (
            With<HunterShotCore>,
            Without<Camera3d>,
            Without<HunterShotRig>,
            Without<ClientParticle>,
        ),
    >,
    mut particles: Query<
        (&ClientParticle, &mut Transform),
        (
            Without<Camera3d>,
            Without<HunterShotCore>,
            Without<HunterShotRig>,
        ),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let facing = rig.rotation.inverse() * cam.rotation;
        for child in children.iter() {
            if let Ok(mut core) = cores.get_mut(child) {
                core.rotation = facing;
            }
        }
    }
    for (particle, mut transform) in particles.iter_mut() {
        if !particle.owned {
            transform.rotation = particle_rotation(particle, Quat::IDENTITY, cam.rotation);
        }
    }
}

// ── The Auto Shot arrow ────────────────────────────────────────────────────

/// Meshes and materials of the arrow, built once.
pub struct AutoShotArrowAssets {
    shaft: Handle<Mesh>,
    head: Handle<Mesh>,
    fletch: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    feather: Handle<StandardMaterial>,
    ribbon_quad: Handle<Mesh>,
    ribbon: Handle<StandardMaterial>,
}

/// Length of the steel head, yards; the shaft makes up the rest.
const ARROW_HEAD_LEN: f32 = 0.12;
/// Length of the fletching along the shaft, yards.
const ARROW_FLETCH_LEN: f32 = 0.26;
/// Shaft thickness, yards. The 0.14 yd width is the fletching's.
const ARROW_SHAFT_THICKNESS: f32 = 0.035;

impl AutoShotArrowAssets {
    pub fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        // The source shaft is an OPAQUE textured material (blend 0): lit
        // geometry, not a glow.
        let lit = |materials: &mut Assets<StandardMaterial>, color: Color| {
            materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.7,
                ..default()
            })
        };
        Self {
            shaft: meshes.add(Cuboid::new(
                ARROW_SHAFT_THICKNESS,
                ARROW_SHAFT_THICKNESS,
                AUTO_ARROW_LENGTH - ARROW_HEAD_LEN,
            )),
            head: meshes.add(
                Cone::new(ARROW_SHAFT_THICKNESS * 1.4, ARROW_HEAD_LEN)
                    .mesh()
                    .resolution(4)
                    .anchor(ConeAnchor::Base),
            ),
            fletch: meshes.add(Cuboid::new(AUTO_ARROW_WIDTH, 0.008, ARROW_FLETCH_LEN)),
            wood: lit(materials, Color::srgb_u8(107, 74, 43)),
            steel: lit(materials, Color::srgb_u8(154, 160, 166)),
            feather: lit(materials, Color::srgb_u8(216, 210, 196)),
            ribbon_quad: meshes.add(Rectangle::new(1.0, 1.0)),
            ribbon: ribbon_material(materials, images),
        }
    }
}

/// Loose the cosmetic arrow for a landed bow Auto Shot, from `from` at
/// `target`. The entity sits at the TIP; the shaft hangs back along local -Z.
/// `is_crit` rides to the arrival, where the victim's held reaction plays.
pub fn spawn_auto_shot_arrow(
    commands: &mut Commands,
    assets: &AutoShotArrowAssets,
    from: Vec3,
    target: Entity,
    aim: Vec3,
    is_crit: bool,
) {
    let dir = (aim - from).normalize_or_zero();
    let shaft_len = AUTO_ARROW_LENGTH - ARROW_HEAD_LEN;
    let parts = [
        commands
            .spawn((
                Mesh3d(assets.shaft.clone()),
                MeshMaterial3d(assets.wood.clone()),
                Transform::from_translation(-Vec3::Z * (ARROW_HEAD_LEN + shaft_len * 0.5)),
            ))
            .id(),
        commands
            .spawn((
                Mesh3d(assets.head.clone()),
                MeshMaterial3d(assets.steel.clone()),
                // `Cone` runs along +Y from its base; turned onto +Z it points
                // down the flight path with its base at the shaft's front.
                Transform::from_translation(-Vec3::Z * ARROW_HEAD_LEN)
                    .with_rotation(Quat::from_rotation_arc(Vec3::Y, Vec3::Z)),
            ))
            .id(),
        // Two crossed vanes at the nock.
        commands
            .spawn((
                Mesh3d(assets.fletch.clone()),
                MeshMaterial3d(assets.feather.clone()),
                Transform::from_translation(
                    -Vec3::Z * (AUTO_ARROW_LENGTH - ARROW_FLETCH_LEN * 0.5 - 0.02),
                ),
            ))
            .id(),
        commands
            .spawn((
                Mesh3d(assets.fletch.clone()),
                MeshMaterial3d(assets.feather.clone()),
                Transform::from_translation(
                    -Vec3::Z * (AUTO_ARROW_LENGTH - ARROW_FLETCH_LEN * 0.5 - 0.02),
                )
                .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
            ))
            .id(),
    ];
    commands
        .spawn((
            Transform::from_translation(from).with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
            Visibility::default(),
            CosmeticArrow {
                target,
                is_crit,
                to: aim,
                speed: AUTO_ARROW_SPEED,
                ribbon_carry: 0.0,
                last_pos: from,
                ribbon_mesh: assets.ribbon_quad.clone(),
                ribbon_material: assets.ribbon.clone(),
            },
            PlayMatchEntity,
        ))
        .add_children(&parts);
}

/// Update (graphical-only): fly each arrow at its victim's LIVE chest anchor
/// and retire it the frame its tip arrives — it ends at the target, never in
/// the air where the target used to be. The damage already landed; the arrow
/// is pure theater, and its landing is the victim's hit reaction, released
/// here as a [`RangedHitArrival`] rather than at the damage (see there).
pub fn update_cosmetic_arrows(
    mut commands: Commands,
    time: Res<Time>,
    mut arrows: Query<(Entity, &mut CosmeticArrow, &mut Transform)>,
    targets: Query<(&Transform, Option<&Pet>), (With<Combatant>, Without<CosmeticArrow>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut arrow, mut transform) in arrows.iter_mut() {
        // A victim that despawned mid-flight leaves its last aim point.
        if let Ok((target, pet)) = targets.get(arrow.target) {
            arrow.to = impact_origin(ImpactAnchor::Chest, target.translation, pet.is_some());
        }
        let to_target = arrow.to - transform.translation;
        let step = arrow.speed * dt;
        let arrived = to_target.length() <= step;
        let next = if arrived {
            arrow.to
        } else {
            transform.translation + to_target.normalize_or_zero() * step
        };
        let (mesh, material, last) = (
            arrow.ribbon_mesh.clone(),
            arrow.ribbon_material.clone(),
            arrow.last_pos,
        );
        lay_ribbon(
            &mut commands,
            &mesh,
            &material,
            &mut arrow.ribbon_carry,
            last,
            next,
        );
        if arrived {
            // The side it struck is the side it flew in from: back along the
            // arrow's own heading, which stays well-defined when the last
            // step is vanishingly short. A victim that despawned mid-flight
            // still leaves the marker; its consumer drops it, having no body
            // to react.
            commands.spawn((
                RangedHitArrival {
                    target: arrow.target,
                    is_crit: arrow.is_crit,
                    from: -(transform.rotation * Vec3::Z),
                },
                PlayMatchEntity,
            ));
            commands.entity(entity).despawn();
            continue;
        }
        let dir = to_target.normalize_or_zero();
        transform.translation = next;
        transform.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
        arrow.last_pos = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ramp_hits_its_three_keys() {
        let keys = [0.2, 1.0, 0.4];
        assert!((ramp3(keys, 0.5, 0.0) - 0.2).abs() < 1e-6);
        assert!((ramp3(keys, 0.5, 0.5) - 1.0).abs() < 1e-6);
        assert!((ramp3(keys, 0.5, 1.0) - 0.4).abs() < 1e-6);
        assert!((ramp3(keys, 0.5, 0.25) - 0.6).abs() < 1e-6);
        // An early middle key moves the peak, not the ends.
        assert!((ramp3(keys, 0.25, 0.25) - 1.0).abs() < 1e-6);
        assert!((ramp3(keys, 0.25, 1.0) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_palette_covers_a_whole_life() {
        assert_eq!(palette_step(0.0), 0);
        assert_eq!(palette_step(1.0), PALETTE_STEPS - 1);
        assert_eq!(palette_step(2.0), PALETTE_STEPS - 1);
    }

    #[test]
    fn scaling_keeps_the_client_table_and_changes_only_what_it_names() {
        let e = MAGIC_IMPACT_KEPT[1];
        let s = e.scaled(0.8, 0.6, 1.0);
        assert!((s.size[1] - e.size[1] * 0.8).abs() < 1e-6);
        assert!((s.area - e.area * 0.8).abs() < 1e-6);
        assert!((s.speed - e.speed * 0.8 * 0.6).abs() < 1e-6);
        assert_eq!(s.window, e.window);
        assert_eq!(s.life, e.life);
        assert_eq!((s.rate, s.color, s.alpha), (e.rate, e.color, e.alpha));
    }

    #[test]
    fn every_emitter_has_a_unique_palette_key() {
        let mut names: Vec<&str> = [
            &ARCANESHOT_MISSILE[..],
            &POISONSHOT_MISSILE[..],
            &MAGIC_IMPACT_KEPT[..],
            &BESTOWDISEASE_IMPACT[..],
        ]
        .iter()
        .flat_map(|t| t.iter().map(|e| e.name))
        .collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "two emitters share a palette key");
    }
}
