use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, TAU};

use super::school_impact::{IMPACT_HEAD_Y, IMPACT_PET_BODY_Y, IMPACT_PET_STATURE};
use super::spell_bolts::{soft_dot_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Heal impact — the per-spell, Classic-faithful landings on a heal's recipient
// ==============================================================================
//
// The healing counterpart of `school_impact.rs`. Until this module every heal
// landed as the same translucent cylinder (`HealingLightColumn`) — a shape
// that exists NOWHERE in the Classic client data: the measured verdict from
// `design-docs/2026-09-06-heal-impact-client-data.md` (wago.tools DB2 + M2
// parsing, build 1.15.9.69547) is that **no Classic heal lands as a column**.
// What the client actually plays, and what this module reproduces with the
// codebase's primitive-mesh vocabulary:
//
// - **Flash Heal** (`flashheal_base.m2`, Base attach): a golden lens-flare
//   flash with 8 radiating gradient light rays, then a quick sparse column of
//   rising gold star/ribbon motes. The showiest Priest impact.
// - **Heal / Holy Shock's heal** (`heal_low_base.m2`, Base): no flash at all —
//   a narrow (~0.56 u) quiet stream of gold motes rising through the body.
//   Holy Shock's heal trigger resolves to the SAME visual 135 / kit 232.
// - **Holy Light** (`holylight_low_head.m2`, HEAD attach — the only
//   head-attached heal): a glow bloom at the head and a widening curtain of
//   gold stars and soft light-puffs FALLING over the whole body — the exact
//   inverse of the Priest's rising motes, and the grandest in the set.
// - **Flash of Light**: impact-less for players in the source; the non-player
//   variants (visuals 6622/7379) borrow Holy Light's kit 154, so FoL renders
//   the Holy Light effect at reduced intensity and duration.
// - **Lesser Healing Wave / Healing Wave** (`restoration_impact_base.m2`,
//   Base — one visual for both ranks and both spells): green/gold glow layers
//   WRAPPING the torso (the source vertex cloud sits at radial extent
//   1.2–1.7 u — around the body, never inside it), a green pool of light at
//   the feet where the Base attach grounds the effect, 8 small butterflies
//   fluttering in orbit, and a light dusting of rising gold stars. The only
//   non-pure-gold heal.
//
// Everything is additive (every material and emitter in the source set is M2
// blend mode 4), Holy is pure gold, Nature is green + gold. Emitter constants
// (speeds, lives, rates, areas, origins) are transcribed from the parsed M2
// particle records; ramped rates are 3-point piecewise-linear over the emit
// window, as keyed in the source. Butterflies and rays are primitive meshes —
// the polymorph-sheep idiom — shape and motion carry the design.

// --- Flash Heal (blessed defaults) -------------------------------------------
pub const FLASH_HEAL_RAY_COUNT: u32 = 8;
pub const FLASH_HEAL_RAY_LENGTH: f32 = 1.6;
pub const FLASH_HEAL_FLASH_DURATION: f32 = 0.60;
pub const FLASH_HEAL_FLASH_INTENSITY: f32 = 1.0;
/// How long Flash Heal's mote emitters run.
pub const FLASH_HEAL_EMIT_SECS: f32 = 1.2;
/// Width of one of the 8 rays at full alpha, yards.
pub const FLASH_HEAL_RAY_WIDTH: f32 = 0.20;
/// Radius of the central lens-flare quad.
pub const FLASH_HEAL_FLARE_RADIUS: f32 = 0.65;

/// Global tuning knobs over every landing's transcribed emitter constants.
pub const HEAL_MOTE_RATE_SCALE: f32 = 1.0;
pub const HEAL_MOTE_SPEED_SCALE: f32 = 1.0;

// --- Heal / Holy Shock heal --------------------------------------------------
/// Emission area of the stream's star emitters — the narrow envelope.
pub const HEAL_STREAM_WIDTH: f32 = 0.56;
pub const HEAL_STREAM_ACTIVE_SECS: f32 = 1.0;

// --- Holy Light / Flash of Light ---------------------------------------------
pub const HOLY_LIGHT_DURATION: f32 = 1.5;
pub const HOLY_LIGHT_HEAD_GLOW_INTENSITY: f32 = 1.0;
/// Radius of the head glow bloom.
pub const HOLY_LIGHT_HEAD_GLOW_RADIUS: f32 = 0.70;
/// Cone spread of the falling curtain, radians (source: 0.26).
pub const HOLY_LIGHT_CONE_SPREAD: f32 = 0.26;
/// Flash of Light renders Holy Light's effect scaled by this intensity.
pub const FLASH_OF_LIGHT_BORROW_INTENSITY: f32 = 0.55;
pub const FLASH_OF_LIGHT_DURATION: f32 = 0.90;

// --- Lesser Healing Wave / Healing Wave --------------------------------------
pub const HEALING_WAVE_DURATION: f32 = 1.4;
pub const HEALING_WAVE_BUTTERFLIES: u32 = 8;
pub const HEALING_WAVE_ORBIT_RADIUS: f32 = 1.45;
pub const HEALING_WAVE_SWIRL_SPEED_SCALE: f32 = 1.0;
pub const HEALING_WAVE_GLOW_INTENSITY: f32 = 1.0;
/// Radius of the combatant capsule (`Capsule3d::new(0.5, 1.5)`). A glow quad
/// billboarded on the spine axis renders at or BEHIND the capsule's front
/// surface out to this radius, so any wrap layer must clear it with margin or
/// the body occludes the glow entirely — which is exactly how the third build
/// (0.50/0.38/0.28 radii) lost the green glow. Pinned by
/// `healing_wave_glow_wraps_outside_the_body`.
pub const COMBATANT_BODY_RADIUS: f32 = 0.5;
/// Radius of the green pool of light at the recipient's feet — the Base
/// attach grounding the effect (blessed band 0.8–1.2).
pub const HEALING_WAVE_UNDERGLOW_RADIUS: f32 = 1.0;
pub const HEALING_WAVE_UNDERGLOW_ALPHA: f32 = 0.35;
/// World height of the arena floor plane. `create_arena_floor_mesh`
/// (`play_match/mod.rs`) emits its vertices at y = 0 under an identity
/// transform, and combatants stand ON it with their capsule CENTRES at world
/// y = 1.0 — the 2.5-yd capsule is deliberately sunk 0.25 into the ground, so
/// the capsule bottom (combatant-local -1.25) is NOT the floor. Ground decals
/// anchor to this plane, never to the capsule bottom: the in-repo authorities
/// are `selection.rs::RING_GROUND_OFFSET_Y` and `polymorph.rs`'s `ground_y`
/// derivation (`-(transform.y + rest_y)`), both of which resolve to this
/// world plane.
pub const ARENA_FLOOR_WORLD_Y: f32 = 0.0;
/// Lift of the under-glow pool above the floor plane. The floor is opaque and
/// depth-writing, so an additive quad AT or BELOW it is depth-rejected and
/// never renders; this lift matches the selection ring's rendered height
/// (`RING_GROUND_OFFSET_Y` puts the ring at world y = 0.10), well clear of
/// z-fighting.
pub const HEALING_WAVE_UNDERGLOW_LIFT: f32 = 0.10;
/// Butterflies flutter between these heights above the feet (source vertex
/// cloud: z 0.3–1.3).
pub const HEALING_WAVE_BUTTERFLY_MIN_Y: f32 = 0.3;
pub const HEALING_WAVE_BUTTERFLY_MAX_Y: f32 = 1.3;
/// How far a butterfly drifts outward over the effect as it fades, yards.
pub const HEALING_WAVE_OUTWARD_DRIFT: f32 = 0.6;
/// Base orbit rate, radians per second (scaled by the swirl knob).
pub const HEALING_WAVE_SWIRL_RATE: f32 = 2.2;
/// Wing flap rate (full open-close cycles per second) and amplitude.
const BUTTERFLY_FLAP_HZ: f32 = 7.0;
const BUTTERFLY_FLAP_AMPLITUDE: f32 = 0.95;
/// Wing quad dimensions: span away from the body x length along it. The
/// blessed bench renders a butterfly at ~0.2 u full wingspan (6 px sprites at
/// 58 px/u ≈ 0.1 u per wing) — small enough to read as an insect fluttering
/// around a person, not a sheet of paper. Two of these quads side by side
/// give that 0.2 u span.
const BUTTERFLY_WING_SPAN: f32 = 0.10;
const BUTTERFLY_WING_LENGTH: f32 = 0.14;

/// Height of the Base attachment (the recipient's feet) above a combatant's
/// transform. The capsule is `Capsule3d::new(0.5, 1.5)` CENTRED on the
/// transform, spanning -1.25..+1.25 and sunk 0.25 into the ground, so this
/// anchor sits slightly BELOW the floor plane (world y = -0.15 for a
/// combatant at y = 1.0). Rising motes surface within their first frames;
/// anything that must render ON the ground (the under-glow pool) anchors to
/// [`ARENA_FLOOR_WORLD_Y`] instead.
pub const HEAL_BASE_Y: f32 = -1.15;

/// Pure gold — the Holy palette (`star5a`, `yellow_star_dim`, gold ribbons).
fn holy_gold() -> Color {
    Color::srgb(1.0, 0.84, 0.38)
}
/// The dimmer, paler star tint (`yellow_star_dim`).
fn holy_gold_pale() -> Color {
    Color::srgb(1.0, 0.93, 0.62)
}
/// Nature's green (`green_glow3`).
fn nature_green() -> Color {
    Color::srgb(0.45, 0.92, 0.30)
}
/// The butterflies' warm gold-green.
fn butterfly_gold() -> Color {
    Color::srgb(0.85, 0.95, 0.45)
}

fn emissive_of(color: Color, strength: f32) -> LinearRgba {
    let c = color.to_linear();
    LinearRgba::rgb(c.red * strength, c.green * strength, c.blue * strength)
}

/// Where on the recipient a heal landing attaches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealAnchor {
    /// Attachment 19 — the target's origin/feet. Every heal but Holy Light.
    Base,
    /// Attachment 20 — Holy Light's shower falls from here.
    Head,
}

