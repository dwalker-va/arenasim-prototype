use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::TAU;

use super::body_hold::{BodyHold, HoldDisjoint};
use super::heal_impact::{emissive_of, heal_jitter, holy_gold_pale, nature_green};
use super::hunter_shots::{ramp3, ramp_color, PALETTE_STEPS};
use super::ice_barrier::{
    ib_hand_envelope, ib_hand_glow_texture, ib_hand_pulse, ib_hand_ring_texture,
    ib_hand_spark_local, IB_HAND_GLOW, IB_HAND_GLOW_RGB, IB_HAND_RING, IB_HAND_RING_ALPHA,
    IB_HAND_RING_RGB, IB_HAND_SECS, IB_HAND_SPARKS, IB_HAND_SPARK_ALPHA, IB_HAND_SPARK_RGB,
    IB_HAND_SPARK_SIZE,
};
use super::shield::{disc_texture, PWS_CLIENT_TO_RIG};
use super::spell_bolts::{soft_dot_texture, star_flash_texture};
use crate::states::play_match::components::*;
use std::sync::Arc;

// ==============================================================================
// Cast-side heal visuals — the caster's glowing spell hands (graphical-only)
// ==============================================================================
//
// The caster-side counterpart of `heal_impact.rs`. Until this module every
// hard cast — heals included — telegraphed as the same generic gathering orb.
// The Classic client shows something entirely different for heals (measured
// verdict: `docs/design/2026-09-06-cast-side-heal-client-data.md`, wago.tools
// DB2 + M2 parsing, build 1.15.9.69547): the whole cast-side show lives on the
// caster's TWO SPELL HANDS (attach 21/22, always both, pure symmetric
// attachment), in two family dressings over one shared skeleton:
//
// - **Holy** (Priest + Paladin: Flash Heal, Holy Light, Flash of Light) —
//   `holy_precast_low_hand.m2` per hand: a camera-facing two-layer gold glow
//   ball (0.88 + 0.54 u quads) with three wide (0.33 u) gold ribbon wisps
//   swirling around it on a 1 s cycle. The launch (kit 270) is literally the
//   SAME model replayed as a one-shot — a brief re-flare of the identical
//   glow-and-wisps. Holy has no dedicated launch vocabulary.
// - **Nature** (Shaman: Lesser Healing Wave) — `nature_precast_low_hand.m2`:
//   the same swirl skeleton re-dressed green and smaller (0.76 + 0.36 u
//   glows, 0.10 u star-thread wisps) plus a lazy shed of leaves (11/s/hand,
//   0.086 u/s omni drift, 0.8 s life). Nature spends its novelty budget on a
//   dedicated launch model instead: `nature_cast_hand.m2`, a ~0.3 s tight
//   (~2°) jet of water-ring shockwaves (21.4/s) and gold glow sparks.
//
// The Druid (`docs/design/2026-10-03-druid-client-data.md`) draws the Nature
// hands on every cast but Moonfire, whose kit 730 (`magic_cast_hand.m2`) is
// the third family, Arcane: a 1000 ms one-shot sparkle with no precast. Most
// Druid casts are instants, which play the launch alone as they land
// (`spawn_instant_cast_hands`) — there is no cast window to loop through.
// Ice Barrier's kit 207 (`ice_precast_high_hand.m2`) is the fourth, Frost: a
// 1000 ms deep-blue glow and pulsing ring with sparks, also an instant's
// launch only (`docs/design/2026-10-17-ice-barrier-client-data.md`).
// The Paladin talent instants (AS-374,
// `docs/design/2026-10-18-paladin-talent-client-data.md`) add the fifth, the
// instant Holy flash, and reuse the Arcane hands for Divine Favor (kit 425 is
// Moonfire's `magic_cast_hand.m2`); both flash off their aura's landing.
// The Priest's Shadow casts (AS-449) are the sixth, Shadow: the Warlock's
// `shadow_precast_low_hand.m2` licks and rings (*The Shadow hands* below),
// looped through Mind Blast's cast bar and played alone by its instants.
//
// Timing is event-edge-driven in the source: the precast loop lives from the
// cast-start edge to the cast-end edge — its duration IS the actual cast
// time, decided by gameplay, never a visual constant — and an interrupted
// cast fires the end edge without the launch edge, so the hand glow and body
// loop simply STOP DEAD. No interrupt flourish exists, and none is added.
//
// The body language is the omni cast pair (52 ReadySpellOmni looping, then
// 54 SpellCastOmni at launch), approximated on the limbless capsule by
// `update_heal_cast_posture`: a slight hands-raised back-lean through the
// loop and a forward release surge at launch.
//
// Everything here is additive (every material and emitter in the source set
// is M2 blend 4), rides the `VisualBody` rigidly (rig = child at the spell-
// hand socket, so walk bob / posture / facing carry it with zero per-frame
// sim reads — the fixed-timestep strobe lesson), and draws no `game_rng`
// (scatter is position-hashed off a monotonic counter).

// --- The spell-hand sockets ---------------------------------------------------

/// Where a spell hand sits, local to the [`VisualBody`] (capsule radius 0.5,
/// height 2.5). X matches the weapon-mount hand line (`weapon_mount` in
/// `play_match/mod.rs` puts every grip at 0.7–0.8 |x|; the Paladin's mace —
/// the one weapon a heal caster in this set actually holds — grips at 0.72),
/// so the glow lights up where the hands already demonstrably are. Z presents
/// the hands slightly forward of the grips — raised toward the target, the
/// ReadySpellOmni read. Clears the 0.5 capsule radius with margin, like every
/// weapon mount.
pub const SPELL_HAND_X: f32 = 0.72;
pub const SPELL_HAND_Y: f32 = 0.05;
pub const SPELL_HAND_Z: f32 = 0.30;

/// The socket for one hand; `side` is +1 (main-hand side) or -1 (off).
pub fn spell_hand_local(side: f32) -> Vec3 {
    Vec3::new(SPELL_HAND_X * side, SPELL_HAND_Y, SPELL_HAND_Z)
}

// --- Blessed defaults (workshop) ---------------------------------------------

/// Global multiplier on the glow quad sizes.
pub const GLOW_SCALE: f32 = 1.0;
/// Holy glow quad sizes per hand (outer soft, brighter core), yards.
pub const HOLY_CAST_GLOW_SIZES: [f32; 2] = [0.88, 0.54];
/// Nature glow quad sizes per hand, yards.
pub const NATURE_CAST_GLOW_SIZES: [f32; 2] = [0.76, 0.36];
/// Width of a Holy ribbon wisp (source half-widths 0.167 -> 0.33 u full).
pub const HOLY_CAST_WISP_WIDTH: f32 = 0.33;
/// Width of a Nature star-thread wisp (source 0.08–0.11 u; blessed 0.10).
pub const NATURE_CAST_WISP_WIDTH: f32 = 0.10;
/// The three wisps' trail lifetimes, seconds (source edge lifetimes).
pub const HEAL_CAST_WISP_LIFETIMES: [f32; 3] = [0.40, 0.60, 0.50];
/// Orbit rate of the wisp swirl, cycles per second (the model's 1000 ms
/// loop). Shared by both families — same rig template in the source.
pub const HOLY_CAST_ORBIT_HZ: f32 = 1.0;
/// Orbit radius of the wisp heads around the hand (source pivots ~0.2 u out).
pub const HEAL_CAST_WISP_ORBIT_RADIUS: f32 = 0.20;

/// Holy launch: the one-shot re-flare of the SAME glow, and how hard it hits.
pub const HOLY_LAUNCH_FLARE_INTENSITY: f32 = 1.0;
pub const HOLY_LAUNCH_FLARE_SECS: f32 = 0.45;
/// Flash of Light carries no cast kit in the source (its cast, uniquely,
/// just stops). The blessed call overrides that: FoL launches with the
/// standard Holy re-flare. Flip to `false` to restore the source's silence.
pub const FLASH_OF_LIGHT_HAS_LAUNCH_FLASH: bool = true;

/// Nature loop: the leaf shed (rate per second PER HAND, drift speed u/s,
/// life seconds — transcribed from the parsed leaf emitter).
pub const NATURE_CAST_LEAF_RATE: f32 = 11.0;
pub const NATURE_CAST_LEAF_SPEED: f32 = 0.086;
pub const NATURE_CAST_LEAF_LIFE: f32 = 0.8;
const NATURE_LEAF_SIZE: f32 = 0.055;

/// Nature launch: the water-ring + gold-spark jet (`nature_cast_hand.m2`).
/// Rates per second per hand, mote life and speeds transcribed; the ~2°
/// spread is the source's 0.035 rad directional collimation.
pub const NATURE_LAUNCH_BURST_SECS: f32 = 0.3;
pub const NATURE_LAUNCH_RING_RATE: f32 = 21.4;
pub const NATURE_LAUNCH_SPARK_RATE: f32 = 30.0;
pub const NATURE_LAUNCH_SPREAD: f32 = 0.035;
pub const NATURE_LAUNCH_BURST_SCALE: f32 = 1.0;
const NATURE_LAUNCH_RING_SPEED: f32 = 0.261;
const NATURE_LAUNCH_SPARK_SPEED: f32 = 0.278;
// Ring/spark sizes, expansion, brightness and ring alpha are repo render
// tuning, not transcribed source values (the source gives textures, rates,
// speeds, life and spread only). USER-TUNED down in round 2: at 0.16 u /
// swell 2.5 / emissive 1.8 the ~6 rings alive at once (21.4/s x 0.3 s life)
// stacked additively into two blown-out white donuts that swallowed the
// hands. The launch is an accent on the cast, not the centerpiece — the
// blessed rates/speeds/life/spread above are untouched.
const NATURE_LAUNCH_RING_SIZE: f32 = 0.11;
const NATURE_LAUNCH_SPARK_SIZE: f32 = 0.055;
/// How far a water ring expands over its life, as a fraction of its size
/// (scale grows to `1 + SWELL` as it fades).
const NATURE_LAUNCH_RING_SWELL: f32 = 1.6;
/// Emissive strengths of the burst materials (the loop glow uses 2.0/2.8).
const NATURE_LAUNCH_RING_EMISSIVE: f32 = 0.9;
const NATURE_LAUNCH_SPARK_EMISSIVE: f32 = 1.5;
/// Base-color alpha of the ring material: additive rings overlap heavily
/// while young, so each contributes well under full strength.
const NATURE_LAUNCH_RING_ALPHA: f32 = 0.45;

