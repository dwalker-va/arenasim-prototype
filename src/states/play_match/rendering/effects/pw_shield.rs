//! Power Word: Shield and Divine Shield — the client's two shield models on
//! our rig (AS-344, AS-359).
//!
//! The Classic and TBC clients draw the shield as `holydivineshield_state_base.m2`
//! (fdid 166342, kit 847, SpellVisual 784; research and measurements in
//! `docs/design/2026-10-16-pws-client-data.md`). It is not a sphere. It is three
//! flat, hollow, additive CARDS on one cylindrical-billboard bone, worn at the
//! feet, with no particles:
//!
//! - **Rim** (`genericglow2b1`): a gold outline with a hard inner edge;
//! - **Streak A / B** (`shockwave10d` / `10b`): soft radial streaks that flicker
//!   against each other about every 100 ms, which is the shimmer.
//!
//! Its three sequences are its whole lifecycle: a 200 ms delay and then a tall,
//! full-strength pop (birth, 700 ms); a faint shimmer with a slow jelly wobble
//! (loop, 633 ms); and a swell-and-fade (death, 1100 ms) that plays however the
//! shield ends, broken or expired. The loud moment is the arrival and the
//! sustained state is quiet, so this channel costs less on a busy ally than
//! the sphere it replaces for this spell.
//!
//! Every number below is the bench's signed-off value
//! (`docs/design/benches/2026-10-16-pws-bench.html`). The user kept all three
//! layers, the birth pop and the death swell, at the client's own values; the
//! bench's break burst was left out, so it does not exist here.
//!
//! **Anchor.** The client wears the model on the Base attachment, which rides
//! the animated body. The cards follow the unit's `VisualBody` (walk bob, fear
//! bob, swing step, Cyclone lift), not the sim transform. That is also what
//! keeps their draw order fixed: the capsule is `AlphaMode::Blend` with no depth
//! write, so Bevy orders the two by the view depth of their translations. Every
//! card centre sits above the body centre on the same vertical, so it sorts in
//! front at every camera pitch. Anchored on the sim transform instead, a fear
//! bob (0.18 + 0.04) lifts the body past the rim's 0.11–0.14 and the leg-crossing
//! arc flickers behind and in front of the legs
//! (`tests/pw_shield_visual_probes.rs::the_cards_sort_in_front_of_the_body_at_every_pitch`).
//! That is why `PWS_PUSH` stays at the client's 0.
//!
//! Detection is renderer-side, off `ActiveAuras`, like the aura band: headless
//! never registers any of this, so the sim is untouched by construction. The
//! caster side is the Holy two-hand launch heals already draw (kit 270), routed
//! through `InstantAbilityFired` because PW:S never has a `CastingState`
//! (`heal_cast.rs`, `spawn_instant_cast_hands`).
//!
//! **Divine Shield** wears `divineshield_low_base.m2` (fdid 165961, kit 252,
//! SpellVisual 154; `docs/design/2026-10-16-divine-shield-client-data.md`).
//! It is PW:S's model — the same billboard bone, cards, sequences, scale keys
//! and texture-weight keys — with three additions, which are what tell the two
//! apart, since their gold outlines are identical:
//!
//! - **a blue fill** (`genericglow64`, `(0.318, 0.396, 1.0)`) on two more cards,
//!   rim-sized and streak-sized, riding the rim's weight track. It tints the
//!   body blue-white where PW:S is hollow;
//! - **streak A recoloured cyan** `(0.318, 0.91, 1.0)`;
//! - **gold star sparkles**, two `star5a` emitters at 30/s each for the whole
//!   shield, and a faint **halo fall** (`genericglow2c` rings dropping from the
//!   crown) in the first 300 ms of the death.
//!
//! One bearer can wear both rigs at once (a Priest shielding a bubbled
//! Paladin): they are keyed per kind, and both add. Divine Shield's cast is kit
//! 417, the same Holy launch plus a sound, played off its `DivineShieldPending`.
//! Its values are the Divine Shield Bench's defaults, signed off at the
//! client's own numbers (`docs/design/benches/2026-10-16-divine-shield-bench.html`).

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::hunter_shots::{draw, ramp3, ramp_color, PALETTE_STEPS};
use super::spell_bolts::star_flash_texture;
use crate::states::play_match::components::*;

// --- Signed-off values (the bench's defaults) --------------------------------

/// Client units to our rig: `humanmale.m2` stands 2.034, our capsule 2.5.
pub const PWS_CLIENT_TO_RIG: f32 = 2.5 / 2.034;
/// × the client quad sizes on our rig.
pub const PWS_SIZE: f32 = 1.0;
/// × how far the bone scale swings from 1 (birth pop, wobble, death swell).
pub const PWS_SPREAD: f32 = 1.0;
/// Yards added to the feet anchor (client 0).
pub const PWS_LIFT: f32 = 0.0;
/// Yards toward the camera (client 0: the cards stand on the body's centre plane).
pub const PWS_PUSH: f32 = 0.0;
/// × the client rim weight in the loop and death.
pub const PWS_RIM_GAIN: f32 = 1.0;
/// × the client streak weights in the loop and death.
pub const PWS_STREAK_GAIN: f32 = 1.0;
/// × the 300 ms arrival spike (client weight 1.0).
pub const PWS_BIRTH_GAIN: f32 = 1.0;
/// × every sequence length.
pub const PWS_TIME_SCALE: f32 = 1.0;

// --- Divine Shield: the bench's signed-off values (AS-359) --------------------
//
// The Divine Shield Bench's defaults, signed off at the client's own numbers
// (artifact AehhwbuxGwuaDhttmDYqS2, version 1792216579-ec72). Every layer is
// kept — rim, blue fill, cyan streak A, gold streak B, the sparkles, the birth
// pop, the death swell and the caster's hands — so, as for PW:S, the layer
// toggles have no constant. The client-to-rig scale is PW:S's.