/// A 3-point piecewise-linear parameter over the emit window, keyed at
/// start / midpoint / end — how the source keys its ramped rates and areas.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ramp {
    pub start: f32,
    pub mid: f32,
    pub end: f32,
}

impl Ramp {
    pub const fn flat(v: f32) -> Self {
        Ramp { start: v, mid: v, end: v }
    }

    /// Value at normalized window position `k` in 0..1.
    pub fn at(&self, k: f32) -> f32 {
        let k = k.clamp(0.0, 1.0);
        if k < 0.5 {
            self.start + (self.mid - self.start) * (k / 0.5)
        } else {
            self.mid + (self.end - self.mid) * ((k - 0.5) / 0.5)
        }
    }
}

/// One particle emitter of a landing, transcribed from the M2 record.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HealEmitter {
    /// Rig-local origin (x, y up, z). The source's z-up heights map to y.
    pub origin: Vec3,
    /// Vertical emission speed, u/s. Negative FALLS (Holy Light's curtain).
    pub speed: f32,
    pub life: f32,
    /// Motes per second over the emit window.
    pub rate: Ramp,
    /// Emission square side length over the window.
    pub area: Ramp,
    /// Cone spread, radians; lateral velocity = |speed| * spread.
    pub spread: f32,
    pub kind: HealMoteKind,
    /// Mote radius, yards.
    pub size: f32,
}