/// Seconds the hand glow blooms in from nothing at cast start. The source
/// snaps (InitialAnimID -1, no intro); a couple of frames of growth just
/// keeps the pop from strobing at render rates above the tick rate.
pub const HEAL_CAST_BLOOM_SECS: f32 = 0.15;

// --- Body posture (ReadySpellOmni -> SpellCastOmni, capsule dialect) ----------

/// Torso pitch of the hands-raised precast loop, radians. NEGATIVE leans the
/// torso BACK (chest open, hands presented) — the ReadySpellOmni read.
pub const HEAL_CAST_READY_PITCH: f32 = -0.10;
/// Peak forward pitch of the release surge — SpellCastOmni's both-hands
/// upward/outward throw, read as the torso committing toward the target.
pub const HEAL_CAST_RELEASE_PITCH: f32 = 0.16;
/// Seconds the loop pitch eases in over at cast start.
pub const HEAL_CAST_POSTURE_EASE_SECS: f32 = 0.25;

// --- The instant Holy hand flash (AS-374) ------------------------------------
//
// The Paladin talent instants with a Holy cast kit (Aura Mastery's 191,
// Blessing of Sacrifice's 186/189 — `holy_precast_high_hand.m2` and
// `holy_precast_med_hand.m2`) have no cast window, so the precast loop never
// plays. They flash the Holy glow and wisps once, 0.7 s on both hands, gold,
// under the AS-337 bench's hand envelope (signed off at version
// 1792379243-e3d6, `castHands`): in by 80 ms, 0.8 at 420 ms (60% of the
// flash), out at 700 ms. The Sacred Shield, Divine Protection and Repentance
// cards reuse it (`HealCastKind::HolyFlash`).

/// How long the instant Holy hand flash plays.
pub const HOLY_INSTANT_FLASH_SECS: f32 = 0.7;
/// Its alpha envelope, `(seconds, weight)`.
pub const HOLY_INSTANT_FLASH_ENVELOPE: [(f32, f32); 4] =
    [(0.0, 0.0), (0.08, 1.0), (0.42, 0.8), (0.7, 0.0)];

/// The instant Holy hand flash's weight `age` seconds in.
pub fn holy_instant_flash_alpha(age: f32) -> f32 {
    let e = &HOLY_INSTANT_FLASH_ENVELOPE;
    if age <= e[0].0 || age >= e[e.len() - 1].0 {
        return 0.0;
    }
    for pair in e.windows(2) {
        let ((t0, a0), (t1, a1)) = (pair[0], pair[1]);
        if age <= t1 {
            return a0 + (a1 - a0) * (age - t0) / (t1 - t0);
        }
    }
    0.0
}

// --- Instants (the Druid's zero-length casts) ---------------------------------

/// A Nature instant plays the cast kit alone — the launch jet — as it lands
/// (the (3,13) cast event; an instant has no precast window). The hand glow
/// re-flares with it, so the green reads at a glance; `false` leaves the jet
/// alone.
pub const NATURE_INSTANT_HAND_GLOW: bool = true;

// --- Moonfire's arcane hands (kit 730, `magic_cast_hand.m2`) -----------------
//
// AS-160 bench sign-off (`docs/design/2026-10-03-druid-client-data.md`,
// `docs/design/benches/2026-10-03-druid-visuals-bench.html`): three emitters
// per hand over the 1000 ms kit — `cyan_glow3` motes, `star5a` twinkles, and
// a `teleporttarget` ring disc for the first 300 ms. Geometry and tracks are
// the bench's drawing code, ported.

pub const MOONFIRE_CAST_HANDS: bool = true;
pub const MOONFIRE_HAND_SIZE_MUL: f32 = 1.00;
/// The kit's length — the model's single 1000 ms sequence.
pub const MOONFIRE_HAND_SECS: f32 = 1.0;
/// Motes and stars per hand.
pub const ARCANE_HAND_MOTES: u32 = 5;
/// One mote's rise, seconds per cycle; the five are staggered a fifth apart.
pub const ARCANE_MOTE_CYCLE_SECS: f32 = 0.8;
/// Quad diameters, yards (the bench's sprite sizes).
pub const ARCANE_MOTE_SIZE: f32 = 0.32;
pub const ARCANE_STAR_SIZE: f32 = 0.30;
/// The ring disc's window, and its diameter at the start and end of it.
pub const ARCANE_RING_SECS: f32 = 0.30;
pub const ARCANE_RING_DIAMETERS: [f32; 2] = [0.30, 1.50];
/// Seconds per radian of a star's spin.
const ARCANE_STAR_SPIN_SECS: f32 = 0.30;
const ARCANE_EMISSIVE: f32 = 2.0;

/// Colour tracks, 0..255 keyed on a particle's phase (the kit's tracks).
const ARCANE_MOTE_RGB: [(f32, [f32; 3]); 3] = [
    (0.0, [167.0, 84.0, 244.0]),
    (0.55, [62.0, 227.0, 229.0]),
    (1.0, [250.0, 230.0, 2.0]),
];
const ARCANE_STAR_RGB: [(f32, [f32; 3]); 3] = [
    (0.0, [0.0, 221.0, 62.0]),
    (0.5, [183.0, 250.0, 251.0]),
    (1.0, [246.0, 211.0, 20.0]),
];
const ARCANE_RING_RGB: [(f32, [f32; 3]); 3] = [
    (0.0, [255.0, 234.0, 0.0]),
    (0.35, [173.0, 89.0, 238.0]),
    (1.0, [30.0, 236.0, 136.0]),
];
const ARCANE_MOTE_ALPHA: [(f32, f32); 3] = [(0.0, 0.25), (0.55, 0.99), (1.0, 0.04)];
const ARCANE_STAR_ALPHA: [(f32, f32); 3] = [(0.0, 0.44), (0.5, 0.79), (1.0, 0.0)];
const ARCANE_RING_ALPHA: f32 = 0.7;

fn keyed_rgb(track: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    for pair in track.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        if t <= t1 {
            let u = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            return [0, 1, 2].map(|i| (a[i] + (b[i] - a[i]) * u) / 255.0);
        }
    }
    track[track.len() - 1].1.map(|c| c / 255.0)
}

fn keyed_scalar(track: &[(f32, f32)], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    for pair in track.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        if t <= t1 {
            let u = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            return a + (b - a) * u;
        }
    }
    track[track.len() - 1].1
}

/// A mote's colour (sRGB 0..1) at phase `t`: violet -> cyan -> gold.
pub fn arcane_mote_rgb(t: f32) -> [f32; 3] {
    keyed_rgb(&ARCANE_MOTE_RGB, t)
}

/// A star's colour at phase `t`: green -> pale cyan -> gold.
pub fn arcane_star_rgb(t: f32) -> [f32; 3] {
    keyed_rgb(&ARCANE_STAR_RGB, t)
}

/// The ring disc's colour at progress `t` through its window: gold ->
/// violet -> green.
pub fn arcane_ring_rgb(t: f32) -> [f32; 3] {
    keyed_rgb(&ARCANE_RING_RGB, t)
}

/// Phase 0..1 of the `index`th mote (and star) at `age`: each cycles every
/// [`ARCANE_MOTE_CYCLE_SECS`], staggered a fifth of a cycle apart.
pub fn arcane_phase(index: u32, age: f32) -> f32 {
    (age / ARCANE_MOTE_CYCLE_SECS + index as f32 / ARCANE_HAND_MOTES as f32).fract()
}

/// Rig-local position of the `index`th mote at `age`: a 0.15 yd scatter
/// about the hand that swings slowly fore and aft, rising 0.3 yd a cycle.
pub fn arcane_mote_local(index: u32, age: f32) -> Vec3 {
    let a = index as f32 * 2.4;
    Vec3::new(
        a.sin() * 0.15,
        arcane_phase(index, age) * 0.3,
        (a + age * 5.0).cos() * 0.15,
    )
}

/// Rig-local position of the `index`th star at `age`: just above the hand,
/// rising 0.2 yd a cycle.
pub fn arcane_star_local(index: u32, age: f32) -> Vec3 {
    Vec3::new(
        0.0,
        0.1 + arcane_phase(index, age) * 0.2,
        (index as f32 * 1.7).sin() * 0.12,
    )
}

/// The ring disc's diameter at `age`, swelling across its window.
pub fn arcane_ring_diameter(age: f32) -> f32 {
    let t = (age / ARCANE_RING_SECS).clamp(0.0, 1.0);
    (ARCANE_RING_DIAMETERS[0] + (ARCANE_RING_DIAMETERS[1] - ARCANE_RING_DIAMETERS[0]) * t)
        * MOONFIRE_HAND_SIZE_MUL
}

/// An arcane piece's pose and look at `age`: rig-local translation, quad
/// diameter, sRGB colour and alpha. Pure, so the probes and the update
/// system read one definition.
fn arcane_piece_state(role: HealCastPieceRole, age: f32) -> (Vec3, f32, [f32; 3], f32) {
    match role {
        HealCastPieceRole::ArcaneMote { index } => {
            let t = arcane_phase(index, age);
            (
                arcane_mote_local(index, age),
                ARCANE_MOTE_SIZE * MOONFIRE_HAND_SIZE_MUL,
                arcane_mote_rgb(t),
                keyed_scalar(&ARCANE_MOTE_ALPHA, t),
            )
        }
        HealCastPieceRole::ArcaneStar { index } => {
            let t = arcane_phase(index, age);
            (
                arcane_star_local(index, age),
                ARCANE_STAR_SIZE * MOONFIRE_HAND_SIZE_MUL,
                arcane_star_rgb(t),
                keyed_scalar(&ARCANE_STAR_ALPHA, t),
            )
        }
        _ => {
            let t = (age / ARCANE_RING_SECS).clamp(0.0, 1.0);
            (
                Vec3::ZERO,
                arcane_ring_diameter(age),
                arcane_ring_rgb(t),
                ARCANE_RING_ALPHA * (1.0 - t),
            )
        }
    }
}

