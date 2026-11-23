//! The holy starburst — one gold burst at a unit's chest, at two scales
//! (AS-355): Holy Nova's hit on each enemy, and Reflective Shield's reflect on
//! the attacker it reaches.
//!
//! From the client data joined in
//! `docs/design/2026-10-17-priest-talent-client-data.md` (AS-335):
//!
//! - **High** (§4): Holy Nova's impact kit 3153, `holy_impactdd_high_chest.m2`
//!   (Holy Wrath's impact), at the chest of each enemy struck: a big gold
//!   flash about 3.3 yd across at its peak, spark showers falling under
//!   gravity 6.9, gone by 0.93 s.
//! - **Low** (§12): the reflected damage's kit 121,
//!   `holy_impactdd_low_chest.m2` (TBC), at the ATTACKER's chest with its
//!   CombatWound flinch (anim 9): the same vocabulary at a smaller scale, a
//!   2.5 yd flash, lighter showers and gold streaks.
//!
//! Played as the signed-off AS-335 Priest Talent Visuals Bench draws them
//! (https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
//! 1792207490-53c9; its source as committed with the sign-off is
//! `docs/design/benches/2026-10-17-priest-talent-bench.html`). Every layer on,
//! every multiplier at the tab's default. The pieces are the two tabs' records,
//! one for one, from the chest (`ANCH.chest`, 1.45 yd above the feet):
//!
//!   High, the "Holy Nova" tab (its spread knob at the ruling's 1.6):
//!   flash   -> `burstflash`: a camera-facing glow (`bbGlow`), 0.83 -> 3.33
//!              -> 0.03 yd across over 150-300-500 ms, its weight 0 -> 1 ->
//!              0 over 150-220-500 ms, at 0.7, (255,241,200);
//!   showers -> `burst`: four-point stars in every direction at 2.6 x 1.6 =
//!              4.16 yd/s, falling under gravity 6.9, living 0.75 s; the
//!              window opens at 200 ms at 250/s, held to 267 ms, falling to
//!              nothing at 400 ms: 33 stars, radius 0.11 -> 0.03 yd, white ->
//!              (255,220,90), fading out.
//!   Low, the "Reflective Shield" tab:
//!   flash   -> `flash`: the glow 0.83 -> 2.5 -> 0.03 yd across over
//!              0-150-400 ms, its weight 0 -> 1 -> 0 over 0-80-400 ms, at
//!              0.75;
//!   showers -> `showers`: the stars at 2.4 yd/s under gravity 6.9, 0 ->
//!              100/s at 150 ms -> 0 at 300 ms: 15 stars, radius 0.09 ->
//!              0.03 yd;
//!   streaks -> `streaks`: soft glows at 2.0 yd/s, living 0.6 s, 0 -> 150/s
//!              at 150 ms -> 0 at 300 ms: 22 of them, each with a tail
//!              (`tail` 4), (255,240,150) -> brown-gold (212,131,58), radius
//!              0.05 -> 0.03 yd.
//!
//! Every piece is composited `lighter` by the bench (`bbGlow`, and emitters
//! that pass no `blend`), so every piece here is `AlphaMode::Add`.
//!
//! **One helper, two scales.** [`starburst_spec`] is the one table: a scale's
//! flash, its emitters and whether it flinches. Both landings route to it
//! through [`StarburstLanding`], the cosmetic marker the area-hit landing and
//! the reflect's follow-up spawn.
//!
//! **Births are fixed.** Each emitter's births are where the integral of its
//! rate track first reaches 1, 2, ... (the bench's accumulator in continuous
//! time, `inner_focus::track_births`), and a particle's look and flight are a
//! pure function of its birth and its own age, so the count, the flight and
//! every look are the same at any frame rate.
//!
//! **Lifecycle.** A landing plays out on a unit that dies under it (the
//! killing blow included) and on one polymorphed under it (the Conflagrate
//! precedent: a landing is an event, not a state the body wears). It goes at
//! once with a unit that is gone. Stealth owns the body's material, which
//! nothing here touches. Everything is a `PlayMatchEntity`, so a match that
//! ends mid-burst takes all of it.
//!
//! **Departures from the bench**, each deliberate:
//!
//! 1. **Held in front of the body.** The bench paints the burst over its
//!    unit. Here the body is an `AlphaMode::Blend` capsule that writes no
//!    depth, and the transparent pass sorts by each mesh's translation, so
//!    every piece is drawn on its own view ray in front of the unit's
//!    RENDERED body and scaled by the same ratio (the shared body hold,
//!    `body_hold.rs`): the same screen footprint, sorted in front.
//! 2. **Flight is integrated exactly.** The bench steps velocity then
//!    position each frame; here a particle is at `v t − ½ g t²`, the same
//!    curve at any frame rate.
//! 3. **The stars' white core is tinted** (the shared star sprite takes the
//!    star's colour throughout), and a streak is square-ended where the
//!    bench's is round, as Conflagrate's are.
//! 4. **The flash follows its unit in height too**, the rig standing at the
//!    rendered chest. The particles do not follow: each is born where the
//!    chest is on its birth frame and flies free, as the bench's do.
//! 5. **Reflective Shield's tab draws a ring round the Priest** at its chest:
//!    a viewing aid standing in for the shield, which our Power Word: Shield
//!    cards draw. It is not part of the burst and is not drawn.
//! 6. **A pet's burst is at its stature**, its sizes, speeds and fall scaled
//!    as every other landing on a pet is.
//!
//! **At the match camera** (60 yd, pitch 38.7°, FOV 45°, about 22 px a yard at
//! 1080p): the High flash peaks about 73 px across at 0.7, the Low one 55 px
//! at 0.75; a shower star is 2-5 px.
//!
//! Graphical-only: nothing here writes sim state, no `game_rng` is drawn (the
//! scatter is the benches' shared hash), and every system is registered in
//! `states/mod.rs` only. Headless never runs any of it.

