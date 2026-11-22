use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{body_capsule, held_in_front};
use super::druid_heals::{body_point, keyed, keyed_color};
use super::hit_reaction::{insert_flinch, PendingDepth};
use super::hunter_shots::draw;
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// Conflagrate — the client's hard fire detonation at the victim's chest
// ==============================================================================
//
// From the client data, joined in
// `docs/design/2026-10-17-warlock-talent-client-data.md` §5 (AS-334):
// Conflagrate's visual (5199) lands kit 4551, `fire_impactdd_high_chest.m2`,
// on the victim's chest (attachment 34) and plays the victim's CombatWound
// flinch (anim 9). One 1.0 s sequence and no mesh: eight sprite emitters, all
// inside the first 0.83 s — sparks and plumes jetting out, a fire ring and a
// shock ring flashing, yellow-orange puffs and pale smoke lingering to about
// 2 s. No `(7,8)` state, and no row of Immolate's: "Immolate is consumed" is
// Immolate's burn switching off as the burst lands, which the shipped burn
// already does (`cleanup_immolate_burns` despawns it the frame the aura goes).
// The cast side is our cast orb; the client's fire hands are not shipped.
//
// Played as the signed-off AS-334 Warlock Talent Visuals Bench draws it
// (https://claude.ai/artifact/KwBsy13gPub9sKmnKdYMgg, version 1792203924-603a,
// "Conflagrate" tab, ruled client-faithful with all layers on; source
// committed as `docs/design/benches/2026-10-17-warlock-talent-bench.html`). Its
// constants block, at the ruling, is the block below. The bench draws the
// client's eight emitters as SIX layers — five particle emitters (P4 and P5
// merged into one plume layer) and one custom layer drawing two rings (P1 and
// P6) — and its burn layer is a viewing aid standing in for our shipped burn:
//
//   sparks  P3  `flare` stars, 125/s for 0-333 ms, 2.78 yd/s, spinning.
//               No `blend`: `Emitter.draw` composites it `lighter` — additive.
//   plumes  P4+P5  soft glows with a tail, 200/s for 0-167 ms, 4.1 yd/s.
//               Additive.
//   glow    P0  soft clouds, 50/s for 0-833 ms, 0.33 yd/s, rising. Additive.
//   flame   P2  clouds, 30/s for 0-667 ms, 0.28 yd/s, rising.
//               `blend: 'alpha'`: source-over, so BLENDED.
//   smoke   P7  smoke, 20/s for 0-667 ms, 0.56 yd/s in a 1.2 rad cone up, born
//               in a 0.56 yd square, 2 s lives. `blend: 'alpha'`: BLENDED.
//   rings   P1+P6  `customDraw`: a flat soft fire disc growing to 0.97 yd and
//               a flat rune ring (a circle and eight ticks) growing to
//               2.22 yd, both at chest height, both through `groundDisc` /
//               `runeRing`, which composite `lighter` — additive.
//
// Where the card's prose and the bench disagree, the bench was signed off and
// wins: the plumes are one emitter at 4.1 yd/s (not 3.9 and 4.4 apart),
// mid-life (221,180,120) and at most 0.10 yd (not 0.14); the fire ring is a
// filled soft disc in one colour (245,150,0) at 0.6 of its alpha, drawn
// 120-900 ms (not a ring keyed (245,203,0) -> (100,0,0) over 167-233 ms); the
// shock ring is a thin rune ring in one colour (255,120,0), drawn 120-1200 ms
// (not a grey -> (255,162,0) -> (255,0,0) ring over 133-233 ms).
//
// ## Departures from the bench (each deliberate)
//
// 1. **The shock ring's line is world-wide.** The bench strokes it 2 screen
//    pixels wide however near the camera is; here it is a band
//    [`SHOCK_RING_LINE_WIDTH`] yards wide — the bench's 2 px at the match
//    camera (60 yd, FOV 45°, 1080p: about 21.7 px a yard).
// 2. **A star's white-hot centre is tinted.** The bench paints a star's core
//    white over its coloured rays; the sprite here is one shape in the
//    layer's colour, as Shadow Word: Pain's and Siphon Life's stars are.
// 3. **The streak is square-ended.** The bench strokes a plume's tail with a
//    round cap; here it is a flat quad of the same length and width.
// 4. **Follows in height too.** The bench's rings ride the victim in x and z
//    only; here the rig rides its chest, so a victim on a ramp keeps them.
// 5. **Held in front of the body.** The bench paints the body first and the
//    whole burst over it. Here the body is an `AlphaMode::Blend` mesh that
//    writes no depth, and the transparent pass sorts by each mesh's
//    translation, so a piece deeper than the body's centre — half of what is
//    born at the chest, and the back of the rings — is drawn first and the
//    body paints over it. Every particle, the fire disc and every piece of
//    the rune ring is therefore drawn on its own view ray, slid in front of
//    the victim's body and scaled by the same ratio (`body_hold.rs`, the rule
//    AS-347 wrote for Siphon Life): it projects exactly where the bench puts
//    it. A particle's true flight is `ConflagrateParticle::at`; a streak is
//    laid in the camera's plane at its particle's depth, with its projected
//    length, as the bench's screen line is.
//
// ## Lifecycle
//
// The burst is a landing. It plays out on a victim that dies under it — the
// killing blow included, since Conflagrate is a burst finisher and a landing
// retired on the kill would draw nothing on the cast that matters most — and
// on a polymorphed one (the empowered flash's precedent). A victim that is
// GONE retires the rig at once, its rings with it; particles already in the
// air live out their lives, since they never ride the victim (the bench's
// emitters have no `follow`). The rings do ride it, the rig standing at the
// victim's chest. Everything is a `PlayMatchEntity`, so a match that ends
// mid-burst takes all of it.
//
// ## At the match camera (60 yd, pitch 38.7°, FOV 45°)
//
// About 22 px a yard at 1080p: the shock ring is ~96 px across at full size,
// the fire disc ~42 px, a glow puff ~10 px, a smoke puff up to ~19 px; a spark
// is 1-3 px and a plume head 1-4 px with a ~5 px tail. Client size, by the
// ruling.
//
// House rules: spawn / animate / pose / age / billboard systems, `Res<Time>`,
// registered in `states/mod.rs` only (headless never runs any of it and stays
// byte-identical by construction), no `game_rng` — all scatter is hash-seeded
// off the rig's entity index and emission count.