/// A Frost piece's pose and look at `age` on the hand at `side`: rig-local
/// translation, quad diameter and alpha (its colour is fixed). Pure, the
/// bench's drawing code ported: the glow card and ring pulse together, the
/// sparks circle the hand, and all of it rides one envelope.
pub fn frost_piece_state(role: HealCastPieceRole, age: f32, side: f32) -> (Vec3, f32, f32) {
    let env = ib_hand_envelope(age);
    let pulse = ib_hand_pulse(age);
    match role {
        HealCastPieceRole::FrostGlow => (Vec3::ZERO, IB_HAND_GLOW * PWS_CLIENT_TO_RIG * pulse, env),
        HealCastPieceRole::FrostRing => (
            Vec3::ZERO,
            IB_HAND_RING * PWS_CLIENT_TO_RIG * pulse,
            IB_HAND_RING_ALPHA * env,
        ),
        HealCastPieceRole::FrostSpark { index } => (
            ib_hand_spark_local(index, age, side),
            IB_HAND_SPARK_SIZE,
            IB_HAND_SPARK_ALPHA * env,
        ),
        _ => (Vec3::ZERO, 0.0, 0.0),
    }
}

// --- The Shadow hands (AS-449, `shadow_precast_low_hand.m2`) ----------------
//
// Every precast and cast kit of the Priest's four Shadow visuals — Shadow
// Word: Pain (71: kits 217/218), Vampiric Embrace (3582: 217/218), Silence
// (179: 114/418) and Mind Blast (3057: 114/218) — attaches SpellVisualEffectName
// 17, `spells/shadow_precast_low_hand.m2` (fdid 166807), at scale 1 to both
// hands (attach 21 and 22). The same model is the Warlock's cast hand on kits
// 114/118 (`docs/design/2026-09-06-warlock-dot-client-data.md` §5); nothing
// drew it before this card, so this is its first implementation. The kits
// differ only in the body anim (Omni or Directed), which the capsule's cast
// posture does not distinguish.
//
// The model, parsed at build 1.15.9.69547: no mesh, one 1000 ms sequence, and
// three emitters, all point emitters at the hand, none with a window, all
// with flags 0x20129: 0x80 (model space) unset, so their particles live in
// the WORLD — born at the hand and left where they were born as the hand
// moves on — and 0x8 set, so they travel up in world space:
//
//   P0  `fire1a2`, blend 2 (alpha): 10.2/s, 1.2 s lives, rising 0.333 yd/s
//       (±35%), radius 0.222 -> 0.083 -> 0.028 yd, colour (32,255,2) ->
//       (2,2,2) -> (2,2,2), alpha 0.39 -> 1.0 -> 0. `fire1a2` decodes to
//       black (RGB 0, its shape in alpha), and the colour track multiplies
//       the texture, so the lick is a black puff from birth that thickens
//       and shrinks away.
//   P1  `shockwave10`, blend 4 (additive): 5/s, 0.5 s lives, still; radius
//       0.222 -> 0.361 -> 0.222 yd, (18,255,0) -> (10,148,0) -> (18,255,0),
//       alpha 1. A green ring that swells and shrinks.
//   P2  the same as P1 in violet: radius 0.278 -> 0.417 -> 0.278 yd,
//       (114,0,255) -> (55,0,124) -> (114,0,255).
//
// Every ramp keys its middle at half the life; a size key is the sprite's
// radius (`ParticleSize::radii`). The two sprites are generated from the
// decoded textures: `fire1a2`'s alpha averaged by radius over its black, and
// `shockwave10`'s luminance averaged by radius (a streaky ring peaking at 0.24
// at 0.72 of the radius) as white with that shape in alpha.
//
// Where the client gives nothing, a value is chosen and named here:
//
// - **How long the cast kit plays:** the model's one sequence, 1000 ms
//   ([`SHADOW_HAND_SECS`]), as Moonfire's and Ice Barrier's do. The emitters
//   then stop and the rig stays until its last particle has lived out its
//   life ([`shadow_hand_tail_secs`]), so the hands go out as the client's
//   particles do, never cut.
// - **Mind Blast's loop:** its precast kit (114) is the same model, so its
//   emitters run from the cast's start and the cast kit's second follows the
//   landing on the same clock; nothing is cut or restarted.
// - **Where in a frame a particle is born:** where the hand was at its birth,
//   the hand's path taken as straight across the frame
//   ([`place_shadow_hands`]); the client emits at the hand's pose then.
// - **Scale:** client yards on the rig, as the Holy hands (the same model
//   family) are drawn.
// - **The rings' glow:** [`SHADOW_HAND_RING_GLOW`], the additive particles'
//   2.5x emissive convention (`hunter_shots.rs`).

/// How long a Shadow cast kit plays: the model's single 1000 ms sequence.
pub const SHADOW_HAND_SECS: f32 = 1.0;
/// Emissive gain of the additive rings over their ramp colour.
pub const SHADOW_HAND_RING_GLOW: f32 = 2.5;
/// The sprites' side, pixels.
const SHADOW_SPRITE_PX: u32 = 64;

/// Which generated sprite a Shadow emitter draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadowHandSprite {
    /// `fire1a2`: a cloudy black puff (its alpha's radial profile, over
    /// its black).
    Lick,
    /// `shockwave10`: a ring (its luminance's radial profile).
    Ring,
}

/// One emitter of `shadow_precast_low_hand.m2`, as parsed.
#[derive(Clone, Copy, Debug)]
pub struct ShadowHandEmitter {
    pub name: &'static str,
    pub sprite: ShadowHandSprite,
    /// Blend 4 (additive) when true; blend 2 (alpha) when false.
    pub additive: bool,
    /// Particles a second.
    pub rate: f32,
    /// Seconds a particle lives.
    pub life: f32,
    /// Launch speed, yd/s, and its variation (a fraction either way).
    pub speed: f32,
    pub speed_var: f32,
    /// sRGB 0..255 at birth, midlife and death.
    pub color: [[u8; 3]; 3],
    pub alpha: [f32; 3],
    pub size: ParticleSize,
}

/// Where in a particle's life the middle ramp key sits (every key here is at
/// 0.500015 of the life).
pub const SHADOW_HAND_MID: f32 = 0.5;

/// The model's three emitters: P0 the lick, P1 the green ring, P2 the violet.
pub const SHADOW_HAND_EMITTERS: [ShadowHandEmitter; 3] = [
    ShadowHandEmitter {
        name: "shadow_precast_low_hand/p0 licks",
        sprite: ShadowHandSprite::Lick,
        additive: false,
        rate: 10.2,
        life: 1.2,
        speed: 0.333,
        speed_var: 0.35,
        color: [[32, 255, 2], [2, 2, 2], [2, 2, 2]],
        alpha: [0.392, 1.0, 0.0],
        size: ParticleSize::radii([0.222, 0.083, 0.028]),
    },
    ShadowHandEmitter {
        name: "shadow_precast_low_hand/p1 green ring",
        sprite: ShadowHandSprite::Ring,
        additive: true,
        rate: 5.0,
        life: 0.5,
        speed: 0.0,
        speed_var: 0.35,
        color: [[18, 255, 0], [10, 148, 0], [18, 255, 0]],
        alpha: [1.0, 1.0, 1.0],
        size: ParticleSize::radii([0.222, 0.361, 0.222]),
    },
    ShadowHandEmitter {
        name: "shadow_precast_low_hand/p2 violet ring",
        sprite: ShadowHandSprite::Ring,
        additive: true,
        rate: 5.0,
        life: 0.5,
        speed: 0.0,
        speed_var: 0.35,
        color: [[114, 0, 255], [55, 0, 124], [114, 0, 255]],
        alpha: [1.0, 1.0, 1.0],
        size: ParticleSize::radii([0.278, 0.417, 0.278]),
    },
];

/// `fire1a2`'s alpha, averaged over each twentieth of the radius (decoded
/// from the BLP).
const SHADOW_LICK_PROFILE: [f32; 20] = [
    0.889, 0.929, 0.965, 0.96, 0.926, 0.877, 0.828, 0.746, 0.673, 0.619, 0.547, 0.431, 0.333,
    0.256, 0.186, 0.11, 0.057, 0.023, 0.003, 0.0,
];
/// `shockwave10`'s luminance, averaged the same way.
const SHADOW_RING_PROFILE: [f32; 20] = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.005, 0.01, 0.021, 0.042, 0.097, 0.202, 0.242, 0.21,
    0.172, 0.131, 0.082, 0.027,
];

/// A radial profile of twenty bins at radius `r` (0 centre, 1 the quad's
/// edge): linear between the bins' centres, held inside the first and
/// falling to nothing at the edge past the last.
pub fn shadow_profile_at(profile: &[f32; 20], r: f32) -> f32 {
    let x = r * 20.0 - 0.5;
    if x <= 0.0 {
        return profile[0];
    }
    if r >= 1.0 {
        return 0.0;
    }
    let i = x.floor() as usize;
    if i >= 19 {
        let u = (r - 0.975) / 0.025;
        return profile[19] * (1.0 - u);
    }
    let u = x - i as f32;
    profile[i] + (profile[i + 1] - profile[i]) * u
}

/// A Shadow sprite's alpha at radius `r` of the quad.
pub fn shadow_sprite_alpha(sprite: ShadowHandSprite, r: f32) -> f32 {
    match sprite {
        ShadowHandSprite::Lick => shadow_profile_at(&SHADOW_LICK_PROFILE, r),
        ShadowHandSprite::Ring => shadow_profile_at(&SHADOW_RING_PROFILE, r),
    }
}

/// A Shadow sprite: its decoded radial profile in alpha, over its decoded
/// colour — `fire1a2`'s black, or white under `shockwave10`'s luminance.
pub fn shadow_sprite_texture(sprite: ShadowHandSprite) -> Image {
    let mut image = disc_texture(SHADOW_SPRITE_PX, |r, _| shadow_sprite_alpha(sprite, r));
    if sprite == ShadowHandSprite::Lick {
        if let Some(data) = image.data.as_mut() {
            for texel in data.as_chunks_mut::<4>().0 {
                texel[..3].fill(0);
            }
        }
    }
    image
}

/// How long a Shadow rig outlives its cast kit: the longest particle life,
/// so a particle born as the kit ends lives out its life.
pub fn shadow_hand_tail_secs() -> f32 {
    SHADOW_HAND_EMITTERS
        .iter()
        .map(|e| e.life)
        .fold(0.0, f32::max)
}

/// How many particles emitter `e` has emitted once the model has emitted for
/// `clock` seconds: one each time its rate's accumulator fills, so the first
/// comes `1 / rate` in. The epsilon absorbs a frame clock's summing error.
pub fn shadow_emitted_by(e: &ShadowHandEmitter, clock: f32) -> u32 {
    (e.rate * clock + 1e-4).floor().max(0.0) as u32
}

/// The `index`th particle's launch velocity, rig-local: straight up at the
/// emitter's speed, varied by a hashed fraction (never `game_rng`).
pub fn shadow_mote_velocity(e: &ShadowHandEmitter, seed: u32) -> Vec3 {
    let var = (heal_jitter(seed) * 2.0 - 1.0) * e.speed_var;
    Vec3::Y * e.speed * (1.0 + var)
}

