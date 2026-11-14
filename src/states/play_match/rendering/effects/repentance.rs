use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::TAU;

use super::druid_heals::{body_point, keyed_color, BODY_CENTRE_ABOVE_FLOOR};
use super::heal_impact::COMBATANT_BODY_RADIUS;
use super::hunter_shots::{draw, particle_rotation, ClientEmitter, EmitterAssets, EmitterSprite};
use super::paladin_oneshots::{
    at_knobs, emitter, mid_of, oneshot_launch, OneShotKnobs, OneShotLaunch, OneShotLayer,
    CLIENT_KNOBS, ONESHOT_CHEST,
};
use super::school_impact::IMPACT_PET_STATURE;
use super::spell_bolts::star_flash_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// Repentance — the gold impact, then the violet head swirl (AS-378)
// ==============================================================================
//
// From the client data joined in
// `docs/design/2026-10-18-paladin-talent-client-data.md` §6 (visual 5539, the
// same chain at Era, TBC and Wrath):
//
// - **Cast** (kit 190): `holy_precast_high_hand.m2` on both hands — the
//   instant Holy hand flash AS-374 shipped (`heal_cast.rs`
//   `flash_instant_hands`), on the Paladin.
// - **Impact on the victim** (kit 6499): `holy_impactdd_low_chest.m2` at the
//   chest, 0.93 s — yellow-to-white `starflashyellowa` stars (75/s, 67-400
//   ms) and gold `ribbonblur1bea_gold` streaks (peaking 250/s at 134 ms). It
//   is a Paladin talent one-shot (`paladin_oneshots.rs`,
//   `PaladinOneShotKind::RepentanceImpact`); its tables live here.
// - **State for the duration** (kit 4949): `shadowwordbefuddle_head.m2` at the
//   head — two `sparkle` emitters, 111/s each over a 1.5 yd box round the
//   head, falling 3.3 yd/s, 0.5 s life (sky blue -> violet -> white, and
//   indigo -> lavender -> white), and three violet `star8b` points circling
//   at about 0.7-1.2 yd (the circling is inferred: the bones animate and are
//   not parsed). No body pose: we draw none for crowd control.
//
// Played as the signed-off AS-337 Paladin Talent Visuals Bench plays it
// (https://claude.ai/artifact/31rnhhKpACF51iUsqK5g7k, version 1792379243-e3d6,
// the "Repentance" tab; committed as
// `docs/design/benches/2026-10-18-paladin-talent-bench.html`): every layer
// kept at the client's values, every knob 1.0, and the comparison Stun whirl
// (a bench-only reference layer) not built. The tables below are its
// emitters, transcribed; the circling points are its `customDraw`, ported. A
// bench `scale` key is a sprite RADIUS; a hand-drawn sprite's size is its
// full width.
//
// Two departures from the bench, each for the match it plays in:
//
// - **The impact stands in front of the body.** The bench draws without
//   depth; here a camera-facing quad at the chest is inside the 0.5 yd body
//   capsule, and every one of the impact's particles stays within 0.47 yd of
//   the chest, so the body would hide almost all of it. Its particles are
//   born [`REPENTANCE_IMPACT_CAMERA_LIFT`] toward the camera along the line
//   of sight — which leaves every particle where the bench puts it on screen
//   and only brings it in front of the body. (`moonfire.rs` and
//   `warlock_dots.rs`'s UA glow solve the same problem the same way.)
// - **The cast, the impact and the swirl start together, on the landing.**
//   The bench staggers them (the hands at 100 ms, the impact at 400, the
//   state at 500) for a reader to follow; in the match Repentance is an
//   instant whose incapacitate holds from the frame it lands, and the client
//   fires the cast, impact and state kits together for an instant — as every
//   AS-374 one-shot plays its hands and its landing.
//
// **Lifetimes.** The impact is a landing: it plays its 0.93 s on the victim
// whatever the incapacitate does. The swirl shows the incapacitate HOLDING:
// it is spawned while a living unit holds Repentance's incapacitate (the
// aura `AuraApplyRoute::for_instance` routes to `AuraApplyOwner::Repentance`)
// and retired — sparkles, stars and all — the frame it no longer does: it
// expired, damage broke it, it was dispelled, or its victim died (death
// keeps the aura on the corpse, so it is checked apart). Its stars fade in
// over the bench's 0.2 s and out over the incapacitate's last 0.2 s.
//
// Graphical-only: scatter is the shared deterministic hash (never
// `game_rng`), nothing here writes sim state, and the swirl's system is
// registered in `states/mod.rs` only. The impact rides the sim's
// `AuraLanding` marker, a purely cosmetic entity `apply_pending_auras`
// spawns in both modes.