// ── Bench sign-off constants (AS-334, version 1792203924-603a) ───────────────

// Conflagrate landing: fire_impactdd_high_chest.m2 (kit 4551) on the victim's chest, plus the CombatWound flinch. AS-334 bench.
pub const CONFLAGRATE_SPARKS: bool = true;
pub const CONFLAGRATE_PLUMES: bool = true;
pub const CONFLAGRATE_GLOW_PUFFS: bool = true;
pub const CONFLAGRATE_FLAME_PUFFS: bool = true;
pub const CONFLAGRATE_SMOKE: bool = true;
pub const CONFLAGRATE_RINGS: bool = true; // fire ring 0.97 yd, shock ring 2.22 yd
pub const CONFLAGRATE_SIZE_MUL: f32 = 1.00;
pub const CONFLAGRATE_DENSITY_MUL: f32 = 1.00;
// Immolate consumed: the shipped burn despawns with the aura; no extra visual.

/// The bench's chest anchor, above the floor (`ANCH.chest`; attachment 34).
pub const CONFLAGRATE_CHEST_ABOVE_FLOOR: f32 = 1.45;

// ── Rendering constants ──────────────────────────────────────────────────────

/// Emissive gain of an additive piece over its colour — the house's 2-4x glow
/// convention (`shadow_word_pain.rs` uses the same).
pub const CONFLAGRATE_GLOW: f32 = 2.5;
/// The bench draws nothing below this alpha (`a <= 0.003` returns early).
pub const CONFLAGRATE_ALPHA_FLOOR: f32 = 0.003;
/// A streak is drawn only above this speed, yd/s (the bench's `sp > 0.2`).
pub const CONFLAGRATE_STREAK_MIN_SPEED: f32 = 0.2;
/// The bench strokes a streak at 0.85 of the sprite's alpha.
pub const CONFLAGRATE_STREAK_ALPHA: f32 = 0.85;
/// The bench's streak is 0.7 of the sprite's radius wide.
pub const CONFLAGRATE_STREAK_WIDTH: f32 = 0.7;
/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 64;
/// The fire disc's texture, pixels on a side.
const DISC_PX: u32 = 128;

// ── The emitter model (the bench's `Emitter`) ────────────────────────────────

/// What a particle's sprite is drawn with — the bench's sprite `kind`s.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflagrateSprite {
    /// `star`: a four-point star, rays a radius long and 0.12 of it thick,
    /// over a core fading out by 0.45 of the radius (`flare`).
    Star,
    /// `glow`: a soft round light, 0.45 at mid-radius.
    Glow,
    /// `cloud`: a fuller puff, 0.9 at its centre and 0.6 at mid-radius
    /// (`tooncloud64`).
    Cloud,
    /// `smoke`: a softer puff, 0.85 at its centre and 0.5 at mid-radius
    /// (`toonsmoke16`).
    Smoke,
}

impl ConflagrateSprite {
    /// The sprite's alpha over its radius, 0 at the centre to 1 at the rim:
    /// the bench's radial gradient stops (`sprite`'s `soft` triple), or the
    /// star's shape. `angle` is the bearing about the centre.
    pub fn alpha_at(self, r: f32, angle: f32) -> f32 {
        let soft = |stops: [f32; 2]| keyed(&[(0.0, stops[0]), (0.45, stops[1]), (1.0, 0.0)], r);
        match self {
            ConflagrateSprite::Glow => soft([1.0, 0.45]),
            ConflagrateSprite::Cloud => soft([0.9, 0.6]),
            ConflagrateSprite::Smoke => soft([0.85, 0.5]),
            ConflagrateSprite::Star => {
                // Two ellipses crossed (radius 1, half-thickness 0.12), and
                // the core's gradient, 1 at the centre to 0 at 0.45.
                let (x, y) = (r * angle.cos(), r * angle.sin());
                let in_ray = |a: f32, b: f32| a * a + (b / 0.12) * (b / 0.12) <= 1.0;
                let ray = if in_ray(x, y) || in_ray(y, x) {
                    1.0
                } else {
                    0.0
                };
                let core = (1.0 - r / 0.45).clamp(0.0, 1.0);
                (ray + core).min(1.0)
            }
        }
    }
}