/// × the client quad sizes on our rig.
pub const DS_SIZE: f32 = 1.0;
/// × how far the bone scale swings from 1 (birth pop, wobble, death swell).
pub const DS_SPREAD: f32 = 1.0;
/// Yards added to the feet anchor (client 0).
pub const DS_LIFT: f32 = 0.0;
/// × the client rim weight in the loop and death.
pub const DS_RIM_GAIN: f32 = 1.0;
/// × the blue fill's weight (the rim's track) in the loop and death.
pub const DS_FILL_GAIN: f32 = 1.0;
/// × the client streak weights in the loop and death.
pub const DS_STREAK_GAIN: f32 = 1.0;
/// × the 300 ms arrival spike (client weight 1.0).
pub const DS_BIRTH_GAIN: f32 = 1.0;
/// × the sparkle rate (30/s per emitter, two emitters).
pub const DS_SPARKLE_RATE: f32 = 1.0;
/// × the sparkle size.
pub const DS_SPARKLE_SIZE: f32 = 1.0;
/// × the sparkle alpha.
pub const DS_SPARKLE_GAIN: f32 = 1.0;
/// × the halo ring size.
pub const DS_HALO_SIZE: f32 = 1.0;
/// × the halo ring alpha.
pub const DS_HALO_GAIN: f32 = 1.0;
/// × every sequence length and particle life.
pub const DS_TIME_SCALE: f32 = 1.0;
/// When the halo falls. Its `enabled` track has keys only in the death (on,
/// then off at 300 ms); the bench's default, signed off, reads it literally.
pub const DS_HALO_MODE: HaloMode = HaloMode::DeathOnly;

/// The two readings of the halo emitter's `enabled` track.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HaloMode {
    /// No halo.
    Off,
    /// The literal reading: only the first 300 ms of the death.
    DeathOnly,
    /// The per-sequence reading: the whole shield, stopping 300 ms into the
    /// death.
    WholeShield,
}

/// One shield's tuning: the knobs its bench signed off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShieldTuning {
    pub size: f32,
    pub spread: f32,
    pub lift: f32,
    pub push: f32,
    pub rim_gain: f32,
    pub streak_gain: f32,
    pub birth_gain: f32,
    /// The fill's gain; PW:S has no fill.
    pub fill_gain: f32,
    pub time_scale: f32,
}

pub const PWS_TUNING: ShieldTuning = ShieldTuning {
    size: PWS_SIZE,
    spread: PWS_SPREAD,
    lift: PWS_LIFT,
    push: PWS_PUSH,
    rim_gain: PWS_RIM_GAIN,
    streak_gain: PWS_STREAK_GAIN,
    birth_gain: PWS_BIRTH_GAIN,
    fill_gain: 0.0,
    time_scale: PWS_TIME_SCALE,
};

pub const DS_TUNING: ShieldTuning = ShieldTuning {
    size: DS_SIZE,
    spread: DS_SPREAD,
    lift: DS_LIFT,
    // The bench has no push knob: the client's 0, as for PW:S, which is what
    // keeps the cards sorting in front of the body.
    push: 0.0,
    rim_gain: DS_RIM_GAIN,
    streak_gain: DS_STREAK_GAIN,
    birth_gain: DS_BIRTH_GAIN,
    fill_gain: DS_FILL_GAIN,
    time_scale: DS_TIME_SCALE,
};

/// A shield kind's tuning.
pub fn shield_tuning(kind: ShieldKind) -> &'static ShieldTuning {
    match kind {
        ShieldKind::PowerWordShield => &PWS_TUNING,
        ShieldKind::DivineShield => &DS_TUNING,
    }
}

// --- Client data (fdid 166342, build 2.5.6.69795; Era identical) -------------

/// The aura name the shield is keyed on (`abilities.ron`'s `name`, which is
/// what `Aura::ability_name` carries; `the_aura_name_is_the_ability_name` pins
/// the two together).
pub const PW_SHIELD_AURA_NAME: &str = "Power Word: Shield";
/// The aura name Divine Shield is keyed on (`effects/divine_shield.rs` names
/// the aura; `the_divine_shield_aura_carries_the_ability_name` pins it to
/// `abilities.ron`).
pub const DIVINE_SHIELD_AURA_NAME: &str = "Divine Shield";

/// Half the capsule's height: the feet sit this far below a body's centre.
const BODY_HALF_HEIGHT: f32 = 1.25;

/// A card in client units: width, height, bottom z and top z above the Base
/// attachment (the feet).
pub const PWS_RIM_QUAD: [f32; 4] = [3.18, 4.08, -0.91, 3.17];
pub const PWS_STREAK_QUAD: [f32; 4] = [2.26, 2.90, -0.12, 2.78];
/// Bones 1–3 scale about this height (client units above the feet).
pub const PWS_PIVOT_Z: f32 = 1.11;

/// Sequence lengths, ms.
pub const PWS_BIRTH_MS: f32 = 700.0;
pub const PWS_LOOP_MS: f32 = 633.0;
pub const PWS_DEATH_MS: f32 = 1100.0;
/// The rim's death fade runs out here, ms (the streaks keep flickering past it).
const PWS_RIM_FADE_MS: f32 = 1000.0;

