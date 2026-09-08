use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::TAU;

use super::heal_impact::COMBATANT_BODY_RADIUS;
use super::school_impact::{IMPACT_HEAD_Y, IMPACT_PET_BODY_Y, IMPACT_PET_STATURE};
use super::spell_bolts::{soft_dot_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Warlock DoT aura visuals — Corruption / Curse of Agony / Unstable Affliction
// ==============================================================================
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
// - **Curse of Agony** (kit 884, Head): apply-only — a red-shell,
//   yellow-core skull apparition above the victim's head, flickering on the
//   433/267 ms global cycles, crackling with red-orange star sparks and
//   shedding glow motes downward, gone by `COA_APPARITION_SECS`. NOTHING for
//   the remaining curse duration (`COA_SUSTAIN_WHISPER` = false, era-faithful
//   and deliberate — the 1.15 client sells "cursed" entirely through the
//   apply flourish and the debuff icon).
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

// --- Curse of Agony (kit 884) ------------------------------------------------

/// Blessed master scale of the skull apparition (client bbox ≈ 0.85 u tall
/// for the skull proper).
pub const COA_SKULL_SIZE: f32 = 0.85;
/// The apparition is gone by here (client transparency track: fade-out ends
/// ~2.8 s into the 3000 ms one-shot).
pub const COA_APPARITION_SECS: f32 = 2.8;
/// Era-faithful and DELIBERATE: the client has no (7,8) aura-state row for
/// Curse of Agony — a cursed victim shows NOTHING after the apply skull, for
/// the whole remaining curse. Kept false on purpose; flipping it is a design
/// decision, not a bug fix.
pub const COA_SUSTAIN_WHISPER: bool = false;
/// Blessed master scale on the skull's spark emission.
pub const SPARK_RATE_SCALE: f32 = 1.0;
/// Skull fade-in (client transparency track: 134 ms).
pub const COA_FADE_IN_SECS: f32 = 0.134;
/// Fade-out begins here (client: ~2270 ms).
const COA_FADE_OUT_START: f32 = 2.27;
/// The two global flicker cycles (client global sequences: 433 / 267 ms).
const COA_FLICKER_A: f32 = 0.433;
const COA_FLICKER_B: f32 = 0.267;
/// Star spark emission rate (client red_star2 emitter: 180/s).
const COA_SPARK_RATE: f32 = 180.0;
/// Star spark life (client: 0.75 s).
const COA_SPARK_LIFE: f32 = 0.75;
/// Star spark outward speed (client: 0.83 u/s).
const COA_SPARK_SPEED: f32 = 0.83;
/// Downward glow-mote fall speed (client red_glow3: −1.11 u/s).
const COA_FALL_SPEED: f32 = -1.11;
/// Downward glow-mote rate (client: 20.7/s) and life (1.0 s).
const COA_FALL_RATE: f32 = 20.7;
const COA_FALL_LIFE: f32 = 1.0;
/// Height of the cranium's centre above the head anchor, before
/// `COA_SKULL_SIZE` scaling (client: skull mesh ~0.8 u above attach 20).
const COA_SKULL_LIFT: f32 = 0.75;
/// The red shell and yellow core (client mesh tints: red (0.96,0,0) shells,
/// yellow (1.0,0.96,0) glow quads).
const COA_SHELL_COLOR: Color = Color::srgb(0.96, 0.05, 0.05);
const COA_CORE_COLOR: Color = Color::srgb(1.0, 0.94, 0.15);
/// Spark ramp mid-color (white→orange→red in the client; one shared additive
/// material at the orange mid, faded by shrinking).
const COA_SPARK_COLOR: Color = Color::srgb(0.94, 0.42, 0.14);
/// Emissive strengths of the skull apparition's four material groups.
/// USER-TUNED round-3 ("a touch too bright" verdict, 2026-09-07): the round-2
/// unlit→lit fix woke this rig's formerly-dead emissive (it shipped at
/// shells 2.4 / core+sparks 2.6 / fall motes 1.8), and the lit skull read as
/// a floodlight. Dimmed the whole apparition ~33%, preserving the
/// red-shell < yellow-core ordering and the spark/fall balance — a bright
/// event, not a floodlight. Flicker, fade envelope, and geometry untouched.
const COA_SHELL_EMISSIVE: f32 = 1.6;
const COA_CORE_EMISSIVE: f32 = 1.75;
const COA_SPARK_EMISSIVE: f32 = 1.8;
const COA_FALL_EMISSIVE: f32 = 1.25;

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
pub const COA_AURA: &str = "Curse of Agony";
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

/// Cheap deterministic jitter in [0, 1). Visual only — never `game_rng`.
fn dot_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

/// Does this unit carry the named Warlock DoT? Keys on the exact ability
/// name plus the DoT aura type, so e.g. UA's silence backlash never counts.
pub fn has_warlock_dot(auras: Option<&ActiveAuras>, name: &str) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|au| au.effect_type == AuraType::DamageOverTime && au.ability_name == name)
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