/// How a particle launches.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ConflagrateLaunch {
    /// The bench's `vRange >= 3.1`: a direction drawn in the cube and
    /// normalised — every way out of the chest.
    Sphere,
    /// The bench's narrower `vRange`: a polar angle up to `half_angle` off
    /// straight up, at any bearing.
    Cone { half_angle: f32 },
}

/// One emitter, as the bench's layer record carries it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ConflagrateEmitter {
    /// Unique key.
    pub name: &'static str,
    pub sprite: ConflagrateSprite,
    /// `lighter` (additive) when `true`; `source-over` (blended, which can
    /// darken what is behind it) when `false`.
    pub additive: bool,
    pub launch: ConflagrateLaunch,
    /// Launch speed, yd/s.
    pub speed: f32,
    /// Downward acceleration, yd/s². Negative RISES.
    pub gravity: f32,
    pub life: f32,
    /// Particles per second while the window is open.
    pub rate: f32,
    /// `[start, end)` of the emission window, milliseconds from the landing
    /// (the bench's `win`, measured from its landing at 1000 ms).
    pub window_ms: (f32, f32),
    /// Side of the square, in the ground plane, the particles are born in.
    pub area: f32,
    /// Colour, alpha and scale tracks over the particle's normalised life.
    pub color: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// Sprite RADIUS, yards.
    pub scale: &'static [(f32, f32)],
    /// Tail length factor; `0.0` draws no streak.
    pub tail: f32,
    /// Sprite spin, revolutions per second.
    pub spin: f32,
}

impl ConflagrateEmitter {
    /// Whether the emitter is emitting `ms` after the landing (`windowOn`).
    pub fn emitting_at(&self, ms: f32) -> bool {
        ms >= self.window_ms.0 && ms < self.window_ms.1
    }
}

/// `fire_impactdd_high_chest.m2`'s emitters as the bench's five particle
/// layers, in its order.
pub const CONFLAGRATE_EMITTERS: [ConflagrateEmitter; 5] = [
    // P3 flare: the spark spray.
    ConflagrateEmitter {
        name: "fire_impactdd_high_chest/P3 spark spray",
        sprite: ConflagrateSprite::Star,
        additive: true,
        launch: ConflagrateLaunch::Sphere,
        speed: 2.78,
        gravity: 0.0,
        life: 1.0,
        rate: 125.0,
        window_ms: (0.0, 333.0),
        area: 0.0,
        color: &[
            (0.0, [255, 209, 0]),
            (0.5, [255, 159, 35]),
            (1.0, [101, 0, 0]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.5, 0.07), (1.0, 0.03)],
        tail: 0.0,
        spin: -0.3,
    },
    // P4 + P5 fireplume64: the plume jets.
    ConflagrateEmitter {
        name: "fire_impactdd_high_chest/P4+P5 plume jets",
        sprite: ConflagrateSprite::Glow,
        additive: true,
        launch: ConflagrateLaunch::Sphere,
        speed: 4.1,
        gravity: 0.0,
        life: 0.7,
        rate: 200.0,
        window_ms: (0.0, 167.0),
        area: 0.0,
        color: &[
            (0.0, [78, 0, 0]),
            (0.5, [221, 180, 120]),
            (1.0, [213, 95, 0]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.5, 0.1), (1.0, 0.03)],
        tail: 3.0,
        spin: 0.0,
    },
    // P0 tooncloud64_soft: the rising glow puffs.
    ConflagrateEmitter {
        name: "fire_impactdd_high_chest/P0 glow puffs",
        sprite: ConflagrateSprite::Cloud,
        additive: true,
        launch: ConflagrateLaunch::Sphere,
        speed: 0.33,
        gravity: -0.56,
        life: 1.2,
        rate: 50.0,
        window_ms: (0.0, 833.0),
        area: 0.0,
        color: &[
            (0.0, [255, 222, 0]),
            (0.5, [206, 129, 47]),
            (1.0, [91, 91, 91]),
        ],
        alpha: &[(0.0, 0.8), (1.0, 0.0)],
        scale: &[(0.0, 0.22), (0.5, 0.22), (1.0, 0.03)],
        tail: 0.0,
        spin: 0.0,
    },
    // P2 tooncloud64: the opaque flame puffs.
    ConflagrateEmitter {
        name: "fire_impactdd_high_chest/P2 flame puffs",
        sprite: ConflagrateSprite::Cloud,
        additive: false,
        launch: ConflagrateLaunch::Sphere,
        speed: 0.28,
        gravity: -0.56,
        life: 1.25,
        rate: 30.0,
        window_ms: (0.0, 667.0),
        area: 0.0,
        color: &[
            (0.0, [255, 222, 0]),
            (0.5, [236, 148, 54]),
            (1.0, [120, 120, 120]),
        ],
        alpha: &[(0.0, 0.9), (1.0, 0.0)],
        scale: &[(0.0, 0.28), (0.5, 0.28), (1.0, 0.03)],
        tail: 0.0,
        spin: 0.0,
    },
    // P7 toonsmoke16: the pale smoke.
    ConflagrateEmitter {
        name: "fire_impactdd_high_chest/P7 pale smoke",
        sprite: ConflagrateSprite::Smoke,
        additive: false,
        launch: ConflagrateLaunch::Cone { half_angle: 1.2 },
        speed: 0.56,
        gravity: 0.0,
        life: 2.0,
        rate: 20.0,
        window_ms: (0.0, 667.0),
        area: 0.56,
        color: &[(0.0, [235, 220, 174]), (1.0, [245, 245, 240])],
        alpha: &[(0.0, 0.6), (1.0, 0.0)],
        scale: &[(0.0, 0.33), (0.5, 0.44), (1.0, 0.03)],
        tail: 0.0,
        spin: 0.0,
    },
];

