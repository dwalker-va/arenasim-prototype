use bevy::prelude::*;

use super::hunter_shots::{scale_all, ClientEmitter, EmitterSprite};
use super::school_impact::ImpactStyle;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;

// ==============================================================================
// Client landings — the interrupts, Heroic Strike, Mind Blast, Holy Shock
// ==============================================================================
//
// From the Classic Era client data (build 1.15.9.69547), joined in
// docs/design/2026-09-26-interrupts-and-one-offs-client-data.md:
//
// - **Every interrupt lands a mark on the victim's chest**, and the client
//   draws no "silenced" state at all. Kick and Pummel share kit 133,
//   `kick_chest_impact.m2`: one near-white shockwave. Spell Lock shares
//   Counterspell's `counterspell_impact_chest.m2`: rune discs shrinking into
//   the chest, a seal. Wind Shear is not in the Classic client; by the user's
//   decision it lands as the Kick shockwave in a pale wind-blue.
// - **Heroic Strike** is drawn identically to Mortal Strike in the client
//   (visual 39). Ours keeps Mortal Strike's stroke as the signature and gives
//   Heroic Strike the shared landing, `decisivestrike_impact_chest.m2`: a thin
//   red-to-violet ring with a small spark and plume burst (`heroic_strike.rs`
//   draws its trail).
// - **Mind Blast** smoulders on the HEAD (`mindblast_head.m2`): violet flame
//   licks rising under negative gravity, embers, and alpha-blended grey smoke.
// - **Holy Shock's damage leg** lands as Holy Smite's `holysmite_low_chest.m2`,
//   thirteen emitters grouped here, as at the bench, into seven layers.
//
// The tables below are TRANSCRIPTIONS of the parsed M2 records as the AS-137
// Interrupt Landing Bench carries them per emitter. The user signed off the
// bench's defaults as the starting point for every effect: client size,
// spread and duration (x1.0), every layer kept except Spell Lock's rune plate
// (a 28-vertex mesh whose animation was not parsed). Refine the knobs in the
// Animation Sandbox rather than re-deriving the tables.
//
// Three things are inferred, not read: Heroic Strike's burst counts (the rate
// tracks ramp across a 67 ms window, which literally yields one or two
// particles), the rings facing the camera (the particle flag that might lay
// them flat is only community-documented), and Wind Shear's tint.
//
// Graphical-only: the interrupt landings are spawned off `InterruptedBy`,
// which rides the sim's `CastEnding` marker; the rest play through the shared
// impact's `landing_style` override. Scatter is a deterministic hash (never
// `game_rng`), nothing here writes sim state, and the one system is registered
// in `states/mod.rs` only — headless stays byte-identical.

// ── The bench's knobs (AS-137 Interrupt Landing Bench, signed off 2026-09-26) ──

/// Multipliers over the client's sizes, speeds and timings, per effect (1.0 =
/// client). A uniformly scaled model scales its spawn areas and speeds with it;
/// `SPREAD` scales speed and gravity alone; `TIME` stretches windows and lives.
pub const KICK_IMPACT_SIZE: f32 = 1.0;
pub const KICK_IMPACT_SPREAD: f32 = 1.0;
pub const KICK_IMPACT_TIME: f32 = 1.0;

pub const SPELL_LOCK_IMPACT_SIZE: f32 = 1.0;
pub const SPELL_LOCK_IMPACT_SPREAD: f32 = 1.0;
pub const SPELL_LOCK_IMPACT_TIME: f32 = 1.0;

pub const WIND_SHEAR_IMPACT_SIZE: f32 = 1.0;
pub const WIND_SHEAR_IMPACT_SPREAD: f32 = 1.0;
pub const WIND_SHEAR_IMPACT_TIME: f32 = 1.0;

pub const HEROIC_STRIKE_IMPACT_SIZE: f32 = 1.0;
pub const HEROIC_STRIKE_IMPACT_SPREAD: f32 = 1.0;
pub const HEROIC_STRIKE_IMPACT_TIME: f32 = 1.0;

pub const MIND_BLAST_SMOULDER_SIZE: f32 = 1.0;
pub const MIND_BLAST_SMOULDER_SPREAD: f32 = 1.0;
pub const MIND_BLAST_SMOULDER_TIME: f32 = 1.0;