/// Texture weights (ms, weight), all linear (interp 1).
const RIM_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (500.0, 0.12),
    (700.0, 0.15),
];
const RIM_LOOP: &[(f32, f32)] = &[(0.0, 0.15), (300.0, 0.06), (633.0, 0.15)];
const A_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (400.0, 0.12),
    (500.0, 0.0),
    (600.0, 0.12),
    (700.0, 0.0),
];
const A_LOOP: &[(f32, f32)] = &[
    (0.0, 0.0),
    (100.0, 0.12),
    (200.0, 0.0),
    (300.0, 0.12),
    (400.0, 0.0),
    (533.0, 0.2),
    (633.0, 0.0),
];
const A_DEATH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (100.0, 0.12),
    (234.0, 0.0),
    (334.0, 0.12),
    (434.0, 0.0),
    (567.0, 0.2),
    (667.0, 0.0),
    (800.0, 0.12),
    (900.0, 0.0),
    (1100.0, 0.0),
];
const B_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (400.0, 0.0),
    (500.0, 0.12),
    (600.0, 0.0),
    (700.0, 0.12),
];
const B_LOOP: &[(f32, f32)] = &[
    (0.0, 0.12),
    (100.0, 0.0),
    (200.0, 0.2),
    (300.0, 0.0),
    (400.0, 0.12),
    (533.0, 0.0),
    (633.0, 0.12),
];
const B_DEATH: &[(f32, f32)] = &[
    (0.0, 0.12),
    (100.0, 0.0),
    (234.0, 0.2),
    (334.0, 0.0),
    (434.0, 0.12),
    (567.0, 0.0),
    (667.0, 0.12),
    (800.0, 0.0),
    (900.0, 0.12),
    (1000.0, 0.0),
    (1100.0, 0.0),
];
/// Bone scale (width, height): the bone's y and z axes.
const SCALE_BIRTH: &[(f32, [f32; 2])] = &[
    (0.0, [0.0, 0.0]),
    (200.0, [0.0, 0.0]),
    (300.0, [0.522, 1.11]),
    (500.0, [1.09, 1.0]),
    (700.0, [0.901, 1.123]),
];
const SCALE_LOOP: &[(f32, [f32; 2])] = &[
    (0.0, [0.901, 1.123]),
    (300.0, [1.04, 1.0]),
    (633.0, [0.901, 1.123]),
];
/// Death: the first key is replaced by wherever the shield was.
const SCALE_DEATH: &[(f32, [f32; 2])] = &[
    (0.0, [0.901, 1.123]),
    (334.0, [1.04, 1.0]),
    (1000.0, [1.271, 1.158]),
    (1100.0, [1.271, 1.158]),
];

/// The three colours, sRGB (the client's per-batch colour tracks). The 0.318
/// blue is the client's own colour-track value, not an approximation of 1/π.
#[allow(clippy::approx_constant)]
const RIM_RGB: [f32; 3] = [1.0, 0.91, 0.318];
#[allow(clippy::approx_constant)]
const STREAK_A_RGB: [f32; 3] = [0.969, 0.992, 0.318];
#[allow(clippy::approx_constant)]
const STREAK_B_RGB: [f32; 3] = [1.0, 0.976, 0.318];
/// Divine Shield's two colours of its own (fdid 165961's colour tracks 1 and 3).
#[allow(clippy::approx_constant)]
pub const DS_FILL_RGB: [f32; 3] = [0.318, 0.396, 1.0];
#[allow(clippy::approx_constant)]
pub const DS_STREAK_A_RGB: [f32; 3] = [0.318, 0.91, 1.0];

/// Measured radial profiles (angular mean of the decoded textures; r is the
/// fraction of the card's half-size). Generated, never shipped as assets.
const RIM_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.47, 0.0),
    (0.50, 0.93),
    (0.55, 0.74),
    (0.60, 0.46),
    (0.65, 0.20),
    (0.70, 0.03),
    (0.75, 0.0),
    (1.0, 0.0),
];
const A_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.40, 0.0),
    (0.50, 0.009),
    (0.55, 0.037),
    (0.60, 0.105),
    (0.65, 0.166),
    (0.70, 0.177),
    (0.75, 0.154),
    (0.80, 0.114),
    (0.85, 0.072),
    (0.90, 0.029),
    (0.95, 0.006),
    (1.0, 0.0),
];
const B_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.45, 0.0),
    (0.50, 0.005),
    (0.55, 0.018),
    (0.60, 0.060),
    (0.65, 0.134),
    (0.70, 0.173),
    (0.75, 0.163),
    (0.80, 0.133),
    (0.85, 0.097),
    (0.90, 0.058),
    (0.95, 0.020),
    (1.0, 0.005),
];
/// The brightest a streak texel gets (the decoded textures peak near 0.56).
const STREAK_TEXEL_MAX: f32 = 0.58;
const PWS_TEXTURE_PX: u32 = 256;
/// `genericglow64`, the fill: a FILLED soft disc. Measured as rgb × alpha (the
/// client's additive blend adds both), so it falls off faster than either.
const FILL_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.903),
    (0.05, 0.883),
    (0.10, 0.846),
    (0.15, 0.785),
    (0.20, 0.712),
    (0.25, 0.630),
    (0.30, 0.548),
    (0.35, 0.468),
    (0.40, 0.382),
    (0.45, 0.306),
    (0.50, 0.241),
    (0.55, 0.184),
    (0.60, 0.130),
    (0.65, 0.086),
    (0.70, 0.053),
    (0.75, 0.029),
    (0.80, 0.015),
    (0.85, 0.006),
    (0.90, 0.001),
    (0.95, 0.0),
    (1.0, 0.0),
];
/// `genericglow2c`, the halo ring: zero inside r = 0.55, a soft dim ring.
const HALO_PROFILE: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.55, 0.0),
    (0.60, 0.060),
    (0.65, 0.083),
    (0.70, 0.055),
    (0.75, 0.033),
    (0.80, 0.018),
    (0.85, 0.007),
    (0.90, 0.0015),
    (1.0, 0.0),
];
const HALO_TEXTURE_PX: u32 = 128;

// Divine Shield's emitters, in client units (× the rig scale) and ms.

