use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::druid_heals::{body_point, keyed, keyed_color, window_on};
use super::hit_reaction::{insert_flinch, PendingDepth};
use super::hunter_shots::draw;
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use super::siphon_life::particle_material;
use crate::states::play_match::components::*;

// ==============================================================================
// Aftermath — the client's small fire hit at the victim's chest
// ==============================================================================
//
// From the client data, joined in
// `docs/design/2026-10-17-warlock-talent-client-data.md` §7 (AS-334):
// Aftermath's daze (18118 -> visual 4826) lands kit 728,
// `fire_impactdd_chest.m2`, on the victim's chest (attachment 34) with the
// victim's CombatWound flinch (anim 9), then holds the client's fire-slow state
// (kit 985: a heat haze at the feet and a red body tint, `#FF5155`) for the
// slow's life.
//
// Ruled (AS-334, 2026-10-17): our slow ring with a new Fire tint, plus the
// client's one-shot fire hit; no heat haze and no body tint. So:
//
// - **The landing** is this file: the fire hit, played off the sim's
//   `AuraLanding` marker (`apply_pending_auras`, on every application, so a
//   proc that lands again while the slow is up plays a second hit), and the
//   shipped wound flinch (`hit_reaction.rs`'s `insert_flinch`, the one every
//   landed melee auto plays).
// - **The state** is the shipped slow ring at the feet while the victim moves
//   (`slow_ring.rs`), routed by `SlowTint::for_slow` to `SlowTint::Fire`, whose
//   colour is the Fire school's (`SpellSchool::Fire`, (255,128,64)) — the
//   school palette every other tint takes. The bench's (255,110,40) was a
//   stand-in for that arm.
//
// Played as the signed-off AS-334 Warlock Talent Visuals Bench draws it
// (https://claude.ai/artifact/KwBsy13gPub9sKmnKdYMgg, version 1792203924-603a,
// "Aftermath daze" tab, at the ruling: its two fire-hit layers and the ring
// on, every multiplier 1.0; source committed as
// `docs/design/benches/2026-10-17-warlock-talent-bench.html`). The bench draws
// the client's four emitters as TWO layers, both following the victim
// (`follow: true`), both emitting at chest height from 67 ms to 700 ms:
//
//   hit       P1+P2  `flare` stars, 60/s, 1.67 yd/s out of the chest in every
//                    direction, FALLING under 1.39 yd/s², 1.0 s lives, radius
//                    0.22 -> 0.08 -> 0.03 yd, (214,0,0) -> (250,149,18) ->
//                    (214,0,0), alpha 1 -> 0. No `blend`: `Emitter.draw`
//                    composites it `lighter` — additive.
//   hitsmoke P3      `cloud` puffs, 50/s, 0.5 yd/s, no gravity, 0.9 s lives,
//                    radius 0.03 -> 0.42 -> 0.14 yd, (249,234,64) ->
//                    (248,128,30) -> (255,25,25), alpha 0 -> 0.6 -> 0. No
//                    `blend`: additive.
//
// Where the card's prose and the bench disagree, the bench was signed off and
// wins:
//
// - **No grey smoke.** The card (and the client's P0) has a grey alpha-blended
//   smoke cloud; the bench's "Fire hit smoke" layer is described as "grey +
//   gold glow" but its emitter draws the gold-to-red glow only, so no grey
//   smoke is drawn here.
// - **Emission stops at 700 ms**, and the last flare is gone by 1.7 s (the
//   card: running from 67 ms to about 1.9 s, the client's 1900 ms sequence).
// - The flares are one 60/s layer with 1.0 s lives (the client: P1 30/s,
//   1.2 s, and P2 30/s, 0.8 s with a tail), and the glow flies at 0.5 yd/s
//   for 0.9 s (the client's P3: 0.28 yd/s, 0.8 s).
//
// ## Departures from the bench (each deliberate)
//
// 1. **Held in front of the body.** The bench is a 2D canvas: it draws the
//    body first and every particle over it. Here the body is an
//    `AlphaMode::Blend` mesh, which writes no depth, so the transparent pass
//    orders it against these additive quads by each mesh's translation: a
//    particle deeper than the body's centre would be drawn first and painted
//    over. Each particle is therefore drawn on its own view ray, slid toward
//    the camera until it stands nearer than the body's nearest point, and
//    scaled by the same ratio — the shared body hold (`body_hold.rs`) — so
//    it covers the screen footprint the bench gives it and is always drawn
//    after the body.
// 2. **The flare's white-hot core is tinted.** The bench adds a white core
//    over a star's coloured rays; here the star is one shape in the layer's
//    colour, as Siphon Life's and Shadow Word: Pain's stars are.
// 3. **Follows in height too.** The bench's `follow` moves particles with the
//    victim in x and z only; here they ride the rig at the chest of the
//    victim's RENDERED body (the walk and fear bobs, the swing step, the
//    Cyclone lift: [`place_aftermath_hits`]), so a victim on a ramp keeps its
//    burst.
// 4. **Hidden on a sheep.** A victim polymorphed under the burst hides it (its
//    clock runs on), as Siphon Life's chest glow is: the sheep has no chest at
//    this height.
// 5. **The ring is the shipped ring.** The bench sketches "our slow ring" as a
//    thin rune ring pulsing at the feet for the whole 5 s; what ships is the
//    shipped bind ring and scuff, only while the victim moves (`slow_ring.rs`).
//
// ## Lifecycle
//
// A landing on a living victim plays out its whole burst: the slow running
// out, being dispelled or being refreshed does not touch it (it is the hit,
// not the state), and neither does the victim dying under it — the hit
// already happened. A victim that is GONE retires the rig at once, its
// particles with it. The rig is a `PlayMatchEntity`, so a match that ends
// mid-burst takes all of it. The ring lives exactly as long as the slow (the
// slow ring's own lifecycle).
//
// ## At the match camera (60 yd, pitch 38.7°, FOV 45°)
//
// About 22 px a yard at 1080p: a newborn flare is ~10 px across, shrinking to
// ~1 px; a glow puff peaks at ~18 px. The burst spreads about 1.7 yd out and
// the flares sag about 0.7 yd by the end of their lives. Client size, by the
// ruling.
//
// House rules: spawn / animate / age / billboard systems, `Res<Time>`,
// registered in `states/mod.rs` only (headless never runs any of it), no
// `game_rng` — all scatter is hash-seeded off the rig's entity index and
// emission count. The sim's marker is the shared `AuraLanding`, spawned in
// both modes; it reads combat state, writes none and draws no `game_rng`.