pub const HOLY_SHOCK_DAMAGE_IMPACT_SIZE: f32 = 1.0;
pub const HOLY_SHOCK_DAMAGE_IMPACT_SPREAD: f32 = 1.0;
pub const HOLY_SHOCK_DAMAGE_IMPACT_TIME: f32 = 1.0;

/// Wind Shear's ring: `#bee1ff`, i.e. `Color::srgb(0.75, 0.88, 1.00)`. It
/// replaces the Kick ring's whole colour ramp; the alpha and size ramps stay.
pub const WIND_SHEAR_RING_TINT: [u8; 3] = [190, 225, 255];

// ── Rendering constants ────────────────────────────────────────────────────

/// Emissive gain of the additive particles, as the Hunter shot landings use.
const LANDING_GLOW: f32 = 2.5;

// ── The tables ─────────────────────────────────────────────────────────────

const WHITE: [u8; 3] = [255, 255, 255];

/// `kick_chest_impact.m2` (kit 133) — Kick's and Pummel's landing. One
/// emitter: `shockwave8.blp`, about three rings over the first 333 ms.
const KICK_CHEST_IMPACT: [ClientEmitter; 1] = [ClientEmitter {
    name: "kick_chest_impact/e0 shockwave",
    sprite: EmitterSprite::Ring,
    additive: true,
    rate: 10.0,
    window: Some((0.0, 0.333)),
    life: 0.3,
    speed: 0.056,
    gravity: 0.0,
    area: 0.056,
    column: None,
    rise: false,
    color: [[253, 244, 225], WHITE, WHITE],
    alpha: [0.7, 0.53, 0.0],
    size: [0.278, 0.556, 0.972],
    mid: 0.5,
    track: None,
}];

/// Wind Shear: the Kick ring with its colour ramp replaced by
/// [`WIND_SHEAR_RING_TINT`]. Named apart from Kick's, because the palette
/// cache is keyed on the name.
const WIND_SHEAR_IMPACT: [ClientEmitter; 1] = [ClientEmitter {
    name: "wind_shear/e0 shockwave (kick ring, wind-blue)",
    color: [WIND_SHEAR_RING_TINT; 3],
    ..KICK_CHEST_IMPACT[0]
}];

/// `counterspell_impact_chest.m2` (kit 381) — Spell Lock's landing, shared
/// with Counterspell. The two rune emitters SHRINK: discs closing in on the
/// chest. The 28-vertex rune plate is left out (its animation is not parsed).
const COUNTERSPELL_IMPACT_CHEST: [ClientEmitter; 3] = [
    ClientEmitter {
        name: "counterspell_impact_chest/e0 aurarune256b",
        sprite: EmitterSprite::Seal,
        additive: true,
        rate: 5.1,
        window: Some((0.0, 1.0)),
        life: 1.235,
        speed: 0.056,
        gravity: 0.0,
        area: 0.028,
        column: None,
        rise: false,
        color: [[30, 136, 235], [144, 52, 230], [170, 30, 236]],
        alpha: [0.0, 0.55, 0.21],
        size: [0.753, 0.456, 0.167],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "counterspell_impact_chest/e1 aurarune256",
        sprite: EmitterSprite::Seal,
        additive: true,
        rate: 6.0,
        window: Some((0.0, 1.0)),
        life: 1.2,
        speed: 0.056,
        gravity: 0.0,
        area: 0.028,
        column: None,
        rise: false,
        color: [[255, 221, 30], [52, 227, 192], [170, 30, 236]],
        alpha: [0.0, 0.65, 0.36],
        size: [0.906, 0.606, 0.422],
        mid: 0.45,
        track: None,
    },
    ClientEmitter {
        name: "counterspell_impact_chest/e2 star8c",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 30.0,
        window: Some((0.0, 1.0)),
        life: 1.0,
        speed: 0.297,
        gravity: 0.0,
        area: 0.0,
        column: Some((0.417, 0.833)),
        rise: false,
        color: [[172, 77, 245], WHITE, [32, 240, 66]],
        alpha: [0.88, 0.68, 0.0],
        size: [0.031, 0.142, 0.0],
        mid: 0.5,
        track: None,
    },
];

