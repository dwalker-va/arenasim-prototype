use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::sync::Arc;

use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::dot_state::{has_dot_state, DotStateOn, DotStateVisual};
use super::druid_heals::{body_point, keyed, keyed_color};
use super::hunter_shots::draw;
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use super::spell_bolts::{soft_band_texture, star_flash_texture};
use crate::states::play_match::components::*;

// ==============================================================================
// Siphon Life — the TBC client's violet chest glow, held while the DoT runs
// ==============================================================================
//
// Client data (`docs/design/2026-10-17-warlock-talent-client-data.md` §3):
// in the Era client Siphon Life is Corruption's whole visual (visual 381), so
// a victim with both carries the same state twice. TBC gives it its own state
// — visual 8687, `(7,8)` kit 234, `corruption_impactdot_med_base.m2` on the
// victim's chest — and the user ruled for TBC (AS-334). That model is three
// sprite emitters and no mesh: violet rings and violet flares bursting out of
// the chest and shrinking as they fly, and dark streaks pulled in from a shell
// around it. Additive, constant (no pulse key), following the victim.
//
// - **Landing:** Corruption's shipped apply (kit 117, the same `(6,13)` row
//   in both builds). `warlock_dots::spawn_warlock_dot_visuals` is the one
//   detector that lands kit 117: as the state appears it builds this rig
//   ([`spawn_siphon_life_rig`]) beside the burst, as it does for UA —
//   transition-in only, so a refresh does not replay it.
// - **The heal back to the Warlock** draws nothing: aura 53 is a periodic
//   leech with no missile, channel or caster row in either build. The green
//   heal numbers carry it. (The lands-silently audit's heal family does not
//   reach a DoT's leech, so there is no entry to make.)
// - Kit 234 is also TBC's Unstable Affliction state; ours is authored (the
//   AS-37 ruling), so the two never meet.
//
// The emitter table is the signed-off AS-334 Warlock Talent Visuals Bench
// (version 1792203924-603a, "Siphon Life" tab) layer for layer, at its
// defaults: all three layers on, every multiplier 1.0. Its `Emitter` is the
// one `shadow_word_pain.rs` ports; each particle follows the victim
// (`follow: true` on all three layers).
//
// ## Departures from the bench (each deliberate)
//
// 1. **Held in front of the body.** The bench is a 2D canvas: it draws the
//    bodies first and every particle over them. Here the body is an
//    `AlphaMode::Blend` mesh, which writes no depth, so what decides the
//    overlap is the transparent pass's sort by each mesh's translation: a
//    particle deeper than the body's centre is drawn first and the body
//    paints over it — about half the burst would vanish behind the victim.
//    Each particle is therefore drawn on its own view ray, slid toward the
//    camera until it stands nearer than the body's nearest point (the shared
//    body hold, `body_hold.rs`; stricter than the sort needs, and the same
//    rule keeps it clear of an opaque body if one ever is), and scaled by the
//    same ratio, so it covers the exact screen footprint the bench gives it.
//    The hold also sorts the glow in front of a stacked Corruption shroud.
// 2. **The flare's white-hot core is tinted.** The bench draws a star's
//    centre white over its coloured rays; the shared star sprite here is
//    white throughout and takes the flare colour, as Shadow Word: Pain's do.
// 3. **Follows in height too.** The bench's `follow` moves particles with the
//    victim in x and z only; here they ride the rig, so a victim on a ramp
//    keeps its glow.
// 4. **Hidden on a sheep.** A polymorphed victim wears no DoT state (its
//    clock runs on): the one sheep rule, `dot_state::DotStateOn`.
// 6. **Rides the rendered body.** The rig stands at the chest of the
//    victim's drawn body (`VisualBody`: the walk and fear bobs, the swing
//    step, the Cyclone lift), placed after every writer of it
//    ([`place_siphon_life_glows`]).
// 5. **The streak is a soft band.** The bench strokes a solid line with round
//    caps; here it is the shared `soft_band_texture` quad, which fades across
//    its width and has square ends. At the match camera it is ~2 px wide.
//
// Where the research doc's table and the bench differ, the bench was signed
// off and wins: the rings' alpha (0.55 → 0.7 → 0.4, not 0.78 → 1 → 0.78), the
// flares' size (0.3 → 0.15 → 0.06, not the rings' 1.39 → 0.28), and the
// streaks' life (0.55 s, not 0.8 s). The card's prose matches the bench.
//
// ## At the match camera (60 yd, pitch 38.7°, FOV 45°)
//
// About 22 px a yard at 1080p: a newborn ring is ~60 px across, a flare
// ~13 px, a streak's head ~6 px with a ~4 px tail. Nothing needs resizing.
//
// House rules: spawn / animate / age / billboard / cleanup systems,
// `Res<Time>`, registered in `states/mod.rs` only (headless never runs any of
// it and stays byte-identical by construction), no `game_rng` — all scatter is
// hash-seeded off the rig's entity index and emission count.