use std::f32::consts::TAU;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{held_in, BodyCapsule, BodyHold, HoldDisjoint, HoldView};
use super::conflagrate::{conflagrate_material, ConflagrateSprite, CONFLAGRATE_GLOW};
use super::druid_heals::keyed;
use super::hit_reaction::{insert_flinch, PendingDepth};
use super::inner_focus::{focus_track_rgb, track_births};
use super::paladin_oneshots::{bench_hash, oneshot_glow_texture};
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use crate::states::play_match::components::*;

// ── The benches' constants blocks (signed off at version 1792207490-53c9) ───

// Holy Nova: holy_impactdd_high_chest.m2 (kit 3153) on each enemy hit. AS-335 bench.
pub const HOLY_NOVA_ENEMY_STARBURST: bool = true;
pub const HOLY_NOVA_SIZE_MUL: f32 = 1.00;
// Reflective Shield: holy_impactdd_low_chest.m2 (kit 121) on the ATTACKER per reflect, plus the CombatWound flinch. AS-335 bench.
pub const REFLECTIVE_SHIELD_BURST: bool = true; // shares Holy Nova's starburst at 2.5 yd
pub const REFLECTIVE_SHIELD_SIZE_MUL: f32 = 1.00;

/// The Holy Nova tab's spread knob at the ruling (2026-10-17, "the dome shows
/// the true 10 yd reach (spread 1.6)"). The tab's emitters all wear it, so the
/// High showers fly at 2.6 x 1.6 yd/s.
pub const HOLY_NOVA_BENCH_SPREAD: f32 = 1.6;

// ── Geometry ─────────────────────────────────────────────────────────────────

/// The chest attachment, yards above the feet (the bench's `ANCH.chest`).
pub const STARBURST_CHEST: f32 = 1.45;
/// The bench draws nothing at or below this alpha (`a <= 0.003` returns).
pub const STARBURST_ALPHA_FLOOR: f32 = 0.003;
/// A streak is drawn only above this speed, yd/s (the bench's `sp > 0.2`).
pub const STARBURST_STREAK_MIN_SPEED: f32 = 0.2;
/// The bench strokes a streak at 0.85 of its sprite's alpha, 0.7 of its
/// radius wide, `min(tail × 0.12, speed × 0.06)` long.
pub const STARBURST_STREAK_ALPHA: f32 = 0.85;
pub const STARBURST_STREAK_WIDTH: f32 = 0.7;
const STREAK_PER_TAIL: f32 = 0.12;
const STREAK_PER_SPEED: f32 = 0.06;
/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 64;

// ── The table ────────────────────────────────────────────────────────────────