/// Which of [`CONFLAGRATE_EMITTERS`] are kept layers, in table order.
pub const CONFLAGRATE_LAYERS: [bool; 5] = [
    CONFLAGRATE_SPARKS,
    CONFLAGRATE_PLUMES,
    CONFLAGRATE_GLOW_PUFFS,
    CONFLAGRATE_FLAME_PUFFS,
    CONFLAGRATE_SMOKE,
];

// ── The rings (the bench's `customDraw`) ─────────────────────────────────────

/// The rings are drawn only from here to [`CONFLAGRATE_RINGS_TO_MS`] after
/// the landing.
pub const CONFLAGRATE_RINGS_FROM_MS: f32 = 120.0;
pub const CONFLAGRATE_RINGS_TO_MS: f32 = 1300.0;
/// The fire disc's radius, yards, keyed in milliseconds (P1 `firering128`).
pub const FIRE_DISC_RADIUS_KEYS: [(f32, f32); 4] =
    [(120.0, 0.0), (167.0, 0.03), (400.0, 0.83), (900.0, 0.97)];
/// Its alpha keys; the bench draws the disc at 0.6 of them.
pub const FIRE_DISC_ALPHA_KEYS: [(f32, f32); 4] =
    [(120.0, 0.0), (167.0, 1.0), (600.0, 0.6), (900.0, 0.0)];
pub const FIRE_DISC_ALPHA: f32 = 0.6;
pub const FIRE_DISC_COLOR: [u8; 3] = [245, 150, 0];
/// The disc's soft radial falloff over its radius (`groundDisc`'s stops).
pub const FIRE_DISC_FALLOFF: [(f32, f32); 3] = [(0.0, 1.0), (0.55, 0.55), (1.0, 0.0)];
/// The shock ring's radius, yards, keyed in milliseconds (P6 `shockwave10`).
pub const SHOCK_RING_RADIUS_KEYS: [(f32, f32); 4] =
    [(120.0, 0.0), (133.0, 0.03), (600.0, 1.39), (1200.0, 2.22)];
pub const SHOCK_RING_ALPHA_KEYS: [(f32, f32); 4] =
    [(120.0, 0.0), (133.0, 0.9), (700.0, 0.5), (1200.0, 0.0)];
pub const SHOCK_RING_COLOR: [u8; 3] = [255, 120, 0];
/// The ring is a 48-sided circle and eight ticks from 0.82 of its radius out
/// to it (`runeRing`).
pub const SHOCK_RING_SEGMENTS: usize = 48;
pub const SHOCK_RING_TICKS: usize = 8;
pub const SHOCK_RING_TICK_INNER: f32 = 0.82;
/// The ring's line width, yards: the bench's 2 px stroke at the match camera
/// (departure 1).
pub const SHOCK_RING_LINE_WIDTH: f32 = 0.092;

/// One of the two rings as it is drawn: its radius and its alpha.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RingDraw {
    pub radius: f32,
    pub alpha: f32,
}

/// The two rings `ms` after the landing, for a victim of `stature` — each
/// `None` where the bench draws nothing.
pub fn conflagrate_rings(ms: f32, stature: f32) -> (Option<RingDraw>, Option<RingDraw>) {
    if !CONFLAGRATE_RINGS || !(CONFLAGRATE_RINGS_FROM_MS..=CONFLAGRATE_RINGS_TO_MS).contains(&ms) {
        return (None, None);
    }
    let scale = CONFLAGRATE_SIZE_MUL * stature;
    let fire = RingDraw {
        radius: keyed(&FIRE_DISC_RADIUS_KEYS, ms) * scale,
        alpha: keyed(&FIRE_DISC_ALPHA_KEYS, ms) * FIRE_DISC_ALPHA,
    };
    let shock = RingDraw {
        radius: keyed(&SHOCK_RING_RADIUS_KEYS, ms) * scale,
        alpha: keyed(&SHOCK_RING_ALPHA_KEYS, ms),
    };
    (
        (fire.alpha > CONFLAGRATE_ALPHA_FLOOR && fire.radius > 0.001).then_some(fire),
        (shock.alpha > CONFLAGRATE_ALPHA_FLOOR).then_some(shock),
    )
}

/// The rig's whole life: until its last particle is born and its rings have
/// gone. Its particles live on after it.
pub fn conflagrate_rig_life() -> f32 {
    let emission = CONFLAGRATE_EMITTERS
        .iter()
        .map(|e| e.window_ms.1)
        .fold(0.0_f32, f32::max);
    emission.max(CONFLAGRATE_RINGS_TO_MS) / 1000.0
}

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// Where a particle of `e` is born and how it launches, for a victim at
/// `translation` (the bench's `spawn`, at spread 1.0).
pub fn conflagrate_particle_launch(
    e: &ConflagrateEmitter,
    translation: Vec3,
    is_pet: bool,
    seed: u32,
) -> (Vec3, Vec3) {
    let s = stature(is_pet);
    let chest = body_point(CONFLAGRATE_CHEST_ABOVE_FLOOR, translation, is_pet);
    let dir = match e.launch {
        ConflagrateLaunch::Sphere => Vec3::new(
            draw(seed, 1) * 2.0 - 1.0,
            draw(seed, 2) * 2.0 - 1.0,
            draw(seed, 3) * 2.0 - 1.0,
        )
        .try_normalize()
        .unwrap_or(Vec3::Y),
        ConflagrateLaunch::Cone { half_angle } => {
            let polar = (draw(seed, 1) * 2.0 - 1.0) * half_angle;
            let azimuth = draw(seed, 2) * TAU;
            Vec3::new(
                polar.sin() * azimuth.cos(),
                polar.cos(),
                polar.sin() * azimuth.sin(),
            )
        }
    };
    let offset = Vec3::new(draw(seed, 4) - 0.5, 0.0, draw(seed, 5) - 0.5) * e.area * s;
    (chest + offset, dir * e.speed * s)
}

