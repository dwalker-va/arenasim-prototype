use std::f32::consts::TAU;

use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::bleed_spray::bleed_landed;
use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::conflagrate::ConflagrateSprite;
use super::druid_heals::{body_point, keyed};
use super::hunter_shots::draw;
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// Expose Armor — the shield crest over the victim's head that cracks blue
// ==============================================================================
//
// From the client data (`docs/design/2026-10-17-rogue-talent-client-data.md`
// §3): Expose Armor's visual 3441 plays Rupture's finisher hand flash (cast
// kit 394) on the Rogue, and kit 3069, `exposearmor_head.m2`, on the victim's
// head for 3 s: a shield crest (an item texture) about a yard above the head
// that glints gold, turns blue and jolts as the armor breaks, with a glow
// quad behind it, star sparkles across it, a faint blue haze about it and a
// burst of shatter streaks falling through it. The 15 s debuff after it draws
// nothing; its icon carries it.
//
// The user's ruling (AS-336, on the signed-off Rogue Talent Visuals Bench,
// version 1792302646-d16f, its "Expose Armor" tab; committed as
// `docs/design/benches/2026-10-17-rogue-talent-bench.html`): Rupture's
// backhand apply stroke and hand flash, and the client's crest at client size
// with all its layers. So:
//
// - the stroke is `swing_style_for_ability(ExposeArmor)` =
//   `SwingStyle::Backhand`, its trail and the finisher hand flash the ones
//   Rupture ships (`instant_ability.rs` routes them);
// - the landing is this module's rig, held for the backhand's contact
//   (`impact_at`) and fired only if the victim then wears the caster's Expose
//   Armor (`bleed_landed`, the spray's own test) — so a dodge or a parry, an
//   immune victim or a corpse plays the stroke and nothing over the head;
// - its application is owned here, by NAME (`is_expose_armor`), so the
//   generic debuff band stays off it (`AuraApplyRoute::for_instance`).
//
// The bench draws six layers. The hand flash is Rupture's. The other five,
// each with the composite the bench draws it with:
//
//   crest    `crest()`: a heater-shield outline and cross over a 0.45 fill,
//            0.38 × 0.5 yd times its scale track, gold (240,212,81) to 0.667 s
//            then blue (81,137,240) by 1.0 s; the line is that colour × 1.1.
//            Composited `lighter`: ADDITIVE.
//   glow     `bbGlow()`: a soft ellipse 0.92 × 1.0 yd behind the crest, gold
//            (255,210,0) → blue (0,90,255) on the crest's keys, at 0.83 × 0.6
//            of full from 0.4 s to 2.6 s. `lighter`: ADDITIVE.
//   sparkles a star emitter, 45/s from 0.134 s to 0.667 s, spinning 1.1 rev/s.
//            `Emitter.draw` with no `blend`: ADDITIVE.
//   haze     a smoke emitter, 25/s from 0.5 s to 2.6 s, drag 2.5, alpha at
//            most 0.2. ADDITIVE.
//   shatter  a glow emitter with a tail, 150/s from 0.4 s to 0.8 s, falling
//            through the crest. ADDITIVE.
//
// Where the card and the bench disagree, the bench was signed off and wins:
// the sparkles emit throughout 0.134-0.667 s (the client's P2/P4 fill the
// 0.3-0.5 s gap between P0/P1's windows); the streaks fall at 0.9 yd/s under
// gravity 2 (not the client's 0.42 yd/s); the glow reaches 0.83 × 0.6 of
// full, ramped in from nothing at the landing; the crest squash is the
// bench's two-axis track (width 1.5 → 1.5 → 1.0, height 1.5 → 1.7 → 0.8,
// keyed 0.7 / 0.8 / 0.9 / 1.0 s) rather than the client's three-axis one; the
// haze grows 0.24 → 0.36 → 0.57 yd; and the crest is additive throughout.
//
// ## Departures from the bench (each deliberate)
//
// 1. **Held in front of the body.** The bench paints the bodies first and
//    every layer over them. Here the body is an `AlphaMode::Blend` mesh that
//    writes no depth, and the transparent pass sorts by each mesh's
//    translation. At the match camera the crest and its glow stand a yard
//    over the head, nearer the camera than any of the body, and need
//    nothing; but the lowest of the falling shatter streaks (about one piece
//    in twenty-five) stand deeper than the body's top, and from a camera
//    nearer the horizontal everything does — the body would paint over it.
//    So every piece — crest, glow and every particle — is drawn on its own
//    view ray, slid in front of the victim's RENDERED body and scaled by the
//    same ratio — the shared body hold (`body_hold.rs`) — so it projects
//    where the bench puts it. The crest stands over the rendered body's head
//    too, so the walk bob and the Cyclone lift carry it
//    (`pose_expose_armor_crests`).
// 2. **The crest is a texture.** Its outline and cross are a sprite at the
//    crest's rest aspect, so the line's width follows both axes of the
//    squash; the bench strokes 0.16 of the CURRENT half-width, which follows
//    the width squash only. Its corners are rounded where the bench mitres
//    them.
// 3. **A star's white-hot centre is tinted.** The shared star sprite is one
//    shape in the layer's colour, as Conflagrate's and Siphon Life's stars
//    are; the bench paints its core white.
// 4. **A streak is a flat-ended quad** whose two ends project exactly onto
//    the particle and onto the point its tail length back along its flight —
//    the bench's screen line, stroked with round caps there.
// 5. **Follows in height too.** The bench's emitters follow the victim in x
//    and z; here the rig rides the victim's head, so a victim on a ramp
//    keeps its crest.
// 6. **The haze's drag is the bench's rule taken to its limit.** The bench
//    multiplies the velocity by `1 − 7.5·dt` each frame; here it decays as
//    `exp(−7.5·dt)`, the same at any frame rate.
// 7. **Lifecycle.** The landing plays out over its 3 s and its last haze
//    particle's life (3.6 s, as the bench draws it). It is hidden on a sheep
//    (its clock runs on); it ends at once when its victim dies or is gone, as
//    a crest over a fallen body would float in the air; a new landing on the
//    same victim replaces it; stealth changes nothing (a stealthed body is
//    still drawn). Everything is a `PlayMatchEntity` or a child of one, so a
//    match that ends mid-landing takes all of it.
//
// ## At the match camera (60 yd, pitch 38.7°, FOV 45°)
//
// About 22 px a yard at 1080p: the crest is ~12 × 16 px at its 1.5 scale and
// ~8 × 11 px at rest, over a ~20 × 22 px glow, a yard above the head; a
// sparkle up to ~9 px, a shatter head up to ~7 px with a ~2 px tail, a haze
// puff up to ~25 px. Client size, by the ruling.
//
// House rules: spawn / animate / age / pose systems, `Res<Time>`, registered
// in `states/mod.rs` only (headless never runs any of it and stays
// byte-identical by construction), no `game_rng` — all scatter is hash-seeded
// off the rig's entity index and emission count.