/// The flash: the bench's `bbGlow` at the chest.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StarburstFlash {
    /// Its width across, yards, keyed in milliseconds.
    pub size: &'static [(f32, f32)],
    /// Its weight, keyed in milliseconds.
    pub weight: &'static [(f32, f32)],
    /// The weight's gain into the drawn alpha.
    pub gain: f32,
    pub rgb: [u8; 3],
}

/// The flash's colour, both scales (`[255,241,200]`).
pub const STARBURST_FLASH_RGB: [u8; 3] = [255, 241, 200];

/// One particle emitter, as the bench's layer record carries it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StarburstEmitter {
    /// Unique key.
    pub name: &'static str,
    pub sprite: ConflagrateSprite,
    /// The rate, particles a second, keyed in SECONDS over the emitter's open
    /// window only (the bench's `rate` read inside its `win`).
    pub rate: &'static [(f32, f32)],
    /// Launch speed, yd/s, in every direction (`vRange` 3.14).
    pub speed: f32,
    /// Downward acceleration, yd/s².
    pub gravity: f32,
    pub life: f32,
    pub rgb: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// The sprite's RADIUS, yards, over its life (the bench's `scale`).
    pub radius: &'static [(f32, f32)],
    /// Tail length factor; `0.0` draws no streak.
    pub tail: f32,
}

/// One scale of the starburst.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StarburstSpec {
    /// The bench's layer toggles for this landing.
    pub on: bool,
    /// The tab's size knob.
    pub size_mul: f32,
    pub flash: StarburstFlash,
    pub emitters: &'static [StarburstEmitter],
    /// Whether the landing flinches its unit (the client's anim 9).
    pub flinch: bool,
}

const SHOWER_RGB: &[(f32, [u8; 3])] = &[(0.0, [255, 255, 255]), (1.0, [255, 220, 90])];
const FADE_OUT: &[(f32, f32)] = &[(0.0, 1.0), (1.0, 0.0)];

/// `holy_impactdd_high_chest.m2`'s emitters as the Holy Nova tab's `burst`
/// layer.
pub const HIGH_EMITTERS: [StarburstEmitter; 1] = [StarburstEmitter {
    name: "holy_impactdd_high_chest/burst showers",
    sprite: ConflagrateSprite::Star,
    rate: &[(0.2, 250.0), (0.267, 250.0), (0.4, 0.0)],
    speed: 2.6 * HOLY_NOVA_BENCH_SPREAD,
    gravity: 6.9,
    life: 0.75,
    rgb: SHOWER_RGB,
    alpha: FADE_OUT,
    radius: &[(0.0, 0.11), (1.0, 0.03)],
    tail: 0.0,
}];

/// `holy_impactdd_low_chest.m2`'s emitters as the Reflective Shield tab's
/// `showers` and `streaks` layers.
pub const LOW_EMITTERS: [StarburstEmitter; 2] = [
    StarburstEmitter {
        name: "holy_impactdd_low_chest/showers",
        sprite: ConflagrateSprite::Star,
        rate: &[(0.0, 0.0), (0.15, 100.0), (0.3, 0.0)],
        speed: 2.4,
        gravity: 6.9,
        life: 0.75,
        rgb: SHOWER_RGB,
        alpha: FADE_OUT,
        radius: &[(0.0, 0.09), (1.0, 0.03)],
        tail: 0.0,
    },
    StarburstEmitter {
        name: "holy_impactdd_low_chest/streaks",
        sprite: ConflagrateSprite::Glow,
        rate: &[(0.0, 0.0), (0.15, 150.0), (0.3, 0.0)],
        speed: 2.0,
        gravity: 0.0,
        life: 0.6,
        rgb: &[(0.0, [255, 240, 150]), (1.0, [212, 131, 58])],
        alpha: FADE_OUT,
        radius: &[(0.0, 0.05), (1.0, 0.03)],
        tail: 4.0,
    },
];

const HIGH: StarburstSpec = StarburstSpec {
    on: HOLY_NOVA_ENEMY_STARBURST,
    size_mul: HOLY_NOVA_SIZE_MUL,
    flash: StarburstFlash {
        size: &[(150.0, 0.83), (300.0, 3.33), (500.0, 0.03)],
        weight: &[(150.0, 0.0), (220.0, 1.0), (500.0, 0.0)],
        gain: 0.7,
        rgb: STARBURST_FLASH_RGB,
    },
    emitters: &HIGH_EMITTERS,
    flinch: false,
};