/// Flash Heal's opening ray-fan flash.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RayFlash {
    pub rays: u32,
    pub length: f32,
    pub secs: f32,
    pub intensity: f32,
}

/// One of Healing Wave's torso glow layers.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GlowLayer {
    /// Height above the Base anchor.
    pub height: f32,
    pub radius: f32,
    pub color: Color,
    pub alpha: f32,
}

/// One heal landing's full recipe.
#[derive(Clone, PartialEq, Debug)]
pub struct HealStyle {
    pub anchor: HealAnchor,
    /// Seconds the emitters (and glows) run; motes live out their own lives
    /// after it.
    pub emit_secs: f32,
    /// Global multiplier on rates, glow alpha, and emissive — Flash of
    /// Light's 0.55 borrow, 1.0 everywhere else.
    pub intensity: f32,
    pub flash: Option<RayFlash>,
    /// `(radius, alpha)` of Holy Light's head bloom.
    pub head_glow: Option<(f32, f32)>,
    /// `(radius, alpha)` of Healing Wave's green pool at the feet — a flat
    /// ground quad, never billboarded.
    pub under_glow: Option<(f32, f32)>,
    pub torso_glows: Vec<GlowLayer>,
    pub butterflies: bool,
    pub emitters: Vec<HealEmitter>,
}

impl HealStyle {
    /// How long the whole landing plays: the emit window plus the longest
    /// mote life (never shorter than the flash).
    pub fn life(&self) -> f32 {
        let tail = self
            .emitters
            .iter()
            .map(|e| e.life)
            .fold(0.0_f32, f32::max);
        let mut life = self.emit_secs + tail;
        if let Some(flash) = &self.flash {
            life = life.max(flash.secs);
        }
        life
    }
}