/// Heroic Strike's burst window: everything in the source happens in the first
/// 67 ms.
const DECISIVE_WINDOW: (f32, f32) = (0.0, 0.067);

/// `decisivestrike_impact_chest.m2` (kit 437) — Heroic Strike's landing (and,
/// in the client, Mortal Strike's). The counts are the bench's inferred burst:
/// one ring, six sparks, three plume particles.
const DECISIVESTRIKE_IMPACT_CHEST: [ClientEmitter; 3] = [
    ClientEmitter {
        name: "decisivestrike_impact_chest/e0 whiteringthin128",
        sprite: EmitterSprite::Ring,
        additive: true,
        rate: ClientEmitter::burst(1.0, DECISIVE_WINDOW),
        window: Some(DECISIVE_WINDOW),
        life: 0.5,
        speed: 0.0,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[208, 0, 0], [129, 35, 189], [63, 0, 134]],
        alpha: [0.0, 1.0, 0.0],
        size: [0.028, 0.972, 1.667],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "decisivestrike_impact_chest/e1 flare",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: ClientEmitter::burst(6.0, DECISIVE_WINDOW),
        window: Some(DECISIVE_WINDOW),
        life: 0.5,
        speed: 2.778,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[114, 35, 159], [190, 0, 9], [70, 35, 142]],
        alpha: [0.0, 1.0, 0.0],
        size: [0.028, 0.069, 0.028],
        mid: 0.5,
        track: None,
    },
    ClientEmitter {
        name: "decisivestrike_impact_chest/e2 fireplume64",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: ClientEmitter::burst(3.0, DECISIVE_WINDOW),
        window: Some(DECISIVE_WINDOW),
        life: 0.3,
        speed: 5.556,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[81, 0, 144], [36, 0, 171], [81, 0, 144]],
        alpha: [1.0, 1.0, 1.0],
        size: [0.028, 0.139, 0.028],
        mid: 0.5,
        track: None,
    },
];

const MIND_BLAST_FLAME_TRACK: &[(f32, f32)] = &[(0.0, 0.0), (0.2, 75.0), (1.4, 75.0), (2.0, 0.0)];
const MIND_BLAST_EMBER_TRACK: &[(f32, f32)] = &[(0.0, 0.0), (0.2, 2.5), (1.8, 2.5), (2.0, 0.0)];
const MIND_BLAST_SMOKE_TRACK: &[(f32, f32)] = &[(0.0, 0.0), (0.2, 5.0), (1.6, 5.0), (2.0, 0.0)];

/// `mindblast_head.m2` (kit 2709, HEAD attachment 20) — it smoulders rather
/// than bursts: flame licks and embers rising, and grey smoke that can darken.
const MINDBLAST_HEAD: [ClientEmitter; 3] = [
    ClientEmitter {
        name: "mindblast_head/e0 flamelick_purple",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(MIND_BLAST_FLAME_TRACK),
        life: 1.0,
        speed: 0.0,
        gravity: -1.25,
        area: 0.0,
        column: Some((0.069, 0.139)),
        rise: false,
        color: [[79, 55, 255], [126, 55, 255], [210, 0, 255]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.111, 0.278, 0.028],
        mid: 0.5,
        track: Some(MIND_BLAST_FLAME_TRACK),
    },
    ClientEmitter {
        name: "mindblast_head/e1 lavalump2",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(MIND_BLAST_EMBER_TRACK),
        life: 2.25,
        speed: 0.0,
        gravity: -1.25,
        area: 0.083,
        column: None,
        rise: false,
        color: [[150, 0, 255], [90, 0, 255], [0, 42, 255]],
        alpha: [0.0, 1.0, 0.0],
        size: [0.028, 0.056, 0.028],
        mid: 0.6,
        track: Some(MIND_BLAST_EMBER_TRACK),
    },
    ClientEmitter {
        name: "mindblast_head/e2 toonsmoke16",
        sprite: EmitterSprite::Glow,
        // Blend 2: the smoke can darken what is behind it.
        additive: false,
        rate: 0.0,
        window: ClientEmitter::tracked(MIND_BLAST_SMOKE_TRACK),
        life: 2.25,
        speed: 0.0,
        gravity: -1.25,
        area: 0.0,
        column: Some((0.0, 0.181)),
        rise: false,
        color: [[60, 0, 255], [58, 35, 84], [180, 162, 192]],
        alpha: [0.29, 0.59, 0.0],
        size: [0.167, 0.347, 0.694],
        mid: 0.5,
        track: Some(MIND_BLAST_SMOKE_TRACK),
    },
];