/// A Shadow particle's drawn diameter `age` seconds into its life.
pub fn shadow_mote_diameter(e: &ShadowHandEmitter, age: f32) -> f32 {
    ramp3(e.size.diameter_keys(), SHADOW_HAND_MID, age / e.life).max(1e-4)
}

/// The palette step a Shadow particle `age` seconds into its life wears.
pub fn shadow_mote_step(e: &ShadowHandEmitter, age: f32) -> usize {
    let t = (age / e.life).clamp(0.0, 1.0);
    ((t * PALETTE_STEPS as f32) as usize).min(PALETTE_STEPS - 1)
}

/// The material a Shadow emitter's particles wear at palette step `step`:
/// the colour and alpha ramps sampled at the step's middle. The rings add
/// (blend 4, with emissive, unlit off or Bevy drops it); the lick blends
/// (blend 2: a flat, unlit colour that can darken).
pub fn shadow_palette_material(
    e: &ShadowHandEmitter,
    step: usize,
    texture: Handle<Image>,
) -> StandardMaterial {
    let t = (step as f32 + 0.5) / PALETTE_STEPS as f32;
    let color = ramp_color(e.color, SHADOW_HAND_MID, t);
    let alpha = ramp3(e.alpha, SHADOW_HAND_MID, t);
    if e.additive {
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: Some(texture.clone()),
            emissive: emissive_of(color, SHADOW_HAND_RING_GLOW),
            emissive_texture: Some(texture),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: Some(texture),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    }
}

// --- Pure geometry (probed directly) ------------------------------------------

/// How long a family's launch flare runs.
pub fn heal_cast_flare_secs(kind: HealCastKind) -> f32 {
    match kind {
        HealCastKind::Holy => HOLY_LAUNCH_FLARE_SECS,
        HealCastKind::Nature => NATURE_LAUNCH_BURST_SECS,
        HealCastKind::Arcane => MOONFIRE_HAND_SECS,
        HealCastKind::Frost => IB_HAND_SECS,
        HealCastKind::HolyFlash => HOLY_INSTANT_FLASH_SECS,
        // The cast kit's second, then the last particle's life.
        HealCastKind::Shadow => SHADOW_HAND_SECS + shadow_hand_tail_secs(),
    }
}

/// How long the release surge of the cast posture takes: the launch flare,
/// or for the Shadow hands the cast kit alone (their rig outlives it while
/// the last particles die).
pub fn heal_cast_release_secs(kind: HealCastKind) -> f32 {
    match kind {
        HealCastKind::Shadow => SHADOW_HAND_SECS,
        _ => heal_cast_flare_secs(kind),
    }
}

/// The glow quad sizes (outer, core) for a family. Arcane, Frost and Shadow
/// hands draw no glow ball of this kind; they share Nature's numbers only so
/// the table stays total.
pub fn heal_cast_glow_sizes(kind: HealCastKind) -> [f32; 2] {
    let sizes = match kind {
        HealCastKind::Holy | HealCastKind::HolyFlash => HOLY_CAST_GLOW_SIZES,
        HealCastKind::Nature
        | HealCastKind::Arcane
        | HealCastKind::Frost
        | HealCastKind::Shadow => NATURE_CAST_GLOW_SIZES,
    };
    [sizes[0] * GLOW_SCALE, sizes[1] * GLOW_SCALE]
}

/// A family's wisp width (Arcane, Frost and Shadow draw no wisps; see above).
pub fn heal_cast_wisp_width(kind: HealCastKind) -> f32 {
    match kind {
        HealCastKind::Holy | HealCastKind::HolyFlash => HOLY_CAST_WISP_WIDTH,
        HealCastKind::Nature
        | HealCastKind::Arcane
        | HealCastKind::Frost
        | HealCastKind::Shadow => NATURE_CAST_WISP_WIDTH,
    }
}

/// Rendered trail length of the `index`th wisp: the arc its head sweeps in
/// one edge lifetime at the orbit's linear speed.
pub fn heal_cast_wisp_length(index: u32) -> f32 {
    TAU * HOLY_CAST_ORBIT_HZ
        * HEAL_CAST_WISP_ORBIT_RADIUS
        * HEAL_CAST_WISP_LIFETIMES[index as usize % 3]
}

/// The `index`th wisp's orbit plane. The source hangs the three ribbon heads
/// off three independent bone chains pivoting in three different directions;
/// three fixed, visibly distinct plane tilts reproduce that read.
pub fn heal_cast_wisp_plane(index: u32) -> Quat {
    match index % 3 {
        0 => Quat::from_rotation_x(0.45),
        1 => Quat::from_rotation_z(0.55) * Quat::from_rotation_x(-0.30),
        _ => Quat::from_rotation_x(-0.50) * Quat::from_rotation_z(-0.35),
    }
}

/// Rig-local position of the `index`th wisp head at `age` seconds into the
/// cast: an orbit around the hand in the wisp's own tilted plane, all three
/// on the shared 1 s cycle, phase-staggered a third of a turn apart.
pub fn heal_cast_wisp_head(index: u32, age: f32) -> Vec3 {
    let angle = index as f32 * TAU / 3.0 + TAU * HOLY_CAST_ORBIT_HZ * age;
    heal_cast_wisp_plane(index)
        * Vec3::new(angle.cos(), 0.0, angle.sin())
        * HEAL_CAST_WISP_ORBIT_RADIUS
}

/// Rig-local direction the `index`th wisp head is moving at `age` — the
/// orbit tangent its trail streams back along.
pub fn heal_cast_wisp_tangent(index: u32, age: f32) -> Vec3 {
    let angle = index as f32 * TAU / 3.0 + TAU * HOLY_CAST_ORBIT_HZ * age;
    heal_cast_wisp_plane(index) * Vec3::new(-angle.sin(), 0.0, angle.cos())
}

/// The launch jet's rig-local direction: forward along the facing, tilted up
/// — hands throwing the heal out and up toward the target, the SpellCastOmni
/// release line.
pub fn nature_launch_jet_dir() -> Vec3 {
    Vec3::new(0.0, 0.35, 1.0).normalize()
}

/// Torso pitch of the release surge at normalized flare progress `t` in
/// 0..1: a fast commit forward, decaying back to neutral by the flare's end.
pub fn heal_cast_release_pitch(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    HEAL_CAST_RELEASE_PITCH * (t / 0.25).clamp(0.0, 1.0) * (1.0 - t)
}

// --- Palette ------------------------------------------------------------------

/// The three Holy ribbon tints (transcribed ribbon color tracks).
/// The `index`th wisp's colour in a family: the Holy ribbons (the instant
/// Holy flash's too) or the Nature star threads.
pub fn heal_cast_wisp_color(kind: HealCastKind, index: u32) -> Color {
    match kind {
        HealCastKind::Holy | HealCastKind::HolyFlash => holy_wisp_color(index),
        HealCastKind::Nature
        | HealCastKind::Arcane
        | HealCastKind::Frost
        | HealCastKind::Shadow => nature_wisp_color(index),
    }
}

fn holy_wisp_color(index: u32) -> Color {
    match index % 3 {
        0 => Color::srgb(0.988, 0.863, 0.165),
        1 => Color::srgb(0.992, 0.804, 0.290),
        _ => Color::srgb(0.984, 0.855, 0.145),
    }
}

/// The three Nature star-thread tints (transcribed).
// 0.318 is a transcribed green channel, not an approximation of 1/PI.
#[allow(clippy::approx_constant)]
fn nature_wisp_color(index: u32) -> Color {
    match index % 3 {
        0 => Color::srgb(0.235, 1.0, 0.0),
        1 => Color::srgb(0.318, 1.0, 0.106),
        _ => Color::srgb(0.235, 1.0, 0.0),
    }
}

/// `leafbrown` rendered additively — a warm amber.
fn leaf_brown() -> Color {
    Color::srgb(0.62, 0.44, 0.20)
}

/// The launch rings' pale water-blue (`shockwavewater1`).
fn water_ring_blue() -> Color {
    Color::srgb(0.55, 0.75, 0.95)
}

/// A family's hand-glow colour. The instant Holy flash is the Holy hands, so
/// it glows exactly as they do.
pub fn heal_cast_glow_color(kind: HealCastKind) -> Color {
    match kind {
        HealCastKind::Holy | HealCastKind::HolyFlash => holy_gold_pale(),
        HealCastKind::Nature
        | HealCastKind::Arcane
        | HealCastKind::Frost
        | HealCastKind::Shadow => nature_green(),
    }
}

/// Source alphas: Holy ribbons 0.80, Nature threads 0.70.
fn wisp_alpha(kind: HealCastKind) -> f32 {
    match kind {
        HealCastKind::Holy | HealCastKind::HolyFlash => 0.80,
        HealCastKind::Nature
        | HealCastKind::Arcane
        | HealCastKind::Frost
        | HealCastKind::Shadow => 0.70,
    }
}

// --- Shared assets ------------------------------------------------------------

/// Meshes and sprites every hand rig shares. Built once, lazily.
pub struct HealCastAssets {
    quad: Handle<Mesh>,
    ring: Handle<Mesh>,
    /// Moonfire's ring disc: a thin band at 0.85 of the radius, 0.16 wide
    /// (the bench's stroked circle), scaled to the disc's radius.
    arcane_ring: Handle<Mesh>,
    star: Handle<Image>,
    dot: Handle<Image>,
    /// Ice Barrier's glow card and ring sprites.
    frost_glow: Handle<Image>,
    frost_ring: Handle<Image>,
    /// The Shadow hands' lick and ring sprites, and their emitters'
    /// palettes (built the first time a Shadow rig spawns).
    shadow_lick: Handle<Image>,
    shadow_ring: Handle<Image>,
    shadow_palettes: Option<[Arc<[Handle<StandardMaterial>]>; 3]>,
}