/// The per-spell recipe table. Every constant here is either a blessed
/// default or a transcribed M2 emitter record — see the module header.
pub fn heal_style(kind: HealImpactKind) -> HealStyle {
    match kind {
        HealImpactKind::FlashHeal => HealStyle {
            anchor: HealAnchor::Base,
            emit_secs: FLASH_HEAL_EMIT_SECS,
            intensity: 1.0,
            flash: Some(RayFlash {
                rays: FLASH_HEAL_RAY_COUNT,
                length: FLASH_HEAL_RAY_LENGTH,
                secs: FLASH_HEAL_FLASH_DURATION,
                intensity: FLASH_HEAL_FLASH_INTENSITY,
            }),
            head_glow: None,
            under_glow: None,
            torso_glows: Vec::new(),
            butterflies: false,
            // Mote SIZES here and in HealStream are render tuning, not M2
            // transcription (the source records carry no world-size), and the
            // 0.07–0.09 first cut was near-invisible on a 2.5 u body — raised
            // ~35% so the rising stream actually reads. Rates/speeds/lives/
            // areas/origins are the transcribed constants and stay untouched.
            emitters: vec![
                // Two star5a emitters, scattered off-centre.
                HealEmitter {
                    origin: Vec3::new(-0.45, 0.0, -0.30),
                    speed: 3.33,
                    life: 1.0,
                    rate: Ramp::flat(5.0),
                    area: Ramp::flat(0.14),
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.12,
                },
                HealEmitter {
                    origin: Vec3::new(0.35, 0.0, 0.20),
                    speed: 3.33,
                    life: 1.0,
                    rate: Ramp::flat(5.0),
                    area: Ramp::flat(0.14),
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.12,
                },
                // The slow dim-star spread.
                HealEmitter {
                    origin: Vec3::new(0.0, 0.47, 0.0),
                    speed: 1.67,
                    life: 1.25,
                    rate: Ramp::flat(13.0),
                    area: Ramp::flat(0.56),
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.11,
                },
                // Two gold ribbon emitters, rate and area keyed.
                HealEmitter {
                    origin: Vec3::new(0.0, 0.63, 0.0),
                    speed: 2.22,
                    life: 0.9,
                    rate: Ramp { start: 11.0, mid: 15.0, end: 0.0 },
                    area: Ramp { start: 0.14, mid: 0.69, end: 0.14 },
                    spread: 0.0,
                    kind: HealMoteKind::Ribbon,
                    size: 0.09,
                },
                HealEmitter {
                    origin: Vec3::new(0.0, 1.35, 0.0),
                    speed: 2.22,
                    life: 0.9,
                    rate: Ramp { start: 11.0, mid: 15.0, end: 0.0 },
                    area: Ramp { start: 0.14, mid: 0.69, end: 0.14 },
                    spread: 0.0,
                    kind: HealMoteKind::Ribbon,
                    size: 0.09,
                },
            ],
        },
        HealImpactKind::HealStream => HealStyle {
            anchor: HealAnchor::Base,
            emit_secs: HEAL_STREAM_ACTIVE_SECS,
            intensity: 1.0,
            flash: None,
            head_glow: None,
            under_glow: None,
            torso_glows: Vec::new(),
            butterflies: false,
            // Mote sizes raised with Flash Heal's — see the note there.
            emitters: vec![
                HealEmitter {
                    origin: Vec3::new(0.0, 1.35, 0.0),
                    speed: 2.22,
                    life: 0.9,
                    rate: Ramp { start: 15.0, mid: 20.0, end: 5.0 },
                    area: Ramp { start: 0.14, mid: 0.69, end: 0.14 },
                    spread: 0.0,
                    kind: HealMoteKind::Ribbon,
                    size: 0.09,
                },
                HealEmitter {
                    origin: Vec3::new(-0.28, 0.0, 0.0),
                    speed: 3.33,
                    life: 1.0,
                    rate: Ramp::flat(10.0),
                    area: Ramp::flat(HEAL_STREAM_WIDTH),
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.12,
                },
                HealEmitter {
                    origin: Vec3::new(0.28, 0.0, 0.0),
                    speed: 3.33,
                    life: 1.0,
                    rate: Ramp::flat(10.0),
                    area: Ramp::flat(HEAL_STREAM_WIDTH),
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.12,
                },
                HealEmitter {
                    origin: Vec3::new(0.0, 0.47, 0.0),
                    speed: 1.67,
                    life: 1.25,
                    rate: Ramp::flat(16.0),
                    area: Ramp::flat(HEAL_STREAM_WIDTH),
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.11,
                },
            ],
        },
        HealImpactKind::HolyLight => holy_light_style(HOLY_LIGHT_DURATION, 1.0),
        HealImpactKind::FlashOfLight => {
            holy_light_style(FLASH_OF_LIGHT_DURATION, FLASH_OF_LIGHT_BORROW_INTENSITY)
        }
        HealImpactKind::HealingWave => HealStyle {
            anchor: HealAnchor::Base,
            emit_secs: HEALING_WAVE_DURATION,
            intensity: 1.0,
            flash: None,
            head_glow: None,
            under_glow: Some((
                HEALING_WAVE_UNDERGLOW_RADIUS,
                HEALING_WAVE_UNDERGLOW_ALPHA * HEALING_WAVE_GLOW_INTENSITY,
            )),
            // Wrap halos AROUND the torso, layered green under gold under
            // pale gold at the blessed heights. The source glow layers wrap
            // the body at radial extent 1.2–1.7 u — never inside it — and in
            // 3D the geometry enforces the same: a quad billboarded on the
            // spine renders behind the capsule's front surface out to
            // COMBATANT_BODY_RADIUS, so every radius here must clear 0.5 with
            // margin or the body swallows the glow whole (the third build's
            // 0.50/0.38/0.28 defect). The occluded centre is a feature — only
            // the soft annulus around the silhouette reads, so these can't
            // saturate into the first build's solid 2.3 u disc either.
            //
            // Floor clipping is a DELIBERATE partial accept. The floor plane
            // (ARENA_FLOOR_WORLD_Y) sits at combatant-local -1.00 — 0.15
            // ABOVE this rig's Base anchor — so the layer centres stand
            // 0.34 / 0.55 / 0.68 above the floor while their radii are
            // 0.95 / 0.80 / 0.70: the green layer's bottom ~32% and the
            // gold's ~16% render below the floor and are depth-clipped.
            // That is the intended "rising from the ground" read — the wrap
            // emerging out of the green pool at the feet — and each layer's
            // visible portion stays substantial: ≥ ~0.65 of its vertical
            // extent above the floor, and the whole annulus outside the
            // 0.5-yd body silhouette. Pinned (visible-fraction floor of 0.6)
            // by `healing_wave_glow_wraps_outside_the_body`; raising or
            // shrinking the layers to dodge the clip entirely would pull
            // them off the blessed spine heights and radial extents.
            torso_glows: vec![
                GlowLayer {
                    height: 0.49,
                    radius: 0.95,
                    color: nature_green(),
                    alpha: 0.50 * HEALING_WAVE_GLOW_INTENSITY,
                },
                GlowLayer {
                    height: 0.70,
                    radius: 0.80,
                    color: holy_gold(),
                    alpha: 0.35 * HEALING_WAVE_GLOW_INTENSITY,
                },
                GlowLayer {
                    height: 0.83,
                    radius: 0.70,
                    color: holy_gold_pale(),
                    alpha: 0.30 * HEALING_WAVE_GLOW_INTENSITY,
                },
            ],
            butterflies: true,
            emitters: vec![
                // The above-head burst star emitter.
                HealEmitter {
                    origin: Vec3::new(0.0, 2.66, 0.0),
                    speed: 2.78,
                    life: 0.7,
                    rate: Ramp { start: 17.85, mid: 35.25, end: 13.25 },
                    area: Ramp { start: 0.2, mid: 1.03, end: 0.2 },
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.08,
                },
                // The steady low dusting.
                HealEmitter {
                    origin: Vec3::new(0.0, 0.60, 0.0),
                    speed: 2.78,
                    life: 1.0,
                    rate: Ramp::flat(23.0),
                    area: Ramp::flat(0.67),
                    spread: 0.0,
                    kind: HealMoteKind::Star,
                    size: 0.08,
                },
            ],
        },
    }
}