const LOW: StarburstSpec = StarburstSpec {
    on: REFLECTIVE_SHIELD_BURST,
    size_mul: REFLECTIVE_SHIELD_SIZE_MUL,
    flash: StarburstFlash {
        size: &[(0.0, 0.83), (150.0, 2.5), (400.0, 0.03)],
        weight: &[(0.0, 0.0), (80.0, 1.0), (400.0, 0.0)],
        gain: 0.75,
        rgb: STARBURST_FLASH_RGB,
    },
    emitters: &LOW_EMITTERS,
    flinch: true,
};

/// The one table: what a scale draws.
pub fn starburst_spec(scale: StarburstScale) -> &'static StarburstSpec {
    match scale {
        StarburstScale::High => &HIGH,
        StarburstScale::Low => &LOW,
    }
}

// ── Poses ────────────────────────────────────────────────────────────────────

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// The flash `age` seconds after the landing: its width across and its
/// alpha, or `None` while the bench draws nothing.
pub fn starburst_flash(scale: StarburstScale, age: f32, is_pet: bool) -> Option<(f32, f32)> {
    let spec = starburst_spec(scale);
    if !spec.on {
        return None;
    }
    let ms = age * 1000.0;
    let alpha = keyed(spec.flash.weight, ms) * spec.flash.gain;
    (alpha > STARBURST_ALPHA_FLOOR).then(|| {
        (
            keyed(spec.flash.size, ms) * spec.size_mul * stature(is_pet),
            alpha,
        )
    })
}

/// Every birth of emitter `e`, seconds after the landing.
pub fn starburst_births(e: &StarburstEmitter) -> Vec<f32> {
    track_births(e.rate)
}

/// The bench's scatter for particle `j` of emitter `layer`, draw `k`, in the
/// rig seeded `seed`: 0..1.
fn scatter(seed: u32, layer: u32, j: u32, k: u32) -> f32 {
    bench_hash(
        seed.wrapping_mul(977)
            .wrapping_add(j)
            .wrapping_mul(4)
            .wrapping_add(layer)
            .wrapping_mul(8)
            .wrapping_add(k)
            .wrapping_add(355),
    )
}

/// The bench's `vRange` 3.14: a point of the cube `[-1, 1]³`, normalised.
pub fn starburst_direction(seed: u32, layer: u32, j: u32) -> Vec3 {
    Vec3::new(
        scatter(seed, layer, j, 1) * 2.0 - 1.0,
        scatter(seed, layer, j, 2) * 2.0 - 1.0,
        scatter(seed, layer, j, 3) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y)
}

/// One particle's look at a moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StarburstParticle {
    /// Where it truly is, from where it was born.
    pub at: Vec3,
    /// Its velocity now.
    pub velocity: Vec3,
    /// The sprite's full width, yards.
    pub size: f32,
    pub rgb: [u8; 3],
    pub alpha: f32,
    /// Its roll in the camera's plane: fixed, as no layer spins.
    pub roll: f32,
}

/// Particle `j` of the scale's emitter `layer`, born at `born`, `age` seconds
/// after the landing, in the rig seeded `seed`; `None` before its birth and
/// after its life. Pure: the probes and the placement read one definition.
pub fn starburst_particle(
    scale: StarburstScale,
    layer: usize,
    j: u32,
    born: f32,
    age: f32,
    seed: u32,
    is_pet: bool,
) -> Option<StarburstParticle> {
    let spec = starburst_spec(scale);
    let e = spec.emitters.get(layer)?;
    let page = age - born;
    if !spec.on || !(0.0..e.life).contains(&page) {
        return None;
    }
    let s = stature(is_pet);
    let t = page / e.life;
    let launch = starburst_direction(seed, layer as u32, j) * e.speed * s;
    let fall = e.gravity * s;
    Some(StarburstParticle {
        at: launch * page - Vec3::Y * (0.5 * fall * page * page),
        velocity: launch - Vec3::Y * (fall * page),
        size: 2.0 * keyed(e.radius, t) * spec.size_mul * s,
        rgb: focus_track_rgb(e.rgb, t),
        alpha: keyed(e.alpha, t),
        // The canvas turns a sprite clockwise on screen.
        roll: -scatter(seed, layer as u32, j, 7) * TAU,
    })
}