// ── Bench sign-off constants (AS-334, version 1792203924-603a) ───────────────

// Siphon Life DoT state: TBC kit 234 (corruption_impactdot_med_base.m2) on the victim's chest. AS-334 bench.
// DotStateVisual::for_dot("Siphon Life") -> SiphonLifeGlow (new route)
pub const SIPHON_LIFE_RINGS: bool = true;
pub const SIPHON_LIFE_FLARES: bool = true;
pub const SIPHON_LIFE_INWARD_STREAKS: bool = true;
pub const SIPHON_LIFE_SIZE_MUL: f32 = 1.00;
pub const SIPHON_LIFE_DENSITY_MUL: f32 = 1.00;
pub const SIPHON_LIFE_OPACITY_MUL: f32 = 1.00;
// Landing: Corruption's shipped apply (kit 117). Heal back: floating text only.

/// Siphon Life's aura name (the RON `name:` string).
pub const SIPHON_LIFE_AURA: &str = "Siphon Life";

/// The bench's chest anchor, above the floor (`ANCH.chest`; attachment 34).
pub const SIPHON_CHEST_ABOVE_FLOOR: f32 = 1.45;

// ── Rendering constants ──────────────────────────────────────────────────────

/// Emissive gain of an additive particle over its colour — the house's 2-4x
/// glow convention (`shadow_word_pain.rs` uses the same).
const PARTICLE_GLOW: f32 = 2.5;
/// Steps in a particle's colour/alpha palette. Entry `i` is the track at
/// `i / (SIPHON_PALETTE_STEPS - 1)` of a life, so the tracks' keys at 0, 0.5
/// and 1 are drawn exactly.
pub const SIPHON_PALETTE_STEPS: usize = 17;
/// A streak is drawn only above this speed, yd/s (the bench's `sp > 0.2`).
const STREAK_MIN_SPEED: f32 = 0.2;
/// The bench draws the streak at 0.85 of the sprite's alpha.
const STREAK_ALPHA: f32 = 0.85;
/// The bench's streak line is 0.7 of the sprite's radius wide.
const STREAK_WIDTH: f32 = 0.7;
/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 64;

// ── The emitter model (the bench's `Emitter`, as Shadow Word: Pain ports it) ─

/// What a particle's sprite is drawn with — the bench's sprite `kind`s.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SiphonSprite {
    /// `glow`: a soft round light, falling to 0.45 at mid-radius.
    Glow,
    /// `star`: a four-point star (`flare`).
    Star,
}

/// How a particle launches.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SiphonLaunch {
    /// Out of the chest at the emitter's speed, in a direction on the sphere
    /// (the bench's `vRange >= 3.1`: a normalised cube sample).
    Burst,
    /// Born on a shell about the chest — `0.6..1.0` of `shell` out, along a
    /// normalised cube sample — and travelling straight in (the bench's
    /// `inward`).
    Inward { shell: f32 },
}

/// One emitter, as the bench's layer record carries it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SiphonEmitter {
    /// Unique key, model and emitter.
    pub name: &'static str,
    pub sprite: SiphonSprite,
    /// M2 blend 4 (additive) when `true`; blend 2 (alpha), which darkens what
    /// is behind it, when `false`.
    pub additive: bool,
    pub launch: SiphonLaunch,
    /// Launch speed, yd/s (its magnitude, for an inward launch).
    pub speed: f32,
    pub life: f32,
    /// Particles per second.
    pub rate: f32,
    /// Colour, alpha and scale tracks over the particle's normalised life.
    pub color: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// Sprite RADIUS, yards.
    pub scale: &'static [(f32, f32)],
    /// Tail length factor; `0.0` draws no streak.
    pub tail: f32,
}