// ── Bench sign-off constants (AS-336, version 1792302646-d16f) ───────────────

// Expose Armor: kit 394 (finisher hand flash) on cast; kit 3069 (exposearmor_head.m2 @Head, 3 s) on the victim. AS-336 bench.
// swing_style_for_ability(ExposeArmor) = SwingStyle::Backhand: wind up across the body 0.22 s, outward sweep through contact at 0.33 s (mid-arc) to 0.44 s, hold to 0.54 s, recover by 0.9 s; landing at impact_at.
// The bench's EXPOSE_ARMOR_HAND_FLASH is no knob here: the router plays Rupture's kit-394 flash
// for both finishers (`instant_ability.rs`), so it cannot be turned off for one.
pub const EXPOSE_ARMOR_CREST: bool = true; // ~0.38 x 0.5 yd x1.5, 1.0 yd over the head; gold -> blue 0.667-1.0 s
pub const EXPOSE_ARMOR_GLOW: bool = true;
pub const EXPOSE_ARMOR_SPARKLES: bool = true;
pub const EXPOSE_ARMOR_HAZE: bool = true;
pub const EXPOSE_ARMOR_SHATTER: bool = true; // 150/s, 0.4-0.8 s
pub const EXPOSE_ARMOR_SECS: f32 = 3.00;
pub const EXPOSE_ARMOR_SIZE_MUL: f32 = 1.00;

/// The crest's centre above the floor: the bench's head anchor (2.25,
/// attachment 20) and a yard over it.
pub const CREST_ABOVE_FLOOR: f32 = 3.25;

// ── The crest and its glow (the bench's `customDraw`) ────────────────────────

/// The crest's width and height, yards, before its scale track.
pub const CREST_SIZE: (f32, f32) = (0.38, 0.5);
pub const CREST_GOLD: [u8; 3] = [240, 212, 81];
pub const CREST_BLUE: [u8; 3] = [81, 137, 240];
/// The crest's colour, keyed in milliseconds after the landing.
pub const CREST_COLOR_KEYS: [(f32, [u8; 3]); 3] =
    [(0.0, CREST_GOLD), (667.0, CREST_GOLD), (1000.0, CREST_BLUE)];
/// The crest's line is its colour times this, each channel capped at 255.
pub const CREST_LINE_GAIN: f32 = 1.1;
/// The crest's fill is drawn at this fraction of its alpha.
pub const CREST_FILL_ALPHA: f32 = 0.45;
/// The crest's alpha: in by 134 ms, out from 2.8 s.
pub const CREST_FADE_KEYS: [(f32, f32); 4] =
    [(0.0, 0.0), (134.0, 1.0), (2800.0, 1.0), (3000.0, 0.0)];
/// The crest's scale track across and up: 1.5, squashed between 0.7 and
/// 1.0 s as it turns blue, then 1.0.
pub const CREST_SCALE_X_KEYS: [(f32, f32); 5] = [
    (0.0, 1.5),
    (700.0, 1.5),
    (800.0, 1.5),
    (900.0, 1.0),
    (1000.0, 1.0),
];
pub const CREST_SCALE_Y_KEYS: [(f32, f32); 5] = [
    (0.0, 1.5),
    (700.0, 1.5),
    (800.0, 1.7),
    (900.0, 0.8),
    (1000.0, 1.0),
];
/// The crest sprite spans this much of its half-size each way, so the
/// outline's outer half fits on it.
pub const CREST_SPRITE_SPAN: f32 = 1.1;
/// The outline is 0.16 of the crest's half-width wide, the cross 0.10.
pub const CREST_OUTLINE_WIDTH: f32 = 0.16;
pub const CREST_CROSS_WIDTH: f32 = 0.10;

/// The glow behind the crest: a soft ellipse this many yards across and up.
pub const GLOW_SIZE: (f32, f32) = (0.92, 1.0);
pub const GLOW_COLOR_KEYS: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 210, 0]),
    (667.0, [255, 210, 0]),
    (1000.0, [0, 90, 255]),
];
/// The glow's weight (`blue_glowd`, 0.83 from 0.4 to 2.6 s), drawn at
/// [`GLOW_ALPHA`] of it.
pub const GLOW_WEIGHT_KEYS: [(f32, f32); 4] =
    [(0.0, 0.0), (400.0, 0.83), (2600.0, 0.83), (3000.0, 0.0)];
