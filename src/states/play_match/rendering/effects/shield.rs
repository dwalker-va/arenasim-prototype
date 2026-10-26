//! Power Word: Shield, Divine Shield and Ice Barrier — the client's three
//! shield models on our rig (AS-344, AS-359, AS-358).
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
//! 417, the same Holy launch plus a sound, played off its `DivineShieldPending`
//! by `heal_cast.rs`'s `spawn_divine_shield_cast_hands`, in matches and in the
//! animation sandbox alike.
//! Its values are the Divine Shield Bench's defaults, signed off at the
//! client's own numbers (`docs/design/benches/2026-10-16-divine-shield-bench.html`).
//!
//! **Ice Barrier** wears `iceshield_state.m2` (fdid 166392, kit 3672,
//! SpellVisual 4302; `docs/design/2026-10-17-ice-barrier-client-data.md`). The
//! same skeleton again, dressed frost blue and held far brighter: the rim at
//! 0.12–0.30, streak A (here `shockwave10b`) never below 0.6, streak B
//! (`shockwave10d`) flickering against it. It adds icicle clusters on the card
//! at the shoulders and knees, a glint over them that turns with the view, and
//! a frost mist falling from above the head that outlives the barrier; those
//! pieces live in `ice_barrier.rs`. A Mage can wear it beside a Priest's
//! shield. Its cast is kit 207, the Frost two-hand launch (`heal_cast.rs`),
//! off its `InstantAbilityFired`. Its values are the Ice Barrier Bench's
//! defaults, signed off at the client's own numbers
//! (`docs/design/benches/2026-10-17-ice-barrier-bench.html`, artifact
//! 6StjLsouEtbtmBHqjR2fHP version 1792216804-7aa7); the bench's break shatter
//! was declined and is not built. Its blue is the hue floating combat text
//! keeps for absorbs (`ui::theme::fct::ABSORB`), on purpose: the absorbed
//! numbers off a Mage under Ice Barrier are this barrier's own, and the Frost
//! palette already spends the same blue.
//!
//! **Legibility (AS-401).** All of the above is additive light, and on the
//! bright arena sand, which sits on the tonemapper's shoulder, added light
//! barely moves a pixel: at the client's values the held shields were all but
//! invisible in a match. So each shield also wears an [`ShieldLayer::Outline`]:
//! the rim's shape as an alpha-BLENDED line in one of its own client colours,
//! sharp on the inner edge facing the combatant and glowing outward
//! ([`shield_outline_alpha`]), following the rim's life. Divine Shield also
//! wears a [`ShieldLayer::Tint`], its blue fill as a blended wash. Both draw
//! behind every additive card ([`shield_layer_sort_bias`]). The rim gains
//! (and Divine Shield's fill gain) rose with them. Every value is the Shield
//! Legibility Bench's signed-off default
//! (`docs/design/benches/2026-10-25-shield-legibility-bench.html`, artifact
//! 7M3N5Lvgk5m5cPknaps354, version id 1792983198-a728), which composites
//! through Bevy's own TonyMcMapface LUT over sand calibrated to a match.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::hunter_shots::{draw, ramp3, ramp_color, PALETTE_STEPS};
use super::ice_barrier::{
    ib_glint_mesh, ib_icicle_mesh, ib_mist_emit, ib_mist_texture, ib_view_yaw, IB_MIST_ALPHA_KEYS,
    IB_MIST_COLOUR, IB_MIST_DIAMETER, IB_MIST_LIFE, IB_MIST_PER_SEC, IB_MIST_ROLL, IB_MIST_STOP_MS,
    IB_MIST_VARIANTS,
};
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
/// × the client rim weight in the loop and death (AS-401: 2.0).
pub const PWS_RIM_GAIN: f32 = 2.0;
/// × the client streak weights in the loop and death.
pub const PWS_STREAK_GAIN: f32 = 1.0;
/// × the 300 ms arrival spike (client weight 1.0).
pub const PWS_BIRTH_GAIN: f32 = 1.0;
/// × every sequence length.
pub const PWS_TIME_SCALE: f32 = 1.0;

// --- The outline and tint: the Shield Legibility Bench's signed-off values ----
//
// AS-401. The client's shields are additive light, and the arena sand sits on
// the tonemapper's shoulder (HDR about (3.95, 2.76, 1.16), sRGB 241/226/193),
// where added light barely moves a pixel: the held shields measured ΔL* 0.2
// (PW:S), 0.6 (Divine Shield) and 1.2 (Ice Barrier) over it. So every shield
// gains an alpha-BLENDED outline card, which can darken the sand, in one of its
// own client colours: sharp on its inner edge (facing the combatant), a solid
// core, then a glow fading outward as (1 - u)^falloff; Divine Shield also gains
// a blended blue tint of its fill. Signed off at the bench's suggested values
// (`docs/design/benches/2026-10-25-shield-legibility-bench.html`, artifact
// 7M3N5Lvgk5m5cPknaps354, version id 1792983198-a728), measured there at
// ΔL* 10.8 / 13.2 / 11.9. Widths are fractions of the rim card's half-size.

