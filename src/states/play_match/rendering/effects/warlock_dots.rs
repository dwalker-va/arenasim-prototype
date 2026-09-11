use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, TAU};

use super::heal_impact::COMBATANT_BODY_RADIUS;
use super::school_impact::{IMPACT_HEAD_Y, IMPACT_PET_BODY_Y, IMPACT_PET_STATURE};
use super::spell_bolts::{soft_dot_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Warlock DoT + curse aura visuals
// ==============================================================================
//
// Corruption / Unstable Affliction (states) and the three curses — Agony,
// Weakness, Tongues (one-shot apply apparitions, table-driven off
// `CURSE_APPARITIONS`).
//
// The measured Classic client data
// (`docs/design/2026-09-06-warlock-dot-client-data.md`, build 1.15.9.69547)
// gives the three DoTs three very different identities, and this module
// reproduces them with the codebase's primitive-mesh vocabulary:
//
// - **Corruption apply** (kit 117, Chest): an expanding shadow ring flashing
//   green→violet→near-black (~0.75 s) under a burst of green/violet spark
//   motes. Shared VERBATIM with Unstable Affliction's apply — the client
//   gives UA no apply identity of its own (same SpellVisual 381).
// - **Corruption state** (kit 535, head/torso): a DARKENING alpha-blend
//   shroud that re-blooms every `PULSE_PERIOD`, wrapped in slowly swelling
//   murk-green wisps and a constant fizz of small green motes. Ends at aura
//   expire/dispel with no flourish, exactly as the client's (7,8) state pair.
// - **The curses** (kits 884 / 719 / 503): apply-only apparitions, one
//   `CurseApparitionSpec` each in `CURSE_APPARITIONS`. **Curse of Agony**
//   (Head) is a red-shell, yellow-core skull; **Curse of Weakness** (Head)
//   is a violet-shell, GREEN-core skull WITH A BONE beside it, on CoA's
//   identical envelope and 433/267 ms flicker; **Curse of Tongues** (CHEST)
//   is a magenta-violet rune circle — two flat discs plus five upright glyph
//   tablets on a ring turning once every 2 s. All three show NOTHING for the
//   remaining curse duration (`CURSE_SUSTAIN_WHISPER` = false, era-faithful
//   and deliberate — the 1.15 client sells "cursed" entirely through the
//   apply flourish and the debuff icon; the one measured exception, CoT's
//   kit-502 state, is documented in the AS-19 doc and deliberately unbuilt).
//
//   The client differentiates the three by PALETTE and ATTACH + SILHOUETTE,
//   never by timing: red+yellow skull / violet+green skull-and-bone at the
//   head / magenta-violet rune circle at the chest. That is the axis this
//   module spends too — see `distinct_curse_apparitions_read_apart` in
//   `tests/warlock_dot_visual_probes.rs`, which pins it.
// - **Unstable Affliction state** — AUTHORED (the client renders a UA victim
//   pixel-identical to a Corruption victim): an additive violet torso glow on
//   its own `UA_PULSE_PERIOD`, plus a periodic crackle discharge of jagged
//   violet bolts and a bright pop.
//
// Stacking design (why three DoTs on one victim stay readable): Corruption
// DARKENS (alpha-blend), UA GLOWS (additive violet), CoA is a silhouette at a
// moment (the skull); the two sustained pulses run at deliberately different
// periods (3.00 s vs 1.20 s), and UA's crackle pop is sized to read OUTSIDE
// Corruption's shroud silhouette. The UA glow and pop quads are additionally
// held `UA_CAMERA_LIFT` toward the camera by the billboard pass, so their
// bright centres beat the body's (and the stacked shroud's) depth test
// instead of reading as a dim annulus.
//
// ## The AlphaMode::Blend exception (deliberate, justified)
//
// The Corruption shroud shell is the codebase's ONE deliberate exception to
// the `AlphaMode::Add` visual-effects convention: the client's kit-535 state
// mesh is an alpha-DARKENING shroud (`black_glow2`, the only non-additive
// material in the entire AS-9/AS-11/AS-15 research series), and additive
// blending can only add light — it can never dim the victim. Corruption's
// signature IS the dimming, so the shell uses `AlphaMode::Blend`. The
// Z-fighting hazard the Add convention exists to prevent is handled by
// LAYERING, not luck: the shell is a capsule held `SHROUD_STANDOFF` proud of
// the body capsule's surface, so no transparent face is ever coplanar with
// the opaque body, and it carries `NotShadowCaster` like every other effect.
// Nothing else here blends: wisps, fizz, glows, sparks and bolts are all
// additive (the skull's eye sockets are small `Blend` spheres for the same
// cannot-darken-additively reason, offset well inside the cranium shell).
//
// House rules honored: spawn/animate/cleanup system shape, `Res<Time>`,
// `try_insert` where components land on existing entities, registered in
// `states/mod.rs` ONLY (graphical-only — headless never runs any of this and
// stays byte-identical by construction), no `game_rng` (all variation is
// hash-seeded off entity index + emission counters).

// --- Corruption / UA apply (kit 117) -----------------------------------------

/// Blessed master scale on the shared apply burst.
pub const APPLY_BURST_SCALE: f32 = 1.0;
/// The shadow ring's expansion window (client: ring life 0.75 s).
pub const APPLY_RING_SECS: f32 = 0.75;
/// The whole burst is over in ~1.75 s (ring + the last sparks' lives).
pub const APPLY_BURST_LIFE: f32 = 1.75;
/// Spark emission rate over the ring window (client flare emitter: 100/s).
const APPLY_SPARK_RATE: f32 = 100.0;
/// Spark outward speed (client: 2.22 u/s, all directions).
const APPLY_SPARK_SPEED: f32 = 2.22;
/// Spark life (client: 1.0 s).
const APPLY_SPARK_LIFE: f32 = 1.0;
/// The ring's peak scale (client scale track 0.03→0.83→1.25).
const APPLY_RING_SCALE: [f32; 3] = [0.03, 0.83, 1.25];
/// The ring's alpha track (client 0→0.59→0).
const APPLY_RING_ALPHA: [f32; 3] = [0.0, 0.59, 0.0];
/// The apply palette: sickly green flashing to violet, sinking near-black
/// (client color tracks, RGB/255).
const APPLY_COLOR: [Color; 3] = [
    Color::srgb(0.20, 0.45, 0.0),
    Color::srgb(0.25, 0.09, 0.76),
    Color::srgb(0.13, 0.05, 0.41),
];
/// Height of the chest anchor above a combatant's transform (attach 34).
const CHEST_Y: f32 = 0.35;

// --- Corruption state (kit 535) ----------------------------------------------

/// Peak darkening of the alpha-blend shroud (blessed).
pub const SHROUD_DARKNESS: f32 = 0.55;
/// The shroud re-bloom cycle (blessed; client mesh tint: alpha 1→0 over the
/// 3000 ms loop). Deliberately different from `UA_PULSE_PERIOD`.
pub const PULSE_PERIOD: f32 = 3.00;
/// Blessed master scale on the state's mote emission rates.
pub const MOTE_RATE_SCALE: f32 = 1.0;
/// Radius of the shroud capsule shell. Must clear the combatant body capsule
/// (`COMBATANT_BODY_RADIUS`, 0.5) with margin or the opaque body swallows it
/// — the AS-10 buried-inside-the-capsule lesson.
pub const SHROUD_RADIUS: f32 = COMBATANT_BODY_RADIUS + SHROUD_STANDOFF;
/// How far the shroud stands proud of the body capsule's surface. Also the
/// anti-Z-fight layering margin for the Blend exception (see module docs).
pub const SHROUD_STANDOFF: f32 = 0.18;
/// Cylinder length of the shroud shell — head/torso, not the whole body.
const SHROUD_LENGTH: f32 = 1.0;
/// Height of the shroud's centre above the victim's transform (the client
/// attaches the state at the HEAD; a head-centred capsule of this length
/// covers head + torso).
const SHROUD_CENTER_Y: f32 = 0.45;
/// The shroud's near-black-green tint (murk, not signal green).
const SHROUD_COLOR: Color = Color::srgb(0.03, 0.055, 0.02);
/// Wisp emission rate (client clouds8x8fade emitter: 20/s).
const WISP_RATE: f32 = 20.0;
/// Wisp life (client: 2.5 s).
const WISP_LIFE: f32 = 2.5;
/// Wisp drift speed (client: 0.28 u/s, all directions).
const WISP_DRIFT_SPEED: f32 = 0.28;
/// Wisp growth over life. USER-TUNED (2026-09-06): the client track is
/// 0.22→0.31→0.69, but at those quad sizes the wisps read as chunky blocks
/// around the torso — shrunk to ~⅔ and given the soft radial sprite (an
/// untextured additive quad has a hard edge) so they read as soft swelling
/// murk instead.
const WISP_SCALE: [f32; 3] = [0.15, 0.21, 0.46];
/// Wisp alpha envelope (client 0.39→1.0→0).
const WISP_ALPHA: [f32; 3] = [0.39, 1.0, 0.0];
/// Wisp color ramp: near-black green rising to sickly yellow-green (client
/// (18,55,0)→(106,146,0)→(163,196,64), RGB/255).
const WISP_COLOR: [Color; 3] = [
    Color::srgb(0.07, 0.22, 0.0),
    Color::srgb(0.42, 0.57, 0.0),
    Color::srgb(0.64, 0.77, 0.25),
];
/// Fizz mote emission rate (client flare emitter: 150/s).
const FIZZ_RATE: f32 = 150.0;
/// Fizz mote life (client: 0.6 s).
const FIZZ_LIFE: f32 = 0.6;
/// Fizz rise speed (client: 3.33 u/s).
const FIZZ_RISE_SPEED: f32 = 3.33;
/// Fizz green (client mid-ramp (99,136,20)/255, brightened for additive).
const FIZZ_COLOR: Color = Color::srgb(0.42, 0.56, 0.10);

// --- curse apparitions (kits 884 / 719 / 503) --------------------------------
//
// All three of ArenaSim's Warlock curses get a one-shot on-victim apparition
// at apply, driven off the `CURSE_APPARITIONS` table below rather than three
// copies of one spawner. The per-curse constants are measured client data:
// Curse of Agony from AS-15 (`2026-09-06-warlock-dot-client-data.md`), Curse
// of Weakness and Curse of Tongues from AS-19
// (`2026-09-10-warlock-curse-client-data.md`).
//
// The client differentiates the three by PALETTE and ATTACH + SILHOUETTE, not
// by timing — CoA and CoW share their envelope and both flicker cycles to the
// millisecond, and the thing that tells them apart is red-shell/yellow-core
// skull vs violet-shell/green-core skull-and-bone. CoT is the outlier in
// shape: a magenta-violet rune circle at the CHEST, not a head apparition.

/// Era-faithful and DELIBERATE: neither Curse of Agony nor Curse of Weakness
/// has a `(7,8)` aura-state row in the client — a cursed victim shows NOTHING
/// after the apply apparition, for the whole remaining curse. (Curse of
/// Tongues alone DOES have one, kit 502; its constants are measured in the
/// AS-19 doc and implementing it is a scoped decision, not a bug fix.) Kept
/// false on purpose.
pub const CURSE_SUSTAIN_WHISPER: bool = false;
/// Blessed master scale on every apparition's emission (sparks, falling glow
/// motes and blooms alike).
pub const SPARK_RATE_SCALE: f32 = 1.0;
/// Blessed master scale of the skull apparitions (client bbox ≈ 0.85 u tall
/// for the skull proper, in both CoA's and CoW's models).
pub const CURSE_SKULL_SIZE: f32 = 0.85;
/// Height of a cranium's centre above the head anchor, before
/// `CURSE_SKULL_SIZE` scaling (client: skull mesh ~0.8 u above attach 20).
const CURSE_SKULL_LIFT: f32 = 0.75;

/// Curse of Weakness's bone beside the skull. The client's `bone_purple`
/// submesh centres at x = −0.14 with the skull at +0.13 and a mesh radius of
/// ~0.21 u — a separation of ~1.3 skull-radii. `BONE_OFFSET` is that ratio
/// carried onto OUR cranium radius (0.30) so the bone abuts the skull the
/// same way instead of being swallowed by a proportionally fatter sphere.
const BONE_OFFSET: f32 = 0.39;
const BONE_RADIUS: f32 = 0.055;
const BONE_LENGTH: f32 = 0.42;
/// Tilt off vertical, radians — a bone leaning against the skull.
const BONE_TILT: f32 = 0.6;

/// Radius of Curse of Tongues' flat rune discs (client mesh: 1.77 u across).
const RUNE_DISC_RADIUS: f32 = 0.885;
/// The two discs' heights above the chest anchor (client z 0.225 / 0.044).
const RUNE_DISC_HEIGHTS: [f32; 2] = [0.225, 0.044];
/// Upright glyph tablets on the ring, and the ring they stand on (client:
/// five 0.424 u cards at radius ≈ 0.70, spanning z −0.075…0.349).
const RUNE_TABLET_COUNT: u32 = 5;
const RUNE_TABLET_RADIUS: f32 = 0.70;
const RUNE_TABLET_SIZE: f32 = 0.424;
const RUNE_TABLET_HEIGHT: f32 = 0.137;

/// A shrinking mote stream sharing ONE material across the rig (motes fade by
/// shrinking, so per-piece colour is not needed).
pub struct MoteEmitter {
    pub kind: DotMoteKind,
    /// Motes per second.
    pub rate: f32,
    pub life: f32,
    /// Travel speed along the emission direction, yards/sec. Negative on the
    /// downward-falling streams.
    pub speed: f32,
    pub color: Color,
    pub emissive: f32,
}

/// A GROWING wisp stream. Growing pieces cannot fade by shrinking, so each
/// carries its own material and ramps along its [`DotWispKind`] tracks.
pub struct BloomEmitter {
    pub kind: DotWispKind,
    pub rate: f32,
    pub life: f32,
    pub speed: f32,
}

/// Which primitive rig an apparition builds.
pub enum ApparitionShape {
    /// A skull above the victim's head; `bone` adds Curse of Weakness's
    /// `bone_purple` element beside it.
    Skull { bone: bool },
    /// Curse of Tongues' rune circle at the victim's chest: two flat discs
    /// plus upright glyph tablets on a slowly turning ring.
    RuneCircle,
}

/// Everything one curse's apply apparition needs. One `const` per curse in
/// [`CURSE_APPARITIONS`]; adding a fourth curse is a table entry, not a new
/// spawner.
pub struct CurseApparitionSpec {
    pub curse: CurseKind,
    /// The exact RON `name:` string the detector keys on.
    pub aura_name: &'static str,
    /// The aura type the curse applies. The name alone is not enough: CoA is
    /// a `DamageOverTime`, CoW a `DamageReduction`, CoT a `CastTimeIncrease`.
    pub aura_type: AuraType,
    pub shape: ApparitionShape,
    /// Combatant-local height of the rig anchor (head or chest).
    pub anchor_y: f32,
    /// Seconds until the whole apparition is retired.
    pub life: f32,
    pub fade_in: f32,
    pub fade_out_start: f32,
    /// Peak opacity of the envelope (CoT's client track tops out at 0.6).
    pub peak_alpha: f32,
    /// The two global flicker cycles in seconds, or `None` for a steady sigil.
    pub flicker: Option<(f32, f32)>,
    /// Seconds per revolution for a rig that turns on its own axis, or `None`
    /// to yaw toward the camera instead (the skulls, so their faces read).
    pub spin_period: Option<f32>,
    pub shell_color: Color,
    pub shell_emissive: f32,
    pub core_color: Color,
    pub core_emissive: f32,
    /// The shrinking spark / rune-mote stream.
    pub spark: Option<MoteEmitter>,
    /// The downward glow-mote stream (Curse of Agony only — no other curse
    /// kit has a downward emitter).
    pub fall: Option<MoteEmitter>,
    /// The swelling bloom stream.
    pub bloom: Option<BloomEmitter>,
}

/// Curse of Agony — client kit 884, `curseofagony_head.m2`. Blessed values,
/// unchanged from AS-18: red shells, yellow core, red-orange star sparks and
/// glow motes sinking over the face.
///
/// Emissive strengths are USER-TUNED round-3 ("a touch too bright" verdict,
/// 2026-09-07): the round-2 unlit→lit fix woke this rig's formerly-dead
/// emissive (it shipped at shells 2.4 / core+sparks 2.6 / fall motes 1.8),
/// and the lit skull read as a floodlight. Dimmed ~33%, preserving the
/// red-shell < yellow-core ordering and the spark/fall balance.
const AGONY: CurseApparitionSpec = CurseApparitionSpec {
    curse: CurseKind::Agony,
    aura_name: "Curse of Agony",
    aura_type: AuraType::DamageOverTime,
    shape: ApparitionShape::Skull { bone: false },
    anchor_y: IMPACT_HEAD_Y,
    // Client transparency track: fade-out ends ~2.8 s into the 3000 ms
    // one-shot; fade-in 134 ms; fade-out begins ~2270 ms.
    life: 2.8,
    fade_in: 0.134,
    fade_out_start: 2.27,
    peak_alpha: 1.0,
    // Client global sequences: 433 / 267 ms.
    flicker: Some((0.433, 0.267)),
    spin_period: None,
    // Client mesh tints: red (0.96,0,0) shells, yellow (1.0,0.96,0) glows.
    shell_color: Color::srgb(0.96, 0.05, 0.05),
    shell_emissive: 1.6,
    core_color: Color::srgb(1.0, 0.94, 0.15),
    core_emissive: 1.75,
    spark: Some(MoteEmitter {
        kind: DotMoteKind::SkullSpark,
        // Client red_star2 emitter: 180/s, life 0.75 s, speed 0.83 u/s.
        rate: 180.0,
        life: 0.75,
        speed: 0.83,
        // White→orange→red in the client; one shared additive material at
        // the orange mid, faded by shrinking.
        color: Color::srgb(0.94, 0.42, 0.14),
        emissive: 1.8,
    }),
    fall: Some(MoteEmitter {
        kind: DotMoteKind::SkullFall,
        // Client red_glow3 emitter: 20.7/s, life 1.0 s, −1.11 u/s (downward).
        rate: 20.7,
        life: 1.0,
        speed: -1.11,
        color: Color::srgb(1.0, 0.85, 0.75),
        emissive: 1.25,
    }),
    bloom: None,
};

/// Curse of Weakness — client kit 719, `curseofmannoroth_head.m2` (the curse
/// family's filenames lie; the kit joins do not). CoA's envelope and flicker
/// to the millisecond, with the palette inverted: VIOLET shells over a GREEN
/// core, green star sparks, and swelling violet blooms.
///
/// Two documented transcription collapses (AS-19 §5): the client's two green
/// star emitters are identical but for the direction of their white↔green
/// ramp and become one stream; its two additive violet bloom emitters differ
/// by ~10 % in speed/life/size and become one at the pair's per-emitter rate.
/// Its two GREY `toonsmoke16` emitters are omitted outright — they are
/// alpha-blend smoke that REMOVES light from the apparition's interior, and
/// reproducing them additively (the house convention outside Corruption's one
/// blessed Blend exception) would invert their sign.
const WEAKNESS: CurseApparitionSpec = CurseApparitionSpec {
    curse: CurseKind::Weakness,
    aura_name: "Curse of Weakness",
    aura_type: AuraType::DamageReduction,
    shape: ApparitionShape::Skull { bone: true },
    anchor_y: IMPACT_HEAD_Y,
    life: 2.8,
    fade_in: 0.134,
    fade_out_start: 2.27,
    peak_alpha: 1.0,
    flicker: Some((0.433, 0.267)),
    spin_period: None,
    // Client mesh tints: violet (0.329,0,1.0) skull+bone shells, green
    // (0,1,0) glow quads. (A third glow quad is tinted red in the client;
    // dropped here — red on a head apparition is the channel Curse of Agony
    // owns, and keeping it works against the distinctness the rest of the
    // data is spending. AS-19 §5.)
    shell_color: Color::srgb(0.33, 0.02, 1.0),
    shell_emissive: 1.6,
    core_color: Color::srgb(0.18, 1.0, 0.20),
    core_emissive: 1.75,
    spark: Some(MoteEmitter {
        kind: DotMoteKind::SkullSpark,
        // Client starflash_grey + star5a, 150/s each, life 0.75 s, 0.83 u/s.
        // The pair is merged (they differ only in the direction of their
        // white<->green ramp), so a faithful merge would run at 300/s. 200/s
        // is an AUTHORED trim of that, not a measurement: 300/s of star
        // sprites reads as a solid haze at our mote scale, and this is the
        // density the round-1 in-client eyeball blessed. (The bloom below
        // merges its pair's shape but keeps their per-emitter rate.)
        rate: 200.0,
        life: 0.75,
        speed: 0.83,
        // White→bright green→pale green in the client; the shared material
        // sits at the bright-green mid and fades by shrinking.
        color: Color::srgb(0.20, 0.95, 0.10),
        emissive: 1.8,
    }),
    fall: None,
    bloom: Some(BloomEmitter {
        kind: DotWispKind::CurseBloom,
        // Client toonsmoke16 additive pair: 50/s each, lives 0.8/1.2 s,
        // speeds 1.17/1.28 u/s — collapsed to one at the pair's mid values.
        rate: 50.0,
        life: 1.0,
        speed: 1.22,
    }),
};

/// Curse of Tongues — client kit 503, `curseoftongues_impact.m2`. The outlier
/// of the family: a magenta-violet RUNE CIRCLE at the CHEST, not a skull over
/// the head — two flat rune discs plus five upright glyph tablets on a ring
/// that turns once every 2 s, peaking at α 0.6 and gone by 1.67 s.
///
/// The client also gives CoT a persistent aura state (kit 502, a rune plate
/// re-flashing for ~1.9 s of every 8.166 s loop). That is measured in AS-19
/// §3 but deliberately NOT implemented here — see `CURSE_SUSTAIN_WHISPER`.
const TONGUES: CurseApparitionSpec = CurseApparitionSpec {
    curse: CurseKind::Tongues,
    aura_name: "Curse of Tongues",
    aura_type: AuraType::CastTimeIncrease,
    shape: ApparitionShape::RuneCircle,
    anchor_y: CHEST_Y,
    // Client transparency track: 0→0.6 by 333 ms, hold to 1266 ms, out by
    // 1533 ms, dead at 1666 ms.
    life: 1.666,
    fade_in: 0.333,
    fade_out_start: 1.266,
    peak_alpha: 0.6,
    // No flicker; the client drives this one off a single 2000 ms global
    // sequence — transcribed as the ring's rotation.
    flicker: None,
    spin_period: Some(2.0),
    // Client mesh tint (0.592, 0, 0.933), one tint for the whole mesh.
    shell_color: Color::srgb(0.59, 0.0, 0.93),
    shell_emissive: 2.0,
    core_color: Color::srgb(0.72, 0.14, 0.98),
    core_emissive: 2.2,
    spark: Some(MoteEmitter {
        kind: DotMoteKind::RuneMote,
        // Client aurarune_a emitter: 6/s, life 1.2 s, speed 0.056 u/s.
        rate: 6.0,
        life: 1.2,
        speed: 0.06,
        color: Color::srgb(0.72, 0.18, 0.92),
        emissive: 2.0,
    }),
    fall: None,
    bloom: Some(BloomEmitter {
        kind: DotWispKind::RuneGlow,
        // Client genericglow_black emitter: 6/s, life 0.63 s, 0.033 u/s.
        rate: 6.0,
        life: 0.63,
        speed: 0.04,
    }),
};

/// Every curse apparition, in [`CurseKind`] order.
pub const CURSE_APPARITIONS: [CurseApparitionSpec; 3] = [AGONY, WEAKNESS, TONGUES];

/// The measured spec for one curse.
pub fn curse_spec(curse: CurseKind) -> &'static CurseApparitionSpec {
    let spec = &CURSE_APPARITIONS[curse as usize];
    // The table is indexed positionally, so reordering it would hand every
    // curse another curse's whole apparition — a silent swap no probe would
    // catch, since each effect would still spawn and animate correctly.
    debug_assert_eq!(
        spec.curse, curse,
        "CURSE_APPARITIONS is out of CurseKind order"
    );
    spec
}