/// P1/P2, `star5a` on bones 7 and 8: a sphere emitter at the waist.
pub const DS_SPARKLE_HEIGHT: f32 = 0.775;
/// Spawn radius of the sphere (the emitter's area, ~0.78–0.83).
pub const DS_SPARKLE_AREA: f32 = 0.80;
/// Outward launch speed, ±30%.
pub const DS_SPARKLE_SPEED: f32 = 1.667;
const DS_SPARKLE_SPEED_VAR: f32 = 0.3;
pub const DS_SPARKLE_LIFE: f32 = 0.4;
/// Spin, rad/s: +3 on one emitter, −3 on the other.
pub const DS_SPARKLE_SPIN: f32 = 3.0;
/// Both emitters' rate tracks, particles per second each.
const SPARKLE_RATE_BIRTH: &[(f32, f32)] = &[(0.0, 0.0), (300.0, 0.0), (500.0, 30.0), (700.0, 30.0)];
const SPARKLE_RATE_LOOP: f32 = 30.0;
const SPARKLE_RATE_DEATH: &[(f32, f32)] = &[(0.0, 30.0), (1100.0, 0.0)];
const SPARKLE_COLOUR: [[u8; 3]; 3] = [[255, 186, 77], [253, 255, 200], [246, 255, 0]];
const SPARKLE_ALPHA: [f32; 3] = [0.0, 1.0, 0.0];
/// Half-size at birth, at `SPARKLE_MID` and at death.
const SPARKLE_HALF: [f32; 3] = [0.056, 0.083, 0.0];
const SPARKLE_MID: f32 = 0.75;

/// P0, `genericglow2c` on bone 6: a point at the crown, falling.
pub const DS_HALO_HEIGHT: f32 = 2.641;
/// Straight down.
pub const DS_HALO_SPEED: f32 = -1.914;
pub const DS_HALO_LIFE: f32 = 1.355;
pub const DS_HALO_RATE: f32 = 15.1;
/// The `enabled` track turns the halo off this far into the death, ms.
pub const DS_HALO_DEATH_OFF_MS: f32 = 300.0;
const HALO_COLOUR: [[u8; 3]; 3] = [[255, 255, 255], [255, 226, 112], [255, 255, 255]];
const HALO_ALPHA: [f32; 3] = [0.0, 0.37, 0.0];
const HALO_HALF: [f32; 3] = [0.139, 1.389, 0.139];
const HALO_MID: f32 = 0.5;

// --- The pure model (what the probes test) -----------------------------------

/// Linear keyframes, held flat outside their range.
fn keys(track: &[(f32, f32)], t: f32) -> f32 {
    if t <= track[0].0 {
        return track[0].1;
    }
    for pair in track.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let u = (t - a.0) / (b.0 - a.0).max(f32::EPSILON);
            return a.1 + (b.1 - a.1) * u;
        }
    }
    track[track.len() - 1].1
}

fn keys2(track: &[(f32, [f32; 2])], t: f32) -> Vec2 {
    let w: Vec<(f32, f32)> = track.iter().map(|&(t, v)| (t, v[0])).collect();
    let h: Vec<(f32, f32)> = track.iter().map(|&(t, v)| (t, v[1])).collect();
    Vec2::new(keys(&w, t), keys(&h, t))
}

/// The spread knob scales a key's swing away from 1; the birth's zero keys
/// stay zero.
fn spread(v: f32, spread: f32) -> f32 {
    if v == 0.0 {
        0.0
    } else {
        1.0 + (v - 1.0) * spread
    }
}

/// One phase's length in seconds, for one kind of shield.
pub fn shield_phase_secs(kind: ShieldKind, phase: PwsPhase) -> f32 {
    let ms = match phase {
        PwsPhase::Birth => PWS_BIRTH_MS,
        PwsPhase::Loop => PWS_LOOP_MS,
        PwsPhase::Death => PWS_DEATH_MS,
    };
    ms / 1000.0 * shield_tuning(kind).time_scale
}

/// The shield's pose at one instant: card scale (width, height) and the four
/// texture weights — rim, streak A, streak B, fill — gains applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PwsSample {
    pub scale: Vec2,
    pub weights: [f32; 4],
}

impl PwsSample {
    pub fn weight(&self, layer: PwsLayer) -> f32 {
        self.weights[match layer {
            PwsLayer::Rim => 0,
            PwsLayer::StreakA => 1,
            PwsLayer::StreakB => 2,
            // Both fill cards ride the rim's track (texture weight 0).
            PwsLayer::FillOuter | PwsLayer::FillInner => 3,
        }]
    }
}

/// Samples a shield. `t` is seconds into `phase`; the loop wraps on its own.
pub fn pws_sample(shield: &PwShieldVisual) -> PwsSample {
    let k = shield_tuning(shield.kind);
    let ms = shield.t / k.time_scale * 1000.0;
    let gains = [k.rim_gain, k.streak_gain, k.streak_gain, k.fill_gain];
    let sp = |v: Vec2| Vec2::new(spread(v.x, k.spread), spread(v.y, k.spread));
    match shield.phase {
        PwsPhase::Birth => {
            let s = keys2(SCALE_BIRTH, ms);
            // The spike carries the birth gain, settling to each layer's gain by 700 ms.
            let blend = ((ms - 300.0) / 400.0).clamp(0.0, 1.0);
            let rim = keys(RIM_BIRTH, ms);
            let w = [rim, keys(A_BIRTH, ms), keys(B_BIRTH, ms), rim];
            PwsSample {
                scale: sp(s),
                weights: std::array::from_fn(|i| {
                    // A layer the shield does not wear (PW:S's fill) stays dark,
                    // birth spike included.
                    if gains[i] == 0.0 {
                        0.0
                    } else {
                        w[i] * (k.birth_gain + (gains[i] - k.birth_gain) * blend)
                    }
                }),
            }
        }
        PwsPhase::Loop => {
            let m = ms.rem_euclid(PWS_LOOP_MS);
            let rim = keys(RIM_LOOP, m);
            let w = [rim, keys(A_LOOP, m), keys(B_LOOP, m), rim];
            PwsSample {
                scale: sp(keys2(SCALE_LOOP, m)),
                weights: std::array::from_fn(|i| w[i] * gains[i]),
            }
        }
        PwsPhase::Death => {
            let mut track: Vec<(f32, [f32; 2])> = SCALE_DEATH
                .iter()
                .map(|&(t, v)| (t, [spread(v[0], k.spread), spread(v[1], k.spread)]))
                .collect();
            track[0].1 = shield.from_scale.to_array();
            let fade = (1.0 - ms / PWS_RIM_FADE_MS).max(0.0);
            PwsSample {
                scale: keys2(&track, ms),
                weights: [
                    shield.from_rim * fade,
                    keys(A_DEATH, ms) * gains[1],
                    keys(B_DEATH, ms) * gains[2],
                    shield.from_fill * fade,
                ],
            }
        }
    }
}