/// The outline's peak opacity.
pub const PWS_OUTLINE_ALPHA: f32 = 0.85;
/// The solid core, from the rim's hard inner edge outward.
pub const PWS_OUTLINE_WIDTH: f32 = 0.035;
/// How far past the core the outline fades out.
pub const PWS_OUTLINE_GLOW_WIDTH: f32 = 0.12;
/// The glow's falloff exponent: alpha × (1 - u)^n across the glow.
pub const PWS_OUTLINE_GLOW_FALLOFF: f32 = 1.6;
/// The outline's colour: the client's rim gold (sRGB).
pub const PWS_OUTLINE_RGB: [f32; 3] = RIM_RGB;
/// × the colour in linear light; below about 2 the line is darker than the sand.
pub const PWS_OUTLINE_LEVEL: f32 = 0.6;
/// How much of the rim's 633 ms wobble the outline keeps.
pub const PWS_OUTLINE_PULSE: f32 = 0.3;

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
/// × the client rim weight in the loop and death (AS-401: 1.5).
pub const DS_RIM_GAIN: f32 = 1.5;
/// × the blue fill's weight (the rim's track) in the loop and death (AS-401: 2.0).
pub const DS_FILL_GAIN: f32 = 2.0;
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
/// The outline (AS-401; see PW:S's block): the client's fill blue.
pub const DS_OUTLINE_ALPHA: f32 = 0.8;
pub const DS_OUTLINE_WIDTH: f32 = 0.035;
pub const DS_OUTLINE_GLOW_WIDTH: f32 = 0.12;
pub const DS_OUTLINE_GLOW_FALLOFF: f32 = 1.6;
pub const DS_OUTLINE_RGB: [f32; 3] = DS_FILL_RGB;
pub const DS_OUTLINE_LEVEL: f32 = 1.0;
pub const DS_OUTLINE_PULSE: f32 = 0.2;
/// The blue fill as an alpha-blended wash (AS-401): its peak opacity.
pub const DS_FILL_TINT: f32 = 0.18;

// --- Ice Barrier: the bench's signed-off values (AS-358) ----------------------
//
// The Ice Barrier Bench's defaults, signed off at the client's own numbers
// (artifact 6StjLsouEtbtmBHqjR2fHP, version 1792216804-7aa7). Every layer is
// kept — rim, both streak rings, the icicles, the glint, the frost mist, the
// birth pop and the death swell — so, as for PW:S, the layer toggles have no
// constant; the declined break shatter has none either. The client-to-rig
// scale is PW:S's.

/// × the client quad and icicle sizes on our rig.
pub const IB_SIZE: f32 = 1.0;
/// × how far the bone scale swings from 1 (birth pop, wobble, death swell).
pub const IB_SPREAD: f32 = 1.0;
/// Yards added to the feet anchor (client 0).
pub const IB_LIFT: f32 = 0.0;
/// Yards toward the camera (client 0).
pub const IB_PUSH: f32 = 0.0;
/// × the client rim weight in the loop and death (AS-401: 1.5).
pub const IB_RIM_GAIN: f32 = 1.5;
/// × the client streak weights in the loop and death.
pub const IB_STREAK_GAIN: f32 = 1.0;
/// × the 300 ms arrival spike (client weight 1.0).
pub const IB_BIRTH_GAIN: f32 = 1.0;
/// × the icicles' plain pass (client weight 0.6).
pub const IB_ICICLE_GAIN: f32 = 1.0;
/// × the icicles' sphere-mapped glint (client weight 0.1).
pub const IB_GLINT_GAIN: f32 = 1.0;
/// × 11.7 mist puffs per second.
pub const IB_MIST_RATE: f32 = 1.0;
/// × the mist's 0.39 mid-life alpha peak.
pub const IB_MIST_ALPHA: f32 = 1.0;
/// × the mist puff size (0.28 → 0.56 client units).
pub const IB_MIST_SIZE: f32 = 1.0;
/// × the mist's 13° fall cone.
pub const IB_MIST_SPREAD: f32 = 1.0;
/// × every sequence length and the mist's life.
pub const IB_TIME_SCALE: f32 = 1.0;
/// Client colours (rim, streak A, streak B), sRGB multipliers on white textures.
pub const IB_RIM_RGB: [f32; 3] = [0.584, 0.82, 1.0];
pub const IB_STREAK_A_RGB: [f32; 3] = [0.514, 0.741, 1.0];
pub const IB_STREAK_B_RGB: [f32; 3] = [0.478, 0.733, 0.992];
/// The outline (AS-401; see PW:S's block), raised over the other two: the
/// client's rim frost.
pub const IB_OUTLINE_ALPHA: f32 = 0.9;
pub const IB_OUTLINE_WIDTH: f32 = 0.04;
pub const IB_OUTLINE_GLOW_WIDTH: f32 = 0.12;
pub const IB_OUTLINE_GLOW_FALLOFF: f32 = 1.6;
pub const IB_OUTLINE_RGB: [f32; 3] = IB_RIM_RGB;
pub const IB_OUTLINE_LEVEL: f32 = 0.5;
pub const IB_OUTLINE_PULSE: f32 = 0.3;

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
    /// The fill's gain; only Divine Shield has a fill.
    pub fill_gain: f32,
    /// The icicles' and their glint's gains; only Ice Barrier has icicles.
    pub icicle_gain: f32,
    pub glint_gain: f32,
    pub time_scale: f32,
    /// The blended outline (AS-401).
    pub outline: OutlineTuning,
    /// The blended fill tint's peak opacity; only Divine Shield has one.
    pub fill_tint: f32,
}

/// The blended outline's shape, colour and life (AS-401).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineTuning {
    pub alpha: f32,
    pub width: f32,
    pub glow_width: f32,
    pub glow_falloff: f32,
    /// sRGB.
    pub rgb: [f32; 3],
    /// × the colour in linear light.
    pub level: f32,
    pub pulse: f32,
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
    icicle_gain: 0.0,
    glint_gain: 0.0,
    time_scale: PWS_TIME_SCALE,
    outline: OutlineTuning {
        alpha: PWS_OUTLINE_ALPHA,
        width: PWS_OUTLINE_WIDTH,
        glow_width: PWS_OUTLINE_GLOW_WIDTH,
        glow_falloff: PWS_OUTLINE_GLOW_FALLOFF,
        rgb: PWS_OUTLINE_RGB,
        level: PWS_OUTLINE_LEVEL,
        pulse: PWS_OUTLINE_PULSE,
    },
    fill_tint: 0.0,
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
    icicle_gain: 0.0,
    glint_gain: 0.0,
    time_scale: DS_TIME_SCALE,
    outline: OutlineTuning {
        alpha: DS_OUTLINE_ALPHA,
        width: DS_OUTLINE_WIDTH,
        glow_width: DS_OUTLINE_GLOW_WIDTH,
        glow_falloff: DS_OUTLINE_GLOW_FALLOFF,
        rgb: DS_OUTLINE_RGB,
        level: DS_OUTLINE_LEVEL,
        pulse: DS_OUTLINE_PULSE,
    },
    fill_tint: DS_FILL_TINT,
};