/// Holy Light's recipe, shared with Flash of Light's scaled borrow. Twelve
/// source emitters collapse into seven origin bands staggered down from the
/// head, with emission areas widening as the origins descend (the widening
/// curtain), stars up top and soft light-puffs below.
fn holy_light_style(duration: f32, intensity: f32) -> HealStyle {
    // (y offset from head, emission area); the band's 0..1 top-to-bottom
    // position is derived from its index below.
    const BANDS: [(f32, f32); 7] = [
        (1.05, 0.0),
        (0.97, 0.25),
        (0.74, 0.5),
        (0.38, 0.75),
        (-0.05, 1.0),
        (-0.53, 1.25),
        (-1.09, 1.44),
    ];
    let emitters = BANDS
        .iter()
        .enumerate()
        .map(|(i, &(y, area))| {
            let t = i as f32 / (BANDS.len() - 1) as f32;
            HealEmitter {
                origin: Vec3::new(0.0, y, 0.0),
                // Fall speeds -0.56..-0.83, faster toward the bottom bands.
                speed: -(0.56 + 0.27 * t),
                life: 1.5,
                rate: Ramp::flat(10.0),
                area: Ramp::flat(area),
                spread: HOLY_LIGHT_CONE_SPREAD,
                // Stars on the upper bands, cloud puffs on the lower.
                kind: if y >= 0.3 { HealMoteKind::Star } else { HealMoteKind::Puff },
                size: if y >= 0.3 { 0.09 } else { 0.26 },
            }
        })
        .collect();
    HealStyle {
        anchor: HealAnchor::Head,
        emit_secs: duration,
        intensity,
        flash: None,
        head_glow: Some((
            HOLY_LIGHT_HEAD_GLOW_RADIUS,
            0.8 * HOLY_LIGHT_HEAD_GLOW_INTENSITY,
        )),
        under_glow: None,
        torso_glows: Vec::new(),
        butterflies: false,
        emitters,
    }
}

/// Where a heal landing plays, given its recipient's transform.
///
/// Anchors are measured from the capsule CENTRE, where a combatant's
/// transform sits. A pet's body hangs below its transform at about half the
/// stature — the same correction `school_impact.rs` applies.
pub fn heal_origin(anchor: HealAnchor, translation: Vec3, is_pet: bool) -> Vec3 {
    let height = match anchor {
        HealAnchor::Base => HEAL_BASE_Y,
        HealAnchor::Head => IMPACT_HEAD_Y,
    };
    let y = if is_pet {
        IMPACT_PET_BODY_Y + height * IMPACT_PET_STATURE
    } else {
        height
    };
    translation + Vec3::Y * y
}

/// Alpha envelope of a landing's sustained pieces (glows, butterflies) over
/// the normalized emit window: a fast bloom in, a long fade out.
pub fn heal_envelope(k: f32) -> f32 {
    if !(0.0..1.0).contains(&k) {
        return 0.0;
    }
    let rise = (k / 0.12).clamp(0.0, 1.0);
    let fall = ((1.0 - k) / 0.45).clamp(0.0, 1.0);
    rise.min(fall)
}

/// Rig-local centre of the `index`th butterfly at `age` seconds into the
/// landing: an orbit at [`HEALING_WAVE_ORBIT_RADIUS`] around the recipient,
/// drifting outward as the effect fades, each butterfly at its own height in
/// the torso band with a small vertical flutter-bob. Pure so the swirl
/// geometry can be probed.
pub fn butterfly_center(index: u32, age: f32) -> Vec3 {
    let n = HEALING_WAVE_BUTTERFLIES.max(1);
    let t = index as f32 / (n - 1).max(1) as f32;
    let height = HEALING_WAVE_BUTTERFLY_MIN_Y
        + t * (HEALING_WAVE_BUTTERFLY_MAX_Y - HEALING_WAVE_BUTTERFLY_MIN_Y);
    let phase = index as f32 * TAU / n as f32;
    let angle = phase + HEALING_WAVE_SWIRL_RATE * HEALING_WAVE_SWIRL_SPEED_SCALE * age;
    let k = (age / HEALING_WAVE_DURATION).clamp(0.0, 1.0);
    let radius = HEALING_WAVE_ORBIT_RADIUS + HEALING_WAVE_OUTWARD_DRIFT * k;
    let bob = 0.08 * (age * 5.0 + phase).sin();
    Vec3::new(angle.cos() * radius, height + bob, angle.sin() * radius)
}

/// Yaw of the `index`th butterfly at `age`: facing along its orbit tangent.
pub fn butterfly_heading(index: u32, age: f32) -> Quat {
    let n = HEALING_WAVE_BUTTERFLIES.max(1);
    let phase = index as f32 * TAU / n as f32;
    let angle = phase + HEALING_WAVE_SWIRL_RATE * HEALING_WAVE_SWIRL_SPEED_SCALE * age;
    // Orbit position advances counterclockwise in the XZ plane; the tangent
    // of (cos a, sin a) is (-sin a, cos a).
    let dir = Vec3::new(-angle.sin(), 0.0, angle.cos());
    Quat::from_rotation_y(dir.x.atan2(dir.z))
}

/// Cheap deterministic jitter in [0, 1). Visual only — never `game_rng`.
fn heal_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

/// Meshes and sprites every heal landing shares. Built once, lazily.
pub struct HealAssets {
    quad: Handle<Mesh>,
    star: Handle<Image>,
    dot: Handle<Image>,
}

impl HealAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            star: images.add(star_flash_texture()),
            dot: images.add(soft_dot_texture()),
        }
    }
}