// --- Unstable Affliction (authored) ------------------------------------------

/// UA's pulse period — deliberately ≠ Corruption's `PULSE_PERIOD` (3.00 s)
/// so the two sustained rhythms never read as one when stacked.
pub const UA_PULSE_PERIOD: f32 = 1.20;
/// Blessed master scale on the torso glow.
pub const UA_GLOW_SCALE: f32 = 1.0;
/// Depth of the nervous flicker multiplied into the glow (0..1).
pub const UA_FLICKER_AMOUNT: f32 = 0.5;
/// Seconds between crackle discharges.
pub const UA_CRACKLE_PERIOD: f32 = 2.5;
/// Master intensity of the crackle (STACKED-tuned: the pop must read through
/// Corruption's shroud).
pub const UA_CRACKLE_INTENSITY: f32 = 1.4;
/// Length of one crackle discharge.
pub const UA_CRACKLE_SECS: f32 = 0.22;
/// Jagged bolts per discharge.
pub const UA_CRACKLE_BOLTS: u32 = 5;
/// Segments per bolt (the kinks are the jag).
const UA_BOLT_SEGMENTS: u32 = 3;
/// Length of one bolt segment, yards (scaled by intensity at spawn).
const UA_BOLT_SEGMENT_LEN: f32 = 0.34;
/// Bolt quad width. USER-TUNED (2026-09-06): 0.055 read as thin pale
/// threads even at full intensity — widened so the discharge is an event.
const UA_BOLT_WIDTH: f32 = 0.10;
/// Radius of the torso glow quad. The billboard pass holds it
/// `UA_CAMERA_LIFT` toward the camera, so the full disc — bright centre
/// included — reads over the torso instead of only the annulus outside the
/// body silhouette (the round-2 fix; AS-10's clear-the-capsule margin still
/// applies to the silhouette the glow must halo past).
pub const UA_GLOW_RADIUS: f32 = 0.9;
/// Radius of the crackle pop BEFORE the `UA_CRACKLE_INTENSITY` multiplier.
/// At intensity 1.4 the pop reaches 1.19 yd — outside `SHROUD_RADIUS`
/// (0.68), which is what makes it read on a Corruption-stacked victim.
pub const UA_CRACKLE_POP_RADIUS: f32 = 0.85;
/// The authored violet (the client spends violet on shadow apply moments;
/// UA's sustained glow claims it for the state channel). USER-TUNED
/// (2026-09-06): red lifted 0.55→0.62 — over the lit body the original mix
/// read blue, not violet.
const UA_GLOW_COLOR: Color = Color::srgb(0.62, 0.18, 0.92);
const UA_BOLT_COLOR: Color = Color::srgb(0.75, 0.35, 1.0);
/// Height of the torso glow above the victim's transform.
const UA_GLOW_Y: f32 = 0.35;
/// How far the billboard pass lifts the UA glow and crackle pop toward the
/// camera. The quads are anchored at the torso CENTRE, so unlifted they lose
/// the depth test to the body capsule and only the sprite's dim outer
/// annulus reads (the round-2 "blue speckles" finding). The lift must clear
/// both occluders: `COMBATANT_BODY_RADIUS` (0.5 — the opaque body) and
/// `SHROUD_RADIUS` (0.68 — else Corruption's Blend shroud sorts in front and
/// dims the stacked flash), so it is derived from the larger of the two.
pub const UA_CAMERA_LIFT: f32 = SHROUD_RADIUS + 0.07;