pub const IB_TUNING: ShieldTuning = ShieldTuning {
    size: IB_SIZE,
    spread: IB_SPREAD,
    lift: IB_LIFT,
    push: IB_PUSH,
    rim_gain: IB_RIM_GAIN,
    streak_gain: IB_STREAK_GAIN,
    birth_gain: IB_BIRTH_GAIN,
    fill_gain: 0.0,
    icicle_gain: IB_ICICLE_GAIN,
    glint_gain: IB_GLINT_GAIN,
    time_scale: IB_TIME_SCALE,
    outline: OutlineTuning {
        alpha: IB_OUTLINE_ALPHA,
        width: IB_OUTLINE_WIDTH,
        glow_width: IB_OUTLINE_GLOW_WIDTH,
        glow_falloff: IB_OUTLINE_GLOW_FALLOFF,
        rgb: IB_OUTLINE_RGB,
        level: IB_OUTLINE_LEVEL,
        pulse: IB_OUTLINE_PULSE,
    },
    fill_tint: 0.0,
};

/// A shield kind's tuning.
pub fn shield_tuning(kind: ShieldKind) -> &'static ShieldTuning {
    match kind {
        ShieldKind::PowerWordShield => &PWS_TUNING,
        ShieldKind::DivineShield => &DS_TUNING,
        ShieldKind::IceBarrier => &IB_TUNING,
    }
}

// --- Client data (fdid 166342, build 2.5.6.69795; Era identical) -------------

/// The aura name the shield is keyed on (`abilities.ron`'s `name`, which is
/// what `Aura::ability_name` carries; `the_aura_name_is_the_ability_name` pins
/// the two together).
pub const PW_SHIELD_AURA_NAME: &str = "Power Word: Shield";
/// The aura name Divine Shield is keyed on (`effects/divine_shield.rs` names
/// the aura from `abilities.ron`'s `name`;
/// `the_divine_shield_aura_carries_the_ability_name` pins the two together).
pub const DIVINE_SHIELD_AURA_NAME: &str = "Divine Shield";
/// The aura name Ice Barrier is keyed on (`abilities.ron`'s `name`;
/// `the_aura_name_is_the_ability_name` in its probes pins the two together).
pub const ICE_BARRIER_AURA_NAME: &str = "Ice Barrier";

/// Half the capsule's height: the feet sit this far below a body's centre.
pub(super) const BODY_HALF_HEIGHT: f32 = 1.25;

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
/// Ice Barrier's texture weights (fdid 166392): the same keys in time, held
/// far brighter. Its two streak rings share one birth.
const IB_RIM_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (500.0, 0.24),
    (700.0, 0.3),
];
const IB_RIM_LOOP: &[(f32, f32)] = &[(0.0, 0.3), (300.0, 0.12), (633.0, 0.3)];
const IB_STREAK_BIRTH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (200.0, 0.0),
    (300.0, 1.0),
    (400.0, 0.6),
    (500.0, 1.0),
    (600.0, 0.6),
    (700.0, 1.0),
];
const IB_A_LOOP: &[(f32, f32)] = &[
    (0.0, 1.0),
    (100.0, 0.6),
    (200.0, 1.0),
    (300.0, 0.6),
    (400.0, 1.0),
    (533.0, 0.6),
    (633.0, 1.0),
];
const IB_A_DEATH: &[(f32, f32)] = &[
    (0.0, 1.0),
    (100.0, 0.6),
    (234.0, 1.0),
    (334.0, 0.6),
    (434.0, 1.0),
    (567.0, 1.0),
    (667.0, 0.6),
    (800.0, 1.0),
    (900.0, 0.6),
    (1000.0, 0.0),
    (1100.0, 0.0),
];
const IB_B_LOOP: &[(f32, f32)] = &[
    (0.0, 0.6),
    (100.0, 0.24),
    (200.0, 0.0),
    (300.0, 0.24),
    (400.0, 0.0),
    (533.0, 0.4),
    (633.0, 0.0),
];
const IB_B_DEATH: &[(f32, f32)] = &[
    (0.0, 0.0),
    (100.0, 0.24),
    (234.0, 0.0),
    (334.0, 0.24),
    (434.0, 0.0),
    (567.0, 0.4),
    (667.0, 0.0),
    (800.0, 0.24),
    (900.0, 0.0),
    (1100.0, 0.0),
];
/// The icicles' two passes: held at 0.6 and 0.1; in the death the plain pass
/// fades by 834 ms, and the glint keys to −0.4 by 834 ms, crossing 0 at 167.
const IB_ICE_HELD: f32 = 0.6;
const IB_ICE_DEATH: &[(f32, f32)] = &[(0.0, 0.6), (834.0, 0.0), (1100.0, 0.0)];
const IB_GLINT_HELD: f32 = 0.1;
const IB_GLINT_DEATH: &[(f32, f32)] = &[(0.0, 0.1), (834.0, -0.4), (1100.0, -0.4)];

/// One model's texture-weight tracks: rim, streak A and streak B through the
/// three sequences (the rim's death is a fade from wherever it was).
struct WeightTracks {
    rim_birth: &'static [(f32, f32)],
    rim_loop: &'static [(f32, f32)],
    a_birth: &'static [(f32, f32)],
    a_loop: &'static [(f32, f32)],
    a_death: &'static [(f32, f32)],
    b_birth: &'static [(f32, f32)],
    b_loop: &'static [(f32, f32)],
    b_death: &'static [(f32, f32)],
    /// The rim's held peak: the outline's envelope is the rim weight over it.
    rim_peak: f32,
}