// ── The bench's layers and knobs (signed off at version 1792379243-e3d6) ────

// Repentance (visual 5539): kit 6499 holy_impactdd_low_chest.m2 @Chest on
// landing; kit 4949 shadowwordbefuddle_head.m2 @Head for the duration. AS-337 bench.
// Route: AuraApplyRoute::for_instance(Incapacitate, "Repentance") -> its own
// owner (Incapacitate alone routes to IceBlock).
pub const REPENTANCE_HANDS: bool = true;
pub const REPENTANCE_IMPACT_STARS: bool = true;
pub const REPENTANCE_IMPACT_STREAKS: bool = true;
pub const REPENTANCE_SPARKLES_SKY_VIOLET: bool = true;
pub const REPENTANCE_SPARKLES_INDIGO_LAVENDER: bool = true;
pub const REPENTANCE_CIRCLING_POINTS: bool = true;
/// The bench's size, spread, density and opacity knobs, all at the client's
/// 1.0 (the bench's duration knob stretches its clock; ours is the aura's).
pub const REPENTANCE_KNOBS: OneShotKnobs = CLIENT_KNOBS;

// ── The impact: holy_impactdd_low_chest.m2 ───────────────────────────────────

/// The impact's sequence, seconds (the bench's 934 ms span).
pub const REPENTANCE_IMPACT_SECS: f32 = 0.934;

/// The gold streaks' rate track: nothing until 67 ms, 75/s at 100, the peak
/// 250/s at 134, and down to nothing by 400 ms.
const REPENTANCE_STREAK_TRACK: &[(f32, f32)] = &[
    (0.0, 0.0),
    (0.067, 0.0),
    (0.1, 75.0),
    (0.134, 250.0),
    (0.167, 100.0),
    (0.234, 25.0),
    (0.3, 10.0),
    (0.4, 0.0),
];