/// The exact aura names the detectors key on (the RON `name:` strings, same
/// as the class-AI dedup checks).
pub const CORRUPTION_AURA: &str = "Corruption";
/// Kept alongside the `CURSE_APPARITIONS` table's own `aura_name` because
/// the curses are ALSO referenced outside the table (probes, docs).
pub const COA_AURA: &str = "Curse of Agony";
pub const COW_AURA: &str = "Curse of Weakness";
pub const COT_AURA: &str = "Curse of Tongues";
pub const UA_AURA: &str = "Unstable Affliction";

// --- shared helpers ----------------------------------------------------------

/// 3-point piecewise-linear ramp keyed at start / midpoint / end of `k` 0..1.
fn ramp3(track: [f32; 3], k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    if k < 0.5 {
        track[0] + (track[1] - track[0]) * (k / 0.5)
    } else {
        track[1] + (track[2] - track[1]) * ((k - 0.5) / 0.5)
    }
}

/// 3-point color ramp (component-wise [`ramp3`] in linear srgb).
fn ramp_color(track: [Color; 3], k: f32) -> Color {
    let [a, b, c] = track.map(|c| c.to_srgba());
    Color::srgb(
        ramp3([a.red, b.red, c.red], k),
        ramp3([a.green, b.green, c.green], k),
        ramp3([a.blue, b.blue, c.blue], k),
    )
}

fn emissive_of(color: Color, strength: f32) -> LinearRgba {
    let c = color.to_linear();
    LinearRgba::rgb(c.red * strength, c.green * strength, c.blue * strength)
}

/// The scale / alpha / colour tracks one [`DotWispKind`] ramps along over its
/// life, all transcribed from the client's per-particle tracks.
pub struct WispTracks {
    /// Half-size in yards at start / mid / end of life (full quad = 2×).
    pub scale: [f32; 3],
    pub alpha: [f32; 3],
    pub color: [Color; 3],
    pub emissive: f32,
}

/// The per-kind wisp tracks. `color[0]` is what the spawn site builds the
/// wisp's material at; `age_warlock_dot_particles` ramps it from there.
pub fn wisp_tracks(kind: DotWispKind) -> WispTracks {
    match kind {
        DotWispKind::CorruptionMurk => WispTracks {
            scale: WISP_SCALE,
            alpha: WISP_ALPHA,
            color: WISP_COLOR,
            emissive: 1.6,
        },
        // Client kit 719, the additive `toonsmoke16` pair: alpha 0→0.47→0,
        // growing 0.194→0.306→0.611, violet (110,0,234)→(146,116,209)→
        // (156,0,255), RGB/255.
        DotWispKind::CurseBloom => WispTracks {
            scale: [0.19, 0.31, 0.61],
            alpha: [0.0, 0.47, 0.0],
            color: [
                Color::srgb(0.43, 0.0, 0.92),
                Color::srgb(0.57, 0.45, 0.82),
                Color::srgb(0.61, 0.0, 1.0),
            ],
            emissive: 1.3,
        },
        // Client kits 502/503, `genericglow_black`: alpha 0→0.70→0.05,
        // growing 0.303→0.567→0.781, (30,30,30)→(33,8,41)→(170,30,236).
        DotWispKind::RuneGlow => WispTracks {
            scale: [0.30, 0.57, 0.78],
            alpha: [0.0, 0.70, 0.05],
            color: [
                Color::srgb(0.12, 0.12, 0.12),
                Color::srgb(0.13, 0.03, 0.16),
                Color::srgb(0.67, 0.12, 0.93),
            ],
            emissive: 1.2,
        },
    }
}

/// Cheap deterministic jitter in [0, 1). Visual only — never `game_rng`.
fn dot_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

/// Does this unit carry the named Warlock DoT? Keys on the exact ability
/// name plus the DoT aura type, so e.g. UA's silence backlash never counts.
pub fn has_warlock_dot(auras: Option<&ActiveAuras>, name: &str) -> bool {
    has_named_aura(auras, name, AuraType::DamageOverTime)
}

/// Does this unit carry the named aura of the given type? The name alone is
/// not enough — the curses apply three different aura types, and UA's dispel
/// backlash shares its ability name with the DoT.
pub fn has_named_aura(auras: Option<&ActiveAuras>, name: &str, effect_type: AuraType) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|au| au.effect_type == effect_type && au.ability_name == name)
    })
}

/// World anchor for a rig piece at combatant-local height `local_y`. A pet's
/// rendered body hangs below its sim transform (`VisualBody::rest_y` — the
/// AS-14 boar lesson: sim-y alone floats effects 0.73 yd high), so the same
/// stature correction `school_impact.rs` uses is applied here.
pub fn dot_anchor(local_y: f32, translation: Vec3, is_pet: bool) -> Vec3 {
    let y = if is_pet {
        IMPACT_PET_BODY_Y + local_y * IMPACT_PET_STATURE
    } else {
        local_y
    };
    translation + Vec3::Y * y
}