/// The Curse of Agony flicker: the product of the two client global cycles
/// (433 / 267 ms), normalized to 0.55..1.0 so the skull never blinks out.
pub fn coa_flicker(age: f32) -> f32 {
    let a = 0.5 + 0.5 * (age * TAU / COA_FLICKER_A).sin();
    let b = 0.5 + 0.5 * (age * TAU / COA_FLICKER_B).sin();
    0.55 + 0.45 * a * b
}

/// The skull apparition's alpha envelope: 134 ms fade-in, hold, fade-out
/// between 2.27 s and `COA_APPARITION_SECS`.
pub fn coa_envelope(age: f32) -> f32 {
    if !(0.0..COA_APPARITION_SECS).contains(&age) {
        return 0.0;
    }
    let fade_in = (age / COA_FADE_IN_SECS).clamp(0.0, 1.0);
    let fade_out = ((COA_APPARITION_SECS - age) / (COA_APPARITION_SECS - COA_FADE_OUT_START))
        .clamp(0.0, 1.0);
    fade_in.min(fade_out)
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
/// - Curse of Agony newly present → the one-shot skull apparition, latched by
///   the `CoaSkullFired` marker (the skull is apply-only, so a live rig can't
///   be the dedup key). The marker is dropped when the curse leaves, so a
///   re-curse fires a fresh skull.
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
        Option<&CoaSkullFired>,
    )>,
    shrouds: Query<&CorruptionShroudRig>,
    ua_rigs: Query<&UaStateRig>,
) {
    use std::collections::HashSet;
    let shrouded: HashSet<Entity> = shrouds.iter().map(|r| r.target).collect();
    let ua_lit: HashSet<Entity> = ua_rigs.iter().map(|r| r.target).collect();

    for (entity, combatant, transform, auras, pet, coa_mark) in afflicted.iter() {
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

        if has_warlock_dot(auras, COA_AURA) && coa_mark.is_none() {
            spawn_coa_skull(
                &mut commands,
                &mut meshes,
                &mut materials,
                assets,
                entity,
                transform.translation,
                is_pet,
            );
            commands.entity(entity).try_insert(CoaSkullFired);
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

/// Build the Curse of Agony skull apparition above the victim's head: red
/// additive cranium + jaw shells, two dark eye sockets, a yellow core glow —
/// the primitive-mesh transcription of `curseofagony_head.m2` — plus the
/// spark/fall emitters ticked by `animate_coa_skulls`.
fn spawn_coa_skull(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &DotAssets,
    target: Entity,
    at: Vec3,
    is_pet: bool,
) {
    let s = dot_stature(is_pet) * COA_SKULL_SIZE;
    let lift = COA_SKULL_LIFT * s;

    let shell = |materials: &mut Assets<StandardMaterial>| {
        additive_material(materials, COA_SHELL_COLOR, COA_SHELL_EMISSIVE, None)
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
    // The yellow core glow behind the shell (billboarded).
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
                    COA_CORE_COLOR,
                    COA_CORE_EMISSIVE,
                    Some(assets.dot.clone()),
                )),
                Transform::from_translation(Vec3::new(0.0, lift, -0.02 * s)),
                NotShadowCaster,
            ))
            .id(),
    );

    let spark_material = additive_material(
        materials,
        COA_SPARK_COLOR,
        COA_SPARK_EMISSIVE,
        Some(assets.star.clone()),
    );
    let fall_material = additive_material(
        materials,
        Color::srgb(1.0, 0.85, 0.75),
        COA_FALL_EMISSIVE,
        Some(assets.dot.clone()),
    );

    commands
        .spawn((
            CoaSkullRig {
                target,
                age: 0.0,
                spark_carry: 0.0,
                fall_carry: 0.0,
                emitted: 0,
            },
            WarlockDotRigAssets {
                quad: assets.quad.clone(),
                mote_material: spark_material,
                extra_material: fall_material,
                soft_dot: assets.dot.clone(),
            },
            Transform::from_translation(dot_anchor(IMPACT_HEAD_Y, at, is_pet)),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&pieces);
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

/// Drive the Curse of Agony skull: follow the victim's head, run the
/// fade-in/flicker/fade-out envelope over every piece, emit the star sparks
/// and sinking glow motes, and retire the whole apparition at
/// `COA_APPARITION_SECS` — after which the curse shows NOTHING, on purpose
/// (`COA_SUSTAIN_WHISPER`).
pub fn animate_coa_skulls(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(
        Entity,
        &mut CoaSkullRig,
        &WarlockDotRigAssets,
        &mut Transform,
        &Children,
    )>,
    targets: Query<
        (&Transform, Option<&Pet>),
        (With<Combatant>, Without<CoaSkullRig>, Without<DotSprite>),
    >,
    mut pieces: Query<
        (&DotSprite, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<CoaSkullRig>, Without<DotMote>),
    >,
) {
    let dt = time.delta_secs();
    for (entity, mut rig, rig_assets, mut transform, children) in rigs.iter_mut() {
        rig.age += dt;
        let age = rig.age;
        if age >= COA_APPARITION_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        let mut stature = 1.0;
        if let Ok((target, pet)) = targets.get(rig.target) {
            transform.translation = dot_anchor(IMPACT_HEAD_Y, target.translation, pet.is_some());
            stature = dot_stature(pet.is_some());
        }
        let s = stature * COA_SKULL_SIZE;

        let envelope = coa_envelope(age) * coa_flicker(age);
        for child in children.iter() {
            if let Ok((sprite, mut part, material)) = pieces.get_mut(child) {
                let alpha = sprite.base_alpha * envelope;
                match sprite.role {
                    DotSpriteRole::SkullCore => {
                        // The core glow breathes with the flicker (the
                        // client's genericglow2b bloom pulses behind the
                        // shell).
                        let bloom = 0.7 + 0.3 * envelope;
                        part.scale = Vec3::splat((sprite.radius * 2.0 * bloom).max(1e-4));
                    }
                    _ => {}
                }
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(alpha);
                }
            }
        }

        // Red-orange star sparks crackling around the skull.
        rig.spark_carry += COA_SPARK_RATE * SPARK_RATE_SCALE * dt;
        while rig.spark_carry >= 1.0 {
            rig.spark_carry -= 1.0;
            let i = rig.emitted;
            rig.emitted = rig.emitted.wrapping_add(1);
            let seed = entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
            let theta = dot_jitter(seed) * TAU;
            let z = dot_jitter(seed ^ 0x51ED) * 2.0 - 1.0;
            let r = (1.0 - z * z).max(0.0).sqrt();
            let dir = Vec3::new(r * theta.cos(), z, r * theta.sin());
            let origin = Vec3::Y * (COA_SKULL_LIFT * s)
                + Vec3::new(
                    (dot_jitter(seed ^ 0x27D4) - 0.5) * 0.56,
                    (dot_jitter(seed ^ 0x9E37) - 0.5) * 0.66,
                    (dot_jitter(seed ^ 0xC2B2) - 0.5) * 0.56,
                ) * s;
            let mote = commands
                .spawn((
                    DotMote {
                        kind: DotMoteKind::SkullSpark,
                        velocity: dir * COA_SPARK_SPEED,
                        age: 0.0,
                        life: COA_SPARK_LIFE,
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

        // Glow motes sinking down over the victim's face and chest.
        rig.fall_carry += COA_FALL_RATE * SPARK_RATE_SCALE * dt;
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
                        kind: DotMoteKind::SkullFall,
                        velocity: Vec3::Y * COA_FALL_SPEED,
                        age: 0.0,
                        life: COA_FALL_LIFE,
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
        transform.scale = Vec3::splat((ramp3(WISP_SCALE, k) * wisp.stature).max(1e-4));
        if let Some(material) = materials.get_mut(&material.0) {
            let color = ramp_color(WISP_COLOR, k);
            let alpha = ramp3(WISP_ALPHA, k);
            material.base_color = color.with_alpha(alpha);
            material.emissive = emissive_of(color, 1.6 * alpha);
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

/// Yaw each skull apparition to face the camera so its face (eye sockets at
/// local +Z) reads from any bearing. Runs BEFORE
/// [`billboard_warlock_dot_visuals`], whose child facing derives from the
/// rig rotation this writes — a separate system because the billboard pass
/// reads every rig transform and Bevy rejects a same-system mut/read overlap
/// (B0001).
pub fn yaw_coa_skulls(
    camera: Query<&Transform, (With<Camera3d>, Without<CoaSkullRig>)>,
    mut skull_rigs: Query<&mut Transform, (With<CoaSkullRig>, Without<Camera3d>)>,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for mut rig in skull_rigs.iter_mut() {
        let to_cam = cam.translation - rig.translation;
        rig.rotation = Quat::from_rotation_y(to_cam.x.atan2(to_cam.z));
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
                With<CoaSkullRig>,
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
                    // The ring lies flat; shells/spheres are 3D; bolts keep
                    // their kinked poses.
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
/// - The `CoaSkullFired` latch is dropped when the curse is gone, so a fresh
///   curse fires a fresh skull.
/// - One-shot rigs (apply burst, skull) normally play themselves out even if
///   the aura is dispelled mid-flourish — they are apply-moment records, not
///   state — but die with a dead or despawned victim.
pub fn cleanup_warlock_dot_visuals(
    mut commands: Commands,
    shrouds: Query<(Entity, &CorruptionShroudRig)>,
    ua_rigs: Query<(Entity, &UaStateRig)>,
    skulls: Query<(Entity, &CoaSkullRig)>,
    bursts: Query<(Entity, &DotApplyBurst)>,
    marked: Query<(Entity, &Combatant, Option<&ActiveAuras>), With<CoaSkullFired>>,
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
    for (entity, rig) in skulls.iter() {
        if !target_alive(rig.target) {
            commands.entity(entity).despawn();
        }
    }
    for (entity, burst) in bursts.iter() {
        if !target_alive(burst.target) {
            commands.entity(entity).despawn();
        }
    }
    for (entity, combatant, auras) in marked.iter() {
        if !combatant.is_alive() || !has_warlock_dot(auras, COA_AURA) {
            commands.entity(entity).remove::<CoaSkullFired>();
        }
    }
}
