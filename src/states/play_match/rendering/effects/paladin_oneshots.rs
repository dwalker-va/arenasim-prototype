use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

use super::druid_heals::{body_point, keyed, keyed_color};
use super::heal_cast::{flash_instant_hands, HealCastAssets};
use super::hunter_shots::{draw, ClientEmitter, EmitterAssets, EmitterSprite};
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use super::spell_bolts::star_flash_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// Paladin talent one-shots — Divine Favor, Aura Mastery, Blessing of Sacrifice,
// Retribution Aura, Infusion of Light and The Art of War (AS-374)
// ==============================================================================
//
// From the client data joined in
// `docs/design/2026-10-18-paladin-talent-client-data.md` (TBC 2.5.6, and Wrath
// 3.4.5 for the Wrath-form spells): the client draws each of these as a
// ONE-SHOT and nothing standing, and the buff icon carries each buff after.
//
// - **Divine Favor** (§1): Moonfire's arcane hands (`magic_cast_hand.m2`,
//   `heal_cast.rs`), then `innerfocus_impact_chest.m2` at the chest, 0.4 s —
//   Priest Inner Focus's model, so blue, violet and red.
// - **Aura Mastery** (§2): the instant Holy hand flash, then
//   `holyward_impact_chest.m2` on the Paladin ONLY, 3.0 s: a 4.4 yd
//   camera-facing gold glow, gold ripples, a star burst, amber glows and a
//   star ring at 1.0 yd.
// - **Blessing of Sacrifice** (§4): the instant Holy hand flash, then
//   `blessingofsacrifice_impact.m2` at the feet of BOTH the ally and the
//   Paladin, 1.9 s: a red shell, falling red-to-orange motes and faint streaks.
//   No link and nothing per transfer.
// - **Retribution Aura** (§8): `retributionaurared_base.m2` at the Paladin's
//   feet as the aura goes up, 1.43 s: a flat red-orange rune disc whose four
//   arrows grow outward, over a pale-gold underlay. Nothing on the aura's
//   other bearers, and nothing on its hit (`lands_silently_audit`'s
//   silent-by-design entry).
// - **Infusion of Light** (§9): `infusion_of_light_impact.m2` when Holy Shock
//   procs it: a gold ring 2.5 yd up with glows there, chest runes and two flare
//   bursts; every emitter stops by 1.4 s.
// - **The Art of War** (§12): `the_art_of_war_impact.m2` at the chest on the
//   proc, 2.0 s: a small gold disc and a ring of gold sparks opening out.
// - **Light's Grace** (§10) has no client visual at any build: icon only.
//
// Played as the signed-off AS-337 Paladin Talent Visuals Bench plays them
// (https://claude.ai/artifact/31rnhhKpACF51iUsqK5g7k, version 1792379243-e3d6,
// committed as `docs/design/benches/2026-10-18-paladin-talent-bench.html`):
// every client layer kept at the client's values (every knob 1.0), every
// authored layer off — the bench's teammate ward, its Sacrifice pulse and
// tether, and its standing Retribution ring and hit spark are not built. The
// tables below are its emitters, transcribed; the pieces (the glows, rings,
// glyph discs and arrows the bench draws by hand) are its `customDraw` code,
// ported. A bench `scale` key is a sprite RADIUS (`ParticleSize::radii`); a
// hand-drawn sprite's size is its full width.
//
// Spawning rides the sim's `AuraLanding` marker (`apply_pending_auras`, on a
// fresh application or a refresh), which names the aura's caster: Aura
// Mastery's ward and Retribution Aura's disc play only on the landing whose
// bearer IS the caster, and Blessing of Sacrifice's plays at both ends. The
// three casts' hands flash off the same landing (`heal_cast.rs`
// `flash_instant_hands`, `AuraLandingKind::cast_ability`): the class AI
// applies them inline, with no cast state to flash from. Each named aura's
// application is owned by its landing (`AuraApplyRoute::for_instance`), so
// the generic band does not double it.
//
// Graphical-only: scatter is the shared deterministic hash (never
// `game_rng`), nothing here writes sim state, and every system is registered
// in `states/mod.rs` only — headless stays byte-identical. The particles are
// world-space client particles, aged and turned by `animate_client_particles`
// and `billboard_hunter_shots`.

// ── The bench's knobs (signed off at version 1792379243-e3d6) ────────────────

/// Multipliers over the client, as the bench's knobs (1.0 = client). `size`
/// scales every sprite and piece; `spread` speeds, spawn areas and the star
/// ring; `duration` lives, windows and the pieces' clocks; `density` rates;
/// `opacity` every alpha.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OneShotKnobs {
    pub size: f32,
    pub spread: f32,
    pub duration: f32,
    pub density: f32,
    pub opacity: f32,
}

/// The client's own values, which the user signed off for every effect here.
pub const CLIENT_KNOBS: OneShotKnobs = OneShotKnobs {
    size: 1.0,
    spread: 1.0,
    duration: 1.0,
    density: 1.0,
    opacity: 1.0,
};

// Divine Favor (visual 7424): kit 425 magic_cast_hand.m2 @Hands on cast; kit
// 3772 innerfocus_impact_chest.m2 @Chest on the Paladin.
pub const DIVINE_FAVOR_CAST_HANDS: bool = true;
pub const DIVINE_FAVOR_FLARE: bool = true;
pub const DIVINE_FAVOR_STREAKS: bool = true;
pub const DIVINE_FAVOR_POINTS: bool = true;
pub const DIVINE_FAVOR_KNOBS: OneShotKnobs = CLIENT_KNOBS;

// Aura Mastery (Wrath visual 13619): kit 191 holy_precast_high_hand.m2
// @Hands; kit 489 holyward_impact_chest.m2 @Chest, 3.0 s, on the Paladin only.
pub const AURA_MASTERY_HANDS: bool = true;
pub const AURA_MASTERY_GLOW_QUAD: bool = true;
pub const AURA_MASTERY_RIPPLES: bool = true;
pub const AURA_MASTERY_STAR_BURST: bool = true;
pub const AURA_MASTERY_AMBER: bool = true;
pub const AURA_MASTERY_STAR_RING: bool = true;
pub const AURA_MASTERY_KNOBS: OneShotKnobs = CLIENT_KNOBS;

// Blessing of Sacrifice (TBC visual 299): kits 186/189
// holy_precast_med_hand.m2; kit 7815 blessingofsacrifice_impact.m2 @Base on
// caster AND target, 1.9 s.
pub const SACRIFICE_HANDS: bool = true;
pub const SACRIFICE_SHELL: bool = true;
pub const SACRIFICE_MOTES_PALADIN: bool = true;
pub const SACRIFICE_MOTES_ALLY: bool = true;
pub const SACRIFICE_STREAKS: bool = true;
pub const SACRIFICE_KNOBS: OneShotKnobs = CLIENT_KNOBS;

// Retribution Aura (visual 682): kits 753/810 retributionaurared_base.m2
// @Base, 1.434 s, on cast. No state; the damage-shield hit has no visual.
pub const RETRIBUTION_AURA_CAST_DISC: bool = true;
pub const RETRIBUTION_AURA_UNDERLAY: bool = true;
pub const RETRIBUTION_AURA_KNOBS: OneShotKnobs = CLIENT_KNOBS;

// Infusion of Light (Wrath visual 12008): kit 11091
// infusion_of_light_impact.m2 @Base on the proc.
pub const INFUSION_HALO: bool = true;
pub const INFUSION_OVERHEAD_GLOWS: bool = true;
pub const INFUSION_FLASH: bool = true;
pub const INFUSION_CHEST_RUNES: bool = true;
pub const INFUSION_BURSTS: bool = true;
pub const INFUSION_KNOBS: OneShotKnobs = CLIENT_KNOBS;

// The Art of War (Wrath visual 11955): kit 11038 the_art_of_war_impact.m2
// @Chest on the proc, 2.0 s.
pub const ART_OF_WAR_DISC: bool = true;
pub const ART_OF_WAR_SPARK_RING: bool = true;
pub const ART_OF_WAR_GLOWS: bool = true;
pub const ART_OF_WAR_RUNES: bool = true;
pub const ART_OF_WAR_BURST: bool = true;
pub const ART_OF_WAR_KNOBS: OneShotKnobs = CLIENT_KNOBS;

// ── The bench's anchors and sequence lengths ─────────────────────────────────

/// The bench's anchors above the floor (`ANCH`): the chest attachment (34).
pub const ONESHOT_CHEST: f32 = 1.45;

/// Each one-shot's sequence, seconds: the rig lives this long (its particles
/// finish their own lives after it).
pub const DIVINE_FAVOR_SECS: f32 = 0.4;
pub const AURA_MASTERY_SECS: f32 = 3.0;
pub const SACRIFICE_SECS: f32 = 1.9;
pub const RETRIBUTION_AURA_SECS: f32 = 1.434;
/// The model's sequence is 3.83 s, but every emitter has stopped by 1.4 s and
/// the halo is gone at 1.834 s, which is where the bench stops drawing it.
pub const INFUSION_SECS: f32 = 1.834;
pub const ART_OF_WAR_SECS: f32 = 2.0;

// ── The emitter model ────────────────────────────────────────────────────────

/// How a layer's particles launch (the bench's `vRange`, and the M2 ring
/// emitter).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OneShotLaunch {
    /// A uniformly random direction (`vRange` 3.14).
    Sphere,
    /// Straight up at `speed × (1 ± speed_var)` (`vRange` 0, the bench's
    /// `speedVar`); a negative speed falls straight down.
    Vertical { speed_var: f32 },
    /// Born on one of three points of a ring of `radius` yards round the
    /// anchor, the three turning `rev_per_sec` turns a second, then launched
    /// in a uniformly random direction (the bench's `orbit`, an M2 type-3
    /// emitter).
    Ring { radius: f32, rev_per_sec: f32 },
}

/// One emitter of a one-shot: its record as a [`ClientEmitter`] (whose
/// `window`, or `track`, is its emission window from the landing; whose
/// `column` is the bench's `(area, areaY)` spawn box), where it is born, how it
/// launches, and its drag.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OneShotLayer {
    pub on: bool,
    pub emitter: ClientEmitter,
    /// Where the particles are born, yards above the floor (the bench's
    /// anchor plus its `pos` height).
    pub height: f32,
    pub launch: OneShotLaunch,
    /// The velocity's decay per second — the bench integrates `drag` as
    /// `v × (1 − 3 × drag × dt)` a frame, so this is `3 × drag`.
    pub drag: f32,
}

impl OneShotLayer {
    /// Particles this layer owes between `t0` and `t1` seconds after the
    /// landing.
    pub fn count_between(&self, t0: f32, t1: f32, knobs: OneShotKnobs) -> f32 {
        if !self.on {
            return 0.0;
        }
        self.emitter.count_between(t0, t1) * knobs.density
    }
}

/// A layer at an effect's knobs.
const fn at_knobs(layer: OneShotLayer, k: OneShotKnobs) -> OneShotLayer {
    let mut e = layer.emitter;
    e.size = e.size.scaled(k.size);
    if let Some((w, h)) = e.column {
        e.column = Some((w * k.spread, h * k.spread));
    }
    e.speed *= k.spread;
    if let Some((s, t)) = e.window {
        e.window = Some((s * k.duration, t * k.duration));
    }
    e.life *= k.duration;
    e.alpha = [
        e.alpha[0] * k.opacity,
        e.alpha[1] * k.opacity,
        e.alpha[2] * k.opacity,
    ];
    let launch = match layer.launch {
        OneShotLaunch::Ring {
            radius,
            rev_per_sec,
        } => OneShotLaunch::Ring {
            radius: radius * k.spread,
            rev_per_sec,
        },
        other => other,
    };
    OneShotLayer {
        emitter: e,
        launch,
        ..layer
    }
}