/// A particle's streak length behind it, yards (the bench's
/// `min(tail × 0.12, speed × 0.06) × size`), or `None` when it draws none.
pub fn starburst_streak_length(
    scale: StarburstScale,
    e: &StarburstEmitter,
    speed: f32,
    is_pet: bool,
) -> Option<f32> {
    (e.tail > 0.0 && speed > STARBURST_STREAK_MIN_SPEED).then(|| {
        (e.tail * STREAK_PER_TAIL).min(speed * STREAK_PER_SPEED)
            * starburst_spec(scale).size_mul
            * stature(is_pet)
    })
}

/// The rig's whole life: until its flash has gone and its last particle has
/// lived out its life.
pub fn starburst_secs(scale: StarburstScale) -> f32 {
    let spec = starburst_spec(scale);
    let flash = spec.flash.weight[spec.flash.weight.len() - 1].0 / 1000.0;
    spec.emitters
        .iter()
        .map(|e| starburst_births(e).last().map_or(0.0, |b| b + e.life))
        .fold(flash, f32::max)
}

// ── Components ───────────────────────────────────────────────────────────────

/// One landing: it stands at its unit's rendered chest and is the parent of
/// the flash and every particle.
#[derive(Component, Debug)]
pub struct HolyStarburst {
    pub target: Entity,
    pub scale: StarburstScale,
    pub age: f32,
    /// The scatter's seed.
    pub seed: u32,
    pub is_pet: bool,
}

/// One drawn piece of a [`HolyStarburst`].
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum StarburstPiece {
    /// The glow at the chest.
    Flash,
    /// Particle `j` of emitter `layer`, born `born` seconds in, and where the
    /// chest stood on its birth frame (`None` until then).
    Particle {
        layer: usize,
        j: u32,
        born: f32,
        born_at: Option<Vec3>,
    },
    /// The streak behind that particle (an emitter with a tail).
    Streak { layer: usize, j: u32, born: f32 },
}

/// The quad and sprites every piece is drawn with, built once.
#[derive(Clone)]
pub struct StarburstAssets {
    pub quad: Handle<Mesh>,
    pub flash: Handle<Image>,
    pub star: Handle<Image>,
    pub glow: Handle<Image>,
}

impl StarburstAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        let sprite = |images: &mut Assets<Image>, kind: ConflagrateSprite| {
            images.add(disc_texture(SPRITE_PX, move |r, a| kind.alpha_at(r, a)))
        };
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            flash: images.add(oneshot_glow_texture()),
            star: sprite(images, ConflagrateSprite::Star),
            glow: sprite(images, ConflagrateSprite::Glow),
        }
    }

    fn sprite(&self, kind: ConflagrateSprite) -> Handle<Image> {
        match kind {
            ConflagrateSprite::Star => self.star.clone(),
            _ => self.glow.clone(),
        }
    }
}