/// PW:S's tracks, which Divine Shield's model shares key for key.
const HOLY_TRACKS: WeightTracks = WeightTracks {
    rim_birth: RIM_BIRTH,
    rim_loop: RIM_LOOP,
    a_birth: A_BIRTH,
    a_loop: A_LOOP,
    a_death: A_DEATH,
    b_birth: B_BIRTH,
    b_loop: B_LOOP,
    b_death: B_DEATH,
    rim_peak: 0.15,
};
const IB_TRACKS: WeightTracks = WeightTracks {
    rim_birth: IB_RIM_BIRTH,
    rim_loop: IB_RIM_LOOP,
    a_birth: IB_STREAK_BIRTH,
    a_loop: IB_A_LOOP,
    a_death: IB_A_DEATH,
    b_birth: IB_STREAK_BIRTH,
    b_loop: IB_B_LOOP,
    b_death: IB_B_DEATH,
    rim_peak: 0.3,
};

fn weight_tracks(kind: ShieldKind) -> &'static WeightTracks {
    match kind {
        ShieldKind::PowerWordShield | ShieldKind::DivineShield => &HOLY_TRACKS,
        ShieldKind::IceBarrier => &IB_TRACKS,
    }
}

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
pub(super) fn keys(track: &[(f32, f32)], t: f32) -> f32 {
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
pub fn shield_phase_secs(kind: ShieldKind, phase: ShieldPhase) -> f32 {
    let ms = match phase {
        ShieldPhase::Birth => PWS_BIRTH_MS,
        ShieldPhase::Loop => PWS_LOOP_MS,
        ShieldPhase::Death => PWS_DEATH_MS,
    };
    ms / 1000.0 * shield_tuning(kind).time_scale
}

/// The shield's pose at one instant: card scale (width, height) and the eight
/// weights — rim, streak A, streak B, fill, icicles, glint, outline, tint —
/// gains applied. A layer a kind does not wear has a zero gain, so it stays
/// dark. The outline's and tint's weights are their opacities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShieldSample {
    pub scale: Vec2,
    pub weights: [f32; 8],
    /// The outline's and tint's envelope, 0..1: the rim's own life (its birth
    /// pop, `pulse` of its wobble, its death fade) over its held peak.
    pub env: f32,
}

impl ShieldSample {
    pub fn weight(&self, layer: ShieldLayer) -> f32 {
        self.weights[match layer {
            ShieldLayer::Rim => 0,
            ShieldLayer::StreakA => 1,
            ShieldLayer::StreakB => 2,
            // Both fill cards ride the rim's track (texture weight 0).
            ShieldLayer::FillOuter | ShieldLayer::FillInner => 3,
            ShieldLayer::Icicles => 4,
            ShieldLayer::Glint => 5,
            ShieldLayer::Outline => 6,
            ShieldLayer::Tint => 7,
        }]
    }
}

/// Samples a shield. `t` is seconds into `phase`; the loop wraps on its own.
pub fn shield_sample(shield: &ShieldVisual) -> ShieldSample {
    let k = shield_tuning(shield.kind);
    let tr = weight_tracks(shield.kind);
    let ms = shield.t / k.time_scale * 1000.0;
    let gains = [
        k.rim_gain,
        k.streak_gain,
        k.streak_gain,
        k.fill_gain,
        k.icicle_gain,
        k.glint_gain,
    ];
    let sp = |v: Vec2| Vec2::new(spread(v.x, k.spread), spread(v.y, k.spread));
    // The blended layers' opacities at envelope `env`.
    let blended = |env: f32| [k.outline.alpha * env, k.fill_tint * env];
    let (scale, w6, env) = match shield.phase {
        ShieldPhase::Birth => {
            let s = keys2(SCALE_BIRTH, ms);
            // The cards' spike carries the birth gain, settling to each layer's
            // gain by 700 ms; Ice Barrier's icicles hold their own weight and
            // pop by scale.
            let blend = ((ms - 300.0) / 400.0).clamp(0.0, 1.0);
            let rim = keys(tr.rim_birth, ms);
            let w = [
                rim,
                keys(tr.a_birth, ms),
                keys(tr.b_birth, ms),
                rim,
                IB_ICE_HELD,
                IB_GLINT_HELD,
            ];
            let w6: [f32; 6] = std::array::from_fn(|i| {
                // A layer the shield does not wear stays dark, birth spike
                // included.
                if gains[i] == 0.0 {
                    0.0
                } else if i < 4 {
                    w[i] * (k.birth_gain + (gains[i] - k.birth_gain) * blend)
                } else {
                    w[i] * gains[i]
                }
            });
            // The outline pops in with the rim, capped at its held strength.
            (sp(s), w6, (rim / tr.rim_peak).clamp(0.0, 1.0))
        }
        ShieldPhase::Loop => {
            let m = ms.rem_euclid(PWS_LOOP_MS);
            let rim = keys(tr.rim_loop, m);
            let w = [
                rim,
                keys(tr.a_loop, m),
                keys(tr.b_loop, m),
                rim,
                IB_ICE_HELD,
                IB_GLINT_HELD,
            ];
            let env = 1.0 - k.outline.pulse * (1.0 - (rim / tr.rim_peak).clamp(0.0, 1.0));
            (
                sp(keys2(SCALE_LOOP, m)),
                std::array::from_fn(|i| w[i] * gains[i]),
                env,
            )
        }
        ShieldPhase::Death => {
            let mut track: Vec<(f32, [f32; 2])> = SCALE_DEATH
                .iter()
                .map(|&(t, v)| (t, [spread(v[0], k.spread), spread(v[1], k.spread)]))
                .collect();
            track[0].1 = shield.from_scale.to_array();
            let fade = (1.0 - ms / PWS_RIM_FADE_MS).max(0.0);
            (
                keys2(&track, ms),
                [
                    shield.from_rim * fade,
                    keys(tr.a_death, ms) * gains[1],
                    keys(tr.b_death, ms) * gains[2],
                    shield.from_fill * fade,
                    keys(IB_ICE_DEATH, ms).max(0.0) * gains[4],
                    keys(IB_GLINT_DEATH, ms).max(0.0) * gains[5],
                ],
                shield.from_env * fade,
            )
        }
    };
    let [outline, tint] = blended(env);
    ShieldSample {
        scale,
        weights: [w6[0], w6[1], w6[2], w6[3], w6[4], w6[5], outline, tint],
        env,
    }
}