pub const GLOW_ALPHA: f32 = 0.6;
/// The glow's radial falloff (`bbGlow`'s stops): full at the centre, 0.45
/// halfway out, nothing at the rim.
pub const GLOW_FALLOFF: [(f32, f32); 3] = [(0.0, 1.0), (0.5, 0.45), (1.0, 0.0)];

// ── Rendering constants ──────────────────────────────────────────────────────

/// Emissive gain of an additive piece over its colour — the house's 2-4x glow
/// convention (Conflagrate and Siphon Life use the same).
pub const EXPOSE_ARMOR_GLOW_GAIN: f32 = 2.5;
/// A streak is drawn only above this speed, yd/s (the bench's `sp > 0.2`).
pub const EXPOSE_STREAK_MIN_SPEED: f32 = 0.2;
/// The bench strokes a streak at 0.85 of the sprite's alpha.
pub const EXPOSE_STREAK_ALPHA: f32 = 0.85;
/// The bench's streak is 0.7 of the sprite's radius wide.
pub const EXPOSE_STREAK_WIDTH: f32 = 0.7;
/// The bench's drag multiplies velocity by `1 − drag·3·dt` a frame.
pub const EXPOSE_DRAG_SCALE: f32 = 3.0;
/// Keeps float drift from dropping the last particle of an exact count.
const EMIT_EPSILON: f32 = 1e-4;
/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 64;
const CREST_PX: u32 = 128;

// ── The emitters (the bench's three particle layers) ─────────────────────────

/// How a particle launches.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ExposeLaunch {
    /// The bench's `vRange >= 3.1`: a direction drawn in the cube and
    /// normalised — every way out.
    Sphere,
    /// The bench's narrower `vRange`: a polar angle up to `half_angle` off
    /// straight up, at any bearing (a negative speed turns it down).
    Cone { half_angle: f32 },
}

/// One emitter, as the bench's layer record carries it. Every one composites
/// `lighter`: additive.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ExposeEmitter {
    /// Unique key.
    pub name: &'static str,
    pub sprite: ConflagrateSprite,
    pub launch: ExposeLaunch,
    /// Launch speed, yd/s; negative launches the other way along the
    /// direction.
    pub speed: f32,
    /// Downward acceleration, yd/s².
    pub gravity: f32,
    /// The bench's `drag`.
    pub drag: f32,
    pub life: f32,
    /// Particles per second while the window is open.
    pub rate: f32,
    /// `[start, end)` of the emission window, milliseconds after the landing.
    pub window_ms: (f32, f32),
    /// Height above the floor of the box the particles are born in.
    pub centre_above_floor: f32,
    /// The box: its side in the ground plane, and its height.
    pub area: f32,
    pub area_y: f32,
    /// Colour, alpha and scale tracks over the particle's normalised life.
    pub color: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// Sprite RADIUS, yards.
    pub scale: &'static [(f32, f32)],
    /// Tail length factor; `0.0` draws no streak.
    pub tail: f32,
    /// Sprite spin, revolutions per second, clockwise on screen.
    pub spin: f32,
}

impl ExposeEmitter {
    /// Whether the emitter is emitting `ms` after the landing (`windowOn`).
    pub fn emitting_at(&self, ms: f32) -> bool {
        ms >= self.window_ms.0 && ms < self.window_ms.1
    }
}

/// `exposearmor_head.m2`'s particles as the bench's three layers, in order.
pub const EXPOSE_ARMOR_EMITTERS: [ExposeEmitter; 3] = [
    // P0, P1, P2, P4 star4: the gold-to-cyan glints across the crest.
    ExposeEmitter {
        name: "exposearmor_head/sparkles",
        sprite: ConflagrateSprite::Star,
        launch: ExposeLaunch::Sphere,
        speed: 0.28,
        gravity: 0.0,
        drag: 0.0,
        life: 0.2,
        rate: 45.0,
        window_ms: (134.0, 667.0),
        centre_above_floor: 3.25,
        area: 0.42,
        area_y: 0.6,
        color: &[
            (0.0, [255, 210, 0]),
            (0.5, [255, 255, 255]),
            (1.0, [0, 246, 255]),
        ],
        alpha: &[(0.0, 1.0), (0.5, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.02), (0.5, 0.21), (1.0, 0.02)],
        tail: 0.0,
        spin: 1.1,
    },
    // P3 toonsmoke16: the faint blue haze about the crest.
    ExposeEmitter {
        name: "exposearmor_head/haze",
        sprite: ConflagrateSprite::Smoke,
        launch: ExposeLaunch::Sphere,
        speed: 0.56,
        gravity: 0.0,
        drag: 2.5,
        life: 1.0,
        rate: 25.0,
        window_ms: (500.0, 2600.0),
        centre_above_floor: 3.22,
        area: 0.0,
        area_y: 0.0,
        color: &[
            (0.0, [195, 207, 225]),
            (0.5, [186, 210, 255]),
            (1.0, [12, 0, 255]),
        ],
        alpha: &[(0.0, 0.0), (0.5, 0.2), (1.0, 0.0)],
        scale: &[(0.0, 0.24), (0.5, 0.36), (1.0, 0.57)],
        tail: 0.0,
        spin: 0.0,
    },
    // P5 grad2db: the shatter streaks falling through the crest.
    ExposeEmitter {
        name: "exposearmor_head/shatter",
        sprite: ConflagrateSprite::Glow,
        launch: ExposeLaunch::Cone { half_angle: 0.5 },
        speed: -0.9,
        gravity: 2.0,
        drag: 0.0,
        life: 0.5,
        rate: 150.0,
        window_ms: (400.0, 800.0),
        centre_above_floor: 3.5,
        area: 0.6,
        area_y: 0.3,
        color: &[
            (0.0, [255, 18, 115]),
            (0.3, [0, 162, 255]),
            (1.0, [120, 0, 255]),
        ],
        alpha: &[(0.0, 0.39), (0.3, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.3, 0.17), (1.0, 0.01)],
        tail: 4.0,
        spin: 0.0,
    },
];