const HOLY_FLASH_TRACK: &[(f32, f32)] = &[(0.0, 0.0), (0.133, 0.0), (0.2, 15.0), (0.4, 0.0)];
const HOLY_CONVERGE_TRACK: &[(f32, f32)] = &[(0.0, 0.0), (0.133, 0.0), (0.2, 75.0), (0.533, 0.0)];
const HOLY_SPARKS_A_TRACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.133, 0.0),
    (0.2, 30.0),
    (0.333, 100.0),
    (0.5, 50.0),
    (0.667, 2.0),
    (0.9, 1.0),
    (1.0, 0.0),
];
const HOLY_SPARKS_B_TRACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.133, 0.0),
    (0.233, 5.0),
    (0.4, 75.0),
    (0.567, 5.0),
    (0.733, 2.0),
    (0.967, 1.0),
    (1.067, 0.0),
];
const HOLY_RIBBON_TRACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.133, 0.0),
    (0.167, 50.0),
    (0.2, 150.0),
    (0.233, 75.0),
    (0.3, 35.0),
    (0.367, 50.0),
    (0.467, 0.0),
];
const HOLY_DUST_TRACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.267, 0.0),
    (0.333, 75.0),
    (0.433, 30.0),
    (0.533, 0.0),
];
const HOLY_STREAK_TRACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.133, 0.0),
    (0.3, 100.0),
    (0.367, 50.0),
    (0.467, 0.0),
];
const HOLY_GLOW_TRACK: &[(f32, f32)] = &[(0.0, 0.0), (0.133, 0.0), (0.2, 40.0), (0.633, 0.0)];

/// A white spark arcing out and falling (`holysmite_low_chest` e5 / e7).
const fn holy_spark(name: &'static str, speed: f32, track: &'static [(f32, f32)]) -> ClientEmitter {
    ClientEmitter {
        name,
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(track),
        life: 0.75,
        speed,
        gravity: 6.944,
        area: 0.014,
        column: None,
        rise: false,
        color: [WHITE; 3],
        alpha: [1.0, 1.0, 0.0],
        size: [0.111, 0.069, 0.0],
        mid: 0.25,
        track: Some(track),
    }
}