/// A particle's look `t` of the way through its life: its colour, its alpha
/// and its sprite radius (yards, before the victim's stature).
pub fn conflagrate_particle_look(e: &ConflagrateEmitter, t: f32) -> (Color, f32, f32) {
    (
        keyed_color(e.color, t),
        keyed(e.alpha, t),
        keyed(e.scale, t) * CONFLAGRATE_SIZE_MUL,
    )
}

/// Length of a particle's streak behind it, yards (the bench's
/// `min(tail*0.12, speed*0.06) * size`), or `None` when it draws none.
pub fn conflagrate_streak_length(e: &ConflagrateEmitter, speed: f32, size: f32) -> Option<f32> {
    (e.tail > 0.0 && speed > CONFLAGRATE_STREAK_MIN_SPEED)
        .then(|| (e.tail * 0.12).min(speed * 0.06) * size)
}

/// An additive piece is lit-emissive with `AlphaMode::Add`; a blended one is
/// a flat, unlit colour with `AlphaMode::Blend`, which can darken the sand.
pub fn conflagrate_material(
    additive: bool,
    color: Color,
    alpha: f32,
    texture: Option<Handle<Image>>,
) -> StandardMaterial {
    if additive {
        let c = color.to_linear();
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: texture.clone(),
            emissive: LinearRgba::rgb(
                c.red * CONFLAGRATE_GLOW,
                c.green * CONFLAGRATE_GLOW,
                c.blue * CONFLAGRATE_GLOW,
            ),
            emissive_texture: texture,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: texture,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    }
}

/// Write a particle's or ring's colour and alpha into its material.
fn recolor(material: &mut StandardMaterial, additive: bool, color: Color, alpha: f32) {
    material.base_color = color.with_alpha(alpha);
    if additive {
        let c = color.to_linear();
        material.emissive = LinearRgba::rgb(
            c.red * CONFLAGRATE_GLOW,
            c.green * CONFLAGRATE_GLOW,
            c.blue * CONFLAGRATE_GLOW,
        );
    }
}

// ── Components ───────────────────────────────────────────────────────────────

/// Sprites and the quad every piece is drawn on, built once.
#[derive(Clone)]
pub struct ConflagrateAssets {
    pub quad: Handle<Mesh>,
    pub star: Handle<Image>,
    pub glow: Handle<Image>,
    pub cloud: Handle<Image>,
    pub smoke: Handle<Image>,
    pub disc: Handle<Image>,
}

impl ConflagrateAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        let sprite = |images: &mut Assets<Image>, kind: ConflagrateSprite| {
            images.add(disc_texture(SPRITE_PX, move |r, a| kind.alpha_at(r, a)))
        };
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            star: sprite(images, ConflagrateSprite::Star),
            glow: sprite(images, ConflagrateSprite::Glow),
            cloud: sprite(images, ConflagrateSprite::Cloud),
            smoke: sprite(images, ConflagrateSprite::Smoke),
            disc: images.add(disc_texture(DISC_PX, |r, _| keyed(&FIRE_DISC_FALLOFF, r))),
        }
    }

    pub fn sprite(&self, kind: ConflagrateSprite) -> Handle<Image> {
        match kind {
            ConflagrateSprite::Star => self.star.clone(),
            ConflagrateSprite::Glow => self.glow.clone(),
            ConflagrateSprite::Cloud => self.cloud.clone(),
            ConflagrateSprite::Smoke => self.smoke.clone(),
        }
    }
}

/// One landing: it stands at its victim's chest, emits the five layers and
/// carries the two rings as children.
#[derive(Component)]
pub struct ConflagrateBurst {
    pub target: Entity,
    pub age: f32,
    /// Fractional particles owed per emitter.
    pub carry: [f32; 5],
    pub emitted: u32,
    pub stature: f32,
    pub disc_material: Handle<StandardMaterial>,
    pub shock_material: Handle<StandardMaterial>,
    assets: ConflagrateAssets,
}

/// The rig's two ring children.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflagrateRingPart {
    /// The soft fire disc, flat at the chest.
    FireDisc,
    /// The rune ring: its children are the line's pieces.
    ShockRing,
}

/// One flat piece of the shock ring's line.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflagrateShockPiece {
    /// The chord from corner `i` to corner `i + 1` of the 48-sided circle.
    Segment(usize),
    /// Tick `k`, from 0.82 of the radius out to the circle, at `k / 8` of a
    /// turn.
    Tick(usize),
}