/// A card's centre above the FEET and its half-size, both in yards, at `scale`,
/// for one kind of shield. Every bone scales about the shared pivot, so a
/// taller card also rises.
pub fn shield_card_extent(kind: ShieldKind, layer: PwsLayer, scale: Vec2) -> (f32, Vec2) {
    let quad = match layer {
        PwsLayer::Rim | PwsLayer::FillOuter => PWS_RIM_QUAD,
        PwsLayer::StreakA | PwsLayer::StreakB | PwsLayer::FillInner => PWS_STREAK_QUAD,
    };
    let r = PWS_CLIENT_TO_RIG * shield_tuning(kind).size;
    let centre = (quad[2] + quad[3]) / 2.0 * r;
    let pivot = PWS_PIVOT_Z * r;
    (
        pivot + (centre - pivot) * scale.y,
        Vec2::new(quad[0] / 2.0 * r * scale.x, quad[1] / 2.0 * r * scale.y),
    )
}

/// [`shield_card_extent`] for Power Word: Shield.
pub fn pws_card_extent(layer: PwsLayer, scale: Vec2) -> (f32, Vec2) {
    shield_card_extent(ShieldKind::PowerWordShield, layer, scale)
}

/// Where a card stands, given the centre of the bearer's rendered body.
pub fn pws_card_translation(
    kind: ShieldKind,
    body_centre: Vec3,
    layer: PwsLayer,
    scale: Vec2,
    camera: Vec3,
) -> Vec3 {
    let k = shield_tuning(kind);
    let feet = body_centre + Vec3::Y * (k.lift - BODY_HALF_HEIGHT);
    let toward_camera = (camera - body_centre).with_y(0.0).normalize_or_zero();
    let (centre, _) = shield_card_extent(kind, layer, scale);
    feet + toward_camera * k.push + Vec3::Y * centre
}

/// The cylindrical billboard: an upright card (its `Rectangle` faces +Z) yawed
/// to face the camera, never tipped toward it.
pub fn pws_card_rotation(card: Vec3, camera: Vec3) -> Quat {
    let d = (camera - card).with_y(0.0);
    if d.length_squared() < 1e-8 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_y(d.x.atan2(d.z))
}

/// Whether this aura is a live Power Word: Shield (an absorb with something left).
pub fn is_live_pw_shield(aura: &Aura) -> bool {
    aura.effect_type == AuraType::Absorb
        && aura.ability_name == PW_SHIELD_AURA_NAME
        && aura.magnitude > 0.0
}

/// Whether this aura is Divine Shield's immunity.
pub fn is_divine_shield(aura: &Aura) -> bool {
    aura.effect_type == AuraType::DamageImmunity && aura.ability_name == DIVINE_SHIELD_AURA_NAME
}

/// Whether this aura is a live shield of `kind`.
pub fn is_live_shield(kind: ShieldKind, aura: &Aura) -> bool {
    match kind {
        ShieldKind::PowerWordShield => is_live_pw_shield(aura),
        ShieldKind::DivineShield => is_divine_shield(aura),
    }
}

// --- Divine Shield's particles (pure) ------------------------------------------

/// Sparkles per second across both emitters, `ms` into `phase` (time-scaled
/// client ms), before `DS_SPARKLE_RATE`.
pub fn ds_sparkle_rate(phase: PwsPhase, ms: f32) -> f32 {
    let each = match phase {
        PwsPhase::Birth => keys(SPARKLE_RATE_BIRTH, ms),
        PwsPhase::Loop => SPARKLE_RATE_LOOP,
        PwsPhase::Death => keys(SPARKLE_RATE_DEATH, ms),
    };
    2.0 * each
}

/// Whether the halo is falling, `ms` into `phase`, under `mode`.
pub fn ds_halo_on(mode: HaloMode, phase: PwsPhase, ms: f32) -> bool {
    match (mode, phase) {
        (HaloMode::Off, _) => false,
        (_, PwsPhase::Death) => ms < DS_HALO_DEATH_OFF_MS,
        (HaloMode::WholeShield, _) => true,
        (HaloMode::DeathOnly, _) => false,
    }
}

/// One sparkle's launch from the waist point: an offset inside the emitter's
/// sphere and an outward velocity along the same direction, in yards. `seed`
/// is a deterministic jitter key (never `game_rng`).
pub fn ds_sparkle_launch(seed: u32) -> (Vec3, Vec3) {
    let r = PWS_CLIENT_TO_RIG * DS_SIZE;
    let u = draw(seed, 1) * 2.0 - 1.0;
    let theta = draw(seed, 2) * TAU;
    let ring = (1.0 - u * u).max(0.0).sqrt();
    let dir = Vec3::new(ring * theta.cos(), u, ring * theta.sin());
    let radius = draw(seed, 3) * DS_SPARKLE_AREA * r;
    let speed = DS_SPARKLE_SPEED * r * (1.0 + (draw(seed, 4) * 2.0 - 1.0) * DS_SPARKLE_SPEED_VAR);
    (dir * radius, dir * speed)
}

// --- Textures ----------------------------------------------------------------