/// `holysmite_low_chest.m2` (kit 291) — Holy Shock's damage landing, in the
/// bench's seven layer groups: flash, converge (two), sparks (two), ribbons,
/// dust, streaks, glow. Nearly all of it plays between 133 and 550 ms.
const HOLYSMITE_LOW_CHEST: [ClientEmitter; 9] = [
    // flash (e3): a big yellow-white glow growing to 2.78 yd.
    ClientEmitter {
        name: "holysmite_low_chest/flash e3",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(HOLY_FLASH_TRACK),
        life: 0.4,
        speed: 0.417,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[252, 255, 0], WHITE, [255, 246, 0]],
        alpha: [1.0, 1.0, 1.0],
        size: [0.833, 2.778, 0.028],
        mid: 0.5,
        track: Some(HOLY_FLASH_TRACK),
    },
    // converge (e4): stars pulled in, growing as they come.
    ClientEmitter {
        name: "holysmite_low_chest/converge e4",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(HOLY_CONVERGE_TRACK),
        life: 0.5,
        speed: -0.417,
        gravity: 0.0,
        area: 0.014,
        column: None,
        rise: false,
        color: [[255, 255, 0], WHITE, [254, 255, 180]],
        alpha: [0.29, 1.0, 0.0],
        size: [0.167, 1.389, 0.028],
        mid: 0.5,
        track: Some(HOLY_CONVERGE_TRACK),
    },
    // converge (e11 / e12): fine motes drawn in over the first 333 ms.
    ClientEmitter {
        name: "holysmite_low_chest/converge e11-12",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 50.0,
        window: Some((0.0, 0.333)),
        life: 1.5,
        speed: -0.556,
        gravity: 0.0,
        area: 0.278,
        column: None,
        rise: false,
        color: [[255, 186, 77], WHITE, [246, 255, 0]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.028, 0.056, 0.0],
        mid: 0.75,
        track: None,
    },
    // sparks (e5 / e7): white, arcing under gravity 6.94.
    holy_spark("holysmite_low_chest/sparks e5", 2.2, HOLY_SPARKS_A_TRACK),
    holy_spark("holysmite_low_chest/sparks e7", 2.0, HOLY_SPARKS_B_TRACK),
    // ribbons (e1 / e2): gold ribbon-blur, peaking at 150/s.
    ClientEmitter {
        name: "holysmite_low_chest/ribbons e1-2",
        sprite: EmitterSprite::Streak,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(HOLY_RIBBON_TRACK),
        life: 0.75,
        speed: 0.2,
        gravity: 0.0,
        area: 0.068,
        column: None,
        rise: false,
        // Keyed gold -> white across the life; the middle key is that ramp
        // sampled at the emitter's 0.25.
        color: [[255, 236, 160], [255, 241, 184], WHITE],
        alpha: [0.84, 0.78, 0.0],
        size: [0.035, 0.083, 0.0],
        mid: 0.25,
        track: Some(HOLY_RIBBON_TRACK),
    },
    // dust (e6 / e8 / e9): olive to tan, 75/s at 333 ms.
    ClientEmitter {
        name: "holysmite_low_chest/dust e6-9",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(HOLY_DUST_TRACK),
        life: 1.05,
        speed: 0.5,
        gravity: 0.0,
        area: 0.056,
        column: None,
        rise: false,
        color: [[150, 145, 0], [139, 88, 16], [212, 131, 58]],
        alpha: [0.2, 0.29, 0.0],
        size: [0.139, 0.333, 0.389],
        mid: 0.4,
        track: Some(HOLY_DUST_TRACK),
    },
    // streaks (e10): fine white-to-orange.
    ClientEmitter {
        name: "holysmite_low_chest/streaks e10",
        sprite: EmitterSprite::Streak,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(HOLY_STREAK_TRACK),
        life: 1.0,
        speed: 1.667,
        gravity: 0.0,
        area: 0.014,
        column: None,
        rise: false,
        color: [WHITE, [255, 186, 0], [255, 114, 0]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.02, 0.03, 0.0],
        mid: 0.3,
        track: Some(HOLY_STREAK_TRACK),
    },
    // glow (e0): a soft core.
    ClientEmitter {
        name: "holysmite_low_chest/glow e0",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 0.0,
        window: ClientEmitter::tracked(HOLY_GLOW_TRACK),
        life: 0.35,
        speed: 0.056,
        gravity: 0.0,
        area: 0.014,
        column: None,
        rise: false,
        color: [[255, 222, 0], WHITE, [252, 255, 0]],
        alpha: [0.29, 1.0, 0.0],
        size: [0.278, 0.611, 0.417],
        mid: 0.25,
        track: Some(HOLY_GLOW_TRACK),
    },
];

/// What each landing actually plays: the tables at the bench's knobs.
static KICK_LANDING: [ClientEmitter; 1] = scale_all(
    KICK_CHEST_IMPACT,
    KICK_IMPACT_SIZE,
    KICK_IMPACT_SPREAD,
    KICK_IMPACT_TIME,
);
static WIND_SHEAR_LANDING: [ClientEmitter; 1] = scale_all(
    WIND_SHEAR_IMPACT,
    WIND_SHEAR_IMPACT_SIZE,
    WIND_SHEAR_IMPACT_SPREAD,
    WIND_SHEAR_IMPACT_TIME,
);
static SPELL_LOCK_LANDING: [ClientEmitter; 3] = scale_all(
    COUNTERSPELL_IMPACT_CHEST,
    SPELL_LOCK_IMPACT_SIZE,
    SPELL_LOCK_IMPACT_SPREAD,
    SPELL_LOCK_IMPACT_TIME,
);
static HEROIC_STRIKE_LANDING: [ClientEmitter; 3] = scale_all(
    DECISIVESTRIKE_IMPACT_CHEST,
    HEROIC_STRIKE_IMPACT_SIZE,
    HEROIC_STRIKE_IMPACT_SPREAD,
    HEROIC_STRIKE_IMPACT_TIME,
);
static MIND_BLAST_LANDING: [ClientEmitter; 3] = scale_all(
    MINDBLAST_HEAD,
    MIND_BLAST_SMOULDER_SIZE,
    MIND_BLAST_SMOULDER_SPREAD,
    MIND_BLAST_SMOULDER_TIME,
);
static HOLY_SHOCK_LANDING: [ClientEmitter; 9] = scale_all(
    HOLYSMITE_LOW_CHEST,
    HOLY_SHOCK_DAMAGE_IMPACT_SIZE,
    HOLY_SHOCK_DAMAGE_IMPACT_SPREAD,
    HOLY_SHOCK_DAMAGE_IMPACT_TIME,
);