/// Write a piece's colour and alpha into its additive material, touching the
/// asset only when the look moved (`get_mut` marks it changed and re-uploads
/// it).
pub(super) fn set_additive_look(
    materials: &mut Assets<StandardMaterial>,
    handle: &Handle<StandardMaterial>,
    rgb: [u8; 3],
    alpha: f32,
) {
    let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
    let base = color.with_alpha(alpha);
    if materials.get(handle).is_some_and(|m| m.base_color != base) {
        if let Some(m) = materials.get_mut(handle) {
            let c = color.to_linear();
            m.base_color = base;
            m.emissive = LinearRgba::rgb(
                c.red * CONFLAGRATE_GLOW,
                c.green * CONFLAGRATE_GLOW,
                c.blue * CONFLAGRATE_GLOW,
            );
        }
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): a starburst for every landing the sim spawned,
/// its whole rig built at once, and the flinch a reflect's carries (the
/// shipped `hit_reaction.rs` dip). A unit that is gone gets nothing.
pub fn spawn_holy_starbursts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<StarburstAssets>>,
    landings: Query<(Entity, &StarburstLanding)>,
    units: Query<(Has<Pet>, Option<&HitFlinch>), With<Combatant>>,
) {
    let mut pending = PendingDepth::new();
    for (marker, landing) in landings.iter() {
        commands.entity(marker).despawn();
        let Ok((is_pet, flinch)) = units.get(landing.target) else {
            continue;
        };
        let spec = starburst_spec(landing.scale);
        if !spec.on {
            continue;
        }
        if spec.flinch {
            insert_flinch(
                &mut commands,
                &mut pending,
                landing.target,
                is_pet,
                flinch,
                false,
            );
        }
        let assets = assets
            .get_or_insert_with(|| StarburstAssets::build(&mut meshes, &mut images))
            .clone();
        let mut pieces = vec![(StarburstPiece::Flash, Some(assets.flash.clone()), [255; 3])];
        for (layer, e) in spec.emitters.iter().enumerate() {
            for (j, born) in starburst_births(e).into_iter().enumerate() {
                let j = j as u32;
                pieces.push((
                    StarburstPiece::Particle {
                        layer,
                        j,
                        born,
                        born_at: None,
                    },
                    Some(assets.sprite(e.sprite)),
                    e.rgb[0].1,
                ));
                if e.tail > 0.0 {
                    // The bench strokes a solid line.
                    pieces.push((StarburstPiece::Streak { layer, j, born }, None, e.rgb[0].1));
                }
            }
        }
        let children: Vec<Entity> = pieces
            .into_iter()
            .map(|(piece, sprite, rgb)| {
                let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
                commands
                    .spawn((
                        piece,
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(
                            materials.add(conflagrate_material(true, color, 0.0, sprite)),
                        ),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        Visibility::Hidden,
                        NotShadowCaster,
                    ))
                    .id()
            })
            .collect();
        let rig = commands
            .spawn((Transform::default(), Visibility::default(), PlayMatchEntity))
            .id();
        commands
            .entity(rig)
            .insert(HolyStarburst {
                target: landing.target,
                scale: landing.scale,
                age: 0.0,
                seed: rig.index(),
                is_pet,
            })
            .add_children(&children);
    }
}

/// Update (graphical-only): age each starburst, and retire it after its life
/// or at once when its unit is gone. The one owner of a rig's end.
pub fn animate_holy_starbursts(
    mut commands: Commands,
    time: Res<Time>,
    mut rigs: Query<(Entity, &mut HolyStarburst)>,
    units: Query<(), With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (entity, mut rig) in rigs.iter_mut() {
        if units.get(rig.target).is_err() {
            commands.entity(entity).despawn();
            continue;
        }
        rig.age += dt;
        if rig.age >= starburst_secs(rig.scale) {
            commands.entity(entity).despawn();
        }
    }
}

/// Lay a camera-facing quad's long (local X) axis along `back` as the camera
/// sees it.
fn streak_roll(view: Option<HoldView>, back: Vec3) -> Quat {
    let cam = view.map(|v| v.rotation).unwrap_or_default();
    let (right, up) = (cam * Vec3::X, cam * Vec3::Y);
    cam * Quat::from_rotation_z(back.dot(up).atan2(back.dot(right)))
}