/// The bench's hash, ported bit for bit (visual-only; never `game_rng`).
fn pws_hash(n: u32) -> f32 {
    let mut s = n.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (((s >> 22) ^ s) as f64 / 4_294_967_295.0) as f32
}

struct Streak {
    theta: f32,
    width: f32,
    amp: f32,
    drift: f32,
}

/// A streak ring: the measured radial mean times an angular pattern of `n`
/// soft streaks normalised to mean 1 (the bench's `streakFn`).
fn streak_alpha(profile: &[(f32, f32)], n: u32, seed: u32) -> impl Fn(f32, f32) -> f32 {
    use std::f32::consts::TAU;
    let streaks: Vec<Streak> = (0..n)
        .map(|i| Streak {
            theta: (i as f32 + pws_hash(seed + i * 3) * 0.7) / n as f32 * TAU,
            width: 0.05 + 0.07 * pws_hash(seed + i * 3 + 1),
            amp: 0.5 + pws_hash(seed + i * 3 + 2),
            drift: (pws_hash(seed + i * 7) - 0.5) * 0.08,
        })
        .collect();
    let angular = move |theta: f32, r: f32| -> f32 {
        let mut s = 0.12;
        for q in &streaks {
            let mut d = (theta - q.theta).abs();
            d = d.min(TAU - d);
            s += q.amp * (-(d / q.width).powi(2)).exp() * (1.0 + q.drift * (r - 0.7) * 10.0);
        }
        s
    };
    let mean = (0..720)
        .map(|k| angular(k as f32 / 720.0 * TAU, 0.7))
        .sum::<f32>()
        / 720.0;
    let profile = profile.to_vec();
    move |r, theta| {
        (keys(&profile, r) * angular(theta.rem_euclid(TAU), r) / mean).min(STREAK_TEXEL_MAX)
    }
}

/// White texels, the shape in alpha: `alpha(r, theta)` over the unit disc.
fn pws_texture(alpha: impl Fn(f32, f32) -> f32) -> Image {
    disc_texture(PWS_TEXTURE_PX, alpha)
}

fn disc_texture(size: u32, alpha: impl Fn(f32, f32) -> f32) -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let a = alpha((dx * dx + dy * dy).sqrt(), dy.atan2(dx)).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            data[i..i + 3].fill(255);
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

/// Shared by every shield: one card quad and four textures (rim, streak A,
/// streak B, fill), and Divine Shield's particle quad and two palettes.
pub struct PwsAssets {
    quad: Handle<Mesh>,
    textures: [Handle<Image>; 4],
    particle_quad: Handle<Mesh>,
    sparkle_palette: Arc<[Handle<StandardMaterial>]>,
    halo_palette: Arc<[Handle<StandardMaterial>]>,
}

impl PwsAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let rim = keys_fn(RIM_PROFILE);
        let fill = keys_fn(FILL_PROFILE);
        let halo = keys_fn(HALO_PROFILE);
        let star = images.add(star_flash_texture());
        let ring = images.add(disc_texture(HALO_TEXTURE_PX, move |r, _| halo(r)));
        Self {
            // 2 × 2, so a card's scale IS its half-size.
            quad: meshes.add(Rectangle::new(2.0, 2.0)),
            textures: [
                images.add(pws_texture(move |r, _| rim(r))),
                images.add(pws_texture(streak_alpha(A_PROFILE, 17, 101))),
                images.add(pws_texture(streak_alpha(B_PROFILE, 15, 577))),
                images.add(pws_texture(move |r, _| fill(r))),
            ],
            // 1 × 1, so a particle's scale IS its diameter (`ClientParticle`).
            particle_quad: meshes.add(Rectangle::new(1.0, 1.0)),
            sparkle_palette: particle_palette(
                materials,
                &star,
                SPARKLE_COLOUR,
                SPARKLE_ALPHA.map(|a| a * DS_SPARKLE_GAIN),
                SPARKLE_MID,
            ),
            halo_palette: particle_palette(
                materials,
                &ring,
                HALO_COLOUR,
                HALO_ALPHA.map(|a| a * DS_HALO_GAIN),
                HALO_MID,
            ),
        }
    }
}