// ── Routing ────────────────────────────────────────────────────────────────

/// The mark an interrupt lands on its victim's chest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InterruptLanding {
    /// `kick_chest_impact`'s near-white shockwave — Kick and Pummel.
    Shockwave,
    /// Counterspell's shrinking runes — Spell Lock.
    Seal,
    /// The shockwave in wind-blue — Wind Shear.
    WindShockwave,
}

impl InterruptLanding {
    /// The single list the spawn system, the lands-silently audit and the
    /// probes derive from. `None` for anything that is not an interrupt.
    pub fn for_ability(ability: AbilityType) -> Option<Self> {
        match ability {
            AbilityType::Kick | AbilityType::Pummel => Some(InterruptLanding::Shockwave),
            AbilityType::SpellLock => Some(InterruptLanding::Seal),
            AbilityType::WindShear => Some(InterruptLanding::WindShockwave),
            _ => None,
        }
    }

    pub fn emitters(self) -> &'static [ClientEmitter] {
        match self {
            InterruptLanding::Shockwave => &KICK_LANDING,
            InterruptLanding::Seal => &SPELL_LOCK_LANDING,
            InterruptLanding::WindShockwave => &WIND_SHEAR_LANDING,
        }
    }

    fn color(self) -> Color {
        match self {
            InterruptLanding::Shockwave => Color::WHITE,
            InterruptLanding::Seal => Color::srgb_u8(144, 52, 230),
            InterruptLanding::WindShockwave => {
                let [r, g, b] = WIND_SHEAR_RING_TINT;
                Color::srgb_u8(r, g, b)
            }
        }
    }
}

/// A landing that is nothing but client emitters: no flash, ring, spray or
/// blot of the school rows.
fn emitter_landing(color: Color, emitters: &'static [ClientEmitter]) -> ImpactStyle {
    ImpactStyle {
        color,
        emissive: LANDING_GLOW,
        flash: None,
        ring: None,
        spray: None,
        blot: None,
        smoulder: None,
        emitters,
    }
}

/// The client landing an ability plays in place of its school row, if it has
/// one here. Consulted by `landing_style` ahead of the school table.
pub fn client_landing_style(ability: AbilityType) -> Option<ImpactStyle> {
    if let Some(landing) = InterruptLanding::for_ability(ability) {
        return Some(emitter_landing(landing.color(), landing.emitters()));
    }
    match ability {
        AbilityType::HeroicStrike => Some(emitter_landing(
            Color::srgb_u8(208, 0, 0),
            &HEROIC_STRIKE_LANDING,
        )),
        AbilityType::MindBlast => Some(emitter_landing(
            Color::srgb_u8(126, 55, 255),
            &MIND_BLAST_LANDING,
        )),
        AbilityType::HolyShock => Some(emitter_landing(
            Color::srgb_u8(255, 246, 160),
            &HOLY_SHOCK_LANDING,
        )),
        _ => None,
    }
}

// ── The interrupt landing ──────────────────────────────────────────────────