// ── Bench sign-off constants (AS-334, version 1792203924-603a) ───────────────

// Aftermath daze (18118): one-shot fire hit (kit 728) + shipped slow ring. AS-334 bench.
pub const AFTERMATH_FIRE_HIT: bool = true;
// Ruled: the bench's AFTERMATH_HEAT_HAZE, AFTERMATH_EMBERS and
// AFTERMATH_BODY_TINT = false (no client fire-slow state, kit 985).
// The bench's AFTERMATH_SLOW_RING_FIRE = true is no knob here: it is `SlowTint::Fire`'s arm in
// `slow_ring.rs`, which every routed slow must have (`tests/lands_silently_audit.rs`).
pub const AFTERMATH_SIZE_MUL: f32 = 1.00;

/// The bench's chest anchor, above the floor (`ANCH.chest`; attachment 34).
pub const AFTERMATH_CHEST_ABOVE_FLOOR: f32 = 1.45;

// ── Rendering constants ──────────────────────────────────────────────────────

/// Steps in a particle's colour/alpha palette. Entry `i` is the tracks at
/// `i / (AFTERMATH_PALETTE_STEPS - 1)` of a life, so the keys at 0, 0.5 and 1
/// are drawn exactly.
pub const AFTERMATH_PALETTE_STEPS: usize = 17;
/// Generated sprite textures, pixels on a side.
const SPRITE_PX: u32 = 64;

// ── The emitter model (the bench's `Emitter`) ────────────────────────────────

/// What a particle's sprite is drawn with — the bench's sprite `kind`s.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AftermathSprite {
    /// `star`: a four-point star, rays a radius long and 0.12 of it thick,
    /// over a core fading out by 0.45 of the radius (`flare`).
    Star,
    /// `cloud`: a soft puff, 0.9 at its centre and 0.6 at 0.45 of its radius
    /// (`alphacloud`).
    Cloud,
}