impl HealCastAssets {
    pub(super) fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            // The water-ring sprite: a thin annulus, billboarded and expanded
            // over its life (the `hard_cc.rs` CC-flare idiom).
            ring: meshes.add(Annulus::new(0.72, 1.0).mesh().resolution(40)),
            arcane_ring: meshes.add(Annulus::new(0.77, 0.93).mesh().resolution(40)),
            star: images.add(star_flash_texture()),
            dot: images.add(soft_dot_texture()),
            frost_glow: images.add(ib_hand_glow_texture()),
            frost_ring: images.add(ib_hand_ring_texture()),
            shadow_lick: images.add(shadow_sprite_texture(ShadowHandSprite::Lick)),
            shadow_ring: images.add(shadow_sprite_texture(ShadowHandSprite::Ring)),
            shadow_palettes: None,
        }
    }

    /// The three Shadow emitters' palettes, built once.
    fn shadow_palettes(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
    ) -> [Arc<[Handle<StandardMaterial>]>; 3] {
        let (lick, ring) = (self.shadow_lick.clone(), self.shadow_ring.clone());
        self.shadow_palettes
            .get_or_insert_with(|| {
                SHADOW_HAND_EMITTERS.map(|e| {
                    let texture = match e.sprite {
                        ShadowHandSprite::Lick => lick.clone(),
                        ShadowHandSprite::Ring => ring.clone(),
                    };
                    (0..PALETTE_STEPS)
                        .map(|step| {
                            materials.add(shadow_palette_material(&e, step, texture.clone()))
                        })
                        .collect()
                })
            })
            .clone()
    }
}

fn additive_material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    strength: f32,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: texture.clone(),
        emissive: emissive_of(color, strength),
        emissive_texture: texture,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

// --- Systems ------------------------------------------------------------------

/// Spawn the two hand rigs (and the cast posture) when a combatant starts a
/// hard cast routed to a hand family. Every other hard cast is untouched — it
/// keeps the generic casting orb (`spawn_casting_orbs` skips routed casts;
/// this is the replacement). A zero-length cast is left to
/// [`spawn_instant_cast_hands`], and so is a family with no precast loop.
pub fn spawn_heal_cast_glows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HealCastAssets>>,
    new_casts: Query<(Entity, &CastingState, &Children), Added<CastingState>>,
    bodies: Query<(), With<VisualBody>>,
    existing: Query<&HealCastHand>,
) {
    for (caster, casting, children) in new_casts.iter() {
        let Some(kind) = HealCastKind::for_ability(casting.ability) else {
            continue;
        };
        if !kind.has_precast() || casting.time_remaining <= 0.0 {
            continue;
        }
        // A cast already flagged interrupted never lights up (the casting-orb
        // guard: its ending marker was already consumed).
        if casting.interrupted {
            continue;
        }
        // One live LOOP per caster. Scoped to Loop — a back-to-back heal can
        // begin while the previous launch's flare is still playing out, and
        // must not be swallowed by it.
        if existing
            .iter()
            .any(|rig| rig.caster == caster && matches!(rig.phase, HealCastPhase::Loop))
        {
            continue;
        }
        let Some(body) = children.iter().find(|&child| bodies.get(child).is_ok()) else {
            continue; // headless-shaped caster with no rendered body
        };

        let assets = assets.get_or_insert_with(|| HealCastAssets::build(&mut meshes, &mut images));
        let has_launch_flash = match casting.ability {
            crate::states::play_match::abilities::AbilityType::FlashOfLight => {
                FLASH_OF_LIGHT_HAS_LAUNCH_FLASH
            }
            _ => true,
        };
        for side in [1.0_f32, -1.0] {
            spawn_hand_rig(
                &mut commands,
                &mut materials,
                assets,
                HandRig {
                    caster,
                    body,
                    kind,
                    side,
                    phase: HealCastPhase::Loop,
                    has_launch_flash,
                    drives_posture: true,
                },
            );
        }

        commands
            .entity(caster)
            .try_insert(HealCastPosture { pitch: 0.0 });
    }
}

/// The hand flash of a zero-length cast — the launch alone (the (3,13) cast
/// event), since an instant has no precast window. A zero-length cast is
/// inserted and completed inside one sim tick, so no render-frame system
/// ever sees its `CastingState`; this plays off the `Landed` marker instead,
/// which names the ability ([`LandedCast`]). Travel Form carries no cast
/// state at all, so its shift into the form plays it the same way.
///
/// Scoped by [`HealCastKind::for_ability`], not by the instant: only the
/// Druid's instants, Power Word: Shield, Ice Barrier and the Priest's Shadow
/// instants are routed, so every other instant (Frost Shock among them) still
/// draws no hands. Runs in `FixedUpdate` after
/// `CombatResolution` and before `consume_cast_ending_signals`, which
/// despawns the marker; a shift is read the tick it is queued, before
/// `process_travel_form` resolves it the next tick.
///
/// Power Word: Shield, Ice Barrier, Shadow Word: Pain, Vampiric Embrace and
/// Silence are applied inline by their class AIs with no cast state, so their
/// hands play off their [`InstantAbilityFired`] markers instead (read
/// before `consume_instant_ability_signals` despawns it). An ability that
/// announces itself through that marker never plays through a `Landed`
/// marker as well: the animation sandbox lands such an entry through a
/// zero-length cast AND spawns its marker, and the hands must flash once.
///
/// Divine Shield plays its hands elsewhere, in
/// [`spawn_divine_shield_cast_hands`], off its own pending component.
#[allow(clippy::too_many_arguments)]
pub fn spawn_instant_cast_hands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HealCastAssets>>,
    abilities: Res<crate::states::play_match::ability_config::AbilityDefinitions>,
    landed: Query<(&CastEnding, &LandedCast), Added<CastEnding>>,
    shifts: Query<&ShapeshiftPending, Added<ShapeshiftPending>>,
    fired: Query<&InstantAbilityFired, Added<InstantAbilityFired>>,
    casters: Query<&Children>,
    bodies: Query<(), With<VisualBody>>,
) {
    use crate::states::play_match::abilities::AbilityType;

    let instants = landed
        .iter()
        .filter(|(ending, cast)| {
            ending.kind == CastEndingKind::Landed
                && abilities.get_unchecked(&cast.ability).cast_time <= 0.0
                && !InstantAbilityFired::is_spawned_for(cast.ability)
                // Divine Shield's hands come off its own pending component
                // (`spawn_divine_shield_cast_hands`), never a `Landed` marker,
                // so they play once per cast.
                && cast.ability != AbilityType::DivineShield
                // The Paladin talent instants' hands come off their aura's
                // landing (`paladin_oneshots.rs`) for the same reason, and
                // Divine Protection's off its globe's onset
                // (`divine_protection.rs`).
                && !AuraLandingKind::flashes_hands_for(cast.ability)
                && !super::divine_protection::flashes_hands_at_onset(cast.ability)
                // Power Infusion's and Pain Suppression's off their states'
                // onsets (`discipline_states.rs`).
                && !super::discipline_states::discipline_hands_at_onset(cast.ability)
        })
        .map(|(ending, cast)| (ending.caster, cast.ability));
    let marked = fired.iter().map(|signal| (signal.caster, signal.ability));
    let shifts_in = shifts
        .iter()
        .filter(|pending| pending.shift == Shift::IntoTravelForm)
        .map(|pending| (pending.caster, AbilityType::TravelForm));

    for (caster, ability) in instants.chain(shifts_in).chain(marked) {
        flash_instant_hands(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut assets,
            &casters,
            &bodies,
            caster,
            ability,
        );
    }
}

/// FixedUpdate (graphical-only), BEFORE `CombatSystemPhase::ResourcesAndAuras`:
/// Divine Shield's hands (kit 417, the Holy launch), off its
/// `DivineShieldPending`.
///
/// Divine Shield has no cast state and no `InstantAbilityFired`: the Paladin AI
/// queues a `DivineShieldPending` (both paths, `try_divine_shield` and the
/// while-CC break) and `process_divine_shield` resolves it in Phase 1 of the
/// next sim tick, despawning it. The animation sandbox spawns the same pending
/// from `Update`. So the one place both are still alive and newly added is the
/// head of the NEXT fixed tick, ahead of Phase 1: an AI pending queued in
/// Phase 2 and a sandbox pending queued in `Update` are both read here once.
#[allow(clippy::too_many_arguments)]
pub fn spawn_divine_shield_cast_hands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HealCastAssets>>,
    bubbles: Query<&DivineShieldPending, Added<DivineShieldPending>>,
    casters: Query<&Children>,
    bodies: Query<(), With<VisualBody>>,
) {
    for pending in bubbles.iter() {
        flash_instant_hands(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut assets,
            &casters,
            &bodies,
            pending.caster,
            crate::states::play_match::abilities::AbilityType::DivineShield,
        );
    }
}

/// The launch alone on both of `caster`'s spell hands, in the family
/// [`HealCastKind::for_ability`] routes `ability` to (nothing if unrouted, or
/// for a caster with no rendered body). Also called by the Paladin talent
/// one-shots (`paladin_oneshots.rs`), whose hands ride their aura's landing.
#[allow(clippy::too_many_arguments)]
pub(super) fn flash_instant_hands(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    assets: &mut Option<HealCastAssets>,
    casters: &Query<&Children>,
    bodies: &Query<(), With<VisualBody>>,
    caster: Entity,
    ability: crate::states::play_match::abilities::AbilityType,
) {
    let Some(kind) = HealCastKind::for_ability(ability) else {
        return;
    };
    if kind == HealCastKind::Arcane && !MOONFIRE_CAST_HANDS {
        return;
    }
    let Some(body) = casters
        .get(caster)
        .ok()
        .and_then(|children| children.iter().find(|&child| bodies.get(child).is_ok()))
    else {
        return; // headless-shaped caster with no rendered body
    };
    let assets = assets.get_or_insert_with(|| HealCastAssets::build(meshes, images));
    for side in [1.0_f32, -1.0] {
        spawn_hand_rig(
            commands,
            materials,
            assets,
            HandRig {
                caster,
                body,
                kind,
                side,
                phase: HealCastPhase::Flare {
                    remaining: heal_cast_flare_secs(kind),
                },
                has_launch_flash: true,
                drives_posture: false,
            },
        );
    }
}

/// What a hand rig is: whose, which family, which hand, and where it starts.
struct HandRig {
    caster: Entity,
    body: Entity,
    kind: HealCastKind,
    side: f32,
    phase: HealCastPhase,
    has_launch_flash: bool,
    drives_posture: bool,
}