/// Scale multiplier for rig pieces on a pet victim.
pub fn dot_stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// A curse apparition's flicker: the product of its two client global cycles,
/// normalized to 0.55..1.0 so the apparition never blinks out. A spec without
/// a flicker (Curse of Tongues' steady sigil) returns a flat 1.0.
pub fn curse_flicker(curse: CurseKind, age: f32) -> f32 {
    match curse_spec(curse).flicker {
        Some((period_a, period_b)) => {
            let a = 0.5 + 0.5 * (age * TAU / period_a).sin();
            let b = 0.5 + 0.5 * (age * TAU / period_b).sin();
            0.55 + 0.45 * a * b
        }
        None => 1.0,
    }
}

/// A curse apparition's alpha envelope: fade in, hold at `peak_alpha`, fade
/// out between `fade_out_start` and `life`.
pub fn curse_envelope(curse: CurseKind, age: f32) -> f32 {
    let spec = curse_spec(curse);
    if !(0.0..spec.life).contains(&age) {
        return 0.0;
    }
    let fade_in = (age / spec.fade_in).clamp(0.0, 1.0);
    let fade_out = ((spec.life - age) / (spec.life - spec.fade_out_start)).clamp(0.0, 1.0);
    spec.peak_alpha * fade_in.min(fade_out)
}

/// Corruption's shroud darkening at `age`: a re-bloom to `SHROUD_DARKNESS`
/// at the top of every `PULSE_PERIOD` cycle, fading toward zero over the
/// cycle (the client mesh tint's 1→0 alpha key over the 3000 ms loop).
pub fn shroud_alpha(age: f32) -> f32 {
    let phase = (age / PULSE_PERIOD).fract();
    SHROUD_DARKNESS * (1.0 - phase)
}

/// UA crackle timing: `Some(k)` (0..1 through the discharge) when `age` sits
/// in the `UA_CRACKLE_SECS` window at the END of a crackle cycle — the first
/// discharge fires `UA_CRACKLE_PERIOD` after apply, not on top of the apply
/// burst.
pub fn ua_crackle_k(age: f32) -> Option<f32> {
    let into_cycle = age % UA_CRACKLE_PERIOD;
    let start = UA_CRACKLE_PERIOD - UA_CRACKLE_SECS;
    (into_cycle >= start).then(|| (into_cycle - start) / UA_CRACKLE_SECS)
}

/// Which crackle cycle `age` falls in (for fire-once bookkeeping).
pub fn ua_crackle_cycle(age: f32) -> u32 {
    (age / UA_CRACKLE_PERIOD) as u32
}

/// Meshes and sprites every rig shares. Built once, lazily.
pub struct DotAssets {
    quad: Handle<Mesh>,
    ring: Handle<Mesh>,
    dot: Handle<Image>,
    star: Handle<Image>,
}

impl DotAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            // Unit-radius ring in the XZ plane; the ring ramp drives scale.
            ring: meshes.add(Mesh::from(Torus {
                minor_radius: 0.045,
                major_radius: 1.0,
            })),
            dot: images.add(soft_dot_texture()),
            star: images.add(star_flash_texture()),
        }
    }
}

/// Lit-emissive, NOT `unlit`: the unlit branch of `pbr.wgsl` is
/// `out.color = material.base_color` — it discards emissive outright (see
/// `hard_cc.rs::STUN_BEAD_COLOR`), which left every glow in this module LDR
/// flat with nothing for `Bloom::NATURAL` to bloom (the round-2 "UA is
/// substantially less visible than Corruption" finding). `AlphaMode::Add`
/// premultiplies the whole fragment — emissive included — by `base_color`'s
/// alpha, so the animate systems' alpha envelopes still gate the emissive.
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

// ==============================================================================
// Spawn (detector) — aura-keyed transitions
// ==============================================================================

/// Detect Warlock DoT auras appearing on living victims and build the rigs:
///
/// - Corruption newly present → shroud state rig + shared apply burst.
/// - Unstable Affliction newly present → UA state rig + the SAME apply burst
///   (client kit 117 is shared — see module docs).
/// - Any curse in `CURSE_APPARITIONS` newly present → that curse's one-shot
///   apply apparition, latched by a bit in `CurseApparitionsFired` (the
///   apparitions are apply-only, so a live rig can't be the dedup key). The
///   bit is dropped when that curse leaves, so a re-curse fires a fresh
///   apparition.
///
/// State rigs are keyed one-per-victim; a REFRESH of an already-running DoT
/// does not re-fire the apply burst (transition-in only). Dead victims spawn
/// nothing — `update_auras` skips corpses, so their auras linger (the fear
/// corpse lesson).
pub fn spawn_warlock_dot_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<DotAssets>>,
    afflicted: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&Pet>,
        Option<&CurseApparitionsFired>,
    )>,
    shrouds: Query<&CorruptionShroudRig>,
    ua_rigs: Query<&UaStateRig>,
) {
    use std::collections::HashSet;
    let shrouded: HashSet<Entity> = shrouds.iter().map(|r| r.target).collect();
    let ua_lit: HashSet<Entity> = ua_rigs.iter().map(|r| r.target).collect();

    for (entity, combatant, transform, auras, pet, curse_latch) in afflicted.iter() {
        if !combatant.is_alive() {
            continue;
        }
        let is_pet = pet.is_some();
        let assets = assets.get_or_insert_with(|| DotAssets::build(&mut meshes, &mut images));

        if has_warlock_dot(auras, CORRUPTION_AURA) && !shrouded.contains(&entity) {
            spawn_shroud_rig(
                &mut commands,
                &mut meshes,
                &mut materials,
                assets,
                entity,
                transform.translation,
                is_pet,
            );
            spawn_apply_burst(
                &mut commands,
                &mut materials,
                assets,
                entity,
                transform.translation,
                is_pet,
            );
        }

        if has_warlock_dot(auras, UA_AURA) && !ua_lit.contains(&entity) {
            spawn_ua_rig(
                &mut commands,
                &mut materials,
                assets,
                entity,
                transform.translation,
                is_pet,
            );
            // The client's UA apply IS Corruption's — same kit 117.
            spawn_apply_burst(
                &mut commands,
                &mut materials,
                assets,
                entity,
                transform.translation,
                is_pet,
            );
        }

        // Every curse's apply apparition, table-driven and latched by bit so
        // a victim can carry two different Warlocks' curses at once.
        let already = curse_latch.map(|l| l.fired).unwrap_or(0);
        let mut fired = already;
        for curse in CurseKind::ALL {
            let spec = curse_spec(curse);
            if fired & curse.bit() != 0 || !has_named_aura(auras, spec.aura_name, spec.aura_type) {
                continue;
            }
            spawn_curse_apparition(
                &mut commands,
                &mut meshes,
                &mut materials,
                assets,
                spec,
                entity,
                transform.translation,
                is_pet,
            );
            fired |= curse.bit();
        }
        if fired != already {
            commands
                .entity(entity)
                .try_insert(CurseApparitionsFired { fired });
        }
    }
}