/// A card's centre above the FEET and its half-size, both in yards, at `scale`,
/// for one kind of shield. Every bone scales about the shared pivot, so a
/// taller card also rises.
pub fn shield_card_extent(kind: ShieldKind, layer: ShieldLayer, scale: Vec2) -> (f32, Vec2) {
    let quad = match layer {
        ShieldLayer::Rim | ShieldLayer::FillOuter | ShieldLayer::Outline | ShieldLayer::Tint => {
            PWS_RIM_QUAD
        }
        // Ice Barrier's icicles ride bone 3, as streak B does: their card
        // stands at the streak card's centre.
        ShieldLayer::StreakA
        | ShieldLayer::StreakB
        | ShieldLayer::FillInner
        | ShieldLayer::Icicles
        | ShieldLayer::Glint => PWS_STREAK_QUAD,
    };
    let r = PWS_CLIENT_TO_RIG * shield_tuning(kind).size;
    let centre = (quad[2] + quad[3]) / 2.0 * r;
    let pivot = PWS_PIVOT_Z * r;
    (
        pivot + (centre - pivot) * scale.y,
        Vec2::new(quad[0] / 2.0 * r * scale.x, quad[1] / 2.0 * r * scale.y),
    )
}

/// A card's transform scale at bone scale `scale`. The quad cards are 2 × 2,
/// so their scale is their half-size; Ice Barrier's icicle and glint meshes
/// are built in yards about the streak card's centre, so theirs is the bone
/// scale itself.
pub fn shield_card_scale(kind: ShieldKind, layer: ShieldLayer, scale: Vec2) -> Vec3 {
    let v = match layer {
        ShieldLayer::Icicles | ShieldLayer::Glint => scale,
        _ => shield_card_extent(kind, layer, scale).1,
    };
    Vec3::new(v.x.max(PWS_MIN_SCALE), v.y.max(PWS_MIN_SCALE), 1.0)
}

/// [`shield_card_extent`] for Power Word: Shield.
pub fn pws_card_extent(layer: ShieldLayer, scale: Vec2) -> (f32, Vec2) {
    shield_card_extent(ShieldKind::PowerWordShield, layer, scale)
}