/// One particle, in world space: it never rides the victim.
#[derive(Component)]
pub struct ConflagrateParticle {
    /// Index into [`CONFLAGRATE_EMITTERS`].
    pub emitter: usize,
    /// The victim whose body the particle is held in front of.
    pub victim: Entity,
    /// Where the particle truly is. Its `Transform` is where it is DRAWN:
    /// here, or held in front of its victim's body (departure 5).
    pub at: Vec3,
    pub age: f32,
    pub life: f32,
    pub velocity: Vec3,
    /// The sprite's roll in the view plane.
    pub angle: f32,
    pub stature: f32,
}

/// A particle's two drawn pieces.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflagrateParticlePart {
    Sprite,
    Streak,
}

/// Lay a unit quad flat, facing up, its local X along `along` (in the ground
/// plane).
fn flat_along(along: Vec3) -> Quat {
    Quat::from_rotation_y((-along.z).atan2(along.x)) * Quat::from_rotation_x(-FRAC_PI_2)
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): a burst for every Conflagrate the sim landed, and
/// its victim's wound flinch (the shipped `hit_reaction.rs` dip, deeper on a
/// crit). A victim that is gone gets nothing.
#[allow(clippy::too_many_arguments)]
pub fn spawn_conflagrate_landings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<ConflagrateAssets>>,
    landings: Query<(Entity, &ConflagrateLanding)>,
    victims: Query<(&Transform, Option<&Pet>, Option<&HitFlinch>), With<Combatant>>,
) {
    let mut pending = PendingDepth::new();
    for (marker, landing) in landings.iter() {
        commands.entity(marker).despawn();
        let Ok((victim, pet, flinch)) = victims.get(landing.target) else {
            continue;
        };
        let is_pet = pet.is_some();
        insert_flinch(
            &mut commands,
            &mut pending,
            landing.target,
            is_pet,
            flinch,
            landing.is_crit,
        );
        let assets = assets
            .get_or_insert_with(|| ConflagrateAssets::build(&mut meshes, &mut images))
            .clone();
        let disc_material = materials.add(conflagrate_material(
            true,
            Color::srgb_u8(FIRE_DISC_COLOR[0], FIRE_DISC_COLOR[1], FIRE_DISC_COLOR[2]),
            0.0,
            Some(assets.disc.clone()),
        ));
        let shock_material = materials.add(conflagrate_material(
            true,
            Color::srgb_u8(
                SHOCK_RING_COLOR[0],
                SHOCK_RING_COLOR[1],
                SHOCK_RING_COLOR[2],
            ),
            0.0,
            None,
        ));
        let disc = commands
            .spawn((
                ConflagrateRingPart::FireDisc,
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(disc_material.clone()),
                Transform::from_scale(Vec3::splat(1e-4)),
                Visibility::Hidden,
                NotShadowCaster,
            ))
            .id();
        let pieces: Vec<Entity> = (0..SHOCK_RING_SEGMENTS)
            .map(ConflagrateShockPiece::Segment)
            .chain((0..SHOCK_RING_TICKS).map(ConflagrateShockPiece::Tick))
            .map(|piece| {
                commands
                    .spawn((
                        piece,
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(shock_material.clone()),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id()
            })
            .collect();
        let shock = commands
            .spawn((
                ConflagrateRingPart::ShockRing,
                Transform::default(),
                Visibility::Hidden,
            ))
            .add_children(&pieces)
            .id();
        let s = stature(is_pet);
        commands
            .spawn((
                ConflagrateBurst {
                    target: landing.target,
                    age: 0.0,
                    carry: [0.0; 5],
                    emitted: 0,
                    stature: s,
                    disc_material,
                    shock_material,
                    assets,
                },
                Transform::from_translation(body_point(
                    CONFLAGRATE_CHEST_ABOVE_FLOOR,
                    victim.translation,
                    is_pet,
                )),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .add_children(&[disc, shock]);
    }
}

/// Animate (graphical-only): stand each burst at its victim's chest and emit
/// its particles there, integrating the rate over the frame so a frame-rate
/// change does not change the density; retire it once its rings have gone,
/// or at once when its victim is.
pub fn animate_conflagrate_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(Entity, &mut ConflagrateBurst, &mut Transform)>,
    victims: Query<(&Transform, Option<&Pet>), (With<Combatant>, Without<ConflagrateBurst>)>,
) {
    let dt = time.delta_secs();
    for (rig_entity, mut rig, mut transform) in rigs.iter_mut() {
        let Ok((victim, pet)) = victims.get(rig.target) else {
            commands.entity(rig_entity).despawn();
            continue;
        };
        if rig.age >= conflagrate_rig_life() {
            commands.entity(rig_entity).despawn();
            continue;
        }
        let is_pet = pet.is_some();
        rig.age += dt;
        let ms = rig.age * 1000.0;
        transform.translation =
            body_point(CONFLAGRATE_CHEST_ABOVE_FLOOR, victim.translation, is_pet);

        for (ei, e) in CONFLAGRATE_EMITTERS.iter().enumerate() {
            if !CONFLAGRATE_LAYERS[ei] || !e.emitting_at(ms) {
                continue;
            }
            rig.carry[ei] += e.rate * CONFLAGRATE_DENSITY_MUL * dt;
            while rig.carry[ei] >= 1.0 {
                rig.carry[ei] -= 1.0;
                let seed = rig_entity
                    .index()
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let (position, velocity) =
                    conflagrate_particle_launch(e, victim.translation, is_pet, seed);
                let (color, alpha, _) = conflagrate_particle_look(e, 0.0);
                let mut parts = vec![commands
                    .spawn((
                        ConflagrateParticlePart::Sprite,
                        Mesh3d(rig.assets.quad.clone()),
                        MeshMaterial3d(materials.add(conflagrate_material(
                            e.additive,
                            color,
                            alpha,
                            Some(rig.assets.sprite(e.sprite)),
                        ))),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id()];
                if e.tail > 0.0 {
                    parts.push(
                        commands
                            .spawn((
                                ConflagrateParticlePart::Streak,
                                Mesh3d(rig.assets.quad.clone()),
                                MeshMaterial3d(materials.add(conflagrate_material(
                                    e.additive,
                                    color,
                                    alpha * CONFLAGRATE_STREAK_ALPHA,
                                    None,
                                ))),
                                Transform::from_scale(Vec3::splat(1e-4)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
                commands
                    .spawn((
                        ConflagrateParticle {
                            emitter: ei,
                            victim: rig.target,
                            at: position,
                            age: 0.0,
                            life: e.life,
                            velocity,
                            angle: draw(seed, 6) * TAU,
                            stature: rig.stature,
                        },
                        Transform::from_translation(position),
                        Visibility::default(),
                        PlayMatchEntity,
                    ))
                    .add_children(&parts);
            }
        }
    }
}

/// Pose (graphical-only): size, fade and show or hide each burst's two rings
/// for its age — the fire disc and the rune ring, flat at the chest.
#[allow(clippy::type_complexity)]
pub fn pose_conflagrate_rings(
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    rigs: Query<(&ConflagrateBurst, &Transform, &Children)>,
    victims: Query<
        (&Transform, Option<&Pet>),
        (
            With<Combatant>,
            Without<ConflagrateBurst>,
            Without<ConflagrateRingPart>,
            Without<ConflagrateShockPiece>,
        ),
    >,
    mut rings: Query<
        (
            &ConflagrateRingPart,
            &mut Transform,
            &mut Visibility,
            Option<&Children>,
        ),
        (Without<ConflagrateShockPiece>, Without<ConflagrateBurst>),
    >,
    mut pieces: Query<
        (&ConflagrateShockPiece, &mut Transform),
        (Without<ConflagrateRingPart>, Without<ConflagrateBurst>),
    >,
) {
    let view = camera.iter().next().map(|c| c.compute_transform());
    for (rig, rig_transform, children) in rigs.iter() {
        let (fire, shock) = conflagrate_rings(rig.age * 1000.0, rig.stature);
        // Each flat piece is held in front of the body as a particle is
        // (departure 5): scaled about the eye, so it projects where it was.
        let chest = rig_transform.translation;
        let hold = |local: Vec3| -> (Vec3, f32) {
            match (view, victims.get(rig.target)) {
                (Some(c), Ok((victim, pet))) => {
                    let (drawn, k) = held_in_front(
                        chest + local,
                        body_capsule(victim.translation, pet.is_some()),
                        c.translation,
                        c.forward().as_vec3(),
                    );
                    (drawn - chest, k)
                }
                _ => (local, 1.0),
            }
        };
        for child in children.iter() {
            let Ok((part, mut transform, mut visibility, pieces_of)) = rings.get_mut(child) else {
                continue;
            };
            match part {
                ConflagrateRingPart::FireDisc => {
                    let Some(fire) = fire else {
                        *visibility = Visibility::Hidden;
                        continue;
                    };
                    *visibility = Visibility::Inherited;
                    let (local, k) = hold(Vec3::ZERO);
                    transform.translation = local;
                    transform.rotation = flat_along(Vec3::X);
                    transform.scale = Vec3::new(fire.radius * 2.0, fire.radius * 2.0, 1.0) * k;
                    if let Some(m) = materials.get_mut(&rig.disc_material) {
                        let [r, g, b] = FIRE_DISC_COLOR;
                        recolor(m, true, Color::srgb_u8(r, g, b), fire.alpha);
                    }
                }
                ConflagrateRingPart::ShockRing => {
                    let Some(shock) = shock else {
                        *visibility = Visibility::Hidden;
                        continue;
                    };
                    *visibility = Visibility::Inherited;
                    if let Some(m) = materials.get_mut(&rig.shock_material) {
                        let [r, g, b] = SHOCK_RING_COLOR;
                        recolor(m, true, Color::srgb_u8(r, g, b), shock.alpha);
                    }
                    let radius = shock.radius;
                    let on_circle = |a: f32| Vec3::new(a.cos(), 0.0, a.sin());
                    for piece in pieces_of.into_iter().flat_map(|c| c.iter()) {
                        let Ok((piece, mut t)) = pieces.get_mut(piece) else {
                            continue;
                        };
                        let (from, to) = match *piece {
                            ConflagrateShockPiece::Segment(i) => {
                                let a0 = i as f32 / SHOCK_RING_SEGMENTS as f32 * TAU;
                                let a1 = (i + 1) as f32 / SHOCK_RING_SEGMENTS as f32 * TAU;
                                (on_circle(a0) * radius, on_circle(a1) * radius)
                            }
                            ConflagrateShockPiece::Tick(k) => {
                                let a = k as f32 / SHOCK_RING_TICKS as f32 * TAU;
                                (
                                    on_circle(a) * radius * SHOCK_RING_TICK_INNER,
                                    on_circle(a) * radius,
                                )
                            }
                        };
                        let span = to - from;
                        let length = span.length();
                        let (local, k) = hold((from + to) * 0.5);
                        t.translation = local;
                        t.rotation = flat_along(span.try_normalize().unwrap_or(Vec3::X));
                        t.scale = Vec3::new(length.max(1e-4), SHOCK_RING_LINE_WIDTH, 1.0) * k;
                    }
                }
            }
        }
    }
}

/// Animate (graphical-only): age, fall or rise, move and spin every particle,
/// and retire it at the end of its life.
pub fn animate_conflagrate_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut ConflagrateParticle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let e = CONFLAGRATE_EMITTERS[p.emitter];
        p.velocity.y -= e.gravity * p.stature * dt;
        let velocity = p.velocity;
        p.at += velocity * dt;
        p.angle += e.spin * TAU * dt;
    }
}

/// Billboard (graphical-only): place each particle where it is drawn — held
/// in front of its victim's body for the camera (departure 5) — turn its
/// sprite to the camera, sized and coloured along its life, and lay its
/// streak back along its flight as the camera sees it.
#[allow(clippy::type_complexity)]
pub fn billboard_conflagrate_particles(
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    victims: Query<(&Transform, Option<&Pet>), (With<Combatant>, Without<ConflagrateParticle>)>,
    mut particles: Query<
        (&ConflagrateParticle, &mut Transform, &Children),
        (Without<ConflagrateParticlePart>, Without<Combatant>),
    >,
    mut parts: Query<
        (
            &ConflagrateParticlePart,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<ConflagrateParticle>, Without<Combatant>),
    >,
) {
    let view = camera.iter().next().map(|c| c.compute_transform());
    let cam = view.map(|c| c.rotation).unwrap_or(Quat::IDENTITY);
    for (p, mut particle_transform, children) in particles.iter_mut() {
        let (drawn, k) = match (view, victims.get(p.victim)) {
            (Some(c), Ok((victim, pet))) => held_in_front(
                p.at,
                body_capsule(victim.translation, pet.is_some()),
                c.translation,
                c.forward().as_vec3(),
            ),
            _ => (p.at, 1.0),
        };
        particle_transform.translation = drawn;
        let e = CONFLAGRATE_EMITTERS[p.emitter];
        let t = (p.age / p.life).clamp(0.0, 1.0);
        let (color, alpha, radius) = conflagrate_particle_look(&e, t);
        let radius = radius * p.stature * k;
        let speed = p.velocity.length();
        for child in children.iter() {
            let Ok((part, mut transform, material)) = parts.get_mut(child) else {
                continue;
            };
            match part {
                ConflagrateParticlePart::Sprite => {
                    transform.rotation = cam * Quat::from_rotation_z(p.angle);
                    transform.translation = Vec3::ZERO;
                    transform.scale = Vec3::splat((radius * 2.0).max(1e-4));
                    if let Some(m) = materials.get_mut(&material.0) {
                        recolor(m, e.additive, color, alpha);
                    }
                }
                ConflagrateParticlePart::Streak => {
                    let Some(length) =
                        conflagrate_streak_length(&e, speed, CONFLAGRATE_SIZE_MUL * p.stature)
                    else {
                        transform.scale = Vec3::splat(1e-4);
                        continue;
                    };
                    let back = -p.velocity / speed;
                    // The bench's streak is a screen line: the projection of
                    // the segment behind the particle. So the quad lies in
                    // the camera's plane at the particle's own depth (never
                    // deeper, so never behind the body), with the projected
                    // length. Without a camera it lies back along the flight.
                    let (along, length) = match view {
                        Some(c) => {
                            let f = c.forward().as_vec3();
                            let flat = back - f * back.dot(f);
                            (flat.normalize_or_zero(), length * flat.length() * k)
                        }
                        None => (back, length),
                    };
                    // Roll the camera-facing quad until its long (local X)
                    // axis lies back along the flight as the camera sees it.
                    let (right, up) = (cam * Vec3::X, cam * Vec3::Y);
                    let roll = back.dot(up).atan2(back.dot(right));
                    transform.rotation = cam * Quat::from_rotation_z(roll);
                    transform.translation = along * (length * 0.5);
                    transform.scale = Vec3::new(
                        length.max(1e-4),
                        (radius * CONFLAGRATE_STREAK_WIDTH).max(1e-4),
                        1.0,
                    );
                    if let Some(m) = materials.get_mut(&material.0) {
                        recolor(m, e.additive, color, alpha * CONFLAGRATE_STREAK_ALPHA);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emitter_has_a_unique_name() {
        let mut names: Vec<&str> = CONFLAGRATE_EMITTERS.iter().map(|e| e.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn a_flat_quad_faces_up_and_runs_along_its_direction() {
        for along in [
            Vec3::X,
            Vec3::Z,
            -Vec3::X,
            Vec3::new(1.0, 0.0, -1.0).normalize(),
        ] {
            let q = flat_along(along);
            assert!((q * Vec3::Z - Vec3::Y).length() < 1e-5, "{along}");
            assert!((q * Vec3::X - along).length() < 1e-5, "{along}");
        }
    }

    #[test]
    fn the_rig_retires_once_its_rings_have_gone() {
        assert!((conflagrate_rig_life() - 1.3).abs() < 1e-6);
    }
}