/// Which of [`EXPOSE_ARMOR_EMITTERS`] are kept layers, in table order.
pub const EXPOSE_ARMOR_LAYERS: [bool; 3] = [
    EXPOSE_ARMOR_SPARKLES,
    EXPOSE_ARMOR_HAZE,
    EXPOSE_ARMOR_SHATTER,
];

// ── The pure model ───────────────────────────────────────────────────────────

/// The rig's whole life, seconds: the crest's 3 s, or until its last
/// particle has died, whichever is later.
pub fn expose_armor_rig_life() -> f32 {
    EXPOSE_ARMOR_EMITTERS
        .iter()
        .map(|e| e.window_ms.1 / 1000.0 + e.life)
        .fold(EXPOSE_ARMOR_SECS, f32::max)
}

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// Where the crest stands for a victim at `translation`.
pub fn expose_armor_crest_point(translation: Vec3, is_pet: bool) -> Vec3 {
    body_point(CREST_ABOVE_FLOOR, translation, is_pet)
}

/// The crest as drawn `ms` after the landing: its fill colour, its line
/// colour, its alpha, and its width and height in yards (before the victim's
/// stature).
pub fn expose_armor_crest_look(ms: f32) -> (Color, Color, f32, Vec2) {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = CREST_COLOR_KEYS
            .iter()
            .map(|(t, v)| (*t, v[c] as f32))
            .collect();
        keyed(&keys, ms)
    };
    let rgb = [channel(0), channel(1), channel(2)];
    let fill = Color::srgb(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0);
    let line = |v: f32| (v * CREST_LINE_GAIN).min(255.0) / 255.0;
    let line = Color::srgb(line(rgb[0]), line(rgb[1]), line(rgb[2]));
    let size = Vec2::new(
        CREST_SIZE.0 * keyed(&CREST_SCALE_X_KEYS, ms),
        CREST_SIZE.1 * keyed(&CREST_SCALE_Y_KEYS, ms),
    ) * EXPOSE_ARMOR_SIZE_MUL;
    (fill, line, keyed(&CREST_FADE_KEYS, ms), size)
}

/// The glow as drawn `ms` after the landing: its colour and its alpha.
pub fn expose_armor_glow_look(ms: f32) -> (Color, f32) {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = GLOW_COLOR_KEYS
            .iter()
            .map(|(t, v)| (*t, v[c] as f32))
            .collect();
        keyed(&keys, ms) / 255.0
    };
    (
        Color::srgb(channel(0), channel(1), channel(2)),
        keyed(&GLOW_WEIGHT_KEYS, ms) * GLOW_ALPHA,
    )
}

/// A colour track sampled at `t`, channel by channel.
pub fn expose_track_color(keys: &[(f32, [u8; 3])], t: f32) -> Color {
    let channel = |c: usize| {
        let ks: Vec<(f32, f32)> = keys.iter().map(|(k, v)| (*k, v[c] as f32)).collect();
        keyed(&ks, t) / 255.0
    };
    Color::srgb(channel(0), channel(1), channel(2))
}

/// Where a particle of `e` is born, relative to the crest, and how it
/// launches (the bench's `spawn`, at spread 1.0), on a body of `stature`.
pub fn expose_particle_launch(e: &ExposeEmitter, stature: f32, seed: u32) -> (Vec3, Vec3) {
    let dir = match e.launch {
        ExposeLaunch::Sphere => Vec3::new(
            draw(seed, 1) * 2.0 - 1.0,
            draw(seed, 2) * 2.0 - 1.0,
            draw(seed, 3) * 2.0 - 1.0,
        )
        .try_normalize()
        .unwrap_or(Vec3::Y),
        ExposeLaunch::Cone { half_angle } => {
            let polar = (draw(seed, 1) * 2.0 - 1.0) * half_angle;
            let azimuth = draw(seed, 2) * TAU;
            Vec3::new(
                polar.sin() * azimuth.cos(),
                polar.cos(),
                polar.sin() * azimuth.sin(),
            )
        }
    };
    let centre = Vec3::Y * (e.centre_above_floor - CREST_ABOVE_FLOOR);
    let jitter = Vec3::new(
        (draw(seed, 4) - 0.5) * e.area,
        (draw(seed, 5) - 0.5) * e.area_y,
        (draw(seed, 6) - 0.5) * e.area,
    );
    ((centre + jitter) * stature, dir * e.speed * stature)
}

/// A particle's look `t` of the way through its life: its colour, its alpha
/// and its sprite radius (yards, before the victim's stature).
pub fn expose_particle_look(e: &ExposeEmitter, t: f32) -> (Color, f32, f32) {
    (
        expose_track_color(e.color, t),
        keyed(e.alpha, t),
        keyed(e.scale, t) * EXPOSE_ARMOR_SIZE_MUL,
    )
}

/// A particle's roll in the view plane, radians about the camera's view
/// axis, `age` seconds into a life that began at `start` — the bench's
/// `ang`, which a canvas turns clockwise on screen, so a positive spin turns
/// the quad clockwise as the camera sees it.
pub fn expose_particle_roll(e: &ExposeEmitter, start: f32, age: f32) -> f32 {
    -(start + e.spin * TAU * age)
}