/// Where a card stands, given the centre of the bearer's rendered body.
pub fn pws_card_translation(
    kind: ShieldKind,
    body_centre: Vec3,
    layer: ShieldLayer,
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

/// Whether this aura is a live Ice Barrier (an absorb with something left).
pub fn is_live_ice_barrier(aura: &Aura) -> bool {
    aura.effect_type == AuraType::Absorb
        && aura.ability_name == ICE_BARRIER_AURA_NAME
        && aura.magnitude > 0.0
}

/// Whether this aura is a live shield of `kind`.
pub fn is_live_shield(kind: ShieldKind, aura: &Aura) -> bool {
    match kind {
        ShieldKind::PowerWordShield => is_live_pw_shield(aura),
        ShieldKind::DivineShield => is_divine_shield(aura),
        ShieldKind::IceBarrier => is_live_ice_barrier(aura),
    }
}

/// Whether an Ice Barrier rig is shedding mist: from the cast, through the
/// held barrier, and into the first 767 ms of its death.
pub fn ib_mist_spawning(rig: &ShieldVisual) -> bool {
    match rig.phase {
        ShieldPhase::Birth | ShieldPhase::Loop => true,
        ShieldPhase::Death => rig.t / IB_TIME_SCALE * 1000.0 < IB_MIST_STOP_MS,
    }
}

// --- Divine Shield's particles (pure) ------------------------------------------

/// Sparkles per second across both emitters, `ms` into `phase` (time-scaled
/// client ms), before `DS_SPARKLE_RATE`.
pub fn ds_sparkle_rate(phase: ShieldPhase, ms: f32) -> f32 {
    let each = match phase {
        ShieldPhase::Birth => keys(SPARKLE_RATE_BIRTH, ms),
        ShieldPhase::Loop => SPARKLE_RATE_LOOP,
        ShieldPhase::Death => keys(SPARKLE_RATE_DEATH, ms),
    };
    2.0 * each
}

/// Whether the halo is falling, `ms` into `phase`, under `mode`.
pub fn ds_halo_on(mode: HaloMode, phase: ShieldPhase, ms: f32) -> bool {
    match (mode, phase) {
        (HaloMode::Off, _) => false,
        (_, ShieldPhase::Death) => ms < DS_HALO_DEATH_OFF_MS,
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

// --- The outline's profile (AS-401) -------------------------------------------

/// Where the client rim texture's hard inner edge sits, as a fraction of the
/// card's half-size (`RIM_PROFILE` rises from 0 at 0.47 to 0.93 at 0.50).
pub const OUTLINE_INNER: f32 = 0.475;

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let u = ((x - a) / (b - a)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// The outline's alpha at `r` (the fraction of the rim card's half-size from
/// its centre), for one kind of shield: a crisp step at the rim's inner edge,
/// facing the combatant; full through the core; then a glow falling to zero
/// outward as (1 - u)^falloff. The bench's `outlineAt`, ported as is.
pub fn shield_outline_alpha(kind: ShieldKind, r: f32) -> f32 {
    let o = &shield_tuning(kind).outline;
    let inner = smoothstep(OUTLINE_INNER - 0.006, OUTLINE_INNER + 0.002, r);
    let d = r - (OUTLINE_INNER + o.width);
    if d <= 0.0 {
        inner
    } else if d >= o.glow_width {
        0.0
    } else {
        inner * (1.0 - d / o.glow_width).powf(o.glow_falloff)
    }
}

// --- Textures ----------------------------------------------------------------

/// The bench's hash, ported bit for bit (visual-only; never `game_rng`).
pub(super) fn pws_hash(n: u32) -> f32 {
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

/// One kind's outline texture: white texels, its profile in alpha.
fn outline_texture(kind: ShieldKind) -> Image {
    pws_texture(move |r, _| shield_outline_alpha(kind, r))
}

/// White texels, the shape in alpha: `alpha(r, theta)` over the unit disc.
fn pws_texture(alpha: impl Fn(f32, f32) -> f32) -> Image {
    disc_texture(PWS_TEXTURE_PX, alpha)
}

/// A `size`² texture of white texels whose alpha is `alpha(r, theta)` over
/// the unit disc (the benches' `makeTex`).
pub(super) fn disc_texture(size: u32, alpha: impl Fn(f32, f32) -> f32) -> Image {
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

/// Shared by every shield: one card quad and seven textures (rim, the two
/// streak rings, fill, and each kind's outline), Divine Shield's particle quad and two palettes, and
/// Ice Barrier's icicle mesh and mist palettes (one per puff shape).
pub struct ShieldAssets {
    quad: Handle<Mesh>,
    textures: [Handle<Image>; 7],
    particle_quad: Handle<Mesh>,
    sparkle_palette: Arc<[Handle<StandardMaterial>]>,
    halo_palette: Arc<[Handle<StandardMaterial>]>,
    icicles: Handle<Mesh>,
    mist_palettes: Vec<Arc<[Handle<StandardMaterial>]>>,
}

impl ShieldAssets {
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
                images.add(outline_texture(ShieldKind::PowerWordShield)),
                images.add(outline_texture(ShieldKind::DivineShield)),
                images.add(outline_texture(ShieldKind::IceBarrier)),
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
            icicles: meshes.add(ib_icicle_mesh(PWS_CLIENT_TO_RIG * IB_SIZE)),
            mist_palettes: (0..IB_MIST_VARIANTS)
                .map(|i| {
                    let puff = images.add(ib_mist_texture(i));
                    particle_palette(
                        materials,
                        &puff,
                        IB_MIST_COLOUR,
                        IB_MIST_ALPHA_KEYS.map(|a| a * IB_MIST_ALPHA),
                        0.5,
                    )
                })
                .collect(),
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

/// Which of the four textures a card wears; Ice Barrier's icicle and glint
/// meshes wear none (their colour is in their vertices). Slot 1 is
/// `shockwave10d` and slot 2 `shockwave10b`: Ice Barrier's streak A is the
/// `10b` ring and its B the `10d`, the other way round from PW:S's.
fn texture_slot(kind: ShieldKind, layer: ShieldLayer) -> Option<usize> {
    match (kind, layer) {
        (_, ShieldLayer::Rim) => Some(0),
        (ShieldKind::IceBarrier, ShieldLayer::StreakA) => Some(2),
        (ShieldKind::IceBarrier, ShieldLayer::StreakB) => Some(1),
        (_, ShieldLayer::StreakA) => Some(1),
        (_, ShieldLayer::StreakB) => Some(2),
        (_, ShieldLayer::FillOuter | ShieldLayer::FillInner | ShieldLayer::Tint) => Some(3),
        (_, ShieldLayer::Icicles | ShieldLayer::Glint) => None,
        (ShieldKind::PowerWordShield, ShieldLayer::Outline) => Some(4),
        (ShieldKind::DivineShield, ShieldLayer::Outline) => Some(5),
        (ShieldKind::IceBarrier, ShieldLayer::Outline) => Some(6),
    }
}

fn keys_fn(track: &'static [(f32, f32)]) -> impl Fn(f32) -> f32 {
    move |r| keys(track, r)
}

/// A card's colour, sRGB: the client's per-batch colour track.
pub fn shield_layer_rgb(kind: ShieldKind, layer: ShieldLayer) -> [f32; 3] {
    match (kind, layer) {
        (_, ShieldLayer::Outline) => shield_tuning(kind).outline.rgb,
        (_, ShieldLayer::Tint) => DS_FILL_RGB,
        (ShieldKind::IceBarrier, ShieldLayer::Rim) => IB_RIM_RGB,
        (ShieldKind::IceBarrier, ShieldLayer::StreakA) => IB_STREAK_A_RGB,
        (ShieldKind::IceBarrier, ShieldLayer::StreakB) => IB_STREAK_B_RGB,
        // The icicles carry their colour in their vertices.
        (_, ShieldLayer::Icicles | ShieldLayer::Glint) => [1.0, 1.0, 1.0],
        (_, ShieldLayer::Rim) => RIM_RGB,
        (ShieldKind::PowerWordShield, ShieldLayer::StreakA) => STREAK_A_RGB,
        (ShieldKind::DivineShield, ShieldLayer::StreakA) => DS_STREAK_A_RGB,
        (_, ShieldLayer::StreakB) => STREAK_B_RGB,
        (_, ShieldLayer::FillOuter | ShieldLayer::FillInner) => DS_FILL_RGB,
    }
}

fn layer_rgb(kind: ShieldKind, layer: ShieldLayer) -> Color {
    let c = shield_layer_rgb(kind, layer);
    let srgb = Color::srgb(c[0], c[1], c[2]);
    match layer {
        // The bench scales the outline's colour in linear light.
        ShieldLayer::Outline => {
            let l = srgb.to_linear() * shield_tuning(kind).outline.level;
            Color::LinearRgba(l.with_alpha(1.0))
        }
        _ => srgb,
    }
}

/// Whether a card is alpha-BLENDED (ours, AS-401) rather than the client's
/// additive light.
pub fn shield_layer_is_blended(layer: ShieldLayer) -> bool {
    matches!(layer, ShieldLayer::Outline | ShieldLayer::Tint)
}

/// The transparent pass's sort bias for a card. Every card of a shield stands
/// on one vertical, and the outline and tint share the rim card's centre, so
/// view depth alone cannot order them; Bevy adds this to the sort distance
/// (larger draws later, in front). The outline draws first, the tint over it,
/// then every additive card, which is the bench's order: blending after the
/// additive cards would dim their light. Small beside the rim's lead over the
/// body (0.11 yd up, at least 0.0078 of view depth at the flattest pitch the
/// camera allows), so every card still sorts in front of the body. Truncated
/// to an i32 for the depth-bias state, so the depth test is untouched.
pub fn shield_layer_sort_bias(layer: ShieldLayer) -> f32 {
    match layer {
        ShieldLayer::Outline => -0.002,
        ShieldLayer::Tint => -0.001,
        _ => 0.0,
    }
}

/// The client material: unlit, additive, no depth write, both faces. The
/// texture weight rides in the base colour's alpha (Add premultiplies by it).
/// `unlit` is the faithful choice here: the client adds colour × texture ×
/// weight and nothing else, which is exactly what the bench drew.
///
/// The outline and tint (AS-401) are the exception: alpha-blended, so they can
/// darken the sand, with their opacity in the base colour's alpha.
fn pws_material(
    texture: Option<Handle<Image>>,
    kind: ShieldKind,
    layer: ShieldLayer,
) -> StandardMaterial {
    StandardMaterial {
        base_color: layer_rgb(kind, layer).with_alpha(0.0),
        base_color_texture: texture,
        unlit: true,
        alpha_mode: if shield_layer_is_blended(layer) {
            AlphaMode::Blend
        } else {
            AlphaMode::Add
        },
        depth_bias: shield_layer_sort_bias(layer),
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

// --- Systems -----------------------------------------------------------------

/// Update (graphical-only): start, advance and end each bearer's shields, and
/// run Divine Shield's and Ice Barrier's emitters.
///
/// A living bearer whose live shield of a kind has no rig of that kind in birth
/// or loop gets a new one; one bearer can wear a rig of each kind. A rig whose
/// bearer no longer holds a live shield of its kind plays its death and then
/// despawns: the shield broke, ran out or was dispelled, or the bearer died
/// (auras stay on a corpse, so death ends the shield here, as it ends the aura
/// band). A rig whose bearer entity is gone despawns at once.
#[allow(clippy::too_many_arguments)]
pub fn update_shield_visuals(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<ShieldAssets>>,
    camera: Query<&Transform, (With<Camera3d>, Without<Combatant>, Without<VisualBody>)>,
    bearers: Query<(Entity, &Combatant, &ActiveAuras)>,
    placed: Query<(&Transform, Option<&Children>), With<Combatant>>,
    bodies: Query<&Transform, (With<VisualBody>, Without<Combatant>)>,
    mut rigs: Query<(
        Entity,
        &mut ShieldVisual,
        Option<&mut DivineShieldEmitters>,
        Option<&mut IceBarrierEmitters>,
    )>,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next().map(|t| t.translation);
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
    for (rig_entity, mut rig, emitters, mist) in rigs.iter_mut() {
        let Ok((bearer_tf, bearer_children)) = placed.get(rig.bearer) else {
            commands.entity(rig_entity).despawn();
            continue;
        };
        let kind = rig.kind;
        rig.t += dt;
        if rig.phase == ShieldPhase::Birth && rig.t >= shield_phase_secs(kind, ShieldPhase::Birth) {
            rig.t -= shield_phase_secs(kind, ShieldPhase::Birth);
            rig.phase = ShieldPhase::Loop;
        }
        let up = shielded.contains(&(rig.bearer, kind));
        if rig.phase != ShieldPhase::Death && !up {
            let now = shield_sample(&rig);
            rig.from_scale = now.scale;
            rig.from_rim = now.weight(ShieldLayer::Rim);
            rig.from_fill = now.weight(ShieldLayer::FillOuter);
            rig.from_env = now.env;
            rig.phase = ShieldPhase::Death;
            rig.t = 0.0;
        }
        let ended =
            rig.phase == ShieldPhase::Death && rig.t >= shield_phase_secs(kind, ShieldPhase::Death);
        let body = bearer_children
            .and_then(|kids| kids.iter().find_map(|k| bodies.get(k).ok()))
            .map(|body| body.translation)
            .unwrap_or(Vec3::ZERO);
        let body_centre = bearer_tf.transform_point(body);
        if let (Some(mut emitters), false) = (emitters, ended) {
            let feet = body_centre + Vec3::Y * (DS_LIFT - BODY_HALF_HEIGHT);
            let assets = assets.get_or_insert_with(|| {
                ShieldAssets::build(&mut meshes, &mut images, &mut materials)
            });
            emit_divine_shield_particles(&mut commands, assets, &rig, &mut emitters, feet, dt);
        }
        if let (Some(mut mist), Some(cam), true) = (mist, cam, ib_mist_spawning(&rig)) {
            let feet = body_centre + Vec3::Y * (IB_LIFT - BODY_HALF_HEIGHT);
            let assets = assets.get_or_insert_with(|| {
                ShieldAssets::build(&mut meshes, &mut images, &mut materials)
            });
            emit_ice_barrier_mist(&mut commands, assets, &rig, &mut mist, feet, cam, dt);
        }
        if rig.phase == ShieldPhase::Death {
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
            .get_or_insert_with(|| ShieldAssets::build(&mut meshes, &mut images, &mut materials));
        let mut rig = commands.spawn((
            ShieldVisual {
                bearer,
                kind,
                phase: ShieldPhase::Birth,
                t: 0.0,
                from_scale: Vec2::ZERO,
                from_rim: 0.0,
                from_fill: 0.0,
                from_env: 0.0,
            },
            // The rig stays at the origin, so each card's local pose is its world pose.
            Transform::IDENTITY,
            Visibility::default(),
            PlayMatchEntity,
        ));
        match kind {
            ShieldKind::DivineShield => {
                rig.insert(DivineShieldEmitters::default());
            }
            ShieldKind::IceBarrier => {
                rig.insert(IceBarrierEmitters::default());
            }
            ShieldKind::PowerWordShield => {}
        }
        rig.with_children(|rig| {
            for &layer in kind.layers() {
                let mesh = match layer {
                    ShieldLayer::Icicles => assets.icicles.clone(),
                    // Each glint owns its mesh: its sheen follows its own view.
                    ShieldLayer::Glint => {
                        meshes.add(ib_glint_mesh(PWS_CLIENT_TO_RIG * IB_SIZE, 0.0))
                    }
                    _ => assets.quad.clone(),
                };
                let texture = texture_slot(kind, layer).map(|i| assets.textures[i].clone());
                let mut card = rig.spawn((
                    ShieldCard { layer },
                    Mesh3d(mesh),
                    MeshMaterial3d(materials.add(pws_material(texture, kind, layer))),
                    // Zero-sized until the first placement: the birth's first 200 ms are empty.
                    Transform::from_scale(Vec3::splat(PWS_MIN_SCALE)),
                    NotShadowCaster,
                ));
                if layer == ShieldLayer::Glint {
                    card.insert(GlintSheen::default());
                }
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
    assets: &ShieldAssets,
    rig: &ShieldVisual,
    emitters: &mut DivineShieldEmitters,
    feet: Vec3,
    dt: f32,
) {
    let r = PWS_CLIENT_TO_RIG * DS_SIZE;
    let ms = rig.t / DS_TIME_SCALE * 1000.0;
    let ms = if rig.phase == ShieldPhase::Loop {
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
        let sign: i16 = if emitters.emitted.is_multiple_of(2) {
            1
        } else {
            -1
        };
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

/// Emits this frame's frost-mist puffs for one Ice Barrier rig, as world-space
/// `ClientParticle`s that `animate_client_particles` ages, moves, ramps and
/// retires and `billboard_hunter_shots` turns to the camera, rolling as they
/// fall. `feet` is the bearer's rendered feet, `cam` the camera's position.
fn emit_ice_barrier_mist(
    commands: &mut Commands,
    assets: &ShieldAssets,
    rig: &ShieldVisual,
    mist: &mut IceBarrierEmitters,
    feet: Vec3,
    cam: Vec3,
    dt: f32,
) {
    let r = PWS_CLIENT_TO_RIG;
    let life = IB_MIST_LIFE * IB_TIME_SCALE;
    // A per-rig stream: the bearer's index keeps two barriers' mist apart.
    let base = rig.bearer.index().wrapping_mul(0x9E37_79B9);
    mist.mist_carry += IB_MIST_PER_SEC * IB_MIST_RATE * dt;
    while mist.mist_carry >= 1.0 {
        mist.mist_carry -= 1.0;
        let seed = base.wrapping_add(mist.emitted.wrapping_mul(2_654_435_761));
        mist.emitted = mist.emitted.wrapping_add(1);
        let (at, velocity) = ib_mist_emit(seed, feet, cam, IB_MIST_SPREAD);
        let variant = (draw(seed, 6) * IB_MIST_VARIANTS as f32) as usize;
        let palette = &assets.mist_palettes[variant.min(assets.mist_palettes.len() - 1)];
        spawn_shield_particle(
            commands,
            assets,
            palette,
            ClientParticle {
                age: 0.0,
                life,
                velocity,
                gravity: 0.0,
                size: IB_MIST_DIAMETER.map(|d| d * r * IB_MIST_SIZE),
                mid: 0.5,
                palette: palette.clone(),
                step: 0,
                facing: ParticleFacing::Spin {
                    mrad_per_sec: (IB_MIST_ROLL / life * 1000.0) as i16,
                    start_mrad: (draw(seed, 7) * TAU * 1000.0) as i16,
                },
                owned: false,
            },
            at,
        );
    }
}

fn spawn_shield_particle(
    commands: &mut Commands,
    assets: &ShieldAssets,
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
pub fn place_shield_cards(
    camera: Query<&Transform, (With<Camera3d>, Without<ShieldCard>)>,
    rigs: Query<(&ShieldVisual, &Children)>,
    bearers: Query<(&Transform, Option<&Children>), (With<Combatant>, Without<ShieldCard>)>,
    bodies: Query<&Transform, (With<VisualBody>, Without<ShieldCard>, Without<Combatant>)>,
    mut cards: Query<
        (
            &ShieldCard,
            &mut Transform,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Option<&mut GlintSheen>,
        ),
        Without<Combatant>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
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
        let sample = shield_sample(rig);
        for child in children.iter() {
            let Ok((card, mut tf, mesh, material, sheen)) = cards.get_mut(child) else {
                continue;
            };
            let at = pws_card_translation(
                rig.kind,
                body_centre,
                card.layer,
                sample.scale,
                cam.translation,
            );
            tf.translation = at;
            tf.rotation = pws_card_rotation(at, cam.translation);
            tf.scale = shield_card_scale(rig.kind, card.layer, sample.scale);
            // Ice Barrier's glint is view-dependent: rewrite its sheen when the
            // camera has turned about the bearer.
            if let Some(mut sheen) = sheen {
                let yaw = ib_view_yaw(body_centre, cam.translation);
                if (yaw - sheen.yaw).abs() > 1e-4 {
                    sheen.yaw = yaw;
                    if let Some(m) = meshes.get_mut(&mesh.0) {
                        *m = ib_glint_mesh(PWS_CLIENT_TO_RIG * IB_SIZE, yaw);
                    }
                }
            }
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