pub(super) static REPENTANCE_IMPACT_LAYERS: [OneShotLayer; 2] = [
    // `starflashyellowa`: yellow -> white stars, 75/s from 67 to 400 ms, on a
    // 0.3 yd box at the chest.
    at_knobs(
        OneShotLayer {
            on: REPENTANCE_IMPACT_STARS,
            emitter: emitter(
                "holy_impactdd_low_chest/yellow stars",
                EmitterSprite::Flare,
                75.0,
                Some((0.067, 0.4)),
                None,
                0.5,
                -0.42,
                Some((0.3, 0.3)),
                [[255, 255, 0], [255, 255, 255], [254, 255, 180]],
                [0.29, 1.0, 0.0],
                [0.35, mid_of(0.35, 0.2, 0.5), 0.2],
                0.5,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        REPENTANCE_KNOBS,
    ),
    // `ribbonblur1bea_gold`: white-gold streaks, keyed at a quarter of their
    // life (the alpha's 0.78 there; the colour and size ramps run straight
    // through it).
    at_knobs(
        OneShotLayer {
            on: REPENTANCE_IMPACT_STREAKS,
            emitter: emitter(
                "holy_impactdd_low_chest/gold streaks",
                EmitterSprite::Streak,
                0.0,
                ClientEmitter::tracked(REPENTANCE_STREAK_TRACK),
                Some(REPENTANCE_STREAK_TRACK),
                0.75,
                0.33,
                Some((0.2, 0.0)),
                [[255, 240, 190], [255, 235, 173], [255, 220, 120]],
                [0.84, 0.78, 0.0],
                [0.14, mid_of(0.14, 0.06, 0.25), 0.06],
                0.25,
            ),
            height: ONESHOT_CHEST,
            launch: OneShotLaunch::Sphere,
            drag: 0.0,
        },
        REPENTANCE_KNOBS,
    ),
];

/// The farthest the body capsule (radius [`COMBATANT_BODY_RADIUS`], its top
/// hemisphere centred 1.75 yd up) reaches from the chest toward any camera
/// above the chest: the top hemisphere's centre is 0.3 yd above the chest,
/// so `0.3 × sin(elevation) + 0.5`, at most 0.8 straight down.
pub const REPENTANCE_IMPACT_BODY_REACH: f32 =
    (BODY_CENTRE_ABOVE_FLOOR + 0.75 - ONESHOT_CHEST) + COMBATANT_BODY_RADIUS;

/// How far toward the camera the impact's particles are born: past the
/// body's reach ([`REPENTANCE_IMPACT_BODY_REACH`], 0.8) by the farthest a
/// particle strays from its birth point along the line of sight — the spawn
/// box's half-diagonal plus its whole flight (the stars' 0.26 + 0.21) — so
/// no particle of the impact passes behind the body at any camera pitch.
pub const REPENTANCE_IMPACT_CAMERA_LIFT: f32 = 1.3;

/// The farthest one impact layer's particle strays from its birth point:
/// the spawn box's half-diagonal plus its flight over its life.
pub fn impact_excursion(layer: &OneShotLayer) -> f32 {
    let e = &layer.emitter;
    let (w, h) = e.column.unwrap_or((0.0, 0.0));
    let half_diagonal = Vec3::new(w, h, w).length() * 0.5;
    half_diagonal + e.speed.abs() * e.life
}

// ── The swirl: shadowwordbefuddle_head.m2 ────────────────────────────────────

/// The bench's head anchor above the floor (`ANCH.head`): the rig stands
/// here.
pub const REPENTANCE_HEAD: f32 = 2.25;
/// The sparkles are born this far above the head (the bench's `pos`).
pub const SPARKLE_LIFT: f32 = 0.6;

/// One falling sparkle emitter: 111/s on a 1.53 yd box, 0.2 yd tall, falling
/// 3.33 yd/s for its 0.5 s life; colour keyed at birth, half-life and death,
/// alpha in from nothing to full at half-life and out again.
const fn sparkle_layer(on: bool, name: &'static str, color: [[u8; 3]; 3]) -> OneShotLayer {
    OneShotLayer {
        on,
        emitter: emitter(
            name,
            EmitterSprite::Flare,
            111.0,
            None,
            None,
            0.5,
            -3.33,
            Some((1.53, 0.2)),
            color,
            [0.0, 1.0, 0.0],
            [0.08, mid_of(0.08, 0.04, 0.5), 0.04],
            0.5,
        ),
        height: REPENTANCE_HEAD + SPARKLE_LIFT,
        launch: OneShotLaunch::Vertical { speed_var: 0.0 },
        drag: 0.0,
    }
}

pub(super) static REPENTANCE_SPARKLE_LAYERS: [OneShotLayer; 2] = [
    at_knobs(
        sparkle_layer(
            REPENTANCE_SPARKLES_SKY_VIOLET,
            "shadowwordbefuddle_head/sky-violet sparkles",
            [[104, 182, 242], [184, 0, 245], [255, 255, 255]],
        ),
        REPENTANCE_KNOBS,
    ),
    at_knobs(
        sparkle_layer(
            REPENTANCE_SPARKLES_INDIGO_LAVENDER,
            "shadowwordbefuddle_head/indigo-lavender sparkles",
            [[102, 35, 225], [162, 77, 241], [255, 255, 255]],
        ),
        REPENTANCE_KNOBS,
    ),
];

/// The swirl's sparkle emitters, each for as long as the incapacitate holds.
pub fn sparkle_layers() -> &'static [OneShotLayer] {
    &REPENTANCE_SPARKLE_LAYERS
}

/// Sparkles one layer owes between `t0` and `t1` seconds into the swirl: its
/// rate, for as long as the incapacitate holds (the state has no window of
/// its own).
pub fn sparkles_between(layer: &OneShotLayer, t0: f32, t1: f32) -> f32 {
    if !layer.on || t1 <= t0 {
        return 0.0;
    }
    layer.emitter.rate * (t1 - t0) * REPENTANCE_KNOBS.density
}

/// The three circling points, each trailing three fainter stars.
pub const STAR_POINTS: u8 = 3;
pub const STAR_TRAIL: u8 = 4;
/// Each point's orbit radius, yards, and its height over the head's: the
/// bench's `0.35 + [0.45, 0, -0.45] × 0.4`.
pub const STAR_ORBIT_RADII: [f32; 3] = [0.7, 1.2, 1.05];
pub const STAR_HEIGHTS: [f32; 3] = [0.35 + 0.45 * 0.4, 0.35, 0.35 - 0.45 * 0.4];
/// Turns a second, the three a third of a turn apart.
pub const STAR_REV_PER_SEC: f32 = 0.6;
/// Radians each trailing star sits behind the one before it.
pub const STAR_TRAIL_STEP: f32 = 0.12;
/// The lead star's width, yards; each trailing one is a fifth smaller.
pub const STAR_SIZE: f32 = 0.32;
/// The lead star's alpha; each trailing one is 0.13 fainter.
pub const STAR_ALPHA: f32 = 0.6;
pub const STAR_ALPHA_STEP: f32 = 0.13;
/// The trail's colour, keyed over its place in it (`trail / 4`): violet,
/// orchid, then white.
pub const STAR_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [180, 0, 247]),
    (0.2, [185, 91, 228]),
    (1.0, [255, 255, 255]),
];
/// The stars fade in over this, and out over the incapacitate's last this.
pub const SWIRL_FADE_SECS: f32 = 0.2;