/// `PALETTE_STEPS` materials sampling a particle's colour and alpha ramps, in
/// the cards' own material: unlit and additive, colour × texture × alpha.
fn particle_palette(
    materials: &mut Assets<StandardMaterial>,
    texture: &Handle<Image>,
    colour: [[u8; 3]; 3],
    alpha: [f32; 3],
    mid: f32,
) -> Arc<[Handle<StandardMaterial>]> {
    (0..PALETTE_STEPS)
        .map(|i| {
            let t = (i as f32 + 0.5) / PALETTE_STEPS as f32;
            materials.add(StandardMaterial {
                base_color: ramp_color(colour, mid, t).with_alpha(ramp3(alpha, mid, t)),
                base_color_texture: Some(texture.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        })
        .collect()
}

/// Which of the four textures a card wears.
fn texture_slot(layer: PwsLayer) -> usize {
    match layer {
        PwsLayer::Rim => 0,
        PwsLayer::StreakA => 1,
        PwsLayer::StreakB => 2,
        PwsLayer::FillOuter | PwsLayer::FillInner => 3,
    }
}

fn keys_fn(track: &'static [(f32, f32)]) -> impl Fn(f32) -> f32 {
    move |r| keys(track, r)
}

/// A card's colour, sRGB: the client's per-batch colour track.
pub fn shield_layer_rgb(kind: ShieldKind, layer: PwsLayer) -> [f32; 3] {
    match (kind, layer) {
        (_, PwsLayer::Rim) => RIM_RGB,
        (ShieldKind::PowerWordShield, PwsLayer::StreakA) => STREAK_A_RGB,
        (ShieldKind::DivineShield, PwsLayer::StreakA) => DS_STREAK_A_RGB,
        (_, PwsLayer::StreakB) => STREAK_B_RGB,
        (_, PwsLayer::FillOuter | PwsLayer::FillInner) => DS_FILL_RGB,
    }
}

fn layer_rgb(kind: ShieldKind, layer: PwsLayer) -> Color {
    let c = shield_layer_rgb(kind, layer);
    Color::srgb(c[0], c[1], c[2])
}

/// The client material: unlit, additive, no depth write, both faces. The
/// texture weight rides in the base colour's alpha (Add premultiplies by it).
/// `unlit` is the faithful choice here: the client adds colour × texture ×
/// weight and nothing else, which is exactly what the bench drew.
fn pws_material(texture: &Handle<Image>, kind: ShieldKind, layer: PwsLayer) -> StandardMaterial {
    StandardMaterial {
        base_color: layer_rgb(kind, layer).with_alpha(0.0),
        base_color_texture: Some(texture.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

// --- Systems -----------------------------------------------------------------

/// Update (graphical-only): start, advance and end each bearer's shields, and
/// run Divine Shield's emitters.
///
/// A living bearer whose live shield of a kind has no rig of that kind in birth
/// or loop gets a new one; one bearer can wear a rig of each kind. A rig whose
/// bearer no longer holds a live shield of its kind plays its death and then
/// despawns: the shield broke, ran out or was dispelled, or the bearer died
/// (auras stay on a corpse, so death ends the shield here, as it ends the aura
/// band). A rig whose bearer entity is gone despawns at once.
#[allow(clippy::too_many_arguments)]
pub fn update_pw_shield_visuals(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<PwsAssets>>,
    bearers: Query<(Entity, &Combatant, &ActiveAuras)>,
    placed: Query<(&Transform, Option<&Children>), With<Combatant>>,
    bodies: Query<&Transform, (With<VisualBody>, Without<Combatant>)>,
    mut rigs: Query<(
        Entity,
        &mut PwShieldVisual,
        Option<&mut DivineShieldEmitters>,
    )>,
) {
    let dt = time.delta_secs();
    let shielded: Vec<(Entity, ShieldKind)> = bearers
        .iter()
        .filter(|(_, combatant, _)| combatant.is_alive())
        .flat_map(|(e, _, auras)| {
            ShieldKind::ALL
                .into_iter()
                .filter(|&kind| auras.auras.iter().any(|a| is_live_shield(kind, a)))
                .map(move |kind| (e, kind))
        })
        .collect();

    let mut held: Vec<(Entity, ShieldKind)> = Vec::new();
    for (rig_entity, mut rig, emitters) in rigs.iter_mut() {
        let Ok((bearer_tf, bearer_children)) = placed.get(rig.bearer) else {
            commands.entity(rig_entity).despawn();
            continue;
        };
        let kind = rig.kind;
        rig.t += dt;
        if rig.phase == PwsPhase::Birth && rig.t >= shield_phase_secs(kind, PwsPhase::Birth) {
            rig.t -= shield_phase_secs(kind, PwsPhase::Birth);
            rig.phase = PwsPhase::Loop;
        }
        let up = shielded.contains(&(rig.bearer, kind));
        if rig.phase != PwsPhase::Death && !up {
            let now = pws_sample(&rig);
            rig.from_scale = now.scale;
            rig.from_rim = now.weight(PwsLayer::Rim);
            rig.from_fill = now.weight(PwsLayer::FillOuter);
            rig.phase = PwsPhase::Death;
            rig.t = 0.0;
        }
        let ended =
            rig.phase == PwsPhase::Death && rig.t >= shield_phase_secs(kind, PwsPhase::Death);
        if let (Some(mut emitters), false) = (emitters, ended) {
            let body = bearer_children
                .and_then(|kids| kids.iter().find_map(|k| bodies.get(k).ok()))
                .map(|body| body.translation)
                .unwrap_or(Vec3::ZERO);
            let feet = bearer_tf.transform_point(body) + Vec3::Y * (DS_LIFT - BODY_HALF_HEIGHT);
            let assets = assets
                .get_or_insert_with(|| PwsAssets::build(&mut meshes, &mut images, &mut materials));
            emit_divine_shield_particles(&mut commands, assets, &rig, &mut emitters, feet, dt);
        }
        if rig.phase == PwsPhase::Death {
            if ended {
                commands.entity(rig_entity).despawn();
            }
        } else {
            held.push((rig.bearer, kind));
        }
    }

    for (bearer, kind) in shielded {
        if held.contains(&(bearer, kind)) {
            continue;
        }
        let assets = assets
            .get_or_insert_with(|| PwsAssets::build(&mut meshes, &mut images, &mut materials));
        let mut rig = commands.spawn((
            PwShieldVisual {
                bearer,
                kind,
                phase: PwsPhase::Birth,
                t: 0.0,
                from_scale: Vec2::ZERO,
                from_rim: 0.0,
                from_fill: 0.0,
            },
            // The rig stays at the origin, so each card's local pose is its world pose.
            Transform::IDENTITY,
            Visibility::default(),
            PlayMatchEntity,
        ));
        if kind == ShieldKind::DivineShield {
            rig.insert(DivineShieldEmitters::default());
        }
        rig.with_children(|rig| {
            for &layer in kind.layers() {
                rig.spawn((
                    PwShieldCard { layer },
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(materials.add(pws_material(
                        &assets.textures[texture_slot(layer)],
                        kind,
                        layer,
                    ))),
                    // Zero-sized until the first placement: the birth's first 200 ms are empty.
                    Transform::from_scale(Vec3::splat(PWS_MIN_SCALE)),
                    NotShadowCaster,
                ));
            }
        });
    }
}

/// Emits this frame's sparkles and halo rings for one Divine Shield rig, as
/// world-space `ClientParticle`s that `animate_client_particles` ages, moves
/// and retires and `billboard_hunter_shots` turns to the camera. `feet` is the
/// bearer's rendered feet, where the client's Base attachment sits.
fn emit_divine_shield_particles(
    commands: &mut Commands,
    assets: &PwsAssets,
    rig: &PwShieldVisual,
    emitters: &mut DivineShieldEmitters,
    feet: Vec3,
    dt: f32,
) {
    let r = PWS_CLIENT_TO_RIG * DS_SIZE;
    let ms = rig.t / DS_TIME_SCALE * 1000.0;
    let ms = if rig.phase == PwsPhase::Loop {
        ms.rem_euclid(PWS_LOOP_MS)
    } else {
        ms
    };
    // A per-rig stream: the bearer's index keeps two shields' sparkles apart.
    let base = rig.bearer.index().wrapping_mul(0x9E37_79B9);

    emitters.sparkle_carry += ds_sparkle_rate(rig.phase, ms) * DS_SPARKLE_RATE * dt;
    let waist = feet + Vec3::Y * DS_SPARKLE_HEIGHT * r;
    while emitters.sparkle_carry >= 1.0 {
        emitters.sparkle_carry -= 1.0;
        let seed = base.wrapping_add(emitters.emitted.wrapping_mul(2_654_435_761));
        // The two emitters alternate: one spins each way.
        let sign: i16 = if emitters.emitted % 2 == 0 { 1 } else { -1 };
        emitters.emitted = emitters.emitted.wrapping_add(1);
        let (offset, velocity) = ds_sparkle_launch(seed);
        let size = SPARKLE_HALF.map(|h| 2.0 * h * r * DS_SPARKLE_SIZE);
        spawn_shield_particle(
            commands,
            assets,
            &assets.sparkle_palette,
            ClientParticle {
                age: 0.0,
                life: DS_SPARKLE_LIFE * DS_TIME_SCALE,
                velocity,
                gravity: 0.0,
                size,
                mid: SPARKLE_MID,
                palette: assets.sparkle_palette.clone(),
                step: 0,
                facing: ParticleFacing::Spin {
                    mrad_per_sec: sign * (DS_SPARKLE_SPIN * 1000.0) as i16,
                    start_mrad: (draw(seed, 5) * TAU * 1000.0) as i16,
                },
                owned: false,
            },
            waist + offset,
        );
    }

    if ds_halo_on(DS_HALO_MODE, rig.phase, ms) {
        emitters.halo_carry += DS_HALO_RATE * dt;
    } else {
        emitters.halo_carry = 0.0;
    }
    let crown = feet + Vec3::Y * DS_HALO_HEIGHT * r;
    while emitters.halo_carry >= 1.0 {
        emitters.halo_carry -= 1.0;
        let size = HALO_HALF.map(|h| 2.0 * h * r * DS_HALO_SIZE);
        spawn_shield_particle(
            commands,
            assets,
            &assets.halo_palette,
            ClientParticle {
                age: 0.0,
                life: DS_HALO_LIFE * DS_TIME_SCALE,
                velocity: Vec3::Y * DS_HALO_SPEED * r,
                gravity: 0.0,
                size,
                mid: HALO_MID,
                palette: assets.halo_palette.clone(),
                step: 0,
                facing: ParticleFacing::Camera,
                owned: false,
            },
            crown,
        );
    }
}

fn spawn_shield_particle(
    commands: &mut Commands,
    assets: &PwsAssets,
    palette: &Arc<[Handle<StandardMaterial>]>,
    particle: ClientParticle,
    at: Vec3,
) {
    let size = particle.size[0].max(1e-4);
    commands.spawn((
        particle,
        Mesh3d(assets.particle_quad.clone()),
        MeshMaterial3d(palette[0].clone()),
        Transform::from_translation(at).with_scale(Vec3::splat(size)),
        NotShadowCaster,
        PlayMatchEntity,
    ));
}

/// A card is never scaled to exactly zero (a singular matrix breaks normals).
const PWS_MIN_SCALE: f32 = 1e-4;

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each card on its bearer's rendered body, turn it
/// to the camera, and set its weight.
pub fn place_pw_shield_cards(
    camera: Query<&Transform, (With<Camera3d>, Without<PwShieldCard>)>,
    rigs: Query<(&PwShieldVisual, &Children)>,
    bearers: Query<(&Transform, Option<&Children>), (With<Combatant>, Without<PwShieldCard>)>,
    bodies: Query<&Transform, (With<VisualBody>, Without<PwShieldCard>, Without<Combatant>)>,
    mut cards: Query<
        (
            &PwShieldCard,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<Combatant>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let Ok((bearer_tf, bearer_children)) = bearers.get(rig.bearer) else {
            continue;
        };
        let body_local = bearer_children
            .and_then(|kids| kids.iter().find_map(|k| bodies.get(k).ok()))
            .map(|body| body.translation)
            .unwrap_or(Vec3::ZERO);
        let body_centre = bearer_tf.transform_point(body_local);
        let sample = pws_sample(rig);
        for child in children.iter() {
            let Ok((card, mut tf, material)) = cards.get_mut(child) else {
                continue;
            };
            let at = pws_card_translation(
                rig.kind,
                body_centre,
                card.layer,
                sample.scale,
                cam.translation,
            );
            let (_, half) = shield_card_extent(rig.kind, card.layer, sample.scale);
            tf.translation = at;
            tf.rotation = pws_card_rotation(at, cam.translation);
            tf.scale = Vec3::new(half.x.max(PWS_MIN_SCALE), half.y.max(PWS_MIN_SCALE), 1.0);
            // Only touch the asset when the weight moved: `get_mut` marks it
            // changed and re-uploads it, and a held weight is common.
            let alpha = sample.weight(card.layer).clamp(0.0, 1.0);
            if materials
                .get(&material.0)
                .is_some_and(|m| m.base_color.alpha() != alpha)
            {
                if let Some(m) = materials.get_mut(&material.0) {
                    m.base_color.set_alpha(alpha);
                }
            }
        }
    }
}