impl AftermathSprite {
    /// The sprite's alpha at radius `r` (0 at the centre, 1 at the rim) and
    /// bearing `angle`: the bench's radial gradient stops, or the star's
    /// shape.
    pub fn alpha_at(self, r: f32, angle: f32) -> f32 {
        match self {
            AftermathSprite::Cloud => keyed(&[(0.0, 0.9), (0.45, 0.6), (1.0, 0.0)], r),
            AftermathSprite::Star => {
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

/// One emitter, as the bench's layer record carries it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AftermathEmitter {
    /// Unique key, model and emitter.
    pub name: &'static str,
    pub sprite: AftermathSprite,
    /// M2 blend 4 (additive) when `true`; blend 2 (alpha) when `false`.
    pub additive: bool,
    /// Launch speed out of the chest, yd/s, in a direction on the sphere (the
    /// bench's `vRange >= 3.1`: a normalised cube sample).
    pub speed: f32,
    /// Downward acceleration, yd/s² (the bench's `gravity`; negative rises).
    pub gravity: f32,
    pub life: f32,
    /// Particles per second while the window is open.
    pub rate: f32,
    /// The bench's `win`: `(ms from the landing, emitting)` keys.
    pub window: &'static [(f32, bool)],
    /// Colour, alpha and scale tracks over the particle's normalised life.
    pub color: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// Sprite RADIUS, yards.
    pub scale: &'static [(f32, f32)],
}

/// Both layers' window: on from 67 ms to 700 ms.
const HIT_WINDOW: &[(f32, bool)] = &[(0.0, false), (67.0, true), (700.0, false)];

/// `fire_impactdd_chest.m2`'s emitters as the bench's two layers.
pub const AFTERMATH_EMITTERS: [AftermathEmitter; 2] = [
    // P1 + P2 flare: red-orange flares bursting out and falling.
    AftermathEmitter {
        name: "fire_impactdd_chest/P1+P2 falling flares",
        sprite: AftermathSprite::Star,
        additive: true,
        speed: 1.67,
        gravity: 1.39,
        life: 1.0,
        rate: 60.0,
        window: HIT_WINDOW,
        color: &[
            (0.0, [214, 0, 0]),
            (0.5, [250, 149, 18]),
            (1.0, [214, 0, 0]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.22), (0.5, 0.08), (1.0, 0.03)],
    },
    // P3 alphacloud: the gold-to-red glow.
    AftermathEmitter {
        name: "fire_impactdd_chest/P3 gold glow",
        sprite: AftermathSprite::Cloud,
        additive: true,
        speed: 0.5,
        gravity: 0.0,
        life: 0.9,
        rate: 50.0,
        window: HIT_WINDOW,
        color: &[
            (0.0, [249, 234, 64]),
            (0.5, [248, 128, 30]),
            (1.0, [255, 25, 25]),
        ],
        alpha: &[(0.0, 0.0), (0.5, 0.6), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.5, 0.42), (1.0, 0.14)],
    },
];

/// Which of [`AFTERMATH_EMITTERS`] are kept layers, in table order (the
/// bench's one `AFTERMATH_FIRE_HIT` switch covers both).
pub const AFTERMATH_LAYERS: [bool; 2] = [AFTERMATH_FIRE_HIT, AFTERMATH_FIRE_HIT];

/// When the last emission window closes, seconds from the landing.
fn emission_end(e: &AftermathEmitter) -> f32 {
    e.window
        .iter()
        .filter(|(_, on)| !on)
        .map(|(ms, _)| ms / 1000.0)
        .fold(0.0, f32::max)
}

/// How long a hit's rig lives: until its last particle can have died — the
/// latest window close plus that layer's life (1.7 s).
pub fn aftermath_hit_secs() -> f32 {
    AFTERMATH_EMITTERS
        .iter()
        .map(|e| emission_end(e) + e.life)
        .fold(0.0, f32::max)
}

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// The chest the rig stands at, for a victim at `translation`.
pub fn aftermath_chest(translation: Vec3, is_pet: bool) -> Vec3 {
    body_point(AFTERMATH_CHEST_ABOVE_FLOOR, translation, is_pet)
}

/// A particle's launch velocity: the bench's normalised cube sample times its
/// speed, on a body of `stature`.
pub fn aftermath_launch(e: &AftermathEmitter, stature: f32, seed: u32) -> Vec3 {
    let d = Vec3::new(
        draw(seed, 1) * 2.0 - 1.0,
        draw(seed, 2) * 2.0 - 1.0,
        draw(seed, 3) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y);
    d * e.speed * stature
}

/// A particle's sprite radius at life fraction `t`, yards.
pub fn aftermath_radius(e: &AftermathEmitter, t: f32, stature: f32) -> f32 {
    keyed(e.scale, t) * AFTERMATH_SIZE_MUL * stature
}

/// The palette entry a particle `t` through its life draws.
pub fn aftermath_palette_step(t: f32) -> usize {
    let last = (AFTERMATH_PALETTE_STEPS - 1) as f32;
    ((t.clamp(0.0, 1.0) * last).round() as usize).min(AFTERMATH_PALETTE_STEPS - 1)
}

// ── Components ───────────────────────────────────────────────────────────────

/// One fire hit: a top-level entity parked at its victim's chest, parent of
/// every particle it emits (so the burst moves with a running victim).
#[derive(Component)]
pub struct AftermathHitRig {
    pub target: Entity,
    pub age: f32,
    /// Fractional particles owed, per emitter.
    pub carry: [f32; 2],
    pub emitted: u32,
    /// Whether the victim is a pet (its stature).
    pub is_pet: bool,
}

/// One particle: a child of its rig, at the point it is DRAWN.
#[derive(Component)]
pub struct AftermathHitParticle {
    /// Index into [`AFTERMATH_EMITTERS`].
    pub emitter: usize,
    /// Where the particle truly is, relative to the chest.
    pub offset: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub stature: f32,
    /// The sprite's roll in the camera plane (the bench's `ang = rnd()*TAU`).
    pub angle: f32,
    pub palette: Arc<[Handle<StandardMaterial>]>,
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// The quad and the two layers' palettes, built once, lazily.
pub struct AftermathAssets {
    quad: Handle<Mesh>,
    palettes: [Arc<[Handle<StandardMaterial>]>; 2],
}

impl AftermathAssets {
    fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        let palettes = AFTERMATH_EMITTERS.map(|e| {
            let texture = images.add(disc_texture(SPRITE_PX, |r, a| e.sprite.alpha_at(r, a)));
            (0..AFTERMATH_PALETTE_STEPS)
                .map(|i| {
                    let t = i as f32 / (AFTERMATH_PALETTE_STEPS - 1) as f32;
                    let alpha = keyed(e.alpha, t).clamp(0.0, 1.0);
                    materials.add(particle_material(
                        e.additive,
                        keyed_color(e.color, t),
                        alpha,
                        &texture,
                    ))
                })
                .collect::<Arc<[_]>>()
        });
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            palettes,
        }
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): a fire hit for every Aftermath landing the sim
/// marked on a living victim, and its wound flinch. Only Aftermath's markers
/// are consumed.
#[allow(clippy::type_complexity)]
pub fn spawn_aftermath_hits(
    mut commands: Commands,
    landings: Query<(Entity, &AuraLanding)>,
    victims: Query<(&Combatant, &Transform, Option<&Pet>, Option<&HitFlinch>)>,
) {
    let mut pending = PendingDepth::new();
    for (marker, landing) in landings.iter() {
        if landing.kind != AuraLandingKind::AftermathFireHit {
            continue;
        }
        commands.entity(marker).despawn();
        let Ok((victim, transform, pet, live_flinch)) = victims.get(landing.target) else {
            continue;
        };
        if !victim.is_alive() {
            continue;
        }
        let is_pet = pet.is_some();
        insert_flinch(
            &mut commands,
            &mut pending,
            landing.target,
            is_pet,
            live_flinch,
            false,
        );
        commands.spawn((
            AftermathHitRig {
                target: landing.target,
                age: 0.0,
                carry: [0.0; 2],
                emitted: 0,
                is_pet,
            },
            Transform::from_translation(aftermath_chest(transform.translation, is_pet)),
            Visibility::default(),
            PlayMatchEntity,
        ));
    }
}

/// Hide the burst while its victim is a sheep, emit each kept layer while
/// its window is open, integrating the rate over the frame so a frame-rate
/// change does not change the count, and retire the rig once its last
/// particle has died or its victim is gone.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_aftermath_hits(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<AftermathAssets>>,
    mut rigs: Query<(Entity, &mut AftermathHitRig, &mut Visibility)>,
    victims: Query<(Has<Pet>, Has<PolymorphedVisual>), With<Combatant>>,
) {
    let dt = time.delta_secs();
    let assets = assets
        .get_or_insert_with(|| AftermathAssets::build(&mut meshes, &mut materials, &mut images));
    for (rig_entity, mut rig, mut visibility) in rigs.iter_mut() {
        let Ok((is_pet, sheep)) = victims.get(rig.target) else {
            commands.entity(rig_entity).despawn();
            continue;
        };
        rig.age += dt;
        if rig.age >= aftermath_hit_secs() {
            commands.entity(rig_entity).despawn();
            continue;
        }
        rig.is_pet = is_pet;
        visibility.set_if_neq(if sheep {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
        let s = stature(is_pet);
        let ms = rig.age * 1000.0;

        for (ei, e) in AFTERMATH_EMITTERS.iter().enumerate() {
            if !AFTERMATH_LAYERS[ei] || !window_on(Some(e.window), ms) {
                continue;
            }
            rig.carry[ei] += e.rate * dt;
            while rig.carry[ei] >= 1.0 {
                rig.carry[ei] -= 1.0;
                let seed = rig_entity
                    .index()
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let particle = commands
                    .spawn((
                        AftermathHitParticle {
                            emitter: ei,
                            offset: Vec3::ZERO,
                            velocity: aftermath_launch(e, s, seed),
                            age: 0.0,
                            life: e.life,
                            stature: s,
                            angle: draw(seed, 5) * TAU,
                            palette: assets.palettes[ei].clone(),
                        },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(assets.palettes[ei][0].clone()),
                        Transform::from_scale(Vec3::splat(1e-4)),
                        Visibility::default(),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(rig_entity).add_child(particle);
            }
        }
    }
}

/// Age every particle, pull it down under its layer's gravity and move it
/// (the bench's order: velocity first, then position), and retire it at the
/// end of its life.
pub fn age_aftermath_hit_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut AftermathHitParticle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let gravity = AFTERMATH_EMITTERS[p.emitter].gravity * p.stature;
        p.velocity.y -= gravity * dt;
        let step = p.velocity * dt;
        p.offset += step;
    }
}

/// `BodyPlacement` (PostUpdate): stand each rig at its victim's rendered
/// chest, and place every particle where it is drawn — held in front of the
/// rendered body for the camera (departure 1) — turned to the camera, sized
/// and coloured along its life.
#[allow(clippy::type_complexity)]
pub fn place_aftermath_hits(
    hold: BodyHold,
    mut rigs: Query<
        (&AftermathHitRig, &mut Transform, &Children),
        (Without<AftermathHitParticle>, HoldDisjoint),
    >,
    mut particles: Query<
        (
            &AftermathHitParticle,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (Without<AftermathHitRig>, HoldDisjoint),
    >,
) {
    let cam = hold.view();
    let rotation = cam.map(|c| c.rotation).unwrap_or(Quat::IDENTITY);
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.target) else {
            continue;
        };
        let chest = aftermath_chest(body.root, body.is_pet);
        rig_transform.translation = chest;
        let capsule = body.capsule();
        for child in children.iter() {
            let Ok((p, mut transform, mut material)) = particles.get_mut(child) else {
                continue;
            };
            let e = &AFTERMATH_EMITTERS[p.emitter];
            let at = chest + p.offset;
            let (drawn, k) = held_in(cam, at, capsule);
            let t = (p.age / p.life).clamp(0.0, 1.0);
            transform.translation = drawn - chest;
            transform.rotation = rotation * Quat::from_rotation_z(p.angle);
            transform.scale = Vec3::splat((aftermath_radius(e, t, p.stature) * 2.0 * k).max(1e-4));
            material.0 = p.palette[aftermath_palette_step(t)].clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_lives_until_its_last_particle_can_have_died() {
        assert!((aftermath_hit_secs() - 1.7).abs() < 1e-6);
    }

    #[test]
    fn the_palette_draws_each_track_key_exactly() {
        assert_eq!(aftermath_palette_step(0.0), 0);
        assert_eq!(
            aftermath_palette_step(0.5),
            (AFTERMATH_PALETTE_STEPS - 1) / 2
        );
        assert_eq!(aftermath_palette_step(1.0), AFTERMATH_PALETTE_STEPS - 1);
    }
}