/// Length of a particle's streak behind it, yards (the bench's
/// `min(tail*0.12, speed*0.06) * size`), or `None` when it draws none.
pub fn expose_streak_length(e: &ExposeEmitter, speed: f32, size: f32) -> Option<f32> {
    (e.tail > 0.0 && speed > EXPOSE_STREAK_MIN_SPEED)
        .then(|| (e.tail * 0.12).min(speed * 0.06) * size)
}

/// The crest's outline, in the crest's half-size units (x right, y DOWN, as
/// the bench's canvas path runs): the top edge, down each side to −0.1, and a
/// curve from each to the point at the bottom.
fn crest_outline() -> Vec<Vec2> {
    let curve = |from: Vec2, ctrl: Vec2, to: Vec2, steps: usize| -> Vec<Vec2> {
        (1..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                from * (1.0 - t) * (1.0 - t) + ctrl * 2.0 * (1.0 - t) * t + to * t * t
            })
            .collect()
    };
    let mut path = vec![
        Vec2::new(-1.0, -1.0),
        Vec2::new(1.0, -1.0),
        Vec2::new(1.0, -0.1),
    ];
    path.extend(curve(
        Vec2::new(1.0, -0.1),
        Vec2::new(0.9, 0.6),
        Vec2::new(0.0, 1.0),
        48,
    ));
    path.extend(curve(
        Vec2::new(0.0, 1.0),
        Vec2::new(-0.9, 0.6),
        Vec2::new(-1.0, -0.1),
        48,
    ));
    path.push(Vec2::new(-1.0, -1.0));
    path
}

/// Whether `p` (half-size units, y down) is inside the crest's shape.
pub fn crest_contains(p: Vec2) -> bool {
    if !(-1.0..=1.0).contains(&p.y) {
        return false;
    }
    if p.y <= -0.1 {
        return p.x.abs() <= 1.0;
    }
    // The side curve's y(t) = −0.1 + 1.4t − 0.3t², solved for t; its
    // half-width there is x(t) = (1 − t)(1 + 0.8t).
    let t = (1.4 - (1.96 - 1.2 * (0.1 + p.y)).max(0.0).sqrt()) / 0.6;
    p.x.abs() <= (1.0 - t) * (1.0 + 0.8 * t)
}