/// The stars' weight `age` seconds into a swirl whose incapacitate has
/// `remaining` seconds left: in over [`SWIRL_FADE_SECS`], held, and out over
/// the incapacitate's last [`SWIRL_FADE_SECS`] — the bench's envelope for its
/// six seconds, whatever the incapacitate lasts.
pub fn swirl_envelope(age: f32, remaining: f32) -> f32 {
    (age / SWIRL_FADE_SECS)
        .min(remaining / SWIRL_FADE_SECS)
        .clamp(0.0, 1.0)
}

/// One circling star at a moment: its place relative to the head (the rig),
/// its width, its colour, its alpha (before the envelope) and its roll in the
/// view plane, radians.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StarPose {
    pub at: Vec3,
    pub size: f32,
    pub rgb: [u8; 3],
    pub alpha: f32,
    pub roll: f32,
}

/// Star `trail` of point `point`, `age` seconds into the swirl, at the
/// client's values. Pure: the probes and the update system read one
/// definition.
pub fn star_pose(point: u8, trail: u8, age: f32) -> StarPose {
    let k = point as usize;
    let j = trail as f32;
    let lead = k as f32 / STAR_POINTS as f32 * TAU + age * STAR_REV_PER_SEC * TAU;
    let angle = lead - j * STAR_TRAIL_STEP;
    let r = STAR_ORBIT_RADII[k];
    let rgb = keyed_color(&STAR_RGB, j / STAR_TRAIL as f32).to_srgba();
    StarPose {
        at: Vec3::new(r * angle.cos(), STAR_HEIGHTS[k], r * angle.sin()),
        size: STAR_SIZE * (1.0 - j * 0.2),
        rgb: [rgb.red, rgb.green, rgb.blue].map(|c| (c * 255.0).round() as u8),
        alpha: STAR_ALPHA - j * STAR_ALPHA_STEP,
        roll: angle,
    }
}

/// The stars a swirl draws, by its toggle.
pub fn swirl_stars() -> Vec<RepentanceStar> {
    if !REPENTANCE_CIRCLING_POINTS {
        return Vec::new();
    }
    (0..STAR_POINTS)
        .flat_map(|point| (0..STAR_TRAIL).map(move |trail| RepentanceStar { point, trail }))
        .collect()
}

/// The rotation a star takes facing the camera at `camera`, rolled as the
/// bench rolls its sprite (`ctx.rotate(angle)`, clockwise on the screen).
pub fn star_rotation(roll: f32, camera: Quat) -> Quat {
    camera * Quat::from_rotation_z(-roll)
}

/// How long Repentance's incapacitate on a unit with these auras has left,
/// or `None` when it holds none: the incapacitate whose application the
/// Repentance owner owns.
pub fn repentance_remaining(auras: Option<&ActiveAuras>) -> Option<f32> {
    auras?
        .auras
        .iter()
        .filter(|a| {
            AuraApplyRoute::for_instance(a.effect_type, &a.ability_name)
                == AuraApplyRoute::Owned(AuraApplyOwner::Repentance)
        })
        .map(|a| a.duration)
        .reduce(f32::max)
}

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