/// FixedUpdate (graphical-only): land each interrupt's mark on its victim.
///
/// Reads the `InterruptedBy` riding the sim's `CastEnding` marker, which
/// exists only when an interrupt ABILITY cut a cast short — so a fizzle, and a
/// cast broken by crowd control or Silence, never gets one. The landing is a
/// shared-impact rig (`SchoolImpact`) at the victim's chest, played through
/// [`client_landing_style`]. Runs BEFORE `consume_cast_ending_signals`, which
/// despawns the marker; FixedUpdate for the same reason it does, so an
/// interrupt that lands on one of several ticks in a rendered frame is not
/// missed.
pub fn spawn_interrupt_landings(
    mut commands: Commands,
    abilities: Res<AbilityDefinitions>,
    endings: Query<(&CastEnding, &InterruptedBy)>,
    positions: Query<&Transform, With<Combatant>>,
) {
    for (ending, by) in endings.iter() {
        if InterruptLanding::for_ability(by.ability).is_none() {
            continue;
        }
        let victim = ending.caster;
        let from = match (positions.get(victim), positions.get(by.interrupter)) {
            (Ok(v), Ok(i)) => (i.translation - v.translation).normalize_or_zero(),
            _ => Vec3::ZERO,
        };
        commands.spawn((
            SchoolImpact {
                target: victim,
                ability: by.ability,
                school: abilities.get_unchecked(&by.ability).spell_school,
                anchor: ImpactAnchor::Chest,
                from,
                magnitude: 0.0,
                is_crit: false,
                age: 0.0,
            },
            PlayMatchEntity,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_tables() -> Vec<&'static [ClientEmitter]> {
        vec![
            &KICK_LANDING,
            &WIND_SHEAR_LANDING,
            &SPELL_LOCK_LANDING,
            &HEROIC_STRIKE_LANDING,
            &MIND_BLAST_LANDING,
            &HOLY_SHOCK_LANDING,
        ]
    }

    #[test]
    fn every_emitter_has_a_unique_palette_key() {
        let mut names: Vec<&str> = all_tables()
            .iter()
            .flat_map(|t| t.iter().map(|e| e.name))
            .collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "two emitters share a palette key");
    }

    #[test]
    fn a_tracked_emitter_spans_its_track() {
        for e in all_tables().into_iter().flatten() {
            if let Some(track) = e.track {
                assert_eq!(
                    e.window,
                    Some((track[0].0, track[track.len() - 1].0)),
                    "{}: the window must span the track",
                    e.name
                );
            }
        }
    }

    #[test]
    fn the_heroic_strike_burst_emits_its_counts() {
        // The integrated rate over the window is the inferred burst: one
        // ring, six sparks, three plume particles.
        let counts: Vec<f32> = DECISIVESTRIKE_IMPACT_CHEST
            .iter()
            .map(|e| e.count_between(-1.0, 1.0))
            .collect();
        for (got, want) in counts.iter().zip([1.0, 6.0, 3.0]) {
            assert!((got - want).abs() < 1e-3, "{counts:?}");
        }
    }

    #[test]
    fn a_track_integrates_piecewise() {
        // Mind Blast's flames: a 0->75 ramp over 0.2s (7.5), 75/s held for
        // 1.2s (90), 75->0 over 0.6s (22.5).
        let flames = &MINDBLAST_HEAD[0];
        assert!((flames.count_between(0.0, 2.0) - 120.0).abs() < 1e-3);
        assert!((flames.count_between(0.0, 0.2) - 7.5).abs() < 1e-3);
        assert!(flames.count_between(2.0, 3.0).abs() < 1e-6);
        // Split spans sum to the whole, whatever the frame boundaries.
        let parts: f32 = (0..125)
            .map(|i| flames.count_between(i as f32 * 0.016, (i + 1) as f32 * 0.016))
            .sum();
        assert!((parts - 120.0).abs() < 1e-2, "{parts}");
    }

    #[test]
    fn stretching_the_window_stretches_the_track() {
        let flames = MINDBLAST_HEAD[0].scaled(1.0, 1.0, 2.0);
        assert_eq!(flames.window, Some((0.0, 4.0)));
        // Twice as long at the same rates: twice the particles.
        assert!((flames.count_between(0.0, 4.0) - 240.0).abs() < 1e-2);
    }

    #[test]
    fn wind_shear_is_the_kick_ring_in_its_own_colour() {
        let (kick, wind) = (KICK_CHEST_IMPACT[0], WIND_SHEAR_IMPACT[0]);
        assert_eq!(wind.color, [WIND_SHEAR_RING_TINT; 3]);
        assert_eq!(
            (wind.rate, wind.window, wind.life, wind.alpha, wind.size),
            (kick.rate, kick.window, kick.life, kick.alpha, kick.size)
        );
        assert_ne!(wind.name, kick.name);
    }
}