/// Spawn one hand rig at its spell-hand socket under the caster's
/// `VisualBody`, with the pieces its family shows from its starting phase:
/// a precast loop's glow and wisps; an instant Nature flash's glow (the jet
/// is emitted by the update pass); Moonfire's motes, stars and ring disc.
/// A Shadow rig has no pieces: its emitters, driven by the update pass, are
/// the whole model.
fn spawn_hand_rig(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &mut HealCastAssets,
    spec: HandRig,
) {
    let HandRig {
        caster,
        body,
        kind,
        side,
        phase,
        has_launch_flash,
        drives_posture,
    } = spec;
    let rig = commands
        .spawn((
            Transform::from_translation(spell_hand_local(side)),
            Visibility::default(),
            HealCastHand {
                caster,
                body,
                kind,
                side,
                age: 0.0,
                phase,
                has_launch_flash,
                drives_posture,
                leaf_carry: 0.0,
                burst_carry: [0.0; 2],
                emitted: 0,
                quad: assets.quad.clone(),
                ring_mesh: assets.ring.clone(),
                leaf_material: additive_material(
                    materials,
                    leaf_brown(),
                    1.2,
                    Some(assets.dot.clone()),
                ),
                spark_material: additive_material(
                    materials,
                    super::heal_impact::holy_gold(),
                    NATURE_LAUNCH_SPARK_EMISSIVE,
                    Some(assets.dot.clone()),
                ),
                ring_material: additive_material(
                    materials,
                    water_ring_blue().with_alpha(NATURE_LAUNCH_RING_ALPHA),
                    NATURE_LAUNCH_RING_EMISSIVE,
                    None,
                ),
            },
        ))
        .id();
    commands.entity(body).add_child(rig);

    if kind == HealCastKind::Shadow {
        let palettes = assets.shadow_palettes(materials);
        commands.entity(rig).insert(HealCastShadowEmitter {
            clock: 0.0,
            hand: None,
            emitted: [0; 3],
            palettes,
            quad: assets.quad.clone(),
        });
        return;
    }

    let instant = !matches!(phase, HealCastPhase::Loop);
    // Per-piece materials throughout: alphas (and the arcane colours) are
    // animated absolutely each frame, so nothing else may share a handle.
    let mut piece = |role: HealCastPieceRole,
                     base_alpha: f32,
                     mesh: Handle<Mesh>,
                     material: Handle<StandardMaterial>,
                     at: Vec3,
                     scale: Vec3| {
        commands
            .spawn((
                HealCastPiece { role, base_alpha },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(at).with_scale(scale),
                NotShadowCaster,
            ))
            .id()
    };
    let mut parts: Vec<Entity> = Vec::new();

    if kind == HealCastKind::Frost {
        let mut frost = |role: HealCastPieceRole, rgb: [f32; 3], texture: Handle<Image>| {
            let (at, _, _) = frost_piece_state(role, 0.0, side);
            let material = materials.add(StandardMaterial {
                base_color: Color::srgb(rgb[0], rgb[1], rgb[2]).with_alpha(0.0),
                base_color_texture: Some(texture),
                // The kit adds colour × texture × weight and nothing else.
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            piece(
                role,
                1.0,
                assets.quad.clone(),
                material,
                at,
                Vec3::splat(1e-3),
            )
        };
        parts.push(frost(
            HealCastPieceRole::FrostGlow,
            IB_HAND_GLOW_RGB,
            assets.frost_glow.clone(),
        ));
        parts.push(frost(
            HealCastPieceRole::FrostRing,
            IB_HAND_RING_RGB,
            assets.frost_ring.clone(),
        ));
        for index in 0..IB_HAND_SPARKS {
            parts.push(frost(
                HealCastPieceRole::FrostSpark { index },
                IB_HAND_SPARK_RGB,
                assets.frost_glow.clone(),
            ));
        }
        commands.entity(rig).add_children(&parts);
        return;
    }

    if kind == HealCastKind::Arcane {
        let mut arcane = |role: HealCastPieceRole, mesh: Handle<Mesh>, texture| {
            let (at, _, rgb, _) = arcane_piece_state(role, 0.0);
            let material = additive_material(
                materials,
                Color::srgb(rgb[0], rgb[1], rgb[2]).with_alpha(0.0),
                ARCANE_EMISSIVE,
                texture,
            );
            piece(role, 1.0, mesh, material, at, Vec3::splat(1e-3))
        };
        for index in 0..ARCANE_HAND_MOTES {
            parts.push(arcane(
                HealCastPieceRole::ArcaneMote { index },
                assets.quad.clone(),
                Some(assets.dot.clone()),
            ));
            parts.push(arcane(
                HealCastPieceRole::ArcaneStar { index },
                assets.quad.clone(),
                Some(assets.star.clone()),
            ));
        }
        parts.push(arcane(
            HealCastPieceRole::ArcaneRing,
            assets.arcane_ring.clone(),
            None,
        ));
        commands.entity(rig).add_children(&parts);
        return;
    }

    let holy = matches!(kind, HealCastKind::Holy | HealCastKind::HolyFlash);
    if !instant || holy || NATURE_INSTANT_HAND_GLOW {
        for (role, strength, alpha) in [
            (HealCastPieceRole::GlowOuter, 2.0, 0.55),
            (HealCastPieceRole::GlowCore, 2.8, 0.85),
        ] {
            let material = additive_material(
                materials,
                heal_cast_glow_color(kind),
                strength,
                Some(assets.dot.clone()),
            );
            parts.push(piece(
                role,
                alpha,
                assets.quad.clone(),
                material,
                Vec3::ZERO,
                Vec3::splat(1e-3),
            ));
        }
    }
    // Wisps belong to the precast loop (and to Holy's re-flare of it, and
    // its instant flash); a Nature launch is the jet, never the swirl.
    if !instant || holy {
        let wisp_width = heal_cast_wisp_width(kind);
        for index in 0..3_u32 {
            // Ribbon-blur streamers for Holy, star threads for Nature — the
            // source's genericglow2b vs star11b split.
            let texture = if holy {
                assets.dot.clone()
            } else {
                assets.star.clone()
            };
            let color = heal_cast_wisp_color(kind, index);
            let material = additive_material(materials, color, 2.2, Some(texture));
            parts.push(piece(
                HealCastPieceRole::Wisp { index },
                wisp_alpha(kind),
                assets.quad.clone(),
                material,
                heal_cast_wisp_head(index, 0.0),
                Vec3::new(wisp_width, 1e-3, 1.0),
            ));
        }
    }
    commands.entity(rig).add_children(&parts);
}

/// Per-frame animation: the glow pulse, the wisp swirl, the Nature leaf
/// shed, the launch flare/burst, and mote aging; for a Shadow rig, its
/// clock alone (its emitters are [`place_shadow_hands`]'). Time comes from
/// `Res<Time>` accumulation — never gated on per-frame sim movement.
pub fn update_heal_cast_glows(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(Entity, &mut HealCastHand, Option<&Children>)>,
    mut pieces: Query<
        (
            &HealCastPiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<HealCastLeaf>, Without<HealCastBurstMote>),
    >,
    mut leaves: Query<
        (Entity, &mut HealCastLeaf, &mut Transform),
        (Without<HealCastPiece>, Without<HealCastBurstMote>),
    >,
    mut bursts: Query<
        (Entity, &mut HealCastBurstMote, &mut Transform),
        (Without<HealCastPiece>, Without<HealCastLeaf>),
    >,
) {
    let dt = time.delta_secs();

    for (rig_entity, mut rig, children) in rigs.iter_mut() {
        rig.age += dt;
        let age = rig.age;

        // The flare clock, and the alpha/scale factors each phase applies.
        // `bloom` covers cast start; `flare_alpha`/`flare_scale` cover launch.
        // The instant Holy flash rides its own envelope, which brings it in.
        let bloom = if rig.kind == HealCastKind::HolyFlash {
            1.0
        } else {
            (age / HEAL_CAST_BLOOM_SECS).clamp(0.0, 1.0)
        };
        let (flare_alpha, flare_scale) = match rig.phase {
            HealCastPhase::Loop => (1.0, 1.0),
            HealCastPhase::Flare { remaining } => {
                let remaining = remaining - dt;
                if remaining <= 0.0 {
                    commands.entity(rig_entity).despawn();
                    continue;
                }
                rig.phase = HealCastPhase::Flare { remaining };
                let total = heal_cast_flare_secs(rig.kind);
                if rig.kind == HealCastKind::HolyFlash {
                    // The instant flash: the bench's envelope, at full size.
                    (holy_instant_flash_alpha(total - remaining), 1.0)
                } else {
                    let t = 1.0 - remaining / total;
                    // The re-flare: brighter than the loop ever was, swelling as
                    // it dies (Holy). Nature's loop pieces are already gone.
                    (
                        (1.0 + HOLY_LAUNCH_FLARE_INTENSITY) * (1.0 - t),
                        1.0 + 0.5 * HOLY_LAUNCH_FLARE_INTENSITY * t,
                    )
                }
            }
        };

        // The Shadow hands are their three emitters, which emit at the hand
        // as the body is drawn (`place_shadow_hands`): the rig keeps only
        // the clock.
        if rig.kind == HealCastKind::Shadow {
            continue;
        }

        // Nature emission: leaves through the loop, the jet through the flare.
        if rig.kind == HealCastKind::Nature {
            match rig.phase {
                HealCastPhase::Loop => {
                    rig.leaf_carry += NATURE_CAST_LEAF_RATE * dt;
                    while rig.leaf_carry >= 1.0 {
                        rig.leaf_carry -= 1.0;
                        let i = rig.emitted;
                        rig.emitted = rig.emitted.wrapping_add(1);
                        let seed = rig_entity.index().wrapping_add(i.wrapping_mul(0x9E37_79B9));
                        // Omni drift: a full-sphere direction, position-hashed.
                        let a = heal_jitter(seed) * TAU;
                        let z = heal_jitter(seed ^ 0x1EAF) * 2.0 - 1.0;
                        let r = (1.0 - z * z).max(0.0).sqrt();
                        let dir = Vec3::new(a.cos() * r, z, a.sin() * r);
                        let jitter = Vec3::new(
                            (heal_jitter(seed ^ 0x51ED) - 0.5) * 0.12,
                            (heal_jitter(seed ^ 0x2DDF) - 0.5) * 0.12,
                            0.0,
                        );
                        let leaf = commands
                            .spawn((
                                HealCastLeaf {
                                    velocity: dir * NATURE_CAST_LEAF_SPEED,
                                    age: 0.0,
                                    life: NATURE_CAST_LEAF_LIFE,
                                },
                                Mesh3d(rig.quad.clone()),
                                MeshMaterial3d(rig.leaf_material.clone()),
                                Transform::from_translation(jitter)
                                    .with_scale(Vec3::splat(NATURE_LEAF_SIZE)),
                                NotShadowCaster,
                            ))
                            .id();
                        commands.entity(rig_entity).add_child(leaf);
                    }
                }
                HealCastPhase::Flare { .. } => {
                    let jet = nature_launch_jet_dir();
                    for (ei, (rate, speed, kind)) in [
                        (
                            NATURE_LAUNCH_RING_RATE,
                            NATURE_LAUNCH_RING_SPEED,
                            HealCastBurstKind::WaterRing,
                        ),
                        (
                            NATURE_LAUNCH_SPARK_RATE,
                            NATURE_LAUNCH_SPARK_SPEED,
                            HealCastBurstKind::GoldSpark,
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        rig.burst_carry[ei] += rate * dt;
                        while rig.burst_carry[ei] >= 1.0 {
                            rig.burst_carry[ei] -= 1.0;
                            let i = rig.emitted;
                            rig.emitted = rig.emitted.wrapping_add(1);
                            let seed = rig_entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
                            // A tight directional jet: tilt the axis by a
                            // hashed angle within the ~2° spread cone.
                            let tilt = heal_jitter(seed) * NATURE_LAUNCH_SPREAD;
                            let roll = heal_jitter(seed ^ 0x27D4) * TAU;
                            let dir = Quat::from_axis_angle(jet, roll)
                                * Quat::from_axis_angle(jet.cross(Vec3::Y).normalize(), tilt)
                                * jet;
                            let (mesh, material, size) = match kind {
                                HealCastBurstKind::WaterRing => (
                                    rig.ring_mesh.clone(),
                                    rig.ring_material.clone(),
                                    NATURE_LAUNCH_RING_SIZE,
                                ),
                                HealCastBurstKind::GoldSpark => (
                                    rig.quad.clone(),
                                    rig.spark_material.clone(),
                                    NATURE_LAUNCH_SPARK_SIZE,
                                ),
                            };
                            let mote = commands
                                .spawn((
                                    HealCastBurstMote {
                                        kind,
                                        velocity: dir * speed * NATURE_LAUNCH_BURST_SCALE,
                                        age: 0.0,
                                        life: NATURE_LAUNCH_BURST_SECS,
                                    },
                                    Mesh3d(mesh),
                                    MeshMaterial3d(material),
                                    Transform::from_scale(Vec3::splat(
                                        size * NATURE_LAUNCH_BURST_SCALE,
                                    )),
                                    NotShadowCaster,
                                ))
                                .id();
                            commands.entity(rig_entity).add_child(mote);
                        }
                    }
                }
            }
        }

        let Some(children) = children else {
            continue;
        };
        let glow_sizes = heal_cast_glow_sizes(rig.kind);
        let wisp_width = heal_cast_wisp_width(rig.kind);
        // The model's own 1 s loop, as a gentle breathing pulse.
        let pulse = 1.0 + 0.06 * (TAU * HOLY_CAST_ORBIT_HZ * age).sin();

        for child in children.iter() {
            if let Ok((piece, mut part, material)) = pieces.get_mut(child) {
                if matches!(
                    piece.role,
                    HealCastPieceRole::ArcaneMote { .. }
                        | HealCastPieceRole::ArcaneStar { .. }
                        | HealCastPieceRole::ArcaneRing
                ) {
                    // Moonfire's kit runs on its own tracks, not the
                    // glow-ball bloom and re-flare.
                    let (at, size, rgb, alpha) = arcane_piece_state(piece.role, age);
                    part.translation = at;
                    let radius_scale = match piece.role {
                        // The ring mesh is unit-RADIUS; the quads unit-width.
                        HealCastPieceRole::ArcaneRing => 0.5,
                        _ => 1.0,
                    };
                    let shown = alpha > 0.0;
                    part.scale = Vec3::splat(if shown {
                        (size * radius_scale).max(1e-3)
                    } else {
                        1e-3
                    });
                    if let Some(material) = materials.get_mut(&material.0) {
                        let color = Color::srgb(rgb[0], rgb[1], rgb[2]);
                        material.emissive = emissive_of(color, ARCANE_EMISSIVE);
                        material.base_color = color.with_alpha(alpha * piece.base_alpha);
                    }
                    continue;
                }
                if matches!(
                    piece.role,
                    HealCastPieceRole::FrostGlow
                        | HealCastPieceRole::FrostRing
                        | HealCastPieceRole::FrostSpark { .. }
                ) {
                    // Ice Barrier's kit runs on its own envelope and pulse.
                    let (at, size, alpha) = frost_piece_state(piece.role, age, rig.side);
                    part.translation = at;
                    part.scale = Vec3::splat(if alpha > 0.0 { size.max(1e-3) } else { 1e-3 });
                    if let Some(material) = materials.get_mut(&material.0) {
                        material.base_color.set_alpha(alpha * piece.base_alpha);
                    }
                    continue;
                }
                let alpha = piece.base_alpha * bloom * flare_alpha;
                match piece.role {
                    HealCastPieceRole::GlowOuter => {
                        part.scale = Vec3::splat((glow_sizes[0] * pulse * flare_scale).max(1e-3));
                    }
                    HealCastPieceRole::GlowCore => {
                        part.scale = Vec3::splat((glow_sizes[1] * pulse * flare_scale).max(1e-3));
                    }
                    HealCastPieceRole::Wisp { index } => {
                        // The head orbits; the trail streams back along the
                        // tangent, one edge-lifetime long, centred behind it.
                        let head = heal_cast_wisp_head(index, age);
                        let tangent = heal_cast_wisp_tangent(index, age);
                        let length = heal_cast_wisp_length(index) * bloom;
                        part.translation = head - tangent * (length * 0.5);
                        part.scale = Vec3::new(wisp_width * flare_scale, length.max(1e-3), 1.0);
                    }
                    HealCastPieceRole::ArcaneMote { .. }
                    | HealCastPieceRole::ArcaneStar { .. }
                    | HealCastPieceRole::ArcaneRing
                    | HealCastPieceRole::FrostGlow
                    | HealCastPieceRole::FrostRing
                    | HealCastPieceRole::FrostSpark { .. } => {}
                }
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(alpha);
                }
            }

            if let Ok((leaf_entity, mut leaf, mut part)) = leaves.get_mut(child) {
                leaf.age += dt;
                if leaf.age >= leaf.life {
                    commands.entity(leaf_entity).despawn();
                    continue;
                }
                part.translation += leaf.velocity * dt;
                let k = 1.0 - leaf.age / leaf.life;
                part.scale = Vec3::splat((NATURE_LEAF_SIZE * k.powf(0.5)).max(1e-4));
            }

            if let Ok((mote_entity, mut mote, mut part)) = bursts.get_mut(child) {
                mote.age += dt;
                if mote.age >= mote.life {
                    commands.entity(mote_entity).despawn();
                    continue;
                }
                part.translation += mote.velocity * dt;
                let k = 1.0 - mote.age / mote.life;
                match mote.kind {
                    HealCastBurstKind::WaterRing => {
                        // The shockwave read: the ring EXPANDS as it fades.
                        let swell = 1.0 + NATURE_LAUNCH_RING_SWELL * (1.0 - k);
                        part.scale = Vec3::splat(
                            (NATURE_LAUNCH_RING_SIZE
                                * NATURE_LAUNCH_BURST_SCALE
                                * swell
                                * k.powf(0.35))
                            .max(1e-4),
                        );
                    }
                    HealCastBurstKind::GoldSpark => {
                        part.scale = Vec3::splat(
                            (NATURE_LAUNCH_SPARK_SIZE * NATURE_LAUNCH_BURST_SCALE * k.powf(0.6))
                                .max(1e-4),
                        );
                    }
                }
            }
        }
    }
}

/// PostUpdate, a [`BodyPlacement`] system: one frame of every Shadow hand's
/// emitters. Each particle lives in the world, as the client's do (no
/// model-space flag): it is born at the hand as the body is drawn this frame
/// ([`BodyHold`], after every writer of the body), rises straight up from
/// there, and is left behind as the hand moves on. A particle born part-way
/// through the frame is born where the hand was then (the hand's path is
/// taken as straight across the frame) and that much aged, and its pose is
/// its birth point plus its velocity times its age, so what is drawn does not
/// depend on the frame rate. Particles go with their rig.
#[allow(clippy::type_complexity)]
pub fn place_shadow_hands(
    mut commands: Commands,
    time: Res<Time>,
    hold: BodyHold,
    mut rigs: Query<(Entity, &HealCastHand, &mut HealCastShadowEmitter)>,
    mut motes: Query<
        (
            Entity,
            &mut HealCastShadowMote,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        HoldDisjoint,
    >,
) {
    let dt = time.delta_secs();
    let facing = hold.view().map(|view| view.rotation);

    for (entity, mut mote, mut part, mut material) in motes.iter_mut() {
        let Ok((_, _, emitter)) = rigs.get(mote.rig) else {
            commands.entity(entity).despawn();
            continue;
        };
        mote.age += dt;
        if mote.age >= mote.life {
            commands.entity(entity).despawn();
            continue;
        }
        let e = &SHADOW_HAND_EMITTERS[mote.emitter];
        part.translation = mote.origin + mote.velocity * mote.age;
        part.scale = Vec3::splat(shadow_mote_diameter(e, mote.age));
        if let Some(facing) = facing {
            // Square to the camera, unrolled: the model gives its emitters
            // no spin and no tile rotation.
            part.rotation = facing;
        }
        let step = shadow_mote_step(e, mote.age);
        if step != mote.step {
            mote.step = step;
            material.0 = emitter.palettes[mote.emitter][step].clone();
        }
    }

    for (rig_entity, rig, mut emitter) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.caster) else {
            continue;
        };
        let hand = body
            .unit
            .transform_point(body.local.transform_point(spell_hand_local(rig.side)));
        let before = emitter.hand.unwrap_or(hand);
        emitter.hand = Some(hand);

        // The loop emits all the while; the cast kit for its one sequence,
        // counted from the frame the flare began.
        let emit_dt = match rig.phase {
            HealCastPhase::Loop => dt,
            HealCastPhase::Flare { remaining } => {
                let elapsed = heal_cast_flare_secs(HealCastKind::Shadow) - remaining - dt;
                (SHADOW_HAND_SECS - elapsed).clamp(0.0, dt)
            }
        };
        let start = emitter.clock;
        emitter.clock += emit_dt;
        for (index, e) in SHADOW_HAND_EMITTERS.iter().enumerate() {
            let owed = shadow_emitted_by(e, emitter.clock);
            while emitter.emitted[index] < owed {
                emitter.emitted[index] += 1;
                let k = emitter.emitted[index];
                // When in this frame it was born, and so how old it is now.
                let born = (k as f32 / e.rate - start).clamp(0.0, dt);
                let age = dt - born;
                if age >= e.life {
                    continue;
                }
                let origin = if dt > 0.0 {
                    before.lerp(hand, born / dt)
                } else {
                    hand
                };
                let seed = rig_entity.index().wrapping_mul(0x9E37_79B9)
                    ^ (index as u32 + 1).wrapping_mul(0x85EB_CA6B)
                    ^ k.wrapping_mul(0xC2B2_AE35);
                let velocity = shadow_mote_velocity(e, seed);
                let step = shadow_mote_step(e, age);
                commands.spawn((
                    HealCastShadowMote {
                        rig: rig_entity,
                        emitter: index,
                        origin,
                        velocity,
                        age,
                        life: e.life,
                        step,
                    },
                    Mesh3d(emitter.quad.clone()),
                    MeshMaterial3d(emitter.palettes[index][step].clone()),
                    Transform::from_translation(origin + velocity * age)
                        .with_rotation(facing.unwrap_or_default())
                        .with_scale(Vec3::splat(shadow_mote_diameter(e, age))),
                    NotShadowCaster,
                    PlayMatchEntity,
                ));
            }
        }
    }
}

/// Consume `CastEnding` markers for heal casts, transitioning the hand rigs.
/// Landed -> the launch (Holy re-flares its own glow; Nature swaps the loop
/// pieces for the burst jet). Fizzled or Interrupted -> everything stops
/// DEAD, immediately — the source has no interrupt visual and fires none.
///
/// Runs in `FixedUpdate` after `CombatSystemPhase::CombatResolution` and
/// BEFORE `consume_cast_ending_signals`: that consumer owns despawning the
/// marker (this one only reads it), and FixedUpdate can tick multiple times
/// per rendered frame, so an Update-schedule consumer could miss a marker.
pub fn consume_heal_cast_endings(
    mut commands: Commands,
    signals: Query<&CastEnding>,
    mut rigs: Query<(Entity, &mut HealCastHand, Option<&Children>)>,
    loop_pieces: Query<(), Or<(With<HealCastPiece>, With<HealCastLeaf>)>>,
) {
    for ending in signals.iter() {
        for (rig_entity, mut rig, children) in rigs.iter_mut() {
            if rig.caster != ending.caster || !matches!(rig.phase, HealCastPhase::Loop) {
                continue;
            }
            match ending.kind {
                CastEndingKind::Landed if rig.has_launch_flash => {
                    rig.phase = HealCastPhase::Flare {
                        remaining: heal_cast_flare_secs(rig.kind),
                    };
                    if rig.kind == HealCastKind::Nature {
                        // The loop kit dies; the dedicated launch model takes
                        // over. Glows, wisps and airborne leaves all go.
                        if let Some(children) = children {
                            for child in children.iter() {
                                if loop_pieces.get(child).is_ok() {
                                    commands.entity(child).despawn();
                                }
                            }
                        }
                    }
                }
                _ => {
                    // Fizzle, interrupt, or a flash-less Flash of Light
                    // landing: the hand glow just vanishes, mid-swirl.
                    commands.entity(rig_entity).despawn();
                }
            }
        }
    }
}

/// The capsule dialect of ReadySpellOmni -> SpellCastOmni. Derives the torso
/// pitch from the caster's live hand rigs (main side owns it, the body-lean
/// rule) and writes the `VisualBody` rotation ABSOLUTELY — no compounding.
/// Runs after `animate_body_lean`, so while a heal cast is live the cast
/// posture owns the torso; the frame no rig remains it snaps the body back
/// to identity and removes itself (an interrupted heal's body loop stops
/// dead — faithful). Cedes entirely to the death fall.
pub fn update_heal_cast_posture(
    mut commands: Commands,
    time: Res<Time>,
    rigs: Query<&HealCastHand>,
    mut casters: Query<
        (
            Entity,
            &mut HealCastPosture,
            &Children,
            Option<&DeathAnimation>,
        ),
        With<Combatant>,
    >,
    mut bodies: Query<&mut Transform, With<VisualBody>>,
) {
    let dt = time.delta_secs();

    for (caster, mut posture, children, dying) in casters.iter_mut() {
        // Main side owns the posture (the body-lean rule). Prefer a LOOP rig
        // over a still-flaring one — a back-to-back heal's new loop starts
        // while the previous launch's flare plays out, and the loop's ready
        // lean is the pose the new cast wants.
        let rig = rigs
            .iter()
            .filter(|rig| rig.caster == caster && rig.side > 0.0 && rig.drives_posture)
            .max_by_key(|rig| matches!(rig.phase, HealCastPhase::Loop) as u8);

        if dying.is_some() {
            // The death fall owns rotation from here; never write it again.
            commands.entity(caster).remove::<HealCastPosture>();
            continue;
        }

        let Some(rig) = rig else {
            // Cast over (landed flare spent, interrupt, fizzle): stop dead.
            for child in children.iter() {
                if let Ok(mut body) = bodies.get_mut(child) {
                    body.rotation = Quat::IDENTITY;
                }
            }
            commands.entity(caster).remove::<HealCastPosture>();
            continue;
        };

        match rig.phase {
            HealCastPhase::Loop => {
                let rate = HEAL_CAST_READY_PITCH.abs() / HEAL_CAST_POSTURE_EASE_SECS;
                posture.pitch = (posture.pitch - rate * dt).max(HEAL_CAST_READY_PITCH);
            }
            HealCastPhase::Flare { remaining } => {
                let elapsed = heal_cast_flare_secs(rig.kind) - remaining;
                posture.pitch = heal_cast_release_pitch(elapsed / heal_cast_release_secs(rig.kind));
            }
        }

        for child in children.iter() {
            if let Ok(mut body) = bodies.get_mut(child) {
                body.rotation = Quat::from_rotation_x(posture.pitch);
            }
        }
    }
}

/// Turn the flat pieces of every hand rig to face the camera.
///
/// The rigs are children of the `VisualBody`, so a piece's rendered rotation
/// composes through the caster's facing and the body's posture pitch; the
/// billboard therefore counter-rotates by that full parent world rotation
/// (`caster * body` — the rig's own local rotation is always identity).
/// Wisps keep a roll about the view axis aligning their long axis with the
/// orbit tangent's projection, so the trail visibly streams behind its head.
pub fn billboard_heal_cast_glows(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<HealCastPiece>,
            Without<HealCastLeaf>,
            Without<HealCastBurstMote>,
        ),
    >,
    rigs: Query<(&HealCastHand, &Children)>,
    casters: Query<
        &Transform,
        (
            With<Combatant>,
            Without<Camera3d>,
            Without<HealCastPiece>,
            Without<HealCastLeaf>,
            Without<HealCastBurstMote>,
        ),
    >,
    bodies: Query<
        &Transform,
        (
            With<VisualBody>,
            Without<Camera3d>,
            Without<HealCastPiece>,
            Without<HealCastLeaf>,
            Without<HealCastBurstMote>,
        ),
    >,
    mut pieces: Query<
        (&HealCastPiece, &mut Transform),
        (Without<HealCastLeaf>, Without<HealCastBurstMote>),
    >,
    mut leaves: Query<
        &mut Transform,
        (
            With<HealCastLeaf>,
            Without<HealCastPiece>,
            Without<HealCastBurstMote>,
        ),
    >,
    mut bursts: Query<
        &mut Transform,
        (
            With<HealCastBurstMote>,
            Without<HealCastPiece>,
            Without<HealCastLeaf>,
        ),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let caster_rot = casters
            .get(rig.caster)
            .map(|t| t.rotation)
            .unwrap_or(Quat::IDENTITY);
        let body_rot = bodies
            .get(rig.body)
            .map(|t| t.rotation)
            .unwrap_or(Quat::IDENTITY);
        let parent_world = caster_rot * body_rot;
        let facing = parent_world.inverse() * cam.rotation;

        for child in children.iter() {
            if let Ok((piece, mut part)) = pieces.get_mut(child) {
                match piece.role {
                    HealCastPieceRole::Wisp { index } => {
                        let tangent_world = parent_world * heal_cast_wisp_tangent(index, rig.age);
                        let t_cam = cam.rotation.inverse() * tangent_world;
                        // Roll the quad's long (+Y) axis onto the tangent's
                        // projection in the billboard plane.
                        let roll = (-t_cam.x).atan2(t_cam.y);
                        part.rotation = facing * Quat::from_rotation_z(roll);
                    }
                    HealCastPieceRole::ArcaneStar { .. } => {
                        // The twinkle spins as it rises.
                        part.rotation =
                            facing * Quat::from_rotation_z(rig.age / ARCANE_STAR_SPIN_SECS);
                    }
                    _ => {
                        part.rotation = facing;
                    }
                }
            }
            if let Ok(mut part) = leaves.get_mut(child) {
                part.rotation = facing;
            }
            if let Ok(mut part) = bursts.get_mut(child) {
                part.rotation = facing;
            }
        }
    }
}