/// Build the shared kit-117 apply burst: the expanding shadow ring at the
/// chest plus its spark-sphere emitter (sparks are emitted over the ring
/// window by `animate_dot_apply_bursts`).
fn spawn_apply_burst(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &DotAssets,
    target: Entity,
    at: Vec3,
    is_pet: bool,
) {
    let s = dot_stature(is_pet);
    // The ring and the sparks share the burst's ramping palette: one material
    // each, mutated along the green→violet→dark track by burst age.
    let ring_material = additive_material(materials, APPLY_COLOR[0], 2.2, None);
    let spark_material = additive_material(materials, APPLY_COLOR[0], 2.4, Some(assets.star.clone()));

    let ring = commands
        .spawn((
            DotSprite {
                role: DotSpriteRole::ShadowRing,
                radius: APPLY_RING_SCALE[2] * APPLY_BURST_SCALE * s,
                base_alpha: APPLY_RING_ALPHA[1],
                life: f32::INFINITY,
                age: 0.0,
            },
            Mesh3d(assets.ring.clone()),
            MeshMaterial3d(ring_material),
            Transform::from_scale(Vec3::splat(1e-4)),
            NotShadowCaster,
        ))
        .id();

    commands
        .spawn((
            DotApplyBurst {
                target,
                kind: DotApplyKind::ShadowRing,
                age: 0.0,
                spark_carry: 0.0,
                emitted: 0,
            },
            WarlockDotRigAssets {
                quad: assets.quad.clone(),
                mote_material: spark_material.clone(),
                extra_material: spark_material,
                soft_dot: assets.dot.clone(),
            },
            Transform::from_translation(dot_anchor(CHEST_Y, at, is_pet)),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&[ring]);
}

/// Build Corruption's persistent state rig: the darkening shroud shell (the
/// Blend exception — see module docs) with the wisp/fizz emitters ticked by
/// `animate_corruption_shrouds`.
fn spawn_shroud_rig(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &DotAssets,
    target: Entity,
    at: Vec3,
    is_pet: bool,
) {
    let s = dot_stature(is_pet);
    let shell_mesh = meshes.add(Capsule3d::new(SHROUD_RADIUS * s, SHROUD_LENGTH * s));
    // The ONE deliberate AlphaMode::Blend: an additive shroud cannot DARKEN
    // its victim (module docs). Z-fighting is prevented by construction — the
    // shell stands SHROUD_STANDOFF off the body capsule, never coplanar.
    let shell_material = materials.add(StandardMaterial {
        base_color: SHROUD_COLOR.with_alpha(SHROUD_DARKNESS),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let fizz_material = additive_material(materials, FIZZ_COLOR, 2.2, Some(assets.star.clone()));

    let shell = commands
        .spawn((
            DotSprite {
                role: DotSpriteRole::ShroudShell,
                radius: SHROUD_RADIUS * s,
                base_alpha: SHROUD_DARKNESS,
                life: f32::INFINITY,
                age: 0.0,
            },
            Mesh3d(shell_mesh),
            MeshMaterial3d(shell_material),
            Transform::default(),
            NotShadowCaster,
        ))
        .id();

    commands
        .spawn((
            CorruptionShroudRig {
                target,
                age: 0.0,
                wisp_carry: 0.0,
                fizz_carry: 0.0,
                emitted: 0,
            },
            WarlockDotRigAssets {
                quad: assets.quad.clone(),
                mote_material: fizz_material.clone(),
                extra_material: fizz_material,
                soft_dot: assets.dot.clone(),
            },
            Transform::from_translation(dot_anchor(SHROUD_CENTER_Y, at, is_pet)),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&[shell]);
}

/// Build one curse's apply apparition from its [`CurseApparitionSpec`] — a
/// skull above the head (Agony, Weakness) or a rune circle at the chest
/// (Tongues) — plus the emitter streams ticked by
/// `animate_curse_apparitions`.
fn spawn_curse_apparition(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &DotAssets,
    spec: &'static CurseApparitionSpec,
    target: Entity,
    at: Vec3,
    is_pet: bool,
) {
    let stature = dot_stature(is_pet);
    let pieces = match spec.shape {
        ApparitionShape::Skull { bone } => {
            spawn_skull_pieces(commands, meshes, materials, assets, spec, stature, bone)
        }
        ApparitionShape::RuneCircle => {
            spawn_rune_circle_pieces(commands, materials, assets, spec, stature)
        }
    };

    let spark_material = spec.spark.as_ref().map(|e| {
        additive_material(materials, e.color, e.emissive, Some(assets.star.clone()))
    });
    let fall_material = spec.fall.as_ref().map(|e| {
        additive_material(materials, e.color, e.emissive, Some(assets.dot.clone()))
    });
    // Every rig carries both mote slots; a spec without an emitter simply
    // never emits into its slot, so the placeholder is never rendered.
    // DORMANT TODAY: all three curses declare a spark emitter, so this arm is
    // unreachable — `unwrap_or_else` means it also costs nothing. It stays so
    // that a fourth curse with no spark stream is a table entry rather than a
    // new code path (or a panic).
    let placeholder = || additive_material(materials, spec.shell_color, 1.0, None);
    let mote_material = spark_material.unwrap_or_else(placeholder);
    let extra_material = fall_material.unwrap_or_else(|| mote_material.clone());

    commands
        .spawn((
            CurseApparitionRig {
                target,
                curse: spec.curse,
                age: 0.0,
                spark_carry: 0.0,
                fall_carry: 0.0,
                bloom_carry: 0.0,
                emitted: 0,
            },
            WarlockDotRigAssets {
                quad: assets.quad.clone(),
                mote_material,
                extra_material,
                soft_dot: assets.dot.clone(),
            },
            Transform::from_translation(dot_anchor(spec.anchor_y, at, is_pet)),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&pieces);
}

/// The skull rig: additive cranium + jaw shells, two dark eye sockets, a core
/// glow, and — for Curse of Weakness — the bone beside the skull. The
/// primitive-mesh transcription of `curseofagony_head.m2` /
/// `curseofmannoroth_head.m2`, which share a silhouette and differ in palette.
fn spawn_skull_pieces(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &DotAssets,
    spec: &CurseApparitionSpec,
    stature: f32,
    bone: bool,
) -> Vec<Entity> {
    let s = stature * CURSE_SKULL_SIZE;
    let lift = CURSE_SKULL_LIFT * s;

    let shell = |materials: &mut Assets<StandardMaterial>| {
        additive_material(materials, spec.shell_color, spec.shell_emissive, None)
    };
    let cranium = commands
        .spawn((
            DotSprite {
                role: DotSpriteRole::SkullCranium,
                radius: 0.30 * s,
                base_alpha: 0.85,
                life: f32::INFINITY,
                age: 0.0,
            },
            Mesh3d(meshes.add(Sphere::new(0.30 * s))),
            MeshMaterial3d(shell(materials)),
            Transform::from_translation(Vec3::Y * lift).with_scale(Vec3::new(1.0, 1.08, 1.0)),
            NotShadowCaster,
        ))
        .id();
    let jaw = commands
        .spawn((
            DotSprite {
                role: DotSpriteRole::SkullJaw,
                radius: 0.16 * s,
                base_alpha: 0.85,
                life: f32::INFINITY,
                age: 0.0,
            },
            Mesh3d(meshes.add(Sphere::new(0.16 * s))),
            MeshMaterial3d(shell(materials)),
            Transform::from_translation(Vec3::new(0.0, lift - 0.32 * s, 0.08 * s))
                .with_scale(Vec3::new(0.9, 0.75, 0.9)),
            NotShadowCaster,
        ))
        .id();
    // Eye sockets: small Blend-dark spheres — the only way to punch dark
    // holes in an additive shell. Offset well off every other surface (no
    // coplanar faces → no Z-fight; same rationale as the fear shards).
    let eye_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.05, 0.0, 0.0, 0.9),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let mut pieces = vec![cranium, jaw];
    for side in [-1.0_f32, 1.0] {
        pieces.push(
            commands
                .spawn((
                    DotSprite {
                        role: DotSpriteRole::SkullEye,
                        radius: 0.075 * s,
                        base_alpha: 0.9,
                        life: f32::INFINITY,
                        age: 0.0,
                    },
                    Mesh3d(meshes.add(Sphere::new(0.075 * s))),
                    MeshMaterial3d(eye_material.clone()),
                    Transform::from_translation(Vec3::new(
                        side * 0.115 * s,
                        lift + 0.05 * s,
                        0.24 * s,
                    )),
                    NotShadowCaster,
                ))
                .id(),
        );
    }
    // The core glow behind the shell (billboarded) — yellow on Agony, green
    // on Weakness.
    pieces.push(
        commands
            .spawn((
                DotSprite {
                    role: DotSpriteRole::SkullCore,
                    radius: 0.45 * s,
                    base_alpha: 0.8,
                    life: f32::INFINITY,
                    age: 0.0,
                },
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(additive_material(
                    materials,
                    spec.core_color,
                    spec.core_emissive,
                    Some(assets.dot.clone()),
                )),
                Transform::from_translation(Vec3::new(0.0, lift, -0.02 * s)),
                NotShadowCaster,
            ))
            .id(),
    );

    // Curse of Weakness's bone: the client's `bone_purple` submesh sits at
    // (−0.14, −0.01, 0.76) in model units, on the opposite side from the
    // skull's own +0.13 offset. A tilted capsule beside the skull is what
    // tells CoW from CoA at a glance when the palette is washed out by
    // bloom.
    if bone {
        pieces.push(
            commands
                .spawn((
                    DotSprite {
                        role: DotSpriteRole::SkullBone,
                        radius: BONE_RADIUS * s,
                        base_alpha: 0.85,
                        life: f32::INFINITY,
                        age: 0.0,
                    },
                    Mesh3d(meshes.add(Capsule3d::new(BONE_RADIUS * s, BONE_LENGTH * s))),
                    MeshMaterial3d(shell(materials)),
                    Transform::from_translation(Vec3::new(-BONE_OFFSET * s, lift + 0.01 * s, 0.0))
                        .with_rotation(Quat::from_rotation_z(BONE_TILT)),
                    NotShadowCaster,
                ))
                .id(),
        );
    }
    pieces
}

/// Curse of Tongues' rune circle at the chest: two flat magenta-violet rune
/// discs lying in the ground plane plus `RUNE_TABLET_COUNT` upright glyph
/// tablets on a ring, each turned to face OUTWARD. (The client's five glyph
/// cards share one plane normal — translated copies, not rotated ones — which
/// reads edge-on from half the bearings an arena camera can take; facing them
/// outward is the deliberate transcription deviation. AS-19 §5.)
fn spawn_rune_circle_pieces(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &DotAssets,
    spec: &CurseApparitionSpec,
    stature: f32,
) -> Vec<Entity> {
    let s = stature;
    // Untextured, like the apply burst's shadow ring: the soft radial sprite
    // tiles around a torus's tube and would read as a string of blobs rather
    // than a clean glowing rune circle.
    let disc_material =
        additive_material(materials, spec.shell_color, spec.shell_emissive, None);
    let mut pieces = Vec::new();
    for height in RUNE_DISC_HEIGHTS {
        let radius = RUNE_DISC_RADIUS * s;
        pieces.push(
            commands
                .spawn((
                    DotSprite {
                        role: DotSpriteRole::RuneDisc,
                        radius,
                        base_alpha: 0.9,
                        life: f32::INFINITY,
                        age: 0.0,
                    },
                    Mesh3d(assets.ring.clone()),
                    MeshMaterial3d(disc_material.clone()),
                    // The unit-radius torus lies in the XZ plane; the ring
                    // scale is the disc radius (y left at 1 so the tube keeps
                    // its thickness).
                    Transform::from_translation(Vec3::Y * (height * s))
                        .with_scale(Vec3::new(radius, 1.0, radius)),
                    NotShadowCaster,
                ))
                .id(),
        );
    }

    let tablet_material = additive_material(
        materials,
        spec.core_color,
        spec.core_emissive,
        Some(assets.star.clone()),
    );
    for i in 0..RUNE_TABLET_COUNT {
        let theta = i as f32 / RUNE_TABLET_COUNT as f32 * TAU;
        let outward = Vec3::new(theta.cos(), 0.0, theta.sin());
        let size = RUNE_TABLET_SIZE * s;
        pieces.push(
            commands
                .spawn((
                    DotSprite {
                        role: DotSpriteRole::RuneTablet,
                        radius: size,
                        base_alpha: 0.95,
                        life: f32::INFINITY,
                        age: 0.0,
                    },
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(tablet_material.clone()),
                    // The unit quad's normal is +Z; rotate it so the normal
                    // points radially outward and the tablet stands upright.
                    Transform::from_translation(
                        outward * (RUNE_TABLET_RADIUS * s) + Vec3::Y * (RUNE_TABLET_HEIGHT * s),
                    )
                    .with_rotation(Quat::from_rotation_y(FRAC_PI_2 - theta))
                    .with_scale(Vec3::splat(size)),
                    NotShadowCaster,
                ))
                .id(),
        );
    }
    pieces
}

/// Build Unstable Affliction's authored state rig: the pulsing violet torso
/// glow and the (initially invisible) crackle pop; bolts are spawned per
/// discharge by `animate_ua_states`.
fn spawn_ua_rig(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &DotAssets,
    target: Entity,
    at: Vec3,
    is_pet: bool,
) {
    let s = dot_stature(is_pet);
    let glow = commands
        .spawn((
            DotSprite {
                role: DotSpriteRole::UaGlow,
                radius: UA_GLOW_RADIUS * UA_GLOW_SCALE * s,
                // USER-TUNED (2026-09-06): 0.55 → 0.75. The rendered level is
                // base_alpha × glow_level with emissive × glow_level on top —
                // a squared pulse falloff — and at 0.55 the steady state read
                // as barely-there.
                base_alpha: 0.75,
                life: f32::INFINITY,
                age: 0.0,
            },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(additive_material(
                materials,
                UA_GLOW_COLOR,
                2.2,
                Some(assets.dot.clone()),
            )),
            Transform::from_scale(Vec3::splat(1e-4)),
            NotShadowCaster,
        ))
        .id();
    let pop = commands
        .spawn((
            DotSprite {
                role: DotSpriteRole::CracklePop,
                radius: UA_CRACKLE_POP_RADIUS * UA_CRACKLE_INTENSITY * s,
                base_alpha: 0.9,
                life: f32::INFINITY,
                age: 0.0,
            },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(additive_material(
                materials,
                UA_BOLT_COLOR,
                2.8 * UA_CRACKLE_INTENSITY,
                Some(assets.star.clone()),
            )),
            Transform::from_scale(Vec3::splat(1e-4)),
            NotShadowCaster,
        ))
        .id();

    commands
        .spawn((
            UaStateRig {
                target,
                age: 0.0,
                last_crackle_cycle: u32::MAX,
            },
            WarlockDotRigAssets {
                quad: assets.quad.clone(),
                mote_material: additive_material(materials, UA_BOLT_COLOR, 2.6, None),
                extra_material: additive_material(materials, UA_BOLT_COLOR, 2.6, None),
                soft_dot: assets.dot.clone(),
            },
            Transform::from_translation(dot_anchor(UA_GLOW_Y, at, is_pet)),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&[glow, pop]);
}

// ==============================================================================
// Animate — one system per rig kind (follow, envelopes, emission)
// ==============================================================================

/// Drive the shared apply burst: follow the victim, ramp the shadow ring
/// (scale/color/alpha along the client tracks), shift the whole burst's
/// palette green→violet→near-black, emit the outward spark sphere over the
/// ring window, and retire the rig at `APPLY_BURST_LIFE`.
pub fn animate_dot_apply_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bursts: Query<(
        Entity,
        &mut DotApplyBurst,
        &WarlockDotRigAssets,
        &mut Transform,
        &Children,
    )>,
    targets: Query<
        (&Transform, Option<&Pet>),
        (With<Combatant>, Without<DotApplyBurst>, Without<DotSprite>),
    >,
    mut rings: Query<
        (&DotSprite, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<DotApplyBurst>, Without<DotMote>),
    >,
) {
    let dt = time.delta_secs();
    for (entity, mut burst, rig_assets, mut transform, children) in bursts.iter_mut() {
        burst.age += dt;
        let age = burst.age;
        if age >= APPLY_BURST_LIFE {
            commands.entity(entity).despawn();
            continue;
        }
        let mut stature = 1.0;
        if let Ok((target, pet)) = targets.get(burst.target) {
            transform.translation = dot_anchor(CHEST_Y, target.translation, pet.is_some());
            stature = dot_stature(pet.is_some());
        }

        // The whole burst's palette rides the ring window, holding the final
        // near-black once the ring is spent.
        let k = (age / APPLY_RING_SECS).clamp(0.0, 1.0);
        let palette = ramp_color(APPLY_COLOR, k);
        if let Some(material) = materials.get_mut(&rig_assets.mote_material) {
            material.base_color = palette;
            material.emissive = emissive_of(palette, 2.4);
        }

        for child in children.iter() {
            if let Ok((sprite, mut part, material)) = rings.get_mut(child) {
                if sprite.role != DotSpriteRole::ShadowRing {
                    continue;
                }
                let (scale, alpha) = if age >= APPLY_RING_SECS {
                    (1e-4, 0.0)
                } else {
                    (
                        ramp3(APPLY_RING_SCALE, k) * APPLY_BURST_SCALE * stature,
                        ramp3(APPLY_RING_ALPHA, k),
                    )
                };
                part.scale = Vec3::new(scale, 1.0, scale).max(Vec3::splat(1e-4));
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color = palette.with_alpha(alpha);
                    material.emissive = emissive_of(palette, 2.2 * alpha.max(0.0));
                }
            }
        }

        // The spark sphere: dense green→violet motes blowing outward in all
        // directions over the ring window.
        if age < APPLY_RING_SECS {
            burst.spark_carry += APPLY_SPARK_RATE * dt;
            while burst.spark_carry >= 1.0 {
                burst.spark_carry -= 1.0;
                let i = burst.emitted;
                burst.emitted = burst.emitted.wrapping_add(1);
                let seed = entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
                // Uniform-ish direction over the sphere from two jitters.
                let theta = dot_jitter(seed) * TAU;
                let z = dot_jitter(seed ^ 0x51ED) * 2.0 - 1.0;
                let r = (1.0 - z * z).max(0.0).sqrt();
                let dir = Vec3::new(r * theta.cos(), z, r * theta.sin());
                let mote = commands
                    .spawn((
                        DotMote {
                            kind: DotMoteKind::ApplySpark,
                            velocity: dir * APPLY_SPARK_SPEED,
                            age: 0.0,
                            life: APPLY_SPARK_LIFE,
                            size: stature,
                        },
                        Mesh3d(rig_assets.quad.clone()),
                        MeshMaterial3d(rig_assets.mote_material.clone()),
                        Transform::from_translation(dir * 0.1 * stature)
                            .with_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(entity).add_child(mote);
            }
        }
    }
}

/// Drive Corruption's state: follow the victim, re-bloom the darkening
/// shroud on the `PULSE_PERIOD` cycle, and emit the murk wisps and green
/// fizz. The rig lives until `cleanup_warlock_dot_visuals` sees the aura
/// gone — no self-expiry, no end flourish.
pub fn animate_corruption_shrouds(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(
        Entity,
        &mut CorruptionShroudRig,
        &WarlockDotRigAssets,
        &mut Transform,
        &Children,
    )>,
    targets: Query<
        (&Transform, Option<&Pet>),
        (With<Combatant>, Without<CorruptionShroudRig>, Without<DotSprite>),
    >,
    shells: Query<
        (&DotSprite, &MeshMaterial3d<StandardMaterial>),
        Without<CorruptionShroudRig>,
    >,
) {
    let dt = time.delta_secs();
    for (entity, mut rig, rig_assets, mut transform, children) in rigs.iter_mut() {
        rig.age += dt;
        let age = rig.age;
        let mut stature = 1.0;
        if let Ok((target, pet)) = targets.get(rig.target) {
            transform.translation =
                dot_anchor(SHROUD_CENTER_Y, target.translation, pet.is_some());
            stature = dot_stature(pet.is_some());
        }

        // The re-bloom throb: darkest at each cycle top, fading over 3 s.
        let alpha = shroud_alpha(age);
        for child in children.iter() {
            if let Ok((sprite, material)) = shells.get(child) {
                if sprite.role == DotSpriteRole::ShroudShell {
                    if let Some(material) = materials.get_mut(&material.0) {
                        material.base_color = SHROUD_COLOR.with_alpha(alpha);
                    }
                }
            }
        }

        // Murk-green swelling wisps: slow drift, per-wisp color ramp (each
        // carries its own material — the ramp near-black-green → sickly
        // yellow-green is written per-wisp, unlike the shrink-faded motes).
        rig.wisp_carry += WISP_RATE * MOTE_RATE_SCALE * dt;
        while rig.wisp_carry >= 1.0 {
            rig.wisp_carry -= 1.0;
            let i = rig.emitted;
            rig.emitted = rig.emitted.wrapping_add(1);
            let seed = entity.index().wrapping_add(i.wrapping_mul(0x9E37_79B9));
            let theta = dot_jitter(seed) * TAU;
            let z = dot_jitter(seed ^ 0x51ED) * 2.0 - 1.0;
            let r = (1.0 - z * z).max(0.0).sqrt();
            let dir = Vec3::new(r * theta.cos(), z, r * theta.sin());
            let origin = Vec3::new(
                (dot_jitter(seed ^ 0x27D4) - 0.5) * 0.56,
                (dot_jitter(seed ^ 0x9E37) - 0.5) * 0.8,
                (dot_jitter(seed ^ 0xC2B2) - 0.5) * 0.56,
            ) * stature;
            // The soft radial sprite, not a bare quad — a hard-edged
            // additive rectangle reads as a block, not a wisp.
            let wisp_material = additive_material(
                &mut materials,
                WISP_COLOR[0],
                1.6,
                Some(rig_assets.soft_dot.clone()),
            );
            let wisp = commands
                .spawn((
                    DotWisp {
                        kind: DotWispKind::CorruptionMurk,
                        velocity: dir * WISP_DRIFT_SPEED,
                        age: 0.0,
                        life: WISP_LIFE,
                        stature,
                    },
                    Mesh3d(rig_assets.quad.clone()),
                    MeshMaterial3d(wisp_material),
                    Transform::from_translation(origin).with_scale(Vec3::splat(1e-4)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(entity).add_child(wisp);
        }

        // The green mote fizz streaming up off the victim.
        rig.fizz_carry += FIZZ_RATE * MOTE_RATE_SCALE * dt;
        while rig.fizz_carry >= 1.0 {
            rig.fizz_carry -= 1.0;
            let i = rig.emitted;
            rig.emitted = rig.emitted.wrapping_add(1);
            let seed = entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
            let origin = Vec3::new(
                (dot_jitter(seed) - 0.5) * 0.56,
                (dot_jitter(seed ^ 0x51ED) - 0.5) * 1.0,
                (dot_jitter(seed ^ 0x27D4) - 0.5) * 0.56,
            ) * stature;
            let mote = commands
                .spawn((
                    DotMote {
                        kind: DotMoteKind::Fizz,
                        velocity: Vec3::Y * FIZZ_RISE_SPEED,
                        age: 0.0,
                        life: FIZZ_LIFE,
                        size: stature,
                    },
                    Mesh3d(rig_assets.quad.clone()),
                    MeshMaterial3d(rig_assets.mote_material.clone()),
                    Transform::from_translation(origin).with_scale(Vec3::splat(1e-4)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(entity).add_child(mote);
        }
    }
}

/// Drive every curse apply apparition off its [`CurseApparitionSpec`]: follow
/// the victim's anchor, run the fade-in/flicker/fade-out envelope over every
/// piece, spin the rigs that spin, emit the spark / fall / bloom streams the
/// spec declares, and retire the whole apparition at `spec.life` — after
/// which the curse shows NOTHING, on purpose (`CURSE_SUSTAIN_WHISPER`).
pub fn animate_curse_apparitions(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(
        Entity,
        &mut CurseApparitionRig,
        &WarlockDotRigAssets,
        &mut Transform,
        &Children,
    )>,
    targets: Query<
        (&Transform, Option<&Pet>),
        (With<Combatant>, Without<CurseApparitionRig>, Without<DotSprite>),
    >,
    mut pieces: Query<
        (&DotSprite, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<CurseApparitionRig>, Without<DotMote>),
    >,
) {
    let dt = time.delta_secs();
    for (entity, mut rig, rig_assets, mut transform, children) in rigs.iter_mut() {
        let spec = curse_spec(rig.curse);
        rig.age += dt;
        let age = rig.age;
        if age >= spec.life {
            commands.entity(entity).despawn();
            continue;
        }
        let mut stature = 1.0;
        if let Ok((target, pet)) = targets.get(rig.target) {
            transform.translation = dot_anchor(spec.anchor_y, target.translation, pet.is_some());
            stature = dot_stature(pet.is_some());
        }
        // The skull rig's local frame is scaled by the blessed master size;
        // the rune circle is authored at client scale.
        let s = match spec.shape {
            ApparitionShape::Skull { .. } => stature * CURSE_SKULL_SIZE,
            ApparitionShape::RuneCircle => stature,
        };

        let envelope = curse_envelope(rig.curse, age) * curse_flicker(rig.curse, age);
        for child in children.iter() {
            if let Ok((sprite, mut part, material)) = pieces.get_mut(child) {
                let alpha = sprite.base_alpha * envelope;
                if sprite.role == DotSpriteRole::SkullCore {
                    // The core glow breathes with the flicker (the client's
                    // genericglow2b bloom pulses behind the shell).
                    let bloom = 0.7 + 0.3 * envelope;
                    part.scale = Vec3::splat((sprite.radius * 2.0 * bloom).max(1e-4));
                }
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(alpha);
                }
            }
        }

        // The shrinking spark / rune-mote stream, blowing outward.
        if let Some(emitter) = spec.spark.as_ref() {
            rig.spark_carry += emitter.rate * SPARK_RATE_SCALE * dt;
            while rig.spark_carry >= 1.0 {
                rig.spark_carry -= 1.0;
                let i = rig.emitted;
                rig.emitted = rig.emitted.wrapping_add(1);
                let seed = entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
                let theta = dot_jitter(seed) * TAU;
                let z = dot_jitter(seed ^ 0x51ED) * 2.0 - 1.0;
                let r = (1.0 - z * z).max(0.0).sqrt();
                let dir = Vec3::new(r * theta.cos(), z, r * theta.sin());
                let origin = spark_origin(spec, seed, s);
                let mote = commands
                    .spawn((
                        DotMote {
                            kind: emitter.kind,
                            velocity: dir * emitter.speed,
                            age: 0.0,
                            life: emitter.life,
                            size: stature,
                        },
                        Mesh3d(rig_assets.quad.clone()),
                        MeshMaterial3d(rig_assets.mote_material.clone()),
                        Transform::from_translation(origin).with_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(entity).add_child(mote);
            }
        }

        // Glow motes sinking down over the victim's face and chest.
        if let Some(emitter) = spec.fall.as_ref() {
            rig.fall_carry += emitter.rate * SPARK_RATE_SCALE * dt;
            while rig.fall_carry >= 1.0 {
                rig.fall_carry -= 1.0;
                let i = rig.emitted;
                rig.emitted = rig.emitted.wrapping_add(1);
                let seed = entity.index().wrapping_add(i.wrapping_mul(0x9E37_79B9));
                let origin = Vec3::Y * (0.25 * s)
                    + Vec3::new(
                        (dot_jitter(seed) - 0.5) * 1.12,
                        0.0,
                        (dot_jitter(seed ^ 0x51ED) - 0.5) * 1.12,
                    ) * s;
                let mote = commands
                    .spawn((
                        DotMote {
                            kind: emitter.kind,
                            velocity: Vec3::Y * emitter.speed,
                            age: 0.0,
                            life: emitter.life,
                            size: stature,
                        },
                        Mesh3d(rig_assets.quad.clone()),
                        MeshMaterial3d(rig_assets.extra_material.clone()),
                        Transform::from_translation(origin).with_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(entity).add_child(mote);
            }
        }

        // The swelling blooms. Like Corruption's wisps these GROW, so each
        // carries its own material and ramps by alpha rather than shrinking.
        if let Some(emitter) = spec.bloom.as_ref() {
            rig.bloom_carry += emitter.rate * SPARK_RATE_SCALE * dt;
            while rig.bloom_carry >= 1.0 {
                rig.bloom_carry -= 1.0;
                let i = rig.emitted;
                rig.emitted = rig.emitted.wrapping_add(1);
                let seed = entity.index().wrapping_add(i.wrapping_mul(0x27D4_EB2F));
                let theta = dot_jitter(seed) * TAU;
                let z = dot_jitter(seed ^ 0x51ED) * 2.0 - 1.0;
                let r = (1.0 - z * z).max(0.0).sqrt();
                let dir = Vec3::new(r * theta.cos(), z, r * theta.sin());
                let origin = spark_origin(spec, seed ^ 0xB529, s);
                let tracks = wisp_tracks(emitter.kind);
                let bloom_material = additive_material(
                    &mut materials,
                    tracks.color[0],
                    tracks.emissive,
                    Some(rig_assets.soft_dot.clone()),
                );
                let bloom = commands
                    .spawn((
                        DotWisp {
                            kind: emitter.kind,
                            velocity: dir * emitter.speed,
                            age: 0.0,
                            life: emitter.life,
                            stature,
                        },
                        Mesh3d(rig_assets.quad.clone()),
                        MeshMaterial3d(bloom_material),
                        Transform::from_translation(origin).with_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(entity).add_child(bloom);
            }
        }
    }
}

/// Where one emitted piece starts, in the rig's local frame. The skulls emit
/// from a jittered box around the cranium (client emitters all sit at
/// (0,0,0.83) above the head attach); the rune circle emits off its ring
/// (the client's two emitters sit ~0.55 u out in front of the chest).
fn spark_origin(spec: &CurseApparitionSpec, seed: u32, s: f32) -> Vec3 {
    match spec.shape {
        ApparitionShape::Skull { .. } => {
            Vec3::Y * (CURSE_SKULL_LIFT * s)
                + Vec3::new(
                    (dot_jitter(seed ^ 0x27D4) - 0.5) * 0.56,
                    (dot_jitter(seed ^ 0x9E37) - 0.5) * 0.66,
                    (dot_jitter(seed ^ 0xC2B2) - 0.5) * 0.56,
                ) * s
        }
        ApparitionShape::RuneCircle => {
            let theta = dot_jitter(seed ^ 0x27D4) * TAU;
            Vec3::new(
                theta.cos() * RUNE_TABLET_RADIUS * s,
                RUNE_TABLET_HEIGHT * s,
                theta.sin() * RUNE_TABLET_RADIUS * s,
            )
        }
    }
}

/// Drive Unstable Affliction's authored state: follow the victim, pulse and
/// flicker the violet torso glow on `UA_PULSE_PERIOD`, and fire the crackle
/// discharge (5 jagged bolts + the bright pop) once per `UA_CRACKLE_PERIOD`.
pub fn animate_ua_states(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(
        Entity,
        &mut UaStateRig,
        &WarlockDotRigAssets,
        &mut Transform,
        &Children,
    )>,
    targets: Query<
        (&Transform, Option<&Pet>),
        (With<Combatant>, Without<UaStateRig>, Without<DotSprite>),
    >,
    mut pieces: Query<
        (&DotSprite, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<UaStateRig>, Without<DotMote>),
    >,
) {
    let dt = time.delta_secs();
    for (entity, mut rig, rig_assets, mut transform, children) in rigs.iter_mut() {
        rig.age += dt;
        let age = rig.age;
        let mut stature = 1.0;
        if let Ok((target, pet)) = targets.get(rig.target) {
            transform.translation = dot_anchor(UA_GLOW_Y, target.translation, pet.is_some());
            stature = dot_stature(pet.is_some());
        }

        // Glow pulse (1.2 s) with a nervous hash-seeded flicker on top —
        // deterministic (entity index + a 50 ms time bucket), never game_rng.
        let pulse = 0.5 + 0.5 * (age * TAU / UA_PULSE_PERIOD).sin();
        let bucket = (age / 0.05) as u32;
        let flicker =
            1.0 - UA_FLICKER_AMOUNT * dot_jitter(entity.index().wrapping_add(bucket.wrapping_mul(0xB529_7A4D)));
        let glow_level = (0.45 + 0.55 * pulse) * flicker;

        let crackle = ua_crackle_k(age);
        let cycle = ua_crackle_cycle(age);

        for child in children.iter() {
            if let Ok((sprite, mut part, material)) = pieces.get_mut(child) {
                match sprite.role {
                    DotSpriteRole::UaGlow => {
                        let bloom = 0.85 + 0.15 * pulse;
                        part.scale = Vec3::splat((sprite.radius * 2.0 * bloom).max(1e-4));
                        if let Some(material) = materials.get_mut(&material.0) {
                            material.base_color =
                                UA_GLOW_COLOR.with_alpha(sprite.base_alpha * glow_level);
                            // USER-TUNED (2026-09-06): 2.2 → 3.0 emissive —
                            // alpha premultiplies the emissive under Add, so
                            // the effective glow rides glow_level SQUARED.
                            material.emissive =
                                emissive_of(UA_GLOW_COLOR, 3.0 * UA_GLOW_SCALE * glow_level);
                        }
                    }
                    DotSpriteRole::CracklePop => {
                        let (scale, alpha) = match crackle {
                            Some(k) => {
                                // Snap open, fade over the discharge.
                                let open = (k / 0.25).clamp(0.0, 1.0);
                                (
                                    sprite.radius * 2.0 * (0.5 + 0.5 * open),
                                    sprite.base_alpha * (1.0 - k),
                                )
                            }
                            None => (1e-4, 0.0),
                        };
                        part.scale = Vec3::splat(scale.max(1e-4));
                        if let Some(material) = materials.get_mut(&material.0) {
                            material.base_color.set_alpha(alpha);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Fire the discharge's bolts exactly once per cycle.
        if crackle.is_some() && rig.last_crackle_cycle != cycle {
            rig.last_crackle_cycle = cycle;
            for bolt in 0..UA_CRACKLE_BOLTS {
                let bolt_seed = entity
                    .index()
                    .wrapping_add(cycle.wrapping_mul(0x27D4_EB2F))
                    .wrapping_add(bolt.wrapping_mul(0x1656_67B1));
                let base_theta = dot_jitter(bolt_seed) * TAU;
                let mut from = Vec3::ZERO;
                let mut dir = Vec3::new(base_theta.cos(), 0.0, base_theta.sin());
                for seg in 0..UA_BOLT_SEGMENTS {
                    // Kink the direction per segment — the jag.
                    let j = |k: u32| {
                        dot_jitter(
                            bolt_seed
                                ^ seg
                                    .wrapping_mul(31)
                                    .wrapping_add(k)
                                    .wrapping_mul(0x9E37_79B9),
                        )
                    };
                    let yaw = Quat::from_rotation_y((j(1) - 0.5) * 1.1);
                    let pitch_axis = Vec3::Y.cross(dir).normalize_or_zero();
                    let pitch = Quat::from_axis_angle(pitch_axis, (j(2) - 0.5) * 1.0);
                    dir = (pitch * yaw * dir).normalize_or_zero();
                    let len = UA_BOLT_SEGMENT_LEN * UA_CRACKLE_INTENSITY * stature
                        * (0.8 + 0.4 * j(3));
                    let to = from + dir * len;
                    let mid = (from + to) / 2.0;
                    let rotation = Quat::from_rotation_arc(Vec3::Y, dir);
                    let segment = commands
                        .spawn((
                            DotSprite {
                                role: DotSpriteRole::CrackleBolt,
                                radius: len,
                                base_alpha: 0.95,
                                life: UA_CRACKLE_SECS,
                                age: 0.0,
                            },
                            Mesh3d(rig_assets.quad.clone()),
                            MeshMaterial3d(additive_material(
                                &mut materials,
                                UA_BOLT_COLOR,
                                2.6 * UA_CRACKLE_INTENSITY,
                                None,
                            )),
                            Transform::from_translation(mid)
                                .with_rotation(rotation)
                                .with_scale(Vec3::new(UA_BOLT_WIDTH * stature, len, 1.0)),
                            NotShadowCaster,
                        ))
                        .id();
                    commands.entity(entity).add_child(segment);
                    from = to;
                }
            }
        }
    }
}

// ==============================================================================
// Particle aging — motes, wisps, finite-life sprites
// ==============================================================================

/// Age every emitted piece: move motes along their velocities and fade them
/// by SHRINKING (their materials are shared per rig), grow the wisps along
/// the client scale track while their per-wisp materials ramp
/// near-black-green → sickly yellow-green, and fade/despawn finite-life
/// sprites (the crackle bolts). Time-driven (`Res<Time>`, never gated on sim
/// movement — the fixed-timestep-strobe trap).
pub fn age_warlock_dot_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut motes: Query<(Entity, &mut DotMote, &mut Transform), (Without<DotWisp>, Without<DotSprite>)>,
    mut wisps: Query<
        (Entity, &mut DotWisp, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<DotMote>, Without<DotSprite>),
    >,
    mut sprites: Query<
        (Entity, &mut DotSprite, &MeshMaterial3d<StandardMaterial>),
        (Without<DotMote>, Without<DotWisp>),
    >,
) {
    let dt = time.delta_secs();

    for (entity, mut mote, mut transform) in motes.iter_mut() {
        mote.age += dt;
        if mote.age >= mote.life {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += mote.velocity * dt;
        let k = (mote.age / mote.life).clamp(0.0, 1.0);
        // Size tracks transcribed from the client, raised ~35% like the heal
        // motes (the raw 0.03–0.13 u quads are near-invisible on a 2.5 u
        // body); full quad size = 2 × the track value.
        let half = match mote.kind {
            DotMoteKind::ApplySpark => ramp3([0.10, 0.16, 0.10], k),
            DotMoteKind::Fizz => ramp3([0.13, 0.08, 0.04], k),
            DotMoteKind::SkullSpark => ramp3([0.14, 0.11, 0.03], k),
            DotMoteKind::SkullFall => ramp3([0.14, 0.08, 0.01], k),
            // Client kit 503 `aurarune_a`: 0.708→0.339→0.097, a big rune
            // mote collapsing to a speck.
            DotMoteKind::RuneMote => ramp3([0.35, 0.17, 0.05], k),
        };
        transform.scale = Vec3::splat((half * 2.0 * mote.size).max(1e-4));
    }

    for (entity, mut wisp, mut transform, material) in wisps.iter_mut() {
        wisp.age += dt;
        if wisp.age >= wisp.life {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += wisp.velocity * dt;
        let k = (wisp.age / wisp.life).clamp(0.0, 1.0);
        let tracks = wisp_tracks(wisp.kind);
        transform.scale = Vec3::splat((ramp3(tracks.scale, k) * wisp.stature).max(1e-4));
        if let Some(material) = materials.get_mut(&material.0) {
            let color = ramp_color(tracks.color, k);
            let alpha = ramp3(tracks.alpha, k);
            material.base_color = color.with_alpha(alpha);
            material.emissive = emissive_of(color, tracks.emissive * alpha);
        }
    }

    for (entity, mut sprite, material) in sprites.iter_mut() {
        if !sprite.life.is_finite() {
            continue;
        }
        sprite.age += dt;
        if sprite.age >= sprite.life {
            commands.entity(entity).despawn();
            continue;
        }
        let k = (sprite.age / sprite.life).clamp(0.0, 1.0);
        if let Some(material) = materials.get_mut(&material.0) {
            material.base_color.set_alpha(sprite.base_alpha * (1.0 - k));
        }
    }
}

// ==============================================================================
// Billboarding
// ==============================================================================

/// Orient each curse apparition. A spec without a `spin_period` (the skulls)
/// yaws to face the camera so its face (eye sockets at local +Z) reads from
/// any bearing; a spec WITH one (Curse of Tongues' rune circle) turns on its
/// own axis at that rate instead, transcribing the client's 2000 ms global
/// sequence. Runs BEFORE [`billboard_warlock_dot_visuals`], whose child
/// facing derives from the rig rotation this writes — a separate system
/// because the billboard pass reads every rig transform and Bevy rejects a
/// same-system mut/read overlap (B0001).
pub fn orient_curse_apparitions(
    camera: Query<&Transform, (With<Camera3d>, Without<CurseApparitionRig>)>,
    mut apparitions: Query<
        (&CurseApparitionRig, &mut Transform),
        (With<CurseApparitionRig>, Without<Camera3d>),
    >,
) {
    let cam = camera.iter().next();
    for (apparition, mut rig) in apparitions.iter_mut() {
        match curse_spec(apparition.curse).spin_period {
            Some(period) => {
                rig.rotation = Quat::from_rotation_y(apparition.age / period * TAU);
            }
            None => {
                let Some(cam) = cam else { continue };
                let to_cam = cam.translation - rig.translation;
                rig.rotation = Quat::from_rotation_y(to_cam.x.atan2(to_cam.z));
            }
        }
    }
}

/// Turn the flat pieces toward the camera: the UA glow, the crackle pop, the
/// skull's core glow, every mote and wisp. The shadow ring stays flat in the
/// ground plane, the shroud shell and skull spheres are 3D, and bolt
/// segments keep their kinked world poses — none of those are billboarded.
pub fn billboard_warlock_dot_visuals(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<DotSprite>,
            Without<DotMote>,
            Without<DotWisp>,
        ),
    >,
    rigs: Query<
        (&Transform, &Children),
        (
            Or<(
                With<DotApplyBurst>,
                With<CorruptionShroudRig>,
                With<CurseApparitionRig>,
                With<UaStateRig>,
            )>,
            Without<Camera3d>,
            Without<DotSprite>,
            Without<DotMote>,
            Without<DotWisp>,
        ),
    >,
    mut sprites: Query<
        (&DotSprite, &mut Transform),
        (Without<Camera3d>, Without<DotMote>, Without<DotWisp>),
    >,
    mut motes: Query<
        &mut Transform,
        (
            Or<(With<DotMote>, With<DotWisp>)>,
            Without<Camera3d>,
            Without<DotSprite>,
        ),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };

    for (rig, children) in rigs.iter() {
        let facing = rig.rotation.inverse() * cam.rotation;
        // Local-frame step toward the camera for the lifted quads (rigs are
        // top-level entities, so rig.translation IS world space).
        let lift = rig.rotation.inverse()
            * ((cam.translation - rig.translation).normalize_or_zero() * UA_CAMERA_LIFT);
        for child in children.iter() {
            if let Ok((sprite, mut part)) = sprites.get_mut(child) {
                match sprite.role {
                    DotSpriteRole::UaGlow | DotSpriteRole::CracklePop => {
                        part.rotation = facing;
                        // Lift the quad proud of the body capsule (and the
                        // stacked shroud shell) so its bright centre wins
                        // the depth test instead of only the dim annulus
                        // outside the silhouette reading — the round-2
                        // "blue speckles" finding. Safe to overwrite: these
                        // two quads author no local translation.
                        part.translation = lift;
                    }
                    DotSpriteRole::SkullCore => {
                        // Billboards but keeps its authored skull-local
                        // translation.
                        part.rotation = facing;
                    }
                    // The ring lies flat; shells/spheres/bones are 3D; the
                    // rune discs lie flat and the tablets keep their radial
                    // poses; bolts keep their kinked poses.
                    _ => {}
                }
            }
            if let Ok(mut part) = motes.get_mut(child) {
                part.rotation = facing;
            }
        }
    }
}

// ==============================================================================
// Cleanup — aura-keyed lifecycle ends
// ==============================================================================

/// End the aura-keyed visuals when their aura ends:
///
/// - Corruption shroud / UA state rigs despawn the frame their DoT is gone
///   from the victim (expire OR dispel — the detector keys on presence, so
///   both paths are one code path) or the victim dies (auras linger on
///   corpses — the fear lesson) or despawns.
/// - A `CurseApparitionsFired` bit is dropped when its curse is gone, so a
///   fresh curse fires a fresh apparition; the whole latch is removed once no
///   curse remains.
/// - One-shot rigs (apply burst, curse apparitions) normally play themselves
///   out even if the aura is dispelled mid-flourish — they are apply-moment
///   records, not state — but die with a dead or despawned victim.
pub fn cleanup_warlock_dot_visuals(
    mut commands: Commands,
    shrouds: Query<(Entity, &CorruptionShroudRig)>,
    ua_rigs: Query<(Entity, &UaStateRig)>,
    apparitions: Query<(Entity, &CurseApparitionRig)>,
    bursts: Query<(Entity, &DotApplyBurst)>,
    marked: Query<
        (Entity, &Combatant, Option<&ActiveAuras>, &CurseApparitionsFired),
        With<CurseApparitionsFired>,
    >,
    targets: Query<(&Combatant, Option<&ActiveAuras>)>,
) {
    let state_lives = |target: Entity, aura: &str| -> bool {
        targets
            .get(target)
            .map(|(c, a)| c.is_alive() && has_warlock_dot(a, aura))
            .unwrap_or(false)
    };
    let target_alive = |target: Entity| -> bool {
        targets.get(target).map(|(c, _)| c.is_alive()).unwrap_or(false)
    };

    for (entity, rig) in shrouds.iter() {
        if !state_lives(rig.target, CORRUPTION_AURA) {
            commands.entity(entity).despawn();
        }
    }
    for (entity, rig) in ua_rigs.iter() {
        if !state_lives(rig.target, UA_AURA) {
            commands.entity(entity).despawn();
        }
    }
    for (entity, rig) in apparitions.iter() {
        if !target_alive(rig.target) {
            commands.entity(entity).despawn();
        }
    }
    for (entity, burst) in bursts.iter() {
        if !target_alive(burst.target) {
            commands.entity(entity).despawn();
        }
    }
    for (entity, combatant, auras, latch) in marked.iter() {
        let mut still = 0u8;
        if combatant.is_alive() {
            for curse in CurseKind::ALL {
                let spec = curse_spec(curse);
                if latch.fired & curse.bit() != 0
                    && has_named_aura(auras, spec.aura_name, spec.aura_type)
                {
                    still |= curse.bit();
                }
            }
        }
        if still == latch.fired {
            continue;
        }
        if still == 0 {
            commands.entity(entity).remove::<CurseApparitionsFired>();
        } else {
            commands
                .entity(entity)
                .try_insert(CurseApparitionsFired { fired: still });
        }
    }
}