const VIOLET: &[(f32, [u8; 3])] = &[
    (0.0, [22, 0, 133]),
    (0.5, [83, 0, 186]),
    (1.0, [112, 0, 183]),
];

/// `corruption_impactdot_med_base.m2`'s emitters as the bench's three layers.
pub const SIPHON_LIFE_EMITTERS: [SiphonEmitter; 3] = [
    // P0 shockwave4white: violet rings bursting out and shrinking.
    SiphonEmitter {
        name: "corruption_impactdot_med_base/P0 violet rings",
        sprite: SiphonSprite::Glow,
        additive: true,
        launch: SiphonLaunch::Burst,
        speed: 1.67,
        life: 1.5,
        rate: 30.0,
        color: VIOLET,
        alpha: &[(0.0, 0.55), (0.5, 0.7), (1.0, 0.4)],
        scale: &[(0.0, 1.39), (0.5, 0.56), (1.0, 0.28)],
        tail: 0.0,
    },
    // P1 flare: violet flares, the rings' motion.
    SiphonEmitter {
        name: "corruption_impactdot_med_base/P1 violet flares",
        sprite: SiphonSprite::Star,
        additive: true,
        launch: SiphonLaunch::Burst,
        speed: 1.67,
        life: 1.5,
        rate: 30.0,
        color: VIOLET,
        alpha: &[(0.0, 0.78), (0.5, 1.0), (1.0, 0.78)],
        scale: &[(0.0, 0.3), (0.5, 0.15), (1.0, 0.06)],
        tail: 0.0,
    },
    // P2 flare, blend 2: dark streaks pulled in from a 2.22 yd shell.
    SiphonEmitter {
        name: "corruption_impactdot_med_base/P2 inward dark streaks",
        sprite: SiphonSprite::Glow,
        additive: false,
        launch: SiphonLaunch::Inward { shell: 2.22 },
        speed: 3.89,
        life: 0.55,
        rate: 25.0,
        color: &[(0.0, [0, 0, 0]), (0.5, [37, 0, 82]), (1.0, [112, 0, 183])],
        alpha: &[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.139), (1.0, 0.139)],
        tail: 1.5,
    },
];

/// Which of [`SIPHON_LIFE_EMITTERS`] are kept layers, in table order.
pub const SIPHON_LIFE_LAYERS: [bool; 3] = [
    SIPHON_LIFE_RINGS,
    SIPHON_LIFE_FLARES,
    SIPHON_LIFE_INWARD_STREAKS,
];

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// The chest the rig stands at, for a victim at `translation`.
pub fn siphon_chest(translation: Vec3, is_pet: bool) -> Vec3 {
    body_point(SIPHON_CHEST_ABOVE_FLOOR, translation, is_pet)
}

/// A unit direction from three scatter draws: the bench's normalised cube
/// sample, `norm([rnd*2-1, rnd*2-1, rnd*2-1])`.
fn cube_direction(seed: u32) -> Vec3 {
    Vec3::new(
        draw(seed, 1) * 2.0 - 1.0,
        draw(seed, 2) * 2.0 - 1.0,
        draw(seed, 3) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y)
}

/// Where a particle of `e` is born, relative to the chest, and how it
/// launches (the bench's `spawn`, at spread 1.0), on a body of `stature`.
pub fn siphon_particle_launch(e: &SiphonEmitter, stature: f32, seed: u32) -> (Vec3, Vec3) {
    let d = cube_direction(seed);
    match e.launch {
        SiphonLaunch::Burst => (Vec3::ZERO, d * e.speed * stature),
        SiphonLaunch::Inward { shell } => {
            let r = shell * stature * (0.6 + 0.4 * draw(seed, 4));
            (d * r, -d * e.speed * stature)
        }
    }
}

/// A particle's sprite radius at life fraction `t`, yards.
pub fn siphon_radius(e: &SiphonEmitter, t: f32, stature: f32) -> f32 {
    keyed(e.scale, t) * SIPHON_LIFE_SIZE_MUL * stature
}

/// Length of a particle's streak behind it, yards (the bench's
/// `min(tail*0.12, speed*0.06) * size`), or `None` when it draws none.
pub fn siphon_streak_length(e: &SiphonEmitter, speed: f32, size: f32) -> Option<f32> {
    (e.tail > 0.0 && speed > STREAK_MIN_SPEED).then(|| (e.tail * 0.12).min(speed * 0.06) * size)
}