/// Despawn hand rigs whose cast silently went away with no `CastEnding`
/// marker — caster death, match end, entity teardown — and rigs whose
/// `CastingState` sits in its interrupted-display window (the state lingers
/// for HUD feedback, but the hand glow stops DEAD at the interrupt, not half
/// a second later). Flare-phase rigs own their remaining lifetime; the
/// update system retires them.
///
/// A Shadow rig plays on for up to [`heal_cast_flare_secs`] after its cast,
/// long enough to outlast its caster: it goes the frame its caster is dead
/// or polymorphed (the sheep has no hands), in any phase.
pub fn cleanup_heal_cast_glows(
    mut commands: Commands,
    rigs: Query<(Entity, &HealCastHand)>,
    cast_states: Query<&CastingState>,
    casters: Query<(&Combatant, Has<PolymorphedVisual>)>,
) {
    for (rig_entity, rig) in rigs.iter() {
        if rig.kind == HealCastKind::Shadow {
            let handless = match casters.get(rig.caster) {
                Ok((combatant, sheep)) => !combatant.is_alive() || sheep,
                Err(_) => true,
            };
            if handless {
                commands.entity(rig_entity).despawn();
                continue;
            }
        }
        if !matches!(rig.phase, HealCastPhase::Loop) {
            continue;
        }
        match cast_states.get(rig.caster) {
            Ok(casting) if !casting.interrupted => {}
            _ => {
                commands.entity(rig_entity).despawn();
            }
        }
    }
}