/// Whether `p` (half-size units, y down) is on the crest's outline or its
/// cross, at the crest's rest aspect: the outline is
/// [`CREST_OUTLINE_WIDTH`] of the half-width wide, straddling the path; the
/// cross's two bars are [`CREST_CROSS_WIDTH`] wide, square-ended — the
/// upright from −0.75 to 0.7 of the half-height, the bar at −0.25 from
/// −0.6 to 0.6 of the half-width.
pub fn crest_line_contains(p: Vec2) -> bool {
    // Measure in yards at the rest aspect, so the line's width is the same
    // across and up, as a canvas stroke is.
    let aspect = Vec2::new(CREST_SIZE.0, CREST_SIZE.1) * 0.5;
    let half_w = aspect.x;
    let q = p * aspect;
    let outline = crest_outline();
    let near_outline = outline.windows(2).any(|pair| {
        let (a, b) = (pair[0] * aspect, pair[1] * aspect);
        let ab = b - a;
        let u = ((q - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        (q - (a + ab * u)).length() <= CREST_OUTLINE_WIDTH * half_w * 0.5
    });
    let bar = CREST_CROSS_WIDTH * half_w * 0.5;
    let upright = q.x.abs() <= bar && (-0.75..=0.7).contains(&p.y);
    let crossbar = (q.y - (-0.25 * aspect.y)).abs() <= bar && (-0.6..=0.6).contains(&p.x);
    near_outline || upright || crossbar
}

/// A crest sprite: white, with `shape` in its alpha channel, sampled 4 × 4
/// per pixel so its edges are smooth. The sprite spans
/// [`CREST_SPRITE_SPAN`] of the crest's half-size each way; row 0 is the top.
pub fn crest_texture(shape: fn(Vec2) -> bool) -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let size = CREST_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    const SUB: u32 = 4;
    for y in 0..size {
        for x in 0..size {
            let mut hits = 0;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let u = (x as f32 + (sx as f32 + 0.5) / SUB as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SUB as f32) / size as f32;
                    let p = Vec2::new(u * 2.0 - 1.0, v * 2.0 - 1.0) * CREST_SPRITE_SPAN;
                    if shape(p) {
                        hits += 1;
                    }
                }
            }
            let i = ((y * size + x) * 4) as usize;
            data[i..i + 3].fill(255);
            data[i + 3] = (hits as f32 / (SUB * SUB) as f32 * 255.0).round() as u8;
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

/// Every piece is additive: lit-emissive with `AlphaMode::Add` (`unlit`
/// would discard the emissive), the sprite driving both channels.
pub fn expose_armor_material(
    color: Color,
    alpha: f32,
    texture: Option<Handle<Image>>,
) -> StandardMaterial {
    let c = color.to_linear();
    StandardMaterial {
        base_color: color.with_alpha(alpha),
        base_color_texture: texture.clone(),
        emissive: LinearRgba::rgb(
            c.red * EXPOSE_ARMOR_GLOW_GAIN,
            c.green * EXPOSE_ARMOR_GLOW_GAIN,
            c.blue * EXPOSE_ARMOR_GLOW_GAIN,
        ),
        emissive_texture: texture,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// Write a piece's colour and alpha into its material.
fn recolor(material: &mut StandardMaterial, color: Color, alpha: f32) {
    material.base_color = color.with_alpha(alpha);
    let c = color.to_linear();
    material.emissive = LinearRgba::rgb(
        c.red * EXPOSE_ARMOR_GLOW_GAIN,
        c.green * EXPOSE_ARMOR_GLOW_GAIN,
        c.blue * EXPOSE_ARMOR_GLOW_GAIN,
    );
}

// ── Components ───────────────────────────────────────────────────────────────

/// A landing waiting for its stroke's contact. Fires after `delay` seconds if
/// `victim` is alive and wears `aura` from `caster`.
#[derive(Component)]
pub struct ExposeArmorPending {
    pub victim: Entity,
    pub caster: Entity,
    /// The RON `name:` of the aura whose landing the crest shows.
    pub aura: String,
    pub delay: f32,
}

/// Sprites and the quad every piece is drawn on, built once.
#[derive(Clone)]
pub struct ExposeArmorAssets {
    pub quad: Handle<Mesh>,
    pub crest_fill: Handle<Image>,
    pub crest_line: Handle<Image>,
    pub glow: Handle<Image>,
    pub star: Handle<Image>,
    pub dot: Handle<Image>,
    pub smoke: Handle<Image>,
}

impl ExposeArmorAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        let sprite = |images: &mut Assets<Image>, kind: ConflagrateSprite| {
            images.add(disc_texture(SPRITE_PX, move |r, a| kind.alpha_at(r, a)))
        };
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            crest_fill: images.add(crest_texture(crest_contains)),
            crest_line: images.add(crest_texture(crest_line_contains)),
            glow: images.add(disc_texture(SPRITE_PX, |r, _| keyed(&GLOW_FALLOFF, r))),
            star: sprite(images, ConflagrateSprite::Star),
            dot: sprite(images, ConflagrateSprite::Glow),
            smoke: sprite(images, ConflagrateSprite::Smoke),
        }
    }

    fn sprite(&self, kind: ConflagrateSprite) -> Handle<Image> {
        match kind {
            ConflagrateSprite::Star => self.star.clone(),
            ConflagrateSprite::Smoke => self.smoke.clone(),
            ConflagrateSprite::Glow | ConflagrateSprite::Cloud => self.dot.clone(),
        }
    }
}

/// One landing: it stands at the crest point over its victim's head, carries
/// the crest and its glow, and emits the three particle layers as its own
/// children, so all of it rides the victim.
#[derive(Component)]
pub struct ExposeArmorCrest {
    pub target: Entity,
    pub age: f32,
    /// Fractional particles owed per emitter.
    pub carry: [f32; 3],
    pub emitted: u32,
    /// Whether the victim is a pet (its stature).
    pub is_pet: bool,
    assets: ExposeArmorAssets,
}

/// The rig's three fixed quads.
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExposeArmorPiece {
    /// The soft glow behind the crest.
    Glow,
    /// The crest's fill, at 0.45 of its alpha.
    CrestFill,
    /// The crest's outline and cross, its colour × 1.1.
    CrestLine,
}

/// One particle: a child of its rig. Its `Transform` is where it is DRAWN —
/// held in front of the victim's body (departure 1); `offset` is where it
/// truly is, relative to the crest.
#[derive(Component)]
pub struct ExposeArmorParticle {
    /// Index into [`EXPOSE_ARMOR_EMITTERS`].
    pub emitter: usize,
    pub offset: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    /// The roll it was born with (the bench's `ang = rnd()*TAU`).
    pub start_angle: f32,
    pub stature: f32,
}

/// A particle's two drawn pieces.
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExposeArmorParticlePart {
    Sprite,
    Streak,
}

// ── Spawn ────────────────────────────────────────────────────────────────────

/// Hold a landing for `delay` seconds — the stroke's contact — then fire it
/// over `victim`'s head if the strike landed `aura` there.
pub fn spawn_expose_armor_pending(
    commands: &mut Commands,
    victim: Entity,
    caster: Entity,
    aura: String,
    delay: f32,
) {
    commands.spawn((
        ExposeArmorPending {
            victim,
            caster,
            aura,
            delay,
        },
        PlayMatchEntity,
    ));
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): count each held landing down, and at its contact
/// build the crest over the victim's head if the strike landed — replacing a
/// crest still playing on that victim — or drop it.
#[allow(clippy::too_many_arguments)]
pub fn fire_expose_armor_crests(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<ExposeArmorAssets>>,
    mut pending: Query<(Entity, &mut ExposeArmorPending)>,
    crests: Query<(Entity, &ExposeArmorCrest)>,
    victims: Query<(&Combatant, &Transform, Option<&ActiveAuras>, Option<&Pet>)>,
) {
    let dt = time.delta_secs();
    let mut replaced: Vec<Entity> = Vec::new();
    for (entity, mut held) in pending.iter_mut() {
        held.delay -= dt;
        if held.delay > 0.0 {
            continue;
        }
        commands.entity(entity).despawn();
        let Ok((victim, transform, auras, pet)) = victims.get(held.victim) else {
            continue;
        };
        if !bleed_landed(victim.is_alive(), auras, held.caster, &held.aura) {
            continue;
        }
        for (rig, crest) in crests.iter() {
            if crest.target == held.victim && !replaced.contains(&rig) {
                commands.entity(rig).despawn();
                replaced.push(rig);
            }
        }
        let assets = assets
            .get_or_insert_with(|| ExposeArmorAssets::build(&mut meshes, &mut images))
            .clone();
        let is_pet = pet.is_some();
        let (fill, line, _, _) = expose_armor_crest_look(0.0);
        let (glow, _) = expose_armor_glow_look(0.0);
        let pieces: Vec<Entity> = [
            (
                ExposeArmorPiece::Glow,
                EXPOSE_ARMOR_GLOW,
                glow,
                assets.glow.clone(),
            ),
            (
                ExposeArmorPiece::CrestFill,
                EXPOSE_ARMOR_CREST,
                fill,
                assets.crest_fill.clone(),
            ),
            (
                ExposeArmorPiece::CrestLine,
                EXPOSE_ARMOR_CREST,
                line,
                assets.crest_line.clone(),
            ),
        ]
        .into_iter()
        .filter(|(_, kept, _, _)| *kept)
        .map(|(piece, _, color, texture)| {
            commands
                .spawn((
                    piece,
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(materials.add(expose_armor_material(color, 0.0, Some(texture)))),
                    Transform::from_scale(Vec3::splat(1e-4)),
                    Visibility::default(),
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
        let rig = commands
            .spawn((
                ExposeArmorCrest {
                    target: held.victim,
                    age: 0.0,
                    carry: [0.0; 3],
                    emitted: 0,
                    is_pet,
                    assets,
                },
                Transform::from_translation(expose_armor_crest_point(
                    transform.translation,
                    is_pet,
                )),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .add_children(&pieces)
            .id();
        replaced.push(rig);
    }
}

/// Update (graphical-only): hide each crest while its victim is a sheep, emit
/// its particle layers over the head — integrating
/// the rate over the frame, so a frame-rate change does not change the
/// density — and retire it at the end of its life, or at once when its victim
/// dies or is gone.
#[allow(clippy::type_complexity)]
pub fn animate_expose_armor_crests(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(Entity, &mut ExposeArmorCrest, &mut Visibility)>,
    victims: Query<(&Combatant, Has<Pet>, Has<PolymorphedVisual>)>,
) {
    let dt = time.delta_secs();
    for (rig_entity, mut rig, mut visibility) in rigs.iter_mut() {
        let Ok((victim, is_pet, sheep)) = victims.get(rig.target) else {
            commands.entity(rig_entity).despawn();
            continue;
        };
        if !victim.is_alive() || rig.age >= expose_armor_rig_life() {
            commands.entity(rig_entity).despawn();
            continue;
        }
        let (from_ms, to_ms) = (rig.age * 1000.0, (rig.age + dt) * 1000.0);
        rig.age += dt;
        rig.is_pet = is_pet;
        visibility.set_if_neq(if sheep {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
        let s = stature(is_pet);

        for (ei, e) in EXPOSE_ARMOR_EMITTERS.iter().enumerate() {
            if !EXPOSE_ARMOR_LAYERS[ei] {
                continue;
            }
            // The time this frame spends inside the window.
            let open = (to_ms.min(e.window_ms.1) - from_ms.max(e.window_ms.0)).max(0.0);
            rig.carry[ei] += e.rate * open / 1000.0;
            while rig.carry[ei] >= 1.0 - EMIT_EPSILON {
                rig.carry[ei] -= 1.0;
                let seed = rig_entity
                    .index()
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let (offset, velocity) = expose_particle_launch(e, s, seed);
                let (color, alpha, _) = expose_particle_look(e, 0.0);
                let mut parts = vec![commands
                    .spawn((
                        ExposeArmorParticlePart::Sprite,
                        Mesh3d(rig.assets.quad.clone()),
                        MeshMaterial3d(materials.add(expose_armor_material(
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
                                ExposeArmorParticlePart::Streak,
                                Mesh3d(rig.assets.quad.clone()),
                                MeshMaterial3d(materials.add(expose_armor_material(
                                    color,
                                    alpha * EXPOSE_STREAK_ALPHA,
                                    None,
                                ))),
                                Transform::from_scale(Vec3::splat(1e-4)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
                let particle = commands
                    .spawn((
                        ExposeArmorParticle {
                            emitter: ei,
                            offset,
                            velocity,
                            age: 0.0,
                            life: e.life,
                            start_angle: draw(seed, 7) * TAU,
                            stature: s,
                        },
                        Transform::from_translation(offset),
                        Visibility::default(),
                    ))
                    .add_children(&parts)
                    .id();
                commands.entity(rig_entity).add_child(particle);
            }
        }
    }
}

/// Update (graphical-only): age, fall, drag and move every particle, and
/// retire it at the end of its life.
pub fn age_expose_armor_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut ExposeArmorParticle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let e = EXPOSE_ARMOR_EMITTERS[p.emitter];
        p.velocity.y -= e.gravity * p.stature * dt;
        p.velocity *= (-e.drag * EXPOSE_DRAG_SCALE * dt).exp();
        let velocity = p.velocity;
        p.offset += velocity * dt;
    }
}

/// `BodyPlacement` (PostUpdate): stand each crest over its victim's rendered
/// head and pose everything as drawn — each piece held in front of the
/// rendered body for the camera (departure 1) and turned to it —
/// the crest and glow sized, coloured and faded for the rig's age, each
/// particle sized and coloured along its life, and each streak laid back
/// along its flight as the camera sees it.
#[allow(clippy::type_complexity)]
pub fn pose_expose_armor_crests(
    mut materials: ResMut<Assets<StandardMaterial>>,
    hold: BodyHold,
    mut rigs: Query<
        (&ExposeArmorCrest, &mut Transform, &Children),
        (
            Without<ExposeArmorPiece>,
            Without<ExposeArmorParticle>,
            Without<ExposeArmorParticlePart>,
            HoldDisjoint,
        ),
    >,
    mut pieces: Query<
        (
            &ExposeArmorPiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<ExposeArmorCrest>,
            Without<ExposeArmorParticle>,
            Without<ExposeArmorParticlePart>,
            HoldDisjoint,
        ),
    >,
    mut particles: Query<
        (&ExposeArmorParticle, &mut Transform, &Children),
        (
            Without<ExposeArmorCrest>,
            Without<ExposeArmorPiece>,
            Without<ExposeArmorParticlePart>,
            HoldDisjoint,
        ),
    >,
    mut parts: Query<
        (
            &ExposeArmorParticlePart,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<ExposeArmorCrest>,
            Without<ExposeArmorPiece>,
            Without<ExposeArmorParticle>,
            HoldDisjoint,
        ),
    >,
) {
    let view = hold.view();
    let cam = view.map(|c| c.rotation).unwrap_or(Quat::IDENTITY);
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.target) else {
            continue;
        };
        let crest = expose_armor_crest_point(body.root, body.is_pet);
        rig_transform.translation = crest;
        let capsule = body.capsule();
        // Where a point truly at `at` is drawn, and its scale: held in front
        // of the body on its own view ray.
        let hold = |at: Vec3| -> (Vec3, f32) { held_in(view, at, capsule) };
        let s = stature(rig.is_pet);
        let ms = rig.age * 1000.0;
        let (fill, line, fade, size) = expose_armor_crest_look(ms);
        let (glow_color, glow_alpha) = expose_armor_glow_look(ms);
        let (drawn, k) = hold(crest);
        for child in children.iter() {
            if let Ok((piece, mut transform, material)) = pieces.get_mut(child) {
                let (color, alpha, extent) = match piece {
                    ExposeArmorPiece::Glow => (
                        glow_color,
                        glow_alpha,
                        Vec2::new(GLOW_SIZE.0, GLOW_SIZE.1) * EXPOSE_ARMOR_SIZE_MUL,
                    ),
                    ExposeArmorPiece::CrestFill => {
                        (fill, fade * CREST_FILL_ALPHA, size * CREST_SPRITE_SPAN)
                    }
                    ExposeArmorPiece::CrestLine => (line, fade, size * CREST_SPRITE_SPAN),
                };
                let extent = extent * s * k;
                transform.translation = drawn - crest;
                transform.rotation = cam;
                transform.scale = Vec3::new(extent.x.max(1e-4), extent.y.max(1e-4), 1.0);
                if let Some(m) = materials.get_mut(&material.0) {
                    recolor(m, color, alpha);
                }
                continue;
            }
            let Ok((p, mut particle_transform, part_children)) = particles.get_mut(child) else {
                continue;
            };
            let e = EXPOSE_ARMOR_EMITTERS[p.emitter];
            let at = crest + p.offset;
            let (drawn, k) = hold(at);
            particle_transform.translation = drawn - crest;
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let (color, alpha, radius) = expose_particle_look(&e, t);
            let radius = radius * p.stature * k;
            let speed = p.velocity.length();
            for part_entity in part_children.iter() {
                let Ok((part, mut transform, material)) = parts.get_mut(part_entity) else {
                    continue;
                };
                match part {
                    ExposeArmorParticlePart::Sprite => {
                        transform.rotation = cam
                            * Quat::from_rotation_z(expose_particle_roll(&e, p.start_angle, p.age));
                        transform.translation = Vec3::ZERO;
                        transform.scale = Vec3::splat((radius * 2.0).max(1e-4));
                        if let Some(m) = materials.get_mut(&material.0) {
                            recolor(m, color, alpha);
                        }
                    }
                    ExposeArmorParticlePart::Streak => {
                        let Some(length) =
                            expose_streak_length(&e, speed, EXPOSE_ARMOR_SIZE_MUL * p.stature)
                        else {
                            transform.scale = Vec3::splat(1e-4);
                            continue;
                        };
                        // The tail's far end, truly `length` back along the
                        // flight, drawn on ITS view ray at the particle's
                        // drawn depth: so the quad lies in the camera's plane
                        // and its two ends project exactly onto the particle
                        // and onto that point (departure 4).
                        let tail_at = at - p.velocity / speed * length;
                        let tail_drawn = match view {
                            Some(c) => {
                                let f = c.forward;
                                let depth = (drawn - c.eye).dot(f);
                                let tail_depth = (tail_at - c.eye).dot(f);
                                if tail_depth > 1e-4 {
                                    c.eye + (tail_at - c.eye) * (depth / tail_depth)
                                } else {
                                    tail_at
                                }
                            }
                            None => tail_at,
                        };
                        let span = tail_drawn - drawn;
                        let drawn_length = span.length();
                        let (right, up) = (cam * Vec3::X, cam * Vec3::Y);
                        let roll = span.dot(up).atan2(span.dot(right));
                        transform.rotation = cam * Quat::from_rotation_z(roll);
                        transform.translation = span * 0.5;
                        transform.scale = Vec3::new(
                            drawn_length.max(1e-4),
                            (radius * EXPOSE_STREAK_WIDTH).max(1e-4),
                            1.0,
                        );
                        if let Some(m) = materials.get_mut(&material.0) {
                            recolor(m, color, alpha * EXPOSE_STREAK_ALPHA);
                        }
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
        let mut names: Vec<&str> = EXPOSE_ARMOR_EMITTERS.iter().map(|e| e.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn the_crest_shape_is_a_heater_shield() {
        // Square shoulders, a point at the bottom.
        assert!(crest_contains(Vec2::new(0.95, -0.95)));
        assert!(crest_contains(Vec2::new(-0.95, -0.5)));
        assert!(crest_contains(Vec2::new(0.0, 0.98)));
        assert!(!crest_contains(Vec2::new(0.8, 0.9)));
        assert!(!crest_contains(Vec2::new(0.0, -1.05)));
        assert!(!crest_contains(Vec2::new(1.05, -0.5)));
    }

    #[test]
    fn the_rig_lives_until_its_last_haze_puff_dies() {
        assert!((expose_armor_rig_life() - 3.6).abs() < 1e-6);
    }
}