/// The palette entry a particle `t` through its life draws.
pub fn siphon_palette_step(t: f32) -> usize {
    let last = (SIPHON_PALETTE_STEPS - 1) as f32;
    ((t.clamp(0.0, 1.0) * last).round() as usize).min(SIPHON_PALETTE_STEPS - 1)
}

// ── Components ───────────────────────────────────────────────────────────────

/// One afflicted victim's emitter rig: a top-level entity parked at the
/// victim's chest, parent of every particle it emits (so the glow moves with
/// a running victim). Lives exactly as long as a Siphon Life DoT routed to
/// [`DotStateVisual::SiphonLifeGlow`] is on a living victim.
#[derive(Component)]
pub struct SiphonLifeRig {
    pub target: Entity,
    pub age: f32,
    /// Fractional particles owed, per emitter.
    pub carry: [f32; 3],
    pub emitted: u32,
    /// Whether the victim is a pet (its stature).
    pub is_pet: bool,
}

/// One particle: a child of its rig, at the point it is DRAWN; its children
/// are the sprite and, for a tailed layer, the streak.
#[derive(Component)]
pub struct SiphonLifeParticle {
    /// Index into [`SIPHON_LIFE_EMITTERS`].
    pub emitter: usize,
    /// Where the particle truly is, relative to the chest.
    pub offset: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub stature: f32,
    /// The sprite's roll in the camera plane (the bench's `ang = rnd()*TAU`;
    /// it shows on the four-point flare, the glows being round).
    pub angle: f32,
    pub sprite_palette: Arc<[Handle<StandardMaterial>]>,
    pub streak_palette: Arc<[Handle<StandardMaterial>]>,
}

/// The two quads a particle can draw.
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SiphonLifePart {
    Sprite,
    Streak,
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// The glow sprite's falloff: the bench's `glow` gradient stops, full at the
/// centre, 0.45 halfway out, nothing at the rim, linear between.
pub fn siphon_glow_alpha(r: f32) -> f32 {
    keyed(&[(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)], r)
}

/// Sprites and palettes, built once, lazily.
pub struct SiphonAssets {
    quad: Handle<Mesh>,
    glow: Handle<Image>,
    star: Handle<Image>,
    band: Handle<Image>,
    /// `(sprite palette, streak palette)` per emitter.
    palettes: [Option<(
        Arc<[Handle<StandardMaterial>]>,
        Arc<[Handle<StandardMaterial>]>,
    )>; 3],
}

impl SiphonAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            glow: images.add(disc_texture(SPRITE_PX, |r, _| siphon_glow_alpha(r))),
            star: images.add(star_flash_texture()),
            band: images.add(soft_band_texture()),
            palettes: [None, None, None],
        }
    }

    fn palettes(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        index: usize,
    ) -> (
        Arc<[Handle<StandardMaterial>]>,
        Arc<[Handle<StandardMaterial>]>,
    ) {
        if let Some(p) = &self.palettes[index] {
            return p.clone();
        }
        let e = &SIPHON_LIFE_EMITTERS[index];
        let texture = match e.sprite {
            SiphonSprite::Glow => self.glow.clone(),
            SiphonSprite::Star => self.star.clone(),
        };
        let build =
            |materials: &mut Assets<StandardMaterial>, texture: &Handle<Image>, gain: f32| {
                (0..SIPHON_PALETTE_STEPS)
                    .map(|i| {
                        let t = i as f32 / (SIPHON_PALETTE_STEPS - 1) as f32;
                        let color = keyed_color(e.color, t);
                        let alpha =
                            (keyed(e.alpha, t) * SIPHON_LIFE_OPACITY_MUL * gain).clamp(0.0, 1.0);
                        materials.add(particle_material(e.additive, color, alpha, texture))
                    })
                    .collect::<Arc<[_]>>()
            };
        let sprite = build(materials, &texture, 1.0);
        let streak = build(materials, &self.band.clone(), STREAK_ALPHA);
        self.palettes[index] = Some((sprite.clone(), streak.clone()));
        (sprite, streak)
    }
}