/// A [`ClientEmitter`] with the fields every table here shares: additive
/// (every layer is M2 blend 4), no gravity, no rise.
#[allow(clippy::too_many_arguments)]
const fn emitter(
    name: &'static str,
    sprite: EmitterSprite,
    rate: f32,
    window: Option<(f32, f32)>,
    track: Option<&'static [(f32, f32)]>,
    life: f32,
    speed: f32,
    column: Option<(f32, f32)>,
    color: [[u8; 3]; 3],
    alpha: [f32; 3],
    radii: [f32; 3],
    mid: f32,
) -> ClientEmitter {
    ClientEmitter {
        name,
        sprite,
        additive: true,
        rate,
        window,
        life,
        speed,
        gravity: 0.0,
        area: 0.0,
        column,
        rise: false,
        color,
        alpha,
        size: ParticleSize::radii(radii),
        mid,
        track,
    }
}

/// A two-key ramp's middle key, `t` of the way along — so a bench ramp keyed
/// only at birth and death stays a straight line through the middle key.
const fn mid_of(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

const WHITE: [u8; 3] = [255, 255, 255];

// ── Divine Favor: innerfocus_impact_chest.m2 ─────────────────────────────────

/// The flare's 60 ms window (`flare` 100/s from 40 to 100 ms, ramped over a
/// millisecond at each end as the bench keys it): about six flares.
const DIVINE_FAVOR_FLARE_TRACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.040, 0.0),
    (0.041, 100.0),
    (0.100, 100.0),
    (0.101, 0.0),
];