/// `BodyPlacement` (PostUpdate): stand each starburst at its unit's RENDERED
/// chest, and place each piece where it is drawn — held in front of that
/// body for the camera — turned to the camera, sized and coloured for its
/// age. The rig is unrotated, so a piece's local pose is its world offset.
pub fn place_holy_starbursts(
    hold: BodyHold,
    mut rigs: Query<
        (&HolyStarburst, &mut Transform, &Children),
        (Without<StarburstPiece>, HoldDisjoint),
    >,
    mut pieces: Query<
        (
            &mut StarburstPiece,
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<HolyStarburst>, HoldDisjoint),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let view = hold.view();
    let cam = view.map(|v| v.rotation).unwrap_or_default();
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.target) else {
            continue;
        };
        let chest = body.point(STARBURST_CHEST);
        rig_transform.translation = chest;
        let capsule = body.capsule();
        let spec = starburst_spec(rig.scale);
        // The particles' true positions, read once for their streaks.
        let mut particles: Vec<(usize, u32, Vec3, StarburstParticle)> = Vec::new();
        for child in children.iter() {
            let Ok((mut piece, ..)) = pieces.get_mut(child) else {
                continue;
            };
            if let StarburstPiece::Particle {
                layer,
                j,
                born,
                ref mut born_at,
            } = *piece
            {
                match starburst_particle(rig.scale, layer, j, born, rig.age, rig.seed, rig.is_pet) {
                    Some(p) => {
                        let origin = *born_at.get_or_insert(chest);
                        particles.push((layer, j, origin + p.at, p));
                    }
                    None => *born_at = None,
                }
            }
        }
        for child in children.iter() {
            let Ok((piece, mut transform, mut visibility, material)) = pieces.get_mut(child) else {
                continue;
            };
            let drawn = match *piece {
                StarburstPiece::Flash => {
                    starburst_flash(rig.scale, rig.age, rig.is_pet).map(|(size, alpha)| {
                        let (at, k) = held_in(view, chest, capsule);
                        (
                            at,
                            cam,
                            Vec3::new(size * k, size * k, 1.0),
                            spec.flash.rgb,
                            alpha,
                        )
                    })
                }
                StarburstPiece::Particle { layer, j, .. } => particles
                    .iter()
                    .find(|(l, i, ..)| *l == layer && *i == j)
                    .map(|&(_, _, at, p)| {
                        let (at, k) = held_in(view, at, capsule);
                        (
                            at,
                            cam * Quat::from_rotation_z(p.roll),
                            Vec3::new(p.size * k, p.size * k, 1.0),
                            p.rgb,
                            p.alpha,
                        )
                    }),
                StarburstPiece::Streak { layer, j, .. } => particles
                    .iter()
                    .find(|(l, i, ..)| *l == layer && *i == j)
                    .and_then(|&(_, _, at, p)| {
                        streak_pose(view, capsule, rig, layer, at, p).map(
                            |(mid, rotation, length, width)| {
                                (
                                    mid,
                                    rotation,
                                    Vec3::new(length, width, 1.0),
                                    p.rgb,
                                    p.alpha * STARBURST_STREAK_ALPHA,
                                )
                            },
                        )
                    }),
            };
            match drawn {
                Some((at, rotation, scale, rgb, alpha)) => {
                    transform.translation = at - chest;
                    transform.rotation = rotation;
                    transform.scale = scale.max(Vec3::splat(1e-4));
                    visibility.set_if_neq(Visibility::Inherited);
                    set_additive_look(&mut materials, &material.0, rgb, alpha.clamp(0.0, 1.0));
                }
                None => {
                    transform.scale = Vec3::splat(1e-4);
                    visibility.set_if_neq(Visibility::Hidden);
                }
            }
        }
    }
}

/// A particle's streak as it is drawn: the bench's screen line, laid in the
/// camera's plane at its particle's held depth with its projected length,
/// back along the flight as the camera sees it (Conflagrate's streak). Its
/// midpoint, rotation, length and width; `None` when it draws none.
fn streak_pose(
    view: Option<HoldView>,
    capsule: BodyCapsule,
    rig: &HolyStarburst,
    layer: usize,
    at: Vec3,
    p: StarburstParticle,
) -> Option<(Vec3, Quat, f32, f32)> {
    let e = starburst_spec(rig.scale).emitters.get(layer)?;
    let speed = p.velocity.length();
    let length = starburst_streak_length(rig.scale, e, speed, rig.is_pet)?;
    let (drawn, k) = held_in(view, at, capsule);
    let back = -p.velocity / speed;
    let (along, length) = match view {
        Some(v) => {
            let flat = back - v.forward * back.dot(v.forward);
            (flat.normalize_or_zero(), length * flat.length() * k)
        }
        None => (back, length),
    };
    Some((
        drawn + along * (length * 0.5),
        streak_roll(view, back),
        length,
        p.size * 0.5 * k * STARBURST_STREAK_WIDTH,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The High showers bear 33 stars, the Low showers 15 and its streaks 22.
    #[test]
    fn each_emitter_bears_the_benchs_count() {
        let counts = |scale| -> Vec<usize> {
            starburst_spec(scale)
                .emitters
                .iter()
                .map(|e| starburst_births(e).len())
                .collect()
        };
        assert_eq!(counts(StarburstScale::High), vec![33]);
        assert_eq!(counts(StarburstScale::Low), vec![15, 22]);
    }

    #[test]
    fn every_emitter_has_a_unique_name() {
        let mut names: Vec<&str> = HIGH_EMITTERS
            .iter()
            .chain(LOW_EMITTERS.iter())
            .map(|e| e.name)
            .collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }
}