/// An additive layer is lit-emissive with `AlphaMode::Add` (`unlit` would
/// discard the emissive); an alpha layer is a flat, unlit colour with
/// `AlphaMode::Blend`, so the dark streaks DARKEN what is behind them — the
/// client's blend 2. Neither writes depth (both are transparent passes).
pub(super) fn particle_material(
    additive: bool,
    color: Color,
    alpha: f32,
    texture: &Handle<Image>,
) -> StandardMaterial {
    if additive {
        let c = color.to_linear();
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: Some(texture.clone()),
            emissive: LinearRgba::rgb(
                c.red * PARTICLE_GLOW,
                c.green * PARTICLE_GLOW,
                c.blue * PARTICLE_GLOW,
            ),
            emissive_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color: color.with_alpha(alpha),
            base_color_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Build the rig for a victim at `translation` whose Siphon Life has just
/// appeared. Called by `warlock_dots::spawn_warlock_dot_visuals`, the one
/// detector that lands kit 117, beside the apply burst it lands for the same
/// transition — so a refresh (the aura already present, the rig already
/// built) replays neither, and a corpse gets neither.
pub(super) fn spawn_siphon_life_rig(
    commands: &mut Commands,
    target: Entity,
    translation: Vec3,
    is_pet: bool,
) {
    commands.spawn((
        SiphonLifeRig {
            target,
            age: 0.0,
            carry: [0.0; 3],
            emitted: 0,
            is_pet,
        },
        DotStateOn::new(target, DotStateVisual::SiphonLifeGlow),
        Transform::from_translation(siphon_chest(translation, is_pet)),
        Visibility::default(),
        PlayMatchEntity,
    ));
}

/// Emit every kept layer from the victim's chest, integrating the rate over
/// the frame so a frame-rate change does not change the density. The rig
/// follows the victim in [`place_siphon_life_glows`].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_siphon_life_glows(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<SiphonAssets>>,
    mut rigs: Query<(Entity, &mut SiphonLifeRig)>,
    victims: Query<Has<Pet>, With<Combatant>>,
) {
    let dt = time.delta_secs();
    let assets = assets.get_or_insert_with(|| SiphonAssets::build(&mut meshes, &mut images));
    for (rig_entity, mut rig) in rigs.iter_mut() {
        let Ok(is_pet) = victims.get(rig.target) else {
            continue;
        };
        rig.age += dt;
        rig.is_pet = is_pet;
        let s = stature(is_pet);

        for (ei, e) in SIPHON_LIFE_EMITTERS.iter().enumerate() {
            if !SIPHON_LIFE_LAYERS[ei] {
                continue;
            }
            rig.carry[ei] += e.rate * SIPHON_LIFE_DENSITY_MUL * dt;
            if rig.carry[ei] < 1.0 {
                continue;
            }
            let (sprite_palette, streak_palette) = assets.palettes(&mut materials, ei);
            while rig.carry[ei] >= 1.0 {
                rig.carry[ei] -= 1.0;
                let seed = rig_entity
                    .index()
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let (offset, velocity) = siphon_particle_launch(e, s, seed);
                let mut parts = vec![commands
                    .spawn((
                        SiphonLifePart::Sprite,
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(sprite_palette[0].clone()),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id()];
                if e.tail > 0.0 {
                    parts.push(
                        commands
                            .spawn((
                                SiphonLifePart::Streak,
                                Mesh3d(assets.quad.clone()),
                                MeshMaterial3d(streak_palette[0].clone()),
                                Transform::from_scale(Vec3::splat(1e-4)),
                                NotShadowCaster,
                            ))
                            .id(),
                    );
                }
                let particle = commands
                    .spawn((
                        SiphonLifeParticle {
                            emitter: ei,
                            offset,
                            velocity,
                            age: 0.0,
                            life: e.life,
                            stature: s,
                            angle: draw(seed, 5) * std::f32::consts::TAU,
                            sprite_palette: sprite_palette.clone(),
                            streak_palette: streak_palette.clone(),
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

/// Age and move every particle along its straight line, and retire it at the
/// end of its life.
pub fn age_siphon_life_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut SiphonLifeParticle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let step = p.velocity * dt;
        p.offset += step;
    }
}

/// `BodyPlacement` (PostUpdate): stand each rig at its victim's rendered
/// chest, place every particle where it is drawn — held in front of the
/// rendered body for the camera (departure 1) — turn its sprite to the camera,
/// sized and coloured along its life, and lay its streak back along its
/// flight as the camera sees it.
#[allow(clippy::type_complexity)]
pub fn place_siphon_life_glows(
    hold: BodyHold,
    mut rigs: Query<
        (&SiphonLifeRig, &mut Transform, &Children),
        (
            Without<SiphonLifeParticle>,
            Without<SiphonLifePart>,
            HoldDisjoint,
        ),
    >,
    mut particles: Query<
        (&SiphonLifeParticle, &mut Transform, &Children),
        (
            Without<SiphonLifeRig>,
            Without<SiphonLifePart>,
            HoldDisjoint,
        ),
    >,
    mut parts: Query<
        (
            &SiphonLifePart,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<SiphonLifeParticle>,
            Without<SiphonLifeRig>,
            HoldDisjoint,
        ),
    >,
) {
    let cam = hold.view();
    let rotation = cam.map(|c| c.rotation).unwrap_or(Quat::IDENTITY);
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.target) else {
            continue;
        };
        let chest = siphon_chest(body.root, body.is_pet);
        rig_transform.translation = chest;
        let capsule = body.capsule();
        for child in children.iter() {
            let Ok((p, mut particle_transform, part_children)) = particles.get_mut(child) else {
                continue;
            };
            let e = SIPHON_LIFE_EMITTERS[p.emitter];
            let at = chest + p.offset;
            let (drawn, k) = held_in(cam, at, capsule);
            particle_transform.translation = drawn - chest;
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let step = siphon_palette_step(t);
            let radius = siphon_radius(&e, t, p.stature) * k;
            let speed = p.velocity.length();
            for part_entity in part_children.iter() {
                let Ok((part, mut transform, mut material)) = parts.get_mut(part_entity) else {
                    continue;
                };
                match part {
                    SiphonLifePart::Sprite => {
                        transform.rotation = rotation * Quat::from_rotation_z(p.angle);
                        transform.translation = Vec3::ZERO;
                        transform.scale = Vec3::splat((radius * 2.0).max(1e-4));
                        material.0 = p.sprite_palette[step].clone();
                    }
                    SiphonLifePart::Streak => {
                        let Some(length) =
                            siphon_streak_length(&e, speed, SIPHON_LIFE_SIZE_MUL * p.stature)
                        else {
                            transform.scale = Vec3::splat(1e-4);
                            continue;
                        };
                        let back = -p.velocity / speed;
                        // The bench's streak is a screen line: the
                        // projection of the segment behind the particle. So
                        // the quad lies in the camera's plane, at the
                        // particle's own depth (never deeper, so never
                        // behind the body), with the projected length.
                        // Without a camera it lies back along the flight.
                        let (along, length) = match cam {
                            Some(c) => {
                                let f = c.forward;
                                let flat = back - f * back.dot(f);
                                (flat.normalize_or_zero(), length * flat.length() * k)
                            }
                            None => (back, length),
                        };
                        // Roll the camera-facing quad until its long (local
                        // X) axis lies back along the flight as the camera
                        // sees it.
                        let (right, up) = (rotation * Vec3::X, rotation * Vec3::Y);
                        let roll = back.dot(up).atan2(back.dot(right));
                        transform.rotation = rotation * Quat::from_rotation_z(roll);
                        transform.translation = along * (length * 0.5);
                        transform.scale =
                            Vec3::new(length.max(1e-4), (radius * STREAK_WIDTH).max(1e-4), 1.0);
                        material.0 = p.streak_palette[step].clone();
                    }
                }
            }
        }
    }
}

/// End the glow the frame the Siphon Life DoT is gone (expiry and dispel are
/// one path — the detector keys on presence) or the victim dies or despawns.
/// Despawning the rig takes its particles with it; nothing lingers.
pub fn cleanup_siphon_life_glows(
    mut commands: Commands,
    rigs: Query<(Entity, &SiphonLifeRig)>,
    targets: Query<(&Combatant, Option<&ActiveAuras>)>,
) {
    for (entity, rig) in rigs.iter() {
        let lives = targets
            .get(rig.target)
            .map(|(c, a)| c.is_alive() && has_dot_state(a, DotStateVisual::SiphonLifeGlow))
            .unwrap_or(false);
        if !lives {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_draws_each_track_key_exactly() {
        assert_eq!(siphon_palette_step(0.0), 0);
        assert_eq!(siphon_palette_step(0.5), (SIPHON_PALETTE_STEPS - 1) / 2);
        assert_eq!(siphon_palette_step(1.0), SIPHON_PALETTE_STEPS - 1);
    }
}