static DIVINE_FAVOR_LAYERS: [OneShotLayer; 3] = [
    // `flare`: blue -> yellow -> red stars bursting out at 2.2 yd/s.
    at_knobs(
        OneShotLayer {
            on: DIVINE_FAVOR_FLARE,
            emitter: emitter(
                "innerfocus_impact_chest/flare",
                EmitterSprite::Flare,
                0.0,
                ClientEmitter::tracked(DIVINE_FAVOR_FLARE_TRACK),
                Some(DIVINE_FAVOR_FLARE_TRACK),
                1.0,
                2.22,
                Some((0.28, 0.28)),
                [[64, 121, 251], [255, 255, 0], [220, 0, 0]],
                [1.0, 1.0, 0.0],
                [0.25, mid_of(0.25, 0.12, 0.5), 0.12],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        DIVINE_FAVOR_KNOBS,
    ),
    // `gradient64flipa`: violet -> red -> sky streaks at 3.3 yd/s.
    at_knobs(
        OneShotLayer {
            on: DIVINE_FAVOR_STREAKS,
            emitter: emitter(
                "innerfocus_impact_chest/violet streaks",
                EmitterSprite::Streak,
                60.0,
                Some((0.0, 0.4)),
                None,
                0.8,
                3.33,
                None,
                [[88, 0, 248], [255, 0, 0], [98, 181, 239]],
                [0.0, 1.0, 0.0],
                [0.15, mid_of(0.15, 0.08, 0.5), 0.08],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        DIVINE_FAVOR_KNOBS,
    ),
    // Four `flare` points, 70/s each, on a 0.36 yd box: deep blue -> cyan ->
    // lavender.
    at_knobs(
        OneShotLayer {
            on: DIVINE_FAVOR_POINTS,
            emitter: emitter(
                "innerfocus_impact_chest/blue points",
                EmitterSprite::Glow,
                280.0,
                Some((0.0, 0.4)),
                None,
                0.8,
                0.56,
                Some((0.72, 0.72)),
                [[16, 29, 126], [44, 155, 225], [185, 172, 223]],
                [1.0, 1.0, 0.0],
                [0.12, mid_of(0.12, 0.05, 0.5), 0.05],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        DIVINE_FAVOR_KNOBS,
    ),
];

// ── Aura Mastery: holyward_impact_chest.m2 ───────────────────────────────────

static AURA_MASTERY_LAYERS: [OneShotLayer; 4] = [
    // `shockwavewater1` ripples, gold, alpha 0.4.
    at_knobs(
        OneShotLayer {
            on: AURA_MASTERY_RIPPLES,
            emitter: emitter(
                "holyward_impact_chest/gold ripples",
                EmitterSprite::Glow,
                10.8,
                Some((0.0, AURA_MASTERY_SECS)),
                None,
                1.0,
                0.06,
                Some((0.1, 0.0)),
                [[253, 221, 94], [240, 213, 60], [253, 222, 13]],
                [0.4, 0.38, 0.0],
                [0.4, mid_of(0.4, 1.2, 0.5), 1.2],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        AURA_MASTERY_KNOBS,
    ),
    // `star8b` burst, 67-600 ms, white -> gold, under heavy drag (3.35).
    at_knobs(
        OneShotLayer {
            on: AURA_MASTERY_STAR_BURST,
            emitter: emitter(
                "holyward_impact_chest/star burst",
                EmitterSprite::Flare,
                45.0,
                Some((0.067, 0.6)),
                None,
                0.75,
                2.56,
                None,
                [WHITE, [243, 200, 27], [255, 245, 191]],
                [1.0, 0.68, 0.0],
                [0.22, mid_of(0.22, 0.1, 0.5), 0.1],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 3.0 * 3.35,
        },
        AURA_MASTERY_KNOBS,
    ),
    // `genericglow2c` amber glows.
    at_knobs(
        OneShotLayer {
            on: AURA_MASTERY_AMBER,
            emitter: emitter(
                "holyward_impact_chest/amber glows",
                EmitterSprite::Glow,
                7.5,
                Some((0.0, AURA_MASTERY_SECS)),
                None,
                1.2,
                0.06,
                None,
                [[228, 159, 49], [232, 166, 25], [196, 154, 25]],
                [0.5, 0.65, 0.0],
                [0.6, mid_of(0.6, 0.9, 0.5), 0.9],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        AURA_MASTERY_KNOBS,
    ),
    // `star8b` ring emitter at 1.0 yd round the chest, gold -> white.
    at_knobs(
        OneShotLayer {
            on: AURA_MASTERY_STAR_RING,
            emitter: emitter(
                "holyward_impact_chest/star ring",
                EmitterSprite::Flare,
                61.0,
                Some((0.0, AURA_MASTERY_SECS)),
                None,
                0.81,
                0.17,
                None,
                [[250, 225, 57], WHITE, WHITE],
                [0.51, 0.72, 0.0],
                [0.14, mid_of(0.14, 0.08, 0.5), 0.08],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Ring {
                radius: 1.0,
                rev_per_sec: 0.5,
            },
            drag: 0.0,
        },
        AURA_MASTERY_KNOBS,
    ),
];

// ── Blessing of Sacrifice: blessingofsacrifice_impact.m2 ─────────────────────

/// `beerbubble` motes, 75/s over a 1.78 yd box a yard up, FALLING at 1.67
/// yd/s: dark red -> red -> orange. `on` is the end's own toggle.
const fn sacrifice_motes(on: bool) -> OneShotLayer {
    OneShotLayer {
        on,
        emitter: emitter(
            "blessingofsacrifice_impact/falling motes",
            EmitterSprite::Blob,
            75.0,
            Some((0.0, SACRIFICE_SECS)),
            None,
            1.5,
            -1.67,
            Some((1.78, 0.4)),
            [[42, 0, 0], [223, 0, 0], [255, 60, 0]],
            [0.0, 1.0, 0.0],
            [0.12, mid_of(0.12, 0.08, 0.5), 0.08],
            0.5,
        ),
        height: 1.0,
        launch: OneShotLaunch::Vertical { speed_var: 0.0 },
        drag: 0.0,
    }
}

/// `gradient64flipa` streaks, 1.28 yd up, flying out at 6.1 yd/s, alpha at
/// most 0.25. The bench's 66/s is shared between the two ends (each particle
/// is born at one of them), so each end emits 33/s.
const SACRIFICE_STREAK_LAYER: OneShotLayer = OneShotLayer {
    on: SACRIFICE_STREAKS,
    emitter: emitter(
        "blessingofsacrifice_impact/red streaks",
        EmitterSprite::Streak,
        33.0,
        Some((0.0, SACRIFICE_SECS)),
        None,
        0.7,
        6.1,
        None,
        [[124, 0, 0], [255, 12, 0], [248, 111, 0]],
        [0.0, 0.25, 0.0],
        [0.12, mid_of(0.12, 0.06, 0.5), 0.06],
        0.5,
    ),
    height: 1.28,
    launch: OneShotLaunch::Sphere,
    drag: 0.0,
};

static SACRIFICE_ALLY_LAYERS: [OneShotLayer; 2] = [
    at_knobs(sacrifice_motes(SACRIFICE_MOTES_ALLY), SACRIFICE_KNOBS),
    at_knobs(SACRIFICE_STREAK_LAYER, SACRIFICE_KNOBS),
];
static SACRIFICE_PALADIN_LAYERS: [OneShotLayer; 2] = [
    at_knobs(sacrifice_motes(SACRIFICE_MOTES_PALADIN), SACRIFICE_KNOBS),
    at_knobs(SACRIFICE_STREAK_LAYER, SACRIFICE_KNOBS),
];

// ── Infusion of Light: infusion_of_light_impact.m2 ───────────────────────────

const INFUSION_ORB_TRACK: &[(f32, f32)] = &[(0.0, 15.0), (1.367, 15.0), (1.4, 0.0)];
const INFUSION_FLASH_TRACK: &[(f32, f32)] = &[(0.0, 150.0), (0.067, 0.0)];
const INFUSION_RUNE_TRACK: &[(f32, f32)] = &[(0.0, 44.0), (0.5, 44.0), (0.534, 0.0)];
const INFUSION_BURST_TRACK: &[(f32, f32)] = &[
    (0.0, 111.0),
    (0.234, 0.0),
    (0.267, 77.0),
    (0.8, 66.0),
    (0.867, 0.0),
];

static INFUSION_LAYERS: [OneShotLayer; 4] = [
    // `crystalball_large` and `shockwave_hip1` at 2.5 yd: pale gold -> amber,
    // keyed at 0.35 of the life.
    at_knobs(
        OneShotLayer {
            on: INFUSION_OVERHEAD_GLOWS,
            emitter: emitter(
                "infusion_of_light_impact/overhead glows",
                EmitterSprite::Glow,
                0.0,
                ClientEmitter::tracked(INFUSION_ORB_TRACK),
                Some(INFUSION_ORB_TRACK),
                0.7,
                0.0,
                None,
                [[251, 243, 157], [241, 189, 49], [236, 162, 44]],
                [0.0, 0.46, 0.0],
                [0.35, mid_of(0.35, 0.6, 0.35), 0.6],
                0.35,
            ),
            height: 2.5,
            launch: OneShotLaunch::Vertical { speed_var: 0.0 },
            drag: 0.0,
        },
        INFUSION_KNOBS,
    ),
    // `flare1_tc` + `flare05`: the opening flash, 0-67 ms.
    at_knobs(
        OneShotLayer {
            on: INFUSION_FLASH,
            emitter: emitter(
                "infusion_of_light_impact/opening flash",
                EmitterSprite::Glow,
                0.0,
                ClientEmitter::tracked(INFUSION_FLASH_TRACK),
                Some(INFUSION_FLASH_TRACK),
                0.4,
                0.0,
                None,
                [WHITE, [237, 192, 72], WHITE],
                [1.0, 0.64, 0.0],
                [0.5, mid_of(0.5, 0.3, 0.5), 0.3],
                0.5,
            ),
            height: 2.49,
            launch: OneShotLaunch::Vertical { speed_var: 0.0 },
            drag: 0.0,
        },
        INFUSION_KNOBS,
    ),
    // Four `rune1d` runes at the chest, rising and falling at 1.1 yd/s.
    at_knobs(
        OneShotLayer {
            on: INFUSION_CHEST_RUNES,
            emitter: emitter(
                "infusion_of_light_impact/chest runes",
                EmitterSprite::Flare,
                0.0,
                ClientEmitter::tracked(INFUSION_RUNE_TRACK),
                Some(INFUSION_RUNE_TRACK),
                0.6,
                1.11,
                Some((0.1, 0.0)),
                [WHITE, [253, 218, 112], [244, 252, 255]],
                [0.0, 0.78, 0.0],
                [0.25, mid_of(0.25, 0.2, 0.5), 0.2],
                0.5,
            ),
            height: 1.4,
            launch: OneShotLaunch::Vertical { speed_var: 2.0 },
            drag: 0.0,
        },
        INFUSION_KNOBS,
    ),
    // `flare09` bursts high over the Paladin: white -> gold.
    at_knobs(
        OneShotLayer {
            on: INFUSION_BURSTS,
            emitter: emitter(
                "infusion_of_light_impact/flare bursts",
                EmitterSprite::Streak,
                0.0,
                ClientEmitter::tracked(INFUSION_BURST_TRACK),
                Some(INFUSION_BURST_TRACK),
                0.45,
                1.11,
                Some((0.5, 1.6)),
                [[254, 253, 242], [233, 193, 32], WHITE],
                [0.0, 0.54, 0.0],
                [0.18, mid_of(0.18, 0.1, 0.5), 0.1],
                0.5,
            ),
            height: 2.9,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        INFUSION_KNOBS,
    ),
];

// ── The Art of War: the_art_of_war_impact.m2 ─────────────────────────────────

const ART_OF_WAR_GLOW_TRACK: &[(f32, f32)] = &[(0.0, 11.0), (0.8, 11.0), (0.867, 0.0)];
const ART_OF_WAR_RUNE_TRACK: &[(f32, f32)] = &[(0.0, 6.0), (0.434, 6.0), (0.5, 0.0)];
const ART_OF_WAR_BURST_TRACK: &[(f32, f32)] = &[(0.0, 77.0), (0.134, 66.0), (0.2, 0.0)];

static ART_OF_WAR_LAYERS: [OneShotLayer; 3] = [
    // `flare09` gold glows at the chest.
    at_knobs(
        OneShotLayer {
            on: ART_OF_WAR_GLOWS,
            emitter: emitter(
                "the_art_of_war_impact/gold glows",
                EmitterSprite::Glow,
                0.0,
                ClientEmitter::tracked(ART_OF_WAR_GLOW_TRACK),
                Some(ART_OF_WAR_GLOW_TRACK),
                0.7,
                0.0,
                Some((0.14, 0.0)),
                [[244, 235, 141], [233, 193, 32], WHITE],
                [1.0, 0.36, 0.0],
                [0.4, mid_of(0.4, 0.3, 0.5), 0.3],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Vertical { speed_var: 0.0 },
            drag: 0.0,
        },
        ART_OF_WAR_KNOBS,
    ),
    // `rune1d` runes, white.
    at_knobs(
        OneShotLayer {
            on: ART_OF_WAR_RUNES,
            emitter: emitter(
                "the_art_of_war_impact/runes",
                EmitterSprite::Flare,
                0.0,
                ClientEmitter::tracked(ART_OF_WAR_RUNE_TRACK),
                Some(ART_OF_WAR_RUNE_TRACK),
                0.7,
                0.0,
                None,
                [WHITE, WHITE, WHITE],
                [0.0, 0.38, 0.0],
                [0.4, 0.4, 0.4],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Vertical { speed_var: 0.0 },
            drag: 0.0,
        },
        ART_OF_WAR_KNOBS,
    ),
    // `flare09` burst 1.33 yd above the chest: white -> gold.
    at_knobs(
        OneShotLayer {
            on: ART_OF_WAR_BURST,
            emitter: emitter(
                "the_art_of_war_impact/burst above the chest",
                EmitterSprite::Streak,
                0.0,
                ClientEmitter::tracked(ART_OF_WAR_BURST_TRACK),
                Some(ART_OF_WAR_BURST_TRACK),
                0.4,
                1.11,
                Some((0.6, 0.0)),
                [[254, 253, 242], [233, 193, 32], WHITE],
                [0.0, 0.54, 0.0],
                [0.16, mid_of(0.16, 0.08, 0.5), 0.08],
                0.5,
            ),
            height: ONESHOT_CHEST + 1.33,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        ART_OF_WAR_KNOBS,
    ),
];

// ── Per-kind routing ─────────────────────────────────────────────────────────

/// A one-shot's emitter layers (empty for Retribution Aura, which is all
/// drawn pieces).
pub fn oneshot_layers(kind: PaladinOneShotKind) -> &'static [OneShotLayer] {
    match kind {
        PaladinOneShotKind::DivineFavor => &DIVINE_FAVOR_LAYERS,
        PaladinOneShotKind::AuraMastery => &AURA_MASTERY_LAYERS,
        PaladinOneShotKind::SacrificeAlly => &SACRIFICE_ALLY_LAYERS,
        PaladinOneShotKind::SacrificePaladin => &SACRIFICE_PALADIN_LAYERS,
        PaladinOneShotKind::Retribution => &[],
        PaladinOneShotKind::InfusionOfLight => &INFUSION_LAYERS,
        PaladinOneShotKind::ArtOfWar => &ART_OF_WAR_LAYERS,
    }
}

/// A one-shot's knobs.
pub fn oneshot_knobs(kind: PaladinOneShotKind) -> OneShotKnobs {
    match kind {
        PaladinOneShotKind::DivineFavor => DIVINE_FAVOR_KNOBS,
        PaladinOneShotKind::AuraMastery => AURA_MASTERY_KNOBS,
        PaladinOneShotKind::SacrificeAlly | PaladinOneShotKind::SacrificePaladin => SACRIFICE_KNOBS,
        PaladinOneShotKind::Retribution => RETRIBUTION_AURA_KNOBS,
        PaladinOneShotKind::InfusionOfLight => INFUSION_KNOBS,
        PaladinOneShotKind::ArtOfWar => ART_OF_WAR_KNOBS,
    }
}

/// How long a one-shot's rig lives, at its knobs.
pub fn oneshot_secs(kind: PaladinOneShotKind) -> f32 {
    let secs = match kind {
        PaladinOneShotKind::DivineFavor => DIVINE_FAVOR_SECS,
        PaladinOneShotKind::AuraMastery => AURA_MASTERY_SECS,
        PaladinOneShotKind::SacrificeAlly | PaladinOneShotKind::SacrificePaladin => SACRIFICE_SECS,
        PaladinOneShotKind::Retribution => RETRIBUTION_AURA_SECS,
        PaladinOneShotKind::InfusionOfLight => INFUSION_SECS,
        PaladinOneShotKind::ArtOfWar => ART_OF_WAR_SECS,
    };
    secs * oneshot_knobs(kind).duration
}

/// What the lands-silently audit names as each one-shot's visual.
pub fn oneshot_visual(kind: PaladinOneShotKind) -> &'static str {
    match kind {
        PaladinOneShotKind::DivineFavor => {
            "arcane hands + innerfocus burst at the chest (paladin_oneshots.rs)"
        }
        PaladinOneShotKind::AuraMastery => {
            "Holy hand flash + holyward ward on the Paladin (paladin_oneshots.rs)"
        }
        PaladinOneShotKind::SacrificeAlly | PaladinOneShotKind::SacrificePaladin => {
            "Holy hand flash + red shell at both units' feet (paladin_oneshots.rs)"
        }
        PaladinOneShotKind::Retribution => {
            "red rune disc at the Paladin's feet as the aura goes up (paladin_oneshots.rs)"
        }
        PaladinOneShotKind::InfusionOfLight => {
            "gold halo above the Paladin's head (paladin_oneshots.rs)"
        }
        PaladinOneShotKind::ArtOfWar => {
            "gold spark ring opening round the chest (paladin_oneshots.rs)"
        }
    }
}

/// Where a landing plays: the rigs it spawns (unit, one-shot), and the unit
/// whose hands flash, if any. Aura Mastery and Retribution Aura play only on
/// the landing whose bearer is the caster — the client draws nothing on the
/// teammates they cover — and Blessing of Sacrifice plays at both ends.
pub fn oneshot_plan(
    landing: AuraLandingKind,
    bearer: Entity,
    caster: Option<Entity>,
) -> (Vec<(Entity, PaladinOneShotKind)>, Option<Entity>) {
    let Some(kind) = PaladinOneShotKind::for_landing(landing) else {
        return (Vec::new(), None);
    };
    let own = caster == Some(bearer);
    match kind {
        PaladinOneShotKind::DivineFavor => (
            vec![(bearer, kind)],
            DIVINE_FAVOR_CAST_HANDS.then_some(bearer),
        ),
        PaladinOneShotKind::AuraMastery if own => {
            (vec![(bearer, kind)], AURA_MASTERY_HANDS.then_some(bearer))
        }
        PaladinOneShotKind::Retribution if own => (vec![(bearer, kind)], None),
        PaladinOneShotKind::AuraMastery | PaladinOneShotKind::Retribution => (Vec::new(), None),
        PaladinOneShotKind::SacrificeAlly | PaladinOneShotKind::SacrificePaladin => {
            let mut rigs = vec![(bearer, PaladinOneShotKind::SacrificeAlly)];
            let paladin = caster.filter(|c| *c != bearer);
            if let Some(paladin) = paladin {
                rigs.push((paladin, PaladinOneShotKind::SacrificePaladin));
            }
            (rigs, caster.filter(|_| SACRIFICE_HANDS))
        }
        PaladinOneShotKind::InfusionOfLight | PaladinOneShotKind::ArtOfWar => {
            (vec![(bearer, kind)], None)
        }
    }
}

// ── The launch ───────────────────────────────────────────────────────────────

/// Where a particle is born, relative to the layer's anchor point, and how
/// it launches, `age` seconds into the one-shot (the ring emitter turns).
pub fn oneshot_launch(layer: &OneShotLayer, age: f32, seed: u32) -> (Vec3, Vec3) {
    let e = &layer.emitter;
    let mut offset = match e.column {
        Some((w, h)) => Vec3::new(
            (draw(seed, 1) - 0.5) * w,
            (draw(seed, 2) - 0.5) * h,
            (draw(seed, 3) - 0.5) * w,
        ),
        None => Vec3::ZERO,
    };
    let sphere = || {
        let u = draw(seed, 4) * 2.0 - 1.0;
        let theta = draw(seed, 5) * TAU;
        let r = (1.0 - u * u).max(0.0).sqrt();
        Vec3::new(r * theta.cos(), u, r * theta.sin())
    };
    let velocity = match layer.launch {
        OneShotLaunch::Sphere => sphere() * e.speed,
        OneShotLaunch::Vertical { speed_var } => {
            Vec3::Y * e.speed * (1.0 + speed_var * (draw(seed, 6) * 2.0 - 1.0))
        }
        OneShotLaunch::Ring {
            radius,
            rev_per_sec,
        } => {
            let point = ((draw(seed, 7) * 3.0) as u32).min(2) as f32;
            let angle = age * rev_per_sec * TAU + point * TAU / 3.0;
            offset += Vec3::new(radius * angle.cos(), 0.0, radius * angle.sin());
            sphere() * e.speed
        }
    };
    (offset, velocity)
}

// ── The drawn pieces ─────────────────────────────────────────────────────────

/// How a piece is turned.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PieceFacing {
    /// A quad facing the camera (the bench's `bbGlow` and `sprite`).
    Camera,
    /// A quad lying flat, facing up, turned `turn` radians about world up
    /// (the bench's `ring3`, `groundDisc` and `glyphDisc`).
    Flat { turn: f32 },
    /// A flat arrow pointing out along `angle` radians in the ground plane
    /// (from +X toward +Z, as the bench measures it).
    Arrow { angle: f32 },
}

/// One piece's pose and look at a moment: rig-local position (the rig stands
/// at the unit's feet), size (a quad's width and height; an arrow's length
/// and base width), facing, colour and alpha.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PiecePose {
    pub at: Vec3,
    pub size: Vec2,
    pub facing: PieceFacing,
    pub rgb: [u8; 3],
    pub alpha: f32,
}

/// Aura Mastery's ward: the `yellow_glow` quad's weight track.
pub const WARD_WEIGHT: [(f32, f32); 4] = [(0.0, 0.0), (0.667, 0.6), (2.334, 0.6), (3.0, 0.0)];
/// The ward's width, yards, its colour and its drawn alpha over the weight.
pub const WARD_SIZE: f32 = 4.4;
pub const WARD_RGB: [u8; 3] = [255, 232, 130];
pub const WARD_ALPHA: f32 = 0.55;

/// Blessing of Sacrifice's shell weight: in by 0.57 s, held to 1.5 s, out by
/// 1.73 s.
pub const SHELL_WEIGHT: [(f32, f32); 4] = [(0.0, 0.0), (0.567, 1.0), (1.5, 1.0), (1.733, 0.0)];
/// The client's shell, `(width, height)` yards, and the fraction of it the
/// bench draws as its glow.
pub const SHELL_CLIENT_SIZE: Vec2 = Vec2::new(3.6, 3.3);
pub const SHELL_GLOW_FRACTION: f32 = 0.75;
pub const SHELL_GLOW_HEIGHT: f32 = 1.4;
pub const SHELL_GLOW_RGB: [u8; 3] = [220, 10, 0];
pub const SHELL_RING_RADIUS: f32 = 1.2;
pub const SHELL_RING_RGB: [u8; 3] = [255, 54, 0];
pub const SHELL_GLYPH_RADIUS: f32 = 0.9;
pub const SHELL_GLYPH_RGB: [u8; 3] = [243, 137, 0];
/// The shell glyph's turn, rad/s, and its ticks.
pub const SHELL_GLYPH_SPIN: f32 = 1.0;
pub const SHELL_GLYPH_TICKS: u32 = 8;

/// Retribution Aura's disc: the glyph rings' weight, the inner ring's, the
/// arrows' and the underlay's, and the two scale tracks (glyph rings
/// 1.0 -> 1.31 by 0.4 s, arrows 1.0 -> 1.62 by 0.8 s).
pub const RETRIBUTION_GLYPH_WEIGHT: [(f32, f32); 4] =
    [(0.0, 0.0), (0.1, 0.8), (0.634, 0.8), (1.034, 0.0)];
pub const RETRIBUTION_RING_WEIGHT: [(f32, f32); 4] =
    [(0.0, 0.0), (0.4, 0.8), (0.834, 0.8), (1.234, 0.0)];
pub const RETRIBUTION_ARROW_WEIGHT: [(f32, f32); 4] =
    [(0.0, 0.0), (0.5, 0.0), (0.9, 1.0), (1.3, 0.0)];
pub const RETRIBUTION_UNDERLAY_WEIGHT: [(f32, f32); 4] =
    [(0.0, 0.0), (0.1, 0.4), (1.034, 0.4), (1.434, 0.0)];
pub const RETRIBUTION_GLYPH_SCALE: [(f32, f32); 2] = [(0.0, 1.0), (0.4, 1.31)];
pub const RETRIBUTION_ARROW_SCALE: [(f32, f32); 2] = [(0.0, 1.0), (0.8, 1.62)];
/// The glyph rings' radius before their scale (the bench's `0.83 × 0.9`).
pub const RETRIBUTION_GLYPH_RADIUS: f32 = 0.83 * 0.9;
pub const RETRIBUTION_GLYPH_RGB: [u8; 3] = [252, 93, 12];
pub const RETRIBUTION_GLYPH_SPIN: f32 = 0.5;
pub const RETRIBUTION_GLYPH_TICKS: u32 = 16;
pub const RETRIBUTION_RING_RADIUS: f32 = 0.6;
pub const RETRIBUTION_RING_RGB: [u8; 3] = [255, 0, 0];
/// The arrows' bases sit on this circle (the client's 0.91 yd, as the bench
/// draws it at 0.95), spanning 0.15 rad either side of the arrow's line; each
/// reaches 0.4 yd past it before its scale.
pub const RETRIBUTION_ARROW_BASE: f32 = 0.91 * 0.95;
pub const RETRIBUTION_ARROW_HALF_ANGLE: f32 = 0.15;
pub const RETRIBUTION_ARROW_LENGTH: f32 = 0.4;
pub const RETRIBUTION_ARROW_RGB: [u8; 3] = [255, 162, 0];
pub const RETRIBUTION_ARROW_ALPHA: f32 = 0.9;
/// The pale-gold `blue_glow2` underlay: 2.9 yd across.
pub const RETRIBUTION_UNDERLAY_RADIUS: f32 = 1.44;
pub const RETRIBUTION_UNDERLAY_RGB: [u8; 3] = [255, 199, 29];
pub const RETRIBUTION_UNDERLAY_ALPHA: f32 = 0.6;

/// Infusion of Light's halo: the `timebar2` ring's weight, its height, its
/// radius (the bench draws the client's 0.3 yd ring at 0.15 × 1.6), and the
/// glow behind it.
pub const HALO_WEIGHT: [(f32, f32); 4] = [(0.0, 0.0), (0.567, 0.55), (1.434, 0.55), (1.834, 0.0)];
pub const HALO_HEIGHT: f32 = 2.5;
pub const HALO_RADIUS: f32 = 0.15 * 1.6;
pub const HALO_RGB: [u8; 3] = [237, 192, 72];
pub const HALO_RING_ALPHA: f32 = 1.4;
pub const HALO_GLOW_SIZE: Vec2 = Vec2::new(0.6, 0.3);
pub const HALO_GLOW_RGB: [u8; 3] = [255, 230, 150];
pub const HALO_GLOW_ALPHA: f32 = 0.6;

/// The Art of War's disc: `shockwave_fullrays1`, 0.35 yd across, in at
/// 100 ms and out by 634 ms, turning; and the glow behind it.
pub const AOW_DISC_WEIGHT: [(f32, f32); 3] = [(0.0, 0.0), (0.1, 1.0), (0.634, 0.0)];
pub const AOW_DISC_RADIUS: f32 = 0.35 * 0.5;
pub const AOW_DISC_RGB: [u8; 3] = [221, 181, 57];
pub const AOW_DISC_SPIN: f32 = 2.0;
pub const AOW_DISC_TICKS: u32 = 10;
pub const AOW_GLOW_SIZE: f32 = 0.5;
pub const AOW_GLOW_RGB: [u8; 3] = [255, 230, 140];
pub const AOW_GLOW_ALPHA: f32 = 0.6;
/// The opening spark ring: forty sparks, each born in the first 0.8 s and
/// living 0.7 s where it is born, on a ring whose radius at its birth runs
/// 0.1 -> 1.0 over 0.9 s (drawn at half that, plus 0.05 yd, as the bench
/// does), 0.22 yd across.
pub const AOW_SPARKS: u8 = 40;
pub const AOW_SPARK_BIRTH_SPAN: f32 = 0.8;
pub const AOW_SPARK_LIFE: f32 = 0.7;
pub const AOW_SPARK_RING: [(f32, f32); 2] = [(0.0, 0.1), (0.9, 1.0)];
pub const AOW_SPARK_SIZE: f32 = 0.22;
pub const AOW_SPARK_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 249, 181]),
    (0.5, [229, 148, 62]),
    (1.0, [242, 227, 65]),
];
pub const AOW_SPARK_ALPHA: [(f32, f32); 3] = [(0.0, 0.0), (0.5, 0.48), (1.0, 0.0)];
pub const AOW_SPARK_ALPHA_GAIN: f32 = 1.6;

/// The bench's `hh`: a deterministic 0..1 scatter, `fract(sin(n × 12.9898) ×
/// 43758.5453)`, in double precision as the bench computes it — so the spark
/// ring scatters exactly as the signed-off bench drew it.
pub fn bench_hash(n: u32) -> f32 {
    let x = (n as f64 * 12.9898).sin() * 43758.5453;
    (x - x.floor()) as f32
}

/// The pieces a one-shot draws, by its toggles.
pub fn oneshot_pieces(kind: PaladinOneShotKind) -> Vec<OneShotPieceRole> {
    use OneShotPieceRole as R;
    let mut pieces = Vec::new();
    match kind {
        PaladinOneShotKind::DivineFavor => {}
        PaladinOneShotKind::AuraMastery => {
            if AURA_MASTERY_GLOW_QUAD {
                pieces.push(R::WardGlow);
            }
        }
        PaladinOneShotKind::SacrificeAlly | PaladinOneShotKind::SacrificePaladin => {
            if SACRIFICE_SHELL {
                pieces.extend([R::ShellGlow, R::ShellRing, R::ShellGlyph]);
            }
        }
        PaladinOneShotKind::Retribution => {
            if RETRIBUTION_AURA_UNDERLAY {
                pieces.push(R::RetributionUnderlay);
            }
            if RETRIBUTION_AURA_CAST_DISC {
                pieces.extend([R::RetributionGlyph, R::RetributionRing]);
                pieces.extend((0..4).map(|index| R::RetributionArrow { index }));
            }
        }
        PaladinOneShotKind::InfusionOfLight => {
            if INFUSION_HALO {
                pieces.extend([R::HaloRing, R::HaloGlow]);
            }
        }
        PaladinOneShotKind::ArtOfWar => {
            if ART_OF_WAR_DISC {
                pieces.extend([R::ArtOfWarDisc, R::ArtOfWarGlow]);
            }
            if ART_OF_WAR_SPARK_RING {
                pieces.extend((0..AOW_SPARKS).map(|index| R::ArtOfWarSpark { index }));
            }
        }
    }
    pieces
}

/// The kind a piece belongs to, for its knobs.
fn piece_kind(role: OneShotPieceRole) -> PaladinOneShotKind {
    use OneShotPieceRole as R;
    match role {
        R::WardGlow => PaladinOneShotKind::AuraMastery,
        R::ShellGlow | R::ShellRing | R::ShellGlyph => PaladinOneShotKind::SacrificeAlly,
        R::RetributionGlyph
        | R::RetributionRing
        | R::RetributionArrow { .. }
        | R::RetributionUnderlay => PaladinOneShotKind::Retribution,
        R::HaloRing | R::HaloGlow => PaladinOneShotKind::InfusionOfLight,
        R::ArtOfWarDisc | R::ArtOfWarGlow | R::ArtOfWarSpark { .. } => PaladinOneShotKind::ArtOfWar,
    }
}

/// The weight of a spark of the opening ring `age` seconds in, and its pose
/// inside the ring, or `None` while it is unborn or spent.
fn aow_spark(index: u8, age: f32) -> Option<PiecePose> {
    let k = index as u32;
    let born = bench_hash(k) * AOW_SPARK_BIRTH_SPAN;
    let since = age - born;
    if !(0.0..=AOW_SPARK_LIFE).contains(&since) {
        return None;
    }
    let ph = since / AOW_SPARK_LIFE;
    let radius = keyed(&AOW_SPARK_RING, born) * 0.5 + 0.05;
    let angle = bench_hash(k + 40) * TAU;
    let lift = (bench_hash(k + 80) - 0.5) * 0.1;
    let rgb = keyed_color(&AOW_SPARK_RGB, ph).to_srgba();
    Some(PiecePose {
        at: Vec3::new(
            radius * angle.cos(),
            ONESHOT_CHEST + lift,
            radius * angle.sin(),
        ),
        size: Vec2::splat(AOW_SPARK_SIZE),
        facing: PieceFacing::Camera,
        rgb: [
            (rgb.red * 255.0).round() as u8,
            (rgb.green * 255.0).round() as u8,
            (rgb.blue * 255.0).round() as u8,
        ],
        alpha: keyed(&AOW_SPARK_ALPHA, ph) * AOW_SPARK_ALPHA_GAIN,
    })
}

/// A piece's pose `age` seconds into its one-shot, at the client's values
/// (the knobs are applied by the caller). Pure: the probes and the update
/// system read one definition.
pub fn piece_pose(role: OneShotPieceRole, age: f32) -> PiecePose {
    use OneShotPieceRole as R;
    let flat = |turn: f32| PieceFacing::Flat { turn };
    let disc = |height: f32, radius: f32, facing, rgb, alpha| PiecePose {
        at: Vec3::Y * height,
        size: Vec2::splat(2.0 * radius),
        facing,
        rgb,
        alpha,
    };
    match role {
        R::WardGlow => disc(
            ONESHOT_CHEST,
            WARD_SIZE * 0.5,
            PieceFacing::Camera,
            WARD_RGB,
            keyed(&WARD_WEIGHT, age) * WARD_ALPHA,
        ),
        R::ShellGlow => PiecePose {
            at: Vec3::Y * SHELL_GLOW_HEIGHT,
            size: SHELL_CLIENT_SIZE * SHELL_GLOW_FRACTION,
            facing: PieceFacing::Camera,
            rgb: SHELL_GLOW_RGB,
            alpha: keyed(&SHELL_WEIGHT, age) * 0.45,
        },
        R::ShellRing => disc(
            0.05,
            SHELL_RING_RADIUS,
            flat(0.0),
            SHELL_RING_RGB,
            keyed(&SHELL_WEIGHT, age) * 0.6,
        ),
        R::ShellGlyph => disc(
            0.04,
            SHELL_GLYPH_RADIUS,
            flat(age * SHELL_GLYPH_SPIN),
            SHELL_GLYPH_RGB,
            keyed(&SHELL_WEIGHT, age) * 0.5,
        ),
        R::RetributionGlyph => disc(
            0.03,
            RETRIBUTION_GLYPH_RADIUS * keyed(&RETRIBUTION_GLYPH_SCALE, age),
            flat(age * RETRIBUTION_GLYPH_SPIN),
            RETRIBUTION_GLYPH_RGB,
            keyed(&RETRIBUTION_GLYPH_WEIGHT, age),
        ),
        R::RetributionRing => disc(
            0.035,
            RETRIBUTION_RING_RADIUS * keyed(&RETRIBUTION_GLYPH_SCALE, age),
            flat(0.0),
            RETRIBUTION_RING_RGB,
            keyed(&RETRIBUTION_RING_WEIGHT, age),
        ),
        R::RetributionArrow { index } => {
            let angle = index as f32 * TAU / 4.0 + FRAC_PI_4;
            // The base is the chord between the circle's points 0.15 rad
            // either side; the tip reaches the scaled length past the circle.
            let base = RETRIBUTION_ARROW_BASE * RETRIBUTION_ARROW_HALF_ANGLE.cos();
            let tip = RETRIBUTION_ARROW_BASE
                + RETRIBUTION_ARROW_LENGTH * keyed(&RETRIBUTION_ARROW_SCALE, age);
            PiecePose {
                at: Vec3::new(base * angle.cos(), 0.04, base * angle.sin()),
                size: Vec2::new(
                    tip - base,
                    2.0 * RETRIBUTION_ARROW_BASE * RETRIBUTION_ARROW_HALF_ANGLE.sin(),
                ),
                facing: PieceFacing::Arrow { angle },
                rgb: RETRIBUTION_ARROW_RGB,
                alpha: keyed(&RETRIBUTION_ARROW_WEIGHT, age) * RETRIBUTION_ARROW_ALPHA,
            }
        }
        R::RetributionUnderlay => disc(
            0.02,
            RETRIBUTION_UNDERLAY_RADIUS,
            flat(0.0),
            RETRIBUTION_UNDERLAY_RGB,
            keyed(&RETRIBUTION_UNDERLAY_WEIGHT, age) * RETRIBUTION_UNDERLAY_ALPHA,
        ),
        R::HaloRing => disc(
            HALO_HEIGHT,
            HALO_RADIUS,
            flat(0.0),
            HALO_RGB,
            keyed(&HALO_WEIGHT, age) * HALO_RING_ALPHA,
        ),
        R::HaloGlow => PiecePose {
            at: Vec3::Y * HALO_HEIGHT,
            size: HALO_GLOW_SIZE,
            facing: PieceFacing::Camera,
            rgb: HALO_GLOW_RGB,
            alpha: keyed(&HALO_WEIGHT, age) * HALO_GLOW_ALPHA,
        },
        R::ArtOfWarDisc => disc(
            ONESHOT_CHEST,
            AOW_DISC_RADIUS,
            flat(age * AOW_DISC_SPIN),
            AOW_DISC_RGB,
            keyed(&AOW_DISC_WEIGHT, age),
        ),
        R::ArtOfWarGlow => disc(
            ONESHOT_CHEST,
            AOW_GLOW_SIZE * 0.5,
            PieceFacing::Camera,
            AOW_GLOW_RGB,
            keyed(&AOW_DISC_WEIGHT, age) * AOW_GLOW_ALPHA,
        ),
        R::ArtOfWarSpark { index } => aow_spark(index, age).unwrap_or(PiecePose {
            at: Vec3::Y * ONESHOT_CHEST,
            size: Vec2::splat(AOW_SPARK_SIZE),
            facing: PieceFacing::Camera,
            rgb: AOW_SPARK_RGB[0].1,
            alpha: 0.0,
        }),
    }
}

/// The rotation a piece takes under a rig turned by `rig` (the rig is never
/// turned, so this is its world rotation), facing the camera at `camera`.
pub fn piece_rotation(facing: PieceFacing, rig: Quat, camera: Option<Quat>) -> Quat {
    let world = match facing {
        PieceFacing::Camera => match camera {
            Some(camera) => camera,
            None => return Quat::IDENTITY,
        },
        // `Rectangle` faces +Z; tipped back a quarter turn it faces up.
        PieceFacing::Flat { turn } => {
            Quat::from_rotation_y(turn) * Quat::from_rotation_x(-FRAC_PI_2)
        }
        // The arrow mesh lies in the ground plane pointing along +X; turning
        // it by `-angle` about up points it along (cos, 0, sin).
        PieceFacing::Arrow { angle } => Quat::from_rotation_y(-angle),
    };
    rig.inverse() * world
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// Generated sprite textures, pixels on a side.
const PIECE_PX: u32 = 128;
/// A drawn ring's stroke: its centre radius and half-width over the sprite's
/// radius (the bench's 2-3 px `ring3` stroke).
const RING_STROKE: (f32, f32) = (0.965, 0.035);
/// A glyph disc's inner ring sits at 0.78 of the outer (the bench's
/// `glyphDisc`), its ticks run from the inner ring to the outer, slanted
/// 0.12 rad, and its soft fill is a quarter of the ground disc.
const GLYPH_INNER: f32 = 0.78;
const GLYPH_TICK_SLANT: f32 = 0.12;
const GLYPH_TICK_HALF_WIDTH: f32 = 0.02;
/// The bench's `bbGlow` gradient, and its `groundDisc` one.
pub const GLOW_STOPS: [(f32, f32); 3] = [(0.0, 1.0), (0.5, 0.45), (1.0, 0.0)];
pub const GROUND_STOPS: [(f32, f32); 3] = [(0.0, 1.0), (0.55, 0.55), (1.0, 0.0)];

fn stroke(r: f32, centre: f32, half: f32) -> f32 {
    (1.0 - (r - centre).abs() / half).clamp(0.0, 1.0)
}

/// The soft camera-facing glow (`bbGlow`).
pub fn oneshot_glow_texture() -> Image {
    disc_texture(PIECE_PX, |r, _| keyed(&GLOW_STOPS, r))
}

/// The soft ground disc (`groundDisc`).
pub fn oneshot_ground_texture() -> Image {
    disc_texture(PIECE_PX, |r, _| {
        if r >= 1.0 {
            0.0
        } else {
            keyed(&GROUND_STOPS, r)
        }
    })
}

/// A thin ring (`ring3`).
pub fn oneshot_ring_texture() -> Image {
    disc_texture(PIECE_PX, |r, _| stroke(r, RING_STROKE.0, RING_STROKE.1))
}

/// A glyph disc (`glyphDisc`) with `ticks` slanted ticks: the outer ring,
/// the inner ring at 0.7, the ticks, and the ground disc at 0.25 — summed, as
/// the bench draws them additively.
pub fn oneshot_glyph_texture(ticks: u32) -> Image {
    let (outer, half) = RING_STROKE;
    let inner = outer * GLYPH_INNER;
    let tick_ends: Vec<(Vec2, Vec2)> = (0..ticks)
        .map(|k| {
            let t = k as f32 / ticks as f32 * TAU;
            (
                Vec2::from_angle(t) * inner,
                Vec2::from_angle(t + GLYPH_TICK_SLANT) * outer,
            )
        })
        .collect();
    disc_texture(PIECE_PX, move |r, a| {
        if r >= 1.0 {
            return 0.0;
        }
        let p = Vec2::from_angle(a) * r;
        let tick = tick_ends
            .iter()
            .map(|(s, e)| {
                let d = *e - *s;
                let u = ((p - *s).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                (p - (*s + d * u)).length()
            })
            .fold(f32::MAX, f32::min);
        let tick = (1.0 - tick / GLYPH_TICK_HALF_WIDTH).clamp(0.0, 1.0);
        stroke(r, outer, half)
            + 0.7 * stroke(r, inner, half)
            + tick
            + 0.25 * keyed(&GROUND_STOPS, r)
    })
}

/// Meshes and textures every one-shot piece draws with, built once.
pub struct OneShotAssets {
    quad: Handle<Mesh>,
    arrow: Handle<Mesh>,
    glow: Handle<Image>,
    ground: Handle<Image>,
    ring: Handle<Image>,
    star: Handle<Image>,
    shell_glyph: Handle<Image>,
    retribution_glyph: Handle<Image>,
    aow_glyph: Handle<Image>,
}

impl OneShotAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            // A unit arrow in the ground plane: its base a unit wide across
            // Z at the origin, its tip a unit out along +X.
            arrow: meshes.add(Triangle3d::new(
                Vec3::new(0.0, 0.0, -0.5),
                Vec3::new(0.0, 0.0, 0.5),
                Vec3::X,
            )),
            glow: images.add(oneshot_glow_texture()),
            ground: images.add(oneshot_ground_texture()),
            ring: images.add(oneshot_ring_texture()),
            star: images.add(star_flash_texture()),
            shell_glyph: images.add(oneshot_glyph_texture(SHELL_GLYPH_TICKS)),
            retribution_glyph: images.add(oneshot_glyph_texture(RETRIBUTION_GLYPH_TICKS)),
            aow_glyph: images.add(oneshot_glyph_texture(AOW_DISC_TICKS)),
        }
    }

    /// A piece's mesh and texture (`None`: a flat colour, the arrows).
    fn for_piece(&self, role: OneShotPieceRole) -> (Handle<Mesh>, Option<Handle<Image>>) {
        use OneShotPieceRole as R;
        let texture = match role {
            R::WardGlow | R::ShellGlow | R::HaloGlow | R::ArtOfWarGlow => self.glow.clone(),
            R::ShellRing | R::RetributionRing | R::HaloRing => self.ring.clone(),
            R::ShellGlyph => self.shell_glyph.clone(),
            R::RetributionGlyph => self.retribution_glyph.clone(),
            R::ArtOfWarDisc => self.aow_glyph.clone(),
            R::RetributionUnderlay => self.ground.clone(),
            R::ArtOfWarSpark { .. } => self.star.clone(),
            R::RetributionArrow { .. } => return (self.arrow.clone(), None),
        };
        (self.quad.clone(), Some(texture))
    }
}

/// How much smaller a pet's body is.
fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): for every Paladin talent landing the sim marked, a
/// rig on each unit it plays on ([`oneshot_plan`]) with its drawn pieces, and
/// the cast's hand flash. Only these landings' markers are consumed; the
/// Druid's and Shadow Word: Pain's are their renderers'.
#[allow(clippy::too_many_arguments)]
pub fn spawn_paladin_oneshots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<OneShotAssets>>,
    mut hand_assets: Local<Option<HealCastAssets>>,
    landings: Query<(Entity, &AuraLanding)>,
    units: Query<(&Transform, Option<&Pet>), With<Combatant>>,
    casters: Query<&Children>,
    bodies: Query<(), With<VisualBody>>,
) {
    for (marker, landing) in landings.iter() {
        if PaladinOneShotKind::for_landing(landing.kind).is_none() {
            continue;
        }
        commands.entity(marker).despawn();
        let (rigs, hands) = oneshot_plan(landing.kind, landing.target, landing.caster);
        for (unit, kind) in rigs {
            let Ok((transform, pet)) = units.get(unit) else {
                continue;
            };
            let is_pet = pet.is_some();
            let assets =
                assets.get_or_insert_with(|| OneShotAssets::build(&mut meshes, &mut images));
            let rig = commands
                .spawn((
                    PaladinOneShot {
                        kind,
                        bearer: unit,
                        is_pet,
                        age: 0.0,
                        carry: [0.0; 4],
                        emitted: 0,
                    },
                    Transform::from_translation(body_point(0.0, transform.translation, is_pet)),
                    Visibility::default(),
                    PlayMatchEntity,
                ))
                .id();
            for role in oneshot_pieces(kind) {
                let (mesh, texture) = assets.for_piece(role);
                let pose = piece_pose(role, 0.0);
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2])
                        .with_alpha(0.0),
                    base_color_texture: texture,
                    // The bench adds colour × sprite × alpha and nothing else.
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                });
                let piece = commands
                    .spawn((
                        PaladinOneShotPiece { role },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_scale(Vec3::splat(1e-3)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(rig).add_child(piece);
            }
        }
        if let (Some(caster), Some(ability)) = (hands, landing.kind.cast_ability()) {
            flash_instant_hands(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                &mut hand_assets,
                &casters,
                &bodies,
                caster,
                ability,
            );
        }
    }
}

/// Play every live one-shot: follow its unit's feet, emit each layer's
/// particles over its windows, pose and fade its pieces, and retire it when
/// its sequence has run (or as soon as its unit is gone). Runs before
/// `animate_client_particles`, which ages what it emits.
#[allow(clippy::too_many_arguments)]
pub fn update_paladin_oneshots(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut emitters: Local<Option<EmitterAssets>>,
    mut rigs: Query<(
        Entity,
        &mut PaladinOneShot,
        &mut Transform,
        Option<&Children>,
    )>,
    mut pieces: Query<
        (
            &PaladinOneShotPiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<PaladinOneShot>,
    >,
    units: Query<
        &Transform,
        (
            With<Combatant>,
            Without<PaladinOneShot>,
            Without<PaladinOneShotPiece>,
        ),
    >,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<PaladinOneShot>,
            Without<PaladinOneShotPiece>,
            Without<Combatant>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let cam_rotation = camera.iter().next().map(|c| c.rotation);
    for (entity, mut rig, mut transform, children) in rigs.iter_mut() {
        let Ok(unit) = units.get(rig.bearer) else {
            commands.entity(entity).despawn();
            continue;
        };
        let (from, to) = (rig.age, rig.age + dt);
        rig.age = to;
        if from >= oneshot_secs(rig.kind) {
            commands.entity(entity).despawn();
            continue;
        }
        let s = stature(rig.is_pet);
        transform.translation = body_point(0.0, unit.translation, rig.is_pet);

        // The particles: world-space, born at the unit as it stands now.
        let knobs = oneshot_knobs(rig.kind);
        let assets = emitters.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        for (i, layer) in oneshot_layers(rig.kind).iter().enumerate() {
            rig.carry[i] += layer.count_between(from, to, knobs);
            if rig.carry[i] < 1.0 {
                continue;
            }
            let e = &layer.emitter;
            let palette = assets.palette(&mut materials, e);
            let quad = assets.quad();
            let anchor = body_point(layer.height, unit.translation, rig.is_pet);
            while rig.carry[i] >= 1.0 {
                rig.carry[i] -= 1.0;
                let seed = entity
                    .index()
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(rig.emitted.wrapping_mul(0x85EB_CA6B));
                rig.emitted = rig.emitted.wrapping_add(1);
                let (offset, velocity) = oneshot_launch(layer, to, seed);
                let facing = match e.sprite {
                    // A star lies at the random roll the bench gives it.
                    EmitterSprite::Flare => ParticleFacing::Spin {
                        mrad_per_sec: 0,
                        start_mrad: (draw(seed, 8) * TAU * 1000.0) as i16,
                    },
                    EmitterSprite::Streak => ParticleFacing::Streak,
                    _ => ParticleFacing::Camera,
                };
                let size = e.size.scaled(s);
                let mut particle = commands.spawn((
                    ClientParticle {
                        age: 0.0,
                        life: e.life,
                        velocity: velocity * s,
                        gravity: 0.0,
                        size,
                        mid: e.mid,
                        palette: palette.clone(),
                        step: 0,
                        facing,
                        owned: false,
                    },
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(palette[0].clone()),
                    Transform::from_translation(anchor + offset * s).with_scale(size.birth_scale()),
                    NotShadowCaster,
                    PlayMatchEntity,
                ));
                if layer.drag > 0.0 {
                    particle.insert(ParticleDrag {
                        per_sec: layer.drag,
                    });
                }
            }
        }

        // The pieces: posed on the one-shot's own clock, at its knobs.
        for child in children.into_iter().flat_map(|c| c.iter()) {
            let Ok((piece, mut part, material)) = pieces.get_mut(child) else {
                continue;
            };
            let k = oneshot_knobs(piece_kind(piece.role));
            let pose = piece_pose(piece.role, to / k.duration);
            part.translation = Vec3::new(pose.at.x * k.size, pose.at.y, pose.at.z * k.size) * s;
            let size = pose.size * k.size * s;
            part.scale = match pose.facing {
                PieceFacing::Arrow { .. } => Vec3::new(size.x, 1.0, size.y),
                _ => size.extend(1.0),
            }
            .max(Vec3::splat(1e-3));
            part.rotation = piece_rotation(pose.facing, transform.rotation, cam_rotation);
            if let Some(material) = materials.get_mut(&material.0) {
                material.base_color = Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2])
                    .with_alpha((pose.alpha * k.opacity).clamp(0.0, 1.0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One emitter as the signed-off bench carries it.
    struct Bench {
        name: &'static str,
        sprite: EmitterSprite,
        rate: Option<f32>,
        window: (f32, f32),
        life: f32,
        speed: f32,
        area: Option<(f32, f32)>,
        color: [[u8; 3]; 3],
        alpha: [f32; 3],
        radii: [f32; 2],
        mid: f32,
        height: f32,
        launch: OneShotLaunch,
        drag: f32,
    }

    fn check(layers: &[OneShotLayer], bench: &[Bench]) {
        assert_eq!(layers.len(), bench.len());
        for (layer, b) in layers.iter().zip(bench) {
            let (e, name) = (&layer.emitter, b.name);
            assert!(e.name.ends_with(name), "{} is not the {name}", e.name);
            assert!(layer.on, "{name}: every client layer is kept");
            assert!(e.additive, "{name}: blend 4");
            assert_eq!(e.sprite, b.sprite, "{name} sprite");
            if let Some(rate) = b.rate {
                assert_eq!(e.rate, rate, "{name} rate");
                assert!(e.track.is_none(), "{name} holds one rate");
            }
            assert_eq!(e.window, Some(b.window), "{name} window");
            assert_eq!(e.life, b.life, "{name} life");
            assert_eq!(e.speed, b.speed, "{name} speed");
            assert_eq!(e.column, b.area, "{name} area");
            assert_eq!(e.color, b.color, "{name} colour keys");
            assert_eq!(e.alpha, b.alpha, "{name} alpha keys");
            assert_eq!(e.mid, b.mid, "{name} mid");
            let d = e.size.diameter_keys();
            assert!((d[0] - 2.0 * b.radii[0]).abs() < 1e-6, "{name} birth size");
            assert!((d[2] - 2.0 * b.radii[1]).abs() < 1e-6, "{name} death size");
            // A two-key ramp stays straight through its middle key.
            let straight = 2.0 * (b.radii[0] + (b.radii[1] - b.radii[0]) * b.mid);
            assert!((d[1] - straight).abs() < 1e-5, "{name} middle size");
            assert!((layer.height - b.height).abs() < 1e-5, "{name} height");
            assert_eq!(layer.launch, b.launch, "{name} launch");
            assert!((layer.drag - b.drag).abs() < 1e-6, "{name} drag");
        }
    }

    const SPHERE: OneShotLaunch = OneShotLaunch::Sphere;
    const UP: OneShotLaunch = OneShotLaunch::Vertical { speed_var: 0.0 };

    #[test]
    fn every_knob_is_the_clients() {
        for kind in PaladinOneShotKind::ALL {
            assert_eq!(oneshot_knobs(kind), CLIENT_KNOBS, "{kind:?}");
        }
    }

    #[test]
    fn divine_favor_is_the_benchs_innerfocus_burst() {
        check(
            &DIVINE_FAVOR_LAYERS,
            &[
                Bench {
                    name: "flare",
                    sprite: EmitterSprite::Flare,
                    rate: None,
                    window: (0.0, 0.101),
                    life: 1.0,
                    speed: 2.22,
                    area: Some((0.28, 0.28)),
                    color: [[64, 121, 251], [255, 255, 0], [220, 0, 0]],
                    alpha: [1.0, 1.0, 0.0],
                    radii: [0.25, 0.12],
                    mid: 0.5,
                    height: 1.45,
                    launch: SPHERE,
                    drag: 0.0,
                },
                Bench {
                    name: "violet streaks",
                    sprite: EmitterSprite::Streak,
                    rate: Some(60.0),
                    window: (0.0, 0.4),
                    life: 0.8,
                    speed: 3.33,
                    area: None,
                    color: [[88, 0, 248], [255, 0, 0], [98, 181, 239]],
                    alpha: [0.0, 1.0, 0.0],
                    radii: [0.15, 0.08],
                    mid: 0.5,
                    height: 1.45,
                    launch: SPHERE,
                    drag: 0.0,
                },
                Bench {
                    name: "blue points",
                    sprite: EmitterSprite::Glow,
                    rate: Some(280.0),
                    window: (0.0, 0.4),
                    life: 0.8,
                    speed: 0.56,
                    area: Some((0.72, 0.72)),
                    color: [[16, 29, 126], [44, 155, 225], [185, 172, 223]],
                    alpha: [1.0, 1.0, 0.0],
                    radii: [0.12, 0.05],
                    mid: 0.5,
                    height: 1.45,
                    launch: SPHERE,
                    drag: 0.0,
                },
            ],
        );
        // The flare is 100/s for 60 ms (plus the bench's millisecond ramps).
        let n = DIVINE_FAVOR_LAYERS[0].count_between(0.0, 1.0, CLIENT_KNOBS);
        assert!((n - 6.0).abs() < 0.01, "{n} flares");
    }

    #[test]
    fn aura_mastery_is_the_benchs_holyward() {
        check(
            &AURA_MASTERY_LAYERS,
            &[
                Bench {
                    name: "gold ripples",
                    sprite: EmitterSprite::Glow,
                    rate: Some(10.8),
                    window: (0.0, 3.0),
                    life: 1.0,
                    speed: 0.06,
                    area: Some((0.1, 0.0)),
                    color: [[253, 221, 94], [240, 213, 60], [253, 222, 13]],
                    alpha: [0.4, 0.38, 0.0],
                    radii: [0.4, 1.2],
                    mid: 0.5,
                    height: 1.45,
                    launch: SPHERE,
                    drag: 0.0,
                },
                Bench {
                    name: "star burst",
                    sprite: EmitterSprite::Flare,
                    rate: Some(45.0),
                    window: (0.067, 0.6),
                    life: 0.75,
                    speed: 2.56,
                    area: None,
                    color: [WHITE, [243, 200, 27], [255, 245, 191]],
                    alpha: [1.0, 0.68, 0.0],
                    radii: [0.22, 0.1],
                    mid: 0.5,
                    height: 1.45,
                    launch: SPHERE,
                    drag: 3.0 * 3.35,
                },
                Bench {
                    name: "amber glows",
                    sprite: EmitterSprite::Glow,
                    rate: Some(7.5),
                    window: (0.0, 3.0),
                    life: 1.2,
                    speed: 0.06,
                    area: None,
                    color: [[228, 159, 49], [232, 166, 25], [196, 154, 25]],
                    alpha: [0.5, 0.65, 0.0],
                    radii: [0.6, 0.9],
                    mid: 0.5,
                    height: 1.45,
                    launch: SPHERE,
                    drag: 0.0,
                },
                Bench {
                    name: "star ring",
                    sprite: EmitterSprite::Flare,
                    rate: Some(61.0),
                    window: (0.0, 3.0),
                    life: 0.81,
                    speed: 0.17,
                    area: None,
                    color: [[250, 225, 57], WHITE, WHITE],
                    alpha: [0.51, 0.72, 0.0],
                    radii: [0.14, 0.08],
                    mid: 0.5,
                    height: 1.45,
                    launch: OneShotLaunch::Ring {
                        radius: 1.0,
                        rev_per_sec: 0.5,
                    },
                    drag: 0.0,
                },
            ],
        );
    }

    #[test]
    fn blessing_of_sacrifice_is_the_benchs_red_impact_at_both_ends() {
        for layers in [&SACRIFICE_ALLY_LAYERS, &SACRIFICE_PALADIN_LAYERS] {
            check(
                layers,
                &[
                    Bench {
                        name: "falling motes",
                        sprite: EmitterSprite::Blob,
                        rate: Some(75.0),
                        window: (0.0, 1.9),
                        life: 1.5,
                        speed: -1.67,
                        area: Some((1.78, 0.4)),
                        color: [[42, 0, 0], [223, 0, 0], [255, 60, 0]],
                        alpha: [0.0, 1.0, 0.0],
                        radii: [0.12, 0.08],
                        mid: 0.5,
                        height: 1.0,
                        launch: UP,
                        drag: 0.0,
                    },
                    Bench {
                        name: "red streaks",
                        sprite: EmitterSprite::Streak,
                        // Half the bench's 66/s at each of the two ends.
                        rate: Some(33.0),
                        window: (0.0, 1.9),
                        life: 0.7,
                        speed: 6.1,
                        area: None,
                        color: [[124, 0, 0], [255, 12, 0], [248, 111, 0]],
                        alpha: [0.0, 0.25, 0.0],
                        radii: [0.12, 0.06],
                        mid: 0.5,
                        height: 1.28,
                        launch: SPHERE,
                        drag: 0.0,
                    },
                ],
            );
        }
    }

    #[test]
    fn infusion_of_light_is_the_benchs_halo() {
        check(
            &INFUSION_LAYERS,
            &[
                Bench {
                    name: "overhead glows",
                    sprite: EmitterSprite::Glow,
                    rate: None,
                    window: (0.0, 1.4),
                    life: 0.7,
                    speed: 0.0,
                    area: None,
                    color: [[251, 243, 157], [241, 189, 49], [236, 162, 44]],
                    alpha: [0.0, 0.46, 0.0],
                    radii: [0.35, 0.6],
                    mid: 0.35,
                    height: 2.5,
                    launch: UP,
                    drag: 0.0,
                },
                Bench {
                    name: "opening flash",
                    sprite: EmitterSprite::Glow,
                    rate: None,
                    window: (0.0, 0.067),
                    life: 0.4,
                    speed: 0.0,
                    area: None,
                    color: [WHITE, [237, 192, 72], WHITE],
                    alpha: [1.0, 0.64, 0.0],
                    radii: [0.5, 0.3],
                    mid: 0.5,
                    height: 2.49,
                    launch: UP,
                    drag: 0.0,
                },
                Bench {
                    name: "chest runes",
                    sprite: EmitterSprite::Flare,
                    rate: None,
                    window: (0.0, 0.534),
                    life: 0.6,
                    speed: 1.11,
                    area: Some((0.1, 0.0)),
                    color: [WHITE, [253, 218, 112], [244, 252, 255]],
                    alpha: [0.0, 0.78, 0.0],
                    radii: [0.25, 0.2],
                    mid: 0.5,
                    height: 1.4,
                    launch: OneShotLaunch::Vertical { speed_var: 2.0 },
                    drag: 0.0,
                },
                Bench {
                    name: "flare bursts",
                    sprite: EmitterSprite::Streak,
                    rate: None,
                    window: (0.0, 0.867),
                    life: 0.45,
                    speed: 1.11,
                    area: Some((0.5, 1.6)),
                    color: [[254, 253, 242], [233, 193, 32], WHITE],
                    alpha: [0.0, 0.54, 0.0],
                    radii: [0.18, 0.1],
                    mid: 0.5,
                    height: 2.9,
                    launch: SPHERE,
                    drag: 0.0,
                },
            ],
        );
        // Every emitter has stopped by 1.4 s.
        for layer in &INFUSION_LAYERS {
            assert_eq!(layer.count_between(1.4, 5.0, CLIENT_KNOBS), 0.0);
        }
        // The rates as the bench keys them: 15/s glows to 1.37 s, about five
        // flash particles, 44/s runes to 0.5 s, and the two bursts.
        let n = |i: usize, t0, t1| INFUSION_LAYERS[i].count_between(t0, t1, CLIENT_KNOBS);
        assert!((n(0, 0.0, 1.367) - 15.0 * 1.367).abs() < 1e-3);
        assert!((n(1, 0.0, 1.0) - 0.5 * 150.0 * 0.067).abs() < 1e-3);
        assert!((n(2, 0.0, 0.5) - 22.0).abs() < 1e-3);
        assert!((n(3, 0.0, 0.234) - 0.5 * 111.0 * 0.234).abs() < 1e-3);
        assert_eq!(n(3, 0.234, 0.234), 0.0);
    }

    #[test]
    fn the_art_of_war_is_the_benchs_spark_ring() {
        check(
            &ART_OF_WAR_LAYERS,
            &[
                Bench {
                    name: "gold glows",
                    sprite: EmitterSprite::Glow,
                    rate: None,
                    window: (0.0, 0.867),
                    life: 0.7,
                    speed: 0.0,
                    area: Some((0.14, 0.0)),
                    color: [[244, 235, 141], [233, 193, 32], WHITE],
                    alpha: [1.0, 0.36, 0.0],
                    radii: [0.4, 0.3],
                    mid: 0.5,
                    height: 1.45,
                    launch: UP,
                    drag: 0.0,
                },
                Bench {
                    name: "runes",
                    sprite: EmitterSprite::Flare,
                    rate: None,
                    window: (0.0, 0.5),
                    life: 0.7,
                    speed: 0.0,
                    area: None,
                    color: [WHITE, WHITE, WHITE],
                    alpha: [0.0, 0.38, 0.0],
                    radii: [0.4, 0.4],
                    mid: 0.5,
                    height: 1.45,
                    launch: UP,
                    drag: 0.0,
                },
                Bench {
                    name: "burst above the chest",
                    sprite: EmitterSprite::Streak,
                    rate: None,
                    window: (0.0, 0.2),
                    life: 0.4,
                    speed: 1.11,
                    area: Some((0.6, 0.0)),
                    color: [[254, 253, 242], [233, 193, 32], WHITE],
                    alpha: [0.0, 0.54, 0.0],
                    radii: [0.16, 0.08],
                    mid: 0.5,
                    height: 2.78,
                    launch: SPHERE,
                    drag: 0.0,
                },
            ],
        );
    }

    #[test]
    fn every_emitter_has_a_unique_palette_key() {
        let mut names: Vec<&str> = [
            &DIVINE_FAVOR_LAYERS[..],
            &AURA_MASTERY_LAYERS[..],
            &SACRIFICE_ALLY_LAYERS[..],
            &INFUSION_LAYERS[..],
            &ART_OF_WAR_LAYERS[..],
        ]
        .iter()
        .flat_map(|t| t.iter().map(|l| l.emitter.name))
        .collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
        // The two Sacrifice ends play the same emitters.
        assert_eq!(SACRIFICE_ALLY_LAYERS, SACRIFICE_PALADIN_LAYERS);
    }

    #[test]
    fn a_tracked_layer_spans_its_track() {
        for kind in PaladinOneShotKind::ALL {
            for layer in oneshot_layers(kind) {
                let e = &layer.emitter;
                if let Some(track) = e.track {
                    assert_eq!(e.window, Some((track[0].0, track[track.len() - 1].0)));
                }
                // No layer emits past its one-shot's sequence.
                assert_eq!(
                    layer.count_between(oneshot_secs(kind), 60.0, CLIENT_KNOBS),
                    0.0,
                    "{}",
                    e.name
                );
            }
        }
    }

    /// Each launch stays in its box, at its speed and direction.
    #[test]
    fn launches_keep_their_shape() {
        // Sphere: the speed, every direction.
        let points = &DIVINE_FAVOR_LAYERS[2];
        let (mut up, mut down) = (false, false);
        for seed in 0..400 {
            let (at, v) = oneshot_launch(points, 0.0, seed);
            assert!(at.x.abs() <= 0.36 && at.y.abs() <= 0.36 && at.z.abs() <= 0.36);
            assert!((v.length() - 0.56).abs() < 1e-4);
            up |= v.y > 0.4;
            down |= v.y < -0.4;
        }
        assert!(up && down, "the sphere launches every way");
        // The Sacrifice motes fall straight down at 1.67 yd/s from a flat box.
        let motes = &SACRIFICE_ALLY_LAYERS[0];
        for seed in 0..200 {
            let (at, v) = oneshot_launch(motes, 0.0, seed);
            assert!(at.x.abs() <= 0.89 && at.z.abs() <= 0.89 && at.y.abs() <= 0.2);
            assert!((v - Vec3::new(0.0, -1.67, 0.0)).length() < 1e-5, "{v}");
        }
        // The Infusion runes rise AND fall: 1.11 × (1 ± 2) yd/s.
        let runes = &INFUSION_LAYERS[2];
        let speeds: Vec<f32> = (0..400)
            .map(|s| oneshot_launch(runes, 0.0, s).1.y)
            .collect();
        assert!(speeds
            .iter()
            .all(|v| (-1.11 - 1e-4..=3.33 + 1e-4).contains(v)));
        assert!(speeds.iter().any(|v| *v < -0.5) && speeds.iter().any(|v| *v > 2.5));
        // The star ring: born on three points of the 1.0 yd ring, turning
        // half a turn a second.
        let ring = &AURA_MASTERY_LAYERS[3];
        for age in [0.0, 0.5, 1.3] {
            let mut angles: Vec<i32> = (0..300)
                .map(|seed| {
                    let (at, _) = oneshot_launch(ring, age, seed);
                    assert!((at.xz().length() - 1.0).abs() < 1e-4);
                    assert_eq!(at.y, 0.0);
                    let a = (at.z.atan2(at.x) - age * 0.5 * TAU).rem_euclid(TAU);
                    (a / (TAU / 3.0)).round() as i32 % 3
                })
                .collect();
            angles.sort();
            angles.dedup();
            assert_eq!(angles, vec![0, 1, 2], "the three ring points at {age}");
        }
    }

    #[test]
    fn the_bench_hash_is_the_benchs() {
        // hh(1) = fract(sin(12.9898) × 43758.5453), as JavaScript computes it.
        let x = 12.9898_f64.sin() * 43758.5453;
        assert_eq!(bench_hash(1), (x - x.floor()) as f32);
        for n in 0..200 {
            assert!((0.0..1.0).contains(&bench_hash(n)));
        }
    }

    #[test]
    fn the_spark_ring_opens_from_a_tenth_of_a_yard() {
        // Each spark lives 0.7 s where it was born; the ring at its birth
        // opens 0.1 -> 1.0 over 0.9 s, drawn at half plus 0.05.
        let mut early = Vec::new();
        let mut late = Vec::new();
        for index in 0..AOW_SPARKS {
            let born = bench_hash(index as u32) * AOW_SPARK_BIRTH_SPAN;
            assert!(aow_spark(index, born - 0.01).is_none(), "unborn");
            assert!(
                aow_spark(index, born + AOW_SPARK_LIFE + 0.01).is_none(),
                "spent"
            );
            let pose = aow_spark(index, born + 0.35).unwrap();
            let r = pose.at.xz().length();
            let want = (0.1 + 0.9 * born / 0.9) * 0.5 + 0.05;
            assert!((r - want).abs() < 1e-4, "{r} vs {want}");
            assert!(
                (pose.alpha - 0.48 * 1.6).abs() < 1e-4,
                "peak alpha mid-life"
            );
            assert_eq!(pose.rgb, [229, 148, 62], "orange mid-life");
            if born < 0.2 {
                early.push(r);
            } else if born > 0.6 {
                late.push(r);
            }
        }
        assert!(!early.is_empty() && !late.is_empty());
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean(&late) > mean(&early) + 0.15, "the ring opens");
    }

    #[test]
    fn the_retribution_disc_grows_on_the_benchs_tracks() {
        let glyph = |age| piece_pose(OneShotPieceRole::RetributionGlyph, age);
        assert!((glyph(0.0).size.x - 2.0 * 0.747).abs() < 1e-4);
        assert!((glyph(0.4).size.x - 2.0 * 0.747 * 1.31).abs() < 1e-4);
        assert!(
            (glyph(1.2).size.x - 2.0 * 0.747 * 1.31).abs() < 1e-4,
            "held"
        );
        assert!((glyph(0.3).alpha - 0.8).abs() < 1e-5);
        assert_eq!(glyph(1.034).alpha, 0.0);
        let arrow = |age| piece_pose(OneShotPieceRole::RetributionArrow { index: 0 }, age);
        let base = 0.8645 * 0.15_f32.cos();
        assert!((arrow(0.0).size.x - (0.8645 + 0.4 - base)).abs() < 1e-4);
        assert!((arrow(0.8).size.x - (0.8645 + 0.4 * 1.62 - base)).abs() < 1e-4);
        assert_eq!(arrow(0.4).alpha, 0.0, "the arrows wait for 0.5 s");
        assert!((arrow(0.9).alpha - 0.9).abs() < 1e-5);
        assert_eq!(arrow(1.3).alpha, 0.0);
        // The four arrows point out on the diagonals.
        for index in 0..4u8 {
            let PieceFacing::Arrow { angle } =
                piece_pose(OneShotPieceRole::RetributionArrow { index }, 0.9).facing
            else {
                panic!("an arrow")
            };
            assert!((angle - (index as f32 * TAU / 4.0 + FRAC_PI_4)).abs() < 1e-6);
        }
        let under = piece_pose(OneShotPieceRole::RetributionUnderlay, 0.5);
        assert!((under.size.x - 2.88).abs() < 1e-4, "2.9 yd across");
        assert!((under.alpha - 0.4 * 0.6).abs() < 1e-5);
    }

    #[test]
    fn the_ward_halo_and_shell_follow_their_weights() {
        let ward = |age| piece_pose(OneShotPieceRole::WardGlow, age);
        assert_eq!(ward(0.0).alpha, 0.0);
        assert!((ward(1.5).alpha - 0.6 * 0.55).abs() < 1e-5);
        assert_eq!(ward(3.0).alpha, 0.0);
        assert_eq!(ward(1.5).size, Vec2::splat(4.4));
        assert_eq!(ward(1.5).at.y, 1.45);
        let halo = |age| piece_pose(OneShotPieceRole::HaloRing, age);
        assert!((halo(1.0).alpha - 0.55 * 1.4).abs() < 1e-5);
        assert_eq!(halo(1.834).alpha, 0.0);
        assert!((halo(1.0).size.x - 0.48).abs() < 1e-5);
        assert_eq!(halo(1.0).at.y, 2.5);
        let shell = |age| piece_pose(OneShotPieceRole::ShellGlow, age);
        assert!((shell(1.0).alpha - 0.45).abs() < 1e-5);
        assert_eq!(shell(1.733).alpha, 0.0);
        assert!((shell(1.0).size - Vec2::new(2.7, 2.475)).length() < 1e-5);
    }

    #[test]
    fn a_landing_plays_where_the_client_draws_it() {
        let (paladin, ally) = (Entity::from_raw(1), Entity::from_raw(2));
        let plan = |k, bearer, caster| oneshot_plan(k, bearer, caster);
        use AuraLandingKind as L;
        use PaladinOneShotKind as P;
        assert_eq!(
            plan(L::DivineFavorBurst, paladin, Some(paladin)),
            (vec![(paladin, P::DivineFavor)], Some(paladin))
        );
        // Aura Mastery and Retribution Aura: the Paladin's own landing only.
        assert_eq!(
            plan(L::AuraMasteryWard, paladin, Some(paladin)),
            (vec![(paladin, P::AuraMastery)], Some(paladin))
        );
        assert_eq!(
            plan(L::AuraMasteryWard, ally, Some(paladin)),
            (vec![], None)
        );
        assert_eq!(
            plan(L::RetributionDisc, paladin, Some(paladin)),
            (vec![(paladin, P::Retribution)], None)
        );
        assert_eq!(
            plan(L::RetributionDisc, ally, Some(paladin)),
            (vec![], None)
        );
        // Blessing of Sacrifice: both ends, the Paladin's hands.
        assert_eq!(
            plan(L::SacrificeShell, ally, Some(paladin)),
            (
                vec![(ally, P::SacrificeAlly), (paladin, P::SacrificePaladin)],
                Some(paladin)
            )
        );
        // The procs play on their Paladin.
        assert_eq!(
            plan(L::InfusionHalo, paladin, Some(paladin)),
            (vec![(paladin, P::InfusionOfLight)], None)
        );
        assert_eq!(
            plan(L::ArtOfWarRing, paladin, Some(paladin)),
            (vec![(paladin, P::ArtOfWar)], None)
        );
        // Another renderer's landing plays nothing here.
        assert_eq!(
            plan(L::RejuvenationSwirl, ally, Some(paladin)),
            (vec![], None)
        );
    }

    /// The bench's glyph discs: eight ticks on the Sacrifice shell, sixteen on
    /// the Retribution disc, ten on the Art of War disc; turning 1.0, 0.5
    /// and 2.0 rad/s (its `t/1000`, `u/2000` and `u/500` over milliseconds).
    #[test]
    fn the_glyph_discs_are_the_benchs() {
        assert_eq!(
            (SHELL_GLYPH_TICKS, RETRIBUTION_GLYPH_TICKS, AOW_DISC_TICKS),
            (8, 16, 10)
        );
        for (role, spin) in [
            (OneShotPieceRole::ShellGlyph, 1.0),
            (OneShotPieceRole::RetributionGlyph, 0.5),
            (OneShotPieceRole::ArtOfWarDisc, 2.0),
        ] {
            for age in [0.0, 0.3, 1.1] {
                assert_eq!(
                    piece_pose(role, age).facing,
                    PieceFacing::Flat { turn: age * spin },
                    "{role:?} turns {spin} rad/s"
                );
            }
        }
        // The rings and the underlay lie still.
        for role in [
            OneShotPieceRole::ShellRing,
            OneShotPieceRole::RetributionRing,
            OneShotPieceRole::RetributionUnderlay,
            OneShotPieceRole::HaloRing,
        ] {
            assert_eq!(
                piece_pose(role, 0.7).facing,
                PieceFacing::Flat { turn: 0.0 }
            );
        }
    }

    /// Every piece's colour, as the bench draws it.
    #[test]
    fn every_piece_is_the_benchs_colour() {
        use OneShotPieceRole as R;
        for (role, rgb) in [
            (R::WardGlow, [255, 232, 130]),
            (R::ShellGlow, [220, 10, 0]),
            (R::ShellRing, [255, 54, 0]),
            (R::ShellGlyph, [243, 137, 0]),
            (R::RetributionGlyph, [252, 93, 12]),
            (R::RetributionRing, [255, 0, 0]),
            (R::RetributionArrow { index: 2 }, [255, 162, 0]),
            (R::RetributionUnderlay, [255, 199, 29]),
            (R::HaloRing, [237, 192, 72]),
            (R::HaloGlow, [255, 230, 150]),
            (R::ArtOfWarDisc, [221, 181, 57]),
            (R::ArtOfWarGlow, [255, 230, 140]),
        ] {
            for age in [0.0, 0.5, 1.2] {
                assert_eq!(piece_pose(role, age).rgb, rgb, "{role:?}");
            }
        }
        // A spark runs pale gold -> orange -> yellow over its life.
        let born = bench_hash(3) * AOW_SPARK_BIRTH_SPAN;
        let at = |ph: f32| aow_spark(3, born + ph * AOW_SPARK_LIFE).unwrap().rgb;
        assert_eq!(at(0.0), [255, 249, 181]);
        assert_eq!(at(0.5), [229, 148, 62]);
        assert_eq!(at(1.0), [242, 227, 65]);
    }

    /// A layer switched off emits nothing (every shipped layer is on by the
    /// ruling, so only a constructed one can show the gate).
    #[test]
    fn a_layer_switched_off_emits_nothing() {
        let on = DIVINE_FAVOR_LAYERS[1];
        assert!(on.count_between(0.0, 1.0, CLIENT_KNOBS) > 20.0);
        let off = OneShotLayer { on: false, ..on };
        assert_eq!(off.count_between(0.0, 1.0, CLIENT_KNOBS), 0.0);
    }

    /// The glyph texture carries its ticks: around the band between the two
    /// rings, alpha rises and falls `ticks` times.
    #[test]
    fn a_glyph_texture_carries_its_ticks() {
        for ticks in [8, 16, 10] {
            let image = oneshot_glyph_texture(ticks);
            let data = image.data.as_ref().unwrap();
            let px = PIECE_PX as usize;
            let alpha_at = |r: f32, a: f32| {
                let (x, y) = (r * a.cos(), r * a.sin());
                let ix = (((x + 1.0) * 0.5) * px as f32) as usize;
                let iy = (((y + 1.0) * 0.5) * px as f32) as usize;
                data[(iy.min(px - 1) * px + ix.min(px - 1)) * 4 + 3] as f32 / 255.0
            };
            let r = 0.965 * (1.0 + GLYPH_INNER) * 0.5;
            let samples: Vec<bool> = (0..720)
                .map(|i| alpha_at(r, i as f32 / 720.0 * TAU) > 0.5)
                .collect();
            let rises = (0..samples.len())
                .filter(|&i| samples[i] && !samples[(i + samples.len() - 1) % samples.len()])
                .count();
            assert_eq!(rises as u32, ticks, "{ticks} ticks");
        }
    }
}