/// Build the landing on a new [`HealImpact`].
///
/// The rig is posed at the recipient's anchor (feet for everything but Holy
/// Light's head shower), follows the recipient, and emits its motes from
/// [`animate_heal_impacts`]. Every piece is a child, so the whole thing dies
/// with the rig.
pub fn spawn_heal_impacts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HealAssets>>,
    new_impacts: Query<(Entity, &HealImpact), Added<HealImpact>>,
    targets: Query<(&Transform, Option<&Pet>)>,
) {
    if new_impacts.is_empty() {
        return;
    }
    let assets = assets.get_or_insert_with(|| HealAssets::build(&mut meshes, &mut images));

    for (entity, impact) in new_impacts.iter() {
        let style = heal_style(impact.kind);
        let at = targets
            .get(impact.target)
            .map(|(t, pet)| heal_origin(style.anchor, t.translation, pet.is_some()))
            .unwrap_or(Vec3::ZERO);

        let glow = |materials: &mut Assets<StandardMaterial>,
                    color: Color,
                    strength: f32,
                    texture: Option<Handle<Image>>| {
            materials.add(StandardMaterial {
                base_color: color,
                base_color_texture: texture.clone(),
                emissive: emissive_of(color, strength * style.intensity),
                emissive_texture: texture,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        };

        let mut parts: Vec<Entity> = Vec::new();

        if let Some(flash) = &style.flash {
            // The central lens flare.
            parts.push(
                commands
                    .spawn((
                        HealSprite {
                            role: HealSpriteRole::LensFlare,
                            radius: FLASH_HEAL_FLARE_RADIUS,
                            base_alpha: flash.intensity,
                        },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(glow(
                            &mut materials,
                            holy_gold_pale(),
                            2.8,
                            Some(assets.star.clone()),
                        )),
                        Transform::default(),
                        NotShadowCaster,
                    ))
                    .id(),
            );
            // The 8 radiating gradient light rays.
            for i in 0..flash.rays {
                let angle = i as f32 * TAU / flash.rays as f32;
                parts.push(
                    commands
                        .spawn((
                            HealSprite {
                                role: HealSpriteRole::Ray { angle },
                                radius: flash.length,
                                base_alpha: flash.intensity,
                            },
                            Mesh3d(assets.quad.clone()),
                            MeshMaterial3d(glow(
                                &mut materials,
                                holy_gold(),
                                2.4,
                                Some(assets.dot.clone()),
                            )),
                            Transform::from_scale(Vec3::ZERO),
                            NotShadowCaster,
                        ))
                        .id(),
                );
            }
        }

        if let Some((radius, alpha)) = style.head_glow {
            parts.push(
                commands
                    .spawn((
                        HealSprite {
                            role: HealSpriteRole::HeadGlow,
                            radius,
                            base_alpha: alpha,
                        },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(glow(
                            &mut materials,
                            holy_gold_pale(),
                            2.6,
                            Some(assets.dot.clone()),
                        )),
                        Transform::default(),
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }

        if let Some((radius, alpha)) = style.under_glow {
            // A flat pool of green light ON the arena floor under the feet.
            // Laid into the XZ plane at spawn and never billboarded — the
            // ground is its plane. Its local height is derived from the rig's
            // world anchor so the rendered quad lands at ARENA_FLOOR_WORLD_Y
            // + LIFT whatever the anchor's own height (the Base anchor sits
            // BELOW the floor plane, and a pet's anchor at yet another
            // height): a pool anchored below the opaque depth-writing floor
            // is fully depth-rejected and never renders. Derived once at
            // spawn — a recipient's sim y never changes mid-match.
            let pool_local_y = ARENA_FLOOR_WORLD_Y + HEALING_WAVE_UNDERGLOW_LIFT - at.y;
            parts.push(
                commands
                    .spawn((
                        HealSprite {
                            role: HealSpriteRole::UnderGlow,
                            radius,
                            base_alpha: alpha,
                        },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(glow(
                            &mut materials,
                            nature_green(),
                            2.0,
                            Some(assets.dot.clone()),
                        )),
                        Transform::from_translation(Vec3::Y * pool_local_y)
                            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }

        for layer in &style.torso_glows {
            parts.push(
                commands
                    .spawn((
                        HealSprite {
                            role: HealSpriteRole::TorsoGlow,
                            radius: layer.radius,
                            base_alpha: layer.alpha,
                        },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(glow(
                            &mut materials,
                            layer.color,
                            2.0,
                            Some(assets.dot.clone()),
                        )),
                        Transform::from_translation(Vec3::Y * layer.height),
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }

        if style.butterflies {
            let wing_mesh = meshes.add(Rectangle::new(BUTTERFLY_WING_SPAN, BUTTERFLY_WING_LENGTH));
            let wing_material = glow(&mut materials, butterfly_gold(), 1.8, None);
            for index in 0..HEALING_WAVE_BUTTERFLIES {
                for side in [-1.0_f32, 1.0] {
                    parts.push(
                        commands
                            .spawn((
                                HealButterflyWing { index, side },
                                Mesh3d(wing_mesh.clone()),
                                MeshMaterial3d(wing_material.clone()),
                                Transform::from_translation(butterfly_center(index, 0.0)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
            }
        }

        // One material per mote kind for the whole landing; motes fade by
        // shrinking, so nothing per-piece has to be written.
        let star_material = glow(&mut materials, holy_gold(), 2.4, Some(assets.star.clone()));
        let ribbon_material = glow(&mut materials, holy_gold(), 2.2, Some(assets.dot.clone()));
        let puff_material = glow(
            &mut materials,
            holy_gold_pale().with_alpha(0.55),
            1.2,
            Some(assets.dot.clone()),
        );

        let carry = [0.0; 8];
        debug_assert!(
            style.emitters.len() <= carry.len(),
            "{:?} declares {} emitters but the rig carries {} — a further \
             emitter would silently never emit; grow HealImpactRig::carry",
            impact.kind,
            style.emitters.len(),
            carry.len(),
        );
        commands.entity(entity).try_insert((
            Transform::from_translation(at),
            Visibility::default(),
            HealImpactRig {
                carry,
                emitted: 0,
                quad: assets.quad.clone(),
                star_material,
                ribbon_material,
                puff_material,
            },
        ));
        commands.entity(entity).add_children(&parts);
    }
}

/// Drive every live heal landing, and retire it when it is spent.
pub fn animate_heal_impacts(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // `Option<&Children>`: a Heal-stream rig spawns with NO static pieces —
    // its first child arrives with the first emitted mote — and a plain
    // `&Children` query would skip it forever.
    mut impacts: Query<(
        Entity,
        &mut HealImpact,
        &mut HealImpactRig,
        &mut Transform,
        Option<&Children>,
    )>,
    // Read-only recipient lookup, provably disjoint from the mutable part
    // queries below or Bevy rejects the set as B0001.
    targets: Query<
        (&Transform, Option<&Pet>),
        (
            With<Combatant>,
            Without<HealImpact>,
            Without<HealSprite>,
            Without<HealMote>,
            Without<HealButterflyWing>,
        ),
    >,
    mut sprites: Query<
        (
            &HealSprite,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<HealImpact>,
            Without<HealMote>,
            Without<HealButterflyWing>,
        ),
    >,
    mut motes: Query<
        (&mut HealMote, &mut Transform),
        (
            Without<HealImpact>,
            Without<HealSprite>,
            Without<HealButterflyWing>,
        ),
    >,
    mut wings: Query<
        (
            &HealButterflyWing,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<HealImpact>, Without<HealSprite>, Without<HealMote>),
    >,
) {
    let dt = time.delta_secs();

    for (entity, mut impact, mut rig, mut transform, children) in impacts.iter_mut() {
        impact.age += dt;
        let age = impact.age;
        let style = heal_style(impact.kind);
        if age >= style.life() {
            commands.entity(entity).despawn();
            continue;
        }
        // Attached: follow a recipient that is still moving.
        if let Ok((target, pet)) = targets.get(impact.target) {
            transform.translation = heal_origin(style.anchor, target.translation, pet.is_some());
        }

        // Emit motes while the window is open.
        if age < style.emit_secs {
            let k = age / style.emit_secs;
            for (ei, emitter) in style.emitters.iter().enumerate().take(rig.carry.len()) {
                rig.carry[ei] += emitter.rate.at(k) * style.intensity * HEAL_MOTE_RATE_SCALE * dt;
                while rig.carry[ei] >= 1.0 {
                    rig.carry[ei] -= 1.0;
                    let i = rig.emitted;
                    rig.emitted = rig.emitted.wrapping_add(1);
                    let seed = entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
                    let area = emitter.area.at(k);
                    let local = emitter.origin
                        + Vec3::new(
                            (heal_jitter(seed) - 0.5) * area,
                            0.0,
                            (heal_jitter(seed ^ 0x51ED) - 0.5) * area,
                        );
                    let mut velocity = Vec3::Y * emitter.speed * HEAL_MOTE_SPEED_SCALE;
                    if emitter.spread > 0.0 {
                        let a = heal_jitter(seed ^ 0x27D4) * TAU;
                        let m = emitter.speed.abs() * emitter.spread * heal_jitter(seed ^ 0x9E37);
                        velocity += Vec3::new(a.cos() * m, 0.0, a.sin() * m);
                    }
                    let material = match emitter.kind {
                        HealMoteKind::Star => rig.star_material.clone(),
                        HealMoteKind::Ribbon => rig.ribbon_material.clone(),
                        HealMoteKind::Puff => rig.puff_material.clone(),
                    };
                    let mote = commands
                        .spawn((
                            HealMote {
                                kind: emitter.kind,
                                velocity,
                                age: 0.0,
                                life: emitter.life,
                                radius: emitter.size,
                            },
                            Mesh3d(rig.quad.clone()),
                            MeshMaterial3d(material),
                            Transform::from_translation(local).with_scale(Vec3::ZERO),
                            NotShadowCaster,
                        ))
                        .id();
                    commands.entity(entity).add_child(mote);
                }
            }
        }

        let window_k = (age / style.emit_secs).clamp(0.0, 1.0);
        let envelope = heal_envelope(window_k) * style.intensity;

        let Some(children) = children else {
            continue;
        };
        for child in children.iter() {
            if let Ok((sprite, mut part, material)) = sprites.get_mut(child) {
                let (scale, alpha) = match sprite.role {
                    HealSpriteRole::Ray { .. } => {
                        let secs = style.flash.as_ref().map(|f| f.secs).unwrap_or(0.0);
                        let k = if secs > 0.0 { (age / secs).clamp(0.0, 1.0) } else { 1.0 };
                        if age > secs {
                            (Vec3::ZERO, 0.0)
                        } else {
                            // Snaps open, then thins and dies.
                            let open = (k / 0.25).clamp(0.0, 1.0);
                            (
                                Vec3::new(
                                    FLASH_HEAL_RAY_WIDTH * (1.0 - 0.6 * k),
                                    sprite.radius * (0.35 + 0.65 * open),
                                    1.0,
                                ),
                                (1.0 - k) * sprite.base_alpha * style.intensity,
                            )
                        }
                    }
                    HealSpriteRole::LensFlare => {
                        let secs = style.flash.as_ref().map(|f| f.secs).unwrap_or(0.0);
                        let k = if secs > 0.0 { (age / secs).clamp(0.0, 1.0) } else { 1.0 };
                        if age > secs {
                            (Vec3::ZERO, 0.0)
                        } else {
                            let open = (k / 0.25).clamp(0.0, 1.0);
                            (
                                Vec3::splat(
                                    sprite.radius * 2.0 * (0.4 + 0.6 * open) * (1.0 - 0.5 * k),
                                ),
                                (1.0 - k) * sprite.base_alpha * style.intensity,
                            )
                        }
                    }
                    HealSpriteRole::HeadGlow => {
                        // Bloom in with the envelope, gone with the window.
                        let bloom = 0.5 + 0.5 * envelope;
                        (
                            Vec3::splat((sprite.radius * 2.0 * bloom).max(1e-4)),
                            sprite.base_alpha * envelope,
                        )
                    }
                    HealSpriteRole::TorsoGlow | HealSpriteRole::UnderGlow => {
                        // The wrap and the pool breathe rather than bloom: a
                        // deep bloom would drag the wrap's drawn radius back
                        // under COMBATANT_BODY_RADIUS and bury it in the body
                        // for part of its life.
                        let bloom = 0.8 + 0.2 * envelope;
                        (
                            Vec3::splat((sprite.radius * 2.0 * bloom).max(1e-4)),
                            sprite.base_alpha * envelope,
                        )
                    }
                };
                part.scale = scale.max(Vec3::splat(1e-4));
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(alpha);
                }
            }

            if let Ok((mut mote, mut part)) = motes.get_mut(child) {
                mote.age += dt;
                if mote.age > mote.life {
                    part.scale = Vec3::ZERO;
                    continue;
                }
                part.translation += mote.velocity * dt;
                let k = 1.0 - mote.age / mote.life;
                match mote.kind {
                    HealMoteKind::Star => {
                        part.scale = Vec3::splat((mote.radius * 2.0 * k.powf(0.6)).max(1e-4));
                    }
                    HealMoteKind::Ribbon => {
                        // A vertically stretched streak — the ribbon-blur read.
                        part.scale = Vec3::new(
                            (mote.radius * 0.8 * k.powf(0.6)).max(1e-4),
                            (mote.radius * 4.5 * k.powf(0.4)).max(1e-4),
                            1.0,
                        );
                    }
                    HealMoteKind::Puff => {
                        // A soft cloud: swells as it dies.
                        let swell = 1.0 + 0.6 * (1.0 - k);
                        part.scale =
                            Vec3::splat((mote.radius * 2.0 * swell * k.powf(0.4)).max(1e-4));
                    }
                }
            }

            if let Ok((wing, mut part, material)) = wings.get_mut(child) {
                let center = butterfly_center(wing.index, age);
                let heading = butterfly_heading(wing.index, age);
                let flap = BUTTERFLY_FLAP_AMPLITUDE
                    * (age * BUTTERFLY_FLAP_HZ * TAU
                        + wing.index as f32 * TAU / HEALING_WAVE_BUTTERFLIES.max(1) as f32)
                        .sin();
                let fold = Quat::from_rotation_z(wing.side * flap);
                // The wing quad lies flat (normal up), hinged at the body,
                // extending sideways; the flap folds it about the body axis.
                part.rotation = heading * fold * Quat::from_rotation_x(-FRAC_PI_2);
                part.translation =
                    center + heading * (fold * Vec3::new(wing.side * BUTTERFLY_WING_SPAN * 0.5, 0.0, 0.0));
                part.scale = Vec3::ONE;
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(0.9 * envelope);
                }
            }
        }
    }
}

/// Turn the flat pieces of a heal landing to face the camera.
///
/// The lens flare, glows and motes are flat quads; the rays keep their own
/// roll about the view axis so the fan stays a fan. Butterfly wings are NOT
/// billboarded — their 3D flutter is the design — and neither is the feet
/// under-glow, which lies in the ground plane.
pub fn billboard_heal_impacts(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<HealImpact>,
            Without<HealSprite>,
            Without<HealMote>,
        ),
    >,
    rigs: Query<(&Transform, &Children), With<HealImpact>>,
    mut sprites: Query<
        (&HealSprite, &mut Transform),
        (Without<HealImpact>, Without<Camera3d>, Without<HealMote>),
    >,
    mut motes: Query<
        (&HealMote, &mut Transform),
        (Without<HealImpact>, Without<Camera3d>, Without<HealSprite>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let facing = rig.rotation.inverse() * cam.rotation;
        for child in children.iter() {
            if let Ok((sprite, mut part)) = sprites.get_mut(child) {
                match sprite.role {
                    HealSpriteRole::Ray { angle } => {
                        let roll = facing * Quat::from_rotation_z(angle);
                        part.rotation = roll;
                        // Extend outward from the flash centre along the
                        // ray's own axis in the billboard plane.
                        part.translation = roll * Vec3::Y * (part.scale.y * 0.5);
                    }
                    // The under-glow is a pool ON the ground — the ground is
                    // its plane, so it keeps its spawn-time flat rotation.
                    HealSpriteRole::UnderGlow => {}
                    _ => {
                        part.rotation = facing;
                    }
                }
            }
            if let Ok((_, mut part)) = motes.get_mut(child) {
                part.rotation = facing;
            }
        }
    }
}