// ── System ───────────────────────────────────────────────────────────────────

/// The swirl (graphical-only): retire every swirl whose victim is gone, dead
/// or no longer holds Repentance's incapacitate, sparkles and stars with it;
/// for the rest follow the head, drop spent sparkles, emit what the two
/// emitters owe, and turn and pose the stars; then raise a swirl over every
/// living unit that holds the incapacitate and has none. A swirl despawned
/// out from under a held incapacitate is raised again the next frame. Runs
/// before `animate_client_particles`, which ages the sparkles.
#[allow(clippy::too_many_arguments)]
pub fn update_repentance_swirls(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut emitter_assets: Local<Option<EmitterAssets>>,
    mut star_texture: Local<Option<Handle<Image>>>,
    mut swirls: Query<(
        Entity,
        &mut RepentanceSwirl,
        &mut Transform,
        Option<&Children>,
    )>,
    mut stars: Query<
        (
            &RepentanceStar,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<RepentanceSwirl>, Without<ClientParticle>),
    >,
    mut sparkles: Query<
        (&ClientParticle, &mut Transform),
        (Without<RepentanceSwirl>, Without<RepentanceStar>),
    >,
    units: Query<
        (
            Entity,
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            Option<&Pet>,
        ),
        (
            Without<RepentanceSwirl>,
            Without<RepentanceStar>,
            Without<ClientParticle>,
        ),
    >,
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<RepentanceSwirl>,
            Without<RepentanceStar>,
            Without<ClientParticle>,
            Without<Combatant>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next().map(|c| c.rotation);
    let held = |owner: Entity| {
        units
            .get(owner)
            .ok()
            .filter(|(_, unit, ..)| unit.is_alive())
            .and_then(|(_, _, at, auras, _)| repentance_remaining(auras).map(|left| (at, left)))
    };

    let mut swirled: Vec<Entity> = Vec::new();
    for (entity, mut swirl, mut transform, children) in swirls.iter_mut() {
        let Some((at, remaining)) = held(swirl.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        swirled.push(swirl.owner);
        let (t0, t1) = (swirl.age, swirl.age + dt);
        swirl.age = t1;
        let s = stature(swirl.is_pet);
        transform.translation = body_point(REPENTANCE_HEAD, at.translation, swirl.is_pet);

        // Spent sparkles go; the living ones and the stars turn to the camera.
        let env = swirl_envelope(t1, remaining);
        for child in children.into_iter().flat_map(|c| c.iter()) {
            if let Ok((particle, mut part)) = sparkles.get_mut(child) {
                if particle.age >= particle.life {
                    commands.entity(child).despawn();
                } else if let Some(cam) = cam {
                    part.rotation = particle_rotation(particle, Quat::IDENTITY, cam);
                }
                continue;
            }
            let Ok((star, mut part, material)) = stars.get_mut(child) else {
                continue;
            };
            let pose = star_pose(star.point, star.trail, t1);
            let k = REPENTANCE_KNOBS;
            part.translation = Vec3::new(pose.at.x * k.size, pose.at.y, pose.at.z * k.size) * s;
            part.scale = Vec3::splat((pose.size * k.size * s).max(1e-3));
            if let Some(cam) = cam {
                part.rotation = star_rotation(pose.roll, cam);
            }
            if let Some(material) = materials.get_mut(&material.0) {
                material.base_color = Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2])
                    .with_alpha((pose.alpha * env * k.opacity).clamp(0.0, 1.0));
            }
        }

        // The two sparkle emitters, born over the head and falling.
        let assets =
            emitter_assets.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        for (i, layer) in sparkle_layers().iter().enumerate() {
            swirl.carry[i] += sparkles_between(layer, t0, t1);
            if swirl.carry[i] < 1.0 {
                continue;
            }
            let e = &layer.emitter;
            let palette = assets.palette(&mut materials, e);
            let quad = assets.quad();
            let born_at = Vec3::Y * (layer.height - REPENTANCE_HEAD);
            while swirl.carry[i] >= 1.0 {
                swirl.carry[i] -= 1.0;
                let seed = entity
                    .index()
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(swirl.emitted.wrapping_mul(0x85EB_CA6B));
                swirl.emitted = swirl.emitted.wrapping_add(1);
                let (offset, velocity) = oneshot_launch(layer, t1, seed);
                let particle = ClientParticle {
                    age: 0.0,
                    life: e.life,
                    velocity: velocity * s,
                    gravity: 0.0,
                    size: e.size.scaled(s),
                    mid: e.mid,
                    palette: palette.clone(),
                    step: 0,
                    // A star lies at the random roll the bench gives it.
                    facing: ParticleFacing::Spin {
                        mrad_per_sec: 0,
                        start_mrad: (draw(seed, 8) * TAU * 1000.0) as i16,
                    },
                    owned: true,
                };
                let rotation = cam.map_or(Quat::IDENTITY, |cam| {
                    particle_rotation(&particle, Quat::IDENTITY, cam)
                });
                let sparkle = commands
                    .spawn((
                        Mesh3d(quad.clone()),
                        MeshMaterial3d(palette[0].clone()),
                        Transform::from_translation((born_at + offset) * s).with_rotation(rotation),
                        NotShadowCaster,
                        particle,
                    ))
                    .id();
                commands.entity(entity).add_child(sparkle);
            }
        }
    }

    // Raise a swirl over every living unit that holds the incapacitate and
    // has none.
    for (owner, unit, at, auras, pet) in units.iter() {
        if !unit.is_alive() || swirled.contains(&owner) || repentance_remaining(auras).is_none() {
            continue;
        }
        let is_pet = pet.is_some();
        let texture = star_texture
            .get_or_insert_with(|| images.add(star_flash_texture()))
            .clone();
        let quad = emitter_assets
            .get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images))
            .quad();
        let swirl = commands
            .spawn((
                RepentanceSwirl {
                    owner,
                    is_pet,
                    age: 0.0,
                    carry: [0.0; 2],
                    emitted: 0,
                },
                Transform::from_translation(body_point(REPENTANCE_HEAD, at.translation, is_pet)),
                Visibility::default(),
                PlayMatchEntity,
            ))
            .id();
        for star in swirl_stars() {
            let pose = star_pose(star.point, star.trail, 0.0);
            let material = materials.add(StandardMaterial {
                base_color: Color::srgb_u8(pose.rgb[0], pose.rgb[1], pose.rgb[2]).with_alpha(0.0),
                base_color_texture: Some(texture.clone()),
                // The bench adds colour × sprite × alpha and nothing else.
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            let piece = commands
                .spawn((
                    star,
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(1e-3)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(swirl).add_child(piece);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bench's emitters, transcribed field by field.
    #[test]
    fn the_impact_is_the_benchs_holy_impactdd() {
        let [stars, streaks] = &REPENTANCE_IMPACT_LAYERS;
        let e = &stars.emitter;
        assert!(stars.on && e.additive);
        assert_eq!(e.sprite, EmitterSprite::Flare, "kind star");
        assert_eq!(
            (e.rate, e.window, e.track),
            (75.0, Some((0.067, 0.4)), None)
        );
        assert_eq!((e.life, e.speed), (0.5, -0.42));
        assert_eq!(e.column, Some((0.3, 0.3)));
        assert_eq!(e.color, [[255, 255, 0], [255, 255, 255], [254, 255, 180]]);
        assert_eq!((e.alpha, e.mid), ([0.29, 1.0, 0.0], 0.5));
        let d = e.size.diameter_keys();
        assert!(
            (d[0] - 0.7).abs() < 1e-6 && (d[2] - 0.4).abs() < 1e-6,
            "{d:?}"
        );
        assert!((d[1] - 0.55).abs() < 1e-6, "{d:?}");
        assert_eq!(stars.launch, OneShotLaunch::Sphere);
        assert_eq!((stars.height, stars.drag), (1.45, 0.0));

        let e = &streaks.emitter;
        assert!(streaks.on && e.additive);
        assert_eq!(e.sprite, EmitterSprite::Streak, "a glow with a tail");
        assert_eq!(e.window, Some((0.0, 0.4)));
        assert_eq!(e.track, Some(REPENTANCE_STREAK_TRACK));
        assert_eq!((e.life, e.speed), (0.75, 0.33));
        assert_eq!(e.column, Some((0.2, 0.0)));
        // Keyed at a quarter: the bench's straight colour ramp passes
        // (255, 235, 172.5) there.
        assert_eq!(e.color, [[255, 240, 190], [255, 235, 173], [255, 220, 120]]);
        assert_eq!((e.alpha, e.mid), ([0.84, 0.78, 0.0], 0.25));
        let d = e.size.diameter_keys();
        assert!((d[0] - 0.28).abs() < 1e-6 && (d[2] - 0.12).abs() < 1e-6);
        assert!((d[1] - 0.24).abs() < 1e-6, "{d:?}");
        assert_eq!(streaks.launch, OneShotLaunch::Sphere);
        assert_eq!(streaks.height, 1.45);
    }

    /// The streaks peak at 250/s at 134 ms; the stars emit 75/s for 333 ms.
    #[test]
    fn the_impact_emits_the_benchs_counts() {
        let [stars, streaks] = &REPENTANCE_IMPACT_LAYERS;
        let n = stars.count_between(0.0, 1.0, CLIENT_KNOBS);
        assert!((n - 75.0 * 0.333).abs() < 1e-3, "{n} stars");
        let peak = streaks.count_between(0.133, 0.135, CLIENT_KNOBS) / 0.002;
        assert!((peak - 250.0).abs() < 5.0, "{peak}/s at the peak");
        assert_eq!(streaks.count_between(0.0, 0.067, CLIENT_KNOBS), 0.0);
        let total = streaks.count_between(0.0, 1.0, CLIENT_KNOBS);
        // The track's area: 0.5 × Σ (r0 + r1) × dt over its keys.
        let want: f32 = REPENTANCE_STREAK_TRACK
            .windows(2)
            .map(|w| 0.5 * (w[0].1 + w[1].1) * (w[1].0 - w[0].0))
            .sum();
        assert!((total - want).abs() < 1e-3, "{total} vs {want}");
        // About eighteen streaks in all: a 0.33 s puff, densest at 134 ms.
        assert!((total - 18.38).abs() < 0.01, "{total} streaks");
    }

    /// The lift clears the body's reach by every layer's straying, at
    /// every camera elevation (the reach is largest straight down).
    #[test]
    fn the_impact_lift_clears_the_body_at_every_pitch() {
        assert!((REPENTANCE_IMPACT_BODY_REACH - 0.8).abs() < 1e-6);
        let stray = REPENTANCE_IMPACT_LAYERS
            .iter()
            .map(impact_excursion)
            .fold(0.0_f32, f32::max);
        assert!(
            (stray - (0.15 * 3f32.sqrt() + 0.21)).abs() < 1e-5,
            "{stray}"
        );
        assert!(REPENTANCE_IMPACT_CAMERA_LIFT > REPENTANCE_IMPACT_BODY_REACH + stray);
        assert!(
            REPENTANCE_IMPACT_CAMERA_LIFT < REPENTANCE_IMPACT_BODY_REACH + stray + 0.1,
            "no farther than it needs"
        );
    }

    #[test]
    fn the_sparkles_are_the_benchs_two_emitters() {
        let want = [
            [[104, 182, 242], [184, 0, 245], [255, 255, 255]],
            [[102, 35, 225], [162, 77, 241], [255, 255, 255]],
        ];
        assert_eq!(sparkle_layers().len(), 2);
        for (layer, color) in sparkle_layers().iter().zip(want) {
            let e = &layer.emitter;
            assert!(layer.on && e.additive);
            assert_eq!(e.sprite, EmitterSprite::Flare, "kind star");
            assert_eq!((e.rate, e.life, e.speed), (111.0, 0.5, -3.33));
            assert_eq!(e.column, Some((1.53, 0.2)));
            assert_eq!(e.color, color);
            assert_eq!((e.alpha, e.mid), ([0.0, 1.0, 0.0], 0.5));
            let d = e.size.diameter_keys();
            assert!((d[0] - 0.16).abs() < 1e-6 && (d[1] - 0.12).abs() < 1e-6);
            assert!((d[2] - 0.08).abs() < 1e-6);
            assert!((layer.height - 2.85).abs() < 1e-6, "0.6 above the head");
            assert_eq!(layer.launch, OneShotLaunch::Vertical { speed_var: 0.0 });
            // Straight down at 3.33 yd/s, born on the box.
            for seed in 0..100 {
                let (at, v) = oneshot_launch(layer, 1.0, seed);
                assert_eq!(v, Vec3::new(0.0, -3.33, 0.0));
                assert!(at.x.abs() <= 0.765 && at.z.abs() <= 0.765 && at.y.abs() <= 0.1);
            }
            // 111 a second for as long as it holds.
            assert!((sparkles_between(layer, 2.0, 3.0) - 111.0).abs() < 1e-3);
            assert_eq!(sparkles_between(layer, 3.0, 3.0), 0.0);
        }
        let off = OneShotLayer {
            on: false,
            ..sparkle_layers()[0]
        };
        assert_eq!(sparkles_between(&off, 0.0, 1.0), 0.0);
    }

    /// Three points circling 0.6 turns a second, a third of a turn apart, at
    /// 0.7, 1.2 and 1.05 yd, each with three fainter, smaller stars behind.
    #[test]
    fn the_points_circle_as_the_bench_draws_them() {
        for point in 0..3u8 {
            let k = point as usize;
            for age in [0.0, 0.4, 1.7] {
                let lead = star_pose(point, 0, age);
                assert!((lead.at.xz().length() - STAR_ORBIT_RADII[k]).abs() < 1e-5);
                let want = k as f32 * TAU / 3.0 + age * 0.6 * TAU;
                let got = lead.at.z.atan2(lead.at.x);
                assert!(((got - want).rem_euclid(TAU)).min((want - got).rem_euclid(TAU)) < 1e-4);
                assert!((lead.roll - want).abs() < 1e-5);
                for trail in 1..4u8 {
                    let p = star_pose(point, trail, age);
                    assert!((p.roll - (want - trail as f32 * 0.12)).abs() < 1e-5);
                    assert_eq!(p.at.y, lead.at.y);
                }
            }
        }
        // Heights 0.53, 0.35 and 0.17 above the head anchor.
        assert!((STAR_HEIGHTS[0] - 0.53).abs() < 1e-6);
        assert!((STAR_HEIGHTS[2] - 0.17).abs() < 1e-6);
        // The trail: 0.32, 0.256, 0.192, 0.128 wide; 0.6, 0.47, 0.34, 0.21
        // alpha; violet into white (the bench's `|0` truncation of the ramp
        // is within one of these rounded values).
        let trail: Vec<StarPose> = (0..4).map(|j| star_pose(1, j, 0.5)).collect();
        // The bench's `keys([[0, violet], [0.2, orchid], [1, white]], j / 4)`.
        let bench_rgb = |t: f64| -> [f64; 3] {
            let (a, b, u) = if t <= 0.2 {
                ([180.0, 0.0, 247.0], [185.0, 91.0, 228.0], t / 0.2)
            } else {
                ([185.0, 91.0, 228.0], [255.0, 255.0, 255.0], (t - 0.2) / 0.8)
            };
            [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * u)
        };
        for (j, (size, alpha)) in [(0.32, 0.6), (0.256, 0.47), (0.192, 0.34), (0.128, 0.21)]
            .into_iter()
            .enumerate()
        {
            assert!((trail[j].size - size).abs() < 1e-5, "{j} size");
            assert!((trail[j].alpha - alpha).abs() < 1e-5, "{j} alpha");
            let want = bench_rgb(j as f64 / 4.0);
            for c in 0..3 {
                assert!(
                    (trail[j].rgb[c] as f64 - want[c]).abs() <= 0.5 + 1e-3,
                    "{j} colour {:?} vs {want:?}",
                    trail[j].rgb
                );
            }
        }
        assert_eq!(
            trail[0].rgb,
            [180, 0, 247],
            "the lead is the client's violet"
        );
        assert_eq!(swirl_stars().len(), 12);
    }

    #[test]
    fn the_stars_fade_in_and_out_with_the_incapacitate() {
        assert_eq!(swirl_envelope(0.0, 6.0), 0.0);
        assert!((swirl_envelope(0.1, 5.9) - 0.5).abs() < 1e-5);
        assert_eq!(swirl_envelope(0.2, 5.8), 1.0);
        assert_eq!(swirl_envelope(3.0, 3.0), 1.0);
        assert!((swirl_envelope(5.9, 0.1) - 0.5).abs() < 1e-5);
        assert_eq!(swirl_envelope(6.0, 0.0), 0.0);
        assert_eq!(swirl_envelope(1.0, -0.1), 0.0);
    }
}
